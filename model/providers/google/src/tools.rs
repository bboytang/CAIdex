use caidex_model_core::{ProviderError, ProviderResult, ResponseItem, ToolInput, ToolKind};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_google_tools")
}
#[derive(Clone)]
struct Binding {
    name: String,
    namespace: Option<String>,
    kind: ToolKind,
}
/// Execution-side declarations for one request. Aliases bind identity, not
/// declaration order or schema. This is a codec, never a tool executor.
#[derive(Clone)]
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
                let namespace = identity(&declaration["name"])?;
                let description = match crate::content::present(declaration, "description") {
                    None => "",
                    Some(Value::String(text)) => text,
                    _ => return Err(invalid()),
                };
                for tool in declaration["tools"].as_array().ok_or_else(invalid)? {
                    map.add(tool, Some(namespace), max_tools, &mut identities)?;
                    if !description.is_empty() {
                        let native = map.tools.last_mut().expect("added tool");
                        native["description"] = format!(
                            "Namespace description: {description}\n{}",
                            native["description"].as_str().unwrap()
                        )
                        .into();
                    }
                }
            } else {
                map.add(declaration, None, max_tools, &mut identities)?;
            }
        }
        Ok(map)
    }
    fn add(
        &mut self,
        tool: &Value,
        namespace: Option<&str>,
        max: usize,
        identities: &mut BTreeSet<(Option<String>, String)>,
    ) -> ProviderResult<()> {
        let kind = match tool["type"].as_str() {
            Some("function") => ToolKind::Function,
            Some("custom") => ToolKind::Custom,
            Some("tool_search") => {
                return Err(ProviderError::new(400, "unsupported_google_tool_discovery"));
            }
            Some("web_search") => {
                return Err(ProviderError::new(400, "unsupported_google_web_search"));
            }
            _ => return Err(invalid()),
        };
        let name = identity(&tool["name"])?;
        if self.tools.len() >= max
            || !identities.insert((namespace.map(str::to_owned), name.to_owned()))
        {
            return Err(invalid());
        }
        for field in ["strict", "defer_loading"] {
            if let Some(value) = crate::content::present(tool, field) {
                if !value.is_boolean() {
                    return Err(invalid());
                }
                if value == true {
                    return Err(ProviderError::new(
                        400,
                        match field {
                            "strict" => "unsupported_google_strict_tools",
                            _ => "unsupported_google_tool_discovery",
                        },
                    ));
                }
            }
        }
        let schema = match kind {
            ToolKind::Function => {
                if !tool["parameters"].is_object() || tool["parameters"]["type"] != "object" {
                    return Err(invalid());
                }
                tool["parameters"].clone()
            }
            ToolKind::Custom => {
                if let Some(format) = crate::content::present(tool, "format") {
                    match format["type"].as_str() {
                        Some("text") => (),
                        Some("grammar")
                            if matches!(format["syntax"].as_str(), Some("lark" | "regex"))
                                && format["definition"].as_str().is_some_and(|s| !s.is_empty()) =>
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
            ToolKind::Function => "function",
            ToolKind::Custom => "custom",
        };
        let alias = format!(
            "ct_{:x}",
            Sha256::digest(json!([namespace, name, kind_name]).to_string().as_bytes())
        );
        let description = match crate::content::present(tool, "description") {
            None => "",
            Some(Value::String(text)) => text,
            _ => return Err(invalid()),
        };
        let mut description = format!(
            "Tool identity: {}{name}.\n{description}",
            namespace.map(|s| format!("{s}::")).unwrap_or_default()
        );
        if kind == ToolKind::Custom {
            description.push_str("\nPut the complete tool input unchanged in the input string.");
            if let Some(format) = crate::content::present(tool, "format") {
                description.push_str(&format!(
                    "\nOriginal input format (guidance only): {format}"
                ));
            }
        }
        // parametersJsonSchema avoids lossy conversion to Google's Schema subset.
        self.tools
            .push(json!({"name":alias,"description":description,"parametersJsonSchema":schema}));
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
        Ok(())
    }
    pub fn native_tools(&self) -> &[Value] {
        &self.tools
    }
    pub fn source(&self) -> &[Value] {
        &self.source
    }
    /// Grammar is preserved as prompting guidance; no native grammar guarantee.
    pub fn has_grammar_tools(&self) -> bool {
        self.grammar
    }
    pub fn native_call(&self, item: &ResponseItem) -> ProviderResult<Value> {
        let call = item
            .tool_call()
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        identity(&Value::String(call.call_id.into()))?;
        let (alias, _) = self
            .bindings
            .iter()
            .find(|(_, b)| {
                b.name == call.name
                    && b.namespace.as_deref() == call.namespace
                    && b.kind == call.kind
            })
            .ok_or_else(invalid)?;
        let args = match call.input {
            ToolInput::JsonArguments(text) => {
                let args: Value = serde_json::from_str(text).map_err(|_| invalid())?;
                if !args.is_object() {
                    return Err(invalid());
                }
                args
            }
            ToolInput::Text(text) => json!({"input":text}),
        };
        Ok(json!({"name":alias,"id":call.call_id,"args":args}))
    }
    /// Native ID is optional; the caller supplies the stable generation/Part ID
    /// when absent. Original native Parts and signatures are never modified.
    pub fn responses_call(&self, call: &Value, fallback_id: &str) -> ProviderResult<ResponseItem> {
        if fallback_id.trim().is_empty() || fallback_id.chars().any(char::is_control) {
            return Err(invalid());
        }
        crate::content::validate_content(&json!({"parts":[{"functionCall":call}]}), true)
            .map_err(|_| invalid())?;
        let binding = self
            .bindings
            .get(call["name"].as_str().ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        let args = crate::content::present(call, "args")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let mut item =
            json!({"name":binding.name,"call_id":call["id"].as_str().unwrap_or(fallback_id)});
        if let Some(namespace) = &binding.namespace {
            item["namespace"] = namespace.clone().into();
        }
        match binding.kind {
            ToolKind::Function => {
                item["type"] = "function_call".into();
                item["arguments"] = args.to_string().into();
            }
            ToolKind::Custom => {
                let object = args.as_object().ok_or_else(invalid)?;
                if object.len() != 1 || !object.get("input").is_some_and(Value::is_string) {
                    return Err(invalid());
                }
                item["type"] = "custom_tool_call".into();
                item["input"] = args["input"].clone();
            }
        }
        ResponseItem::new(item).map_err(|_| invalid())
    }
}
fn identity(value: &Value) -> ProviderResult<&str> {
    crate::content::nonempty(value).ok_or_else(invalid)
}
impl fmt::Debug for ToolMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ToolMap([DECLARATIONS OMITTED])")
    }
}
