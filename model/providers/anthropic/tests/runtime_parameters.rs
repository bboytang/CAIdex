use caidex_model_core::{CanonicalRequest, ResponsesDialect};
use caidex_provider_anthropic::{MessagesRequest, RequestOptions, ServiceTierMapping};
use serde_json::{Value, json};
fn compile(
    extra: Value,
    options: &RequestOptions<'_>,
    dialect: ResponsesDialect,
) -> caidex_model_core::ProviderResult<MessagesRequest> {
    let mut source = json!({"model":"alias","input":[{"role":"user","content":"question"}]});
    source
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let request = CanonicalRequest::new(source, dialect).unwrap();
    MessagesRequest::from_responses_with_options(&request, "native", 100, 128 * 1024, 10, options)
}
#[test]
fn runtime_hints_are_locally_retained_and_native_tier_is_explicit_in_both_dialects() {
    let mappings = [ServiceTierMapping::new("default".into(), "standard_only".into()).unwrap()];
    let options = RequestOptions {
        retain_runtime_metadata: true,
        service_tier_mappings: &mappings,
        ..Default::default()
    };
    for dialect in [ResponsesDialect::Classic, ResponsesDialect::Lite] {
        let extra = json!({"include":["reasoning.encrypted_content"],"client_metadata":{"session_id":"synthetic","future":"preserved"},"prompt_cache_key":"routing hint","service_tier":"default"});
        let request = compile(extra.clone(), &options, dialect).unwrap();
        assert_eq!(request.wire()["service_tier"], "standard_only");
        for key in ["include", "client_metadata", "prompt_cache_key"] {
            assert_eq!(request.source()[key], extra[key]);
            assert!(request.wire().get(key).is_none());
        }
        assert!(request.wire().get("metadata").is_none());
        assert!(request.wire().get("cache_control").is_none());
        assert!(!format!("{request:?}").contains("routing hint"));
    }
}
#[test]
fn unsupported_delivery_and_access_semantics_are_never_silently_ignored() {
    let options = RequestOptions {
        retain_runtime_metadata: true,
        ..Default::default()
    };
    for extra in [
        json!({"include":["message.output_text.logprobs"]}),
        json!({"stream_options":{"reasoning_summary_delivery":"sequential_cutoff"}}),
        json!({"access_programs":{"cyber":"standard"}}),
        json!({"service_tier":"priority"}),
        json!({"service_tier":"default"}),
        json!({"prompt_cache_retention":"24h"}),
    ] {
        assert!(compile(extra, &options, ResponsesDialect::Classic).is_err());
    }
    for extra in [
        json!({"client_metadata":{}}),
        json!({"prompt_cache_key":"hint"}),
    ] {
        assert!(compile(extra, &RequestOptions::default(), ResponsesDialect::Classic).is_err());
    }
}
#[test]
fn malformed_hints_and_duplicate_configuration_fail_and_nulls_are_absent() {
    let options = RequestOptions {
        retain_runtime_metadata: true,
        ..Default::default()
    };
    for extra in [
        json!({"include":1}),
        json!({"include":[1]}),
        json!({"include":["reasoning.encrypted_content","reasoning.encrypted_content"]}),
        json!({"prompt_cache_key":false}),
        json!({"prompt_cache_key":""}),
        json!({"client_metadata":[]}),
        json!({"client_metadata":{"key":1}}),
        json!({"stream_options":[]}),
        json!({"service_tier":1}),
    ] {
        assert!(compile(extra, &options, ResponsesDialect::Classic).is_err());
    }
    let request = compile(json!({"include":[],"stream_options":{},"service_tier":null,"client_metadata":null,"prompt_cache_key":null,"access_programs":null}),&RequestOptions::default(),ResponsesDialect::Classic).unwrap();
    assert!(request.wire().get("service_tier").is_none());
    for (source, native) in [
        ("priority", "auto"),
        ("flex", "standard_only"),
        ("default", "priority"),
    ] {
        assert!(ServiceTierMapping::new(source.into(), native.into()).is_err());
    }
    let make = || ServiceTierMapping::new("auto".into(), "auto".into()).unwrap();
    let mappings = [make(), make()];
    let options = RequestOptions {
        service_tier_mappings: &mappings,
        ..Default::default()
    };
    assert!(compile(json!({}), &options, ResponsesDialect::Classic).is_err());
}
