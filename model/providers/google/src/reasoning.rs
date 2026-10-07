use caidex_model_core::{ProviderError, ProviderResult};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_google_reasoning")
}
/// Executor-declared native retention policy, not proof of live compatibility.
/// This local policy never removes or reconstructs signed historical Parts.
#[derive(Clone, Copy, Debug)]
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
/// Explicit intent mapping to one native thinking mode. Model-specific budget
/// ranges/level support belong to the executor profile, never model-name guesses.
#[derive(Debug)]
pub struct ReasoningMapping {
    source_effort: String,
    native: Value,
}
impl ReasoningMapping {
    pub fn new(source_effort: String, thinking: Value) -> ProviderResult<Self> {
        let fields = thinking.as_object().ok_or_else(invalid)?;
        if source_effort.trim().is_empty()
            || source_effort.chars().any(char::is_control)
            || fields.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "thinkingBudget" | "thinkingLevel" | "includeThoughts"
                )
            })
            || fields.contains_key("thinkingBudget") == fields.contains_key("thinkingLevel")
        {
            return Err(invalid());
        }
        if let Some(budget) = fields.get("thinkingBudget") {
            // Native integer is int32: -1 dynamic, 0 off, positive token intent.
            if budget
                .as_i64()
                .is_none_or(|v| !(-1..=i64::from(i32::MAX)).contains(&v))
            {
                return Err(invalid());
            }
        }
        if let Some(level) = fields.get("thinkingLevel")
            && !matches!(level.as_str(), Some("MINIMAL" | "LOW" | "MEDIUM" | "HIGH"))
        {
            return Err(invalid());
        }
        if fields
            .get("includeThoughts")
            .is_some_and(|v| !v.is_boolean())
            || (source_effort == "none" && thinking["thinkingBudget"] != 0)
        {
            // MINIMAL means little thinking, not a hard guarantee of off.
            return Err(invalid());
        }
        Ok(Self {
            source_effort,
            native: thinking,
        })
    }
}
/// Explicit display intent. Non-none summaries require includeThoughts=true;
/// verbosity scales differ across providers and do not change the thinking mode.
#[derive(Debug)]
pub struct SummaryMapping {
    source: String,
    include_thoughts: bool,
}
impl SummaryMapping {
    pub fn new(source: String, include_thoughts: bool) -> ProviderResult<Self> {
        if !matches!(source.as_str(), "auto" | "concise" | "detailed" | "none")
            || include_thoughts != (source != "none")
        {
            return Err(invalid());
        }
        Ok(Self {
            source,
            include_thoughts,
        })
    }
}
pub(crate) fn apply(
    wire: &mut Value,
    source: &Value,
    options: &crate::RequestOptions<'_>,
) -> ProviderResult<()> {
    let mut efforts = BTreeSet::new();
    let mut summaries = BTreeSet::new();
    if options
        .reasoning_mappings
        .iter()
        .any(|mapping| !efforts.insert(mapping.source_effort.as_str()))
        || options
            .summary_mappings
            .iter()
            .any(|mapping| !summaries.insert(mapping.source.as_str()))
    {
        return Err(invalid());
    }
    let Some(reasoning) = source.get("reasoning").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let fields = reasoning.as_object().ok_or_else(invalid)?;
    if fields
        .keys()
        .any(|key| !matches!(key.as_str(), "effort" | "summary" | "context"))
    {
        return Err(ProviderError::new(400, "unsupported_google_reasoning"));
    }
    if let Some(context) = fields.get("context").filter(|v| !v.is_null()) {
        let context = context.as_str().ok_or_else(invalid)?;
        if options
            .thinking_context
            .is_none_or(|policy| policy.wire_value() != context)
        {
            return Err(ProviderError::new(
                400,
                "unsupported_google_reasoning_context",
            ));
        }
    }
    let mut thinking = json!({});
    if let Some(effort) = fields.get("effort").filter(|v| !v.is_null()) {
        let effort = effort.as_str().ok_or_else(invalid)?;
        let mapping = options
            .reasoning_mappings
            .iter()
            .find(|mapping| mapping.source_effort == effort)
            .ok_or_else(|| ProviderError::new(400, "unsupported_google_reasoning"))?;
        thinking = mapping.native.clone();
    }
    if let Some(summary) = fields.get("summary").filter(|v| !v.is_null()) {
        let summary = summary.as_str().ok_or_else(invalid)?;
        let mapping = options
            .summary_mappings
            .iter()
            .find(|mapping| mapping.source == summary)
            .ok_or_else(|| ProviderError::new(400, "unsupported_google_reasoning_summary"))?;
        if mapping.include_thoughts && thinking["thinkingBudget"] == 0 {
            return Err(ProviderError::new(
                400,
                "unsupported_google_reasoning_summary",
            ));
        }
        // includeThoughts controls available display only. Summary-only requests
        // leave the model's mode untouched; native defaults are not guessed.
        thinking["includeThoughts"] = mapping.include_thoughts.into();
    }
    if !thinking.as_object().unwrap().is_empty() {
        wire["generationConfig"]["thinkingConfig"] = thinking;
    }
    Ok(())
}
