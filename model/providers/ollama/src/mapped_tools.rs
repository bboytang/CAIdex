use crate::tools;
use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, ProviderError, ProviderResult, ResponsesDialect,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

type Identity = (Option<String>, String);
fn invalid() -> ProviderError {
    ProviderError::new(400, "ollama_invalid_tool_mapping")
}
fn fields(value: &Value, names: &[&str]) -> ProviderResult<()> {
    if value
        .as_object()
        .ok_or_else(invalid)?
        .keys()
        .any(|name| !names.contains(&name.as_str()))
    {
        return Err(invalid());
    }
    Ok(())
}
fn name(value: &Value) -> ProviderResult<&str> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
        .ok_or_else(invalid)
}
/// Original declarations and their actual native counterparts. Source snapshots
/// bind custom/function kind and client discovery; they are not authentication.
#[derive(Clone)]
pub(crate) struct MappedTools {
    source: Value,
    native: Vec<Value>,
    results: HashMap<String, Vec<Value>>,
    bindings: HashMap<Identity, (Value, bool)>,
}
impl MappedTools {
    pub(crate) fn from_request(request: &CanonicalRequest) -> ProviderResult<Self> {
        let results: Vec<_> = request.wire()["input"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["type"] == "tool_search_output")
            .map(|item| json!({"call_id":item["call_id"],"tools":item["tools"]}))
            .collect();
        Self::from_source(
            json!({"tools":request.wire().get("tools").cloned().unwrap_or_else(||json!([])),"search_results":results}),
        )
    }
    pub(crate) fn from_source(source: Value) -> ProviderResult<Self> {
        fields(&source, &["tools", "search_results"])?;
        let mut map = Self {
            source: source.clone(),
            native: Vec::new(),
            results: HashMap::new(),
            bindings: HashMap::new(),
        };
        map.native = map.compile(&source["tools"], false)?;
        for result in source["search_results"].as_array().ok_or_else(invalid)? {
            fields(result, &["call_id", "tools"])?;
            let id = name(&result["call_id"])?;
            let native = map.compile(&result["tools"], true)?;
            if map.results.insert(id.to_owned(), native).is_some() {
                return Err(invalid());
            }
        }
        Ok(map)
    }
    fn compile(&mut self, declarations: &Value, repeat: bool) -> ProviderResult<Vec<Value>> {
        let mut native = declarations.as_array().ok_or_else(invalid)?.clone();
        let mut batch = HashSet::new();
        for tool in &mut native {
            if tool["type"] == "namespace" {
                fields(tool, &["type", "name", "description", "tools"])?;
                let namespace = name(&tool["name"])?.to_owned();
                for member in tool["tools"].as_array_mut().ok_or_else(invalid)? {
                    self.member(member, Some(&namespace), repeat, &mut batch)?;
                }
            } else if tool["type"] != "tool_search" {
                self.member(tool, None, repeat, &mut batch)?;
            }
        }
        let request = CanonicalRequest::new(
            json!({"model":"mapping","input":"","tools":native}),
            ResponsesDialect::Classic,
        )
        .map_err(|_| invalid())?;
        let request = tools::normalize(request)?;
        // Reuse native declaration/alias validation; no parallel tool registry.
        tools::NativeTools::from_request(&request)?;
        Ok(request.wire()["tools"].as_array().unwrap().clone())
    }
    fn member(
        &mut self,
        tool: &mut Value,
        namespace: Option<&str>,
        repeat: bool,
        batch: &mut HashSet<Identity>,
    ) -> ProviderResult<()> {
        let kind = tool["type"].as_str().ok_or_else(invalid)?;
        if kind != "custom" && kind != "function" {
            return Err(invalid());
        }
        let custom = kind == "custom";
        let identity = (
            namespace.map(str::to_owned),
            name(&tool["name"])?.to_owned(),
        );
        if !batch.insert(identity.clone()) {
            return Err(invalid());
        }
        if let Some((previous, _)) = self.bindings.get(&identity) {
            if !repeat || previous != tool {
                return Err(invalid());
            }
        } else {
            self.bindings.insert(identity, (tool.clone(), custom));
        }
        if !custom {
            return Ok(());
        }
        fields(
            tool,
            &["type", "name", "description", "format", "defer_loading"],
        )?;
        let description = match tool.get("description") {
            None => "",
            Some(Value::String(s)) => s,
            _ => return Err(invalid()),
        };
        let mut guidance = format!(
            "{description}\nPass the complete freeform tool input verbatim as the input string."
        );
        if let Some(format) = tool.get("format").filter(|v| !v.is_null()) {
            match format["type"].as_str() {
                Some("text") => fields(format, &["type"])?,
                Some("grammar") => {
                    fields(format, &["type", "syntax", "definition"])?;
                    let syntax = name(&format["syntax"])?;
                    if !matches!(syntax, "lark" | "regex") {
                        return Err(invalid());
                    }
                    let definition = format["definition"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or_else(invalid)?;
                    guidance.push_str(&format!("\nOriginal {syntax} grammar guidance (not native constrained decoding):\n{definition}"));
                }
                _ => return Err(invalid()),
            }
        }
        let mut native = json!({"type":"function","name":tool["name"],"description":guidance,"parameters":{"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false}});
        if let Some(defer) = tool.get("defer_loading") {
            native["defer_loading"] = defer.clone();
        }
        *tool = native;
        Ok(())
    }
    pub(crate) fn source(&self) -> &Value {
        &self.source
    }
    pub(crate) fn compile_request(
        &self,
        request: CanonicalRequest,
    ) -> ProviderResult<CanonicalRequest> {
        let mut wire = request.wire().clone();
        wire["tools"] = json!(self.native);
        if let Some(input) = wire["input"].as_array_mut() {
            for item in input {
                if item["type"] == "tool_search_output" {
                    item["tools"] = json!(
                        self.results
                            .get(name(&item["call_id"])?)
                            .ok_or_else(invalid)?
                    );
                }
            }
        }
        CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
    }
    fn is_custom(&self, item: &Value) -> ProviderResult<bool> {
        let identity = (
            item.get("namespace")
                .map(name)
                .transpose()?
                .map(str::to_owned),
            name(&item["name"])?.to_owned(),
        );
        self.bindings
            .get(&identity)
            .or_else(|| {
                identity
                    .0
                    .is_none()
                    .then(|| {
                        self.bindings
                            .iter()
                            .find(|((ns, member), _)| {
                                tools::native_name(ns.as_deref(), member) == identity.1
                                    || ns.as_ref().is_some_and(|ns| {
                                        format!("{ns}.{member}") == identity.1
                                            || format!("{ns}:{member}") == identity.1
                                    })
                            })
                            .map(|(_, binding)| binding)
                    })
                    .flatten()
            })
            .map(|(_, custom)| *custom)
            .ok_or_else(invalid)
    }
    /// Only caller-supplied items are converted here. Saved response groups
    /// restore the original native argument string without reserialization.
    pub(crate) fn compile_item(&self, item: &Value, prefix: &[Value]) -> ProviderResult<Value> {
        let mut item = item.clone();
        match item["type"].as_str() {
            Some("custom_tool_call") => {
                if !self.is_custom(&item)?
                    || !item["input"].is_string()
                    || item.get("arguments").is_some()
                {
                    return Err(invalid());
                }
                item["arguments"] = json!({"input":item["input"]}).to_string().into();
                item["type"] = "function_call".into();
                item.as_object_mut().unwrap().remove("input");
            }
            Some("function_call") if self.is_custom(&item)? => return Err(invalid()),
            Some("custom_tool_call_output" | "function_call_output") => {
                let custom = item["type"] == "custom_tool_call_output";
                if let Some(id) = item.get("call_id") {
                    let call = prefix
                        .iter()
                        .rev()
                        .find(|v| v["type"] == "function_call" && v.get("call_id") == Some(id))
                        .ok_or_else(invalid)?;
                    if custom != self.is_custom(call)? {
                        return Err(invalid());
                    }
                } else {
                    if custom {
                        return Err(invalid());
                    }
                    let supplied =
                        tools::native_name(item["namespace"].as_str(), name(&item["name"])?);
                    if prefix
                        .iter()
                        .filter(|v| v["type"] == "function_call")
                        .any(|call| {
                            tools::native_name(
                                call["namespace"].as_str(),
                                call["name"].as_str().unwrap_or(""),
                            ) == supplied
                                && self.is_custom(call).unwrap_or(false)
                        })
                    {
                        return Err(invalid());
                    }
                }
                if custom {
                    item["type"] = "function_call_output".into();
                }
            }
            _ => (),
        }
        Ok(item)
    }
    /// Bind precisely the discovery declarations preceding a saved response,
    /// rather than future declarations from the current complete request.
    pub(crate) fn at_prefix(&self, request: &CanonicalRequest) -> ProviderResult<Self> {
        let results: Vec<_> = request.wire()["input"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["type"] == "tool_search_output")
            .map(|item| {
                self.source["search_results"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["call_id"] == item["call_id"])
                    .cloned()
                    .ok_or_else(invalid)
            })
            .collect::<ProviderResult<_>>()?;
        Self::from_source(json!({"tools":self.source["tools"],"search_results":results}))
    }
    pub(crate) fn matches_request(&self, request: &CanonicalRequest) -> ProviderResult<()> {
        if request.wire()["tools"] != json!(self.native) {
            return Err(invalid());
        }
        let input_results: Vec<_> = request.wire()["input"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["type"] == "tool_search_output")
            .collect();
        if input_results.len() != self.results.len() {
            return Err(invalid());
        }
        for (item, source) in input_results
            .iter()
            .zip(self.source["search_results"].as_array().unwrap())
        {
            if item["call_id"] != source["call_id"]
                || item["tools"]
                    != json!(
                        self.results
                            .get(name(&item["call_id"])?)
                            .ok_or_else(invalid)?
                    )
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub(crate) fn project(
        &self,
        response: &CanonicalResponse,
    ) -> ProviderResult<CanonicalResponse> {
        let mut wire = response.wire().clone();
        for item in wire["output"].as_array_mut().unwrap() {
            if item["type"] != "function_call" {
                continue;
            }
            let identity = (
                item.get("namespace")
                    .map(name)
                    .transpose()?
                    .map(str::to_owned),
                name(&item["name"])?.to_owned(),
            );
            let (_, custom) = self.bindings.get(&identity).ok_or_else(invalid)?;
            if !custom {
                continue;
            }
            let arguments: Value =
                serde_json::from_str(item["arguments"].as_str().ok_or_else(invalid)?)
                    .map_err(|_| invalid())?;
            if arguments.as_object().is_none_or(|m| m.len() != 1) || !arguments["input"].is_string()
            {
                return Err(invalid());
            }
            item["type"] = "custom_tool_call".into();
            item["input"] = arguments["input"].clone();
            item.as_object_mut().unwrap().remove("arguments");
        }
        CanonicalResponse::new(wire).map_err(|_| invalid())
    }
}
