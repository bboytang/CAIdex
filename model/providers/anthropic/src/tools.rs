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
struct Binding {
    name: String,
    namespace: Option<String>,
    kind: ToolKind,
}
/// Fixed declarations for one request. Native aliases depend on identity, not
/// list order or schema, so parallel calls and same names stay unambiguous.
pub struct ToolMap {
    source: Vec<Value>,
    bindings: BTreeMap<String, Binding>,
    tools: Vec<Value>,
    grammar: bool,
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
    fn add(
        &mut self,
        tool: &Value,
        namespace: Option<&str>,
        max: usize,
        identities: &mut BTreeSet<(Option<String>, String)>,
    ) -> ProviderResult<()> {
        let name = identity(tool, "name")?;
        let kind = match tool["type"].as_str() {
            Some("function") => ToolKind::Function,
            Some("custom") => ToolKind::Custom,
            _ => return Err(invalid()),
        };
        if self.tools.len() >= max
            || !identities.insert((namespace.map(str::to_owned), name.to_owned()))
        {
            return Err(invalid());
        }
        let schema = match kind {
            ToolKind::Function => {
                if !tool["parameters"].is_object() || tool["parameters"]["type"] != "object" {
                    return Err(invalid());
                }
                tool["parameters"].clone()
            }
            ToolKind::Custom => {
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
        let kind_name = if kind == ToolKind::Function {
            "function"
        } else {
            "custom"
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
        if kind == ToolKind::Custom {
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
        if self
            .bindings
            .insert(
                alias,
                Binding {
                    name: name.into(),
                    namespace: namespace.map(str::to_owned),
                    kind,
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
        let call = item
            .tool_call()
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        let (alias, _) = self
            .bindings
            .iter()
            .find(|(_, binding)| {
                binding.kind == call.kind
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
            ToolKind::Function => json!({}),
            ToolKind::Custom => json!({"input":""}),
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
        let mut item = json!({"call_id":block["id"],"name":binding.name});
        if let Some(namespace) = &binding.namespace {
            item["namespace"] = namespace.clone().into();
        }
        match binding.kind {
            ToolKind::Function => {
                item["type"] = "function_call".into();
                item["arguments"] = block["input"].to_string().into();
            }
            ToolKind::Custom => {
                let input = block["input"].as_object().unwrap();
                if input.len() != 1 || !input.get("input").is_some_and(Value::is_string) {
                    return Err(invalid());
                }
                item["type"] = "custom_tool_call".into();
                item["input"] = input["input"].clone();
            }
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
