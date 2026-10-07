use crate::{MessageOutcome, NativeMessage, ToolMap};
use caidex_model_core::{CanonicalResponse, ProviderError, ProviderResult};
use serde_json::{Value, json};

const PREFIX: &str = "caidex.anthropic.native-message.v1:";
const BOUND_PREFIX: &str = "caidex.anthropic.native-message.v3:";
const TOOLS_PREFIX: &str = "caidex.anthropic.native-message.v2:";
fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_anthropic_replay")
}

impl NativeMessage {
    /// CAIdex's Responses boundary only: encrypted_content is an opaque Runtime
    /// carrier, not a claim that this JSON is encrypted or an OpenAI ciphertext.
    /// The original native message is authoritative; display items are views.
    pub fn to_responses(&self, max_replay_bytes: usize) -> ProviderResult<CanonicalResponse> {
        self.project_response(None, None, max_replay_bytes)
    }
    /// Bind tools to the declarations that produced this reply, rather than
    /// reinterpret old native aliases using the next request's tools.
    pub fn to_responses_with_tools(
        &self,
        tools: &ToolMap,
        max_replay_bytes: usize,
    ) -> ProviderResult<CanonicalResponse> {
        self.project_response(Some(tools), None, max_replay_bytes)
    }
    pub(crate) fn to_responses_with_binding(
        &self,
        tools: &ToolMap,
        binding: Option<&crate::binding::ReplayBinding>,
        max_replay_bytes: usize,
    ) -> ProviderResult<CanonicalResponse> {
        self.project_response(Some(tools), binding, max_replay_bytes)
    }
    fn project_response(
        &self,
        tools: Option<&ToolMap>,
        binding: Option<&crate::binding::ReplayBinding>,
        max_replay_bytes: usize,
    ) -> ProviderResult<CanonicalResponse> {
        crate::message::check_input_bindings(self.wire())?;
        let (prefix, envelope) = match (tools, binding) {
            (Some(tools), Some(binding)) => (
                BOUND_PREFIX,
                json!({"provider":"anthropic","version":3,"message":self.wire(),"tools":tools.source(),"binding":binding.wire()}),
            ),
            (Some(tools), None) => (
                TOOLS_PREFIX,
                json!({"provider":"anthropic","version":2,"message":self.wire(),"tools":tools.source()}),
            ),
            (None, None) => (
                PREFIX,
                json!({"provider":"anthropic","version":1,"message":self.wire()}),
            ),
            (None, Some(_)) => unreachable!("bound projection includes tools"),
        };
        let capsule = format!("{prefix}{envelope}");
        if max_replay_bytes == 0 || capsule.len() > max_replay_bytes {
            return Err(ProviderError::new(502, "anthropic_replay_too_large"));
        }
        let mut output = vec![
            json!({"type":"reasoning","id":format!("rs_{}_native",self.id()),
            "summary":self.reasoning_summary(),"encrypted_content":capsule}),
        ];
        output.extend(self.projected_items(tools)?);
        let (status, reason) = match self.outcome() {
            MessageOutcome::EndTurn
            | MessageOutcome::StopSequence
            | MessageOutcome::ToolUse
            | MessageOutcome::Refusal => ("completed", None),
            MessageOutcome::MaxTokens => ("incomplete", Some("max_output_tokens")),
            MessageOutcome::ContextWindowExceeded => {
                ("incomplete", Some("context_window_exceeded"))
            }
            MessageOutcome::PauseTurn => ("incomplete", Some("provider_pause_turn")),
            MessageOutcome::Unknown => ("incomplete", Some("unknown_provider_stop_reason")),
        };
        let mut wire = json!({"id":self.id(),"object":"response","model":self.model(),"status":status,
            "output":output,"usage":self.normalized_usage()?,"caidex_native_stop_reason":self.wire()["stop_reason"],
            "caidex_native_outcome":format!("{:?}",self.outcome())});
        if let Some(reason) = reason {
            wire["incomplete_details"] = json!({"reason":reason});
        }
        CanonicalResponse::new(wire)
            .map_err(|_| ProviderError::new(502, "anthropic_invalid_projection"))
    }
    /// Restore one complete projected native response group. Caller must keep
    /// its model boundary and all matching display/tool items; partial,
    /// mismatching or cross-model groups cannot silently resurrect old native data.
    /// This validates structure/coherence, not the provider's cryptographic signature.
    pub fn from_responses_output(
        output: &[Value],
        expected_model: &str,
        max_replay_bytes: usize,
    ) -> ProviderResult<Self> {
        let item = output.first().ok_or_else(invalid)?;
        if item["type"] != "reasoning" {
            return Err(invalid());
        }
        let (envelope, version) = replay_envelope(item, max_replay_bytes)?;
        let binding = replay_binding(&envelope, version)?;
        let message = Self::parse(envelope["message"].clone()).map_err(|_| invalid())?;
        crate::message::check_input_bindings(message.wire()).map_err(|_| invalid())?;
        if message.model() != expected_model {
            return Err(ProviderError::new(400, "anthropic_replay_model_mismatch"));
        }
        if item["summary"] != message.reasoning_summary() {
            return Err(invalid());
        }
        let tools = if version >= 2 {
            Some(
                ToolMap::new(
                    envelope["tools"].as_array().ok_or_else(invalid)?,
                    max_replay_bytes,
                )
                .map_err(|_| invalid())?,
            )
        } else {
            if envelope.get("tools").is_some() {
                return Err(invalid());
            }
            None
        };
        if let Some(binding) = binding {
            binding.check_tools(tools.as_ref().expect("v3 includes tools"))?;
        }
        let projected = message
            .projected_items(tools.as_ref())
            .map_err(|_| invalid())?;
        if output.len() != projected.len() + 1 {
            return Err(invalid());
        }
        for (actual, expected) in output[1..].iter().zip(projected.iter()) {
            match expected["type"].as_str().unwrap() {
                "message" => {
                    if actual["type"] != "message"
                        || actual["role"] != "assistant"
                        || actual["content"] != expected["content"]
                    {
                        return Err(invalid());
                    }
                }
                "function_call" => {
                    if actual["type"] != "function_call"
                        || actual["call_id"] != expected["call_id"]
                        || actual["name"] != expected["name"]
                        || actual["namespace"] != expected["namespace"]
                    {
                        return Err(invalid());
                    }
                    let actual: Value =
                        serde_json::from_str(actual["arguments"].as_str().ok_or_else(invalid)?)
                            .map_err(|_| invalid())?;
                    let expected: Value =
                        serde_json::from_str(expected["arguments"].as_str().unwrap())
                            .expect("projected native JSON");
                    if actual != expected {
                        return Err(invalid());
                    }
                }
                "custom_tool_call" => {
                    if actual["type"] != "custom_tool_call"
                        || actual["call_id"] != expected["call_id"]
                        || actual["name"] != expected["name"]
                        || actual["namespace"] != expected["namespace"]
                        || actual["input"] != expected["input"]
                    {
                        return Err(invalid());
                    }
                }
                _ => unreachable!("known projection type"),
            }
        }
        Ok(message)
    }
    fn reasoning_summary(&self) -> Value {
        self.content()
            .iter()
            .filter(|block| block["type"] == "thinking")
            .map(|block| json!({"type":"summary_text","text":block["thinking"]}))
            .collect::<Vec<_>>()
            .into()
    }
    fn projected_items(&self, tools: Option<&ToolMap>) -> ProviderResult<Vec<Value>> {
        let mut output = Vec::new();
        let phase = if matches!(
            self.outcome(),
            MessageOutcome::EndTurn | MessageOutcome::StopSequence | MessageOutcome::Refusal
        ) {
            "final_answer"
        } else {
            "commentary"
        };
        for (index, block) in self.projected_blocks() {
            match block["type"].as_str().unwrap() {
                "text" => output.push(json!({"type":"message","id":format!("msg_{}_{index}",self.id()),
                    "role":"assistant","phase":phase,"content":[{"type":"output_text","text":block["text"]}]})),
                "tool_use" if tools.is_some() => {
                    let mut item = tools.unwrap().responses_call(block)?.wire().clone();
                    item["id"] = format!("fc_{}_{index}", self.id()).into();
                    output.push(item);
                }
                "tool_use" => output.push(json!({"type":"function_call","id":format!("fc_{}_{index}",self.id()),
                    "call_id":block["id"],"name":block["name"],"arguments":block["input"].to_string()})),
                // Server tools and future blocks stay native inside the carrier;
                // they never become executable Runtime client tool calls.
                _=>(),
            }
        }
        Ok(output)
    }
    fn projected_blocks(&self) -> impl Iterator<Item = (usize, &Value)> {
        let boundary = self.last_fallback_index();
        self.content()
            .iter()
            .enumerate()
            .filter(move |(index, block)| {
                block["type"] == "text"
                    || (block["type"] == "tool_use"
                        && boundary.is_none_or(|boundary| *index > boundary))
            })
    }
    fn normalized_usage(&self) -> ProviderResult<Value> {
        let raw = &self.wire()["usage"];
        let count = |name| raw[name].as_u64();
        let checked = |a: u64, b: u64| {
            a.checked_add(b)
                .ok_or_else(|| ProviderError::new(502, "anthropic_usage_overflow"))
        };
        // Anthropic's input_tokens excludes cached/read and newly cached input.
        // Missing counters cannot be silently guessed to mean zero.
        let input = match (
            count("input_tokens"),
            count("cache_read_input_tokens"),
            count("cache_creation_input_tokens"),
        ) {
            (Some(a), Some(b), Some(c)) => Some(checked(checked(a, b)?, c)?),
            _ => None,
        };
        let output = count("output_tokens");
        let total = match (input, output) {
            (Some(a), Some(b)) => Some(checked(a, b)?),
            _ => None,
        };
        Ok(
            json!({"input_tokens":input,"output_tokens":output,"total_tokens":total,
            "input_tokens_details":{"cached_tokens":count("cache_read_input_tokens")},
            "caidex_native_usage":raw}),
        )
    }
}

/// Determine the contiguous display group length before full coherence checking.
pub(crate) fn replay_group_len(item: &Value, max_bytes: usize) -> ProviderResult<usize> {
    let (envelope, _) = replay_envelope(item, max_bytes)?;
    let native = NativeMessage::parse(envelope["message"].clone()).map_err(|_| invalid())?;
    Ok(1 + native.projected_blocks().count())
}

pub(crate) fn replay_binding_for_item(
    item: &Value,
    max_bytes: usize,
) -> ProviderResult<Option<crate::binding::ReplayBinding>> {
    let (envelope, version) = replay_envelope(item, max_bytes)?;
    replay_binding(&envelope, version)
}
fn replay_binding(
    envelope: &Value,
    version: u64,
) -> ProviderResult<Option<crate::binding::ReplayBinding>> {
    if version == 3 {
        Ok(Some(crate::binding::ReplayBinding::parse(
            envelope["binding"].clone(),
        )?))
    } else if envelope.get("binding").is_some() {
        Err(invalid())
    } else {
        Ok(None)
    }
}
fn replay_envelope(item: &Value, max_bytes: usize) -> ProviderResult<(Value, u64)> {
    let capsule = item["encrypted_content"].as_str().ok_or_else(invalid)?;
    if max_bytes == 0 || capsule.len() > max_bytes {
        return Err(invalid());
    }
    let (text, version) = if let Some(text) = capsule.strip_prefix(PREFIX) {
        (text, 1)
    } else if let Some(text) = capsule.strip_prefix(TOOLS_PREFIX) {
        (text, 2)
    } else if let Some(text) = capsule.strip_prefix(BOUND_PREFIX) {
        (text, 3)
    } else {
        return Err(invalid());
    };
    let envelope: Value = serde_json::from_str(text).map_err(|_| invalid())?;
    if envelope["provider"] != "anthropic" || envelope["version"] != version {
        return Err(invalid());
    }
    Ok((envelope, version))
}
