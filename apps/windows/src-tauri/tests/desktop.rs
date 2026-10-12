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
        let _ = socket.write_all(output[..first].as_bytes()).await;
        tokio::time::sleep(delay).await;
        let _ = socket.write_all(output[first..].as_bytes()).await;
    });
    (endpoint, task)
}
#[test]
fn settings_key_boundary_restart_and_endpoint_consent() {
    let root = Root::new();
    let store = Store::default();
    let app = Desktop::open(&root.0, store.clone()).unwrap();
    let mut p = profile("https://example.test/v1/responses");
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()), false)
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
        app.save_profile(p.clone(), None, false),
        Err("key_destination_changed")
    );
    app.save_profile(p.clone(), None, true).unwrap();
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
fn invalid_endpoints_and_each_existing_adapter_constructs_without_network() {
    let root = Root::new();
    let app = Desktop::open(&root.0, Store::default()).unwrap();
    for endpoint in [
        "http://example.test/responses",
        "https://u:p@example.test/responses",
        "https://example.test/responses?key=secret",
        "file:///tmp/key",
    ] {
        assert!(app.save_profile(profile(endpoint), None, false).is_err());
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
        app.save_profile(p, None, false).unwrap();
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
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()), false)
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
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()), false)
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
    app.save_profile(p.clone(), Some("synthetic-j1-secret".into()), false)
        .unwrap();
    app.select_profile(p.id).unwrap();
    let id = app.create("Guard".into()).unwrap();
    let reply = app
        .send(request(Uuid::new_v4(), id, None, "Hello".into(), vec![]))
        .await
        .unwrap();
    assert_eq!(reply.end, End::Invalid);
    server.await.unwrap();
    assert!(!app.export(id).unwrap().contains("synthetic-j1-secret"));
}
#[cfg(windows)]
#[test]
fn windows_native_store_roundtrip_remove_and_no_secret_in_bootstrap() {
    let root = Root::new();
    let app = Desktop::open(&root.0, caidex_credentials::SystemStore).unwrap();
    let p = profile("https://example.test/v1/responses");
    app.save_profile(p.clone(), Some("synthetic-native-j1-key".into()), false)
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
