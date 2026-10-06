use crate::{Error, Result, StreamState};
use serde::Serialize;
use serde_json::Value;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponsesDialect {
    Classic,
    Lite,
}
impl ResponsesDialect {
    pub fn lite_header(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Classic => None,
            Self::Lite => Some(("x-openai-internal-codex-responses-lite", "true")),
        }
    }
}

/// A canonical request keeps the complete received wire representation. Dialect
/// is transport metadata and is never injected as an extra JSON field.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct CanonicalRequest {
    wire: Value,
    #[serde(skip)]
    dialect: ResponsesDialect,
}
impl CanonicalRequest {
    pub fn new(wire: Value, dialect: ResponsesDialect) -> Result<Self> {
        let object = wire.as_object().ok_or(Error::InvalidRequest)?;
        if string(&wire, "model").is_none()
            || !matches!(wire.get("input"), Some(Value::Array(_) | Value::String(_)))
            || object
                .get("stream")
                .is_some_and(|value| !value.is_boolean())
        {
            return Err(Error::InvalidRequest);
        }
        if dialect == ResponsesDialect::Lite
            && (object.contains_key("instructions")
                || object.contains_key("tools")
                || !wire["input"].is_array())
        {
            return Err(Error::InvalidRequest);
        }
        Ok(Self { wire, dialect })
    }
    pub fn wire(&self) -> &Value {
        &self.wire
    }
    pub fn dialect(&self) -> ResponsesDialect {
        self.dialect
    }
    pub fn model(&self) -> &str {
        self.wire["model"].as_str().expect("validated model")
    }
    pub fn is_streaming(&self) -> bool {
        self.wire["stream"] == true
    }
}
impl fmt::Debug for CanonicalRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CanonicalRequest")
            .field("dialect", &self.dialect)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Function,
    Custom,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolInput<'a> {
    JsonArguments(&'a str),
    Text(&'a str),
}
/// Borrowed views do not parse/re-encode JSON arguments or freeform input.
pub struct ToolCall<'a> {
    pub kind: ToolKind,
    pub call_id: &'a str,
    pub name: &'a str,
    pub namespace: Option<&'a str>,
    pub input: ToolInput<'a>,
}
pub struct ToolResult<'a> {
    pub kind: ToolKind,
    // Pinned Runtime also supports legacy name/namespace outputs without call_id.
    pub call_id: Option<&'a str>,
    pub name: Option<&'a str>,
    pub namespace: Option<&'a str>,
    pub output: &'a Value,
}

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct ResponseItem(Value);
impl ResponseItem {
    pub fn new(wire: Value) -> Result<Self> {
        if !wire.is_object() || string(&wire, "type").is_none() {
            return Err(Error::InvalidItem);
        }
        Ok(Self(wire))
    }
    pub fn wire(&self) -> &Value {
        &self.0
    }
    pub fn kind(&self) -> &str {
        self.0["type"].as_str().expect("validated type")
    }
    pub fn tool_call(&self) -> Result<Option<ToolCall<'_>>> {
        let (kind, input) = match self.kind() {
            "function_call" => (
                ToolKind::Function,
                ToolInput::JsonArguments(self.0["arguments"].as_str().ok_or(Error::InvalidItem)?),
            ),
            "custom_tool_call" => (
                ToolKind::Custom,
                ToolInput::Text(self.0["input"].as_str().ok_or(Error::InvalidItem)?),
            ),
            _ => return Ok(None),
        };
        Ok(Some(ToolCall {
            kind,
            input,
            call_id: string(&self.0, "call_id").ok_or(Error::InvalidItem)?,
            name: string(&self.0, "name").ok_or(Error::InvalidItem)?,
            namespace: optional_string(&self.0, "namespace").ok_or(Error::InvalidItem)?,
        }))
    }
    pub fn tool_result(&self) -> Result<Option<ToolResult<'_>>> {
        let kind = match self.kind() {
            "function_call_output" => ToolKind::Function,
            "custom_tool_call_output" => ToolKind::Custom,
            _ => return Ok(None),
        };
        let output = self.0.get("output").ok_or(Error::InvalidItem)?;
        if !matches!(output, Value::String(_) | Value::Array(_)) {
            return Err(Error::InvalidItem);
        }
        let call_id = optional_string(&self.0, "call_id").ok_or(Error::InvalidItem)?;
        if kind == ToolKind::Custom && call_id.is_none() {
            return Err(Error::InvalidItem);
        }
        Ok(Some(ToolResult {
            kind,
            call_id,
            output,
            name: optional_string(&self.0, "name").ok_or(Error::InvalidItem)?,
            namespace: optional_string(&self.0, "namespace").ok_or(Error::InvalidItem)?,
        }))
    }
}
impl fmt::Debug for ResponseItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResponseItem([WIRE OMITTED])")
    }
}

/// Normalized counters plus the full provider usage object for cache/reasoning,
/// pricing metadata and future fields. Missing counters remain unknown, not zero.
pub struct Usage<'a> {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub raw: &'a Value,
}
impl<'a> Usage<'a> {
    pub fn new(raw: &'a Value) -> Result<Self> {
        if !raw.is_object() {
            return Err(Error::InvalidUsage);
        }
        let count = |name| match raw.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value.as_u64().map(Some).ok_or(Error::InvalidUsage),
        };
        Ok(Self {
            input_tokens: count("input_tokens")?,
            output_tokens: count("output_tokens")?,
            total_tokens: count("total_tokens")?,
            raw,
        })
    }
}

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct ResponseEvent(Value);
impl ResponseEvent {
    pub fn new(wire: Value) -> Result<Self> {
        if !wire.is_object() || string(&wire, "type").is_none() {
            return Err(Error::InvalidEvent);
        }
        let event = Self(wire);
        if matches!(
            event.kind(),
            "response.output_item.added" | "response.output_item.done"
        ) {
            let item = event.0.get("item").ok_or(Error::InvalidEvent)?;
            ResponseItem::new(item.clone()).map_err(|_| Error::InvalidEvent)?;
        }
        if matches!(
            event.kind(),
            "response.output_text.delta"
                | "response.function_call_arguments.delta"
                | "response.custom_tool_call_input.delta"
        ) && !event.0["delta"].is_string()
        {
            return Err(Error::InvalidEvent);
        }
        if matches!(event.kind(), "response.completed" | "response.incomplete") {
            let response = &event.0["response"];
            if !response.is_object() || string(response, "id").is_none() {
                return Err(Error::InvalidEvent);
            }
            let expected = if event.kind() == "response.completed" {
                "completed"
            } else {
                "incomplete"
            };
            if response
                .get("status")
                .is_some_and(|value| value != expected)
            {
                return Err(Error::InvalidEvent);
            }
            event.usage()?;
        }
        Ok(event)
    }
    pub fn wire(&self) -> &Value {
        &self.0
    }
    pub fn kind(&self) -> &str {
        self.0["type"].as_str().expect("validated type")
    }
    pub fn terminal(&self) -> Option<StreamState> {
        match self.kind() {
            "response.completed" => Some(StreamState::Completed),
            "response.incomplete"
                if self.0["response"]["incomplete_details"]["reason"] == "interrupted" =>
            {
                Some(StreamState::Interrupted)
            }
            "response.incomplete" => Some(StreamState::Incomplete),
            "response.failed" | "error" => Some(StreamState::Failed),
            _ => None,
        }
    }
    pub fn item(&self) -> Result<Option<ResponseItem>> {
        self.0
            .get("item")
            .cloned()
            .map(ResponseItem::new)
            .transpose()
    }
    pub fn text_delta(&self) -> Option<&str> {
        (self.kind() == "response.output_text.delta")
            .then(|| self.0["delta"].as_str())
            .flatten()
    }
    pub fn usage(&self) -> Result<Option<Usage<'_>>> {
        match self.0["response"].get("usage") {
            None | Some(Value::Null) => Ok(None),
            Some(raw) => Usage::new(raw).map(Some),
        }
    }
}
impl fmt::Debug for ResponseEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResponseEvent([WIRE OMITTED])")
    }
}

fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str().filter(|value| !value.is_empty())
}
fn optional_string<'a>(value: &'a Value, key: &str) -> Option<Option<&'a str>> {
    match value.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(_) => string(value, key).map(Some),
    }
}
