use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, CapabilitySupport, ProviderError, ProviderResult,
    ProviderStream, ProviderStreamEvent, RequestContext, StreamState,
};
use futures_util::{StreamExt, stream};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Instant,
};

fn invalid() -> ProviderError {
    ProviderError::new(400, "openrouter_invalid_tools")
}
fn native_error() -> ProviderError {
    ProviderError::new(502, "openrouter_invalid_native_tools")
}
fn fields(value: &Value, allowed: &[&str]) -> ProviderResult<()> {
    if value
        .as_object()
        .ok_or_else(invalid)?
        .keys()
        .any(|k| !allowed.contains(&k.as_str()))
    {
        return Err(invalid());
    }
    Ok(())
}
fn id(value: &Value) -> ProviderResult<&str> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
        .ok_or_else(invalid)
}
fn name(value: &Value) -> ProviderResult<&str> {
    let s = id(value)?;
    if s.len() > 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(invalid());
    }
    Ok(s)
}
fn arguments(value: &Value) -> ProviderResult<()> {
    if !serde_json::from_str::<Value>(value.as_str().ok_or_else(invalid)?)
        .map_err(|_| invalid())?
        .is_object()
    {
        return Err(invalid());
    }
    Ok(())
}
pub(crate) fn input_item(item: &Value) -> bool {
    matches!(
        item["type"].as_str(),
        Some("function_call" | "function_call_output")
    )
}

/// Route-local declarations and selection, never an executor or schema validator.
pub(crate) struct ToolPolicy {
    declared: HashSet<String>,
    allowed: HashSet<String>,
    required: bool,
    single: bool,
    input_calls: HashSet<String>,
    input_item_ids: HashSet<String>,
}
impl ToolPolicy {
    pub(crate) fn new(
        request: &CanonicalRequest,
        parallel: CapabilitySupport,
    ) -> ProviderResult<Self> {
        let wire = request.wire();
        let mut declared = HashSet::new();
        if let Some(tools) = wire.get("tools").filter(|v| !v.is_null()) {
            for tool in tools.as_array().ok_or_else(invalid)? {
                fields(
                    tool,
                    &[
                        "type",
                        "name",
                        "description",
                        "parameters",
                        "strict",
                        "defer_loading",
                    ],
                )?;
                if tool["type"] != "function"
                    || tool
                        .get("description")
                        .is_some_and(|v| !v.is_null() && !v.is_string())
                    || !tool["parameters"].is_object()
                    || ["strict", "defer_loading"]
                        .iter()
                        .any(|key| tool.get(key).is_some_and(|v| !v.is_null() && v != false))
                    || !declared.insert(name(&tool["name"])?.to_owned())
                {
                    return Err(invalid());
                }
            }
        }
        let mut allowed = declared.clone();
        let choice = &wire["tool_choice"];
        let mut required = false;
        if !choice.is_null() {
            if let Some(mode) = choice.as_str() {
                match mode {
                    "auto" => {}
                    "none" => allowed.clear(),
                    "required" => required = true,
                    _ => return Err(invalid()),
                }
            } else {
                fields(choice, &["type", "name"])?;
                let selected = name(&choice["name"])?;
                if choice["type"] != "function" || !declared.contains(selected) {
                    return Err(invalid());
                }
                allowed = HashSet::from([selected.to_owned()]);
                required = true;
            }
        }
        if required && allowed.is_empty() {
            return Err(invalid());
        }
        let flag = wire.get("parallel_tool_calls").filter(|v| !v.is_null());
        if flag.is_some_and(|v| !v.is_boolean())
            || flag == Some(&Value::Bool(true)) && parallel == CapabilitySupport::Unsupported
        {
            return Err(invalid());
        }
        let mut policy = Self {
            declared,
            allowed,
            required,
            single: flag == Some(&Value::Bool(false)) || parallel == CapabilitySupport::Unsupported,
            input_calls: HashSet::new(),
            input_item_ids: HashSet::new(),
        };
        policy.validate_input(wire)?;
        Ok(policy)
    }
    pub(crate) fn has_tools(&self) -> bool {
        !self.declared.is_empty()
    }
    fn call(&self, item: &Value) -> ProviderResult<()> {
        id(&item["call_id"])?;
        if !self.declared.contains(name(&item["name"])?)
            || item.get("namespace").is_some()
            || item.get("status").is_some_and(|v| v != "completed")
            || item.get("id").is_some_and(|v| id(v).is_err())
        {
            return Err(invalid());
        }
        arguments(&item["arguments"])
    }
    fn validate_input(&mut self, wire: &Value) -> ProviderResult<()> {
        let mut pending = HashSet::new();
        let mut ids = HashSet::new();
        for item in wire["input"].as_array().into_iter().flatten() {
            if !input_item(item) {
                if !pending.is_empty() && item["role"] != "assistant" {
                    return Err(invalid());
                }
                continue;
            }
            if let Some(item_id) = item.get("id")
                && !ids.insert(id(item_id)?.to_owned())
            {
                return Err(invalid());
            }
            if item["type"] == "function_call" {
                fields(
                    item,
                    &["type", "id", "status", "name", "call_id", "arguments"],
                )?;
                self.call(item)?;
                let call_id = id(&item["call_id"])?;
                if !self.input_calls.insert(call_id.to_owned()) {
                    return Err(invalid());
                }
                pending.insert(call_id.to_owned());
            } else {
                fields(item, &["type", "id", "status", "call_id", "output"])?;
                if !pending.remove(id(&item["call_id"])?)
                    || item.get("status").is_some_and(|v| v != "completed")
                {
                    return Err(invalid());
                }
                if !item["output"].is_string() {
                    for part in item["output"].as_array().ok_or_else(invalid)? {
                        fields(part, &["type", "text"])?;
                        if part["type"] != "input_text" || !part["text"].is_string() {
                            return Err(invalid());
                        }
                    }
                }
            }
        }
        if !pending.is_empty() {
            return Err(invalid());
        }
        self.input_item_ids = ids;
        Ok(())
    }
    pub(crate) fn response(&self, wire: &Value) -> ProviderResult<()> {
        let check = || {
            let response = CanonicalResponse::new(wire.clone()).map_err(|_| invalid())?;
            let mut call_ids = self.input_calls.clone();
            let mut ids: HashSet<&str> = response
                .output()
                .iter()
                .filter(|item| item["type"] != "function_call")
                .filter_map(|item| item["id"].as_str())
                .collect();
            let mut count = 0;
            for item in response.output() {
                if item["type"] != "function_call" {
                    continue;
                }
                if !ids.insert(id(&item["id"])?) {
                    return Err(invalid());
                }
                self.call(item)?;
                if self.input_item_ids.contains(id(&item["id"])?) {
                    return Err(invalid());
                }
                if response.state() != StreamState::Completed
                    || !call_ids.insert(id(&item["call_id"])?.to_owned())
                    || !self.allowed.contains(name(&item["name"])?)
                {
                    return Err(invalid());
                }
                count += 1;
            }
            if self.single && count > 1
                || self.required && response.state() == StreamState::Completed && count == 0
            {
                return Err(invalid());
            }
            Ok(())
        };
        check().map_err(|_: ProviderError| native_error())
    }
}

struct PendingCall {
    added: Value,
    arguments: String,
    delta: bool,
    arguments_done: bool,
    done: Option<Value>,
}
#[derive(Default)]
struct ToolEvents {
    calls: HashMap<u64, PendingCall>,
    terminal: Option<Value>,
}
impl ToolEvents {
    fn observe(&mut self, wire: &Value) -> ProviderResult<()> {
        let kind = wire["type"].as_str().ok_or_else(native_error)?;
        if wire["type"] == "function_call"
            || wire["output"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["type"] == "function_call"))
            || wire["item"]["type"] == "function_call"
                && !matches!(
                    kind,
                    "response.output_item.added" | "response.output_item.done"
                )
        {
            return Err(native_error());
        }
        if matches!(
            kind,
            "response.completed" | "response.failed" | "response.incomplete"
        ) {
            self.terminal = Some(wire["response"].clone());
            return Ok(());
        }
        if wire["response"]["output"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["type"] == "function_call"))
        {
            return Err(native_error());
        }
        if kind == "response.output_item.added" && wire["item"]["type"] == "function_call" {
            let index = wire["output_index"].as_u64().ok_or_else(native_error)?;
            let item = &wire["item"];
            id(&item["id"])?;
            id(&item["call_id"])?;
            name(&item["name"])?;
            if item.get("namespace").is_some()
                || item
                    .get("status")
                    .is_some_and(|v| v != "in_progress" && v != "completed")
            {
                return Err(native_error());
            }
            let arguments = item["arguments"]
                .as_str()
                .ok_or_else(native_error)?
                .to_owned();
            if self
                .calls
                .insert(
                    index,
                    PendingCall {
                        added: item.clone(),
                        arguments,
                        delta: false,
                        arguments_done: false,
                        done: None,
                    },
                )
                .is_some()
            {
                return Err(native_error());
            }
        } else if matches!(
            kind,
            "response.function_call_arguments.delta" | "response.function_call_arguments.done"
        ) || kind == "response.output_item.done"
            && wire["item"]["type"] == "function_call"
        {
            let index = wire["output_index"].as_u64().ok_or_else(native_error)?;
            let call = self.calls.get_mut(&index).ok_or_else(native_error)?;
            if call.done.is_some() {
                return Err(native_error());
            }
            if kind == "response.output_item.done" {
                let arguments = wire["item"]["arguments"]
                    .as_str()
                    .ok_or_else(native_error)?;
                if (call.delta || call.arguments_done) && arguments != call.arguments
                    || !call.delta
                        && !call.arguments_done
                        && !arguments.starts_with(&call.arguments)
                {
                    return Err(native_error());
                }
                call.arguments = arguments.to_owned();
                call.done = Some(wire["item"].clone());
            } else {
                if wire["item_id"] != call.added["id"] || call.arguments_done {
                    return Err(native_error());
                }
                if kind.ends_with(".delta") {
                    call.arguments
                        .push_str(wire["delta"].as_str().ok_or_else(native_error)?);
                    call.delta = true;
                } else {
                    let arguments = wire["arguments"].as_str().ok_or_else(native_error)?;
                    if call.delta && call.arguments != arguments
                        || !call.delta && !arguments.starts_with(&call.arguments)
                    {
                        return Err(native_error());
                    }
                    call.arguments = arguments.to_owned();
                    call.arguments_done = true;
                }
            }
        }
        Ok(())
    }
    fn finish(&self, policy: &ToolPolicy) -> ProviderResult<()> {
        let terminal = self.terminal.as_ref().ok_or_else(native_error)?;
        policy.response(terminal)?;
        for (index, call) in &self.calls {
            let item = terminal["output"]
                .get(*index as usize)
                .ok_or_else(native_error)?;
            if call.done.as_ref() != Some(item)
                || item["type"] != "function_call"
                || ["id", "call_id", "name"]
                    .iter()
                    .any(|k| item[k] != call.added[k])
                || item["arguments"] != call.arguments
            {
                return Err(native_error());
            }
        }
        Ok(())
    }
}

struct BufferedTools {
    events: ProviderStream,
    policy: ToolPolicy,
    context: RequestContext,
    pending: VecDeque<ProviderStreamEvent>,
    validated: bool,
    observed: ToolEvents,
    bytes: usize,
}
// shortcut: buffer model events on tool routes; add incremental text delivery after safe tool gating is verified.
pub(crate) fn buffered_stream(
    events: ProviderStream,
    policy: ToolPolicy,
    context: RequestContext,
    limit: usize,
) -> ProviderStream {
    let state = BufferedTools {
        events,
        policy,
        context,
        pending: VecDeque::new(),
        validated: false,
        observed: ToolEvents::default(),
        bytes: 0,
    };
    Box::pin(stream::unfold(Some(state), move |state| async move {
        let mut state = state?;
        if let Err(error) = state.check_context() {
            return Some((Err(error), None));
        }
        if !state.validated {
            while let Some(next) = state.events.next().await {
                let event = match next {
                    Ok(event) => event,
                    Err(error) => return Some((Err(error), None)),
                };
                if let ProviderStreamEvent::Model(model) = &event {
                    let wire = model.response.wire();
                    state.bytes = state.bytes.saturating_add(wire.to_string().len());
                    if state.bytes > limit {
                        return Some((
                            Err(ProviderError::new(502, "openrouter_tool_stream_too_large")),
                            None,
                        ));
                    }
                    if let Err(error) = crate::request::output(wire, true) {
                        return Some((Err(error), None));
                    }
                    if state.observed.observe(wire).is_err() {
                        return Some((Err(native_error()), None));
                    }
                    state.pending.push_back(event);
                } else {
                    return Some((Ok(event), Some(state)));
                }
            }
            if let Err(error) = state.observed.finish(&state.policy) {
                return Some((Err(error), None));
            }
            state.validated = true;
        }
        // Cancellation after native completion still prevents queued tool delivery.
        if let Err(error) = state.check_context() {
            return Some((Err(error), None));
        }
        state
            .pending
            .pop_front()
            .map(|event| (Ok(event), Some(state)))
    }))
}
impl BufferedTools {
    fn check_context(&self) -> ProviderResult<()> {
        if self.context.cancellation.is_cancelled() {
            return Err(ProviderError::new(503, "provider_cancelled"));
        }
        if self.context.deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(ProviderError::new(504, "provider_timeout"));
        }
        Ok(())
    }
}
