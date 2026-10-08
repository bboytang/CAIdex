use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, ProviderError, ProviderResult, StreamState,
};
use jsonschema::{PatternOptions, Retrieve, Uri, Validator};
use serde_json::Value;

// Keep the trust boundary offline even if another workspace member later
// enables jsonschema's HTTP/file features through Cargo feature unification.
struct Offline;
impl Retrieve for Offline {
    fn retrieve(&self, _: &Uri<String>) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("external schema retrieval disabled".into())
    }
}

pub(crate) struct StrictOutput(Validator);
impl StrictOutput {
    pub(crate) fn compile(request: &CanonicalRequest) -> ProviderResult<Option<Self>> {
        let format = &request.wire()["text"]["format"];
        if format["strict"] != true {
            return Ok(None);
        }
        // ponytail: evaluation is synchronous, with byte/backtracking limits,
        // not a hard CPU deadline; process isolation belongs to Host policy.
        jsonschema::options()
            .with_retriever(Offline)
            .should_validate_formats(true)
            .should_ignore_unknown_formats(false)
            .with_pattern_options(PatternOptions::fancy_regex().backtrack_limit(10_000))
            .build(&format["schema"])
            .map(|validator| Some(Self(validator)))
            .map_err(|_| ProviderError::new(400, "ollama_invalid_output_schema"))
    }
    pub(crate) fn validate(&self, response: &CanonicalResponse) -> ProviderResult<()> {
        if response.state() != StreamState::Completed {
            return Ok(());
        }
        let invalid = || ProviderError::new(502, "ollama_invalid_structured_output");
        let mut result = false;
        for item in response.output() {
            match item["type"].as_str() {
                Some("reasoning") => (),
                Some("function_call") => result = true,
                Some("message") => {
                    if item["role"] != "assistant"
                        || item.get("status").is_some_and(|s| s != "completed")
                    {
                        return Err(invalid());
                    }
                    let mut text = String::new();
                    let mut refusal = false;
                    for part in item["content"].as_array().ok_or_else(invalid)? {
                        match part["type"].as_str() {
                            Some("output_text") => {
                                text.push_str(part["text"].as_str().ok_or_else(invalid)?)
                            }
                            Some("refusal") if part["refusal"].is_string() => refusal = true,
                            _ => return Err(invalid()),
                        }
                    }
                    // Refusals and tool-only rounds are explicit alternatives;
                    // neither is represented as a Schema-conforming JSON answer.
                    if refusal {
                        if !text.is_empty() {
                            return Err(invalid());
                        }
                    } else {
                        let value: Value = serde_json::from_str(&text).map_err(|_| invalid())?;
                        if !self.0.is_valid(&value) {
                            return Err(invalid());
                        }
                    }
                    result = true;
                }
                _ => return Err(invalid()),
            }
        }
        if !result {
            return Err(invalid());
        }
        Ok(())
    }
}
