use caidex_chat::End;
use caidex_credentials::{CredentialRef, Secret, SecretStore};
use caidex_desktop::{Desktop, Preferences, Profile, ProviderKind, request};
use serde_json::json;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use uuid::Uuid;

#[derive(Clone, Default)]
struct Store(Arc<Mutex<HashMap<CredentialRef, String>>>);
impl SecretStore for Store {
    fn get(&self, key: &CredentialRef) -> caidex_credentials::Result<Option<Secret>> {
        self.0
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .map(Secret::new)
            .transpose()
    }
    fn set(&self, key: &CredentialRef, value: &Secret) -> caidex_credentials::Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(key.clone(), value.expose().to_owned());
        Ok(())
    }
    fn remove(&self, key: &CredentialRef) -> caidex_credentials::Result<bool> {
        Ok(self.0.lock().unwrap().remove(key).is_some())
    }
}
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("caidex-j1-{}", Uuid::new_v4())))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn profile(endpoint: &str) -> Profile {
    Profile {
        id: Uuid::new_v4(),
        name: "Synthetic".into(),
        kind: ProviderKind::CustomResponses,
        endpoint: endpoint.into(),
        model: "synthetic".into(),
        effort: None,
        tier: None,
    }
}
fn response(text: &str) -> String {
    format!(
        "data: {}\n\ndata: {}\n\n",
        json!({"type":"response.output_text.delta","delta":text}),
        json!({"type":"response.completed","response":{"id":"fixture","status":"completed","output":[{"id":"message","type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}]}})
    )
}
async fn fixture(text: &str, delay: Duration) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    let output = response(text);
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0; 4096];
            let n = socket.read(&mut chunk).await.unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..index]);
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= index + 4 + length {
                    let request: serde_json::Value =
                        serde_json::from_slice(&bytes[index + 4..]).unwrap();
                    assert_eq!(request["tools"], json!([]));
                    assert_eq!(request["tool_choice"], "none");
                    assert_eq!(request["store"], false);
                    assert!(
                        !String::from_utf8_lossy(&bytes[index + 4..])
                            .contains("synthetic-j1-secret")
                    );
                    break;
                }
            }
        }
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let first = output.find("\n\n").unwrap() + 2;
        let _ = socket.write_all(&output.as_bytes()[..first]).await;
        tokio::time::sleep(delay).await;
        let _ = socket.write_all(&output.as_bytes()[first..]).await;
    });
    (endpoint, task)
}
#[test]
fn settings_key_boundary_restart_and_immutable_endpoint() {
    let root = Root::new();
    let store = Store::default();
    let app = Desktop::open(&root.0, store.clone()).unwrap();
    let mut p = profile("https://example.test/v1/responses");
    let mut accidental = p.clone();
    accidental.name = "synthetic-j1-secret".into();
    assert_eq!(
        app.save_profile(accidental, Some("synthetic-j1-secret".into())),
        Err("invalid_or_sensitive_input")
    );
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()))
        .unwrap();
    app.select_profile(p.id).unwrap();
    let id = app.create("Saved Chat".into()).unwrap();
    app.draft(id, Some("Saved draft".into())).unwrap();
    app.preferences(Preferences {
        language: "en".into(),
        theme: "light".into(),
    })
    .unwrap();
    assert_eq!(
        app.create("synthetic-j1-secret".into()),
        Err("invalid_or_sensitive_input")
    );
    assert_eq!(
        app.draft(id, Some("synthetic-j1-secret".into())),
        Err("invalid_or_sensitive_input")
    );
    let boot = serde_json::to_string(&app.bootstrap().unwrap()).unwrap();
    assert!(!boot.contains("synthetic-j1-secret"));
    let bytes = std::fs::read(root.0.join("desktop.sqlite3")).unwrap();
    assert!(!bytes.windows(19).any(|w| w == b"synthetic-j1-secret"));
    p.endpoint = "https://new.example.test/responses".into();
    assert_eq!(
        app.save_profile(p.clone(), None),
        Err("new_profile_required")
    );
    assert_eq!(
        app.save_profile(p.clone(), Some("synthetic-new-key".into())),
        Err("new_profile_required")
    );
    let orphan = profile("https://orphan.example.test/responses");
    store
        .set(
            &orphan.reference(),
            &Secret::new("synthetic-orphan-key".into()).unwrap(),
        )
        .unwrap();
    assert_eq!(app.save_profile(orphan, None), Err("new_profile_required"));
    drop(app);
    let app = Desktop::open(&root.0, store).unwrap();
    let boot = app.bootstrap().unwrap();
    assert_eq!(boot.preferences.language, "en");
    assert_eq!(boot.preferences.theme, "light");
    assert_eq!(boot.selected, Some(p.id));
    assert_eq!(app.draft(id, None).unwrap(), "Saved draft");
    assert_eq!(boot.conversations.len(), 1);
    app.remove_key(p.id).unwrap();
    assert!(!app.bootstrap().unwrap().profiles[0].configured);
}
#[test]
fn every_history_page_is_reachable_and_attachment_limits_are_enforced() {
    let root = Root::new();
    let app = Desktop::open(&root.0, Store::default()).unwrap();
    let mut ids = Vec::new();
    for i in 0..140 {
        ids.push(app.create(format!("Chat {i}")).unwrap());
    }
    let first = app.conversations(None).unwrap();
    assert_eq!(first.len(), 128);
    let next = app.conversations(first.last().map(|c| c.id)).unwrap();
    assert_eq!(next.len(), 12);
    assert!(next.iter().all(|c| !first.iter().any(|old| old.id == c.id)));
    let id = ids[0];
    assert!(
        app.attach(id, "../key".into(), "text/plain".into(), b"safe".to_vec())
            .is_err()
    );
    assert!(
        app.attach(
            id,
            "bad.exe".into(),
            "application/octet-stream".into(),
            b"safe".to_vec()
        )
        .is_err()
    );
    assert!(
        app.attach(id, "empty.txt".into(), "text/plain".into(), vec![])
            .is_err()
    );
    let attachment = app
        .attach(
            id,
            "note.txt".into(),
            "text/plain".into(),
            b"safe attachment".to_vec(),
        )
        .unwrap();
    assert_eq!(
        app.chat.attachment(id, attachment).unwrap().bytes,
        b"safe attachment"
    );
}
#[test]
fn invalid_endpoints_and_each_existing_adapter_constructs_without_network() {
    let root = Root::new();
    let app = Desktop::open(&root.0, Store::default()).unwrap();
    for endpoint in [
        "http://example.test/responses",
        "https://u:p@example.test/responses",
        "https://example.test/responses?key=secret",
        "file:///tmp/key",
    ] {
        assert!(app.save_profile(profile(endpoint), None).is_err());
    }
    for kind in [
        ProviderKind::Openai,
        ProviderKind::Anthropic,
        ProviderKind::Google,
        ProviderKind::Deepseek,
        ProviderKind::Qwen,
        ProviderKind::Openrouter,
        ProviderKind::Ollama,
        ProviderKind::CustomResponses,
        ProviderKind::CustomChatCompletions,
    ] {
        let mut p = profile("https://example.test/v1/");
        p.kind = kind;
        if kind == ProviderKind::CustomResponses {
            p.endpoint.push_str("responses");
        }
        if kind == ProviderKind::CustomChatCompletions {
            p.endpoint.push_str("chat/completions");
        }
        if kind == ProviderKind::Google {
            p.model = "models/synthetic".into();
        }
        app.save_profile(p, None)
            .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
    }
    assert_eq!(app.bootstrap().unwrap().profiles.len(), 9);
    assert!(
        app.preferences(Preferences {
            language: "xx".into(),
            theme: "light".into()
        })
        .is_err()
    );
}
#[tokio::test]
async fn actual_stream_progress_completion_restart_and_branch_history() {
    let root = Root::new();
    let store = Store::default();
    let app = Arc::new(Desktop::open(&root.0, store.clone()).unwrap());
    let (endpoint, server) = fixture("Synthetic reply", Duration::from_millis(350)).await;
    let p = profile(&endpoint);
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()))
        .unwrap();
    app.select_profile(p.id).unwrap();
    let id = app.create("Stream".into()).unwrap();
    let op = Uuid::new_v4();
    let worker = {
        let app = app.clone();
        tokio::spawn(async move {
            app.send(request(op, id, None, "Hello".into(), vec![]))
                .await
        })
    };
    for _ in 0..100 {
        if app.snapshot(id, 0).unwrap().draft.as_deref() == Some("Synthetic reply") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        app.snapshot(id, 0).unwrap().draft.as_deref(),
        Some("Synthetic reply")
    );
    assert_eq!(app.select_profile(p.id), Err("turn_active"));
    let reply = worker.await.unwrap().unwrap();
    assert_eq!(reply.end, End::Complete);
    server.await.unwrap();
    assert_eq!(app.snapshot(id, 0).unwrap().messages.len(), 2);
    assert!(app.chat.draft(op).unwrap().is_none());
    assert_eq!(
        app.send(request(op, id, None, "Hello".into(), vec![]))
            .await
            .err(),
        Some("already_submitted")
    );
    drop(app);
    let app = Desktop::open(&root.0, store).unwrap();
    assert_eq!(
        app.snapshot(id, 0).unwrap().messages[1].content.text,
        "Synthetic reply"
    );
    assert!(app.export(id).unwrap().contains("Synthetic reply"));
}
#[tokio::test]
async fn cancellation_finalizes_partial_output_without_replay() {
    let root = Root::new();
    let app = Arc::new(Desktop::open(&root.0, Store::default()).unwrap());
    let (endpoint, server) = fixture("Partial reply", Duration::from_secs(2)).await;
    let p = profile(&endpoint);
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()))
        .unwrap();
    app.select_profile(p.id).unwrap();
    let id = app.create("Cancel".into()).unwrap();
    let op = Uuid::new_v4();
    let worker = {
        let app = app.clone();
        tokio::spawn(async move {
            app.send(request(op, id, None, "Hello".into(), vec![]))
                .await
        })
    };
    for _ in 0..100 {
        if app.snapshot(id, 0).unwrap().draft.as_deref() == Some("Partial reply") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    app.cancel(op).unwrap();
    assert_eq!(worker.await.unwrap().err(), Some("cancelled"));
    let messages = app.snapshot(id, 0).unwrap().messages;
    assert_eq!(messages[1].end, End::Cancelled);
    assert_eq!(messages[1].content.text, "Partial reply");
    server.await.unwrap();
}
#[tokio::test]
async fn provider_key_echo_is_rejected_before_cache_checkpoint() {
    let root = Root::new();
    let app = Desktop::open(&root.0, Store::default()).unwrap();
    let (endpoint, server) = fixture("synthetic-j1-secret", Duration::ZERO).await;
    let p = profile(&endpoint);
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()))
        .unwrap();
    app.select_profile(p.id).unwrap();
    let id = app.create("Guard".into()).unwrap();
    let error = app
        .send(request(Uuid::new_v4(), id, None, "Hello".into(), vec![]))
        .await
        .err()
        .unwrap();
    assert_eq!(error, "provider_protocol_error");
    assert_eq!(app.snapshot(id, 0).unwrap().messages[1].end, End::Invalid);
    server.await.unwrap();
    assert!(!app.export(id).unwrap().contains("synthetic-j1-secret"));
}
#[tokio::test]
async fn harmless_credential_configuration_prose_is_still_valid_chat() {
    let root = Root::new();
    let app = Desktop::open(&root.0, Store::default()).unwrap();
    let (endpoint, server) = fixture("API Key: configured locally", Duration::ZERO).await;
    let p = profile(&endpoint);
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()))
        .unwrap();
    app.select_profile(p.id).unwrap();
    let id = app.create("Configuration help".into()).unwrap();
    let reply = app
        .send(request(
            Uuid::new_v4(),
            id,
            None,
            "How is it configured?".into(),
            vec![],
        ))
        .await
        .unwrap();
    assert_eq!(reply.end, End::Complete);
    assert_eq!(reply.content.text, "API Key: configured locally");
    server.await.unwrap();
}
#[cfg(windows)]
#[test]
fn inherited_webview_directory_is_refused_but_private_child_opens() {
    let root = Root::new();
    std::fs::create_dir(&root.0).unwrap();
    assert_eq!(
        Desktop::open(&root.0, Store::default()).err(),
        Some("unsafe_storage")
    );
    let child = root.0.join("local-v1");
    let app = Desktop::open(&child, Store::default()).unwrap();
    assert!(app.bootstrap().unwrap().conversations.is_empty());
    assert!(child.join("desktop.sqlite3").is_file());
    assert!(!root.0.join("desktop.sqlite3").exists());
}
#[cfg(windows)]
#[test]
fn windows_native_store_roundtrip_remove_and_no_secret_in_bootstrap() {
    let root = Root::new();
    let app = Desktop::open(&root.0, caidex_credentials::SystemStore).unwrap();
    let p = profile("https://example.test/v1/responses");
    app.save_profile(p.clone(), Some("synthetic-native-j1-key".into()))
        .unwrap();
    assert!(app.bootstrap().unwrap().profiles[0].configured);
    assert!(
        !serde_json::to_string(&app.bootstrap().unwrap())
            .unwrap()
            .contains("synthetic-native-j1-key")
    );
    app.remove_key(p.id).unwrap();
    assert!(!app.bootstrap().unwrap().profiles[0].configured);
}

async fn read_http(socket: &mut tokio::net::TcpStream) -> (String, serde_json::Value) {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 4096];
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8(bytes[..index].to_vec()).unwrap();
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap_or(0);
            if bytes.len() >= index + 4 + length {
                let body = if length == 0 {
                    serde_json::Value::Null
                } else {
                    serde_json::from_slice(&bytes[index + 4..index + 4 + length]).unwrap()
                };
                return (headers, body);
            }
        }
    }
}
fn deepseek_stream(model: &str, text: &str) -> String {
    let item = json!({"id":"message","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":text,"annotations":[]}]});
    let frames = vec![
        json!({"type":"response.created","response":{"id":"fixture","status":"in_progress","output":[]}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"id":"message","type":"message","status":"in_progress","role":"assistant","content":[]}}),
        json!({"type":"response.content_part.added","output_index":0,"item_id":"message","content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        json!({"type":"response.output_text.delta","output_index":0,"item_id":"message","content_index":0,"delta":text}),
        json!({"type":"response.output_text.done","output_index":0,"item_id":"message","content_index":0,"text":text}),
        json!({"type":"response.content_part.done","output_index":0,"item_id":"message","content_index":0,"part":item["content"][0]}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"fixture","object":"response","model":model,"created_at":1,"status":"completed","store":false,"output":[item]}}),
    ];
    frames
        .into_iter()
        .enumerate()
        .map(|(sequence, mut frame)| {
            frame["sequence_number"] = sequence.into();
            format!(
                "event: {}\ndata: {}\n\n",
                frame["type"].as_str().unwrap(),
                frame
            )
        })
        .collect()
}
#[tokio::test]
async fn deepseek_catalog_two_models_share_one_key_and_actual_reasoning_wire() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for index in 0..3 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (headers, body) = read_http(&mut socket).await;
            assert!(
                headers
                    .to_lowercase()
                    .contains("authorization: bearer synthetic-j1-secret")
            );
            if index == 0 {
                assert!(headers.starts_with("GET /models "));
                let body=json!({"object":"list","data":[{"id":"deepseek-flash","object":"model","owned_by":"deepseek"},{"id":"deepseek-v4-pro","object":"model","owned_by":"deepseek"}]}).to_string();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
            } else {
                assert!(headers.starts_with("POST /responses "));
                let (model, effort) = if index == 1 {
                    ("deepseek-flash", "low")
                } else {
                    ("deepseek-v4-pro", "high")
                };
                assert_eq!(body["model"], model);
                assert_eq!(body["reasoning"]["effort"], effort);
                assert_eq!(body["tools"], json!([]));
                assert_eq!(body["tool_choice"], "none");
                assert!(!body.to_string().contains("synthetic-j1-secret"));
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
                socket
                    .write_all(deepseek_stream(model, "DeepSeek fixture reply").as_bytes())
                    .await
                    .unwrap();
            }
        }
    });
    let root = Root::new();
    let store = Store::default();
    let app = Desktop::open(&root.0, store.clone()).unwrap();
    let mut p = profile(&endpoint);
    p.kind = ProviderKind::Deepseek;
    p.model.clear();
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()))
        .unwrap();
    // Reproduce the shipped rejection using its original adapter policy before the fixed GUI path.
    let old_broker = Arc::new(caidex_credentials::Broker::new(
        caidex_chat::Namespace::guest().credential_owner(),
        store.clone(),
    ));
    let old = caidex_provider_deepseek::DeepSeekProvider::new(
        caidex_provider_deepseek::DeepSeekConfig::new(p.reference())
            .unwrap()
            .with_base_url(&endpoint)
            .unwrap(),
        vec![caidex_model_core::ModelMetadata::configured(
            p.id.to_string(),
            "deepseek-flash".into(),
            vec![caidex_model_core::ResponsesDialect::Classic],
        )],
        old_broker,
        caidex_provider_custom::Limits::default(),
    )
    .unwrap();
    let wire = caidex_model_core::CanonicalRequest::new(json!({"model":p.id.to_string(),"input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"Hello"}]}],"stream":true,"store":false,"tools":[],"tool_choice":"none"}),caidex_model_core::ResponsesDialect::Classic).unwrap();
    let rejected =
        caidex_model_core::ModelProvider::stream_response(&old, wire, Default::default())
            .await
            .err()
            .unwrap();
    assert_eq!(
        (rejected.http_status, rejected.code),
        (400, "deepseek_unsupported_request")
    );
    let models = app.discover_models(p.id).await.unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].efforts, vec!["none", "low", "high", "max"]);
    assert_eq!(
        app.select_model(p.id, "missing".into(), None, None),
        Err("model_unavailable")
    );
    assert_eq!(
        app.select_model(p.id, "deepseek-flash".into(), Some("medium".into()), None),
        Err("unsupported_controls")
    );
    let id = app.create("DeepSeek".into()).unwrap();
    let mut parent = None;
    for (model, effort) in [("deepseek-flash", "low"), ("deepseek-v4-pro", "high")] {
        app.select_model(p.id, model.into(), Some(effort.into()), None)
            .unwrap();
        let reply = tokio::time::timeout(
            Duration::from_secs(5),
            app.send(request(Uuid::new_v4(), id, parent, "Hello".into(), vec![])),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(reply.end, End::Complete);
        assert_eq!(reply.content.text, "DeepSeek fixture reply");
        parent = Some(reply.id);
    }
    server.await.unwrap();
    assert_eq!(store.0.lock().unwrap().len(), 1);
    drop(app);
    let app = Desktop::open(&root.0, store).unwrap();
    let boot = app.bootstrap().unwrap();
    assert_eq!(boot.profiles[0].models.len(), 2);
    assert_eq!(boot.profiles[0].profile.model, "deepseek-v4-pro");
    assert_eq!(boot.profiles[0].profile.effort.as_deref(), Some("high"));
    app.remove_key(p.id).unwrap();
    assert!(app.bootstrap().unwrap().profiles[0].models.is_empty());
}
#[tokio::test]
async fn safe_http_failure_is_visible_and_committed_turn_is_never_replayed() {
    for (status, code) in [
        (401, "provider_authentication_failed"),
        (402, "provider_quota_exhausted"),
        (404, "provider_endpoint_missing"),
        (429, "provider_rate_limited"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_http(&mut socket).await;
            let body =
                json!({"error":{"message":"synthetic-j1-secret","code":"fixture_secret_echo"}})
                    .to_string();
            socket.write_all(format!("HTTP/1.1 {status} Refused\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
        });
        let root = Root::new();
        let app = Desktop::open(&root.0, Store::default()).unwrap();
        let p = profile(&endpoint);
        app.save_profile(p.clone(), Some("synthetic-j1-secret".into()))
            .unwrap();
        app.select_profile(p.id).unwrap();
        let id = app.create("Failure".into()).unwrap();
        let op = Uuid::new_v4();
        assert_eq!(
            app.send(request(op, id, None, "Hello".into(), vec![]))
                .await
                .err(),
            Some(code)
        );
        server.await.unwrap();
        let snapshot = app.snapshot(id, 0).unwrap();
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.messages[1].end, End::Failed);
        assert_eq!(
            app.send(request(op, id, None, "Hello".into(), vec![]))
                .await
                .err(),
            Some("already_submitted")
        );
        assert!(!app.export(id).unwrap().contains("synthetic-j1-secret"));
    }
}
#[test]
fn original_profile_json_loads_without_reentering_key_or_model() {
    let p = profile("https://api.deepseek.com");
    let old = json!({"id":p.id,"name":"DeepSeek","kind":"deepseek","endpoint":p.endpoint,"model":"deepseek-flash"});
    let p: Profile = serde_json::from_value(old).unwrap();
    assert!(p.effort.is_none() && p.tier.is_none());
    p.validate().unwrap();
}

#[tokio::test]
async fn catalog_fetched_with_removed_key_is_not_committed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let (headers, _) = read_http(&mut socket).await;
        assert!(headers.starts_with("GET /models "));
        started.send(()).unwrap();
        wait.await.unwrap();
        let body=json!({"object":"list","data":[{"id":"synthetic","object":"model","created":1,"owned_by":"fixture"}]}).to_string();
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
    });
    let root = Root::new();
    let app = Arc::new(Desktop::open(&root.0, Store::default()).unwrap());
    let p = profile(&endpoint);
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()))
        .unwrap();
    let worker = {
        let app = app.clone();
        tokio::spawn(async move { app.discover_models(p.id).await })
    };
    ready.await.unwrap();
    app.remove_key(p.id).unwrap();
    release.send(()).unwrap();
    assert_eq!(worker.await.unwrap().err(), Some("configuration_changed"));
    server.await.unwrap();
    let boot = app.bootstrap().unwrap();
    assert!(!boot.profiles[0].configured);
    assert!(boot.profiles[0].models.is_empty());
}
