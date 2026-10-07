use caidex_model_core::{
    CanonicalResponse, CapabilitySupport, CompatibilityLevel, CompatibilityReport, ContextHeaders,
    EvidenceSource, ModelMetadata, ModelRegistry, REQUEST_HEADERS, RESPONSE_HEADERS, ResponseItem,
    StreamState,
};
use serde_json::json;

#[test]
fn canonical_response_keeps_tools_reasoning_extensions_and_precise_usage() {
    let wire = json!({"id":"fixture", "status":"completed", "output":[
        {"type":"message", "content":[{"type":"output_text", "text":"中文🙂"}]},
        {"type":"function_call", "name":"fixture", "call_id":"call", "arguments":" { \"n\": 1.00 } "},
        {"type":"reasoning", "encrypted_content":"opaque+/==", "signature":"retain"},
        {"type":"future_item", "data":{"number":18446744073709551616_u128}}
    ], "usage":{"input_tokens":1,"unknown_cost":"0.0001"}, "future":"retain"});
    let response = CanonicalResponse::new(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&response).unwrap(), wire);
    assert_eq!(response.id(), "fixture");
    assert_eq!(response.output_text().collect::<Vec<_>>(), ["中文🙂"]);
    assert_eq!(response.state(), StreamState::Completed);
    assert_eq!(response.usage().unwrap().unwrap().output_tokens, None);
    assert!(
        ResponseItem::new(response.output()[1].clone())
            .unwrap()
            .tool_call()
            .unwrap()
            .is_some()
    );
    assert!(!format!("{response:?}").contains("opaque+/=="));
}

#[test]
fn canonical_response_rejects_malformed_sync_values_and_keeps_terminal_distinctions() {
    for (status, reason, expected) in [
        ("failed", "unused", StreamState::Failed),
        ("incomplete", "interrupted", StreamState::Interrupted),
        ("incomplete", "max_output_tokens", StreamState::Incomplete),
    ] {
        let response = CanonicalResponse::new(json!({"id":"fixture", "status":status, "output":[], "incomplete_details":{"reason":reason}})).unwrap();
        assert_eq!(response.state(), expected);
    }
    for wire in [
        json!({"status":"completed", "output":[]}),
        json!({"id":"fixture", "status":"in_progress", "output":[]}),
        json!({"id":"fixture", "status":"completed", "output":[{}]}),
        json!({"id":"fixture", "status":"completed", "output":[{"type":"function_call", "name":"tool", "arguments":{}, "call_id":"call"}]}),
        json!({"id":"fixture", "status":"completed", "output":[], "usage":{"input_tokens":-1}}),
    ] {
        assert!(CanonicalResponse::new(wire).is_err());
    }
}

fn model(id: &str) -> ModelMetadata {
    ModelMetadata::configured(
        id.into(),
        "native-fixture".into(),
        vec![caidex_model_core::ResponsesDialect::Classic],
    )
}

#[test]
fn registry_is_deterministic_unverified_and_does_not_invent_model_limits() {
    let registry = ModelRegistry::new([model("z"), model("a")]).unwrap();
    assert_eq!(
        registry
            .models()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    let metadata = registry.get("a").unwrap();
    assert_eq!(metadata.capabilities.vision, CapabilitySupport::Unknown);
    assert_eq!(metadata.capabilities.context_window, None);
    assert_eq!(metadata.codex_compatibility, None);
    assert_eq!(
        serde_json::from_value::<ModelMetadata>(serde_json::to_value(metadata).unwrap()).unwrap(),
        *metadata
    );
    assert!(registry.get("unknown").is_none());
    assert!(ModelRegistry::new([model("a"), model("a")]).is_err());
    let mut invalid = model("invalid");
    invalid.capabilities.context_window = Some(0);
    assert!(ModelRegistry::new([invalid]).is_err());
}

#[test]
fn configured_models_and_protocol_fixtures_cannot_claim_full_live_runtime_compatibility() {
    let mut metadata = model("fixture");
    let report = CompatibilityReport {
        schema_version: 1,
        level: CompatibilityLevel::Full,
        source: EvidenceSource::ProtocolFixture,
        reference: "tests/fixture-v1".into(),
        tested_model_version: "fixture-v1".into(),
        limitations: vec![],
    };
    metadata.codex_compatibility = Some(report);
    assert!(metadata.validate().is_err());
    metadata.codex_compatibility.as_mut().unwrap().level = CompatibilityLevel::Experimental;
    assert!(metadata.validate().is_ok());
    metadata.codex_compatibility.as_mut().unwrap().source = EvidenceSource::Configured;
    assert!(metadata.validate().is_err());
}

#[test]
fn context_headers_allow_only_explicit_wire_context_and_never_echo_values() {
    let mut headers = ContextHeaders::default();
    headers
        .insert("X-Codex-Turn-State", "opaque+/==".into(), REQUEST_HEADERS)
        .unwrap();
    assert_eq!(headers.get("x-codex-turn-state"), Some("opaque+/=="));
    assert!(!format!("{headers:?}").contains("opaque+/=="));
    assert!(
        headers
            .insert("x-codex-turn-state", "replacement".into(), REQUEST_HEADERS)
            .is_err()
    );
    assert_eq!(headers.get("x-codex-turn-state"), Some("opaque+/=="));
    for name in [
        "authorization",
        "cookie",
        "host",
        "x-api-key",
        "x-random-header",
    ] {
        assert!(
            headers
                .insert(name, "synthetic-secret".into(), &[name])
                .is_err()
        );
    }
    assert!(
        headers
            .insert("session_id", "bad\r\nheader".into(), REQUEST_HEADERS)
            .is_err()
    );
    assert!(
        headers
            .insert("x-request-id", "x".repeat(8193), RESPONSE_HEADERS)
            .is_err()
    );
}
