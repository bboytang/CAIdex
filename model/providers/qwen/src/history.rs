use crate::{QwenConfig, tools::ToolMap};
use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, ProviderError, ProviderResult, ResponsesDialect,
    ResponsesStream, StreamState,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fmt,
};

const PREFIX: &str = "caidex.qwen.native-history.v1:";
const TOOLS_PREFIX: &str = "caidex.qwen.native-history.v2:";
const CUSTOM_PREFIX: &str = "caidex.qwen.native-history.v3:";
pub(crate) fn invalid() -> ProviderError {
    ProviderError::new(400, "qwen_invalid_history")
}
pub(crate) fn native_error(error: ProviderError) -> ProviderError {
    if error.http_status == 400 {
        ProviderError::new(502, "qwen_invalid_native_history")
    } else {
        error
    }
}
fn size(wire: &Value, limit: usize) -> ProviderResult<()> {
    if wire.to_string().len().saturating_add(PREFIX.len()) > limit {
        return Err(ProviderError::new(502, "qwen_history_too_large"));
    }
    Ok(())
}
pub(crate) fn valid_id(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
}
pub(crate) fn summary(item: &Value) -> ProviderResult<String> {
    let mut text = String::new();
    for part in item["summary"].as_array().ok_or_else(invalid)? {
        if part["type"] != "summary_text" {
            return Err(invalid());
        }
        text.push_str(part["text"].as_str().ok_or_else(invalid)?);
    }
    Ok(text)
}

/// Full sensitive wire, not encryption or source authentication.
#[derive(Clone)]
pub struct NativeHistory(Value);
impl NativeHistory {
    pub(crate) fn record(
        scope: &Value,
        request: &CanonicalRequest,
        response: &CanonicalResponse,
        tools: Option<&ToolMap>,
        chunks: Option<&[Value]>,
        limit: usize,
    ) -> ProviderResult<Self> {
        let mut wire = json!({"provider":"qwen","version":1,"scope":scope,"native_model":request.model(),"request":request.wire(),"response":response.wire(),"source":if chunks.is_some(){"sse"}else{"json"}});
        if let Some(tools) = tools {
            wire["version"] = tools.history_version().into();
            wire["tool_mapping"] = tools.source().clone();
        }
        if let Some(chunks) = chunks {
            wire["chunks"] = json!(chunks);
        }
        Self::new(wire, limit)
    }
    fn new(wire: Value, limit: usize) -> ProviderResult<Self> {
        size(&wire, limit)?;
        if wire["provider"] != "qwen"
            || !matches!(wire["version"].as_u64(), Some(1..=3))
            || !wire["scope"].is_object()
            || wire["request"]["model"] != wire["native_model"]
        {
            return Err(invalid());
        }
        let tools = if wire["version"] != 1 {
            let tools = ToolMap::from_source(wire["tool_mapping"].clone())?;
            if wire["version"] != tools.history_version() {
                return Err(invalid());
            }
            Some(tools)
        } else {
            if wire.get("tool_mapping").is_some() {
                return Err(invalid());
            }
            None
        };
        let request = CanonicalRequest::new(wire["request"].clone(), ResponsesDialect::Classic)
            .map_err(|_| invalid())?;
        crate::output(&wire["request"], tools.is_some())?;
        let response = CanonicalResponse::new(wire["response"].clone()).map_err(|_| invalid())?;
        crate::output(response.wire(), tools.is_some())?;
        if response.state() != StreamState::Completed
            || !valid_id(&response.wire()["id"])
            || response
                .wire()
                .get("model")
                .is_some_and(|v| v != &wire["native_model"])
        {
            return Err(invalid());
        }
        if let Some(tools) = &tools {
            tools.matches(&request)?;
            tools.validate_response(&request, &response)?;
        }
        // A carrier cannot introduce a different native request contract.
        if request.wire().as_object().unwrap().keys().any(|k| {
            ![
                "model",
                "input",
                "stream",
                "instructions",
                "store",
                "max_output_tokens",
                "temperature",
                "top_p",
                "reasoning",
            ]
            .contains(&k.as_str())
                && !(tools.is_some() && matches!(k.as_str(), "tools" | "tool_choice"))
        }) || request.wire()["store"] != false
        {
            return Err(invalid());
        }
        let mut ids = HashSet::new();
        for item in response.output() {
            if let Some(id) = item.get("id").filter(|v| v.is_string())
                && (!valid_id(id) || !ids.insert(id.clone()))
            {
                return Err(invalid());
            }
            if matches!(
                item["type"].as_str(),
                Some("reasoning" | "message" | "function_call")
            ) && item.get("status").is_some_and(|v| v != "completed")
            {
                return Err(invalid());
            }
            if item["type"] == "reasoning" {
                if !valid_id(&item["id"])
                    || item.get("encrypted_content").is_some_and(|v| !v.is_null())
                    || item
                        .get("content")
                        .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
                {
                    return Err(invalid());
                }
                summary(item)?;
            } else if item["type"] == "message" {
                if !valid_id(&item["id"]) || item["role"] != "assistant" {
                    return Err(invalid());
                }
                for part in item["content"].as_array().ok_or_else(invalid)? {
                    if part["type"] != "output_text"
                        || !part["text"].is_string()
                        || part.get("annotations").is_some_and(|v| !v.is_array())
                    {
                        return Err(invalid());
                    }
                }
            }
        }
        match wire["source"].as_str() {
            Some("json") if wire.get("chunks").is_none() => (),
            Some("sse") => validate_chunks(
                &response,
                tools.is_some(),
                wire["chunks"].as_array().ok_or_else(invalid)?,
                limit,
            )?,
            _ => return Err(invalid()),
        }
        Ok(Self(wire))
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
        size(&self.0, limit)?;
        let response =
            CanonicalResponse::new(self.native_response().clone()).map_err(|_| invalid())?;
        let response = if self.0["version"] != 1 {
            ToolMap::from_source(self.0["tool_mapping"].clone())?.project(&response)?
        } else {
            response
        };
        let carrier_id = format!("rs_{}_native", response.id());
        if response
            .output()
            .iter()
            .any(|item| item["type"] != "reasoning" && item["id"] == carrier_id)
        {
            return Err(invalid());
        }
        let mut wire = response.wire().clone();
        // Each native reasoning item projects one flat summary; retain its exact
        // original part boundaries/extensions inside the bound capsule.
        let summaries = response
            .output()
            .iter()
            .filter(|i| i["type"] == "reasoning")
            .map(|i| summary(i).map(|text| json!({"type":"summary_text","text":text})))
            .collect::<ProviderResult<Vec<_>>>()?;
        // shortcut: nested prefixes grow quadratically; byte budgets refuse
        // overflow until Host persistence can deduplicate full native history.
        let prefix = if self.0["version"] == 3 {
            CUSTOM_PREFIX
        } else if self.0["version"] == 2 {
            TOOLS_PREFIX
        } else {
            PREFIX
        };
        let mut output = vec![
            json!({"type":"reasoning","id":carrier_id,"summary":summaries,"encrypted_content":format!("{prefix}{}",self.0)}),
        ];
        output.extend(
            response
                .output()
                .iter()
                .filter(|i| i["type"] != "reasoning")
                .cloned(),
        );
        wire["output"] = output.into();
        size(&wire, limit)?;
        CanonicalResponse::new(wire).map_err(|_| invalid())
    }
    fn restore(
        input: &[Value],
        config: &QwenConfig,
        expected: &CanonicalRequest,
        tools: Option<&ToolMap>,
        limit: usize,
    ) -> ProviderResult<(Self, usize)> {
        let capsule = input.first().ok_or_else(invalid)?["encrypted_content"]
            .as_str()
            .ok_or_else(invalid)?;
        if capsule.len() > limit {
            return Err(invalid());
        }
        let (encoded, version) = if let Some(wire) = capsule.strip_prefix(PREFIX) {
            (wire, 1)
        } else if let Some(wire) = capsule.strip_prefix(CUSTOM_PREFIX) {
            (wire, 3)
        } else {
            (capsule.strip_prefix(TOOLS_PREFIX).ok_or_else(invalid)?, 2)
        };
        let history = Self::new(serde_json::from_str(encoded).map_err(|_| invalid())?, limit)
            .map_err(|_| invalid())?;
        if history.0["version"] != version
            || history.0["version"] != tools.map_or(1, ToolMap::history_version)
            || tools.is_some_and(|t| history.0["tool_mapping"] != *t.source())
        {
            return Err(invalid());
        }
        if history.0["scope"] != config.replay_scope()
            || history.0["native_model"] != expected.model()
        {
            return Err(ProviderError::new(400, "qwen_history_model_mismatch"));
        }
        if prefix(history.request()) != prefix(expected.wire()) {
            return Err(ProviderError::new(400, "qwen_history_prefix_mismatch"));
        }
        let display = history.to_responses(limit).map_err(|_| invalid())?;
        let count = display.output().len();
        for (actual, expected) in input
            .get(..count)
            .ok_or_else(invalid)?
            .iter()
            .zip(display.output())
        {
            let known = matches!(
                actual["type"].as_str(),
                Some("reasoning" | "message" | "function_call" | "custom_tool_call")
            );
            if known
                && (actual.get("id").is_some_and(|v| !valid_id(v))
                    || actual.get("status").is_some_and(|v| v != "completed"))
                || without_identity(actual) != without_identity(expected)
            {
                return Err(invalid());
            }
        }
        Ok((history, count))
    }
}
impl fmt::Debug for NativeHistory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeHistory([WIRE OMITTED])")
    }
}
fn without_identity(item: &Value) -> Value {
    let mut item = item.clone();
    if matches!(
        item["type"].as_str(),
        Some("reasoning" | "message" | "function_call" | "custom_tool_call")
    ) && let Some(object) = item.as_object_mut()
    {
        object.remove("id");
        object.remove("status");
        if object.get("type").is_some_and(|v| v == "reasoning")
            && object.get("content").is_some_and(Value::is_null)
        {
            object.remove("content");
        }
    }
    item
}
fn prefix(wire: &Value) -> Value {
    let input = if let Some(text) = wire["input"].as_str() {
        json!([{"role":"user","content":text}])
    } else {
        wire["input"].clone()
    };
    json!({"model":wire["model"],"input":input,"instructions":wire["instructions"]})
}
pub(crate) fn expand(
    request: CanonicalRequest,
    config: &QwenConfig,
    tools: Option<&ToolMap>,
    limit: usize,
) -> ProviderResult<CanonicalRequest> {
    let Some(input) = request.wire()["input"].as_array() else {
        return Ok(request);
    };
    let mut native = Vec::new();
    let mut index = 0;
    while index < input.len() {
        if input[index]["type"] == "reasoning" {
            if let Some(tools) = tools {
                native = tools.pair_input(&native)?;
            }
            let mut wire = request.wire().clone();
            wire["input"] = json!(native);
            let expected =
                CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())?;
            let (history, count) =
                NativeHistory::restore(&input[index..], config, &expected, tools, limit)?;
            native.extend(
                history.native_response()["output"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
            index += count;
        } else if tools.is_some()
            && matches!(
                input[index]["type"].as_str(),
                Some(
                    "function_call"
                        | "function_call_output"
                        | "custom_tool_call"
                        | "custom_tool_call_output"
                )
            )
        {
            native.push(tools.unwrap().compile_item(&input[index], &native)?);
            index += 1;
        } else {
            crate::request::validate_message(&input[index])?;
            native.push(input[index].clone());
            index += 1;
        }
    }
    if let Some(tools) = tools {
        native = tools.pair_input(&native)?;
    }
    let mut wire = request.wire().clone();
    wire["input"] = native.into();
    if wire.to_string().len() > limit {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}

fn validate_chunks(
    response: &CanonicalResponse,
    native_tools: bool,
    chunks: &[Value],
    limit: usize,
) -> ProviderResult<()> {
    let mut parser = ResponsesStream::new(limit).map_err(|_| invalid())?;
    let mut terminal = None;
    let mut created = false;
    let mut added = HashSet::new();
    let mut done = HashSet::new();
    let mut deltas: HashMap<(usize, usize), String> = HashMap::new();
    let mut text_done = HashSet::new();
    let mut parts_added = HashSet::new();
    let mut parts_done = HashSet::new();
    for chunk in chunks {
        crate::output(chunk, native_tools)?;
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
                .and_then(|i| usize::try_from(i).ok())
                .ok_or_else(invalid)?;
            let final_item = response.output().get(index).ok_or_else(invalid)?;
            if kind == "response.output_item.added" {
                if !added.insert(index) {
                    return Err(invalid());
                }
                for field in ["type", "id", "role"] {
                    if chunk["item"]
                        .get(field)
                        .is_some_and(|v| final_item.get(field) != Some(v))
                    {
                        return Err(invalid());
                    }
                }
                if final_item["type"] == "function_call" {
                    let args = chunk["item"]["arguments"].as_str().ok_or_else(invalid)?;
                    if !args.is_empty() {
                        deltas.insert((index, usize::MAX), args.to_owned());
                    }
                    if chunk["item"]["call_id"] != final_item["call_id"]
                        || chunk["item"]["name"] != final_item["name"]
                    {
                        return Err(invalid());
                    }
                }
                if final_item["type"] == "reasoning" && !summary(&chunk["item"])?.is_empty() {
                    deltas.insert((index, 0), summary(&chunk["item"])?);
                }
            } else if !added.contains(&index) || !done.insert(index) || chunk["item"] != *final_item
            {
                return Err(invalid());
            }
        }
        if matches!(
            kind,
            "response.function_call_arguments.delta" | "response.function_call_arguments.done"
        ) {
            let index = chunk["output_index"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(invalid)?;
            let item = response.output().get(index).ok_or_else(invalid)?;
            let key = (index, usize::MAX);
            if !native_tools
                || !created
                || !added.contains(&index)
                || done.contains(&index)
                || item["type"] != "function_call"
                || chunk["item_id"] != item["id"]
                || chunk.get("content_index").is_some()
            {
                return Err(invalid());
            }
            if kind.ends_with(".delta") {
                if text_done.contains(&key) {
                    return Err(invalid());
                }
                deltas
                    .entry(key)
                    .or_default()
                    .push_str(chunk["delta"].as_str().ok_or_else(invalid)?);
            } else if !text_done.insert(key) || chunk["arguments"] != item["arguments"] {
                return Err(invalid());
            }
        } else if matches!(
            kind,
            "response.reasoning_text.delta"
                | "response.reasoning_text.done"
                | "response.output_text.delta"
                | "response.output_text.done"
                | "response.content_part.added"
                | "response.content_part.done"
        ) {
            let index = chunk["output_index"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .ok_or_else(invalid)?;
            let item = response.output().get(index).ok_or_else(invalid)?;
            let reasoning = kind.starts_with("response.reasoning_text.");
            let part = if reasoning {
                if chunk.get("content_index").is_some_and(|v| v != 0) {
                    return Err(invalid());
                }
                0
            } else {
                chunk["content_index"]
                    .as_u64()
                    .and_then(|i| usize::try_from(i).ok())
                    .ok_or_else(invalid)?
            };
            let key = (index, part);
            if !created
                || !added.contains(&index)
                || done.contains(&index)
                || chunk["item_id"] != item["id"]
                || item["type"] != if reasoning { "reasoning" } else { "message" }
            {
                return Err(invalid());
            }
            let content = if reasoning {
                Value::Null
            } else {
                item["content"]
                    .as_array()
                    .and_then(|p| p.get(part))
                    .ok_or_else(invalid)?
                    .clone()
            };
            let expected = if reasoning {
                summary(item)?
            } else {
                if content["type"] != "output_text" {
                    return Err(invalid());
                }
                content["text"].as_str().ok_or_else(invalid)?.to_owned()
            };
            if kind.ends_with(".delta") {
                if text_done.contains(&key) || parts_done.contains(&key) {
                    return Err(invalid());
                }
                deltas
                    .entry(key)
                    .or_default()
                    .push_str(chunk["delta"].as_str().ok_or_else(invalid)?);
            } else if kind.ends_with("text.done") {
                if !text_done.insert(key) || parts_done.contains(&key) || chunk["text"] != expected
                {
                    return Err(invalid());
                }
            } else if kind == "response.content_part.added" {
                if !parts_added.insert(key)
                    || deltas.contains_key(&key)
                    || text_done.contains(&key)
                    || parts_done.contains(&key)
                    || chunk["part"]["type"] != "output_text"
                {
                    return Err(invalid());
                }
                let text = chunk["part"]["text"].as_str().ok_or_else(invalid)?;
                if !text.is_empty() {
                    deltas.insert(key, text.to_owned());
                }
            } else if !parts_done.insert(key) || chunk["part"] != content {
                return Err(invalid());
            }
        } else if kind.starts_with("response.function_call_arguments.")
            || kind.starts_with("response.reasoning_")
            || kind.starts_with("response.content_part.")
            || kind.starts_with("response.output_text.")
        {
            return Err(invalid());
        }
    }
    if !created
        || parser.finish().map_err(|_| invalid())? != StreamState::Completed
        || terminal.as_ref() != Some(response.wire())
    {
        return Err(invalid());
    }
    for ((index, part), text) in deltas {
        let item = &response.output()[index];
        let expected = if item["type"] == "function_call" {
            item["arguments"].as_str().ok_or_else(invalid)?.to_owned()
        } else if item["type"] == "reasoning" {
            summary(item)?
        } else {
            item["content"][part]["text"]
                .as_str()
                .ok_or_else(invalid)?
                .to_owned()
        };
        if text != expected {
            return Err(invalid());
        }
    }
    Ok(())
}
