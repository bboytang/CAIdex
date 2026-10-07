use crate::{MessageOutcome, NativeMessage};
use caidex_model_core::{CanonicalResponse, ProviderError, ProviderResult};
use serde_json::{Value, json};

const PREFIX: &str = "caidex.anthropic.native-message.v1:";
fn invalid() -> ProviderError {
    ProviderError::new(400, "invalid_anthropic_replay")
}

impl NativeMessage {
    /// CAIdex's Responses boundary only: encrypted_content is an opaque Runtime
    /// carrier, not a claim that this JSON is encrypted or an OpenAI ciphertext.
    /// The original native message is authoritative; display items are views.
    pub fn to_responses(&self, max_replay_bytes: usize) -> ProviderResult<CanonicalResponse> {
        let capsule = format!(
            "{PREFIX}{}",
            json!({"provider":"anthropic","version":1,"message":self.wire()})
        );
        if max_replay_bytes == 0 || capsule.len() > max_replay_bytes {
            return Err(ProviderError::new(502, "anthropic_replay_too_large"));
        }
        let mut output = vec![
            json!({"type":"reasoning","id":format!("rs_{}_native",self.id()),
            "summary":self.reasoning_summary(),"encrypted_content":capsule}),
        ];
        output.extend(self.projected_items());
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
        let capsule = item["encrypted_content"].as_str().ok_or_else(invalid)?;
        if max_replay_bytes == 0 || capsule.len() > max_replay_bytes {
            return Err(invalid());
        }
        let text = capsule.strip_prefix(PREFIX).ok_or_else(invalid)?;
        let envelope: Value = serde_json::from_str(text).map_err(|_| invalid())?;
        if envelope["provider"] != "anthropic" || envelope["version"] != 1 {
            return Err(invalid());
        }
        let message = Self::parse(envelope["message"].clone()).map_err(|_| invalid())?;
        if message.model() != expected_model {
            return Err(ProviderError::new(400, "anthropic_replay_model_mismatch"));
        }
        if item["summary"] != message.reasoning_summary() {
            return Err(invalid());
        }
        let projected = message.projected_items();
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
                        || actual.get("namespace").is_some_and(|v| !v.is_null())
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
    fn projected_items(&self) -> Vec<Value> {
        let mut output = Vec::new();
        let phase = if matches!(
            self.outcome(),
            MessageOutcome::EndTurn | MessageOutcome::StopSequence | MessageOutcome::Refusal
        ) {
            "final_answer"
        } else {
            "commentary"
        };
        for (index, block) in self.content().iter().enumerate() {
            match block["type"].as_str().unwrap() {
                "text" => output.push(json!({"type":"message","id":format!("msg_{}_{index}",self.id()),
                    "role":"assistant","phase":phase,"content":[{"type":"output_text","text":block["text"]}]})),
                "tool_use" => output.push(json!({"type":"function_call","id":format!("fc_{}_{index}",self.id()),
                    "call_id":block["id"],"name":block["name"],"arguments":block["input"].to_string()})),
                // Server tools and future blocks stay native inside the carrier;
                // they never become executable Runtime client tool calls.
                _=>(),
            }
        }
        output
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
