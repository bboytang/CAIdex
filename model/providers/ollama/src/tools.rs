use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, ProviderError, ProviderResult, StreamState,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

type Identity = (Option<String>, String);
fn invalid() -> ProviderError {
    ProviderError::new(400, "ollama_invalid_tools")
}
fn unsupported() -> ProviderError {
    ProviderError::new(400, "ollama_unsupported_request")
}
fn fields(value: &Value, names: &[&str]) -> ProviderResult<()> {
    if value
        .as_object()
        .ok_or_else(invalid)?
        .keys()
        .any(|name| !names.contains(&name.as_str()))
    {
        return Err(unsupported());
    }
    Ok(())
}
fn name(value: &Value) -> ProviderResult<&str> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
        .ok_or_else(invalid)
}

pub(crate) fn native_name(namespace: Option<&str>, member: &str) -> String {
    let Some(namespace) = namespace else {
        return member.to_owned();
    };
    if member.starts_with(&format!("{namespace}.")) || member.starts_with(&format!("{namespace}_"))
    {
        member.to_owned()
    } else if member.starts_with('_') {
        format!("{namespace}{member}")
    } else {
        format!("{namespace}.{member}")
    }
}

/// Compile namespace guidance before native prefix binding. The native decoder
/// ignores the wrapper description; member descriptions carry that intent.
pub(crate) fn normalize(request: CanonicalRequest) -> ProviderResult<CanonicalRequest> {
    let mut wire = request.wire().clone();
    if let Some(tools) = wire.get_mut("tools") {
        descriptions(tools)?;
    }
    if let Some(items) = wire["input"].as_array_mut() {
        for item in items
            .iter_mut()
            .filter(|v| v["type"] == "tool_search_output")
        {
            descriptions(&mut item["tools"])?;
        }
    }
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}
fn descriptions(tools: &mut Value) -> ProviderResult<()> {
    for tool in tools.as_array_mut().ok_or_else(invalid)? {
        if tool["type"] != "namespace" {
            continue;
        }
        fields(tool, &["type", "name", "description", "tools"])?;
        name(&tool["name"])?;
        let description = match tool.get("description") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            _ => return Err(invalid()),
        };
        for member in tool["tools"].as_array_mut().ok_or_else(invalid)? {
            if member["type"] != "function" {
                return Err(unsupported());
            }
            if !description.is_empty() {
                let original = match member.get("description") {
                    None => "",
                    Some(Value::String(s)) => s,
                    _ => return Err(invalid()),
                };
                member["description"] =
                    format!("Namespace description: {description}\n{original}").into();
            }
        }
        tool.as_object_mut().unwrap().remove("description");
    }
    Ok(())
}

/// One native request's declared identities, including preceding client search
/// results. No tool execution, opaque aliases, or cross-provider dependency.
pub(crate) struct NativeTools {
    functions: HashMap<Identity, Value>,
    aliases: HashMap<String, Identity>,
    search: bool,
    call_ids: HashSet<String>,
    mapping: Option<crate::mapped_tools::MappedTools>,
}
impl NativeTools {
    pub(crate) fn from_request(request: &CanonicalRequest) -> ProviderResult<Self> {
        let wire = request.wire();
        let mut tools = Self {
            functions: HashMap::new(),
            aliases: HashMap::new(),
            search: false,
            call_ids: HashSet::new(),
            mapping: None,
        };
        if let Some(declarations) = wire.get("tools") {
            tools.add(declarations, false, true)?;
        }
        let mut pending: HashMap<String, Option<Identity>> = HashMap::new();
        let mut seen = HashSet::new();
        if let Some(items) = wire["input"].as_array() {
            for item in items {
                match item["type"].as_str() {
                    Some("function_call") => {
                        fields(
                            item,
                            &[
                                "type",
                                "id",
                                "status",
                                "name",
                                "namespace",
                                "call_id",
                                "arguments",
                            ],
                        )?;
                        let identity = tools.call(item)?;
                        let id = name(&item["call_id"])?;
                        if !seen.insert(id.to_owned()) {
                            return Err(invalid());
                        }
                        pending.insert(id.to_owned(), Some(identity));
                    }
                    Some("function_call_output") => {
                        fields(
                            item,
                            &[
                                "type",
                                "id",
                                "status",
                                "name",
                                "namespace",
                                "call_id",
                                "output",
                            ],
                        )?;
                        let explicit = if item.get("name").is_some() {
                            Some(tools.identity(item)?)
                        } else {
                            if item.get("namespace").is_some() {
                                return Err(invalid());
                            }
                            None
                        };
                        let id = if let Some(id) = item.get("call_id") {
                            name(id)?.to_owned()
                        } else {
                            let matches: Vec<_> = pending
                                .iter()
                                .filter(|(_, v)| {
                                    explicit.is_some() && v.as_ref() == explicit.as_ref()
                                })
                                .collect();
                            if matches.len() != 1 {
                                return Err(invalid());
                            }
                            matches[0].0.clone()
                        };
                        let expected = pending
                            .remove(&id)
                            .ok_or_else(invalid)?
                            .ok_or_else(invalid)?;
                        if explicit
                            .as_ref()
                            .is_some_and(|identity| identity != &expected)
                        {
                            return Err(invalid());
                        }
                        if item.get("call_id").is_none()
                            && native_name(item["namespace"].as_str(), name(&item["name"])?)
                                != native_name(expected.0.as_deref(), &expected.1)
                        {
                            // Named native results bypass call-ID matching. Its
                            // ToolName must exactly match the native call name.
                            return Err(invalid());
                        }
                    }
                    Some("tool_search_call") => {
                        fields(
                            item,
                            &["type", "id", "status", "execution", "call_id", "arguments"],
                        )?;
                        tools.search_call(item)?;
                        let id = name(&item["call_id"])?;
                        if !seen.insert(id.to_owned()) {
                            return Err(invalid());
                        }
                        pending.insert(id.to_owned(), None);
                    }
                    Some("tool_search_output") => {
                        fields(
                            item,
                            &["type", "id", "status", "execution", "call_id", "tools"],
                        )?;
                        if item["execution"] != "client"
                            || item["status"] != "completed"
                            || pending
                                .remove(name(&item["call_id"])?)
                                .ok_or_else(invalid)?
                                .is_some()
                        {
                            return Err(invalid());
                        }
                        tools.add(&item["tools"], true, false)?;
                    }
                    _ => (),
                }
            }
        }
        if !pending.is_empty() {
            return Err(invalid());
        }
        tools.call_ids = seen;
        Ok(tools)
    }
    fn add(&mut self, declarations: &Value, repeat: bool, root: bool) -> ProviderResult<()> {
        let mut batch = HashSet::new();
        for tool in declarations.as_array().ok_or_else(invalid)? {
            if tool["type"] == "tool_search" {
                fields(
                    tool,
                    &["type", "name", "execution", "description", "parameters"],
                )?;
                if !root
                    || self.search
                    || tool["execution"] != "client"
                    || tool.get("name").is_some_and(|v| v != "tool_search")
                {
                    return Err(unsupported());
                }
                Self::function_shape(tool)?;
                self.search = true;
            } else if tool["type"] == "namespace" {
                fields(tool, &["type", "name", "tools"])?;
                let namespace = name(&tool["name"])?;
                for member in tool["tools"].as_array().ok_or_else(invalid)? {
                    self.function(member, Some(namespace), repeat, &mut batch)?;
                }
            } else {
                self.function(tool, None, repeat, &mut batch)?;
            }
        }
        if self.search && self.aliases.contains_key("tool_search") {
            return Err(invalid());
        }
        Ok(())
    }
    fn function_shape(tool: &Value) -> ProviderResult<()> {
        if !tool["parameters"].is_object()
            || tool.get("description").is_some_and(|v| !v.is_string())
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn function(
        &mut self,
        tool: &Value,
        namespace: Option<&str>,
        repeat: bool,
        batch: &mut HashSet<Identity>,
    ) -> ProviderResult<()> {
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
        if tool["type"] != "function" {
            return Err(unsupported());
        }
        Self::function_shape(tool)?;
        for flag in ["strict", "defer_loading"] {
            if tool.get(flag).is_some_and(|v| !v.is_null() && v != false) {
                return Err(unsupported());
            }
        }
        let member = name(&tool["name"])?;
        let identity = (namespace.map(str::to_owned), member.to_owned());
        if !batch.insert(identity.clone()) {
            return Err(invalid());
        }
        if let Some(previous) = self.functions.get(&identity) {
            if !repeat || previous != tool {
                return Err(invalid());
            }
            return Ok(());
        }
        let aliases = if let Some(namespace) = namespace {
            let qualified = native_name(Some(namespace), member);
            vec![
                qualified,
                format!("{namespace}.{member}"),
                format!("{namespace}:{member}"),
            ]
        } else {
            vec![member.to_owned()]
        };
        for alias in aliases {
            if self
                .aliases
                .get(&alias)
                .is_some_and(|previous| previous != &identity)
            {
                return Err(invalid());
            }
            self.aliases.insert(alias, identity.clone());
        }
        self.functions.insert(identity, tool.clone());
        Ok(())
    }
    fn identity(&self, item: &Value) -> ProviderResult<Identity> {
        let member = name(&item["name"])?;
        if let Some(namespace) = item.get("namespace") {
            let identity = (Some(name(namespace)?.to_owned()), member.to_owned());
            if !self.functions.contains_key(&identity) {
                return Err(invalid());
            }
            Ok(identity)
        } else {
            self.aliases.get(member).cloned().ok_or_else(invalid)
        }
    }
    fn call(&self, item: &Value) -> ProviderResult<Identity> {
        let arguments: Value =
            serde_json::from_str(item["arguments"].as_str().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
        if !arguments.is_object() {
            return Err(invalid());
        }
        self.identity(item)
    }
    fn search_call(&self, item: &Value) -> ProviderResult<()> {
        if !self.search || item["execution"] != "client" || !item["arguments"].is_object() {
            return Err(invalid());
        }
        name(&item["call_id"])?;
        Ok(())
    }
    pub(crate) fn validate_response(&self, response: &CanonicalResponse) -> ProviderResult<()> {
        let mut ids = self.call_ids.clone();
        let mut item_ids = HashSet::new();
        (|| -> ProviderResult<()> {
            if self
                .mapping
                .as_ref()
                .is_some_and(|mapping| mapping.source()["lite_single_tool_call"] == true)
                && response
                    .output()
                    .iter()
                    .filter(|item| {
                        matches!(
                            item["type"].as_str(),
                            Some("function_call" | "tool_search_call")
                        )
                    })
                    .count()
                    > 1
            {
                return Err(invalid());
            }
            for item in response.output() {
                if let Some(id) = item.get("id").and_then(Value::as_str)
                    && !item_ids.insert(id.to_owned())
                {
                    return Err(invalid());
                }
                match item["type"].as_str() {
                    Some("function_call") => {
                        let identity = self.call(item)?;
                        if item["namespace"].as_str() != identity.0.as_deref()
                            || item["name"].as_str() != Some(identity.1.as_str())
                        {
                            return Err(invalid());
                        }
                    }
                    Some("tool_search_call") => self.search_call(item)?,
                    Some("custom_tool_call" | "web_search_call" | "tool_search_output") => {
                        return Err(invalid());
                    }
                    _ => continue,
                }
                name(&item["id"])?;
                if !ids.insert(name(&item["call_id"])?.to_owned())
                    || response.state() == StreamState::Completed
                        && item.get("status").is_some_and(|v| v != "completed")
                {
                    return Err(invalid());
                }
            }
            if let Some(mapping) = &self.mapping {
                mapping.project(response)?;
            }
            Ok(())
        })()
        .map_err(|_| ProviderError::new(502, "ollama_invalid_native_tools"))
    }
    pub(crate) fn set_mapping(&mut self, mapping: crate::mapped_tools::MappedTools) {
        self.mapping = Some(mapping);
    }
    pub(crate) fn mapping(&self) -> Option<&crate::mapped_tools::MappedTools> {
        self.mapping.as_ref()
    }
}
