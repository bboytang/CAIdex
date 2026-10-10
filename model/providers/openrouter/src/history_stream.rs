use crate::{
    Limits, NativeHistory,
    history::{self, HistoryContext},
};
use caidex_model_core::{
    CanonicalResponse, ProviderError, ProviderResult, ProviderStream, ProviderStreamEvent,
    RequestContext, ResponseEvent, SseEvent, StreamEvent, StreamState,
};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};
use std::{collections::VecDeque, time::Instant};

struct BufferedHistory {
    native: ProviderStream,
    history: HistoryContext,
    context: RequestContext,
    chunks: Vec<Value>,
    bytes: usize,
    pending: VecDeque<ProviderStreamEvent>,
    validated: bool,
}
fn check(context: &RequestContext) -> ProviderResult<()> {
    if context.cancellation.is_cancelled() {
        return Err(ProviderError::new(503, "provider_cancelled"));
    }
    if context.deadline.is_some_and(|d| Instant::now() >= d) {
        return Err(ProviderError::new(504, "provider_timeout"));
    }
    Ok(())
}
// shortcut: buffer native history until verified terminal; add incremental display after safe opaque/tool gating is verified.
pub(crate) fn buffered_stream(
    native: ProviderStream,
    history: HistoryContext,
    context: RequestContext,
    limits: Limits,
) -> ProviderStream {
    let state = BufferedHistory {
        native,
        history,
        context,
        chunks: Vec::new(),
        bytes: 0,
        pending: VecDeque::new(),
        validated: false,
    };
    Box::pin(stream::unfold(Some(state), move |state| {
        let limits = limits.clone();
        async move {
            let mut state = state?;
            if let Err(error) = check(&state.context) {
                return Some((Err(error), None));
            }
            if !state.validated {
                while let Some(event) = state.native.next().await {
                    let event = match event {
                        Ok(event) => event,
                        Err(error) => return Some((Err(error), None)),
                    };
                    if let ProviderStreamEvent::Model(model) = event {
                        let wire = model.response.wire();
                        state.bytes = state.bytes.saturating_add(wire.to_string().len());
                        if state.bytes > limits.response_bytes {
                            return Some((Err(history::too_large()), None));
                        }
                        if let Err(error) = crate::request::output(
                            wire,
                            state.history.policy["native_tools"] == true,
                        ) {
                            return Some((Err(error), None));
                        }
                        state.chunks.push(wire.clone());
                        state.pending.push_back(ProviderStreamEvent::Model(model));
                    } else {
                        return Some((Ok(event), Some(state)));
                    }
                }
                let result = (|| {
                    let terminal = state
                        .chunks
                        .iter()
                        .rev()
                        .find(|v| {
                            matches!(
                                v["type"].as_str(),
                                Some(
                                    "response.completed"
                                        | "response.failed"
                                        | "response.incomplete"
                                )
                            )
                        })
                        .ok_or_else(history::invalid)?;
                    let response = CanonicalResponse::new(terminal["response"].clone())
                        .map_err(|_| history::invalid())?;
                    if response.state() != StreamState::Completed {
                        history::validate_noncompleted(
                            &state.history,
                            &state.chunks,
                            limits.request_bytes.min(limits.response_bytes),
                        )?;
                        return Ok(None);
                    }
                    let recorded = NativeHistory::record(
                        &state.history,
                        &response,
                        Some(&state.chunks),
                        limits.request_bytes.min(limits.response_bytes),
                    )?;
                    projected_stream(&recorded, &limits).map(Some)
                })();
                match result {
                    Ok(Some(pending)) => state.pending = pending,
                    Ok(None) => {}
                    Err(error) => return Some((Err(history::native_error(error)), None)),
                };
                state.chunks.clear();
                state.validated = true;
            }
            if let Err(error) = check(&state.context) {
                return Some((Err(error), None));
            }
            state
                .pending
                .pop_front()
                .map(|event| (Ok(event), Some(state)))
        }
    }))
}
fn projected_stream(
    history: &NativeHistory,
    limits: &Limits,
) -> ProviderResult<VecDeque<ProviderStreamEvent>> {
    let projected = history.to_responses(limits.request_bytes.min(limits.response_bytes))?;
    let mut queue = VecDeque::new();
    let mut bytes = 0;
    let mut sequence = 0;
    let mut emit = |mut wire: Value| -> ProviderResult<()> {
        wire["sequence_number"] = json!(sequence);
        sequence += 1;
        let data = wire.to_string();
        bytes += data.len();
        if data.len() > limits.frame_bytes || bytes > limits.response_bytes {
            return Err(history::too_large());
        }
        let response = ResponseEvent::new(wire).map_err(|_| history::invalid())?;
        queue.push_back(ProviderStreamEvent::Model(StreamEvent {
            frame: SseEvent {
                event: response.kind().into(),
                data,
                id: String::new(),
                retry_ms: None,
            },
            response,
        }));
        Ok(())
    };
    emit(json!({"type":"response.created","response":{"id":projected.id(),"output":[]}}))?;
    for (index, item) in projected.output().iter().enumerate() {
        let mut added = item.clone();
        if item["type"] == "reasoning" {
            added.as_object_mut().unwrap().remove("encrypted_content");
            added["summary"] = json!([]);
        } else if item["type"] == "message" {
            added["status"] = "in_progress".into();
            added["content"] = json!([]);
        } else {
            added["status"] = "in_progress".into();
            let key = if item["type"] == "custom_tool_call" {
                "input"
            } else {
                "arguments"
            };
            added[key] = "".into();
        }
        emit(json!({"type":"response.output_item.added","output_index":index,"item":added}))?;
        if item["type"] == "message" {
            for (part_index, part) in item["content"].as_array().unwrap().iter().enumerate() {
                emit(
                    json!({"type":"response.content_part.added","output_index":index,"content_index":part_index,"item_id":item["id"],"part":{"type":"output_text","text":""}}),
                )?;
                emit(
                    json!({"type":"response.output_text.delta","output_index":index,"content_index":part_index,"item_id":item["id"],"delta":part["text"]}),
                )?;
                emit(
                    json!({"type":"response.output_text.done","output_index":index,"content_index":part_index,"item_id":item["id"],"text":part["text"]}),
                )?;
                emit(
                    json!({"type":"response.content_part.done","output_index":index,"content_index":part_index,"item_id":item["id"],"part":part}),
                )?;
            }
        } else if item["type"] != "reasoning" {
            let (kind, key) = if item["type"] == "custom_tool_call" {
                ("response.custom_tool_call_input", "input")
            } else {
                ("response.function_call_arguments", "arguments")
            };
            emit(
                json!({"type":format!("{kind}.delta"),"output_index":index,"item_id":item["id"],"delta":item[key]}),
            )?;
            let mut done =
                json!({"type":format!("{kind}.done"),"output_index":index,"item_id":item["id"]});
            done[key] = item[key].clone();
            emit(done)?;
        }
        emit(json!({"type":"response.output_item.done","output_index":index,"item":item}))?;
    }
    emit(json!({"type":"response.completed","response":projected.wire()}))?;
    Ok(queue)
}
