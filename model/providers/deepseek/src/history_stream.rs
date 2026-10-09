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

struct NativeItem {
    kind: String,
    id: Value,
    parts: Option<u64>,
}

/// Incremental text uses the existing native I/O. Tool calls are delivered
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
    items: HashMap<u64, NativeItem>,
    pending_content: Vec<usize>,
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
            items: HashMap::new(),
            pending_content: Vec::new(),
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
    fn emit_content(&mut self, chunk: usize) -> ProviderResult<bool> {
        let mut wire = self.chunks[chunk].clone();
        let source = wire["output_index"].as_u64().ok_or_else(invalid)?;
        let item = self.items.get(&source).ok_or_else(invalid)?;
        let reasoning = item.kind == "reasoning";
        let mut output_index = 1u64;
        let mut summary_index = wire["content_index"].as_u64().unwrap_or(0);
        for index in 0..source {
            let Some(previous) = self.items.get(&index) else {
                return Ok(false);
            };
            if previous.kind == "reasoning" {
                if reasoning {
                    let Some(parts) = previous.parts else {
                        return Ok(false);
                    };
                    summary_index = summary_index.checked_add(parts).ok_or_else(invalid)?;
                }
            } else {
                output_index = output_index.checked_add(1).ok_or_else(invalid)?;
            }
        }
        if reasoning {
            wire["type"] = match wire["type"].as_str() {
                Some("response.content_part.added") => "response.reasoning_summary_part.added",
                Some("response.content_part.done") => "response.reasoning_summary_part.done",
                Some("response.reasoning_text.delta") => "response.reasoning_summary_text.delta",
                Some("response.reasoning_text.done") => "response.reasoning_summary_text.done",
                _ => return Err(invalid()),
            }
            .into();
            wire["output_index"] = 0.into();
            wire["item_id"] = self
                .reasoning_id
                .as_ref()
                .ok_or_else(invalid)?
                .clone()
                .into();
            wire["summary_index"] = summary_index.into();
            wire.as_object_mut().unwrap().remove("content_index");
            if wire.get("part").is_some() {
                wire["part"]["type"] = "summary_text".into();
            }
        } else {
            wire["output_index"] = output_index.into();
            if wire["type"] == "response.output_item.added" {
                self.added
                    .insert(item.id.as_str().ok_or_else(invalid)?.to_owned());
            }
        }
        self.emit(wire)?;
        Ok(true)
    }
    fn flush_content(&mut self) -> ProviderResult<()> {
        // Reuse bounded native chunks; later reasoning waits for prior part counts.
        for chunk in std::mem::take(&mut self.pending_content) {
            if !self.emit_content(chunk)? {
                self.pending_content.push(chunk);
            }
        }
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
            "response.output_item.added" => {
                let source = wire["output_index"].as_u64().ok_or_else(invalid)?;
                let kind = wire["item"]["type"].as_str().ok_or_else(invalid)?;
                if kind.ends_with("_call") && !matches!(kind, "function_call" | "custom_tool_call")
                {
                    return Err(invalid());
                }
                if self
                    .items
                    .insert(
                        source,
                        NativeItem {
                            kind: kind.into(),
                            id: wire["item"]["id"].clone(),
                            parts: None,
                        },
                    )
                    .is_some()
                {
                    return Err(invalid());
                }
                if kind == "message" {
                    self.pending_content.push(self.chunks.len() - 1);
                }
                self.flush_content()?;
            }
            "response.output_text.delta"
            | "response.output_text.done"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.reasoning_text.delta"
            | "response.reasoning_text.done" => {
                let source = wire["output_index"].as_u64().ok_or_else(invalid)?;
                let item = self.items.get(&source).ok_or_else(invalid)?;
                if item.parts.is_some()
                    || wire["item_id"] != item.id
                    || wire["content_index"].as_u64().is_none()
                    || !matches!(item.kind.as_str(), "message" | "reasoning")
                    || event.response.kind().starts_with("response.reasoning_text")
                        && item.kind != "reasoning"
                    || event.response.kind().starts_with("response.output_text")
                        && item.kind != "message"
                {
                    return Err(invalid());
                }
                if event.response.kind().starts_with("response.content_part")
                    && (wire["part"]["type"]
                        != if item.kind == "reasoning" {
                            "reasoning_text"
                        } else {
                            "output_text"
                        }
                        || !wire["part"]["text"].is_string())
                    || event.response.kind().ends_with(".delta") && !wire["delta"].is_string()
                    || event.response.kind().ends_with("text.done") && !wire["text"].is_string()
                {
                    return Err(invalid());
                }
                let chunk = self.chunks.len() - 1;
                if !self.emit_content(chunk)? {
                    self.pending_content.push(chunk);
                }
            }
            "response.output_item.done" => {
                let source = wire["output_index"].as_u64().ok_or_else(invalid)?;
                let item = self.items.get_mut(&source).ok_or_else(invalid)?;
                if item.parts.is_some()
                    || wire["item"]["id"] != item.id
                    || wire["item"]["type"] != item.kind
                {
                    return Err(invalid());
                }
                item.parts = Some(if item.kind == "reasoning" || item.kind == "message" {
                    wire["item"]["content"]
                        .as_array()
                        .ok_or_else(invalid)?
                        .len() as u64
                } else {
                    0
                });
                self.flush_content()?;
            }
            "response.function_call_arguments.delta"
            | "response.function_call_arguments.done"
            | "response.custom_tool_call_input.delta"
            | "response.custom_tool_call_input.done" => (),
            kind if kind.starts_with("response.custom_tool_call")
                || kind.starts_with("response.tool_search")
                || kind.starts_with("response.reasoning_summary")
                || kind.starts_with("response.reasoning_text") =>
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
                    for (index, item) in response.output().iter().enumerate() {
                        let state = self
                            .items
                            .entry(index as u64)
                            .or_insert_with(|| NativeItem {
                                kind: item["type"].as_str().unwrap().into(),
                                id: item["id"].clone(),
                                parts: None,
                            });
                        state.parts =
                            Some(item["content"].as_array().map_or(0, |v| v.len() as u64));
                    }
                    self.flush_content()?;
                    if !self.pending_content.is_empty() {
                        return Err(invalid());
                    }
                    for (index, item) in projected.output().iter().enumerate() {
                        if item["type"] != "reasoning"
                            && !item["id"]
                                .as_str()
                                .is_some_and(|id| self.added.contains(id))
                        {
                            let mut start = item.clone();
                            if matches!(
                                item["type"].as_str(),
                                Some("function_call" | "custom_tool_call")
                            ) {
                                let field = if item["type"] == "function_call" {
                                    "arguments"
                                } else {
                                    "input"
                                };
                                start[field] = "".into();
                                start["status"] = "in_progress".into();
                            }
                            self.emit(json!({"type":"response.output_item.added", "output_index":index, "item":start}))?;
                        }
                        if item["type"] == "function_call" {
                            self.emit(json!({"type":"response.function_call_arguments.delta", "output_index":index, "item_id":item["id"], "delta":item["arguments"]}))?;
                            self.emit(json!({"type":"response.function_call_arguments.done", "output_index":index, "item_id":item["id"], "arguments":item["arguments"]}))?;
                        }
                        if item["type"] == "custom_tool_call" {
                            self.emit(json!({"type":"response.custom_tool_call_input.delta", "output_index":index, "item_id":item["id"], "delta":item["input"]}))?;
                            self.emit(json!({"type":"response.custom_tool_call_input.done", "output_index":index, "item_id":item["id"], "input":item["input"]}))?;
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
        self.pending_content.clear();
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
