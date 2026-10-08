use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, ProviderError, ProviderResult, StreamState,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

fn invalid() -> ProviderError {
    ProviderError::new(400, "deepseek_invalid_tools")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "deepseek_unsupported_request")
}
fn fields(value: &Value, allowed: &[&str]) -> ProviderResult<()> {
    if value
        .as_object()
        .ok_or_else(invalid)?
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(unsupported());
    }
    Ok(())
}
fn name(value: &Value) -> ProviderResult<&str> {
    value
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        })
        .ok_or_else(invalid)
}
fn id(value: &Value) -> ProviderResult<&str> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
        .ok_or_else(invalid)
}

/// Original declarations and explicit apply_patch policy remain bound beside
/// the exact native aliases and tool kinds. Lite is not enabled.
pub(crate) struct ToolMap {
    source: Value,
    native: Vec<Value>,
    identities: HashMap<(Option<String>, String), String>,
}
impl ToolMap {
    pub(crate) fn from_request(
        request: &CanonicalRequest,
        apply_patch: bool,
    ) -> ProviderResult<Self> {
        let mut source = json!({"tools":request.wire()["tools"], "tool_choice":request.wire()["tool_choice"], "parallel_tool_calls":request.wire()["parallel_tool_calls"]});
        if apply_patch {
            source["apply_patch"] = true.into();
        }
        Self::from_source(source)
    }
    pub(crate) fn from_source(source: Value) -> ProviderResult<Self> {
        fields(
            &source,
            &["tools", "tool_choice", "parallel_tool_calls", "apply_patch"],
        )?;
        if source.get("apply_patch").is_some_and(|v| v != true) {
            return Err(invalid());
        }
        let mut map = Self {
            source,
            native: Vec::new(),
            identities: HashMap::new(),
        };
        let declarations = map.source["tools"].clone();
        if !declarations.is_null() {
            for tool in declarations.as_array().ok_or_else(invalid)? {
                if tool["type"] == "namespace" {
                    fields(tool, &["type", "name", "description", "tools"])?;
                    let namespace = id(&tool["name"])?;
                    let description = match tool.get("description") {
                        None | Some(Value::Null) => "",
                        Some(Value::String(s)) => s,
                        _ => return Err(invalid()),
                    };
                    for member in tool["tools"].as_array().ok_or_else(invalid)? {
                        map.add(member, Some(namespace), description)?;
                    }
                } else {
                    map.add(tool, None, "")?;
                }
            }
        }
        if !map.source["parallel_tool_calls"].is_null() && map.source["parallel_tool_calls"] != true
        {
            // Native always permits parallel calls; false cannot be enforced by
            // echoing an ignored flag. Lite gets its own delivery policy later.
            return Err(unsupported());
        }
        map.choice()?;
        Ok(map)
    }
    fn add(&mut self, tool: &Value, namespace: Option<&str>, guidance: &str) -> ProviderResult<()> {
        let custom = tool["type"] == "custom";
        fields(
            tool,
            if custom {
                &["type", "name", "description", "format", "defer_loading"]
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
        if custom {
            if self.source["apply_patch"] != true || tool["name"] != "apply_patch" {
                return Err(unsupported());
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
                    _ => return Err(unsupported()),
                }
            }
        } else if tool["type"] != "function" {
            return Err(unsupported());
        }
        let member = name(&tool["name"])?;
        if tool.get("description").is_some_and(|v| !v.is_string())
            || tool.get("parameters").is_some_and(|v| !v.is_object())
        {
            return Err(invalid());
        }
        for flag in ["strict", "defer_loading"] {
            if tool.get(flag).is_some_and(|v| !v.is_null() && v != false) {
                return Err(unsupported());
            }
        }
        let identity = (namespace.map(str::to_owned), member.to_owned());
        let alias = if namespace.is_some() && !custom {
            format!("caidex_ns_{}", self.native.len())
        } else {
            member.to_owned()
        };
        if self.identities.insert(identity, alias.clone()).is_some()
            || self.native.iter().any(|tool| tool["name"] == alias)
        {
            return Err(invalid());
        }
        let mut native = tool.clone();
        native["name"] = alias.into();
        if !guidance.is_empty() {
            native["description"] = format!(
                "Namespace description: {guidance}\n{}",
                tool["description"].as_str().unwrap_or("")
            )
            .into();
        }
        if custom && let Some(format) = tool.get("format") {
            native["description"] = format!(
                "{}\nOriginal apply_patch input format (guidance only): {format}",
                native["description"].as_str().unwrap_or("")
            )
            .into();
        }
        let object = native.as_object_mut().unwrap();
        object.remove("format");
        object.remove("strict");
        object.remove("defer_loading");
        self.native.push(native);
        Ok(())
    }
    fn alias(&self, item: &Value, kind: &str) -> ProviderResult<&str> {
        let namespace = item
            .get("namespace")
            .map(id)
            .transpose()?
            .map(str::to_owned);
        self.identities
            .get(&(namespace, name(&item["name"])?.to_owned()))
            .map(String::as_str)
            .filter(|alias| {
                self.native
                    .iter()
                    .any(|tool| tool["name"] == *alias && tool["type"] == kind)
            })
            .ok_or_else(invalid)
    }
    fn choice(&self) -> ProviderResult<Value> {
        let choice = &self.source["tool_choice"];
        if choice.is_null() {
            return Ok(Value::Null);
        }
        if choice.is_string() {
            if !matches!(choice.as_str(), Some("auto" | "none" | "required"))
                || choice == "required" && self.native.is_empty()
            {
                return Err(unsupported());
            }
            return Ok(choice.clone());
        }
        fields(choice, &["type", "name", "namespace"])?;
        if choice["type"] != "function" {
            return Err(unsupported());
        }
        Ok(json!({"type":"function", "name":self.alias(choice, "function")?}))
    }
    pub(crate) fn source(&self) -> &Value {
        &self.source
    }
    pub(crate) fn compile(&self, request: CanonicalRequest) -> ProviderResult<CanonicalRequest> {
        let mut wire = request.wire().clone();
        let object = wire.as_object_mut().unwrap();
        object.remove("parallel_tool_calls");
        if self.source["tools"].is_null() {
            object.remove("tools");
        } else {
            object.insert("tools".into(), self.native.clone().into());
        }
        let choice = self.choice()?;
        if choice.is_null() {
            object.remove("tool_choice");
        } else {
            object.insert("tool_choice".into(), choice);
        }
        CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
    }
    pub(crate) fn matches(&self, request: &CanonicalRequest) -> ProviderResult<()> {
        if self.compile(request.clone())?.wire()["tools"] != request.wire()["tools"]
            || self.choice()? != request.wire()["tool_choice"]
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn compile_item(&self, item: &Value) -> ProviderResult<Value> {
        let mut item = item.clone();
        if matches!(
            item["type"].as_str(),
            Some("function_call" | "custom_tool_call")
        ) {
            let kind = if item["type"] == "function_call" {
                "function"
            } else {
                "custom"
            };
            fields(
                &item,
                &[
                    "type",
                    "id",
                    "status",
                    "name",
                    "namespace",
                    "call_id",
                    if kind == "function" {
                        "arguments"
                    } else {
                        "input"
                    },
                ],
            )?;
            item["name"] = self.alias(&item, kind)?.into();
            item.as_object_mut().unwrap().remove("namespace");
        }
        Ok(item)
    }
    pub(crate) fn validate_response(
        &self,
        request: &CanonicalRequest,
        response: &CanonicalResponse,
    ) -> ProviderResult<()> {
        let result = (|| {
            let mut ids = validate_input(request)?;
            let mut item_ids = HashSet::new();
            let mut count = 0;
            for item in response.output() {
                if let Some(value) = item.get("id")
                    && !item_ids.insert(id(value)?.to_owned())
                {
                    return Err(invalid());
                }
                match item["type"].as_str() {
                    Some("function_call" | "custom_tool_call") => {
                        count += 1;
                        let kind = if item["type"] == "function_call" {
                            "function"
                        } else {
                            "custom"
                        };
                        let alias = name(&item["name"])?;
                        if item.get("namespace").is_some()
                            || !self
                                .native
                                .iter()
                                .any(|tool| tool["name"] == alias && tool["type"] == kind)
                            || !ids.insert(id(&item["call_id"])?.to_owned())
                        {
                            return Err(invalid());
                        }
                        id(&item["id"])?;
                        if kind == "function" {
                            arguments(item)?;
                        } else if !item["input"].is_string() {
                            return Err(invalid());
                        }
                        if response.state() != StreamState::Completed
                            || item.get("status").is_some_and(|s| s != "completed")
                        {
                            return Err(invalid());
                        }
                    }
                    Some(
                        "tool_search_call"
                        | "tool_search_output"
                        | "function_call_output"
                        | "custom_tool_call_output"
                        | "web_search_call",
                    ) => return Err(invalid()),
                    Some(kind) if kind.ends_with("_call") => return Err(invalid()),
                    _ => (),
                }
            }
            let choice = self.choice()?;
            if choice == "none" && count != 0
                || response.state() == StreamState::Completed
                    && (choice == "required" || choice.is_object())
                    && count == 0
            {
                return Err(invalid());
            }
            if choice.is_object()
                && response
                    .output()
                    .iter()
                    .filter(|item| {
                        matches!(
                            item["type"].as_str(),
                            Some("function_call" | "custom_tool_call")
                        )
                    })
                    .any(|item| item["type"] != "function_call" || item["name"] != choice["name"])
            {
                return Err(invalid());
            }
            Ok(())
        })();
        result.map_err(|_: ProviderError| ProviderError::new(502, "deepseek_invalid_native_tools"))
    }
    pub(crate) fn project(
        &self,
        response: &CanonicalResponse,
    ) -> ProviderResult<CanonicalResponse> {
        let mut wire = response.wire().clone();
        for item in wire["output"].as_array_mut().unwrap() {
            if matches!(
                item["type"].as_str(),
                Some("function_call" | "custom_tool_call")
            ) {
                let ((namespace, member), _) = self
                    .identities
                    .iter()
                    .find(|(_, alias)| item["name"] == alias.as_str())
                    .ok_or_else(invalid)?;
                item["name"] = member.clone().into();
                if let Some(namespace) = namespace {
                    item["namespace"] = namespace.clone().into();
                }
            }
        }
        CanonicalResponse::new(wire).map_err(|_| invalid())
    }
}
fn arguments(item: &Value) -> ProviderResult<()> {
    let value: Value = serde_json::from_str(item["arguments"].as_str().ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    if !value.is_object() {
        return Err(invalid());
    }
    Ok(())
}
pub(crate) fn validate_input(request: &CanonicalRequest) -> ProviderResult<HashSet<String>> {
    let mut seen = HashSet::new();
    let mut pending = HashMap::new();
    if let Some(input) = request.wire()["input"].as_array() {
        for item in input {
            if item.get("id").is_some_and(|v| id(v).is_err())
                || item.get("status").is_some_and(|v| v != "completed")
            {
                return Err(invalid());
            }
            match item["type"].as_str() {
                Some("function_call" | "custom_tool_call") => {
                    let call = id(&item["call_id"])?;
                    name(&item["name"])?;
                    if item["type"] == "function_call" {
                        arguments(item)?;
                    } else if item["name"] != "apply_patch" || !item["input"].is_string() {
                        return Err(invalid());
                    }
                    if !seen.insert(call.to_owned()) {
                        return Err(invalid());
                    }
                    pending.insert(call.to_owned(), item["type"].as_str().unwrap());
                }
                Some("function_call_output" | "custom_tool_call_output") => {
                    fields(item, &["type", "id", "status", "call_id", "output"])?;
                    let expected = if item["type"] == "function_call_output" {
                        "function_call"
                    } else {
                        "custom_tool_call"
                    };
                    if pending.remove(id(&item["call_id"])?) != Some(expected) {
                        return Err(invalid());
                    }
                    if !item["output"].is_string() {
                        for part in item["output"].as_array().ok_or_else(invalid)? {
                            fields(part, &["type", "text"])?;
                            if !matches!(part["type"].as_str(), Some("input_text" | "output_text"))
                                || !part["text"].is_string()
                            {
                                return Err(unsupported());
                            }
                        }
                    }
                }
                _ if !pending.is_empty()
                    && (item.get("role").is_some() || item["type"] == "reasoning") =>
                {
                    return Err(invalid());
                }
                _ => (),
            }
        }
    }
    if !pending.is_empty() {
        return Err(invalid());
    }
    Ok(seen)
}
