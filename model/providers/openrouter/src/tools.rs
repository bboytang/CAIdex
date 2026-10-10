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
fn call_item(item: &Value) -> bool {
    matches!(
        item["type"].as_str(),
        Some("function_call" | "custom_tool_call")
    )
}
fn payload(item: &Value) -> &'static str {
    if item["type"] == "custom_tool_call" {
        "input"
    } else {
        "arguments"
    }
}
pub(crate) fn input_item(item: &Value) -> bool {
    call_item(item)
        || matches!(
            item["type"].as_str(),
            Some("function_call_output" | "custom_tool_call_output")
        )
}
type Identity = (Option<String>, String);
fn identity(item: &Value) -> ProviderResult<Identity> {
    Ok((
        item.get("namespace")
            .map(name)
            .transpose()?
            .map(str::to_owned),
        name(&item["name"])?.to_owned(),
    ))
}

/// Route-local declarations and selection, never an executor or schema validator.
pub(crate) struct ToolPolicy {
    declared: HashMap<Identity, bool>,
    allowed: HashSet<Identity>,
    required: bool,
    single: bool,
    advanced: bool,
    selection: bool,
    input_calls: HashSet<String>,
    input_item_ids: HashSet<String>,
}
impl ToolPolicy {
    pub(crate) fn new(
        request: &CanonicalRequest,
        parallel: CapabilitySupport,
        advanced: bool,
    ) -> ProviderResult<Self> {
        let wire = request.wire();
        let mut policy = Self {
            declared: HashMap::new(),
            allowed: HashSet::new(),
            required: false,
            single: false,
            advanced,
            selection: false,
            input_calls: HashSet::new(),
            input_item_ids: HashSet::new(),
        };
        let mut namespaces = HashSet::new();
        if let Some(tools) = wire.get("tools").filter(|v| !v.is_null()) {
            for tool in tools.as_array().ok_or_else(invalid)? {
                if advanced && tool["type"] == "namespace" {
                    fields(tool, &["type", "name", "description", "tools"])?;
                    let ns = name(&tool["name"])?;
                    let members = tool["tools"].as_array().ok_or_else(invalid)?;
                    if !namespaces.insert(ns)
                        || members.is_empty()
                        || !tool["description"].is_string()
                    {
                        return Err(invalid());
                    }
                    for member in members {
                        policy.declare(member, Some(ns))?;
                    }
                } else {
                    policy.declare(tool, None)?;
                }
            }
        }
        policy.allowed = policy.declared.keys().cloned().collect();
        let choice = &wire["tool_choice"];
        if !choice.is_null() {
            if let Some(mode) = choice.as_str() {
                match mode {
                    "auto" => {}
                    "none" => policy.allowed.clear(),
                    "required" => policy.required = true,
                    _ => return Err(invalid()),
                }
            } else if advanced && choice["type"] == "allowed_tools" {
                fields(choice, &["type", "mode", "tools"])?;
                match choice["mode"].as_str() {
                    Some("auto") => {}
                    Some("required") => policy.required = true,
                    _ => return Err(invalid()),
                }
                let mut selected = HashSet::new();
                for tool in choice["tools"].as_array().ok_or_else(invalid)? {
                    if !selected.insert(policy.selector(tool)?) {
                        return Err(invalid());
                    }
                }
                if selected.is_empty() {
                    return Err(invalid());
                }
                policy.allowed = selected;
                policy.selection = true;
            } else {
                let selected = policy.selector(choice)?;
                policy.allowed = HashSet::from([selected]);
                policy.required = true;
                policy.selection =
                    advanced && (choice["type"] == "custom" || choice.get("namespace").is_some());
            }
        }
        if policy.required && policy.allowed.is_empty() {
            return Err(invalid());
        }
        let flag = wire.get("parallel_tool_calls").filter(|v| !v.is_null());
        if flag.is_some_and(|v| !v.is_boolean())
            || flag == Some(&Value::Bool(true)) && parallel == CapabilitySupport::Unsupported
        {
            return Err(invalid());
        }
        policy.single =
            flag == Some(&Value::Bool(false)) || parallel == CapabilitySupport::Unsupported;
        policy.validate_input(wire, &HashSet::new())?;
        Ok(policy)
    }
    fn declare(&mut self, tool: &Value, namespace: Option<&str>) -> ProviderResult<()> {
        let custom = self.advanced && tool["type"] == "custom";
        fields(
            tool,
            if custom {
                &["type", "name", "description", "format", "async"]
            } else {
                &[
                    "type",
                    "name",
                    "description",
                    "parameters",
                    "strict",
                    "defer_loading",
                ]
            },
        )?;
        if tool
            .get("description")
            .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return Err(invalid());
        }
        if custom {
            if tool
                .get("async")
                .is_some_and(|v| !v.is_null() && v != false)
            {
                return Err(invalid());
            }
            if let Some(format) = tool.get("format") {
                match format["type"].as_str() {
                    Some("text") => fields(format, &["type"])?,
                    Some("grammar") => {
                        fields(format, &["type", "syntax", "definition"])?;
                        if !matches!(format["syntax"].as_str(), Some("lark" | "regex"))
                            || !format["definition"]
                                .as_str()
                                .is_some_and(|s| !s.trim().is_empty())
                        {
                            return Err(invalid());
                        }
                    }
                    _ => return Err(invalid()),
                }
            }
        } else if tool["type"] != "function"
            || !tool["parameters"].is_object()
            || ["strict", "defer_loading"]
                .iter()
                .any(|key| tool.get(key).is_some_and(|v| !v.is_null() && v != false))
        {
            return Err(invalid());
        }
        let key = (
            namespace.map(str::to_owned),
            name(&tool["name"])?.to_owned(),
        );
        if self.declared.insert(key, custom).is_some() {
            return Err(invalid());
        }
        Ok(())
    }
    fn selector(&self, value: &Value) -> ProviderResult<Identity> {
        fields(
            value,
            if self.advanced {
                &["type", "name", "namespace"]
            } else {
                &["type", "name"]
            },
        )?;
        let key = identity(value)?;
        let custom = match value["type"].as_str() {
            Some("function") => false,
            Some("custom") if self.advanced => true,
            _ => return Err(invalid()),
        };
        if self.declared.get(&key) != Some(&custom) {
            return Err(invalid());
        }
        Ok(key)
    }
    pub(crate) fn compile_selection(&self, wire: &mut Value) {
        if !self.selection {
            return;
        }
        // Native named custom/namespace selectors are unspecified; restrict declarations instead.
        let tools = wire["tools"].as_array_mut().expect("validated tools");
        tools.retain_mut(|tool| {
            if tool["type"] == "namespace" {
                let namespace = tool["name"].as_str().unwrap().to_owned();
                let members = tool["tools"].as_array_mut().unwrap();
                members.retain(|member| {
                    self.allowed.contains(&(
                        Some(namespace.clone()),
                        member["name"].as_str().unwrap().to_owned(),
                    ))
                });
                !members.is_empty()
            } else {
                self.allowed
                    .contains(&(None, tool["name"].as_str().unwrap().to_owned()))
            }
        });
        wire["tool_choice"] = if self.required { "required" } else { "auto" }.into();
    }
    pub(crate) fn has_tools(&self) -> bool {
        !self.declared.is_empty()
    }
    fn call(&self, item: &Value) -> ProviderResult<()> {
        id(&item["call_id"])?;
        let custom = item["type"] == "custom_tool_call";
        if self.declared.get(&identity(item)?) != Some(&custom)
            || item
                .get("async")
                .is_some_and(|v| !v.is_null() && v != false)
            || item.get("subagent_id").is_some()
            || item.get("subagent_items").is_some()
            || item.get("status").is_some_and(|v| v != "completed")
            || item.get("id").is_some_and(|v| id(v).is_err())
        {
            return Err(invalid());
        }
        if custom {
            item["input"].as_str().ok_or_else(invalid).map(|_| ())
        } else {
            arguments(&item["arguments"])
        }
    }
    pub(crate) fn validate_input(
        &mut self,
        wire: &Value,
        trusted: &HashSet<usize>,
    ) -> ProviderResult<()> {
        let mut pending = HashMap::new();
        let mut ids = HashSet::new();
        for (index, item) in wire["input"].as_array().into_iter().flatten().enumerate() {
            if !input_item(item) {
                if !pending.is_empty() && item["role"] != "assistant" && !trusted.contains(&index) {
                    return Err(invalid());
                }
                continue;
            }
            if let Some(item_id) = item.get("id")
                && !ids.insert(id(item_id)?.to_owned())
            {
                return Err(invalid());
            }
            if call_item(item) {
                if !trusted.contains(&index) {
                    fields(
                        item,
                        if self.advanced && item["type"] == "custom_tool_call" {
                            &[
                                "type",
                                "id",
                                "status",
                                "name",
                                "namespace",
                                "call_id",
                                "input",
                                "async",
                            ]
                        } else if self.advanced {
                            &[
                                "type",
                                "id",
                                "status",
                                "name",
                                "namespace",
                                "call_id",
                                "arguments",
                                "async",
                            ]
                        } else {
                            &["type", "id", "status", "name", "call_id", "arguments"]
                        },
                    )?;
                }
                self.call(item)?;
                let call_id = id(&item["call_id"])?;
                if !self.input_calls.insert(call_id.to_owned()) {
                    return Err(invalid());
                }
                pending.insert(call_id.to_owned(), item["type"] == "custom_tool_call");
            } else {
                fields(item, &["type", "id", "status", "call_id", "output"])?;
                if pending.remove(id(&item["call_id"])?)
                    != Some(item["type"] == "custom_tool_call_output")
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
                .filter(|item| !call_item(item))
                .filter_map(|item| item["id"].as_str())
                .collect();
            let mut count = 0;
            for item in response.output() {
                if !call_item(item) {
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
                    || !self.allowed.contains(&identity(item)?)
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
pub(crate) struct ToolEvents {
    calls: HashMap<u64, PendingCall>,
    terminal: Option<Value>,
}
impl ToolEvents {
    pub(crate) fn observe(&mut self, wire: &Value) -> ProviderResult<()> {
        let kind = wire["type"].as_str().ok_or_else(native_error)?;
        if call_item(wire)
            || wire["output"]
                .as_array()
                .is_some_and(|items| items.iter().any(call_item))
            || call_item(&wire["item"])
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
            .is_some_and(|items| items.iter().any(call_item))
        {
            return Err(native_error());
        }
        if kind == "response.output_item.added" && call_item(&wire["item"]) {
            let index = wire["output_index"].as_u64().ok_or_else(native_error)?;
            let item = &wire["item"];
            id(&item["id"])?;
            id(&item["call_id"])?;
            identity(item)?;
            if item
                .get("status")
                .is_some_and(|v| v != "in_progress" && v != "completed")
            {
                return Err(native_error());
            }
            let arguments = item[payload(item)]
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
            "response.function_call_arguments.delta"
                | "response.function_call_arguments.done"
                | "response.custom_tool_call_input.delta"
                | "response.custom_tool_call_input.done"
        ) || kind == "response.output_item.done" && call_item(&wire["item"])
        {
            let index = wire["output_index"].as_u64().ok_or_else(native_error)?;
            let call = self.calls.get_mut(&index).ok_or_else(native_error)?;
            if call.done.is_some() {
                return Err(native_error());
            }
            if kind == "response.output_item.done" {
                let arguments = wire["item"][payload(&call.added)]
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
                if wire["item_id"] != call.added["id"]
                    || call.arguments_done
                    || kind.starts_with("response.custom_tool_call_input.")
                        != (call.added["type"] == "custom_tool_call")
                {
                    return Err(native_error());
                }
                if kind.ends_with(".delta") {
                    call.arguments
                        .push_str(wire["delta"].as_str().ok_or_else(native_error)?);
                    call.delta = true;
                } else {
                    let arguments = wire[payload(&call.added)]
                        .as_str()
                        .ok_or_else(native_error)?;
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
    pub(crate) fn finish(&self, policy: &ToolPolicy) -> ProviderResult<()> {
        let terminal = self.terminal.as_ref().ok_or_else(native_error)?;
        policy.response(terminal)?;
        for (index, call) in &self.calls {
            let item = terminal["output"]
                .get(*index as usize)
                .ok_or_else(native_error)?;
            if call.done.as_ref() != Some(item)
                || !call_item(item)
                || ["type", "id", "call_id", "name", "namespace"]
                    .iter()
                    .any(|k| item[k] != call.added[k])
                || item[payload(item)] != call.arguments
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
