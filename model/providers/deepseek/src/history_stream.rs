use crate::{Limits, NativeHistory, tools::ToolMap};
use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, ProviderError, ProviderResult, ProviderStream,
    ProviderStreamEvent, RequestContext, ResponseEvent, SseEvent, StreamEvent,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

fn invalid() -> ProviderError {
    ProviderError::new(502, "deepseek_invalid_history_stream")
}

/// Incremental text uses the existing native I/O. Function calls are delivered
/// only after terminal validation and exact native-history reconstruction.
pub(crate) struct HistoryStream {
    native: Option<ProviderStream>,
    scope: Value,
    tools: ToolMap,
    request: CanonicalRequest,
    context: RequestContext,
    limits: Limits,
    remaining: usize,
    chunks: Vec<Value>,
    pending: VecDeque<StreamEvent>,
    pending_bytes: usize,
    indices: HashMap<u64, u64>,
    added: HashSet<String>,
    next_index: u64,
    sequence: u64,
    reasoning_id: Option<String>,
    native_reasoning_index: Option<u64>,
    terminal: bool,
}
impl HistoryStream {
    pub(crate) fn new(
        native: ProviderStream,
        scope: Value,
        request: CanonicalRequest,
        context: RequestContext,
        limits: Limits,
        tools: ToolMap,
    ) -> Self {
        Self {
            native: Some(native),
            scope,
            tools,
            request,
            context,
            remaining: limits.request_bytes.min(limits.response_bytes),
            limits,
            chunks: Vec::new(),
            pending: VecDeque::new(),
            pending_bytes: 0,
            indices: HashMap::new(),
            added: HashSet::new(),
            next_index: 1,
            sequence: 0,
            reasoning_id: None,
            native_reasoning_index: None,
            terminal: false,
        }
    }
    fn emit(&mut self, mut wire: Value) -> ProviderResult<()> {
        wire["sequence_number"] = self.sequence.into();
        self.sequence = self.sequence.checked_add(1).ok_or_else(invalid)?;
        let response = ResponseEvent::new(wire.clone()).map_err(|_| invalid())?;
        let data = wire.to_string();
        self.pending_bytes = self
            .pending_bytes
            .checked_add(data.len())
            .ok_or_else(invalid)?;
        if data.len() > self.limits.frame_bytes || self.pending_bytes > self.limits.response_bytes {
            return Err(ProviderError::new(502, "deepseek_history_too_large"));
        }
        self.pending.push_back(StreamEvent {
            frame: SseEvent {
                event: response.kind().into(),
                data,
                id: String::new(),
                retry_ms: None,
            },
            response,
        });
        Ok(())
    }
    fn push(&mut self, event: StreamEvent) -> ProviderResult<()> {
        self.remaining = self
            .remaining
            .checked_sub(event.frame.data.len())
            .ok_or_else(|| ProviderError::new(502, "deepseek_history_too_large"))?;
        let mut wire = event.response.wire().clone();
        self.chunks.push(wire.clone());
        match event.response.kind() {
            "response.created" => {
                if self.reasoning_id.is_some() {
                    return Err(invalid());
                }
                let id = wire["response"]["id"].as_str().ok_or_else(invalid)?;
                let reasoning_id = format!("rs_{id}_native");
                self.reasoning_id = Some(reasoning_id.clone());
                wire["response"]["output"] = json!([]);
                self.emit(wire)?;
                self.emit(json!({"type":"response.output_item.added", "output_index":0, "item":{"type":"reasoning", "id":reasoning_id, "summary":[]}}))?;
            }
            "response.in_progress" => {
                wire["response"]["output"] = json!([]);
                self.emit(wire)?;
            }
            "response.output_item.added" => match wire["item"]["type"].as_str() {
                Some("reasoning") => {
                    // shortcut: reject multiple native reasoning items until
                    // their summary indices have an explicit streaming mapping.
                    let index = wire["output_index"].as_u64().ok_or_else(invalid)?;
                    if self.native_reasoning_index.replace(index).is_some() {
                        return Err(invalid());
                    }
                }
                Some(kind) if kind.ends_with("_call") && kind != "function_call" => {
                    return Err(invalid());
                }
                _ => {
                    let source = wire["output_index"].as_u64().ok_or_else(invalid)?;
                    if self.indices.insert(source, self.next_index).is_some() {
                        return Err(invalid());
                    }
                    wire["output_index"] = self.next_index.into();
                    self.next_index = self.next_index.checked_add(1).ok_or_else(invalid)?;
                    if wire["item"]["type"] == "message" {
                        self.added
                            .insert(wire["item"]["id"].as_str().ok_or_else(invalid)?.to_owned());
                        self.emit(wire)?;
                    }
                }
            },
            "response.output_text.delta" | "response.content_part.added" => {
                let source = wire["output_index"].as_u64().ok_or_else(invalid)?;
                wire["output_index"] = self
                    .indices
                    .get(&source)
                    .copied()
                    .ok_or_else(invalid)?
                    .into();
                self.emit(wire)?;
            }
            "response.reasoning_text.delta" => {
                if wire["output_index"].as_u64() != self.native_reasoning_index
                    || self.native_reasoning_index.is_none()
                {
                    return Err(invalid());
                }
                wire["type"] = "response.reasoning_summary_text.delta".into();
                wire["output_index"] = 0.into();
                wire["item_id"] = self
                    .reasoning_id
                    .as_ref()
                    .ok_or_else(invalid)?
                    .clone()
                    .into();
                wire["summary_index"] = wire["content_index"].clone();
                wire.as_object_mut().unwrap().remove("content_index");
                self.emit(wire)?;
            }
            "response.output_item.done"
            | "response.function_call_arguments.delta"
            | "response.function_call_arguments.done"
            | "response.output_text.done"
            | "response.content_part.done"
            | "response.reasoning_text.done" => (),
            kind if kind.starts_with("response.custom_tool_call")
                || kind.starts_with("response.tool_search")
                || kind.starts_with("response.reasoning_summary") =>
            {
                return Err(invalid());
            }
            _ if event.response.terminal().is_some() => {
                if wire.get("response").is_none() {
                    self.emit(wire)?;
                } else {
                    if self.reasoning_id.is_none() {
                        return Err(invalid());
                    }
                    let response =
                        CanonicalResponse::new(wire["response"].clone()).map_err(|_| invalid())?;
                    let budget = self.limits.request_bytes.min(self.limits.response_bytes);
                    let projected = NativeHistory::record(
                        &self.scope,
                        &self.request,
                        &response,
                        &self.tools,
                        Some(&self.chunks),
                        budget,
                    )
                    .map_err(crate::history::native_error)?
                    .to_responses(budget)
                    .map_err(crate::history::native_error)?;
                    for (index, item) in projected.output().iter().enumerate() {
                        if item["type"] != "reasoning"
                            && !item["id"]
                                .as_str()
                                .is_some_and(|id| self.added.contains(id))
                        {
                            let mut start = item.clone();
                            if item["type"] == "function_call" {
                                start["arguments"] = "".into();
                                start["status"] = "in_progress".into();
                            }
                            self.emit(json!({"type":"response.output_item.added", "output_index":index, "item":start}))?;
                        }
                        if item["type"] == "function_call" {
                            self.emit(json!({"type":"response.function_call_arguments.delta", "output_index":index, "item_id":item["id"], "delta":item["arguments"]}))?;
                            self.emit(json!({"type":"response.function_call_arguments.done", "output_index":index, "item_id":item["id"], "arguments":item["arguments"]}))?;
                        }
                        self.emit(json!({"type":"response.output_item.done", "output_index":index, "item":item}))?;
                    }
                    wire["response"] = projected.wire().clone();
                    self.emit(wire)?;
                }
                self.terminal = true;
                self.native.take();
            }
            _ => self.emit(wire)?,
        }
        Ok(())
    }
    fn pop(&mut self) -> Option<ProviderStreamEvent> {
        let event = self.pending.pop_front()?;
        self.pending_bytes -= event.frame.data.len();
        Some(ProviderStreamEvent::Model(event))
    }
    fn fail(&mut self, error: ProviderError) -> Poll<Option<ProviderResult<ProviderStreamEvent>>> {
        self.pending.clear();
        self.pending_bytes = 0;
        self.chunks.clear();
        self.native.take();
        self.terminal = true;
        Poll::Ready(Some(Err(error)))
    }
}
impl futures_util::Stream for HistoryStream {
    type Item = ProviderResult<ProviderStreamEvent>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.context.cancellation.is_cancelled() && (!self.terminal || !self.pending.is_empty())
        {
            return self.fail(ProviderError::new(503, "provider_cancelled"));
        }
        if self.context.deadline.is_some_and(|d| d <= Instant::now())
            && (!self.terminal || !self.pending.is_empty())
        {
            return self.fail(ProviderError::new(504, "provider_timeout"));
        }
        if let Some(event) = self.pop() {
            return Poll::Ready(Some(Ok(event)));
        }
        if self.terminal {
            return Poll::Ready(None);
        }
        let Some(native) = self.native.as_mut() else {
            return self.fail(invalid());
        };
        match native.as_mut().poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(ProviderStreamEvent::Heartbeat))) => {
                Poll::Ready(Some(Ok(ProviderStreamEvent::Heartbeat)))
            }
            Poll::Ready(Some(Ok(ProviderStreamEvent::Model(event)))) => match self.push(event) {
                Ok(()) => Poll::Ready(Some(Ok(self
                    .pop()
                    .unwrap_or(ProviderStreamEvent::Heartbeat)))),
                Err(error) => self.fail(error),
            },
            Poll::Ready(Some(Err(error))) => self.fail(error),
            Poll::Ready(None) => self.fail(ProviderError::new(502, "provider_stream_truncated")),
        }
    }
}
