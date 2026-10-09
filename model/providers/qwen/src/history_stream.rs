use crate::{Limits, NativeHistory, history, tools::ToolMap};
use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, ProviderError, ProviderResult, ProviderStream,
    ProviderStreamEvent, RequestContext, ResponseEvent, SseEvent, StreamEvent, StreamState,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};
fn invalid() -> ProviderError {
    ProviderError::new(502, "qwen_invalid_history_stream")
}
struct NativeItem {
    kind: String,
    id: Value,
    done: bool,
}

/// Incremental display and exact raw capture share the existing native stream.
/// Only a validated completed terminal can publish a replayable capsule.
pub(crate) struct HistoryStream {
    native: Option<ProviderStream>,
    scope: Value,
    request: CanonicalRequest,
    tools: Option<ToolMap>,
    context: RequestContext,
    limits: Limits,
    remaining: usize,
    chunks: Vec<Value>,
    pending: VecDeque<StreamEvent>,
    pending_bytes: usize,
    items: HashMap<u64, NativeItem>,
    waiting: Vec<usize>,
    added: HashSet<String>,
    sequence: u64,
    reasoning_id: Option<String>,
    terminal: bool,
}
impl HistoryStream {
    pub(crate) fn new(
        native: ProviderStream,
        scope: Value,
        request: CanonicalRequest,
        tools: Option<ToolMap>,
        context: RequestContext,
        limits: Limits,
    ) -> Self {
        Self {
            native: Some(native),
            scope,
            request,
            tools,
            context,
            remaining: limits.request_bytes.min(limits.response_bytes),
            limits,
            chunks: Vec::new(),
            pending: VecDeque::new(),
            pending_bytes: 0,
            items: HashMap::new(),
            waiting: Vec::new(),
            added: HashSet::new(),
            sequence: 0,
            reasoning_id: None,
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
            return Err(ProviderError::new(502, "qwen_history_too_large"));
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
    fn content(&mut self, chunk: usize) -> ProviderResult<bool> {
        let mut wire = self.chunks[chunk].clone();
        let source = wire["output_index"].as_u64().ok_or_else(invalid)?;
        let item = self.items.get(&source).ok_or_else(invalid)?;
        let reasoning = item.kind == "reasoning";
        let mut index = 1u64;
        let mut summary_index = 0u64;
        for previous in 0..source {
            let Some(previous) = self.items.get(&previous) else {
                return Ok(false);
            };
            if previous.kind == "reasoning" {
                summary_index = summary_index.checked_add(1).ok_or_else(invalid)?;
            } else {
                index = index.checked_add(1).ok_or_else(invalid)?;
            }
        }
        if reasoning {
            let kind = wire["type"].as_str().ok_or_else(invalid)?.to_owned();
            wire["type"] = match kind.as_str() {
                "response.output_item.added" => "response.reasoning_summary_part.added",
                "response.reasoning_text.delta" => "response.reasoning_summary_text.delta",
                "response.reasoning_text.done" => "response.reasoning_summary_text.done",
                _ => return Err(invalid()),
            }
            .into();
            if kind == "response.output_item.added" {
                wire["part"] =
                    json!({"type":"summary_text","text":history::summary(&wire["item"])?});
                wire.as_object_mut().unwrap().remove("item");
            }
            wire["output_index"] = 0.into();
            wire["summary_index"] = summary_index.into();
            wire["item_id"] = self
                .reasoning_id
                .as_ref()
                .ok_or_else(invalid)?
                .clone()
                .into();
            wire.as_object_mut().unwrap().remove("content_index");
        } else {
            wire["output_index"] = index.into();
            if wire["type"] == "response.output_item.added" {
                self.added
                    .insert(item.id.as_str().ok_or_else(invalid)?.to_owned());
            }
        }
        self.emit(wire)?;
        Ok(true)
    }
    fn flush(&mut self) -> ProviderResult<()> {
        for chunk in std::mem::take(&mut self.waiting) {
            if !self.content(chunk)? {
                self.waiting.push(chunk);
            }
        }
        Ok(())
    }
    fn push(&mut self, event: StreamEvent) -> ProviderResult<()> {
        crate::output(event.response.wire(), self.tools.is_some())?;
        self.remaining = self
            .remaining
            .checked_sub(event.frame.data.len())
            .ok_or_else(|| ProviderError::new(502, "qwen_history_too_large"))?;
        let mut wire = event.response.wire().clone();
        self.chunks.push(wire.clone());
        match event.response.kind() {
            "response.created" => {
                if self.reasoning_id.is_some()
                    || wire["response"]["output"]
                        .as_array()
                        .is_none_or(|v| !v.is_empty())
                {
                    return Err(invalid());
                }
                let id = wire["response"]["id"].as_str().ok_or_else(invalid)?;
                let id = format!("rs_{id}_native");
                self.reasoning_id = Some(id.clone());
                self.emit(wire)?;
                self.emit(json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":id,"summary":[]}}))?;
            }
            "response.in_progress" => {
                wire["response"]["output"] = json!([]);
                self.emit(wire)?;
            }
            "response.output_item.added" => {
                let index = wire["output_index"].as_u64().ok_or_else(invalid)?;
                let kind = wire["item"]["type"].as_str().ok_or_else(invalid)?;
                if self.reasoning_id.is_none()
                    || matches!(kind, "reasoning" | "message" | "function_call")
                        && !history::valid_id(&wire["item"]["id"])
                    || kind != "reasoning"
                        && wire["item"]["id"].as_str() == self.reasoning_id.as_deref()
                    || self
                        .items
                        .values()
                        .any(|i| i.id.is_string() && i.id == wire["item"]["id"])
                    || self
                        .items
                        .insert(
                            index,
                            NativeItem {
                                kind: kind.into(),
                                id: wire["item"]["id"].clone(),
                                done: false,
                            },
                        )
                        .is_some()
                {
                    return Err(invalid());
                }
                if kind == "reasoning" || kind == "message" {
                    self.waiting.push(self.chunks.len() - 1);
                }
                self.flush()?;
            }
            "response.reasoning_text.delta"
            | "response.reasoning_text.done"
            | "response.output_text.delta"
            | "response.output_text.done"
            | "response.content_part.added"
            | "response.content_part.done" => {
                let index = wire["output_index"].as_u64().ok_or_else(invalid)?;
                let item = self.items.get(&index).ok_or_else(invalid)?;
                let reasoning = event
                    .response
                    .kind()
                    .starts_with("response.reasoning_text.");
                if item.done
                    || wire["item_id"] != item.id
                    || item.kind != if reasoning { "reasoning" } else { "message" }
                    || !reasoning && wire["content_index"].as_u64().is_none()
                    || reasoning && wire.get("content_index").is_some_and(|v| v != 0)
                    || event.response.kind().ends_with(".delta") && !wire["delta"].is_string()
                    || event.response.kind().ends_with("text.done") && !wire["text"].is_string()
                {
                    return Err(invalid());
                }
                if !self.content(self.chunks.len() - 1)? {
                    self.waiting.push(self.chunks.len() - 1);
                }
            }
            "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
                let index = wire["output_index"].as_u64().ok_or_else(invalid)?;
                let item = self.items.get(&index).ok_or_else(invalid)?;
                if self.tools.is_none()
                    || item.done
                    || item.kind != "function_call"
                    || wire["item_id"] != item.id
                    || wire.get("content_index").is_some()
                    || event.response.kind().ends_with(".delta") && !wire["delta"].is_string()
                    || event.response.kind().ends_with(".done") && !wire["arguments"].is_string()
                {
                    return Err(invalid());
                }
                // Raw tool events are retained but withheld until validated completion.
            }
            "response.output_item.done" => {
                let index = wire["output_index"].as_u64().ok_or_else(invalid)?;
                let item = self.items.get_mut(&index).ok_or_else(invalid)?;
                if item.done || wire["item"]["id"] != item.id || wire["item"]["type"] != item.kind {
                    return Err(invalid());
                }
                item.done = true;
            }
            _ if event.response.terminal().is_some() => {
                if let Some(tools) = &self.tools
                    && event.response.kind() != "error"
                {
                    let response =
                        CanonicalResponse::new(wire["response"].clone()).map_err(|_| invalid())?;
                    tools.validate_response(&self.request, &response)?;
                }
                if event.response.terminal() != Some(StreamState::Completed) {
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
                        self.tools.as_ref(),
                        Some(&self.chunks),
                        budget,
                    )
                    .and_then(|h| h.to_responses(budget))
                    .map_err(history::native_error)?;
                    for (index, item) in response.output().iter().enumerate() {
                        self.items
                            .entry(index as u64)
                            .or_insert_with(|| NativeItem {
                                kind: item["type"].as_str().unwrap().into(),
                                id: item["id"].clone(),
                                done: true,
                            });
                    }
                    self.flush()?;
                    if !self.waiting.is_empty() {
                        return Err(invalid());
                    }
                    for (index, item) in projected.output().iter().enumerate() {
                        if item["type"] != "reasoning"
                            && !item["id"]
                                .as_str()
                                .is_some_and(|id| self.added.contains(id))
                        {
                            self.emit(json!({"type":"response.output_item.added","output_index":index,"item":item}))?;
                        }
                        self.emit(json!({"type":"response.output_item.done","output_index":index,"item":item}))?;
                    }
                    wire["response"] = projected.wire().clone();
                    self.emit(wire)?;
                }
                self.terminal = true;
                self.native.take();
            }
            kind if kind.starts_with("response.function_call_arguments.")
                || kind.starts_with("response.reasoning_")
                || kind.starts_with("response.content_part.")
                || kind.starts_with("response.output_text.") =>
            {
                return Err(invalid());
            }
            _ => {
                // Unknown indexed extensions stay in the native capsule; do not
                // pretend their undocumented indices are canonical indices.
                if wire.get("output_index").is_none() && wire.get("item_id").is_none() {
                    self.emit(wire)?;
                }
            }
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
        self.waiting.clear();
        self.items.clear();
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
