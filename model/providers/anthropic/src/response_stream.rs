use crate::{NativeStreamEvent, ToolMap};
use caidex_model_core::{ProviderError, ProviderResult, ResponseEvent, SseEvent, StreamEvent};
use serde_json::{Value, json};
use std::fmt;

fn invalid() -> ProviderError {
    ProviderError::new(502, "anthropic_invalid_projection_stream")
}
struct Block {
    item: Option<Value>,
    output_index: usize,
    summary_index: Option<usize>,
    text: String,
    arguments: String,
    stopped: bool,
    native: Option<Value>,
}
/// Incremental Responses view over validated native events. Signed history and
/// executable output_item.done are released only with the complete native reply.
/// No network worker or second inference request is created here.
pub struct ResponsesProjection {
    model: String,
    tools: ToolMap,
    max_bytes: usize,
    remaining: usize,
    id: Option<String>,
    blocks: Vec<Block>,
    output_count: usize,
    summary_count: usize,
    sequence: u64,
    stopped: bool,
    terminal: bool,
}
impl ResponsesProjection {
    pub fn new(model: String, tools: ToolMap, max_bytes: usize) -> ProviderResult<Self> {
        if model.trim().is_empty()
            || max_bytes == 0
            || model.len() > max_bytes
            || serde_json::to_string(tools.source())
                .map_err(|_| invalid())?
                .len()
                > max_bytes
        {
            return Err(invalid());
        }
        Ok(Self {
            model,
            tools,
            max_bytes,
            remaining: max_bytes,
            id: None,
            blocks: Vec::new(),
            output_count: 1,
            summary_count: 0,
            sequence: 0,
            stopped: false,
            terminal: false,
        })
    }
    pub fn push(&mut self, native: NativeStreamEvent) -> ProviderResult<Vec<StreamEvent>> {
        if self.terminal {
            return Err(invalid());
        }
        let result = self.push_inner(native).and_then(|events| {
            events
                .into_iter()
                .map(|event| {
                    let mut wire = event.response.wire().clone();
                    wire["sequence_number"] = self.sequence.into();
                    self.sequence = self.sequence.checked_add(1).ok_or_else(invalid)?;
                    let event = event_with_wire(wire)?;
                    if event.frame.data.len() > self.max_bytes {
                        return Err(invalid());
                    }
                    Ok(event)
                })
                .collect()
        });
        if result.is_err() {
            self.terminal = true;
        }
        result
    }
    fn push_inner(&mut self, native: NativeStreamEvent) -> ProviderResult<Vec<StreamEvent>> {
        let mut output = Vec::new();
        match native {
            NativeStreamEvent::Event(event) => {
                self.remaining = self
                    .remaining
                    .checked_sub(event.frame().data.len())
                    .ok_or_else(invalid)?;
                let wire = event.wire();
                match event.kind() {
                    "message_start" => {
                        if self.id.is_some() || wire["message"]["model"] != self.model {
                            return Err(invalid());
                        }
                        crate::message::check_input_bindings(&wire["message"])?;
                        let id = wire["message"]["id"]
                            .as_str()
                            .ok_or_else(invalid)?
                            .to_owned();
                        self.id = Some(id.clone());
                        self.emit(json!({"type":"response.created","response":{"id":id,"object":"response","model":self.model,"status":"in_progress","output":[]}}),&mut output)?;
                        self.emit(json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":self.reasoning_id(),"summary":[]}}),&mut output)?;
                    }
                    "content_block_start" => {
                        if self.stopped || self.id.is_none() || index(wire)? != self.blocks.len() {
                            return Err(invalid());
                        }
                        let native = &wire["content_block"];
                        let mut block = Block {
                            item: None,
                            output_index: self.output_count,
                            summary_index: None,
                            text: String::new(),
                            arguments: String::new(),
                            stopped: false,
                            native: None,
                        };
                        match native["type"].as_str() {
                            Some("text") => {
                                let id = format!(
                                    "msg_{}_{}",
                                    self.id.as_ref().unwrap(),
                                    self.blocks.len()
                                );
                                block.item = Some(
                                    json!({"type":"message","id":id,"role":"assistant","phase":"commentary","content":[]}),
                                );
                                self.emit(json!({"type":"response.output_item.added","output_index":block.output_index,"item":block.item}),&mut output)?;
                                self.emit(json!({"type":"response.content_part.added","item_id":id,"output_index":block.output_index,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),&mut output)?;
                                self.output_count += 1;
                                block.text =
                                    native["text"].as_str().ok_or_else(invalid)?.to_owned();
                            }
                            Some("tool_use") => {
                                let mut item = self.tools.responses_call_start(native)?;
                                item["id"] = format!(
                                    "fc_{}_{}",
                                    self.id.as_ref().unwrap(),
                                    self.blocks.len()
                                )
                                .into();
                                self.emit(json!({"type":"response.output_item.added","output_index":block.output_index,"item":item}),&mut output)?;
                                block.item = Some(item);
                                self.output_count += 1;
                                if native["input"].as_object().is_some_and(|v| !v.is_empty())
                                    && block.item.as_ref().unwrap()["type"] == "function_call"
                                {
                                    block.arguments = native["input"].to_string();
                                }
                            }
                            Some("thinking") => {
                                block.summary_index = Some(self.summary_count);
                                self.summary_count += 1;
                                self.emit(json!({"type":"response.reasoning_summary_part.added","item_id":self.reasoning_id(),"output_index":0,"summary_index":block.summary_index,"part":{"type":"summary_text","text":""}}),&mut output)?;
                                block.text =
                                    native["thinking"].as_str().ok_or_else(invalid)?.to_owned();
                            }
                            _ => (), // server tools, redactions and future blocks stay opaque
                        }
                        if !block.text.is_empty() {
                            self.text_delta(&block, &block.text, &mut output)?;
                        }
                        if !block.arguments.is_empty() {
                            self.argument_delta(&block, &block.arguments, &mut output)?;
                        }
                        self.blocks.push(block);
                    }
                    "content_block_delta" => {
                        if self.stopped {
                            return Err(invalid());
                        }
                        let i = index(wire)?;
                        let block = self
                            .blocks
                            .get(i)
                            .filter(|b| !b.stopped)
                            .ok_or_else(invalid)?;
                        let delta = &wire["delta"];
                        match delta["type"].as_str() {
                            Some("text_delta" | "thinking_delta") => {
                                let field = if delta["type"] == "text_delta" {
                                    "text"
                                } else {
                                    "thinking"
                                };
                                let text = delta[field].as_str().ok_or_else(invalid)?;
                                self.text_delta(block, text, &mut output)?;
                                self.blocks[i].text.push_str(text);
                            }
                            Some("input_json_delta")
                                if block
                                    .item
                                    .as_ref()
                                    .is_some_and(|item| item["type"] == "function_call") =>
                            {
                                let partial = delta["partial_json"].as_str().ok_or_else(invalid)?;
                                self.argument_delta(block, partial, &mut output)?;
                                self.blocks[i].arguments.push_str(partial);
                            }
                            _ => (), // signatures and native metadata are never display text
                        }
                    }
                    "content_block_stop" => {
                        let i = index(wire)?;
                        let native = event.stopped_block().ok_or_else(invalid)?;
                        let block = self
                            .blocks
                            .get_mut(i)
                            .filter(|b| !b.stopped)
                            .ok_or_else(invalid)?;
                        if let Some(item) = &block.item {
                            if item["type"] == "message" && native["text"] != block.text {
                                return Err(invalid());
                            }
                            if item["type"] == "function_call" || item["type"] == "custom_tool_call"
                            {
                                let mut complete =
                                    self.tools.responses_call(native)?.wire().clone();
                                complete["id"] = item["id"].clone();
                                if complete["type"] == "function_call" {
                                    if block.arguments.is_empty() {
                                        block.arguments = native["input"].to_string();
                                    }
                                    let parsed: Value = serde_json::from_str(&block.arguments)
                                        .map_err(|_| invalid())?;
                                    if parsed != native["input"] {
                                        return Err(invalid());
                                    }
                                    complete["arguments"] = block.arguments.clone().into();
                                }
                                block.item = Some(complete);
                            }
                        }
                        if block.summary_index.is_some() && native["thinking"] != block.text {
                            return Err(invalid());
                        }
                        block.native = Some(native.clone());
                        block.stopped = true;
                        let block = &self.blocks[i];
                        if let Some(item) = &block.item
                            && item["type"] == "custom_tool_call"
                        {
                            self.emit(json!({"type":"response.custom_tool_call_input.delta","item_id":item["id"],"output_index":block.output_index,"delta":item["input"]}),&mut output)?;
                        }
                    }
                    "message_delta" => crate::message::check_input_bindings(wire)?,
                    "message_stop" => {
                        if self.id.is_none()
                            || self.stopped
                            || self.blocks.iter().any(|b| !b.stopped)
                        {
                            return Err(invalid());
                        }
                        self.stopped = true;
                    }
                    "error" => return Err(ProviderError::new(502, "provider_stream_error")),
                    _ => (),
                }
            }
            NativeStreamEvent::Completed(message) => {
                if !self.stopped
                    || self.id.as_deref() != Some(message.id())
                    || message.model() != self.model
                {
                    return Err(invalid());
                }
                if message.content().len() != self.blocks.len()
                    || self
                        .blocks
                        .iter()
                        .zip(message.content())
                        .any(|(block, native)| block.native.as_ref() != Some(native))
                {
                    return Err(invalid());
                }
                let mut response = message
                    .to_responses_with_tools(&self.tools, self.max_bytes)?
                    .wire()
                    .clone();
                if response["output"].as_array().unwrap().len() != self.output_count {
                    return Err(invalid());
                }
                for block in &self.blocks {
                    if let Some(item) = &block.item {
                        let projected = &mut response["output"][block.output_index];
                        if projected["id"] != item["id"] || projected["type"] != item["type"] {
                            return Err(invalid());
                        }
                        if item["type"] == "function_call" {
                            projected["arguments"] = item["arguments"].clone();
                        }
                    }
                }
                // Revalidate the entire projection against its signed native carrier.
                crate::NativeMessage::from_responses_output(
                    response["output"].as_array().unwrap(),
                    &self.model,
                    self.max_bytes,
                )?;
                for (i, item) in response["output"].as_array().unwrap().iter().enumerate() {
                    match item["type"].as_str() {
                        Some("reasoning") => {
                            for (summary_index, part) in item["summary"].as_array().unwrap().iter().enumerate() {
                                self.emit(json!({"type":"response.reasoning_summary_text.done","item_id":item["id"],"output_index":i,"summary_index":summary_index,"text":part["text"]}), &mut output)?;
                                self.emit(json!({"type":"response.reasoning_summary_part.done","item_id":item["id"],"output_index":i,"summary_index":summary_index,"part":part}), &mut output)?;
                            }
                        }
                        Some("message") => {
                            let part = &item["content"][0];
                            self.emit(json!({"type":"response.output_text.done","item_id":item["id"],"output_index":i,"content_index":0,"text":part["text"]}), &mut output)?;
                            self.emit(json!({"type":"response.content_part.done","item_id":item["id"],"output_index":i,"content_index":0,"part":part}), &mut output)?;
                        }
                        Some("function_call") => self.emit(json!({"type":"response.function_call_arguments.done","item_id":item["id"],"output_index":i,"arguments":item["arguments"]}), &mut output)?,
                        Some("custom_tool_call") => self.emit(json!({"type":"response.custom_tool_call_input.done","item_id":item["id"],"output_index":i,"input":item["input"]}), &mut output)?,
                        _ => return Err(invalid()),
                    }
                    self.emit(
                        json!({"type":"response.output_item.done","output_index":i,"item":item}),
                        &mut output,
                    )?;
                }
                let kind = if response["status"] == "completed" {
                    "response.completed"
                } else {
                    "response.incomplete"
                };
                self.emit(json!({"type":kind,"response":response}), &mut output)?;
                self.terminal = true;
            }
        }
        Ok(output)
    }
    fn text_delta(
        &self,
        block: &Block,
        text: &str,
        output: &mut Vec<StreamEvent>,
    ) -> ProviderResult<()> {
        let wire = if let Some(summary) = block.summary_index {
            json!({"type":"response.reasoning_summary_text.delta","item_id":self.reasoning_id(),"output_index":0,"summary_index":summary,"delta":text})
        } else if let Some(item) = &block.item {
            json!({"type":"response.output_text.delta","item_id":item["id"],"output_index":block.output_index,"content_index":0,"delta":text})
        } else {
            return Err(invalid());
        };
        output.push(event_with_wire(wire)?);
        Ok(())
    }
    fn argument_delta(
        &self,
        block: &Block,
        text: &str,
        output: &mut Vec<StreamEvent>,
    ) -> ProviderResult<()> {
        let item = block.item.as_ref().ok_or_else(invalid)?;
        output.push(event_with_wire(json!({"type":"response.function_call_arguments.delta","item_id":item["id"],"output_index":block.output_index,"delta":text}))?);
        Ok(())
    }
    fn reasoning_id(&self) -> String {
        format!("rs_{}_native", self.id.as_ref().unwrap())
    }
    fn emit(&self, wire: Value, output: &mut Vec<StreamEvent>) -> ProviderResult<()> {
        output.push(event_with_wire(wire)?);
        Ok(())
    }
}
fn index(wire: &Value) -> ProviderResult<usize> {
    wire["index"]
        .as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .ok_or_else(invalid)
}
fn event_with_wire(wire: Value) -> ProviderResult<StreamEvent> {
    let response = ResponseEvent::new(wire.clone()).map_err(|_| invalid())?;
    Ok(StreamEvent {
        frame: SseEvent {
            event: response.kind().into(),
            data: wire.to_string(),
            id: String::new(),
            retry_ms: None,
        },
        response,
    })
}
impl fmt::Debug for ResponsesProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResponsesProjection([WIRE OMITTED])")
    }
}

/// Pull-based view retains the native worker's single delivery slot and deadline.
/// Dropping or failing this stream immediately drops/aborts native transport.
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
        // Bound work per poll when native metadata events produce no model frame.
        for _ in 0..32 {
            let Some(native) = self.native.as_mut() else {
                return Poll::Ready(None);
            };
            match std::pin::Pin::new(native).poll_next(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(native))) => {
                    let heartbeat =
                        matches!(&native, NativeStreamEvent::Event(event) if event.kind()=="ping");
                    match self.projection.push(native) {
                        Ok(events) => {
                            if self.projection.terminal {
                                self.terminal = true;
                                self.native.take();
                            }
                            self.pending.extend(events);
                            if let Some(event) = self.pending.pop_front() {
                                return Poll::Ready(Some(Ok(
                                    caidex_model_core::ProviderStreamEvent::Model(event),
                                )));
                            }
                            if heartbeat {
                                return Poll::Ready(Some(Ok(
                                    caidex_model_core::ProviderStreamEvent::Heartbeat,
                                )));
                            }
                        }
                        Err(error) => {
                            self.terminal = true;
                            self.native.take();
                            return Poll::Ready(Some(Err(error)));
                        }
                    }
                }
                Poll::Ready(result) => {
                    self.terminal = true;
                    self.native.take();
                    return Poll::Ready(Some(Err(result
                        .and_then(Result::err)
                        .unwrap_or_else(invalid))));
                }
            }
        }
        context.waker().wake_by_ref();
        Poll::Pending
    }
}
