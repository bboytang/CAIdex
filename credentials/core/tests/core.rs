use caidex_credentials::{
    Broker, CredentialRef, EnvironmentStore, Error, Id, Redactor, Result, Secret, SecretKind,
    SecretStore,
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

fn reference(owner: &str, provider: &str, profile: &str) -> CredentialRef {
    CredentialRef {
        owner: Id::new(owner).unwrap(),
        provider: Id::new(provider).unwrap(),
        profile: Id::new(profile).unwrap(),
        kind: SecretKind::ApiKey,
    }
}
#[derive(Clone, Default)]
struct MemoryStore(Arc<Mutex<HashMap<CredentialRef, String>>>);
impl SecretStore for MemoryStore {
    fn get(&self, reference: &CredentialRef) -> Result<Option<Secret>> {
        self.0
            .lock()
            .unwrap()
            .get(reference)
            .cloned()
            .map(Secret::new)
            .transpose()
    }
    fn set(&self, reference: &CredentialRef, value: &Secret) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(reference.clone(), value.expose().into());
        Ok(())
    }
    fn remove(&self, reference: &CredentialRef) -> Result<bool> {
        Ok(self.0.lock().unwrap().remove(reference).is_some())
    }
}

#[test]
fn identifiers_validate_deserialized_references_and_prevent_path_aliases() {
    for value in [
        "",
        "..",
        "host.profile",
        "/tmp/secret",
        "host\\profile",
        "host:profile",
        "Main",
        "a\0b",
    ] {
        assert!(Id::new(value).is_err());
        assert!(serde_json::from_value::<Id>(json!(value)).is_err());
    }
    assert!(Id::new("x".repeat(65)).is_err());
    let valid = reference("windows-device", "provider_1", "alternate");
    assert_eq!(
        serde_json::from_value::<CredentialRef>(serde_json::to_value(&valid).unwrap()).unwrap(),
        valid
    );
}

#[test]
fn broker_keeps_endpoint_provider_profile_and_secret_kind_separate() {
    let store = MemoryStore::default();
    let local = Broker::new(Id::new("local").unwrap(), store.clone());
    let remote = Broker::new(Id::new("vps").unwrap(), store.clone());
    let main = reference("local", "openai", "main");
    let alternate = reference("local", "openai", "testing");
    let other = reference("local", "anthropic", "main");
    let host = reference("vps", "openai", "main");
    let token = CredentialRef {
        kind: SecretKind::AccessToken,
        ..main.clone()
    };
    local
        .set(&main, Secret::new("fixture-key-main".into()).unwrap())
        .unwrap();
    local
        .set(&alternate, Secret::new("fixture-key-alt".into()).unwrap())
        .unwrap();
    remote
        .set(&host, Secret::new("fixture-host-key".into()).unwrap())
        .unwrap();
    assert_eq!(
        local.resolve(&main).unwrap().unwrap().expose(),
        "fixture-key-main"
    );
    assert_eq!(
        local.resolve(&alternate).unwrap().unwrap().expose(),
        "fixture-key-alt"
    );
    assert!(local.resolve(&other).unwrap().is_none());
    assert!(local.resolve(&token).unwrap().is_none());
    assert!(matches!(local.resolve(&host), Err(Error::WrongOwner)));
    assert!(matches!(
        local.set(&host, Secret::new("fixture-forbidden".into()).unwrap()),
        Err(Error::WrongOwner)
    ));
    assert_eq!(local.remove(&host), Err(Error::WrongOwner));
    assert_eq!(
        remote.resolve(&host).unwrap().unwrap().expose(),
        "fixture-host-key"
    );
    assert_eq!(store.0.lock().unwrap().len(), 3);
}

#[test]
fn status_debug_and_diagnostics_never_contain_loaded_or_replaced_values() {
    let broker = Broker::new(Id::new("local").unwrap(), MemoryStore::default());
    let reference = reference("local", "custom", "main");
    let first = "fixture-private-旧-key";
    let second = "fixture-private-new-key";
    let secret = Secret::new(first.into()).unwrap();
    assert_eq!(format!("{secret:?}"), "Secret([REDACTED])");
    broker.set(&reference, secret).unwrap();
    let status = broker.status(&reference).unwrap();
    assert!(status.configured);
    assert!(!serde_json::to_string(&status).unwrap().contains(first));
    broker
        .set(&reference, Secret::new(second.into()).unwrap())
        .unwrap();
    assert!(broker.remove(&reference).unwrap());
    assert!(!broker.remove(&reference).unwrap());
    assert!(!broker.status(&reference).unwrap().configured);
    let diagnostic = broker
        .redactor()
        .text(&format!("provider failure: {first}, replacement {second}"));
    assert!(!diagnostic.contains(first));
    assert!(!diagnostic.contains(second));
}

#[test]
fn redaction_masks_structured_headers_cookies_nested_errors_and_raw_json() {
    let redactor = Redactor::default();
    redactor
        .register(&Secret::new("fixture-visible-value".into()).unwrap())
        .unwrap();
    let fields = [
        "Authorization",
        "API_KEY",
        "apikey",
        "x-api-key",
        "access_token",
        "refreshToken",
        "client_secret",
        "cookie",
        "Set-Cookie",
        "X-Goog-API-Key",
    ];
    let mut headers = serde_json::Map::new();
    for field in fields {
        headers.insert(field.into(), json!("unregistered-value"));
    }
    let data = json!({"headers": headers, "items": [{"error": "echo fixture-visible-value"}], "tokenBudget": 100, "threadId": "thread-fixture"});
    let sanitized = redactor.json(&data);
    assert_eq!(sanitized["tokenBudget"], 100);
    assert_eq!(sanitized["threadId"], "thread-fixture");
    assert!(!sanitized.to_string().contains("unregistered-value"));
    assert!(!sanitized.to_string().contains("fixture-visible-value"));
    assert!(
        !redactor
            .text(&data.to_string())
            .contains("unregistered-value")
    );
    assert_eq!(
        redactor.text("Authorization=Bearer unknown:token\napi_key: unknown\npublic: ok\n"),
        "Authorization: [REDACTED]\napi_key: [REDACTED]\npublic: ok\n"
    );
    assert_eq!(data["headers"]["Authorization"], "unregistered-value");
}

#[test]
fn invalid_secret_and_backend_errors_have_safe_debug_output() {
    for value in [String::new(), "\0".into(), "x".repeat(16 * 1024 + 1)] {
        assert!(Secret::new(value).is_err());
    }
    assert_eq!(format!("{}", Error::InvalidSecret), "invalid secret");
    assert_eq!(
        format!("{:?}", Error::BackendUnavailable),
        "BackendUnavailable"
    );
}

#[test]
fn environment_mapping_is_explicit_read_only_and_tested_in_an_isolated_child() {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.env_clear();
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let status = command
        .env(
            "CAIDEX_FIXTURE_PROFILE_KEY",
            "fixture-child-environment-only",
        )
        .args(["--exact", "environment_child", "--ignored"])
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
#[ignore = "internal child fixture; the parent test supplies only synthetic environment"]
fn environment_child() {
    let other = reference("local", "custom", "testing");
    let reference = reference("local", "custom", "personal");
    let store = EnvironmentStore::new(HashMap::from([(
        reference.clone(),
        "CAIDEX_FIXTURE_PROFILE_KEY".into(),
    )]))
    .unwrap();
    assert_eq!(
        store.get(&reference).unwrap().unwrap().expose(),
        "fixture-child-environment-only"
    );
    assert!(store.get(&other).unwrap().is_none());
    assert_eq!(
        store.set(&reference, &Secret::new("fixture-no-write".into()).unwrap()),
        Err(Error::ReadOnly)
    );
    assert_eq!(store.remove(&reference), Err(Error::ReadOnly));
    let broker = Broker::new(Id::new("local").unwrap(), store);
    assert!(broker.status(&reference).unwrap().read_only);
}
