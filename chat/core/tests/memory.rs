use caidex_auth::Trust;
use caidex_chat::memory::*;
use caidex_chat::{Chat, Content, Error, LocalModel, Namespace, Send};
use caidex_cloud_core::Id;
use caidex_credentials::{Redactor, Secret};
use caidex_model_core::*;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use uuid::Uuid;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("caidex-i4-{}", Uuid::new_v4())))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn draft(scope: Scope, text: &str) -> Draft {
    Draft {
        scope: scope.clone(),
        text: text.into(),
        kind: Kind::Preference,
        privacy: Privacy::LocalOnly,
        tags: vec!["editor".into()],
        sources: vec![Source::user(scope, text.into())],
        valid_until_millis: None,
    }
}
fn target(name: &str) -> ProviderTarget {
    ProviderTarget {
        endpoint: format!("https://fixture.invalid/{name}"),
        native_model: name.into(),
    }
}
fn permit(engine: &MemoryEngine<'_>, target: ProviderTarget, purpose: Purpose) -> Uuid {
    engine
        .allow(Permission {
            target,
            purpose,
            scope: Scope::Personal,
            kinds: vec![Kind::AiInference, Kind::Preference],
            source_types: vec![SourceType::User, SourceType::Chat, SourceType::File],
            allow_local_only: true,
        })
        .unwrap()
}
fn space(version: &str) -> Space {
    Space {
        target: target("embedding"),
        model_revision: version.into(),
        dimensions: 2,
        preprocessing_version: "text-v1".into(),
        distance: Distance::Cosine,
    }
}
fn model(p: Arc<Mock>, name: &str) -> LocalModel {
    LocalModel {
        provider: p,
        model: name.into(),
    }
}
struct Mock {
    calls: AtomicUsize,
    inputs: Mutex<Vec<serde_json::Value>>,
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
    slow: bool,
}
impl Mock {
    fn new(slow: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            inputs: Mutex::new(vec![]),
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            slow,
        })
    }
}
impl ModelProvider for Mock {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn metadata(&self, m: &str) -> ProviderResult<ModelMetadata> {
        Ok(ModelMetadata::configured(
            m.into(),
            m.into(),
            vec![ResponsesDialect::Classic],
        ))
    }
    fn capabilities(&self, m: &str) -> ProviderResult<ModelCapabilities> {
        Ok(self.metadata(m)?.capabilities)
    }
    fn credential_requirements(&self, _: &str) -> ProviderResult<CredentialRequirement> {
        Ok(CredentialRequirement::None)
    }
    fn memory_target(&self, m: &str, embedding: bool) -> ProviderResult<ProviderTarget> {
        Ok(target(if embedding { "embedding" } else { m }))
    }
    fn create_response(
        &self,
        r: CanonicalRequest,
        c: RequestContext,
    ) -> ProviderFuture<'_, ProviderResponse> {
        Box::pin(async move {
            assert!(c.memory_request);
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inputs.lock().unwrap().push(r.wire().clone());
            self.started.notify_one();
            if self.slow {
                self.release.notified().await;
            }
            let text = serde_json::to_string(
                &json!([{"text":"Use the preferred editor","tags":["editor"]}]),
            )
            .unwrap();
            Ok(ProviderResponse{headers:ContextHeaders::default(),response:CanonicalResponse::new(json!({"id":"i4","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}]})).unwrap()})
        })
    }
    fn stream_response(
        &self,
        r: CanonicalRequest,
        c: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async move {
            self.inputs.lock().unwrap().push(r.wire().clone());
            self.calls.fetch_add(1, Ordering::SeqCst);
            let _ = c;
            let mut decoder = ResponsesStream::new(1024 * 1024).unwrap();
            let frame = format!(
                "event: response.completed\ndata: {}\n\n",
                json!({"type":"response.completed","response":{"id":"chat","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"reply"}]}]}})
            );
            let events = decoder
                .push(frame.as_bytes())
                .unwrap()
                .into_iter()
                .map(|e| Ok(ProviderStreamEvent::Model(e)))
                .collect::<Vec<_>>();
            Ok(StreamingResponse {
                headers: ContextHeaders::default(),
                events: Box::pin(futures_util::stream::iter(events)),
            })
        })
    }
    fn embed(
        &self,
        r: EmbeddingRequest,
        c: RequestContext,
    ) -> ProviderFuture<'_, EmbeddingResponse> {
        Box::pin(async move {
            assert!(c.memory_request);
            r.validate()?;
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            if self.slow {
                self.release.notified().await;
            }
            Ok(EmbeddingResponse {
                vectors: r.input.iter().map(|_| vec![1.0, 0.0]).collect(),
            })
        })
    }
}
#[test]
fn manual_defaults_cas_conflicts_export_delete_and_account_isolation() {
    let root = Root::new();
    let chat = Chat::open(&root.0, Namespace::guest()).unwrap();
    let e = chat.memory();
    assert!(!e.automatic().unwrap());
    let id = e.create(draft(Scope::Personal, "vim")).unwrap();
    let base = e.get(id).unwrap().head;
    let first = e
        .correct(id, base, draft(Scope::Personal, "emacs"))
        .unwrap();
    let second = e
        .correct(id, base, draft(Scope::Personal, "helix"))
        .unwrap();
    let m = e.get(id).unwrap();
    assert_eq!(m.state, "disputed");
    assert_eq!(m.head, first);
    assert_eq!(m.revisions.len(), 3);
    assert!(
        m.revisions
            .iter()
            .find(|r| r.id == second)
            .unwrap()
            .conflict
    );
    assert!(
        e.search(&Scope::Personal, "", None, None, 10)
            .unwrap()
            .is_empty()
    );
    e.approve(id, second).unwrap();
    assert_eq!(
        e.search(&Scope::Personal, "helix", Some("editor"), None, 10)
            .unwrap()
            .len(),
        1
    );
    let exported = e.export(&Scope::Personal).unwrap();
    assert!(
        exported.contains("vim") && exported.contains("helix") && exported.contains("preference")
    );
    e.delete(id).unwrap();
    assert!(e.get(id).unwrap().revisions.is_empty());
    assert_eq!(e.export(&Scope::Personal).unwrap(), "[]");
    assert!(e.approve(id, second).is_err());
    assert!(
        e.queue_integration(
            Source::user(Scope::Personal, "helix".into()),
            Privacy::LocalOnly
        )
        .is_err()
    );
    let trust = Trust::local_test("http://localhost:3333/").unwrap();
    let user = Id::try_from("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned()).unwrap();
    chat.switch(Namespace::offline_account(&trust, &user))
        .unwrap();
    assert!(chat.memory().get(id).is_err());
    assert!(!chat.memory().automatic().unwrap());
    let owned = chat
        .memory()
        .create(draft(Scope::Personal, "A only"))
        .unwrap();
    assert_eq!(chat.memory().get(owned).unwrap().owner_user_id, Some(user));
    chat.logout().unwrap();
    assert!(chat.memory().get(owned).is_err());
    assert!(chat.memory().get(id).unwrap().revisions.is_empty());
}
struct Files;
impl EvidenceResolver for Files {
    fn read(&self, _: &Scope, o: &Origin) -> Option<Vec<u8>> {
        match o {
            Origin::File { reference }
            | Origin::Git { reference }
            | Origin::Execution { reference } => std::fs::read(reference).ok(),
            _ => None,
        }
    }
}
#[test]
fn scoped_fresh_file_git_runtime_sources_and_known_credentials() {
    let root = Root::new();
    let chat = Chat::open(&root.0, Namespace::guest()).unwrap();
    let e = chat.memory();
    e.set_resolver(Arc::new(Files)).unwrap();
    let project = Id::try_from("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned()).unwrap();
    let scope = Scope::Project {
        project_id: project.clone(),
    };
    assert!(e.create(draft(scope.clone(), "denied")).is_err());
    e.grant_project(&project).unwrap();
    let path = root.0.join("evidence.txt");
    std::fs::write(&path, b"real version one").unwrap();
    for origin in [
        Origin::File {
            reference: path.to_str().unwrap().into(),
        },
        Origin::Git {
            reference: path.to_str().unwrap().into(),
        },
        Origin::Execution {
            reference: path.to_str().unwrap().into(),
        },
    ] {
        let mut d = draft(scope.clone(), "real version one");
        d.sources = vec![Source::evidence(
            scope.clone(),
            origin,
            b"real version one",
            "real version one".into(),
        )];
        d.kind = if matches!(d.sources[0].origin, Origin::Execution { .. }) {
            Kind::ExecutionResult
        } else {
            Kind::FileEvidence
        };
        e.create(d).unwrap();
    }
    assert_eq!(
        e.search(&scope, "version", None, None, 10).unwrap().len(),
        3
    );
    std::fs::write(&path, b"real version two").unwrap();
    assert!(e.search(&scope, "", None, None, 10).unwrap().is_empty());
    e.revoke_project(&project).unwrap();
    assert!(e.export(&scope).is_err());
    let guard = Arc::new(Redactor::default());
    guard
        .register(&Secret::new("synthetic-known-credential".into()).unwrap())
        .unwrap();
    e.protect_credentials(guard).unwrap();
    assert!(
        e.create(draft(Scope::Personal, "synthetic-known-credential"))
            .is_err()
    );
    assert!(
        e.create(draft(Scope::Personal, "Authorization: bearer example"))
            .is_err()
    );
    assert!(
        e.create(draft(Scope::Personal, "{\"refresh_token\":\"example\"}"))
            .is_err()
    );
    e.create(draft(Scope::Personal, "{ \"editor\" : \"vim\" }"))
        .unwrap();
}
#[tokio::test]
async fn waiting_models_permissions_auto_off_candidates_and_no_replay() {
    let root = Root::new();
    let chat = Chat::open(&root.0, Namespace::guest()).unwrap();
    let e = chat.memory();
    let source = Source::user(Scope::Personal, "preferred editor".into());
    let job = e
        .queue_integration(source.clone(), Privacy::LocalOnly)
        .unwrap();
    assert!(e.run(job).await.unwrap().is_empty());
    e.set_automatic(true).unwrap();
    assert!(e.run(job).await.unwrap().is_empty());
    let p = Mock::new(false);
    e.configure_integration(model(p.clone(), "integrator"))
        .unwrap();
    assert!(e.run(job).await.unwrap().is_empty());
    assert_eq!(p.calls.load(Ordering::SeqCst), 0);
    permit(&e, target("integrator"), Purpose::Integration);
    let ids = e.run(job).await.unwrap();
    assert_eq!(ids.len(), 1);
    let m = e.get(ids[0]).unwrap();
    assert_eq!(m.state, "candidate");
    assert_eq!(m.revisions[0].draft.kind, Kind::AiInference);
    assert!(
        e.search(&Scope::Personal, "", None, None, 10)
            .unwrap()
            .is_empty()
    );
    e.approve(m.id, m.head).unwrap();
    assert_eq!(
        e.search(&Scope::Personal, "editor", None, None, 10)
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(e.run(job).await, Err(Error::AlreadySubmitted)));
    assert_ne!(
        e.queue_integration(source.clone(), Privacy::LocalOnly)
            .unwrap(),
        job
    ); // completed job refs have been purged; content/source dedupe is at commit.
    e.delete(m.id).unwrap();
    assert!(e.queue_integration(source, Privacy::LocalOnly).is_err());
    e.set_automatic(false).unwrap();
    e.create(draft(Scope::Personal, "manual still works"))
        .unwrap();
}
#[tokio::test]
async fn late_integration_results_are_cancelled_on_delete_revoke_or_switch() {
    for action in ["delete", "revoke", "switch", "off", "source"] {
        let root = Root::new();
        let chat = Arc::new(Chat::open(&root.0, Namespace::guest()).unwrap());
        let e = chat.memory();
        e.set_automatic(true).unwrap();
        let p = Mock::new(true);
        e.configure_integration(model(p.clone(), "integrator"))
            .unwrap();
        let permission = permit(&e, target("integrator"), Purpose::Integration);
        let path = root.0.join("evidence.txt");
        std::fs::write(&path, b"original").unwrap();
        e.set_resolver(Arc::new(Files)).unwrap();
        let source = Source::evidence(
            Scope::Personal,
            Origin::File {
                reference: path.to_str().unwrap().into(),
            },
            b"original",
            "original".into(),
        );
        let mut d = draft(Scope::Personal, "original");
        d.sources = vec![source.clone()];
        let memory = e.create(d).unwrap();
        let job = e.queue_integration(source, Privacy::LocalOnly).unwrap();
        let c = chat.clone();
        let task = tokio::spawn(async move { c.memory().run(job).await });
        p.started.notified().await;
        match action {
            "delete" => e.delete(memory).unwrap(),
            "revoke" => e.revoke(permission).unwrap(),
            "off" => e.set_automatic(false).unwrap(),
            "source" => std::fs::write(&path, b"changed").unwrap(),
            _ => chat
                .switch(Namespace::offline_account(
                    &Trust::local_test("http://localhost:3333/").unwrap(),
                    &Id::try_from("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned()).unwrap(),
                ))
                .unwrap(),
        }
        p.release.notify_one();
        assert!(task.await.unwrap().is_err());
        assert!(
            chat.memory()
                .search(&Scope::Personal, "preferred", None, None, 100)
                .unwrap()
                .is_empty()
        );
    }
}
#[tokio::test]
async fn exact_embedding_space_revision_and_manual_fallback() {
    let root = Root::new();
    let chat = Chat::open(&root.0, Namespace::guest()).unwrap();
    let e = chat.memory();
    let id = e.create(draft(Scope::Personal, "vim")).unwrap();
    assert_eq!(
        e.semantic_search(&Scope::Personal, "vim", 10)
            .await
            .unwrap()
            .len(),
        1
    );
    let p = Mock::new(false);
    e.configure_embedding(model(p.clone(), "embedder"), space("v1"))
        .unwrap();
    e.set_automatic(true).unwrap();
    permit(&e, target("embedding"), Purpose::Embedding);
    let job = e.queue_embedding(id).unwrap();
    e.run(job).await.unwrap();
    assert_eq!(
        e.semantic_search(&Scope::Personal, "different wording", 10)
            .await
            .unwrap()
            .len(),
        1
    );
    e.configure_embedding(model(p.clone(), "embedder"), space("v2"))
        .unwrap();
    assert!(
        e.semantic_search(&Scope::Personal, "different wording", 10)
            .await
            .unwrap()
            .is_empty()
    );
    let m = e.get(id).unwrap();
    e.correct(id, m.head, draft(Scope::Personal, "helix"))
        .unwrap();
    e.configure_embedding(model(p, "embedder"), space("v1"))
        .unwrap();
    assert!(
        e.semantic_search(&Scope::Personal, "different wording", 10)
            .await
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn automatic_chat_injection_is_explicit_transient_and_independent() {
    let root = Root::new();
    let chat = Chat::open(&root.0, Namespace::guest()).unwrap();
    let p = Mock::new(false);
    chat.configure(model(p.clone(), "chat-a")).unwrap();
    let e = chat.memory();
    e.create(draft(Scope::Personal, "editor memory")).unwrap();
    permit(&e, target("chat-a"), Purpose::Chat);
    let conv = chat.create_conversation(None, "fixture".into()).unwrap();
    let send = || Send {
        operation: Uuid::new_v4(),
        conversation: conv,
        parent: None,
        content: Content::text("hello"),
    };
    chat.send(send()).await.unwrap();
    assert!(
        !p.inputs.lock().unwrap()[0]
            .to_string()
            .contains("editor memory")
    );
    e.set_automatic(true).unwrap();
    chat.send(send()).await.unwrap();
    assert!(
        p.inputs.lock().unwrap()[1]
            .to_string()
            .contains("editor memory")
    );
    assert!(
        !chat
            .messages(conv, 0, 100)
            .unwrap()
            .iter()
            .any(|m| m.content.text.contains("editor memory"))
    );
    assert_eq!(e.jobs(&Scope::Personal, 100).unwrap().len(), 2);
    chat.configure(model(p.clone(), "chat-b")).unwrap();
    chat.send(send()).await.unwrap();
    assert!(
        !p.inputs.lock().unwrap()[2]
            .to_string()
            .contains("editor memory")
    );
    permit(&e, target("chat-b"), Purpose::Chat);
    chat.send(send()).await.unwrap();
    assert!(
        p.inputs.lock().unwrap()[3]
            .to_string()
            .contains("editor memory")
    );
    e.set_automatic(false).unwrap();
    chat.send(send()).await.unwrap();
    assert!(
        !p.inputs.lock().unwrap()[4]
            .to_string()
            .contains("editor memory")
    );
}
#[test]
fn schema_one_migration_and_restart_unknown_jobs_preserve_chat_and_memory() {
    let root = Root::new();
    let chat = Chat::open(&root.0, Namespace::guest()).unwrap();
    let c = chat.create_conversation(None, "saved chat".into()).unwrap();
    let id = chat
        .memory()
        .create(draft(Scope::Personal, "saved memory"))
        .unwrap();
    let job = chat
        .memory()
        .queue_integration(
            Source::user(Scope::Personal, "waiting".into()),
            Privacy::LocalOnly,
        )
        .unwrap();
    drop(chat);
    let db = rusqlite::Connection::open(root.0.join("guest/cache.sqlite3")).unwrap();
    db.execute(
        "UPDATE memory_jobs SET state='running' WHERE id=?1",
        [job.to_string()],
    )
    .unwrap();
    drop(db);
    let chat = Chat::open(&root.0, Namespace::guest()).unwrap();
    assert_eq!(chat.conversations(None, 10).unwrap()[0].id, c);
    assert_eq!(chat.memory().get(id).unwrap().revisions.len(), 1);
    assert_eq!(
        chat.memory().jobs(&Scope::Personal, 10).unwrap()[0].state,
        "unknown"
    );
    // Build an authentic v1 database by using the immutable v1 DDL, not by changing the v2 version marker.
    drop(chat);
    let legacy = Root::new();
    caidex_credentials::private_directory(&legacy.0).unwrap();
    caidex_credentials::private_directory(&legacy.0.join("guest")).unwrap();
    let db = rusqlite::Connection::open(legacy.0.join("guest/cache.sqlite3")).unwrap();
    db.execute_batch(include_str!("../src/schema.sql")).unwrap();
    db.execute("INSERT INTO identity VALUES('guest')", [])
        .unwrap();
    db.pragma_update(None, "application_id", 0x43414943)
        .unwrap();
    db.pragma_update(None, "user_version", 1).unwrap();
    db.execute(
        "INSERT INTO conversations VALUES(?1,NULL,'legacy')",
        [c.to_string()],
    )
    .unwrap();
    drop(db);
    let chat = Chat::open(&legacy.0, Namespace::guest()).unwrap();
    assert_eq!(chat.conversations(None, 10).unwrap()[0].title, "legacy");
    assert!(!chat.memory().automatic().unwrap());
    chat.memory()
        .create(draft(Scope::Personal, "after migration"))
        .unwrap();
}
