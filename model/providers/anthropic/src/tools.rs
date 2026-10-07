use crate::{string, validate_block};
use caidex_model_core::{ProviderError, ProviderResult, ResponseItem, ToolInput, ToolKind};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_anthropic_tools")
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallKind {
    Callable(ToolKind),
    ClientSearch,
}
#[derive(Clone)]
struct Binding {
    name: String,
    namespace: Option<String>,
    kind: CallKind,
    native: Value,
    available: bool,
}
/// Fixed declarations for one request. Native aliases depend on identity, not
/// list order or schema, so parallel calls and same names stay unambiguous.
#[derive(Clone)]
pub struct ToolMap {
    source: Vec<Value>,
    bindings: BTreeMap<String, Binding>,
    tools: Vec<Value>,
    grammar: bool,
    discoveries: Vec<Value>,
    additions: Vec<Value>,
}
impl ToolMap {
    pub fn new(declarations: &[Value], max_tools: usize) -> ProviderResult<Self> {
        if max_tools == 0 {
            return Err(invalid());
        }
        let mut map = Self {
            source: declarations.to_vec(),
            bindings: BTreeMap::new(),
            tools: Vec::new(),
            grammar: false,
            discoveries: Vec::new(),
            additions: Vec::new(),
        };
        let mut identities = BTreeSet::new();
        for declaration in declarations {
            if declaration["type"] == "namespace" {
                let namespace = identity(declaration, "name")?;
                for tool in declaration["tools"].as_array().ok_or_else(invalid)? {
                    map.add(tool, Some(namespace), max_tools, &mut identities)?;
                }
            } else {
                map.add(declaration, None, max_tools, &mut identities)?;
            }
        }
        Ok(map)
    }
    pub fn native_tools(&self) -> &[Value] {
        &self.tools
    }
    /// Original declarations remain available for owned history. Their future
    /// fields are not interpreted as native API options or execution authority.
    pub fn source(&self) -> &[Value] {
        &self.source
    }
    pub fn has_grammar_tools(&self) -> bool {
        self.grammar
    }
    pub(crate) fn has_client_search(&self) -> bool {
        self.bindings
            .values()
            .any(|binding| binding.kind == CallKind::ClientSearch)
    }
    pub(crate) fn is_client_search(&self, block: &Value) -> bool {
        block["type"] == "tool_use"
            && block["name"]
                .as_str()
                .and_then(|name| self.bindings.get(name))
                .is_some_and(|binding| binding.kind == CallKind::ClientSearch)
    }
    pub(crate) fn needs_discovery(&self) -> bool {
        self.has_client_search() || self.tools.iter().any(|tool| tool["defer_loading"] == true)
    }
    pub(crate) fn discoveries(&self) -> &[Value] {
        &self.discoveries
    }
    pub(crate) fn additions(&self) -> &[Value] {
        &self.additions
    }
    /// Caller must first match this output to an outstanding client search.
    pub(crate) fn load(&mut self, output: &ResponseItem, max: usize) -> ProviderResult<Vec<Value>> {
        let result = output
            .tool_search_output()
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        if !self.has_client_search()
            || result.execution != "client"
            || result.status != "completed"
            || self
                .discoveries
                .iter()
                .any(|prior| prior["call_id"].as_str() == result.call_id)
        {
            return Err(invalid());
        }
        let found = Self::new(result.tools, max)?;
        if found.has_client_search() {
            return Err(invalid());
        }
        let mut staged = self.clone();
        let mut additions = Vec::new();
        for (alias, mut binding) in found.bindings {
            // A discovered declaration is loaded now, even if its Responses
            // source was marked deferred. Never change the original tools list.
            binding
                .native
                .as_object_mut()
                .unwrap()
                .remove("defer_loading");
            binding.available = true;
            let existing = staged.bindings.get(&alias);
            if staged.bindings.values().any(|b| {
                b.kind != CallKind::ClientSearch
                    && b.name == binding.name
                    && b.namespace == binding.namespace
                    && b.kind != binding.kind
            }) {
                return Err(invalid());
            }
            if existing.is_none() && staged.bindings.len() >= max {
                return Err(invalid());
            }
            let block = if let Some(prior) = existing {
                let mut prior_native = prior.native.clone();
                prior_native
                    .as_object_mut()
                    .unwrap()
                    .remove("defer_loading");
                if prior_native == binding.native {
                    if prior.available {
                        continue;
                    }
                    json!({"type":"tool_addition","tool":{"type":"tool_reference","name":alias}})
                } else {
                    json!({"type":"tool_addition","tool":{"type":"tool_definition","definition":binding.native}})
                }
            } else {
                json!({"type":"tool_addition","tool":{"type":"tool_definition","definition":binding.native}})
            };
            additions.push(block);
            staged.bindings.insert(alias, binding);
        }
        staged.grammar |= found.grammar;
        staged.discoveries.push(output.wire().clone());
        staged.additions.extend(additions.clone());
        *self = staged;
        Ok(additions)
    }
    fn add(
        &mut self,
        tool: &Value,
        namespace: Option<&str>,
        max: usize,
        identities: &mut BTreeSet<(Option<String>, String)>,
    ) -> ProviderResult<()> {
        let kind = match tool["type"].as_str() {
            Some("function") => CallKind::Callable(ToolKind::Function),
            Some("custom") => CallKind::Callable(ToolKind::Custom),
            Some("tool_search") if namespace.is_none() && tool["execution"] == "client" => {
                CallKind::ClientSearch
            }
            _ => return Err(invalid()),
        };
        let name = if kind == CallKind::ClientSearch {
            "tool_search"
        } else {
            identity(tool, "name")?
        };
        if self.tools.len() >= max
            || (kind == CallKind::ClientSearch && self.has_client_search())
            || (kind != CallKind::ClientSearch
                && !identities.insert((namespace.map(str::to_owned), name.to_owned())))
        {
            return Err(invalid());
        }
        let schema = match kind {
            CallKind::Callable(ToolKind::Function) | CallKind::ClientSearch => {
                if !tool["parameters"].is_object() || tool["parameters"]["type"] != "object" {
                    return Err(invalid());
                }
                tool["parameters"].clone()
            }
            CallKind::Callable(ToolKind::Custom) => {
                if let Some(format) = tool.get("format").filter(|v| !v.is_null()) {
                    match format["type"].as_str() {
                        Some("text") => (),
                        Some("grammar")
                            if matches!(format["syntax"].as_str(), Some("lark" | "regex"))
                                && string(format, "definition").is_some() =>
                        {
                            self.grammar = true
                        }
                        _ => return Err(invalid()),
                    }
                }
                json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false})
            }
        };
        let kind_name = match kind {
            CallKind::Callable(ToolKind::Function) => "function",
            CallKind::Callable(ToolKind::Custom) => "custom",
            CallKind::ClientSearch => "client_tool_search",
        };
        let alias = format!(
            "ct_{:x}",
            Sha256::digest(json!([namespace, name, kind_name]).to_string().as_bytes())
        );
        let description = match tool.get("description") {
            None | Some(Value::Null) => "",
            Some(Value::String(value)) => value,
            _ => return Err(invalid()),
        };
        let mut description = format!(
            "Tool identity: {}{name}.\n{description}",
            namespace.map(|v| format!("{v}::")).unwrap_or_default()
        );
        if kind == CallKind::Callable(ToolKind::Custom) {
            description.push_str("\nPut the complete tool input unchanged in the input string.");
            if let Some(format) = tool.get("format").filter(|v| !v.is_null()) {
                description.push_str(&format!("\nOriginal input format: {format}"));
            }
        }
        let mut native = json!({"name":alias,"description":description,"input_schema":schema});
        if let Some(strict) = tool.get("strict").filter(|v| !v.is_null()) {
            if !strict.is_boolean() {
                return Err(invalid());
            }
            native["strict"] = strict.clone();
        }
        if let Some(deferred) = tool.get("defer_loading").filter(|v| !v.is_null()) {
            if !deferred.is_boolean() || (kind == CallKind::ClientSearch && deferred == true) {
                return Err(invalid());
            }
            if deferred == true {
                native["defer_loading"] = true.into();
            }
        }
        if self
            .bindings
            .insert(
                alias,
                Binding {
                    name: name.into(),
                    namespace: namespace.map(str::to_owned),
                    kind,
                    available: native["defer_loading"] != true,
                    native: native.clone(),
                },
            )
            .is_some()
        {
            return Err(invalid());
        }
        self.tools.push(native);
        Ok(())
    }
    /// Convert a Responses call into native data; no tool is executed here.
    pub fn native_call(&self, item: &ResponseItem) -> ProviderResult<Value> {
        if let Some(call) = item.tool_search_call().map_err(|_| invalid())? {
            if call.execution != "client" || !call.arguments.is_object() {
                return Err(invalid());
            }
            let (alias, _) = self
                .bindings
                .iter()
                .find(|(_, b)| b.kind == CallKind::ClientSearch)
                .ok_or_else(invalid)?;
            return Ok(
                json!({"type":"tool_use","id":call.call_id.ok_or_else(invalid)?,"name":alias,"input":call.arguments}),
            );
        }
        let call = item
            .tool_call()
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        let (alias, _) = self
            .bindings
            .iter()
            .find(|(_, binding)| {
                binding.available
                    && binding.kind == CallKind::Callable(call.kind)
                    && binding.name == call.name
                    && binding.namespace.as_deref() == call.namespace
            })
            .ok_or_else(invalid)?;
        let input = match call.input {
            ToolInput::JsonArguments(value) => {
                let value: Value = serde_json::from_str(value).map_err(|_| invalid())?;
                if !value.is_object() {
                    return Err(invalid());
                }
                value
            }
            ToolInput::Text(value) => json!({"input":value}),
        };
        Ok(json!({"type":"tool_use","id":call.call_id,"name":alias,"input":input}))
    }
    pub(crate) fn responses_call_start(&self, block: &Value) -> ProviderResult<Value> {
        let binding = self
            .bindings
            .get(block["name"].as_str().ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        let mut placeholder = block.clone();
        placeholder["input"] = match binding.kind {
            CallKind::Callable(ToolKind::Function) | CallKind::ClientSearch => json!({}),
            CallKind::Callable(ToolKind::Custom) => json!({"input":""}),
        };
        Ok(self.responses_call(&placeholder)?.wire().clone())
    }
    /// Exact namespace and custom/function kind are recovered from the fixed
    /// map. A name from outside that map cannot select a Runtime tool.
    pub fn responses_call(&self, block: &Value) -> ProviderResult<ResponseItem> {
        validate_block(block, true).map_err(|_| invalid())?;
        if block["type"] != "tool_use" {
            return Err(invalid());
        }
        let binding = self
            .bindings
            .get(block["name"].as_str().unwrap())
            .ok_or_else(invalid)?;
        if !binding.available {
            return Err(invalid());
        }
        if binding.kind == CallKind::ClientSearch {
            return ResponseItem::new(json!({"type":"tool_search_call","execution":"client","call_id":block["id"],"status":"completed","arguments":block["input"]})).map_err(|_| invalid());
        }
        let mut item = json!({"call_id":block["id"],"name":binding.name});
        if let Some(namespace) = &binding.namespace {
            item["namespace"] = namespace.clone().into();
        }
        match binding.kind {
            CallKind::Callable(ToolKind::Function) => {
                item["type"] = "function_call".into();
                item["arguments"] = block["input"].to_string().into();
            }
            CallKind::Callable(ToolKind::Custom) => {
                let input = block["input"].as_object().unwrap();
                if input.len() != 1 || !input.get("input").is_some_and(Value::is_string) {
                    return Err(invalid());
                }
                item["type"] = "custom_tool_call".into();
                item["input"] = input["input"].clone();
            }
            CallKind::ClientSearch => unreachable!("handled dedicated search"),
        }
        ResponseItem::new(item).map_err(|_| invalid())
    }
}
fn identity<'a>(wire: &'a Value, key: &str) -> ProviderResult<&'a str> {
    string(wire, key)
        .filter(|value| !value.trim().is_empty() && !value.chars().any(char::is_control))
        .ok_or_else(invalid)
}
impl fmt::Debug for ToolMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ToolMap([DECLARATIONS OMITTED])")
    }
}

/// Carry the original client discovery output in a native tool result; loaded
/// definitions are offered separately, in append-only system tool additions.
pub(crate) fn search_result(output: &Value) -> Value {
    json!({"type":"tool_result","tool_use_id":output["call_id"],"content":[{"type":"text","text":output.to_string()}]})
}
