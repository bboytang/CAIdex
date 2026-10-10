use crate::request::Prepared;
use caidex_model_core::{
    CancellationToken, CanonicalResponse, ProviderError, ProviderResult, ProviderStream,
    ProviderStreamEvent, ResponseEvent, SseEvent, StreamEvent,
};
use futures_util::{Stream, StreamExt, stream};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    pin::Pin,
};
fn invalid() -> ProviderError {
    ProviderError::new(502, "chat_invalid_native_response")
}
fn string<'a>(value: &'a Value, key: &str) -> ProviderResult<&'a str> {
    value[key]
        .as_str()
        .filter(|v| !v.is_empty() && !v.chars().any(char::is_control))
        .ok_or_else(invalid)
}
fn usage(native: &Value) -> ProviderResult<Value> {
    if native.is_null() {
        return Ok(Value::Null);
    }
    if !native.is_object() {
        return Err(invalid());
    }
    let mut usage = json!({});
    for (native_key, canonical) in [
        ("prompt_tokens", "input_tokens"),
        ("completion_tokens", "output_tokens"),
        ("total_tokens", "total_tokens"),
    ] {
        if let Some(value) = native.get(native_key).filter(|v| !v.is_null()) {
            value.as_u64().ok_or_else(invalid)?;
            usage[canonical] = value.clone();
        }
    }
    for (native_key, count, canonical) in [
        (
            "prompt_tokens_details",
            "cached_tokens",
            "input_tokens_details",
        ),
        (
            "completion_tokens_details",
            "reasoning_tokens",
            "output_tokens_details",
        ),
    ] {
        if let Some(details) = native.get(native_key).filter(|v| !v.is_null()) {
            if !details.is_object() {
                return Err(invalid());
            }
            if let Some(value) = details.get(count).filter(|v| !v.is_null()) {
                let n = value.as_u64().ok_or_else(invalid)?;
                let total = usage[if canonical == "input_tokens_details" {
                    "input_tokens"
                } else {
                    "output_tokens"
                }]
                .as_u64();
                if total.is_some_and(|total| n > total) {
                    return Err(invalid());
                }
                usage[canonical] = json!({count:value});
            }
        }
    }
    if let (Some(input), Some(output), Some(total)) = (
        usage["input_tokens"].as_u64(),
        usage["output_tokens"].as_u64(),
        usage["total_tokens"].as_u64(),
    ) && input.checked_add(output) != Some(total)
    {
        return Err(invalid());
    }
    usage["native_chat_usage"] = native.clone();
    Ok(usage)
}
pub(crate) fn project(native: Value, prepared: &Prepared) -> ProviderResult<CanonicalResponse> {
    if native.get("error").is_some_and(|v| !v.is_null()) {
        return Err(ProviderError::new(502, "chat_native_error"));
    }
    let id = string(&native, "id")?;
    if native["object"] != "chat.completion" || native["model"] != prepared.wire["model"] {
        return Err(invalid());
    }
    native["created"].as_u64().ok_or_else(invalid)?;
    let choices = native["choices"]
        .as_array()
        .filter(|c| c.len() == 1)
        .ok_or_else(invalid)?;
    let choice = &choices[0];
    if choice["index"] != 0 {
        return Err(invalid());
    }
    let message = &choice["message"];
    if message["role"] != "assistant"
        || message.as_object().is_none_or(|o| {
            o.keys()
                .any(|k| !["role", "content", "tool_calls", "refusal"].contains(&k.as_str()))
        })
    {
        return Err(invalid());
    }
    let finish = choice["finish_reason"].as_str().ok_or_else(invalid)?;
    let calls = message
        .get("tool_calls")
        .filter(|v| !v.is_null())
        .map(|v| v.as_array().ok_or_else(invalid))
        .transpose()?
        .cloned()
        .unwrap_or_default();
    let choice_policy = &prepared.wire["tool_choice"];
    if (choice_policy == "none" && !calls.is_empty())
        || (choice_policy == "required" && calls.is_empty())
        || (choice_policy.is_object()
            && (calls.is_empty()
                || calls
                    .iter()
                    .any(|call| call["function"]["name"] != choice_policy["function"]["name"])))
    {
        return Err(ProviderError::new(502, "chat_tool_choice_violation"));
    }
    if prepared.single && calls.len() > 1 {
        return Err(ProviderError::new(502, "chat_parallel_tools_rejected"));
    }
    if (finish == "tool_calls") != !calls.is_empty()
        || !matches!(finish, "stop" | "tool_calls" | "length" | "content_filter")
    {
        return Err(invalid());
    }
    let mut output = Vec::new();
    let mut content = Vec::new();
    if let Some(text) = message.get("content").filter(|v| !v.is_null()) {
        content.push(json!({"type":"output_text","text":text.as_str().ok_or_else(invalid)?,"annotations":[]}));
    }
    if let Some(refusal) = message.get("refusal").filter(|v| !v.is_null()) {
        content.push(json!({"type":"refusal","refusal":refusal.as_str().ok_or_else(invalid)?}));
    }
    if !content.is_empty() {
        output.push(json!({"id":format!("msg_{id}"),"type":"message","role":"assistant","status":"completed","content":content}));
    }
    let mut ids = BTreeSet::new();
    for call in calls {
        if !ids.insert(string(&call, "id")?.to_owned()) {
            return Err(invalid());
        }
        output.push(prepared.tools.output(&call)?);
    }
    let mut response = json!({"id":id,"object":"response","model":prepared.public_model,"status":if matches!(finish,"length" | "content_filter") {"incomplete"} else {"completed"},"output":output,"usage":usage(&native["usage"])?});
    if response["status"] == "incomplete" {
        response["incomplete_details"] =
            json!({"reason":if finish == "length" {"max_output_tokens"} else {"content_filter"}});
    }
    CanonicalResponse::new(response).map_err(|_| invalid())
}
#[derive(Default)]
struct Accumulator {
    id: Option<String>,
    model: Option<String>,
    created: Option<Value>,
    role: bool,
    text: String,
    text_present: bool,
    refusal: String,
    calls: BTreeMap<u64, Value>,
    finish: Option<String>,
    usage: Option<Value>,
}
impl Accumulator {
    fn push(&mut self, native: Value) -> ProviderResult<()> {
        if native.get("error").is_some_and(|v| !v.is_null()) {
            return Err(ProviderError::new(502, "chat_native_error"));
        }
        if native["object"] != "chat.completion.chunk" {
            return Err(invalid());
        }
        let id = string(&native, "id")?;
        let model = string(&native, "model")?;
        if self.id.as_deref().is_some_and(|v| v != id)
            || self.model.as_deref().is_some_and(|v| v != model)
            || self
                .created
                .as_ref()
                .is_some_and(|v| v != &native["created"])
        {
            return Err(invalid());
        }
        native["created"].as_u64().ok_or_else(invalid)?;
        self.id = Some(id.into());
        self.model = Some(model.into());
        self.created = Some(native["created"].clone());
        let choices = native["choices"].as_array().ok_or_else(invalid)?;
        if choices.is_empty() {
            if self.finish.is_none() || self.usage.is_some() || native["usage"].is_null() {
                return Err(invalid());
            }
            usage(&native["usage"])?;
            self.usage = Some(native["usage"].clone());
            return Ok(());
        }
        if choices.len() != 1
            || self.finish.is_some()
            || self.usage.is_some()
            || choices[0]["index"] != 0
            || !native["usage"].is_null()
        {
            return Err(invalid());
        }
        let delta = &choices[0]["delta"];
        if delta.as_object().is_none_or(|o| {
            o.keys()
                .any(|k| !["role", "content", "refusal", "tool_calls"].contains(&k.as_str()))
        }) {
            return Err(invalid());
        }
        if let Some(role) = delta.get("role").filter(|v| !v.is_null()) {
            if role != "assistant" || self.role {
                return Err(invalid());
            }
            self.role = true;
        }
        if !self.role {
            return Err(invalid());
        }
        self.text_present |= delta.get("content").is_some_and(|v| !v.is_null());
        for (key, text) in [("content", &mut self.text), ("refusal", &mut self.refusal)] {
            if let Some(value) = delta.get(key).filter(|v| !v.is_null()) {
                text.push_str(value.as_str().ok_or_else(invalid)?);
            }
        }
        if let Some(calls) = delta.get("tool_calls").filter(|v| !v.is_null()) {
            for call in calls.as_array().ok_or_else(invalid)? {
                if call.as_object().is_none_or(|o| {
                    o.keys()
                        .any(|k| !["index", "id", "type", "function"].contains(&k.as_str()))
                }) {
                    return Err(invalid());
                }
                let index = call["index"]
                    .as_u64()
                    .filter(|i| *i < 128)
                    .ok_or_else(invalid)?;
                let target = self
                    .calls
                    .entry(index)
                    .or_insert_with(|| json!({"type":"function","function":{"arguments":""}}));
                for key in ["id", "type"] {
                    if let Some(value) = call.get(key).filter(|v| !v.is_null()) {
                        if (key == "type" && value != "function")
                            || (key == "id" && target.get("id").is_some_and(|prior| prior != value))
                        {
                            return Err(invalid());
                        }
                        target[key] = value.clone();
                    }
                }
                if let Some(function) = call.get("function").filter(|v| !v.is_null()) {
                    if function.as_object().is_none_or(|o| {
                        o.keys()
                            .any(|k| !["name", "arguments"].contains(&k.as_str()))
                    }) {
                        return Err(invalid());
                    }
                    if let Some(name) = function.get("name").filter(|v| !v.is_null()) {
                        let mut combined =
                            target["function"]["name"].as_str().unwrap_or("").to_owned();
                        combined.push_str(name.as_str().ok_or_else(invalid)?);
                        target["function"]["name"] = combined.into();
                    }
                    if let Some(arguments) = function.get("arguments").filter(|v| !v.is_null()) {
                        let mut text = target["function"]["arguments"].as_str().unwrap().to_owned();
                        text.push_str(arguments.as_str().ok_or_else(invalid)?);
                        target["function"]["arguments"] = text.into();
                    }
                }
            }
        }
        if let Some(finish) = choices[0].get("finish_reason").filter(|v| !v.is_null()) {
            self.finish = Some(finish.as_str().ok_or_else(invalid)?.into());
        }
        Ok(())
    }
    fn complete(self, prepared: &Prepared) -> ProviderResult<CanonicalResponse> {
        if self
            .calls
            .keys()
            .enumerate()
            .any(|(i, key)| *key != i as u64)
        {
            return Err(invalid());
        }
        let finish = self
            .finish
            .ok_or_else(|| ProviderError::new(502, "provider_stream_truncated"))?;
        let mut message = json!({"role":"assistant","content":if self.text_present { Value::String(self.text) } else { Value::Null }});
        if !self.refusal.is_empty() {
            message["refusal"] = self.refusal.into();
        }
        if !self.calls.is_empty() {
            message["tool_calls"] = self.calls.into_values().collect::<Vec<_>>().into();
        }
        project(
            json!({"id":self.id,"object":"chat.completion","created":self.created,"model":self.model,"choices":[{"index":0,"message":message,"finish_reason":finish}],"usage":self.usage}),
            prepared,
        )
    }
}
fn events(
    response: CanonicalResponse,
    limit: usize,
) -> ProviderResult<VecDeque<ProviderStreamEvent>> {
    let mut wires = Vec::new();
    let mut bytes = 0usize;
    let mut add = |mut wire: Value| -> ProviderResult<()> {
        wire["sequence_number"] = wires.len().into();
        let size = wire.to_string().len();
        if size > limit.saturating_sub(bytes) {
            return Err(ProviderError::new(502, "provider_response_too_large"));
        }
        bytes += size;
        wires.push(wire);
        Ok(())
    };
    add(
        json!({"type":"response.created","response":{"id":response.id(),"status":"in_progress","output":[]}}),
    )?;
    for (index, item) in response.output().iter().enumerate() {
        add(json!({"type":"response.output_item.added","output_index":index,"item":item}))?;
        if item["type"] == "message" {
            for (content_index, part) in item["content"].as_array().unwrap().iter().enumerate() {
                add(
                    json!({"type":"response.content_part.added","item_id":item["id"],"output_index":index,"content_index":content_index,"part":part}),
                )?;
                if let Some(text) = part["text"].as_str() {
                    add(
                        json!({"type":"response.output_text.delta","item_id":item["id"],"output_index":index,"content_index":content_index,"delta":text}),
                    )?;
                    add(
                        json!({"type":"response.output_text.done","item_id":item["id"],"output_index":index,"content_index":content_index,"text":text}),
                    )?;
                }
                add(
                    json!({"type":"response.content_part.done","item_id":item["id"],"output_index":index,"content_index":content_index,"part":part}),
                )?;
            }
        } else {
            let (kind, key) = if item["type"] == "custom_tool_call" {
                ("response.custom_tool_call_input", "input")
            } else {
                ("response.function_call_arguments", "arguments")
            };
            add(
                json!({"type":format!("{kind}.delta"),"item_id":item["id"],"output_index":index,"delta":item[key]}),
            )?;
            add(
                json!({"type":format!("{kind}.done"),"item_id":item["id"],"output_index":index,key:item[key]}),
            )?;
        }
        add(json!({"type":"response.output_item.done","output_index":index,"item":item}))?;
    }
    add(
        json!({"type":format!("response.{}",response.wire()["status"].as_str().unwrap()),"response":response.wire()}),
    )?;
    wires
        .into_iter()
        .enumerate()
        .map(|(sequence, mut wire)| {
            wire["sequence_number"] = sequence.into();
            let response = ResponseEvent::new(wire.clone()).map_err(|_| invalid())?;
            Ok(ProviderStreamEvent::Model(StreamEvent {
                frame: SseEvent {
                    event: response.kind().into(),
                    data: wire.to_string(),
                    id: String::new(),
                    retry_ms: None,
                },
                response,
            }))
        })
        .collect()
}
pub(crate) fn project_stream(
    native: Pin<Box<dyn Stream<Item = ProviderResult<SseEvent>> + Send>>,
    prepared: Prepared,
    cancellation: CancellationToken,
    deadline: std::time::Instant,
    limit: usize,
) -> ProviderStream {
    struct Delivery {
        native: Option<Pin<Box<dyn Stream<Item = ProviderResult<SseEvent>> + Send>>>,
        prepared: Prepared,
        cancellation: CancellationToken,
        deadline: std::time::Instant,
        queued: VecDeque<ProviderStreamEvent>,
        loaded: bool,
        limit: usize,
    }
    Box::pin(stream::unfold(
        Some(Delivery {
            native: Some(native),
            prepared,
            cancellation,
            deadline,
            queued: VecDeque::new(),
            loaded: false,
            limit,
        }),
        |delivery| async move {
            let mut delivery = delivery?;
            let result =
                async {
                    if delivery.cancellation.is_cancelled() {
                        return Err(ProviderError::new(503, "provider_cancelled"));
                    }
                    if std::time::Instant::now() >= delivery.deadline {
                        return Err(ProviderError::new(504, "provider_timeout"));
                    }
                    if !delivery.loaded {
                        let mut accumulated = Accumulator::default();
                        loop {
                            let event =
                                delivery.native.as_mut().unwrap().next().await.ok_or_else(
                                    || ProviderError::new(502, "provider_stream_truncated"),
                                )??;
                            if event.event != "message" {
                                return Err(invalid());
                            }
                            if event.data == "[DONE]" {
                                let response = accumulated.complete(&delivery.prepared)?;
                                if serde_json::to_vec(response.wire()).expect("JSON").len()
                                    > delivery.limit
                                {
                                    return Err(ProviderError::new(
                                        502,
                                        "provider_response_too_large",
                                    ));
                                }
                                delivery.queued = events(response, delivery.limit)?;
                                delivery.native = None;
                                delivery.loaded = true;
                                break;
                            }
                            accumulated
                                .push(serde_json::from_str(&event.data).map_err(|_| invalid())?)?;
                        }
                    }
                    Ok(delivery.queued.pop_front())
                }
                .await;
            match result {
                Ok(Some(event)) => Some((Ok(event), Some(delivery))),
                Ok(None) => None,
                Err(error) => Some((Err(error), None)),
            }
        },
    ))
}
