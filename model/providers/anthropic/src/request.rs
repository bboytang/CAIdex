use crate::{NativeMessage, ReasoningMapping, ToolMap};
use base64::Engine;
use caidex_model_core::{
    CanonicalRequest, ProviderError, ProviderResult, ResponseItem, ResponsesDialect, ToolKind,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_anthropic_request")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "unsupported_anthropic_request")
}
struct Pending {
    kind: ToolKind,
    name: String,
    namespace: Option<String>,
}
/// A compiled native request and the exact tool declarations used to project
/// its reply. The received Responses wire remains available for owned history.
pub struct MessagesRequest {
    wire: Value,
    source: Value,
    tools: ToolMap,
}
/// Capabilities and mappings fixed by the execution-side model profile.
#[derive(Default)]
pub struct RequestOptions<'a> {
    pub retain_runtime_metadata: bool,
    pub service_tier_mappings: &'a [crate::ServiceTierMapping],
    pub supports_system_messages: bool,
    pub supports_structured_outputs: bool,
    pub summary_mappings: &'a [crate::SummaryMapping],
    pub thinking_context: Option<crate::ThinkingContext>,
    pub reasoning_mappings: &'a [ReasoningMapping],
}
impl MessagesRequest {
    pub fn from_responses(
        request: &CanonicalRequest,
        native_model: &str,
        max_tokens: u64,
        max_bytes: usize,
        max_tools: usize,
    ) -> ProviderResult<Self> {
        Self::from_responses_with_system_messages(
            request,
            native_model,
            max_tokens,
            max_bytes,
            max_tools,
            false,
        )
    }
    /// Enable only from an explicit execution-side capability declaration.
    /// This flag is not inferred from a model name or provided by request JSON.
    pub fn from_responses_with_system_messages(
        request: &CanonicalRequest,
        native_model: &str,
        max_tokens: u64,
        max_bytes: usize,
        max_tools: usize,
        supports_system_messages: bool,
    ) -> ProviderResult<Self> {
        Self::from_responses_with_reasoning(
            request,
            native_model,
            max_tokens,
            max_bytes,
            max_tools,
            supports_system_messages,
            &[],
        )
    }
    /// All mappings come from fixed execution-side configuration; request JSON
    /// cannot provide a native thinking mode or budget itself.
    pub fn from_responses_with_reasoning(
        request: &CanonicalRequest,
        native_model: &str,
        max_tokens: u64,
        max_bytes: usize,
        max_tools: usize,
        supports_system_messages: bool,
        reasoning_mappings: &[ReasoningMapping],
    ) -> ProviderResult<Self> {
        Self::from_responses_with_options(
            request,
            native_model,
            max_tokens,
            max_bytes,
            max_tools,
            &RequestOptions {
                supports_system_messages,
                reasoning_mappings,
                ..Default::default()
            },
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
        let source = request.wire();
        if native_model.trim().is_empty()
            || max_tokens == 0
            || max_bytes == 0
            || source.to_string().len() > max_bytes
        {
            return Err(invalid());
        }
        // Provider-specific reasoning, structured output and context mappings
        // are separate capability gates; never silently drop their semantics.
        for key in source.as_object().unwrap().keys() {
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
                    | "reasoning"
                    | "text"
                    | "include"
                    | "prompt_cache_key"
                    | "client_metadata"
                    | "service_tier"
                    | "stream_options"
                    | "access_programs"
            ) {
                return Err(unsupported());
            }
        }
        if source
            .get("store")
            .is_some_and(|v| v != &Value::Bool(false))
        {
            return Err(unsupported());
        }
        if source
            .get("parallel_tool_calls")
            .is_some_and(|v| !v.is_boolean())
        {
            return Err(invalid());
        }
        let input = match &source["input"] {
            Value::String(text) => vec![json!({"type":"message","role":"user","content":text})],
            Value::Array(items) => items.clone(),
            _ => return Err(invalid()),
        };
        let mut declarations = match source.get("tools") {
            None => Vec::new(),
            Some(Value::Array(items)) => items.clone(),
            _ => return Err(invalid()),
        };
        let mut seen_tools = false;
        for (index, item) in input.iter().enumerate() {
            if item["type"] == "additional_tools" {
                if request.dialect() != ResponsesDialect::Lite
                    || index != 0
                    || seen_tools
                    || item["role"] != "developer"
                {
                    return Err(invalid());
                }
                declarations = item["tools"].as_array().ok_or_else(invalid)?.clone();
                seen_tools = true;
            }
        }
        let tools = ToolMap::new(&declarations, max_tools)?;
        let mut system = Vec::new();
        if let Some(instructions) = source.get("instructions") {
            let text = instructions.as_str().ok_or_else(invalid)?;
            if !text.is_empty() {
                system.push(json!({"type":"text","text":text}));
            }
        }
        let mut messages: Vec<Value> = Vec::new();
        let mut pending: BTreeMap<String, Pending> = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut index = 0;
        while index < input.len() {
            let item = &input[index];
            if item.get("clear_at").is_some() || item.get("output_config").is_some() {
                return Err(unsupported());
            }
            if item["type"] == "additional_tools" {
                index += 1;
                continue;
            }
            if item["type"] == "reasoning" {
                if !pending.is_empty() {
                    return Err(invalid());
                }
                let count = crate::projection::replay_group_len(item, max_bytes)?;
                let end = index
                    .checked_add(count)
                    .filter(|end| *end <= input.len())
                    .ok_or_else(invalid)?;
                let group = &input[index..end];
                let native = NativeMessage::from_responses_output(group, native_model, max_bytes)?;
                for call in &group[1..] {
                    register(call, &mut pending, &mut ids)?;
                }
                // Keep signed content and unknown native blocks in original order.
                messages.push(native.replay_message());
                index = end;
                continue;
            }
            let kind = item["type"].as_str();
            if matches!(kind, Some("function_call" | "custom_tool_call")) {
                if !pending.is_empty() && messages.last().is_some_and(|v| v["role"] != "assistant")
                {
                    return Err(invalid());
                }
                let call = ResponseItem::new(item.clone()).map_err(|_| invalid())?;
                let native = tools.native_call(&call)?;
                register(item, &mut pending, &mut ids)?;
                append(&mut messages, "assistant", vec![native]);
            } else if matches!(
                kind,
                Some("function_call_output" | "custom_tool_call_output")
            ) {
                let result_item = ResponseItem::new(item.clone()).map_err(|_| invalid())?;
                let result = result_item
                    .tool_result()
                    .map_err(|_| invalid())?
                    .ok_or_else(invalid)?;
                let id = if let Some(id) = result.call_id {
                    id.to_owned()
                } else {
                    let name = result.name.ok_or_else(invalid)?;
                    let candidates: Vec<_> = pending
                        .iter()
                        .filter(|(_, p)| {
                            p.kind == result.kind
                                && p.name == name
                                && p.namespace.as_deref() == result.namespace
                        })
                        .map(|(id, _)| id.clone())
                        .collect();
                    if candidates.len() != 1 {
                        return Err(invalid());
                    }
                    candidates[0].clone()
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
                let content = content(result.output, false)?;
                pending.remove(&id);
                append(
                    &mut messages,
                    "user",
                    vec![json!({"type":"tool_result","tool_use_id":id,"content":content})],
                );
            } else if kind.is_none() || kind == Some("message") {
                let role = item["role"].as_str().ok_or_else(invalid)?;
                if !pending.is_empty()
                    && (role != "assistant"
                        || messages.last().is_none_or(|v| v["role"] != "assistant"))
                {
                    return Err(invalid());
                }
                let blocks = content(&item["content"], role == "assistant")?;
                match role {
                    "developer" | "system" if messages.is_empty() => {
                        if blocks.iter().any(|b| b["type"] != "text") {
                            return Err(unsupported());
                        }
                        system.extend(blocks)
                    }
                    "developer" | "system" if options.supports_system_messages => {
                        if blocks.iter().any(|b| b["type"] != "text") {
                            return Err(unsupported());
                        }
                        append(&mut messages, "system", blocks);
                    }
                    "developer" | "system" => return Err(unsupported()),
                    "user" | "assistant" => append(&mut messages, role, blocks),
                    _ => return Err(invalid()),
                }
            } else {
                return Err(unsupported());
            }
            index += 1;
        }
        if !pending.is_empty() || messages.is_empty() {
            return Err(invalid());
        }
        validate_system_positions(&messages)?;
        let mut wire = json!({"model":native_model,"max_tokens":max_tokens,"messages":messages,"stream":request.is_streaming()});
        if !system.is_empty() {
            wire["system"] = system.into();
        }
        if !tools.native_tools().is_empty() {
            wire["tools"] = json!(tools.native_tools());
        }
        let choice = source
            .get("tool_choice")
            .and_then(Value::as_str)
            .unwrap_or("auto");
        if source.get("tool_choice").is_some_and(|v| !v.is_string()) {
            return Err(unsupported());
        }
        if !matches!(choice, "auto" | "required" | "none") {
            return Err(unsupported());
        }
        if !tools.native_tools().is_empty() {
            let mut native_choice =
                json!({"type":match choice { "required"=>"any", "none"=>"none", _=>"auto" }});
            if let Some(parallel) = source.get("parallel_tool_calls") {
                let parallel = parallel.as_bool().ok_or_else(invalid)?;
                if choice != "none" {
                    native_choice["disable_parallel_tool_use"] = (!parallel).into();
                }
            }
            wire["tool_choice"] = native_choice;
        } else if choice == "required" {
            return Err(invalid());
        }
        crate::reasoning::apply(&mut wire, source, options, max_tokens)?;
        crate::structured::apply(&mut wire, source, options.supports_structured_outputs)?;
        crate::runtime_parameters::apply(&mut wire, source, options)?;
        if wire.to_string().len() > max_bytes {
            return Err(invalid());
        }
        Ok(Self {
            wire,
            source: source.clone(),
            tools,
        })
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
fn register(
    item: &Value,
    pending: &mut BTreeMap<String, Pending>,
    ids: &mut BTreeSet<String>,
) -> ProviderResult<()> {
    let item = ResponseItem::new(item.clone()).map_err(|_| invalid())?;
    if let Some(call) = item.tool_call().map_err(|_| invalid())? {
        if !ids.insert(call.call_id.to_owned()) {
            return Err(invalid());
        }
        pending.insert(
            call.call_id.to_owned(),
            Pending {
                kind: call.kind,
                name: call.name.to_owned(),
                namespace: call.namespace.map(str::to_owned),
            },
        );
    }
    Ok(())
}
fn content(value: &Value, assistant: bool) -> ProviderResult<Vec<Value>> {
    if let Some(text) = value.as_str() {
        return Ok(vec![json!({"type":"text","text":text})]);
    }
    let mut blocks = Vec::new();
    for block in value.as_array().ok_or_else(invalid)? {
        match block["type"].as_str() {
            Some("input_text") | Some("output_text")
                if !assistant || block["type"] == "output_text" =>
            {
                blocks.push(
                    json!({"type":"text","text":block["text"].as_str().ok_or_else(invalid)?}),
                );
            }
            Some("input_image") if !assistant => blocks.push(image(block)?),
            _ => return Err(unsupported()),
        }
    }
    if blocks.is_empty() {
        return Err(invalid());
    }
    Ok(blocks)
}
fn append(messages: &mut Vec<Value>, role: &str, blocks: Vec<Value>) {
    if let Some(last) = messages.last_mut().filter(|v| v["role"] == role) {
        last["content"].as_array_mut().unwrap().extend(blocks);
    } else {
        messages.push(json!({"role":role,"content":blocks}));
    }
}
impl fmt::Debug for MessagesRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MessagesRequest([WIRE OMITTED])")
    }
}

fn image(block: &Value) -> ProviderResult<Value> {
    if block.get("detail").is_some_and(|v| v != "auto") {
        return Err(unsupported());
    }
    let url = block["image_url"].as_str().ok_or_else(invalid)?;
    let source = if let Some(data) = url.strip_prefix("data:") {
        let (header, data) = data.split_once(',').ok_or_else(invalid)?;
        let media = header.strip_suffix(";base64").ok_or_else(invalid)?;
        if !matches!(
            media,
            "image/jpeg" | "image/png" | "image/gif" | "image/webp"
        ) {
            return Err(unsupported());
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| invalid())?;
        if decoded.is_empty() {
            return Err(invalid());
        }
        json!({"type":"base64","media_type":media,"data":data})
    } else {
        let parsed = reqwest::Url::parse(url).map_err(|_| invalid())?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
        {
            return Err(invalid());
        }
        json!({"type":"url","url":url})
    };
    Ok(json!({"type":"image","source":source}))
}

fn validate_system_positions(messages: &[Value]) -> ProviderResult<()> {
    for (index, message) in messages.iter().enumerate() {
        if message["role"] != "system" {
            continue;
        }
        let previous = index
            .checked_sub(1)
            .and_then(|i| messages.get(i))
            .ok_or_else(invalid)?;
        let server_result = previous["role"] == "assistant"
            && previous["content"]
                .as_array()
                .and_then(|v| v.last())
                .is_some_and(|block| {
                    matches!(
                        block["type"].as_str(),
                        Some(
                            "web_search_tool_result"
                                | "web_fetch_tool_result"
                                | "code_execution_tool_result"
                                | "bash_code_execution_tool_result"
                                | "text_editor_code_execution_tool_result"
                                | "tool_search_tool_result"
                        )
                    )
                });
        if previous["role"] != "user" && !server_result {
            return Err(invalid());
        }
        if messages
            .get(index + 1)
            .is_some_and(|next| next["role"] != "assistant")
        {
            return Err(invalid());
        }
    }
    Ok(())
}
