use crate::{DeepSeekConfig, tools::ToolMap};
use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, ProviderError, ProviderResult, ResponsesDialect,
    ResponsesStream,
};
use serde_json::{Value, json};
use std::{collections::HashMap, fmt};

const PREFIX: &str = "caidex.deepseek.native-history.v1:";
const PATCH_PREFIX: &str = "caidex.deepseek.native-history.v2:";
fn invalid() -> ProviderError {
    ProviderError::new(400, "deepseek_invalid_history")
}
pub(crate) fn native_error(error: ProviderError) -> ProviderError {
    if error.http_status == 400 {
        ProviderError::new(502, "deepseek_invalid_native_history")
    } else {
        error
    }
}

/// Sensitive native wire, bound to execution scope and the compiled prefix.
/// The JSON carrier is neither encryption nor source authentication.
#[derive(Clone)]
pub struct NativeHistory(Value);
impl NativeHistory {
    pub(crate) fn record(
        scope: &Value,
        request: &CanonicalRequest,
        response: &CanonicalResponse,
        tools: &ToolMap,
        chunks: Option<&[Value]>,
        max_bytes: usize,
    ) -> ProviderResult<Self> {
        let mut wire = json!({"provider":"deepseek", "version":if tools.source()["apply_patch"] == true {2} else {1}, "scope":scope, "native_model":request.model(), "request":request.wire(), "response":response.wire(), "tool_mapping":tools.source(), "source":if chunks.is_some() {"sse"} else {"json"}});
        if let Some(chunks) = chunks {
            wire["chunks"] = json!(chunks);
        }
        Self::new(wire, max_bytes)
    }
    fn new(wire: Value, max_bytes: usize) -> ProviderResult<Self> {
        let history = Self(wire);
        history.check_size(max_bytes)?;
        if history.0["provider"] != "deepseek"
            || history.0["version"]
                != if history.0["tool_mapping"]["apply_patch"] == true {
                    2
                } else {
                    1
                }
            || !history.0["scope"].is_object()
            || history.0["request"]["model"] != history.0["native_model"]
        {
            return Err(invalid());
        }
        let request = CanonicalRequest::new(history.request().clone(), ResponsesDialect::Classic)
            .map_err(|_| invalid())?;
        let response =
            CanonicalResponse::new(history.native_response().clone()).map_err(|_| invalid())?;
        if response
            .wire()
            .get("model")
            .is_some_and(|v| v != &history.0["native_model"])
        {
            return Err(invalid());
        }
        let tools = ToolMap::from_source(history.0["tool_mapping"].clone())?;
        tools.matches(&request)?;
        tools.validate_response(&request, &response)?;
        for item in response
            .output()
            .iter()
            .filter(|item| item["type"] == "reasoning")
        {
            if item.get("encrypted_content").is_some_and(|v| !v.is_null())
                || item
                    .get("summary")
                    .is_some_and(|v| v.as_array().is_none_or(|v| !v.is_empty()))
            {
                return Err(invalid());
            }
            for part in item["content"].as_array().ok_or_else(invalid)? {
                if part["type"] != "reasoning_text" || !part["text"].is_string() {
                    return Err(invalid());
                }
            }
        }
        match history.0["source"].as_str() {
            Some("json") if history.0.get("chunks").is_none() => (),
            Some("sse") => validate_chunks(
                &response,
                history.0["chunks"].as_array().ok_or_else(invalid)?,
                max_bytes,
            )?,
            _ => return Err(invalid()),
        }
        Ok(history)
    }
    fn check_size(&self, max_bytes: usize) -> ProviderResult<()> {
        if max_bytes == 0 || self.0.to_string().len().saturating_add(PREFIX.len()) > max_bytes {
            return Err(ProviderError::new(502, "deepseek_history_too_large"));
        }
        Ok(())
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
    pub fn to_responses(&self, max_bytes: usize) -> ProviderResult<CanonicalResponse> {
        self.check_size(max_bytes)?;
        let tools = ToolMap::from_source(self.0["tool_mapping"].clone())?;
        let response =
            CanonicalResponse::new(self.native_response().clone()).map_err(|_| invalid())?;
        let mut wire = tools.project(&response)?.wire().clone();
        let native = wire["output"].as_array().unwrap();
        let summary: Vec<_> = native
            .iter()
            .filter(|item| item["type"] == "reasoning")
            .flat_map(|item| item["content"].as_array().unwrap())
            .map(|part| json!({"type":"summary_text", "text":part["text"]}))
            .collect();
        // shortcut: full prefixes grow quadratically; bounded budgets reject
        // overflow until Host persistence can deduplicate native history.
        let prefix = if self.0["version"] == 2 {
            PATCH_PREFIX
        } else {
            PREFIX
        };
        let mut output = vec![
            json!({"type":"reasoning", "id":format!("rs_{}_native", response.id()), "summary":summary, "encrypted_content":format!("{prefix}{}", self.0)}),
        ];
        output.extend(
            native
                .iter()
                .filter(|item| item["type"] != "reasoning")
                .cloned(),
        );
        wire["output"] = output.into();
        if wire.to_string().len() > max_bytes {
            return Err(ProviderError::new(502, "deepseek_history_too_large"));
        }
        CanonicalResponse::new(wire).map_err(|_| invalid())
    }
    fn restore(
        input: &[Value],
        config: &DeepSeekConfig,
        expected: &CanonicalRequest,
        tools: &ToolMap,
        max_bytes: usize,
    ) -> ProviderResult<(Self, usize)> {
        let carrier = input.first().ok_or_else(invalid)?;
        let capsule = carrier["encrypted_content"].as_str().ok_or_else(invalid)?;
        if carrier["type"] != "reasoning" || capsule.len() > max_bytes {
            return Err(invalid());
        }
        let (encoded, version) = if let Some(wire) = capsule.strip_prefix(PREFIX) {
            (wire, 1)
        } else {
            (capsule.strip_prefix(PATCH_PREFIX).ok_or_else(invalid)?, 2)
        };
        let wire = serde_json::from_str(encoded).map_err(|_| invalid())?;
        let history = Self::new(wire, max_bytes).map_err(|_| invalid())?;
        if history.0["version"] != version {
            return Err(invalid());
        }
        if history.0["scope"] != config.replay_scope()
            || history.0["native_model"] != expected.model()
        {
            return Err(ProviderError::new(400, "deepseek_history_model_mismatch"));
        }
        if prefix(history.request()) != prefix(expected.wire())
            || history.0["tool_mapping"] != *tools.source()
        {
            return Err(ProviderError::new(400, "deepseek_history_prefix_mismatch"));
        }
        let display = history.to_responses(max_bytes).map_err(|_| invalid())?;
        let count = display.output().len();
        for (actual, expected) in input
            .get(..count)
            .ok_or_else(invalid)?
            .iter()
            .zip(display.output())
        {
            if actual.get("id").is_some_and(|v| {
                v.as_str()
                    .is_none_or(|s| s.trim().is_empty() || s.chars().any(char::is_control))
            }) || actual.get("status").is_some_and(|v| v != "completed")
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
    if let Some(object) = item.as_object_mut() {
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
        json!([{"role":"user", "content":text}])
    } else {
        wire["input"].clone()
    };
    json!({"model":wire["model"], "input":input, "instructions":wire["instructions"], "tools":wire["tools"], "tool_choice":wire["tool_choice"]})
}
pub(crate) fn native_request(
    request: &CanonicalRequest,
    model: &str,
) -> ProviderResult<CanonicalRequest> {
    let mut wire = request.wire().clone();
    wire["model"] = model.into();
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}
pub(crate) fn expand(
    request: CanonicalRequest,
    config: &DeepSeekConfig,
    tools: &ToolMap,
    max_bytes: usize,
) -> ProviderResult<CanonicalRequest> {
    let Some(input) = request.wire()["input"].as_array() else {
        return Ok(request);
    };
    let mut native = Vec::new();
    let mut index = 0;
    while index < input.len() {
        if input[index]["type"] == "reasoning" {
            let mut wire = request.wire().clone();
            wire["input"] = json!(native);
            let expected =
                CanonicalRequest::new(wire, ResponsesDialect::Classic).map_err(|_| invalid())?;
            let (history, count) =
                NativeHistory::restore(&input[index..], config, &expected, tools, max_bytes)?;
            native.extend(
                history.native_response()["output"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
            index += count;
        } else {
            native.push(tools.compile_item(&input[index])?);
            index += 1;
        }
    }
    let mut wire = request.wire().clone();
    wire["input"] = native.into();
    if wire.to_string().len() > max_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}
fn validate_chunks(
    response: &CanonicalResponse,
    chunks: &[Value],
    max_bytes: usize,
) -> ProviderResult<()> {
    let mut parser = ResponsesStream::new(max_bytes).map_err(|_| invalid())?;
    let mut terminal = None;
    let mut deltas: HashMap<(usize, usize, String), String> = HashMap::new();
    for chunk in chunks {
        for event in parser
            .push(format!("data: {chunk}\n\n").as_bytes())
            .map_err(|_| invalid())?
        {
            if event.response.terminal().is_some() {
                terminal = event.response.wire().get("response").cloned();
            }
        }
        if matches!(
            chunk["type"].as_str(),
            Some(
                "response.function_call_arguments.delta"
                    | "response.custom_tool_call_input.delta"
                    | "response.output_text.delta"
                    | "response.reasoning_text.delta"
            )
        ) {
            let index = chunk["output_index"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(invalid)?;
            let part = if matches!(
                chunk["type"].as_str(),
                Some(
                    "response.function_call_arguments.delta"
                        | "response.custom_tool_call_input.delta"
                )
            ) {
                0
            } else {
                chunk["content_index"]
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(invalid)?
            };
            deltas
                .entry((index, part, chunk["type"].as_str().unwrap().into()))
                .or_default()
                .push_str(chunk["delta"].as_str().ok_or_else(invalid)?);
            if chunk["item_id"] != response.output().get(index).ok_or_else(invalid)?["id"] {
                return Err(invalid());
            }
        }
    }
    parser.finish().map_err(|_| invalid())?;
    if terminal.as_ref() != Some(response.wire()) {
        return Err(invalid());
    }
    for ((index, part, kind), text) in deltas {
        let item = response.output().get(index).ok_or_else(invalid)?;
        let expected = if kind == "response.function_call_arguments.delta" {
            if item["type"] != "function_call" {
                return Err(invalid());
            }
            &item["arguments"]
        } else if kind == "response.custom_tool_call_input.delta" {
            if item["type"] != "custom_tool_call" {
                return Err(invalid());
            }
            &item["input"]
        } else {
            &item["content"][part]["text"]
        };
        if expected.as_str() != Some(&text) {
            return Err(invalid());
        }
    }
    for chunk in chunks {
        if matches!(
            chunk["type"].as_str(),
            Some("response.function_call_arguments.done" | "response.custom_tool_call_input.done")
        ) {
            let index = chunk["output_index"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(invalid)?;
            let item = response.output().get(index).ok_or_else(invalid)?;
            let (kind, field) = if chunk["type"] == "response.function_call_arguments.done" {
                ("function_call", "arguments")
            } else {
                ("custom_tool_call", "input")
            };
            if item["type"] != kind || chunk["item_id"] != item["id"] || chunk[field] != item[field]
            {
                return Err(invalid());
            }
        }
        if matches!(
            chunk["type"].as_str(),
            Some("response.output_item.added" | "response.output_item.done")
        ) {
            let index = chunk["output_index"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(invalid)?;
            let final_item = response.output().get(index).ok_or_else(invalid)?;
            if chunk["type"] == "response.output_item.done" {
                if chunk["item"] != *final_item {
                    return Err(invalid());
                }
            } else {
                for field in ["type", "id", "role", "name", "call_id", "namespace"] {
                    if chunk["item"]
                        .get(field)
                        .is_some_and(|v| final_item.get(field) != Some(v))
                    {
                        return Err(invalid());
                    }
                }
            }
        }
    }
    Ok(())
}
