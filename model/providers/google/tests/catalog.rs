use caidex_model_core::{CapabilitySupport, EvidenceSource, ResponsesDialect};
use caidex_provider_google::{ModelCatalog, ModelsPage};
use serde_json::{Value, json};

fn model(name: &str) -> Value {
    json!({"name":name,"baseModelId":"fixture","version":"1",
        "displayName":"合成模型","supportedGenerationMethods":["generateContent","futureAction"],
        "thinking":true,"inputTokenLimit":4096,"outputTokenLimit":1024})
}

#[test]
fn native_model_metadata_preserves_unknown_fields_without_guessing_capabilities() {
    let mut wire = model("models/fixture-001");
    wire["future"] = serde_json::from_str("{\"number\":18446744073709551616}").unwrap();
    let page = ModelsPage::parse(json!({"models":[wire.clone()]})).unwrap();
    let native = &page.models()[0];
    assert_eq!(native.name(), "models/fixture-001");
    assert_eq!(native.wire(), &wire);
    assert!(!format!("{native:?}").contains("future"));
    assert_eq!(
        native.supports_generation_method("generateContent"),
        Some(true)
    );
    assert_eq!(
        native.supports_generation_method("embedContent"),
        Some(false)
    );
    let metadata = native
        .metadata("alias", vec![ResponsesDialect::Classic])
        .unwrap();
    assert_eq!(metadata.native_model, "models/fixture-001");
    assert_eq!(metadata.source, EvidenceSource::ProviderCatalog);
    assert_eq!(
        metadata.capabilities.reasoning,
        CapabilitySupport::Supported
    );
    assert_eq!(metadata.capabilities.context_window, Some(4096));
    assert_eq!(metadata.capabilities.output_limit, Some(1024));
    assert_eq!(metadata.capabilities.vision, CapabilitySupport::Unknown);
    assert_eq!(
        metadata.capabilities.native_tools,
        CapabilitySupport::Unknown
    );
    assert_eq!(metadata.capabilities.streaming, CapabilitySupport::Unknown);
    assert_eq!(metadata.capabilities.text, CapabilitySupport::Unknown);
    assert!(metadata.codex_compatibility.is_none());
    for key in [
        "thinking",
        "inputTokenLimit",
        "outputTokenLimit",
        "supportedGenerationMethods",
        "displayName",
    ] {
        wire.as_object_mut().unwrap().remove(key);
    }
    let page = ModelsPage::parse(json!({"models":[wire]})).unwrap();
    let native = &page.models()[0];
    assert_eq!(native.supports_generation_method("generateContent"), None);
    let metadata = native
        .metadata("alias", vec![ResponsesDialect::Lite])
        .unwrap();
    assert_eq!(metadata.display_name, "models/fixture-001");
    assert_eq!(metadata.capabilities.reasoning, CapabilitySupport::Unknown);
    assert_eq!(metadata.capabilities.context_window, None);
    assert!(
        native
            .metadata("", vec![ResponsesDialect::Classic])
            .is_err()
    );
}

#[test]
fn malformed_catalogs_and_unsafe_resource_names_are_rejected() {
    for name in [
        "",
        "fixture",
        "models/",
        "models/../other",
        "models/a/b",
        "models/a?key=bad",
        "models/a#bad",
        "models/a%2Fb",
        "models/a\n",
    ] {
        let error = ModelsPage::parse(json!({"models":[model(name)]}))
            .err()
            .unwrap();
        assert_eq!(error.code, "google_invalid_model_catalog");
    }
    for (field, bad) in [
        ("baseModelId", json!("")),
        ("version", json!(3)),
        ("thinking", json!("yes")),
        ("inputTokenLimit", json!(-1)),
        ("outputTokenLimit", json!(1.5)),
        ("supportedGenerationMethods", json!([1])),
        ("displayName", json!(false)),
    ] {
        let mut wire = model("models/fixture");
        wire[field] = bad;
        assert!(ModelsPage::parse(json!({"models":[wire]})).is_err());
    }
    for wire in [
        json!([]),
        json!({"models":{}}),
        json!({"nextPageToken":1}),
        json!({"models":[model("models/a"),model("models/a")]}),
    ] {
        assert!(ModelsPage::parse(wire).is_err());
    }
}

#[test]
fn bounded_paging_rejects_duplicates_cycles_and_partial_catalogs() {
    let mut catalog = ModelCatalog::new(2).unwrap();
    assert_eq!(
        catalog
            .append(
                ModelsPage::parse(
                    json!({"models":[model("models/z")],"nextPageToken":"cursor+/=&?"})
                )
                .unwrap()
            )
            .unwrap()
            .as_deref(),
        Some("cursor+/=&?")
    );
    assert!(
        catalog
            .append(ModelsPage::parse(json!({"models":[model("models/z")]})).unwrap())
            .is_err()
    );
    assert!(
        catalog
            .append(ModelsPage::parse(json!({"nextPageToken":"cursor+/=&?"})).unwrap())
            .is_err()
    );
    assert!(
        catalog
            .append(
                ModelsPage::parse(json!({"models":[model("models/a"),model("models/b")]})).unwrap()
            )
            .is_err()
    );
    assert_eq!(
        catalog
            .append(ModelsPage::parse(json!({"models":[model("models/a")]})).unwrap())
            .unwrap(),
        None
    );
    assert!(
        catalog
            .append(ModelsPage::parse(json!({})).unwrap())
            .is_err()
    );
    assert_eq!(
        catalog
            .finish()
            .unwrap()
            .iter()
            .map(|m| m.name())
            .collect::<Vec<_>>(),
        ["models/a", "models/z"]
    );
    let mut incomplete = ModelCatalog::new(2).unwrap();
    incomplete
        .append(ModelsPage::parse(json!({"nextPageToken":"next"})).unwrap())
        .unwrap();
    assert!(incomplete.finish().is_err());
    assert!(ModelCatalog::new(0).is_err());
}

#[test]
fn empty_proto_pages_and_unknown_capabilities_remain_valid() {
    for wire in [
        json!({}),
        json!({"models":[]}),
        json!({"models":null,"nextPageToken":""}),
    ] {
        let page = ModelsPage::parse(wire).unwrap();
        assert!(page.models().is_empty());
        assert_eq!(page.next_page_token(), None);
    }
    let mut wire = model("models/fixture");
    wire["thinking"] = false.into();
    wire["inputTokenLimit"] = 0.into();
    wire["outputTokenLimit"] = Value::Null;
    let page = ModelsPage::parse(json!({"models":[wire],"nextPageToken":null})).unwrap();
    let metadata = page.models()[0]
        .metadata("alias", vec![ResponsesDialect::Classic])
        .unwrap();
    assert_eq!(
        metadata.capabilities.reasoning,
        CapabilitySupport::Unsupported
    );
    assert_eq!(metadata.capabilities.context_window, None);
    assert_eq!(metadata.capabilities.output_limit, None);
}
