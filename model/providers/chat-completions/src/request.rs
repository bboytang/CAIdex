use crate::{
    ChatCompletionsConfig,
    tools::{Tools, fields, id, invalid},
};
use caidex_model_core::{
    CanonicalRequest, CapabilitySupport, ModelMetadata, ProviderError, ProviderResult,
    RequestContext, ResponsesDialect,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Prepared {
    pub wire: Value,
    pub tools: Tools,
    pub single: bool,
    pub public_model: String,
}
pub(crate) fn local_context(mut context: RequestContext) -> RequestContext {
    context.headers = Default::default();
    context
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "chat_unsupported_request")
}
fn text_content(value: &Value, role: &str) -> ProviderResult<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.into());
    }
    let mut text = String::new();
    for part in value.as_array().ok_or_else(unsupported)? {
        fields(part, &["type", "text", "annotations"])?;
        if part
            .get("annotations")
            .is_some_and(|v| role != "assistant" || v != &json!([]))
        {
            return Err(unsupported());
        }
        if part["type"] != "input_text" && !(role == "assistant" && part["type"] == "output_text") {
            return Err(unsupported());
        }
        text.push_str(part["text"].as_str().ok_or_else(unsupported)?);
    }
    Ok(text)
}
pub(crate) fn prepare(
    request: &CanonicalRequest,
    metadata: &ModelMetadata,
    context: &RequestContext,
    config: &ChatCompletionsConfig,
) -> ProviderResult<Prepared> {
    let source = request.wire();
    fields(
        source,
        &[
            "model",
            "input",
            "instructions",
            "tools",
            "stream",
            "parallel_tool_calls",
            "tool_choice",
            "temperature",
            "top_p",
            "max_output_tokens",
            "store",
            "reasoning",
            "text",
            "include",
            "client_metadata",
            "prompt_cache_key",
        ],
    )?;
    if metadata.capabilities.text == CapabilitySupport::Unsupported
        || request.is_streaming()
            && metadata.capabilities.streaming == CapabilitySupport::Unsupported
    {
        return Err(unsupported());
    }
    if source.get("store").is_some_and(|v| v != false) {
        return Err(unsupported());
    }
    // There is no portable signed reasoning history. The explicit fixed Runtime
    // no-reasoning profile only accepts its unconditional optional include.
    let reasoning = source.get("reasoning");
    if let Some(reasoning) = reasoning {
        fields(reasoning, &["effort", "context"])?;
        if reasoning.get("effort").is_some_and(|v| v != "none")
            || reasoning.get("context").is_some_and(|v| {
                !config.no_reasoning_runtime
                    || !matches!(v.as_str(), Some("all_turns" | "current_turn"))
            })
        {
            return Err(ProviderError::new(400, "chat_unsupported_reasoning"));
        }
    }
    if source.get("include").is_some_and(|v| {
        v != &json!([])
            && !(config.no_reasoning_runtime && v == &json!(["reasoning.encrypted_content"]))
    }) {
        return Err(ProviderError::new(400, "chat_unsupported_reasoning"));
    }
    if source
        .get("text")
        .is_some_and(|v| v != &json!({"format":{"type":"text"}}))
    {
        return Err(unsupported());
    }
    if !config.runtime_context
        && (context.headers.iter().next().is_some()
            || source.get("client_metadata").is_some()
            || source.get("prompt_cache_key").is_some())
    {
        return Err(unsupported());
    }
    if let Some(metadata) = source.get("client_metadata")
        && (!metadata.is_object()
            || metadata
                .as_object()
                .unwrap()
                .values()
                .any(|v| !v.is_string()))
    {
        return Err(unsupported());
    }
    if source
        .get("prompt_cache_key")
        .is_some_and(|v| !v.is_string())
    {
        return Err(unsupported());
    }
    let mut input = match &source["input"] {
        Value::String(text) => vec![json!({"role":"user","content":text})],
        Value::Array(items) => items.clone(),
        _ => return Err(unsupported()),
    };
    let mut declarations = source
        .get("tools")
        .map(|v| v.as_array().ok_or_else(unsupported))
        .transpose()?
        .cloned()
        .unwrap_or_default();
    if request.dialect() == ResponsesDialect::Lite {
        if !config.lite {
            return Err(unsupported());
        }
        if let Some(item) = input.first().filter(|v| v["type"] == "additional_tools") {
            fields(item, &["type", "id", "role", "tools"])?;
            if item["role"] != "developer" || item.get("id").is_some_and(|v| id(v).is_err()) {
                return Err(invalid());
            }
            declarations = item["tools"].as_array().ok_or_else(invalid)?.clone();
            input.remove(0);
        }
    }
    if !declarations.is_empty()
        && metadata.capabilities.native_tools == CapabilitySupport::Unsupported
    {
        return Err(unsupported());
    }
    let tools = Tools::new(&declarations, config.grammar_prompt_mapping)?;
    let mut wire = json!({"model":metadata.native_model,"messages":[],"stream":request.is_streaming(),"store":false,"n":1});
    if request.is_streaming() {
        wire["stream_options"] = json!({"include_usage":true});
    }
    if !tools.native.is_empty() {
        wire["tools"] = tools.native.clone().into();
    }
    if let Some(choice) = source.get("tool_choice") {
        wire["tool_choice"] = tools.choice(choice)?;
    }
    let single = match source.get("parallel_tool_calls") {
        None => false,
        Some(Value::Bool(value)) => !value,
        _ => return Err(unsupported()),
    };
    if let Some(parallel) = source.get("parallel_tool_calls") {
        if parallel == true
            && metadata.capabilities.parallel_tools == CapabilitySupport::Unsupported
        {
            return Err(unsupported());
        }
        wire["parallel_tool_calls"] = parallel.clone();
    }
    for (key, upper) in [("temperature", 2.0), ("top_p", 1.0)] {
        if let Some(value) = source.get(key) {
            if value.as_f64().is_none_or(|v| !(0.0..=upper).contains(&v)) {
                return Err(unsupported());
            }
            wire[key] = value.clone();
        }
    }
    if let Some(value) = source.get("max_output_tokens") {
        if value.as_u64().is_none_or(|v| v == 0) {
            return Err(unsupported());
        }
        wire["max_completion_tokens"] = value.clone();
    }
    let messages = wire["messages"].as_array_mut().unwrap();
    if let Some(instructions) = source.get("instructions") {
        messages.push(
            json!({"role":"system","content":instructions.as_str().ok_or_else(unsupported)?}),
        );
    }
    let mut pending = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for item in input {
        match item["type"].as_str() {
            Some("function_call" | "custom_tool_call") => {
                if !pending.is_empty()
                    && messages
                        .last()
                        .is_none_or(|v| v["role"] != "assistant" || !v["tool_calls"].is_array())
                {
                    return Err(invalid());
                }
                let call = tools.call(&item)?;
                let call_id = id(&item["call_id"])?;
                if !seen.insert(call_id.to_owned()) {
                    return Err(invalid());
                }
                pending.insert(
                    call_id.to_owned(),
                    item["type"].as_str().unwrap().to_owned(),
                );
                if let Some(last) = messages
                    .last_mut()
                    .filter(|v| v["role"] == "assistant" && v["tool_calls"].is_array())
                {
                    last["tool_calls"].as_array_mut().unwrap().push(call);
                } else {
                    messages.push(json!({"role":"assistant","content":null,"tool_calls":[call]}));
                }
            }
            Some("function_call_output" | "custom_tool_call_output") => {
                fields(&item, &["type", "id", "call_id", "output"])?;
                let call_id = id(&item["call_id"])?;
                let kind = pending.remove(call_id).ok_or_else(invalid)?;
                if item["type"] != format!("{kind}_output") {
                    return Err(invalid());
                }
                // Tool result blocks retain their exact serialized bytes; media
                // results have no portable text equivalent and are rejected.
                let content = text_content(&item["output"], "tool")?;
                messages.push(json!({"role":"tool","tool_call_id":call_id,"content":content}));
            }
            None | Some("message") => {
                if !pending.is_empty() {
                    return Err(invalid());
                }
                fields(&item, &["type", "id", "role", "status", "content", "phase"])?;
                let role = item["role"]
                    .as_str()
                    .filter(|s| matches!(*s, "user" | "developer" | "system" | "assistant"))
                    .ok_or_else(unsupported)?;
                if item
                    .get("id")
                    .is_some_and(|v| !config.runtime_context || id(v).is_err())
                    || item.get("status").is_some_and(|v| v != "completed")
                    || item.get("phase").is_some_and(|v| {
                        role != "assistant"
                            || !matches!(v.as_str(), Some("final_answer" | "commentary"))
                    })
                {
                    return Err(unsupported());
                }
                messages.push(json!({"role":role,"content":text_content(&item["content"], role)?}));
            }
            _ => return Err(ProviderError::new(400, "chat_unsupported_history")),
        }
    }
    if !pending.is_empty() || messages.is_empty() {
        return Err(invalid());
    }
    Ok(Prepared {
        wire,
        tools,
        single,
        public_model: metadata.id.clone(),
    })
}
