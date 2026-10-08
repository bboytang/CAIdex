use crate::{NativeHistory, ToolMap};
use caidex_model_core::{
    CanonicalRequest, ProviderError, ProviderResult, ResponseItem, ResponsesDialect, ToolKind,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_google_request")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "unsupported_google_request")
}
struct Pending {
    kind: ToolKind,
    name: String,
    namespace: Option<String>,
    native: Value,
    order: usize,
}
/// Native generateContent body and its original Responses input/declarations.
/// The execution-side model selects the route; this compiler never runs tools.
pub struct GenerateContentRequest {
    wire: Value,
    source: Value,
    tools: ToolMap,
    tool_call_limit: Option<usize>,
}
/// Fixed execution-side capabilities. Empty mappings mean unsupported,
/// not model-name inference or proof of live service compatibility.
#[derive(Default)]
pub struct RequestOptions<'a> {
    /// Retain routing hints locally; no native cache/metadata promise.
    pub retain_runtime_metadata: bool,
    /// Opt into validating a single executable call at the Responses boundary.
    pub enforce_single_tool_call: bool,
    pub service_tier_mappings: &'a [crate::ServiceTierMapping],
    pub verbosity_mappings: &'a [crate::VerbosityMapping],
    pub image_mime_types: &'a [&'a str],
    pub tool_result_image_mime_types: &'a [&'a str],
    pub image_detail_mappings: &'a [crate::ImageDetailMapping],
    pub reasoning_mappings: &'a [crate::ReasoningMapping],
    pub summary_mappings: &'a [crate::SummaryMapping],
    pub thinking_context: Option<crate::ThinkingContext>,
    /// Executor-declared native JSON/schema support, not live compatibility.
    pub supports_structured_outputs: bool,
    /// Separate declaration: native output-schema/tool combinations vary.
    pub supports_structured_outputs_with_tools: bool,
}
impl GenerateContentRequest {
    pub fn from_responses(
        request: &CanonicalRequest,
        native_model: &str,
        max_tokens: u64,
        max_bytes: usize,
        max_tools: usize,
    ) -> ProviderResult<Self> {
        Self::from_responses_with_options(
            request,
            native_model,
            max_tokens,
            max_bytes,
            max_tools,
            &RequestOptions::default(),
        )
    }
    pub fn from_responses_with_options(
        request: &CanonicalRequest,
        native_model: &str,
        max_tokens: u64,
        max_bytes: usize,
        max_tools: usize,
        options: &RequestOptions<'_>,
    ) -> ProviderResult<Self> {
        crate::media::validate_options(options)?;
        let source = request.wire();
        if !crate::catalog::resource_name(native_model)
            || max_tokens == 0
            || max_bytes == 0
            || source.to_string().len() > max_bytes
        {
            return Err(invalid());
        }
        for key in source.as_object().expect("canonical object").keys() {
            if !matches!(
                key.as_str(),
                "model"
                    | "input"
                    | "instructions"
                    | "tools"
                    | "stream"
                    | "store"
                    | "parallel_tool_calls"
                    | "tool_choice"
                    | "include"
                    | "reasoning"
                    | "text"
                    | "prompt_cache_key"
                    | "client_metadata"
                    | "service_tier"
                    | "stream_options"
                    | "access_programs"
            ) {
                return Err(unsupported());
            }
        }
        if source.get("store").is_some_and(|v| v != false) {
            return Err(unsupported());
        }
        if source
            .get("parallel_tool_calls")
            .is_some_and(|v| !v.is_boolean())
        {
            return Err(invalid());
        }
        let input = match &source["input"] {
            Value::String(text) => vec![json!({"role":"user","content":text})],
            Value::Array(items) => items.clone(),
            _ => return Err(invalid()),
        };
        let mut declarations = match source.get("tools") {
            None => Vec::new(),
            Some(Value::Array(tools)) => tools.clone(),
            _ => return Err(invalid()),
        };
        for (index, item) in input.iter().enumerate() {
            if item["type"] == "additional_tools" {
                if request.dialect() != ResponsesDialect::Lite
                    || index != 0
                    || item["role"] != "developer"
                {
                    return Err(invalid());
                }
                declarations = item["tools"].as_array().ok_or_else(invalid)?.clone();
            }
        }
        let tools = ToolMap::new(&declarations, max_tools)?;
        let choice = match source.get("tool_choice") {
            None => "auto",
            Some(Value::String(choice))
                if matches!(choice.as_str(), "auto" | "required" | "none") =>
            {
                choice
            }
            _ => return Err(unsupported()),
        };
        if tools.native_tools().is_empty() && choice == "required" {
            return Err(invalid());
        }
        if !tools.native_tools().is_empty()
            && choice != "none"
            && !options.enforce_single_tool_call
            && source
                .get("parallel_tool_calls")
                .is_some_and(|v| v == false)
        {
            // FunctionCallingConfig has no equivalent single-call constraint.
            return Err(ProviderError::new(
                400,
                "unsupported_google_parallel_tool_calls",
            ));
        }
        let tool_call_limit = if tools.native_tools().is_empty() || choice == "none" {
            Some(0)
        } else if source
            .get("parallel_tool_calls")
            .is_some_and(|v| v == false)
        {
            Some(1)
        } else {
            None
        };
        let mut wire = json!({"generationConfig":{"maxOutputTokens":max_tokens},"contents":[]});
        crate::runtime_parameters::apply(&mut wire, source, options)?;
        // The actual thinking settings are part of every replay prefix.
        crate::reasoning::apply(&mut wire, source, options)?;
        let verbosity =
            crate::structured::apply(&mut wire, source, options, !tools.native_tools().is_empty())?;
        if !tools.native_tools().is_empty() {
            wire["tools"] = json!([{"functionDeclarations":tools.native_tools()}]);
            if source.get("tool_choice").is_some() {
                wire["toolConfig"] = json!({"functionCallingConfig":{"mode":
                    match choice { "required"=>"ANY", "none"=>"NONE", _=>"AUTO" }}});
            }
        }
        let mut system = Vec::new();
        if let Some(instructions) = source.get("instructions") {
            let text = instructions.as_str().ok_or_else(invalid)?;
            if !text.is_empty() {
                system.push(json!({"text":text}));
            }
        }
        let mut contents: Vec<Value> = Vec::new();
        let mut pending = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut results = Vec::new();
        // Never extend a signed native Content with reconstructed display data.
        let mut merge = false;
        let mut index = 0;
        while index < input.len() {
            let item = &input[index];
            if !item.is_object() {
                return Err(invalid());
            }
            if item.get("type").is_some_and(|v| !v.is_string()) {
                return Err(invalid());
            }
            if item.get("clear_at").is_some() || item.get("output_config").is_some() {
                return Err(unsupported());
            }
            let kind = item["type"].as_str();
            if kind == Some("additional_tools") {
                index += 1;
                continue;
            }
            if kind == Some("reasoning") {
                if !pending.is_empty() {
                    return Err(invalid());
                }
                wire["contents"] = json!(contents);
                set_system(&mut wire, &system, verbosity);
                let (history, count) = NativeHistory::from_responses_prefix(
                    &input[index..],
                    native_model,
                    &wire,
                    max_bytes,
                )?;
                let native = history.replay_content().ok_or_else(unsupported)?;
                let calls: Vec<_> = native["parts"]
                    .as_array()
                    .expect("validated parts")
                    .iter()
                    .filter(|part| crate::content::present(part, "functionCall").is_some())
                    .collect();
                if !calls.is_empty()
                    && (history.native_response()["candidates"]
                        .as_array()
                        .expect("validated candidates")
                        .iter()
                        .any(|c| c.get("content") == Some(native) && c["finishReason"] != "STOP")
                        || calls.iter().any(|p| p["thought"] == true))
                {
                    return Err(ProviderError::new(400, "unsupported_google_replay_call"));
                }
                let displayed: Vec<_> = input[index + 1..index + count]
                    .iter()
                    .filter(|item| {
                        matches!(
                            item["type"].as_str(),
                            Some("function_call" | "custom_tool_call")
                        )
                    })
                    .collect();
                if calls.len() != displayed.len() {
                    return Err(invalid());
                }
                for (part, item) in calls.into_iter().zip(displayed) {
                    let native_call = &part["functionCall"];
                    let mapped = tools.responses_call(
                        native_call,
                        item["call_id"].as_str().ok_or_else(invalid)?,
                    )?;
                    let actual = ResponseItem::new(item.clone()).map_err(|_| invalid())?;
                    let a = actual
                        .tool_call()
                        .map_err(|_| invalid())?
                        .ok_or_else(invalid)?;
                    let b = mapped
                        .tool_call()
                        .map_err(|_| invalid())?
                        .ok_or_else(invalid)?;
                    if a.kind != b.kind
                        || a.name != b.name
                        || a.namespace != b.namespace
                        || a.call_id != b.call_id
                    {
                        return Err(invalid());
                    }
                    register(item, native_call.clone(), &mut pending, &mut ids)?;
                }
                let mut replay = native.clone();
                if crate::content::present(&replay, "role").is_none() || replay["role"] == "" {
                    replay["role"] = "model".into();
                }
                contents.push(replay);
                merge = false;
                index += count;
                continue;
            }
            if matches!(kind, Some("function_call" | "custom_tool_call")) {
                if !pending.is_empty()
                    && (!merge || contents.last().is_none_or(|c| c["role"] != "model"))
                {
                    return Err(invalid());
                }
                let call = ResponseItem::new(item.clone()).map_err(|_| invalid())?;
                let native = tools.native_call(&call)?;
                register(item, native.clone(), &mut pending, &mut ids)?;
                append(
                    &mut contents,
                    "model",
                    vec![json!({"functionCall":native})],
                    merge,
                );
            } else if matches!(
                kind,
                Some("function_call_output" | "custom_tool_call_output")
            ) {
                let item = ResponseItem::new(item.clone()).map_err(|_| invalid())?;
                let result = item
                    .tool_result()
                    .map_err(|_| invalid())?
                    .ok_or_else(invalid)?;
                let id = match result.call_id {
                    Some(id) => id.to_owned(),
                    None => {
                        let matches: Vec<_> = pending
                            .iter()
                            .filter(|(_, p): &(_, &Pending)| {
                                p.kind == result.kind
                                    && Some(p.name.as_str()) == result.name
                                    && p.namespace.as_deref() == result.namespace
                            })
                            .map(|(id, _)| id.clone())
                            .collect();
                        if matches.len() != 1 {
                            return Err(invalid());
                        }
                        matches[0].clone()
                    }
                };
                let binding = pending.get(&id).ok_or_else(invalid)?;
                if binding.kind != result.kind
                    || result.name.is_some_and(|v| v != binding.name)
                    || result
                        .namespace
                        .is_some_and(|v| Some(v) != binding.namespace.as_deref())
                {
                    return Err(invalid());
                }
                let mut response =
                    crate::media::tool_output(result.output, binding.order, options, max_bytes)?;
                response["name"] = binding.native["name"].clone();
                if let Some(id) = crate::content::present(&binding.native, "id") {
                    response["id"] = id.clone();
                }
                results.push((binding.order, json!({"functionResponse":response})));
                pending.remove(&id);
                if !pending.is_empty() {
                    // A partial result group cannot admit another model call.
                    merge = false;
                    index += 1;
                    continue;
                }
                // Missing native IDs leave positional association as the only
                // reliable link for repeated aliases. Canonical arrival order
                // remains in source; complete native groups use call order.
                results.sort_by_key(|(order, _)| *order);
                append(
                    &mut contents,
                    "user",
                    std::mem::take(&mut results)
                        .into_iter()
                        .map(|(_, part)| part)
                        .collect(),
                    merge,
                );
            } else if kind.is_none() || kind == Some("message") {
                let role = item["role"].as_str().ok_or_else(invalid)?;
                // Only new, unsigned model Parts may complete the current
                // assistant turn; user text cannot bypass outstanding results.
                if !pending.is_empty()
                    && (role != "assistant"
                        || !merge
                        || contents.last().is_none_or(|c| c["role"] != "model"))
                {
                    return Err(invalid());
                }
                let parts = content_parts(&item["content"], role, options, max_bytes)?;
                match role {
                    "developer" | "system" if contents.is_empty() => {
                        system.extend(parts);
                        index += 1;
                        continue;
                    }
                    "developer" | "system" => return Err(unsupported()),
                    "user" => append(&mut contents, "user", parts, merge),
                    "assistant" => append(&mut contents, "model", parts, merge),
                    _ => return Err(invalid()),
                }
            } else {
                return Err(unsupported());
            }
            merge = true;
            index += 1;
        }
        if !pending.is_empty() || contents.is_empty() {
            return Err(invalid());
        }
        wire["contents"] = json!(contents);
        set_system(&mut wire, &system, verbosity);
        if wire.to_string().len() > max_bytes {
            return Err(invalid());
        }
        crate::client::validate_generation_request(native_model, &wire, true)?;
        Ok(Self {
            wire,
            source: source.clone(),
            tools,
            tool_call_limit,
        })
    }
    /// The Responses consumer must validate this limit before releasing calls.
    /// It is a local delivery policy, never a native generation parameter.
    pub fn tool_call_limit(&self) -> Option<usize> {
        self.tool_call_limit
    }
    pub fn wire(&self) -> &Value {
        &self.wire
    }
    pub fn source(&self) -> &Value {
        &self.source
    }
    pub fn tools(&self) -> &ToolMap {
        &self.tools
    }
}
fn set_system(wire: &mut Value, parts: &[Value], verbosity: Option<&str>) {
    let mut parts = parts.to_vec();
    if let Some(instruction) = verbosity {
        parts.push(json!({"text":instruction}));
    }
    if !parts.is_empty() {
        wire["systemInstruction"] = json!({"parts":parts});
    }
}
fn register(
    item: &Value,
    native: Value,
    pending: &mut BTreeMap<String, Pending>,
    ids: &mut BTreeSet<String>,
) -> ProviderResult<()> {
    let item = ResponseItem::new(item.clone()).map_err(|_| invalid())?;
    let call = item
        .tool_call()
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if !ids.insert(call.call_id.to_owned()) {
        return Err(invalid());
    }
    pending.insert(
        call.call_id.into(),
        Pending {
            kind: call.kind,
            name: call.name.into(),
            namespace: call.namespace.map(str::to_owned),
            native,
            order: ids.len(),
        },
    );
    Ok(())
}
fn content_parts(
    content: &Value,
    role: &str,
    options: &RequestOptions<'_>,
    max_bytes: usize,
) -> ProviderResult<Vec<Value>> {
    if let Some(text) = content.as_str() {
        return Ok(vec![json!({"text":text})]);
    }
    let blocks = content
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or_else(invalid)?;
    blocks
        .iter()
        .map(|block| {
            if block["type"] == "input_image" {
                if role != "user" {
                    return Err(ProviderError::new(400, "unsupported_google_images"));
                }
                crate::media::image(block, options, false, max_bytes)
            } else {
                text_part(block, role == "assistant")
            }
        })
        .collect()
}
pub(crate) fn text_part(block: &Value, assistant: bool) -> ProviderResult<Value> {
    match block["type"].as_str() {
        Some("input_text" | "text") => {
            Ok(json!({"text":block["text"].as_str().ok_or_else(invalid)?}))
        }
        Some("output_text") if assistant => {
            Ok(json!({"text":block["text"].as_str().ok_or_else(invalid)?}))
        }
        _ => Err(unsupported()),
    }
}
fn append(contents: &mut Vec<Value>, role: &str, parts: Vec<Value>, merge: bool) {
    if let Some(last) = contents
        .last_mut()
        .filter(|last| merge && last["role"] == role)
    {
        last["parts"].as_array_mut().expect("parts").extend(parts);
    } else {
        contents.push(json!({"role":role,"parts":parts}));
    }
}
impl fmt::Debug for GenerateContentRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GenerateContentRequest([WIRE OMITTED])")
    }
}
