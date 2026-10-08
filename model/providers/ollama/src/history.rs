use crate::OllamaConfig;
use caidex_model_core::{
    CanonicalRequest, CanonicalResponse, ProviderError, ProviderResult, ResponsesStream,
};
use serde_json::{Value, json};
use std::fmt;

const PREFIX: &str = "caidex.ollama.native-history.v1:";
fn invalid() -> ProviderError {
    ProviderError::new(400, "ollama_invalid_history")
}
pub(crate) fn native_error(error: ProviderError) -> ProviderError {
    if error.http_status == 400 {
        ProviderError::new(502, "ollama_invalid_native_history")
    } else {
        error
    }
}

/// Bounded executor-owned native wire. This is a JSON carrier, not encryption
/// or source authentication. A compiled prefix is supplied by the caller;
/// never use the carrier's own request as its expected replay binding.
#[derive(Clone)]
pub struct NativeHistory(Value);
impl NativeHistory {
    pub fn from_response(
        config: &OllamaConfig,
        native_model: &str,
        request: &CanonicalRequest,
        response: &CanonicalResponse,
        max_bytes: usize,
    ) -> ProviderResult<Self> {
        if request.dialect() != caidex_model_core::ResponsesDialect::Classic {
            return Err(invalid());
        }
        Self::new(
            json!({"provider":"ollama","version":1,"source":"json",
                "scope":config.replay_scope(),"native_model":native_model,
                "request":request.wire(),"response":response.wire()}),
            max_bytes,
        )
    }
    pub fn from_stream(
        config: &OllamaConfig,
        native_model: &str,
        request: &CanonicalRequest,
        response: &CanonicalResponse,
        chunks: &[Value],
        max_bytes: usize,
    ) -> ProviderResult<Self> {
        if request.dialect() != caidex_model_core::ResponsesDialect::Classic {
            return Err(invalid());
        }
        Self::stream_record(
            &config.replay_scope(),
            native_model,
            request,
            response,
            chunks,
            max_bytes,
        )
    }
    pub(crate) fn stream_record(
        scope: &Value,
        native_model: &str,
        request: &CanonicalRequest,
        response: &CanonicalResponse,
        chunks: &[Value],
        max_bytes: usize,
    ) -> ProviderResult<Self> {
        Self::new(
            json!({"provider":"ollama","version":1,"source":"sse",
                "scope":scope,"native_model":native_model,
                "request":request.wire(),"response":response.wire(),"chunks":chunks}),
            max_bytes,
        )
    }
    fn new(wire: Value, max_bytes: usize) -> ProviderResult<Self> {
        let history = Self(wire);
        history.check_size(max_bytes)?;
        if history.0["provider"] != "ollama"
            || history.0["version"] != 1
            || !history.0["scope"].is_object()
            || history.0["request"]["model"] != history.0["native_model"]
            || !history.0["native_model"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
        {
            return Err(invalid());
        }
        CanonicalRequest::new(
            history.0["request"].clone(),
            caidex_model_core::ResponsesDialect::Classic,
        )
        .map_err(|_| invalid())?;
        let response =
            CanonicalResponse::new(history.0["response"].clone()).map_err(|_| invalid())?;
        if response
            .wire()
            .get("model")
            .is_some_and(|model| model != &history.0["native_model"])
        {
            return Err(invalid());
        }
        for item in response
            .output()
            .iter()
            .filter(|item| item["type"] == "reasoning")
        {
            if !item["encrypted_content"].is_string()
                || item["summary"].as_array().is_none_or(|parts| {
                    parts
                        .iter()
                        .any(|p| p["type"] != "summary_text" || !p["text"].is_string())
                })
            {
                return Err(invalid());
            }
        }
        match history.0["source"].as_str() {
            Some("json") if history.0.get("chunks").is_none() => (),
            Some("sse") => {
                let chunks = history.0["chunks"].as_array().ok_or_else(invalid)?;
                let mut parser = ResponsesStream::new(max_bytes).map_err(|_| invalid())?;
                let mut terminal = None;
                for chunk in chunks {
                    for event in parser
                        .push(format!("data: {chunk}\n\n").as_bytes())
                        .map_err(|_| invalid())?
                    {
                        if event.response.terminal().is_some() {
                            terminal = event.response.wire().get("response").cloned();
                        }
                    }
                }
                parser.finish().map_err(|_| invalid())?;
                if terminal.as_ref() != Some(response.wire()) {
                    return Err(invalid());
                }
                for chunk in chunks {
                    if let Some("response.output_item.done" | "response.output_item.added") =
                        chunk["type"].as_str()
                    {
                        let index = chunk["output_index"]
                            .as_u64()
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(invalid)?;
                        let final_item = response.output().get(index).ok_or_else(invalid)?;
                        let item = &chunk["item"];
                        if chunk["type"] == "response.output_item.done" {
                            if item != final_item {
                                return Err(invalid());
                            }
                        } else {
                            for field in ["type", "id", "role", "name", "call_id", "namespace"] {
                                if item
                                    .get(field)
                                    .is_some_and(|value| final_item.get(field) != Some(value))
                                {
                                    return Err(invalid());
                                }
                            }
                        }
                    }
                }
            }
            _ => return Err(invalid()),
        }
        Ok(history)
    }
    fn check_size(&self, max_bytes: usize) -> ProviderResult<()> {
        if max_bytes == 0 || self.0.to_string().len().saturating_add(PREFIX.len()) > max_bytes {
            return Err(ProviderError::new(502, "ollama_history_too_large"));
        }
        Ok(())
    }
    pub fn wire(&self) -> &Value {
        &self.0
    }
    pub fn native_response(&self) -> &Value {
        &self.0["response"]
    }
    pub fn request(&self) -> &Value {
        &self.0["request"]
    }
    pub fn to_responses(&self, max_bytes: usize) -> ProviderResult<CanonicalResponse> {
        self.check_size(max_bytes)?;
        let mut wire = self.native_response().clone();
        let native = wire["output"].as_array().expect("validated output");
        let summary: Vec<_> = native
            .iter()
            .filter(|item| item["type"] == "reasoning")
            .flat_map(|item| {
                item["summary"]
                    .as_array()
                    .expect("validated summary")
                    .iter()
                    .cloned()
            })
            .collect();
        // ponytail: full prefix snapshots grow quadratically across a session;
        // bounded request/history budgets cap this, Host may deduplicate later.
        let mut output = vec![
            json!({"type":"reasoning","id":format!("rs_{}_native",wire["id"].as_str().expect("validated ID")),
            "summary":summary,"encrypted_content":format!("{PREFIX}{}",self.0)}),
        ];
        output.extend(
            native
                .iter()
                .filter(|item| item["type"] != "reasoning")
                .cloned(),
        );
        wire["output"] = output.into();
        if wire.to_string().len() > max_bytes {
            return Err(ProviderError::new(502, "ollama_history_too_large"));
        }
        CanonicalResponse::new(wire).map_err(|_| invalid())
    }
    /// Restore one complete display group at the execution-side compiled
    /// prefix. The returned native output retains all unknown fields/order.
    pub fn from_responses_prefix(
        input: &[Value],
        config: &OllamaConfig,
        native_model: &str,
        expected_request: &CanonicalRequest,
        max_bytes: usize,
    ) -> ProviderResult<(Self, usize)> {
        if expected_request.dialect() != caidex_model_core::ResponsesDialect::Classic {
            return Err(invalid());
        }
        let carrier = input.first().ok_or_else(invalid)?;
        let capsule = carrier["encrypted_content"].as_str().ok_or_else(invalid)?;
        if carrier["type"] != "reasoning" || max_bytes == 0 || capsule.len() > max_bytes {
            return Err(invalid());
        }
        let wire: Value = serde_json::from_str(capsule.strip_prefix(PREFIX).ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        let history = Self::new(wire, max_bytes).map_err(|_| invalid())?;
        if history.0["scope"] != config.replay_scope() || history.0["native_model"] != native_model
        {
            return Err(ProviderError::new(400, "ollama_history_model_mismatch"));
        }
        let original = CanonicalRequest::new(history.request().clone(), expected_request.dialect())
            .map_err(|_| invalid())?;
        if prefix(&original) != prefix(expected_request) {
            return Err(ProviderError::new(400, "ollama_history_prefix_mismatch"));
        }
        let projected = history.to_responses(max_bytes).map_err(|_| invalid())?;
        let count = projected.output().len();
        let actual = input.get(..count).ok_or_else(invalid)?;
        for (actual, expected) in actual.iter().zip(projected.output()) {
            // Runtime may drop transport item IDs/status; meaningful fields,
            // arguments, annotations and future payloads must still match.
            if actual.get("id").is_some_and(|id| !id.is_string())
                || actual
                    .get("status")
                    .is_some_and(|status| status != "completed")
                || without_identity(actual) != without_identity(expected)
            {
                return Err(invalid());
            }
        }
        Ok((history, count))
    }
}
fn without_identity(item: &Value) -> Value {
    let mut item = item.clone();
    if let Some(object) = item.as_object_mut() {
        object.remove("id");
        object.remove("status");
    }
    item
}
fn prefix(request: &CanonicalRequest) -> Value {
    let wire = request.wire();
    let input = if let Some(text) = wire["input"].as_str() {
        json!([{"role":"user","content":text}])
    } else {
        wire["input"].clone()
    };
    json!({"model":request.model(),"input":input,"instructions":wire["instructions"],"tools":wire["tools"]})
}
impl fmt::Debug for NativeHistory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeHistory([WIRE OMITTED])")
    }
}

pub(crate) fn native_request(
    request: &CanonicalRequest,
    model: &str,
) -> ProviderResult<CanonicalRequest> {
    let mut wire = request.wire().clone();
    wire["model"] = model.into();
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}

pub(crate) fn expand(
    request: CanonicalRequest,
    config: &OllamaConfig,
    native_model: &str,
    max_bytes: usize,
) -> ProviderResult<CanonicalRequest> {
    let Some(input) = request.wire()["input"].as_array() else {
        return Ok(request);
    };
    let mut native = Vec::new();
    let mut index = 0;
    while index < input.len() {
        if input[index]["type"] == "reasoning" {
            let mut prefix = request.wire().clone();
            prefix["input"] = json!(native);
            let expected = native_request(
                &CanonicalRequest::new(prefix, request.dialect()).map_err(|_| invalid())?,
                native_model,
            )?;
            let (history, count) = NativeHistory::from_responses_prefix(
                &input[index..],
                config,
                native_model,
                &expected,
                max_bytes,
            )?;
            let output = history.native_response()["output"]
                .as_array()
                .expect("validated output");
            native.extend(output.iter().cloned());
            if output
                .last()
                .is_some_and(|item| item["type"] == "reasoning")
            {
                // Seal a thinking-only turn before a later user message; the
                // native decoder otherwise holds it until a future assistant.
                native.push(json!({"role":"assistant","content":""}));
            }
            index += count;
        } else {
            native.push(input[index].clone());
            index += 1;
        }
    }
    let mut wire = request.wire().clone();
    wire["input"] = native.into();
    if wire.to_string().len() > max_bytes {
        return Err(ProviderError::new(413, "invalid_or_oversized_body"));
    }
    CanonicalRequest::new(wire, request.dialect()).map_err(|_| invalid())
}
