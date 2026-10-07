use caidex_model_core::{ProviderError, ProviderResult};
use serde_json::{Value, json};

/// Local provenance, not encryption or signature authentication. Keep the
/// compiled native fields, never reconstruct them from rendered Responses.
#[derive(Clone)]
pub(crate) struct ReplayBinding(Value);
impl ReplayBinding {
    pub(crate) fn new(organization: &str, wire: &Value) -> Self {
        // ponytail: full prefix snapshots grow quadratically across a session;
        // request/replay budgets cap them, Host storage can deduplicate later.
        Self(json!({"organization":organization,"prefix":prefix(wire)}))
    }
    pub(crate) fn parse(wire: Value) -> ProviderResult<Self> {
        let invalid = || ProviderError::new(400, "invalid_anthropic_replay");
        if !wire["organization"]
            .as_str()
            .is_some_and(crate::client::valid_organization_id)
            || !wire["prefix"].is_object()
            || !wire["prefix"]["messages"].is_array()
            || wire["prefix"]
                .as_object()
                .unwrap()
                .keys()
                .any(|key| !matches!(key.as_str(), "system" | "tools" | "messages"))
            || wire["prefix"]
                .get("system")
                .is_some_and(|system| !system.is_array())
            || wire["prefix"]
                .get("tools")
                .is_some_and(|tools| !tools.is_array())
        {
            return Err(invalid());
        }
        for message in wire["prefix"]["messages"].as_array().unwrap() {
            if !matches!(
                message["role"].as_str(),
                Some("user" | "assistant" | "system")
            ) || !message["content"].is_array()
            {
                return Err(invalid());
            }
        }
        Ok(Self(wire))
    }
    pub(crate) fn wire(&self) -> &Value {
        &self.0
    }
    pub(crate) fn check_tools(&self, tools: &crate::ToolMap) -> ProviderResult<()> {
        if tool_set(&self.0["prefix"]["tools"]) != tool_set(&json!(tools.native_tools())) {
            return Err(ProviderError::new(400, "invalid_anthropic_replay"));
        }
        Ok(())
    }
    pub(crate) fn check(
        &self,
        wire: &Value,
        end: usize,
        organization: Option<&str>,
    ) -> ProviderResult<()> {
        let Some(organization) = organization else {
            return Err(ProviderError::new(
                400,
                "anthropic_replay_organization_required",
            ));
        };
        if self.0["organization"] != organization {
            return Err(ProviderError::new(
                400,
                "anthropic_replay_organization_mismatch",
            ));
        }
        let old = &self.0["prefix"];
        let messages = &wire["messages"].as_array().expect("compiled messages")[..end];
        let previous = old["messages"].as_array().expect("validated prefix");
        // Earlier thinking is not part of the native prefix, but retained blocks
        // must form a suffix of the chain that preceded this signed turn. This
        // permits removing oldest thinking and rejects gaps/reinsertions.
        if old["system"] != wire["system"]
            || tool_set(&old["tools"]) != tool_set(&wire["tools"])
            || without_thinking(previous) != without_thinking(messages)
            || !signed_blocks(previous).ends_with(&signed_blocks(messages))
        {
            return Err(ProviderError::new(400, "anthropic_replay_prefix_mismatch"));
        }
        Ok(())
    }
}
fn prefix(wire: &Value) -> Value {
    wire.as_object()
        .expect("compiled native request")
        .iter()
        .filter(|(key, _)| matches!(key.as_str(), "system" | "tools" | "messages"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}
fn tool_set(wire: &Value) -> Vec<&Value> {
    let mut tools: Vec<_> = wire.as_array().into_iter().flatten().collect();
    tools.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    tools
}
fn thinking(block: &Value) -> bool {
    matches!(
        block["type"].as_str(),
        Some("thinking" | "redacted_thinking")
    )
}
fn signed_blocks(messages: &[Value]) -> Vec<&Value> {
    messages
        .iter()
        .filter(|message| message["role"] == "assistant")
        .flat_map(|message| message["content"].as_array().expect("compiled content"))
        .filter(|block| thinking(block))
        .collect()
}
fn without_thinking(messages: &[Value]) -> Vec<Value> {
    let mut normalized = Vec::new();
    for message in messages {
        let mut message = message.clone();
        if message["role"] == "assistant" {
            message["content"]
                .as_array_mut()
                .expect("compiled content")
                .retain(|block| !thinking(block));
            if message["content"].as_array().unwrap().is_empty() {
                continue;
            }
        }
        // Current compiler merges adjacent same-role messages. Retain any
        // future message-level fields instead of silently normalizing them away.
        if message.as_object().is_some_and(|object| object.len() == 2)
            && normalized
                .last()
                .is_none_or(|last: &Value| last.as_object().is_some_and(|object| object.len() == 2))
        {
            crate::request::append(
                &mut normalized,
                message["role"].as_str().unwrap(),
                message["content"].as_array().unwrap().clone(),
            );
        } else {
            normalized.push(message);
        }
    }
    normalized
}
