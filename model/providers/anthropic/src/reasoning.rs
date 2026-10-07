use caidex_model_core::{ProviderError, ProviderResult};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_anthropic_reasoning")
}
/// Native model's documented prior-turn thinking retention policy.
/// A profile declaration is not proof of actual model compatibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkingContext {
    CurrentTurn,
    AllTurns,
}
impl ThinkingContext {
    fn wire_value(self) -> &'static str {
        match self {
            Self::CurrentTurn => "current_turn",
            Self::AllTurns => "all_turns",
        }
    }
}
/// Explicit summary intent to native display mapping. Summary scales are not
/// equivalent across providers; no setting changes the native reasoning mode.
#[derive(Debug)]
pub struct SummaryMapping {
    source: String,
    display: String,
}
impl SummaryMapping {
    pub fn new(source: String, display: String) -> ProviderResult<Self> {
        if !matches!(source.as_str(), "auto" | "concise" | "detailed") || display != "summarized" {
            return Err(invalid());
        }
        Ok(Self { source, display })
    }
}
/// Execution-side mapping, not a claim that providers' effort scales are equal.
/// Unknown model capabilities never create an implicit default mapping.
#[derive(Debug)]
pub struct ReasoningMapping {
    source_effort: String,
    native: Value,
}
impl ReasoningMapping {
    pub fn new(
        source_effort: String,
        native_effort: Option<String>,
        thinking: Option<Value>,
    ) -> ProviderResult<Self> {
        if source_effort.trim().is_empty() || source_effort.chars().any(char::is_control) {
            return Err(invalid());
        }
        let mut native = json!({});
        if let Some(effort) = native_effort {
            if !matches!(effort.as_str(), "low" | "medium" | "high" | "xhigh" | "max") {
                return Err(invalid());
            }
            native["output_config"] = json!({"effort":effort});
        }
        if let Some(thinking) = thinking {
            let object = thinking.as_object().ok_or_else(invalid)?;
            if object.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "type" | "budget_tokens" | "display" | "block_binding"
                )
            }) {
                return Err(invalid());
            }
            match thinking["type"].as_str() {
                Some("enabled") => {
                    if thinking["budget_tokens"].as_u64().is_none_or(|v| v < 1024) {
                        return Err(invalid());
                    }
                }
                Some("adaptive" | "between_tools" | "disabled") => {
                    if object.contains_key("budget_tokens") {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
            if let Some(display) = object.get("display")
                && (matches!(
                    thinking["type"].as_str(),
                    Some("disabled" | "between_tools")
                ) || !matches!(display.as_str(), Some("summarized" | "omitted")))
            {
                return Err(invalid());
            }
            if let Some(binding) = object.get("block_binding")
                && (!matches!(thinking["type"].as_str(), Some("adaptive" | "enabled"))
                    || binding.as_object().is_none_or(|fields| fields.len() != 1)
                    || binding["prefix_mismatch_behavior"] != "error")
            {
                return Err(invalid());
            }
            if thinking["type"] == "between_tools"
                && matches!(
                    native["output_config"]["effort"].as_str(),
                    Some("xhigh" | "max")
                )
            {
                return Err(invalid());
            }
            native["thinking"] = thinking;
        }
        if native.as_object().unwrap().is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            source_effort,
            native,
        })
    }
    pub fn source_effort(&self) -> &str {
        &self.source_effort
    }
    pub fn native_parameters(&self) -> &Value {
        &self.native
    }
}
pub(crate) fn apply(
    wire: &mut Value,
    source: &Value,
    options: &crate::RequestOptions<'_>,
    max_tokens: u64,
) -> ProviderResult<()> {
    let mappings = options.reasoning_mappings;
    let mut summaries = BTreeSet::new();
    if options
        .summary_mappings
        .iter()
        .any(|mapping| !summaries.insert(mapping.source.as_str()))
    {
        return Err(invalid());
    }
    let mut identities = BTreeSet::new();
    if mappings
        .iter()
        .any(|mapping| !identities.insert(mapping.source_effort()))
    {
        return Err(invalid());
    }
    let Some(reasoning) = source.get("reasoning").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let object = reasoning.as_object().ok_or_else(invalid)?;
    // Summary verbosity and provider retention are independent semantics. Null
    // known optional fields mean absent; unsupported semantics cannot disappear.
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "effort" | "summary" | "context"))
    {
        return Err(ProviderError::new(400, "unsupported_anthropic_reasoning"));
    }
    if let Some(context) = object.get("context").filter(|v| !v.is_null()) {
        let context = context.as_str().ok_or_else(invalid)?;
        if options
            .thinking_context
            .is_none_or(|policy| policy.wire_value() != context)
        {
            return Err(ProviderError::new(
                400,
                "unsupported_anthropic_reasoning_context",
            ));
        }
    }
    let Some(effort) = object.get("effort").filter(|v| !v.is_null()) else {
        return apply_summary(wire, object.get("summary"), options.summary_mappings);
    };
    let effort = effort.as_str().ok_or_else(invalid)?;
    let mapping = mappings
        .iter()
        .find(|mapping| mapping.source_effort == effort)
        .ok_or_else(|| ProviderError::new(400, "unsupported_anthropic_reasoning"))?;
    let native = mapping.native_parameters();
    let thinking = &native["thinking"];
    if thinking["type"] == "enabled" {
        if thinking["budget_tokens"].as_u64().unwrap() >= max_tokens {
            return Err(invalid());
        }
        if let Some(last) = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|message| message["role"] == "assistant")
            && !matches!(
                last["content"][0]["type"].as_str(),
                Some("thinking" | "redacted_thinking")
            )
        {
            return Err(invalid());
        }
    }
    if thinking.get("type").is_some()
        && thinking["type"] != "disabled"
        && wire["tool_choice"]["type"] == "any"
    {
        return Err(invalid());
    }
    for (key, value) in native.as_object().unwrap() {
        wire[key] = value.clone();
    }
    apply_summary(wire, object.get("summary"), options.summary_mappings)
}
fn apply_summary(
    wire: &mut Value,
    summary: Option<&Value>,
    mappings: &[SummaryMapping],
) -> ProviderResult<()> {
    let Some(summary) = summary.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let summary = summary.as_str().ok_or_else(invalid)?;
    let mapping = mappings
        .iter()
        .find(|mapping| mapping.source == summary)
        .ok_or_else(|| ProviderError::new(400, "unsupported_anthropic_reasoning_summary"))?;
    // A summary request must retain readable summaries. Never fabricate
    // thinking enablement or map it to omitted display.
    if !matches!(
        wire["thinking"]["type"].as_str(),
        Some("adaptive" | "enabled")
    ) {
        return Err(invalid());
    }
    wire["thinking"]["display"] = mapping.display.clone().into();
    Ok(())
}
