use crate::{CandidateOutcome, ContentStream, NativeResponse, NativeStreamResponse};
use caidex_model_core::{CanonicalResponse, ProviderError, ProviderResult};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::HashSet, fmt};

const PREFIX: &str = "caidex.google.native-history.v1:";
const TOOLS_PREFIX: &str = "caidex.google.native-history.v2:";
fn invalid() -> ProviderError {
    ProviderError::new(400, "google_invalid_history")
}
fn replay_error() -> ProviderError {
    ProviderError::new(400, "invalid_google_replay")
}
/// Sensitive native history. The Runtime encrypted_content field is a JSON
/// carrier, not encryption or cryptographic authentication. JSON/chunks remain
/// authoritative; the selected candidate's display items are only projections.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct NativeHistory(Value);
impl NativeHistory {
    /// Explicit mapped projection: own the declarations that produced the
    /// response, and verify them against the actual native request. Plain v1
    /// constructors/projections keep their original behavior.
    pub fn with_tools(mut self, tools: &crate::ToolMap, max_bytes: usize) -> ProviderResult<Self> {
        self.0["version"] = 2.into();
        self.0["tools"] = json!(tools.source());
        Self::new(self.0, max_bytes)
    }
    pub fn from_response(
        response: &NativeResponse,
        model: &str,
        request: &Value,
        candidate_index: Option<u64>,
        generation_id: &str,
        max_bytes: usize,
    ) -> ProviderResult<Self> {
        Self::new(
            json!({"provider":"google","version":1,"source":"json","model":model,
            "request":request,"candidate_index":candidate_index,"generation_id":generation_id,
            "response":response.wire()}),
            max_bytes,
        )
    }
    pub fn from_stream(
        response: &NativeStreamResponse,
        model: &str,
        request: &Value,
        candidate_index: Option<u64>,
        generation_id: &str,
        max_bytes: usize,
    ) -> ProviderResult<Self> {
        Self::new(
            json!({"provider":"google","version":1,"source":"sse","model":model,
            "request":request,"candidate_index":candidate_index,"generation_id":generation_id,
            "response":response.response().wire(),"chunks":response.chunks()}),
            max_bytes,
        )
    }
    fn new(wire: Value, max_bytes: usize) -> ProviderResult<Self> {
        let history = Self(wire);
        history.check_size(max_bytes)?;
        if history.0["provider"] != "google"
            || !matches!(history.0["version"].as_u64(), Some(1 | 2))
            || crate::content::nonempty(&history.0["generation_id"]).is_none()
        {
            return Err(invalid());
        }
        if let Some(tools) = history.mapped_tools(max_bytes)? {
            let native_tools = crate::content::present(history.request(), "tools");
            if !(tools.native_tools().is_empty() && native_tools.is_none_or(|v| v == &json!([])))
                && native_tools != Some(&json!([{"functionDeclarations":tools.native_tools()}]))
            {
                return Err(ProviderError::new(400, "google_history_tool_mismatch"));
            }
        }
        let count = crate::client::validate_generation_request(
            history.0["model"].as_str().ok_or_else(invalid)?,
            history.request(),
            true,
        )
        .map_err(|_| invalid())?;
        let response =
            NativeResponse::parse(history.native_response().clone()).map_err(|_| invalid())?;
        if response.blocked_prompt().is_none()
            && (response.candidates().len() != count
                || response
                    .candidates()
                    .iter()
                    .any(|c| c["index"].as_u64().unwrap_or(0) >= count as u64))
        {
            return Err(invalid());
        }
        match crate::content::present(&history.0, "candidate_index") {
            Some(index) if index.as_u64().is_some() && history.selected_candidate().is_some() => (),
            None if response.blocked_prompt().is_some() => (),
            _ => return Err(invalid()),
        }
        match history.0["source"].as_str() {
            Some("json") if history.0.get("chunks").is_none() => (),
            Some("sse") => {
                // The capsule bounds serialized input. Framing adds bytes, so
                // do not mistake re-encoding overhead for original HTTP budget.
                let mut parser =
                    ContentStream::new(max_bytes, usize::MAX, count).map_err(|_| invalid())?;
                for chunk in history.chunks().ok_or_else(invalid)? {
                    parser
                        .push(format!("data: {chunk}\n\n").as_bytes())
                        .map_err(|_| invalid())?;
                }
                parser.finish().map_err(|_| invalid())?;
                if parser
                    .completed_response()
                    .expect("finished history")
                    .response()
                    .wire()
                    != history.native_response()
                {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
        Ok(history)
    }
    fn check_size(&self, max_bytes: usize) -> ProviderResult<()> {
        if max_bytes == 0
            || self.0.to_string().len().saturating_add(self.prefix().len()) > max_bytes
        {
            return Err(ProviderError::new(502, "google_history_too_large"));
        }
        Ok(())
    }
    fn prefix(&self) -> &'static str {
        if self.0["version"] == 2 {
            TOOLS_PREFIX
        } else {
            PREFIX
        }
    }
    fn mapped_tools(&self, max_bytes: usize) -> ProviderResult<Option<crate::ToolMap>> {
        if self.0["version"] == 2 {
            Ok(Some(
                crate::ToolMap::new(self.0["tools"].as_array().ok_or_else(invalid)?, max_bytes)
                    .map_err(|_| invalid())?,
            ))
        } else if self.0.get("tools").is_some() {
            Err(invalid())
        } else {
            Ok(None)
        }
    }
    pub fn native_response(&self) -> &Value {
        &self.0["response"]
    }
    pub fn request(&self) -> &Value {
        &self.0["request"]
    }
    pub fn chunks(&self) -> Option<&[Value]> {
        self.0["chunks"].as_array().map(Vec::as_slice)
    }
    fn selected_candidate(&self) -> Option<&Value> {
        let index = self.0["candidate_index"].as_u64()?;
        self.native_response()["candidates"]
            .as_array()?
            .iter()
            .find(|candidate| candidate["index"].as_u64().unwrap_or(0) == index)
    }
    /// Only nonempty selected native Content is eligible for a subsequent
    /// contents entry. Original Parts/signatures stay at their returned positions.
    pub fn replay_content(&self) -> Option<&Value> {
        self.selected_candidate()
            .and_then(|c| crate::content::present(c, "content"))
            .filter(|c| c["parts"].as_array().is_some_and(|parts| !parts.is_empty()))
    }
    pub fn to_responses(&self, max_bytes: usize) -> ProviderResult<CanonicalResponse> {
        self.check_size(max_bytes)?;
        let tools = self.mapped_tools(max_bytes)?;
        let native =
            NativeResponse::parse(self.native_response().clone()).expect("validated history");
        let position = native.candidates().iter().position(|c| {
            c["index"].as_u64().unwrap_or(0)
                == self.0["candidate_index"].as_u64().unwrap_or(u64::MAX)
        });
        let outcome = position.and_then(|p| native.outcome(p));
        let mut summary = Vec::new();
        let mut items = Vec::new();
        let mut ids = HashSet::new();
        let id = self.0["generation_id"].as_str().expect("validated ID");
        let parts = self
            .replay_content()
            .and_then(|c| c["parts"].as_array())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let phase = if outcome == Some(CandidateOutcome::Stop) {
            "final_answer"
        } else {
            "commentary"
        };
        for (index, part) in parts.iter().enumerate() {
            if part["thought"] == true {
                if let Some(text) = part["text"].as_str() {
                    summary.push(json!({"type":"summary_text","text":text}));
                }
            } else if let Some(text) = part["text"].as_str() {
                items.push(json!({"type":"message","id":format!("msg_{id}_{}_{}",self.0["candidate_index"],index),
                    "role":"assistant","phase":phase,"content":[{"type":"output_text","text":text}]}));
            } else if outcome == Some(CandidateOutcome::ToolCall)
                && let Some(call) = crate::content::present(part, "functionCall")
            {
                let call_id = call["id"].as_str().map(str::to_owned).unwrap_or_else(|| {
                    format!("call_{id}_{}_{}", self.0["candidate_index"], index)
                });
                if !ids.insert(call_id.clone()) {
                    return Err(ProviderError::new(502, "google_invalid_projection"));
                }
                let mut item = if let Some(tools) = &tools {
                    tools
                        .responses_call(call, &call_id)
                        .map_err(|_| ProviderError::new(502, "google_invalid_projection"))?
                        .wire()
                        .clone()
                } else {
                    let args = crate::content::present(call, "args")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    json!({"type":"function_call","call_id":call_id,"name":call["name"],"arguments":args.to_string()})
                };
                item["id"] = format!("fc_{id}_{}_{}", self.0["candidate_index"], index).into();
                item["status"] = "completed".into();
                items.push(item);
            }
            // Server tools, media and future Parts remain opaque native data.
        }
        let mut output = vec![json!({"type":"reasoning","id":format!("rs_{id}_native"),
            "summary":summary,"encrypted_content":format!("{}{}",self.prefix(),self.0)})];
        output.extend(items);
        let (status, detail) = match outcome {
            Some(CandidateOutcome::Stop | CandidateOutcome::ToolCall) => ("completed", None),
            Some(CandidateOutcome::MaxTokens) => ("incomplete", Some("max_output_tokens")),
            Some(CandidateOutcome::Filtered) | None => ("incomplete", Some("content_filter")),
            Some(CandidateOutcome::InvalidToolCall) => ("failed", None),
            Some(CandidateOutcome::Unknown) => ("incomplete", Some("unknown_provider_stop_reason")),
        };
        let mut wire = json!({"id":id,"object":"response","model":self.0["model"],"status":status,
            "output":output,"usage":normalized_usage(&native)?,"caidex_native_usage":native.wire()["usageMetadata"],
            "caidex_native_usage_scope":"generation","caidex_native_finish_reason":self.selected_candidate().map(|c|&c["finishReason"]),
            "caidex_native_prompt_feedback":native.wire()["promptFeedback"],
            "caidex_native_response_id":native.wire()["responseId"],"caidex_native_model_version":native.wire()["modelVersion"]});
        if let Some(reason) = detail {
            wire["incomplete_details"] = json!({"reason":reason});
        }
        if status == "failed" {
            wire["error"] = ProviderError::new(502, "google_invalid_tool_call").wire();
        }
        CanonicalResponse::new(wire)
            .map_err(|_| ProviderError::new(502, "google_invalid_projection"))
    }
    /// Verify one complete carrier/display group against the execution-side
    /// model and original native request, before restoring any signed Content.
    pub fn from_responses_output(
        output: &[Value],
        expected_model: &str,
        expected_request: &Value,
        max_bytes: usize,
    ) -> ProviderResult<Self> {
        let carrier = output.first().ok_or_else(replay_error)?;
        let capsule = carrier["encrypted_content"]
            .as_str()
            .ok_or_else(replay_error)?;
        if carrier["type"] != "reasoning" || max_bytes == 0 || capsule.len() > max_bytes {
            return Err(replay_error());
        }
        let (text, version) = if let Some(text) = capsule.strip_prefix(TOOLS_PREFIX) {
            (text, 2)
        } else if let Some(text) = capsule.strip_prefix(PREFIX) {
            (text, 1)
        } else {
            return Err(replay_error());
        };
        let envelope: Value = serde_json::from_str(text).map_err(|_| replay_error())?;
        if envelope["version"] != version {
            return Err(replay_error());
        }
        let history = Self::new(envelope, max_bytes).map_err(|_| replay_error())?;
        if history.0["model"] != expected_model {
            return Err(ProviderError::new(400, "google_history_model_mismatch"));
        }
        if history.request() != expected_request {
            return Err(ProviderError::new(400, "google_history_request_mismatch"));
        }
        let projected = history
            .to_responses(max_bytes)
            .map_err(|_| replay_error())?;
        if carrier["summary"] != projected.output()[0]["summary"]
            || output.len() != projected.output().len()
        {
            return Err(replay_error());
        }
        for (actual, expected) in output[1..].iter().zip(&projected.output()[1..]) {
            if actual["type"] != expected["type"] {
                return Err(replay_error());
            }
            match expected["type"].as_str().expect("known projection") {
                "message"
                    if actual["role"] == "assistant"
                        && actual["content"] == expected["content"]
                        && actual
                            .get("phase")
                            .is_none_or(|phase| phase == &expected["phase"]) => {}
                "function_call"
                    if actual["call_id"] == expected["call_id"]
                        && actual["name"] == expected["name"]
                        && actual["namespace"] == expected["namespace"] =>
                {
                    let arguments: Value = serde_json::from_str(
                        actual["arguments"].as_str().ok_or_else(replay_error)?,
                    )
                    .map_err(|_| replay_error())?;
                    let original: Value =
                        serde_json::from_str(expected["arguments"].as_str().expect("native JSON"))
                            .expect("native JSON");
                    if arguments != original {
                        return Err(replay_error());
                    }
                }
                "custom_tool_call"
                    if actual["call_id"] == expected["call_id"]
                        && actual["name"] == expected["name"]
                        && actual["namespace"] == expected["namespace"]
                        && actual["input"] == expected["input"] => {}
                _ => return Err(replay_error()),
            }
        }
        Ok(history)
    }
}
impl fmt::Debug for NativeHistory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeHistory([WIRE OMITTED])")
    }
}
fn normalized_usage(native: &NativeResponse) -> ProviderResult<Value> {
    let raw = &native.wire()["usageMetadata"];
    let count = |key| raw[key].as_u64();
    let inconsistent = || ProviderError::new(502, "google_usage_inconsistent");
    let add = |a: u64, b: u64| {
        a.checked_add(b)
            .ok_or_else(|| ProviderError::new(502, "google_usage_overflow"))
    };
    let input = count("promptTokenCount");
    let candidates = count("candidatesTokenCount");
    let thoughts = count("thoughtsTokenCount");
    let provided_total = count("totalTokenCount");
    let direct_output = match (candidates, thoughts) {
        (Some(a), Some(b)) => Some(add(a, b)?),
        _ => None,
    };
    // Prompt includes cached tokens; total includes candidate and thought tokens.
    // Derivation from known total/prompt is exact, without inventing a missing 0.
    let output = match (input, provided_total) {
        (Some(i), Some(t)) => {
            let o = t.checked_sub(i).ok_or_else(inconsistent)?;
            if direct_output.is_some_and(|v| v != o) {
                return Err(inconsistent());
            }
            Some(o)
        }
        _ => direct_output,
    };
    if count("cachedContentTokenCount")
        .zip(input)
        .is_some_and(|(c, i)| c > i)
        || candidates.zip(output).is_some_and(|(c, o)| c > o)
        || thoughts.zip(output).is_some_and(|(t, o)| t > o)
    {
        return Err(inconsistent());
    }
    let (Some(input), Some(output)) = (input, output) else {
        return Ok(Value::Null);
    };
    let total = add(input, output)?;
    if provided_total.is_some_and(|t| t != total) {
        return Err(inconsistent());
    }
    let mut usage = json!({"input_tokens":input,"output_tokens":output,"total_tokens":total,"caidex_native_usage":raw});
    if let Some(cached) = count("cachedContentTokenCount") {
        usage["input_tokens_details"] = json!({"cached_tokens":cached});
    }
    if let Some(thoughts) = thoughts {
        usage["output_tokens_details"] = json!({"reasoning_tokens":thoughts});
    }
    Ok(usage)
}
