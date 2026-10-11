use caidex_auth::{ConfirmedSession, Trust};
use caidex_chat::{
    Cache, Chat, Content, End, Error, LocalModel, MAX_ATTACHMENT, MAX_TEXT, Namespace, Send,
};
use caidex_credentials::{CredentialRef, Id, SecretKind};
use caidex_model_core::{
    CanonicalRequest, ContextHeaders, CredentialRequirement, ModelCapabilities, ModelMetadata,
    ModelProvider, ProviderError, ProviderFuture, ProviderResponse, ProviderResult,
    ProviderStreamEvent, RequestContext, ResponsesDialect, ResponsesStream, StreamingResponse,
};
use futures_util::stream;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("caidex-i3-test-{}", Uuid::new_v4())))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn account(user: &str) -> Namespace {
    let trust = Trust::local_test("http://localhost:3333/").unwrap();
    let reply = json!({"access_token":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE","refresh_token":"AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI","token_type":"Bearer","expires_in":600,"issuer":trust.issuer(),"client_id":caidex_auth::CLI_CLIENT,"audience":caidex_auth::AUDIENCE,"user_id":user,"session_id":"bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"});
    Namespace::account(
        &ConfirmedSession::confirm(&trust, caidex_auth::CLI_CLIENT, reply.clone(), &reply).unwrap(),
    )
}
struct Fixture {
    mode: &'static str,
    owner: Option<Id>,
    requests: Mutex<Vec<Value>>,
    calls: AtomicUsize,
}
impl Fixture {
    fn new(mode: &'static str) -> Arc<Self> {
        Arc::new(Self {
            mode,
            owner: None,
            requests: Mutex::new(vec![]),
            calls: AtomicUsize::new(0),
        })
    }
    fn bind(self: &Arc<Self>, chat: &Chat) {
        chat.configure(LocalModel {
            provider: self.clone(),
            model: "fixture".into(),
        })
        .unwrap();
    }
}
fn event(wire: Value) -> ProviderStreamEvent {
    let mut parser = ResponsesStream::new(2 * MAX_TEXT).unwrap();
    let bytes = format!(
        "event: {}\ndata: {}\n\n",
        wire["type"].as_str().unwrap(),
        wire
    );
    ProviderStreamEvent::Model(parser.push(bytes.as_bytes()).unwrap().remove(0))
}
impl ModelProvider for Fixture {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn metadata(&self, model: &str) -> ProviderResult<ModelMetadata> {
        Ok(ModelMetadata::configured(
            model.into(),
            model.into(),
            vec![ResponsesDialect::Classic],
        ))
    }
    fn capabilities(&self, model: &str) -> ProviderResult<ModelCapabilities> {
        Ok(self.metadata(model)?.capabilities)
    }
    fn credential_requirements(&self, _: &str) -> ProviderResult<CredentialRequirement> {
        Ok(match &self.owner {
            None => CredentialRequirement::None,
            Some(owner) => CredentialRequirement::Bearer {
                reference: CredentialRef {
                    owner: owner.clone(),
                    provider: Id::new("fixture").unwrap(),
                    profile: Id::new("default").unwrap(),
                    kind: SecretKind::ApiKey,
                },
            },
        })
    }
    fn create_response(
        &self,
        _: CanonicalRequest,
        _: RequestContext,
    ) -> ProviderFuture<'_, ProviderResponse> {
        Box::pin(async { Err(ProviderError::new(500, "fixture_stream_only")) })
    }
    fn stream_response(
        &self,
        request: CanonicalRequest,
        _: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests.lock().unwrap().push(request.wire().clone());
            if self.mode == "error" {
                return Err(ProviderError::new(502, "safe_fixture_failure"));
            }
            let mut events = vec![Ok(event(
                json!({"type":"response.output_text.delta","delta":"partial"}),
            ))];
            if self.mode == "hang" {
                return Ok(StreamingResponse {
                    events: Box::pin(stream::iter(events).chain(stream::pending())),
                    headers: ContextHeaders::default(),
                });
            }
            if self.mode != "truncated" {
                let output = if self.mode == "tool" {
                    json!([{"type":"function_call","call_id":"x","name":"shell","arguments":"{}"}])
                } else {
                    json!([{"type":"reasoning","encrypted_content":"PRIVATE-CARRIER"},{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}])
                };
                let status = if self.mode == "incomplete" {
                    "incomplete"
                } else {
                    "completed"
                };
                events.push(Ok(event(json!({"type":format!("response.{status}"),"response":{"id":"r","status":status,"output":output}}))));
            }
            Ok(StreamingResponse {
                events: Box::pin(stream::iter(events)),
                headers: ContextHeaders::default(),
            })
        })
    }
}
use futures_util::StreamExt;
fn send(conversation: Uuid, parent: Option<Uuid>, text: &str) -> Send {
    Send {
        operation: Uuid::new_v4(),
        conversation,
        parent,
        content: Content::text(text),
    }
}

#[tokio::test]
async fn selected_local_export_keeps_branches_attachments_and_account_isolation() {
    let root = Root::new();
    let chat =
        Arc::new(Chat::open(&root.0, account("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")).unwrap());
    Fixture::new("complete").bind(&chat);
    let selected = chat.create_conversation(None, "selected".into()).unwrap();
    let excluded = chat
        .create_conversation(None, "EXCLUDED-CHAT".into())
        .unwrap();
    let attachment = chat
        .attach(
            selected,
            "test.txt".into(),
            "text/plain".into(),
            b"exported attachment".to_vec(),
        )
        .unwrap();
    let mut request = send(selected, None, "original");
    request.content.attachments.push(attachment);
    let first = chat.send(request).await.unwrap();
    chat.send(send(selected, Some(first.id), "left"))
        .await
        .unwrap();
    chat.send(send(selected, Some(first.id), "right"))
        .await
        .unwrap();
    let queued = chat
        .attach(
            selected,
            "queued.txt".into(),
            "text/plain".into(),
            b"not yet sent".to_vec(),
        )
        .unwrap();
    let export = chat.export_conversation(selected).unwrap();
    let parsed: Value = serde_json::from_str(&export).unwrap();
    assert_eq!(parsed["messages"].as_array().unwrap().len(), 6);
    assert_eq!(parsed["attachments"].as_array().unwrap().len(), 2);
    assert!(
        parsed["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == queued.to_string())
    );
    assert!(
        parsed["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["base64"] == "ZXhwb3J0ZWQgYXR0YWNobWVudA==")
    );
    assert!(!export.contains("EXCLUDED-CHAT"));
    assert!(!export.contains("PRIVATE-CARRIER"));
    assert!(!export.contains("refresh_token"));
    assert!(!export.contains("outbox"));
    Fixture::new("hang").bind(&chat);
    let pending = chat
        .create_conversation(None, "pending draft".into())
        .unwrap();
    let request = send(pending, None, "save in progress");
    let operation = request.operation;
    let worker = {
        let chat = chat.clone();
        tokio::spawn(async move { chat.send(request).await })
    };
    let mut saved = false;
    for _ in 0..100 {
        let snapshot: Value =
            serde_json::from_str(&chat.export_conversation(pending).unwrap()).unwrap();
        if snapshot["pending_turns"][0]["draft"] == "partial" {
            saved = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(saved);
    chat.cancel(operation).unwrap();
    let _ = worker.await.unwrap();
    chat.switch(account("cccccccc-cccc-cccc-cccc-cccccccccccc"))
        .unwrap();
    assert!(matches!(
        chat.export_conversation(selected),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        chat.export_conversation(excluded),
        Err(Error::NotFound)
    ));
    let guard = Arc::new(caidex_credentials::Redactor::default());
    guard
        .register(
            &caidex_credentials::Secret::new("synthetic registered \"credential\"".into()).unwrap(),
        )
        .unwrap();
    chat.memory().protect_credentials(guard).unwrap();
    let secret = chat
        .create_conversation(None, "synthetic registered \"credential\"".into())
        .unwrap();
    assert!(matches!(
        chat.export_conversation(secret),
        Err(Error::Invalid)
    ));
}

#[tokio::test]
async fn immutable_branches_selected_history_attachments_and_outbox_survive_reopen() {
    let root = Root::new();
    let chat = Chat::open(&root.0, Namespace::guest()).unwrap();
    let f = Fixture::new("complete");
    f.bind(&chat);
    let c = chat.create_conversation(None, "Chat".into()).unwrap();
    let a = chat
        .attach(
            c,
            "test.txt".into(),
            "text/plain".into(),
            b"synthetic attachment".to_vec(),
        )
        .unwrap();
    let mut request = send(c, None, "start");
    request.content.attachments.push(a);
    let first = chat.send(request).await.unwrap();
    assert_eq!(first.content.text, "hello");
    assert_eq!(first.end, End::Complete);
    assert!(first.created_at_millis > 0);
    let (left, right) = tokio::join!(
        chat.send(send(c, Some(first.id), "left")),
        chat.send(send(c, Some(first.id), "right"))
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_ne!(left.parent, right.parent);
    let original = chat.messages(c, 0, 100).unwrap();
    assert_eq!(original.len(), 6);
    let retry = send(c, Some(left.id), "follow left");
    let operation = retry.operation;
    chat.send(retry).await.unwrap();
    let calls = f.calls.load(Ordering::SeqCst);
    assert!(matches!(
        chat.send(Send {
            operation,
            ..send(c, None, "different payload")
        })
        .await,
        Err(Error::AlreadySubmitted)
    ));
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    {
        let requests = f.requests.lock().unwrap();
        let last = requests.last().unwrap().to_string();
        assert!(last.contains("left"));
        assert!(!last.contains("right"));
        assert!(!last.contains("PRIVATE-CARRIER"));
        assert!(
            requests[0]["input"][0]["content"][1]["file_data"]
                .as_str()
                .unwrap()
                .starts_with("data:text/plain;base64,")
        );
        assert_eq!(requests[0]["tools"], json!([]));
        assert_eq!(requests[0]["tool_choice"], "none");
        assert_eq!(requests[0]["store"], false);
    }
    let c2 = chat.create_conversation(None, "other".into()).unwrap();
    assert!(matches!(chat.attachment(c2, a), Err(Error::NotFound)));
    let mut cross = send(c2, None, "cross");
    cross.content.attachments.push(a);
    assert!(matches!(chat.send(cross).await, Err(Error::NotFound)));
    assert!(matches!(
        chat.send(send(c2, Some(first.id), "wrong parent")).await,
        Err(Error::NotFound)
    ));
    assert!(chat.messages(c2, 0, 10).unwrap().is_empty());
    let outbox = chat.outbox(0, 100).unwrap();
    assert_eq!(outbox.len(), 11);
    let db = rusqlite::Connection::open(root.0.join("guest/cache.sqlite3")).unwrap();
    assert!(db.execute("UPDATE messages SET content='{}'", []).is_err());
    assert!(db.execute("DELETE FROM messages", []).is_err());
    drop(db);
    drop(chat);
    let reopened = Chat::open(&root.0, Namespace::guest()).unwrap();
    assert_eq!(reopened.messages(c, 0, 100).unwrap().len(), 8);
    assert_eq!(
        reopened.attachment(c, a).unwrap().bytes,
        b"synthetic attachment"
    );
    assert_eq!(reopened.outbox(0, 100).unwrap().len(), 11);
    assert!(matches!(
        reopened.send(send(c, None, "no auto model")).await,
        Err(Error::ModelUnavailable)
    ));
}

#[tokio::test]
async fn switch_logout_and_offline_reopen_never_adopt_foreign_data_or_model() {
    let root = Root::new();
    let guest = Namespace::guest();
    let a = account("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
    let b = account("cccccccc-cccc-cccc-cccc-cccccccccccc");
    let chat = Chat::open(&root.0, guest.clone()).unwrap();
    let g = chat.create_conversation(None, "guest".into()).unwrap();
    chat.switch(a.clone()).unwrap();
    let c = chat.create_conversation(None, "A private".into()).unwrap();
    let attachment = chat
        .attach(c, "a.png".into(), "image/png".into(), vec![1])
        .unwrap();
    let f = Fixture::new("hang");
    f.bind(&chat);
    let request = send(c, None, "A-only");
    let op = request.operation;
    let mut pending = Box::pin(chat.send(request));
    tokio::select! { _=&mut pending=>panic!("fixture must wait"),_=tokio::time::sleep(Duration::from_millis(30))=>() }
    chat.switch(b.clone()).unwrap();
    assert!(matches!(pending.await, Err(Error::Cancelled)));
    assert!(chat.conversations(None, 100).unwrap().is_empty());
    assert!(chat.outbox(0, 100).unwrap().is_empty());
    assert!(matches!(chat.messages(c, 0, 10), Err(Error::NotFound)));
    assert!(matches!(
        chat.attachment(c, attachment),
        Err(Error::NotFound)
    ));
    let bc = chat.create_conversation(None, "B".into()).unwrap();
    assert!(matches!(
        chat.send(send(bc, None, "must configure B")).await,
        Err(Error::ModelUnavailable)
    ));
    chat.logout().unwrap();
    assert_eq!(chat.conversations(None, 10).unwrap()[0].id, g);
    chat.switch(a.clone()).unwrap();
    let messages = chat.messages(c, 0, 10).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].end, End::Cancelled);
    assert_eq!(messages[1].content.text, "partial");
    assert!(matches!(chat.cancel(op), Err(Error::Cancelled)));
    drop(chat);
    let trust = Trust::local_test("http://localhost:3333/").unwrap();
    let offline = Namespace::offline_account(
        &trust,
        &caidex_cloud_core::Id::try_from("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned())
            .unwrap(),
    );
    assert!(offline == a);
    let reopened = Chat::open(&root.0, offline).unwrap();
    assert_eq!(reopened.messages(c, 0, 10).unwrap().len(), 2);
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_transport_failure_tool_and_truncation_are_visible_terminal_messages() {
    for (mode, end) in [
        ("error", End::Failed),
        ("truncated", End::Truncated),
        ("tool", End::Invalid),
        ("incomplete", End::Incomplete),
    ] {
        let root = Root::new();
        let chat = Chat::open(&root.0, Namespace::guest()).unwrap();
        Fixture::new(mode).bind(&chat);
        let c = chat.create_conversation(None, mode.into()).unwrap();
        let result = chat.send(send(c, None, "question")).await.unwrap();
        assert_eq!(result.end, end);
        assert_eq!(end.state(), "interrupted");
        assert!(!serde_json::to_string(&result).unwrap().contains("shell"));
    }
    let root = Root::new();
    let chat = Chat::open(&root.0, Namespace::guest()).unwrap();
    Fixture::new("hang").bind(&chat);
    let c = chat.create_conversation(None, "cancel".into()).unwrap();
    let request = send(c, None, "question");
    let op = request.operation;
    let mut pending = Box::pin(chat.send(request));
    tokio::select! { _=&mut pending=>panic!("fixture must wait"),_=tokio::time::sleep(Duration::from_millis(30))=>() }
    chat.cancel(op).unwrap();
    assert!(matches!(pending.await, Err(Error::Cancelled)));
    assert_eq!(chat.messages(c, 0, 10).unwrap()[1].end, End::Cancelled);
}

#[test]
fn host_key_and_other_accounts_provider_reference_cannot_be_bound() {
    let root = Root::new();
    let namespace = Namespace::guest();
    let chat = Chat::open(&root.0, namespace.clone()).unwrap();
    for owner in [
        Id::new("host-secret-owner").unwrap(),
        account("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").credential_owner(),
    ] {
        let f = Arc::new(Fixture {
            mode: "complete",
            owner: Some(owner),
            requests: Mutex::new(vec![]),
            calls: AtomicUsize::new(0),
        });
        assert!(matches!(
            chat.configure(LocalModel {
                provider: f.clone(),
                model: "fixture".into()
            }),
            Err(Error::WrongCredentialOwner)
        ));
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }
    let f = Arc::new(Fixture {
        mode: "complete",
        owner: Some(namespace.credential_owner()),
        requests: Mutex::new(vec![]),
        calls: AtomicUsize::new(0),
    });
    f.bind(&chat);
}

#[test]
fn private_storage_lock_future_schema_and_limits_are_refused_without_mutating_existing_data() {
    let root = Root::new();
    let mut cache = Cache::open(&root.0, Namespace::guest()).unwrap();
    assert!(matches!(
        Cache::open(&root.0, Namespace::guest()),
        Err(Error::UnsafeStorage)
    ));
    let c = cache.create_conversation(None, "valid".into()).unwrap();
    assert!(
        cache
            .attach(c, "../x".into(), "image/png".into(), vec![1])
            .is_err()
    );
    assert!(
        cache
            .attach(
                c,
                "x".into(),
                "image/png".into(),
                vec![0; MAX_ATTACHMENT + 1]
            )
            .is_err()
    );
    assert!(cache.messages(c, 0, 1001).is_err());
    drop(cache);
    let path = root.0.join("guest/cache.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.pragma_update(None, "user_version", 6).unwrap();
    drop(db);
    assert!(matches!(
        Cache::open(&root.0, Namespace::guest()),
        Err(Error::UnsafeStorage)
    ));
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT title FROM conversations", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "valid"
    );
}

#[cfg(unix)]
#[test]
fn unsafe_mode_symlink_hardlink_and_git_paths_are_refused() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = Root::new();
    std::fs::create_dir(&root.0).unwrap();
    std::fs::set_permissions(&root.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        Cache::open(&root.0, Namespace::guest()),
        Err(Error::UnsafeStorage)
    ));
    std::fs::set_permissions(&root.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir(root.0.join(".git")).unwrap();
    assert!(matches!(
        Cache::open(&root.0, Namespace::guest()),
        Err(Error::UnsafeStorage)
    ));
    std::fs::remove_dir(root.0.join(".git")).unwrap();
    let cache = Cache::open(&root.0, Namespace::guest()).unwrap();
    drop(cache);
    let db = root.0.join("guest/cache.sqlite3");
    std::fs::rename(&db, root.0.join("original")).unwrap();
    symlink(root.0.join("original"), &db).unwrap();
    assert!(matches!(
        Cache::open(&root.0, Namespace::guest()),
        Err(Error::UnsafeStorage)
    ));
    std::fs::remove_file(&db).unwrap();
    std::fs::hard_link(root.0.join("original"), db).unwrap();
    assert!(matches!(
        Cache::open(&root.0, Namespace::guest()),
        Err(Error::UnsafeStorage)
    ));
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "scripts/i7-operations.py provides a disposable physical 16MiB tmpfs"]
fn physical_enospc_preserves_committed_cache_and_queue() {
    use std::io::Write;
    let base = PathBuf::from(std::env::var("CAIDEX_I7_DISK_ROOT").unwrap());
    assert_eq!(base, PathBuf::from("/fault"));
    let root = base.join("cache");
    let mut cache = Cache::open(&root, Namespace::guest()).unwrap();
    let first = cache
        .create_conversation(None, "committed before ENOSPC".into())
        .unwrap();
    let queue = cache.outbox(0, 10).unwrap().len();
    let mut filler = std::fs::File::create(base.join("filler")).unwrap();
    let block = vec![0u8; 1024 * 1024];
    let error = loop {
        if let Err(error) = filler.write_all(&block) {
            break error;
        }
    };
    assert_eq!(error.raw_os_error(), Some(28));
    assert!(matches!(
        cache.create_conversation(None, "must roll back".into()),
        Err(Error::Storage)
    ));
    assert_eq!(cache.conversations(None, 10).unwrap().len(), 1);
    assert_eq!(cache.conversations(None, 10).unwrap()[0].id, first);
    drop(filler);
    std::fs::remove_file(base.join("filler")).unwrap();
    drop(cache);
    let cache = Cache::open(&root, Namespace::guest()).unwrap();
    assert_eq!(cache.conversations(None, 10).unwrap()[0].id, first);
    assert_eq!(cache.outbox(0, 10).unwrap().len(), queue);
    println!(
        "I7_CHECK {}",
        json!({"name":"physical_enospc_sqlite_committed_chat_and_outbox_reopen_unchanged","passed":true})
    );
}
