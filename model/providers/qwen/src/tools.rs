use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, ProviderError, ProviderResult, StreamState,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

fn invalid() -> ProviderError {
    ProviderError::new(400, "qwen_invalid_tools")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "qwen_unsupported_tools")
}
fn fields(value: &Value, allowed: &[&str]) -> ProviderResult<()> {
    if value
        .as_object()
        .ok_or_else(invalid)?
        .keys()
        .any(|k| !allowed.contains(&k.as_str()))
    {
        return Err(unsupported());
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
    id(value).and_then(|s| {
        if s.len() <= 64
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            Ok(s)
        } else {
            Err(invalid())
        }
    })
}
fn arguments(item: &Value, custom: bool) -> ProviderResult<()> {
    let value: Value = serde_json::from_str(item["arguments"].as_str().ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    if !value.is_object()
        || custom && (value.as_object().unwrap().len() != 1 || !value["input"].is_string())
    {
        return Err(invalid());
    }
    Ok(())
}
/// Executor declarations and policy bind both compiled aliases and replay.
pub(crate) struct ToolMap {
    source: Value,
    native: Vec<Value>,
    identities: HashMap<(Option<String>, String), String>,
    mapped_custom: HashSet<String>,
    choice: Value,
    allowed: HashSet<String>,
    required: bool,
}
impl ToolMap {
    pub(crate) fn from_request(request: &CanonicalRequest, custom: bool) -> ProviderResult<Self> {
        let mut source = json!({"tools":request.wire()["tools"],"tool_choice":request.wire()["tool_choice"],"parallel_tool_calls":request.wire()["parallel_tool_calls"]});
        if custom {
            source["custom_as_function"] = true.into();
        }
        Self::from_source(source)
    }
    pub(crate) fn from_source(source: Value) -> ProviderResult<Self> {
        fields(
            &source,
            &[
                "tools",
                "tool_choice",
                "parallel_tool_calls",
                "custom_as_function",
            ],
        )?;
        if source.get("custom_as_function").is_some_and(|v| v != true) {
            return Err(invalid());
        }
        if !source["parallel_tool_calls"].is_null() && source["parallel_tool_calls"] != true {
            return Err(unsupported());
        }
        let mut map = Self {
            source,
            native: Vec::new(),
            identities: HashMap::new(),
            mapped_custom: HashSet::new(),
            choice: Value::Null,
            allowed: HashSet::new(),
            required: false,
        };
        let declarations = map.source["tools"].clone();
        if !declarations.is_null() {
            for tool in declarations.as_array().ok_or_else(invalid)? {
                if tool["type"] == "namespace" {
                    fields(tool, &["type", "name", "description", "tools"])?;
                    let namespace = id(&tool["name"])?;
                    let guidance = match tool.get("description") {
                        None | Some(Value::Null) => "",
                        Some(Value::String(s)) => s,
                        _ => return Err(invalid()),
                    };
                    let members = tool["tools"].as_array().ok_or_else(invalid)?;
                    if members.is_empty() {
                        return Err(invalid());
                    }
                    for member in members {
                        map.add(member, Some(namespace), guidance)?;
                    }
                } else {
                    map.add(tool, None, "")?;
                }
            }
        }
        map.select()?;
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
            if self.source["custom_as_function"] != true {
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
        let alias = if namespace.is_some() {
            format!("caidex_ns_{}", self.native.len())
        } else {
            member.into()
        };
        if self
            .identities
            .insert((namespace.map(str::to_owned), member.into()), alias.clone())
            .is_some()
            || self.native.iter().any(|t| t["name"] == alias)
        {
            return Err(invalid());
        }
        let mut native = tool.clone();
        native["name"] = alias.clone().into();
        let description = tool["description"].as_str().unwrap_or("");
        native["description"] = if guidance.is_empty() {
            description.into()
        } else {
            format!("Namespace description: {guidance}\n{description}")
        }
        .into();
        native.as_object_mut().unwrap().remove("strict");
        native.as_object_mut().unwrap().remove("defer_loading");
        if custom {
            if let Some(format) = tool.get("format") {
                native["description"] = format!(
                    "{}\nOriginal custom input format (guidance only): {format}",
                    native["description"].as_str().unwrap()
                )
                .into();
            }
            native["description"] = format!(
                "{}\nReturn the original freeform input as the input string.",
                native["description"].as_str().unwrap()
            )
            .into();
            native["type"] = "function".into();
            native["parameters"] = json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false});
            native.as_object_mut().unwrap().remove("format");
            self.mapped_custom.insert(alias);
        }
        self.native.push(native);
        Ok(())
    }
    fn alias(&self, item: &Value) -> ProviderResult<&str> {
        if !matches!(
            item["type"].as_str(),
            Some("function" | "function_call" | "custom" | "custom_tool_call")
        ) {
            return Err(unsupported());
        }
        let namespace = item
            .get("namespace")
            .map(id)
            .transpose()?
            .map(str::to_owned);
        let alias = self
            .identities
            .get(&(namespace, name(&item["name"])?.into()))
            .map(String::as_str)
            .ok_or_else(invalid)?;
        if matches!(item["type"].as_str(), Some("custom" | "custom_tool_call"))
            != self.mapped_custom.contains(alias)
        {
            return Err(invalid());
        }
        Ok(alias)
    }
    fn select(&mut self) -> ProviderResult<()> {
        self.allowed = self
            .native
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        let choice = self.source["tool_choice"].clone();
        if choice.is_null() {
            return Ok(());
        }
        if let Some(mode) = choice.as_str() {
            if !matches!(mode, "auto" | "none" | "required") {
                return Err(unsupported());
            }
            if mode == "required" && self.native.len() != 1 {
                return Err(unsupported());
            }
            self.required = mode == "required";
            if mode == "none" {
                self.allowed.clear();
            }
            self.choice = choice;
            return Ok(());
        }
        let mut selected = HashSet::new();
        let mode = if choice["type"] == "function" || choice["type"] == "custom" {
            fields(&choice, &["type", "name", "namespace"])?;
            selected.insert(self.alias(&choice)?.to_owned());
            "required"
        } else if choice["type"] == "allowed_tools" {
            fields(&choice, &["type", "mode", "tools"])?;
            let mode = choice["mode"].as_str().ok_or_else(invalid)?;
            if !matches!(mode, "auto" | "required") {
                return Err(unsupported());
            }
            for tool in choice["tools"].as_array().ok_or_else(invalid)? {
                fields(tool, &["type", "name", "namespace"])?;
                if !selected.insert(self.alias(tool)?.to_owned()) {
                    return Err(invalid());
                }
            }
            if selected.is_empty() {
                return Err(invalid());
            }
            mode
        } else {
            return Err(unsupported());
        };
        if mode == "required" && selected.len() != 1 {
            return Err(unsupported());
        }
        self.required = mode == "required";
        self.allowed = selected;
        // Restrict declarations as well as allowed_tools: native required needs
        // exactly one declared tool, rather than an ignored named-choice field.
        self.native
            .retain(|t| self.allowed.contains(t["name"].as_str().unwrap()));
        self.choice = json!({"type":"allowed_tools","mode":mode,"tools":self.native.iter().map(|t| json!({"type":"function","name":t["name"]})).collect::<Vec<_>>()});
        Ok(())
    }
    pub(crate) fn source(&self) -> &Value {
        &self.source
    }
    pub(crate) fn history_version(&self) -> u64 {
        if self.source["custom_as_function"] == true {
            3
        } else {
            2
        }
    }
    pub(crate) fn compile(&self, request: CanonicalRequest) -> ProviderResult<CanonicalRequest> {
        let mut wire = request.wire().clone();
        wire.as_object_mut().unwrap().remove("parallel_tool_calls");
        if self.source["tools"].is_null() {
            wire.as_object_mut().unwrap().remove("tools");
        } else {
            wire["tools"] = self.native.clone().into();
        }
        if self.choice.is_null() {
            wire.as_object_mut().unwrap().remove("tool_choice");
        } else {
            wire["tool_choice"] = self.choice.clone();
        }
        CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
    }
    pub(crate) fn matches(&self, request: &CanonicalRequest) -> ProviderResult<()> {
        let compiled = self.compile(request.clone())?;
        if compiled.wire()["tools"] != request.wire()["tools"]
            || compiled.wire()["tool_choice"] != request.wire()["tool_choice"]
            || request.wire().get("parallel_tool_calls").is_some()
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn compile_item(&self, item: &Value, prefix: &[Value]) -> ProviderResult<Value> {
        let mut item = item.clone();
        let custom = item["type"] == "custom_tool_call";
        if item["type"] == "function_call" || custom {
            fields(
                &item,
                if custom {
                    &[
                        "type",
                        "id",
                        "status",
                        "name",
                        "namespace",
                        "call_id",
                        "input",
                    ]
                } else {
                    &[
                        "type",
                        "id",
                        "status",
                        "name",
                        "namespace",
                        "call_id",
                        "arguments",
                    ]
                },
            )?;
            item["name"] = self.alias(&item)?.to_owned().into();
            item.as_object_mut().unwrap().remove("namespace");
            if custom {
                let input = item["input"].as_str().ok_or_else(invalid)?;
                item["arguments"] = json!({"input":input}).to_string().into();
                item["type"] = "function_call".into();
                item.as_object_mut().unwrap().remove("input");
            }
        } else if matches!(
            item["type"].as_str(),
            Some("function_call_output" | "custom_tool_call_output")
        ) {
            fields(&item, &["type", "id", "status", "call_id", "output"])?;
            let call_id = id(&item["call_id"])?;
            let call = prefix
                .iter()
                .rev()
                .find(|i| i["type"] == "function_call" && i["call_id"] == call_id)
                .ok_or_else(invalid)?;
            if (item["type"] == "custom_tool_call_output")
                != self.mapped_custom.contains(name(&call["name"])?)
            {
                return Err(invalid());
            }
            item["type"] = "function_call_output".into();
            if let Some(parts) = item["output"].as_array() {
                let mut text = Vec::new();
                for part in parts {
                    fields(part, &["type", "text"])?;
                    if !matches!(part["type"].as_str(), Some("input_text" | "output_text")) {
                        return Err(unsupported());
                    }
                    text.push(part["text"].as_str().ok_or_else(invalid)?);
                }
                item["output"] = text.join("\n").into();
            }
        } else {
            return Err(unsupported());
        }
        Ok(item)
    }
    fn call(&self, item: &Value) -> ProviderResult<()> {
        id(&item["call_id"])?;
        if item.get("id").is_some_and(|v| id(v).is_err())
            || item.get("status").is_some_and(|v| v != "completed")
            || item.get("namespace").is_some()
            || !self.identities.values().any(|a| item["name"] == *a)
        {
            return Err(invalid());
        }
        arguments(item, self.mapped_custom.contains(name(&item["name"])?))
    }
    /// Reorder each completed call group into adjacent call/result pairs.
    /// Keep call order and all intervening native display items, never execute.
    pub(crate) fn pair_input(&self, input: &[Value]) -> ProviderResult<Vec<Value>> {
        let mut output = Vec::new();
        let mut seen = HashSet::new();
        let mut calls: Vec<Value> = Vec::new();
        let mut results = HashMap::new();
        let mut extra = Vec::new();
        for item in input {
            if item["type"] == "function_call" {
                self.call(item)?;
                if !seen.insert(id(&item["call_id"])?.to_owned()) {
                    return Err(invalid());
                }
                calls.push(item.clone());
            } else if item["type"] == "function_call_output" {
                let call_id = id(&item["call_id"])?;
                if item.get("id").is_some_and(|v| id(v).is_err())
                    || item.get("status").is_some_and(|v| v != "completed")
                    || !item["output"].is_string()
                    || !calls.iter().any(|c| c["call_id"] == call_id)
                    || results.insert(call_id.to_owned(), item.clone()).is_some()
                {
                    return Err(invalid());
                }
                if results.len() == calls.len() {
                    for call in calls.drain(..) {
                        let result = results.remove(call["call_id"].as_str().unwrap()).unwrap();
                        output.extend([call, result]);
                    }
                    output.append(&mut extra);
                }
            } else if calls.is_empty() {
                output.push(item.clone());
            } else {
                if item.get("role").is_some_and(|v| v != "assistant") {
                    return Err(invalid());
                }
                extra.push(item.clone());
            }
        }
        if !calls.is_empty() {
            return Err(invalid());
        }
        Ok(output)
    }
    pub(crate) fn validate_response(
        &self,
        request: &CanonicalRequest,
        response: &CanonicalResponse,
    ) -> ProviderResult<()> {
        let result = (|| {
            let input = request.wire()["input"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if self.pair_input(&input)? != input {
                return Err(invalid());
            }
            let mut ids: HashSet<String> = input
                .iter()
                .filter(|i| i["type"] == "function_call")
                .map(|i| i["call_id"].as_str().unwrap().to_owned())
                .collect();
            let mut count = 0;
            for item in response
                .output()
                .iter()
                .filter(|i| i["type"] == "function_call")
            {
                self.call(item)?;
                id(&item["id"])?;
                if response.state() != StreamState::Completed
                    || !ids.insert(id(&item["call_id"])?.to_owned())
                    || !self.allowed.contains(name(&item["name"])?)
                {
                    return Err(invalid());
                }
                count += 1;
            }
            if response.state() == StreamState::Completed && self.required && count == 0 {
                return Err(invalid());
            }
            Ok(())
        })();
        result.map_err(|_: ProviderError| ProviderError::new(502, "qwen_invalid_native_tools"))
    }
    pub(crate) fn project(
        &self,
        response: &CanonicalResponse,
    ) -> ProviderResult<CanonicalResponse> {
        let mut wire = response.wire().clone();
        for item in wire["output"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter(|i| i["type"] == "function_call")
        {
            let ((namespace, member), alias) = self
                .identities
                .iter()
                .find(|(_, alias)| item["name"] == alias.as_str())
                .ok_or_else(invalid)?;
            item["name"] = member.clone().into();
            if self.mapped_custom.contains(alias) {
                let arguments: Value =
                    serde_json::from_str(item["arguments"].as_str().ok_or_else(invalid)?)
                        .map_err(|_| invalid())?;
                item["input"] = arguments["input"].as_str().ok_or_else(invalid)?.into();
                item["type"] = "custom_tool_call".into();
                item.as_object_mut().unwrap().remove("arguments");
            }
            if let Some(namespace) = namespace {
                item["namespace"] = namespace.clone().into();
            }
        }
        CanonicalResponse::new(wire).map_err(|_| invalid())
    }
}
