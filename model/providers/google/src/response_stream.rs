use crate::{NativeHistory, NativeStreamEvent, ToolMap};
use caidex_model_core::{ProviderError, ProviderResult, ResponseEvent, SseEvent, StreamEvent};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fmt};

fn invalid() -> ProviderError {
    ProviderError::new(502, "google_invalid_projection_stream")
}
/// Text/summary progress over validated native chunks. Only clean native EOF
/// releases complete signed history and executable calls. Parts stay untouched.
pub struct ResponsesProjection {
    model: String,
    request: Value,
    tools: ToolMap,
    id: String,
    max_bytes: usize,
    tool_call_limit: Option<usize>,
    remaining: usize,
    chunks: Vec<Value>,
    started: bool,
    deferred: bool,
    part_index: usize,
    summary_index: usize,
    output_index: usize,
    added: BTreeSet<String>,
    sequence: u64,
    terminal: bool,
}
impl ResponsesProjection {
    pub fn new(
        model: String,
        request: Value,
        tools: ToolMap,
        id: String,
        max_bytes: usize,
    ) -> ProviderResult<Self> {
        let native_tools = crate::content::present(&request, "tools");
        if max_bytes == 0
            || id.trim().is_empty()
            || id.chars().any(char::is_control)
            || id.len() > max_bytes
            || request.to_string().len() > max_bytes
            || crate::client::validate_generation_request(&model, &request, true)? != 1
            || !(tools.native_tools().is_empty() && native_tools.is_none_or(|v| v == &json!([])))
                && native_tools != Some(&json!([{"functionDeclarations":tools.native_tools()}]))
        {
            return Err(invalid());
        }
        Ok(Self {
            model,
            request,
            tools,
            id,
            max_bytes,
            tool_call_limit: None,
            remaining: max_bytes,
            chunks: Vec::new(),
            started: false,
            deferred: false,
            part_index: 0,
            summary_index: 0,
            output_index: 1,
            added: BTreeSet::new(),
            sequence: 0,
            terminal: false,
        })
    }
    pub(crate) fn with_tool_call_limit(mut self, limit: Option<usize>) -> Self {
        self.tool_call_limit = limit;
        self
    }
    pub fn push(&mut self, native: NativeStreamEvent) -> ProviderResult<Vec<StreamEvent>> {
        if self.terminal {
            return Err(invalid());
        }
        let result = self.push_inner(native);
        if result.is_err() {
            self.terminal = true;
        }
        result
    }
    fn push_inner(&mut self, native: NativeStreamEvent) -> ProviderResult<Vec<StreamEvent>> {
        let mut events = Vec::new();
        if !self.started {
            self.emit(json!({"type":"response.created","response":{"id":self.id,"object":"response","model":self.model,"status":"in_progress","output":[]}}), &mut events)?;
            self.emit(json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":self.reasoning_id(),"summary":[]}}), &mut events)?;
            self.started = true;
        }
        match native {
            NativeStreamEvent::Event(event) => {
                self.remaining = self
                    .remaining
                    .checked_sub(event.frame().data.len())
                    .ok_or_else(invalid)?;
                let wire = event.wire();
                for candidate in wire["candidates"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    if candidate["index"].as_u64().unwrap_or(0) != 0 {
                        return Err(invalid());
                    }
                    for part in candidate["content"]["parts"]
                        .as_array()
                        .map(Vec::as_slice)
                        .unwrap_or(&[])
                    {
                        if part["thought"] == true {
                            if let Some(text) = part["text"].as_str() {
                                self.emit(json!({"type":"response.reasoning_summary_part.added","item_id":self.reasoning_id(),"output_index":0,"summary_index":self.summary_index,"part":{"type":"summary_text","text":""}}), &mut events)?;
                                self.emit(json!({"type":"response.reasoning_summary_text.delta","item_id":self.reasoning_id(),"output_index":0,"summary_index":self.summary_index,"delta":text}), &mut events)?;
                                self.summary_index += 1;
                            }
                        } else if let Some(text) = part["text"].as_str() {
                            if !self.deferred {
                                let id = format!("msg_{}_0_{}", self.id, self.part_index);
                                let item = json!({"type":"message","id":id,"role":"assistant","phase":"commentary","content":[{"type":"output_text","text":text}]});
                                self.add_message(&item, self.output_index, &mut events)?;
                                self.output_index += 1;
                            }
                        } else if crate::content::present(part, "functionCall").is_some() {
                            // The final finishReason can suppress calls. Defer
                            // later texts too, so output indices never shift.
                            self.deferred = true;
                        }
                        self.part_index += 1;
                    }
                }
                self.chunks.push(wire.clone());
            }
            NativeStreamEvent::Completed(native) => {
                if self.chunks != native.chunks() {
                    return Err(invalid());
                }
                crate::provider::validate_tool_call_limit(native.response(), self.tool_call_limit)?;
                let selected = native.response().blocked_prompt().is_none().then_some(0);
                let response = NativeHistory::from_stream(
                    &native,
                    &self.model,
                    &self.request,
                    selected,
                    &self.id,
                    self.max_bytes,
                )?
                .with_tools(&self.tools, self.max_bytes)?
                .to_responses(self.max_bytes)?;
                if response.wire().to_string().len() > self.max_bytes {
                    return Err(invalid());
                }
                for (index, item) in response.output().iter().enumerate() {
                    match item["type"].as_str() {
                        Some("reasoning") => {
                            for (summary, part) in
                                item["summary"].as_array().unwrap().iter().enumerate()
                            {
                                self.emit(json!({"type":"response.reasoning_summary_text.done","item_id":item["id"],"output_index":index,"summary_index":summary,"text":part["text"]}), &mut events)?;
                                self.emit(json!({"type":"response.reasoning_summary_part.done","item_id":item["id"],"output_index":index,"summary_index":summary,"part":part}), &mut events)?;
                            }
                        }
                        Some("message") => {
                            if !self.added.contains(item["id"].as_str().unwrap()) {
                                self.add_message(item, index, &mut events)?;
                            }
                            let part = &item["content"][0];
                            self.emit(json!({"type":"response.output_text.done","item_id":item["id"],"output_index":index,"content_index":0,"text":part["text"]}), &mut events)?;
                            self.emit(json!({"type":"response.content_part.done","item_id":item["id"],"output_index":index,"content_index":0,"part":part}), &mut events)?;
                        }
                        Some("function_call" | "custom_tool_call") => {
                            let custom = item["type"] == "custom_tool_call";
                            let field = if custom { "input" } else { "arguments" };
                            let mut start = item.clone();
                            start[field] = "".into();
                            start["status"] = "in_progress".into();
                            self.emit(json!({"type":"response.output_item.added","output_index":index,"item":start}), &mut events)?;
                            let base = if custom {
                                "response.custom_tool_call_input"
                            } else {
                                "response.function_call_arguments"
                            };
                            self.emit(json!({"type":format!("{base}.delta"),"item_id":item["id"],"output_index":index,"delta":item[field]}), &mut events)?;
                            let mut done = json!({"type":format!("{base}.done"),"item_id":item["id"],"output_index":index});
                            done[field] = item[field].clone();
                            self.emit(done, &mut events)?;
                        }
                        _ => return Err(invalid()),
                    }
                    self.emit(json!({"type":"response.output_item.done","output_index":index,"item":item}), &mut events)?;
                }
                let kind = match response.wire()["status"].as_str() {
                    Some("completed") => "response.completed",
                    Some("incomplete") => "response.incomplete",
                    _ => "response.failed",
                };
                self.emit(json!({"type":kind,"response":response.wire()}), &mut events)?;
                self.terminal = true;
            }
        }
        Ok(events)
    }
    fn reasoning_id(&self) -> String {
        format!("rs_{}_native", self.id)
    }
    fn add_message(
        &mut self,
        item: &Value,
        index: usize,
        events: &mut Vec<StreamEvent>,
    ) -> ProviderResult<()> {
        let id = item["id"].as_str().ok_or_else(invalid)?;
        let mut start = item.clone();
        start["content"] = json!([]);
        self.emit(
            json!({"type":"response.output_item.added","output_index":index,"item":start}),
            events,
        )?;
        self.emit(json!({"type":"response.content_part.added","item_id":id,"output_index":index,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}), events)?;
        self.emit(json!({"type":"response.output_text.delta","item_id":id,"output_index":index,"content_index":0,"delta":item["content"][0]["text"]}), events)?;
        self.added.insert(id.into());
        Ok(())
    }
    fn emit(&mut self, mut wire: Value, events: &mut Vec<StreamEvent>) -> ProviderResult<()> {
        wire["sequence_number"] = self.sequence.into();
        self.sequence = self.sequence.checked_add(1).ok_or_else(invalid)?;
        let response = ResponseEvent::new(wire.clone()).map_err(|_| invalid())?;
        let data = wire.to_string();
        if data.len() > self.max_bytes {
            return Err(invalid());
        }
        events.push(StreamEvent {
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
}
impl fmt::Debug for ResponsesProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResponsesProjection([WIRE OMITTED])")
    }
}

/// Pull-based view keeps the existing native worker/slot/deadline. Drop or
/// projection failure aborts native transport; no second inference is created.
pub struct ProjectedStreamingResponse {
    native: Option<crate::NativeStreamingResponse>,
    projection: ResponsesProjection,
    pending: std::collections::VecDeque<StreamEvent>,
    terminal: bool,
}
impl ProjectedStreamingResponse {
    pub fn new(native: crate::NativeStreamingResponse, projection: ResponsesProjection) -> Self {
        Self {
            native: Some(native),
            projection,
            pending: Default::default(),
            terminal: false,
        }
    }
}
impl futures_util::Stream for ProjectedStreamingResponse {
    type Item = ProviderResult<caidex_model_core::ProviderStreamEvent>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        if let Some(event) = self.pending.pop_front() {
            return Poll::Ready(Some(Ok(caidex_model_core::ProviderStreamEvent::Model(
                event,
            ))));
        }
        if self.terminal {
            return Poll::Ready(None);
        }
        let Some(native) = self.native.as_mut() else {
            return Poll::Ready(None);
        };
        match std::pin::Pin::new(native).poll_next(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(native))) => match self.projection.push(native) {
                Ok(events) => {
                    if self.projection.terminal {
                        self.terminal = true;
                        self.native.take();
                    }
                    self.pending.extend(events);
                    Poll::Ready(Some(Ok(self
                        .pending
                        .pop_front()
                        .map(caidex_model_core::ProviderStreamEvent::Model)
                        .unwrap_or(caidex_model_core::ProviderStreamEvent::Heartbeat))))
                }
                Err(error) => {
                    self.terminal = true;
                    self.native.take();
                    Poll::Ready(Some(Err(error)))
                }
            },
            Poll::Ready(result) => {
                self.terminal = true;
                self.native.take();
                Poll::Ready(Some(Err(result
                    .and_then(Result::err)
                    .unwrap_or_else(invalid))))
            }
        }
    }
}
