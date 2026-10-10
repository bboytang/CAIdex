use crate::{
    Limits, OpenRouterConfig, request,
    tools::{ToolEvents, ToolPolicy},
};
use caidex_credentials::CredentialRef;
use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, CapabilitySupport, ModelCapabilities, ProviderError,
    ProviderResult, ResponsesDialect, ResponsesStream, StreamState,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt,
};

const PREFIX: &str = "caidex.openrouter.native-history.v1:";
pub(crate) fn invalid() -> ProviderError {
    ProviderError::new(400, "openrouter_invalid_history")
}
pub(crate) fn native_error(error: ProviderError) -> ProviderError {
    if error.http_status == 400 {
        ProviderError::new(502, "openrouter_invalid_native_history")
    } else {
        error
    }
}
pub(crate) fn too_large() -> ProviderError {
    ProviderError::new(502, "openrouter_history_too_large")
}
fn bounded(wire: &Value, limit: usize) -> ProviderResult<()> {
    if limit == 0 || wire.to_string().len().saturating_add(PREFIX.len()) > limit {
        return Err(too_large());
    }
    Ok(())
}
fn valid_id(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
}
fn flag(policy: &Value, key: &str) -> ProviderResult<bool> {
    policy[key].as_bool().ok_or_else(invalid)
}
fn map(policy: &Value, key: &str) -> ProviderResult<Option<HashMap<String, String>>> {
    if policy[key].is_null() {
        Ok(None)
    } else {
        serde_json::from_value(policy[key].clone())
            .map(Some)
            .map_err(|_| invalid())
    }
}
fn controls(wire: &Value) -> Value {
    let mut result = wire.clone();
    for key in ["input", "stream"] {
        result.as_object_mut().unwrap().remove(key);
    }
    result
}
fn input(wire: &Value) -> ProviderResult<Vec<Value>> {
    if let Some(text) = wire["input"].as_str() {
        Ok(vec![json!({"role":"user","content":text})])
    } else {
        wire["input"].as_array().cloned().ok_or_else(invalid)
    }
}

/// Full sensitive native wire, not encryption or source authentication.
#[derive(Clone)]
pub struct NativeHistory(Value);
pub(crate) struct HistoryContext {
    pub(crate) scope: Value,
    pub(crate) policy: Value,
    pub(crate) request: CanonicalRequest,
}

/// Compile only executor-owned controls; input is restored and checked separately.
fn compile(policy: &Value, limit: usize) -> ProviderResult<(CanonicalRequest, ToolPolicy)> {
    let runtime = flag(policy, "runtime_context")?;
    let native = flag(policy, "native_tools")?;
    let advanced = flag(policy, "advanced_tools")?;
    let capabilities: ModelCapabilities =
        serde_json::from_value(policy["capabilities"].clone()).map_err(|_| invalid())?;
    let backend = policy["backend"].as_str().ok_or_else(invalid)?;
    if advanced && !native
        || !backend.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
        || !valid_id(&policy["route"])
        || !valid_id(&policy["native_model"])
    {
        return Err(invalid());
    }
    let mut source = policy["controls"].clone();
    if !source.is_object()
        || ["model", "input", "stream"]
            .iter()
            .any(|k| source.get(k).is_some())
    {
        return Err(invalid());
    }
    source["model"] = policy["route"].clone();
    source["input"] = json!([]);
    let source = CanonicalRequest::new(source, ResponsesDialect::Classic).map_err(|_| invalid())?;
    let tools = ToolPolicy::new(&source, capabilities.parallel_tools, advanced)?;
    if native && tools.has_tools() && capabilities.native_tools == CapabilitySupport::Unsupported {
        return Err(ProviderError::new(400, "unsupported_tools"));
    }
    let mut wire = source.wire().clone();
    if native {
        for k in ["tools", "tool_choice", "parallel_tool_calls"] {
            wire.as_object_mut().unwrap().remove(k);
        }
    }
    let choices = |key, valid: fn(&str) -> bool| -> ProviderResult<Option<BTreeSet<String>>> {
        let Some(value) = policy.get(key) else {
            return Ok(None);
        };
        let choices: BTreeSet<String> =
            serde_json::from_value(value.clone()).map_err(|_| invalid())?;
        if !runtime
            || choices.is_empty()
            || choices.iter().any(|v| !valid(v))
            || json!(choices) != *value
        {
            return Err(invalid());
        }
        Ok(Some(choices))
    };
    let summaries = choices("summaries", request::valid_summary)?;
    let contexts = choices("contexts", request::valid_context)?;
    let history_controls = request::history_controls(
        &mut wire,
        runtime,
        summaries.as_ref(),
        contexts.as_ref(),
        capabilities.reasoning,
    )?;
    let efforts = map(policy, "efforts")?;
    let verbosity = map(policy, "verbosity")?;
    let tiers = map(policy, "tiers")?;
    let request = request::compile(
        CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())?,
        limit,
        runtime,
        efforts.as_ref(),
        capabilities.reasoning,
        verbosity.as_ref(),
        tiers.as_ref(),
    )?;
    let mut wire = request.wire().clone();
    if let Some(include) = history_controls.get("include") {
        wire["include"] = include.clone();
    }
    if let Some(reasoning) = history_controls["reasoning"].as_object() {
        for (key, value) in reasoning {
            wire["reasoning"][key] = value.clone();
        }
    }
    if native {
        for k in ["tools", "tool_choice", "parallel_tool_calls"] {
            if let Some(v) = source.wire().get(k) {
                wire[k] = v.clone();
            }
        }
        tools.compile_selection(&mut wire);
    }
    wire["model"] = policy["native_model"].clone();
    wire["provider"]["only"] = json!([backend]);
    Ok((
        CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())?,
        tools,
    ))
}

pub(crate) fn prepare(
    source: CanonicalRequest,
    scope: Value,
    policy: Value,
    limits: &Limits,
) -> ProviderResult<(CanonicalRequest, Option<ToolPolicy>, HistoryContext)> {
    if source.dialect() != ResponsesDialect::Classic {
        return Err(ProviderError::new(400, "unsupported_dialect"));
    }
    let limit = limits.request_bytes.min(limits.response_bytes);
    let (compiled, mut tools) = compile(&policy, limits.request_bytes)?;
    let source_input = input(source.wire())?;
    let mut native = Vec::new();
    let mut trusted = HashSet::new();
    let mut index = 0;
    while index < source_input.len() {
        let item = &source_input[index];
        if item["type"] == "reasoning" {
            let history = NativeHistory::decode(item, limit)?;
            if history.0["scope"] != scope || history.0["policy"] != policy {
                return Err(ProviderError::new(
                    400,
                    "openrouter_history_policy_mismatch",
                ));
            }
            if controls(history.request()) != controls(compiled.wire())
                || input(history.request())? != native
            {
                return Err(ProviderError::new(
                    400,
                    "openrouter_history_prefix_mismatch",
                ));
            }
            let projected = history.to_responses(limit).map_err(|_| invalid())?;
            let count = projected.output().len();
            let group = source_input.get(index..index + count).ok_or_else(invalid)?;
            for (actual, expected) in group.iter().zip(projected.output()) {
                if actual.get("id").is_some_and(|v| !valid_id(v))
                    || actual.get("status").is_some_and(|v| v != "completed")
                    || normalized(actual) != normalized(expected)
                {
                    return Err(invalid());
                }
            }
            for item in history.native_response()["output"].as_array().unwrap() {
                trusted.insert(native.len());
                native.push(item.clone());
            }
            index += count;
        } else {
            if !crate::tools::input_item(item) {
                request::validate_message(item, flag(&policy, "runtime_context")?)?;
            }
            native.push(item.clone());
            index += 1;
        }
    }
    let mut wire = compiled.wire().clone();
    wire["input"] = json!(native);
    wire["stream"] = source.is_streaming().into();
    tools.validate_input(&wire, &trusted)?;
    if wire.to_string().len() > limits.request_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    let request = CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())?;
    let context = HistoryContext {
        scope,
        policy: policy.clone(),
        request: request.clone(),
    };
    let mut wire = request.wire().clone();
    wire["model"] = policy["route"].clone();
    Ok((
        CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())?,
        if flag(&policy, "native_tools")? {
            Some(tools)
        } else {
            None
        },
        context,
    ))
}
fn normalized(item: &Value) -> Value {
    let mut item = item.clone();
    let Some(object) = item.as_object_mut() else {
        return item;
    };
    object.remove("id");
    object.remove("status");
    if object.get("type").is_some_and(|v| v == "reasoning")
        && object.get("content").is_some_and(Value::is_null)
    {
        object.remove("content");
    }
    item
}

impl NativeHistory {
    pub(crate) fn record(
        context: &HistoryContext,
        response: &CanonicalResponse,
        chunks: Option<&[Value]>,
        limit: usize,
    ) -> ProviderResult<Self> {
        let mut wire = json!({"provider":"openrouter","version":1,"scope":context.scope,"policy":context.policy,"request":context.request.wire(),"response":response.wire(),"source":if chunks.is_some(){"sse"}else{"json"}});
        if let Some(chunks) = chunks {
            wire["chunks"] = json!(chunks);
        }
        Self::new(wire, limit).map_err(native_error)
    }
    fn new(wire: Value, limit: usize) -> ProviderResult<Self> {
        bounded(&wire, limit)?;
        if wire["provider"] != "openrouter" || wire["version"] != 1 {
            return Err(invalid());
        }
        let reference: CredentialRef =
            serde_json::from_value(wire["scope"]["credential"].clone()).map_err(|_| invalid())?;
        let config = OpenRouterConfig::new(reference)
            .map_err(|_| invalid())?
            .with_base_url(wire["scope"]["base"].as_str().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        if config.replay_scope() != wire["scope"] {
            return Err(invalid());
        }
        let (compiled, mut tools) = compile(&wire["policy"], limit)?;
        let request = CanonicalRequest::new(wire["request"].clone(), ResponsesDialect::Classic)
            .map_err(|_| invalid())?;
        if controls(request.wire()) != controls(compiled.wire()) {
            return Err(invalid());
        }
        let items = input(request.wire())?;
        for item in &items {
            let native_tools = flag(&wire["policy"], "native_tools")?;
            if !(native_tools && crate::tools::input_item(item)) {
                request::output(item, native_tools)?;
            }
        }
        tools.validate_input(request.wire(), &(0..items.len()).collect())?;
        let response = CanonicalResponse::new(wire["response"].clone()).map_err(|_| invalid())?;
        request::output(response.wire(), flag(&wire["policy"], "native_tools")?)?;
        tools.response(response.wire())?;
        if response.state() != StreamState::Completed
            || !valid_id(&response.wire()["id"])
            || response
                .wire()
                .get("model")
                .is_some_and(|m| m != &wire["policy"]["native_model"])
        {
            return Err(invalid());
        }
        validate_output(&response)?;
        match wire["source"].as_str() {
            Some("json") if wire.get("chunks").is_none() => {}
            Some("sse") => validate_chunks(
                &response,
                &tools,
                flag(&wire["policy"], "native_tools")?,
                wire["chunks"].as_array().ok_or_else(invalid)?,
                limit,
            )?,
            _ => return Err(invalid()),
        }
        Ok(Self(wire))
    }
    fn decode(carrier: &Value, limit: usize) -> ProviderResult<Self> {
        let text = carrier["encrypted_content"].as_str().ok_or_else(invalid)?;
        if carrier["type"] != "reasoning" || text.len() > limit {
            return Err(invalid());
        }
        Self::new(
            serde_json::from_str(text.strip_prefix(PREFIX).ok_or_else(invalid)?)
                .map_err(|_| invalid())?,
            limit,
        )
        .map_err(|_| invalid())
    }
    pub fn from_responses(response: &CanonicalResponse, limit: usize) -> ProviderResult<Self> {
        let history = Self::decode(response.output().first().ok_or_else(invalid)?, limit)?;
        let projected = history.to_responses(limit)?;
        if response.wire() != projected.wire() {
            return Err(invalid());
        }
        Ok(history)
    }
    pub fn wire(&self) -> &Value {
        &self.0
    }
    pub fn request(&self) -> &Value {
        &self.0["request"]
    }
    pub fn native_response(&self) -> &Value {
        &self.0["response"]
    }
    pub fn to_responses(&self, limit: usize) -> ProviderResult<CanonicalResponse> {
        bounded(&self.0, limit)?;
        let response =
            CanonicalResponse::new(self.native_response().clone()).map_err(|_| invalid())?;
        let summary: Vec<Value> = response
            .output()
            .iter()
            .filter(|i| i["type"] == "reasoning")
            .flat_map(|i| i["summary"].as_array().unwrap())
            .map(|p| json!({"type":"summary_text","text":p["text"]}))
            .collect();
        // shortcut: complete prefixes grow quadratically across carriers; budgets bound them until Host persistence deduplicates history.
        let mut output = vec![
            json!({"type":"reasoning","id":format!("rs_{}_openrouter_history",response.id()),"summary":summary,"encrypted_content":format!("{PREFIX}{}",self.0)}),
        ];
        for item in response.output() {
            let mut projected = json!({});
            let fields: &[&str] = match item["type"].as_str() {
                Some("message") => &["type", "id", "status", "role"],
                Some("function_call") => &[
                    "type",
                    "id",
                    "status",
                    "namespace",
                    "name",
                    "call_id",
                    "arguments",
                ],
                Some("custom_tool_call") => &[
                    "type",
                    "id",
                    "status",
                    "namespace",
                    "name",
                    "call_id",
                    "input",
                ],
                _ => continue,
            };
            for key in fields {
                if let Some(v) = item.get(*key) {
                    projected[*key] = v.clone();
                }
            }
            if item["type"] == "message" {
                projected["content"] = json!(
                    item["content"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| json!({"type":"output_text","text":p["text"]}))
                        .collect::<Vec<_>>()
                );
            }
            if projected["id"] == output[0]["id"] {
                return Err(invalid());
            }
            output.push(projected);
        }
        let mut wire = response.wire().clone();
        wire["output"] = json!(output);
        bounded(&wire, limit)?;
        CanonicalResponse::new(wire).map_err(|_| invalid())
    }
}
impl fmt::Debug for NativeHistory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeHistory([WIRE OMITTED])")
    }
}
fn validate_output(response: &CanonicalResponse) -> ProviderResult<()> {
    let mut ids = HashSet::new();
    for item in response.output() {
        if let Some(id) = item.get("id").filter(|v| v.is_string())
            && (!valid_id(id) || !ids.insert(id.clone()))
        {
            return Err(invalid());
        }
        if matches!(
            item["type"].as_str(),
            Some("reasoning" | "message" | "function_call" | "custom_tool_call")
        ) && (!valid_id(&item["id"]) || item.get("status").is_some_and(|v| v != "completed"))
        {
            return Err(invalid());
        }
        if item["type"] == "reasoning" {
            for key in ["encrypted_content", "signature", "format"] {
                if item
                    .get(key)
                    .is_some_and(|v| !v.is_null() && !v.is_string())
                {
                    return Err(invalid());
                }
            }
            for part in item["summary"].as_array().ok_or_else(invalid)? {
                if part["type"] != "summary_text" || !part["text"].is_string() {
                    return Err(invalid());
                }
            }
            if let Some(content) = item.get("content").filter(|v| !v.is_null()) {
                for part in content.as_array().ok_or_else(invalid)? {
                    if part["type"] != "reasoning_text" || !part["text"].is_string() {
                        return Err(invalid());
                    }
                }
            }
        } else if item["type"] == "message" {
            if item["role"] != "assistant" {
                return Err(invalid());
            }
            for part in item["content"].as_array().ok_or_else(invalid)? {
                if part["type"] != "output_text" || !part["text"].is_string() {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_chunks(
    response: &CanonicalResponse,
    tools: &ToolPolicy,
    native_tools: bool,
    chunks: &[Value],
    limit: usize,
) -> ProviderResult<()> {
    let mut parser = ResponsesStream::new(limit).map_err(|_| invalid())?;
    let mut tool_events = ToolEvents::default();
    let mut terminal = None;
    let mut created = false;
    let mut added = HashMap::new();
    let mut done = HashSet::new();
    let mut text: HashMap<(usize, &str, usize), (String, bool)> = HashMap::new();
    let mut text_done = HashSet::new();
    let mut part_added = HashSet::new();
    let mut part_done = HashSet::new();
    for chunk in chunks {
        request::output(chunk, native_tools)?;
        tool_events.observe(chunk)?;
        for event in parser
            .push(format!("data: {chunk}\n\n").as_bytes())
            .map_err(|_| invalid())?
        {
            if event.response.terminal().is_some() {
                terminal = event.response.wire().get("response").cloned();
            }
        }
        let kind = chunk["type"].as_str().ok_or_else(invalid)?;
        if kind == "response.created" {
            if created
                || chunk["response"]["id"] != response.id()
                || chunk["response"]["output"]
                    .as_array()
                    .is_none_or(|v| !v.is_empty())
            {
                return Err(invalid());
            }
            created = true;
        }
        if matches!(
            kind,
            "response.output_item.added" | "response.output_item.done"
        ) {
            let index = chunk["output_index"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(invalid)?;
            let final_item = response.output().get(index).ok_or_else(invalid)?;
            if kind.ends_with(".added") {
                if !created || added.insert(index, chunk["item"].clone()).is_some() {
                    return Err(invalid());
                }
                for field in ["type", "id", "role", "name", "call_id", "namespace"] {
                    if chunk["item"]
                        .get(field)
                        .is_some_and(|v| final_item.get(field) != Some(v))
                    {
                        return Err(invalid());
                    }
                }
            } else if !added.contains_key(&index)
                || !done.insert(index)
                || chunk["item"] != *final_item
            {
                return Err(invalid());
            }
        }
        let section = if kind.starts_with("response.reasoning_summary_text.") {
            Some(("summary", "summary_index", "reasoning"))
        } else if kind.starts_with("response.reasoning_text.") {
            Some(("content", "content_index", "reasoning"))
        } else if kind.starts_with("response.output_text.")
            || kind.starts_with("response.content_part.")
        {
            Some(("content", "content_index", "message"))
        } else {
            None
        };
        if let Some((section, index_key, item_kind)) = section {
            if !matches!(kind.rsplit('.').next(), Some("delta" | "done" | "added")) {
                return Err(invalid());
            }
            let index = chunk["output_index"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(invalid)?;
            let part = chunk[index_key]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(invalid)?;
            let item = response.output().get(index).ok_or_else(invalid)?;
            let expected = item[section]
                .as_array()
                .and_then(|v| v.get(part))
                .ok_or_else(invalid)?;
            let expected_text = expected["text"].as_str().ok_or_else(invalid)?;
            let key = (index, section, part);
            if !created
                || !added.contains_key(&index)
                || done.contains(&index)
                || item["type"] != item_kind
                || chunk["item_id"] != item["id"]
                || part_done.contains(&key)
            {
                return Err(invalid());
            }
            let state = text.entry(key).or_insert_with(|| {
                (
                    added[&index][section]
                        .as_array()
                        .and_then(|v| v.get(part))
                        .and_then(|v| v["text"].as_str())
                        .unwrap_or("")
                        .to_owned(),
                    false,
                )
            });
            if kind.ends_with(".delta") {
                if text_done.contains(&key) {
                    return Err(invalid());
                }
                state
                    .0
                    .push_str(chunk["delta"].as_str().ok_or_else(invalid)?);
                state.1 = true;
            } else if kind.ends_with("text.done") {
                if !text_done.insert(key)
                    || chunk["text"] != expected_text
                    || state.1 && state.0 != expected_text
                    || !state.1 && !expected_text.starts_with(&state.0)
                {
                    return Err(invalid());
                }
                state.0 = expected_text.into();
            } else if kind == "response.content_part.added" {
                if !part_added.insert(key)
                    || text_done.contains(&key)
                    || state.1
                    || chunk["part"]["type"] != expected["type"]
                {
                    return Err(invalid());
                }
                state.0 = chunk["part"]["text"].as_str().ok_or_else(invalid)?.into();
                if !expected_text.starts_with(&state.0) {
                    return Err(invalid());
                }
            } else if kind == "response.content_part.done" {
                if !part_done.insert(key)
                    || chunk["part"] != *expected
                    || state.1 && state.0 != expected_text
                    || !state.1 && !expected_text.starts_with(&state.0)
                {
                    return Err(invalid());
                }
                state.0 = expected_text.into();
            } else {
                return Err(invalid());
            }
        }
    }
    if !created
        || parser.finish().map_err(|_| invalid())? != response.state()
        || terminal.as_ref() != Some(response.wire())
        || added.keys().any(|i| !done.contains(i))
    {
        return Err(invalid());
    }
    tool_events.finish(tools)?;
    for ((index, section, part), (actual, observed_delta)) in text {
        let expected = &response.output()[index][section][part]["text"];
        if observed_delta && expected != &actual {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) fn validate_noncompleted(
    context: &HistoryContext,
    chunks: &[Value],
    limit: usize,
) -> ProviderResult<()> {
    let (_, mut tools) = compile(&context.policy, limit)?;
    let items = input(context.request.wire())?;
    tools.validate_input(context.request.wire(), &(0..items.len()).collect())?;
    let mut observed = ToolEvents::default();
    for chunk in chunks {
        request::output(chunk, flag(&context.policy, "native_tools")?)?;
        observed.observe(chunk)?;
    }
    observed.finish(&tools)
}
