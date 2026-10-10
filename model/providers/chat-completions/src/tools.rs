use caidex_model_core::{ProviderError, ProviderResult};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn invalid() -> ProviderError {
    ProviderError::new(400, "chat_invalid_tools_or_history")
}
pub(crate) fn fields(value: &Value, allowed: &[&str]) -> ProviderResult<()> {
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
pub(crate) fn id(value: &Value) -> ProviderResult<&str> {
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
#[derive(Clone)]
struct Binding {
    name: String,
    namespace: Option<String>,
    custom: bool,
}
#[derive(Clone, Default)]
pub(crate) struct Tools {
    bindings: BTreeMap<String, Binding>,
    pub native: Vec<Value>,
}
impl Tools {
    pub fn new(declarations: &[Value], grammar_prompt_mapping: bool) -> ProviderResult<Self> {
        let mut result = Self::default();
        let mut identities = BTreeSet::new();
        for declaration in declarations {
            let (namespace, members) = if declaration["type"] == "namespace" {
                fields(declaration, &["type", "name", "description", "tools"])?;
                (
                    Some(name(&declaration["name"])?),
                    declaration["tools"]
                        .as_array()
                        .ok_or_else(invalid)?
                        .as_slice(),
                )
            } else {
                (None, std::slice::from_ref(declaration))
            };
            for tool in members {
                fields(
                    tool,
                    &[
                        "type",
                        "name",
                        "description",
                        "parameters",
                        "strict",
                        "format",
                        "defer_loading",
                    ],
                )?;
                let name = name(&tool["name"])?;
                if !identities.insert((namespace, name))
                    || result.native.len() >= 128
                    || tool.get("defer_loading").is_some_and(|v| v != false)
                {
                    return Err(invalid());
                }
                let custom = match tool["type"].as_str() {
                    Some("function") => false,
                    Some("custom") => true,
                    _ => return Err(invalid()),
                };
                if tool.get("description").is_some_and(|v| !v.is_string())
                    || tool.get("strict").is_some_and(|v| !v.is_boolean())
                {
                    return Err(invalid());
                }
                let mut grammar = None;
                let parameters = if custom {
                    if tool.get("parameters").is_some() || tool.get("strict").is_some() {
                        return Err(invalid());
                    }
                    if let Some(format) = tool.get("format")
                        && format != &json!({"type":"text"})
                    {
                        fields(format, &["type", "syntax", "definition"])?;
                        if !grammar_prompt_mapping
                            || format["type"] != "grammar"
                            || !matches!(format["syntax"].as_str(), Some("lark" | "regex"))
                            || format["definition"]
                                .as_str()
                                .is_none_or(|s| s.trim().is_empty())
                        {
                            return Err(ProviderError::new(400, "chat_unsupported_custom_grammar"));
                        }
                        grammar = Some(format.clone());
                    }
                    json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false})
                } else {
                    if !tool["parameters"].is_object() || tool.get("format").is_some() {
                        return Err(invalid());
                    }
                    tool["parameters"].clone()
                };
                let identity = json!([namespace, name, custom]).to_string();
                let alias =
                    format!("caidex_{:x}", Sha256::digest(identity.as_bytes()))[..63].to_owned();
                if result.bindings.contains_key(&alias) {
                    return Err(invalid());
                }
                let mut function = json!({"name":alias, "parameters":parameters});
                for key in ["description", "strict"] {
                    if let Some(value) = tool.get(key) {
                        function[key] = value.clone();
                    }
                }
                function["description"] = format!(
                    "Tool identity: {}{}.\n{}",
                    namespace.map(|ns| format!("{ns}::")).unwrap_or_default(),
                    name,
                    tool["description"].as_str().unwrap_or("")
                )
                .into();
                if let Some(grammar) = grammar {
                    function["description"] = format!(
                        "{}\nInput grammar guidance (not native enforcement): {}\n{}",
                        function["description"].as_str().unwrap(),
                        grammar["syntax"].as_str().unwrap(),
                        grammar["definition"].as_str().unwrap()
                    )
                    .into();
                }
                result
                    .native
                    .push(json!({"type":"function","function":function}));
                result.bindings.insert(
                    alias,
                    Binding {
                        name: name.into(),
                        namespace: namespace.map(str::to_owned),
                        custom,
                    },
                );
            }
        }
        Ok(result)
    }
    fn alias(&self, item: &Value) -> ProviderResult<&str> {
        let namespace = item.get("namespace").map(name).transpose()?;
        let name = name(&item["name"])?;
        self.bindings
            .iter()
            .find(|(_, b)| {
                b.name == name
                    && b.namespace.as_deref() == namespace
                    && b.custom == (item["type"] == "custom_tool_call" || item["type"] == "custom")
            })
            .map(|(a, _)| a.as_str())
            .ok_or_else(invalid)
    }
    pub fn choice(&self, choice: &Value) -> ProviderResult<Value> {
        if let Some(s) = choice
            .as_str()
            .filter(|s| matches!(*s, "none" | "auto" | "required"))
        {
            return Ok(s.into());
        }
        fields(choice, &["type", "name", "namespace"])?;
        if !matches!(choice["type"].as_str(), Some("function" | "custom")) {
            return Err(invalid());
        }
        let alias = self.alias(choice)?;
        Ok(json!({"type":"function","function":{"name":alias}}))
    }
    pub fn call(&self, item: &Value) -> ProviderResult<Value> {
        fields(
            item,
            &[
                "type",
                "id",
                "status",
                "call_id",
                "name",
                "namespace",
                "arguments",
                "input",
            ],
        )?;
        if item.get("status").is_some_and(|v| v != "completed") {
            return Err(invalid());
        }
        let alias = self.alias(item)?;
        let binding = &self.bindings[alias];
        let arguments = if binding.custom {
            if item.get("arguments").is_some() {
                return Err(invalid());
            }
            json!({"input":item["input"].as_str().ok_or_else(invalid)?}).to_string()
        } else {
            if item.get("input").is_some() {
                return Err(invalid());
            }
            let args = item["arguments"].as_str().ok_or_else(invalid)?;
            if !serde_json::from_str::<Value>(args)
                .map_err(|_| invalid())?
                .is_object()
            {
                return Err(invalid());
            }
            args.to_owned()
        };
        Ok(
            json!({"id":id(&item["call_id"])?, "type":"function", "function":{"name":alias,"arguments":arguments}}),
        )
    }
    pub fn output(&self, call: &Value) -> ProviderResult<Value> {
        let fail = || ProviderError::new(502, "chat_invalid_native_tool_call");
        fields(call, &["id", "type", "function"]).map_err(|_| fail())?;
        fields(&call["function"], &["name", "arguments"]).map_err(|_| fail())?;
        if call["type"] != "function" {
            return Err(fail());
        }
        let binding = self
            .bindings
            .get(call["function"]["name"].as_str().ok_or_else(fail)?)
            .ok_or_else(fail)?;
        let arguments = call["function"]["arguments"].as_str().ok_or_else(fail)?;
        let parsed: Value = serde_json::from_str(arguments).map_err(|_| fail())?;
        if !parsed.is_object() {
            return Err(fail());
        }
        let call_id = id(&call["id"]).map_err(|_| fail())?;
        let mut item = json!({"type":if binding.custom {"custom_tool_call"} else {"function_call"},"id":format!("ct_{call_id}"),"status":"completed","call_id":call_id,"name":binding.name});
        if binding.custom {
            fields(&parsed, &["input"]).map_err(|_| fail())?;
            item["input"] = parsed["input"].as_str().ok_or_else(fail)?.into();
        } else {
            item["arguments"] = arguments.into();
        }
        if let Some(namespace) = &binding.namespace {
            item["namespace"] = namespace.clone().into();
        }
        Ok(item)
    }
}
