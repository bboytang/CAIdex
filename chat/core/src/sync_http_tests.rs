//! Real database/HTTP with three independent local cache partitions. No commercial model or Key.
use super::*;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use caidex_auth::{
    AUDIENCE, CLI_CLIENT, Trust,
    server::{SandboxMail, Service, digest, router},
};
use serde_json::json;
use std::sync::atomic::{AtomicU8, Ordering};
use tokio::sync::Notify;
use tokio_postgres::{Client, NoTls};

#[derive(Clone, Default)]
struct Fault {
    mode: Arc<AtomicU8>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}
async fn response_fault(State(fault): State<Fault>, request: Request, next: Next) -> Response {
    let data =
        request.uri().path().starts_with("/v1/chat/") && !request.uri().path().ends_with("status");
    let reply = next.run(request).await;
    if data && reply.status() == StatusCode::OK {
        match fault.mode.swap(0, Ordering::SeqCst) {
            1 => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
            2 => {
                fault.entered.notify_one();
                fault.release.notified().await;
            }
            _ => (),
        }
    }
    reply
}
fn check(name: &str, passed: bool) {
    assert!(passed, "I5 {name}");
    println!("I5_CHECK {}", json!({"name":name,"passed":true}));
}
async fn database(dsn: &str) -> Client {
    let (db, connection) = tokio_postgres::connect(dsn, NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    db
}
async fn fixture_session(
    admin: &Client,
    service: &Service<SandboxMail>,
    trust: &Trust,
    user: Uuid,
    n: u8,
) -> Arc<ConfirmedSession> {
    let access = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([n; 32]);
    let refresh = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([n + 10; 32]);
    let sid = Uuid::new_v4();
    admin
        .execute(
            "INSERT INTO caidex.users(user_id) VALUES($1) ON CONFLICT DO NOTHING",
            &[&user],
        )
        .await
        .unwrap();
    admin.execute("INSERT INTO caidex.auth_sessions(session_id,user_id,access_hash,refresh_hash,client_id,audience,platform,access_expires_at,refresh_expires_at,issuer) VALUES($1,$2,$3,$4,'caidex-cli','caidex-data-v1','cli',now()+interval '10 minutes',now()+interval '30 days',$5)",&[&sid,&user,&digest(&access),&digest(&refresh),&trust.issuer()]).await.unwrap();
    let reply = json!({"access_token":access,"refresh_token":refresh,"token_type":"Bearer","expires_in":600,"user_id":user,"session_id":sid,"issuer":trust.issuer(),"client_id":CLI_CLIENT,"audience":AUDIENCE,"authorization_context":null});
    let me = service
        .request("me", Some(&access), "fixture", json!({}))
        .await
        .unwrap();
    Arc::new(ConfirmedSession::confirm(trust, CLI_CLIENT, reply, &me).unwrap())
}
async fn reset_limits(admin: &Client) {
    admin
        .batch_execute("UPDATE caidex.auth_limits SET reset_at=now()-interval '1 second'")
        .await
        .unwrap();
}
async fn drain(chat: &Chat, admin: &Client) {
    for _ in 0..50 {
        reset_limits(admin).await;
        let round = chat.sync_once().await.unwrap();
        if !round.uploaded && !round.downloaded && !chat.sync_status().unwrap().pending_control {
            return;
        }
    }
    panic!("bounded synchronization did not drain");
}
fn seed(
    chat: &Chat,
    c: Uuid,
    parent: Option<Uuid>,
    text: &str,
    attachment: Option<Uuid>,
    end: End,
) -> crate::Message {
    let mut s = chat.state().unwrap();
    let op = Uuid::new_v4();
    s.cache
        .begin(
            c,
            parent,
            Content {
                text: text.into(),
                attachments: attachment.into_iter().collect(),
            },
            op,
        )
        .unwrap();
    s.cache.checkpoint(op, "terminal assistant body").unwrap();
    s.cache.finish(op, end).unwrap()
}
async fn send(
    session: &ConfirmedSession,
    version: i64,
    operation: Uuid,
    entity: Entity,
) -> Result<Receipt> {
    decode(
        session
            .chat_request(
                "push",
                encode(&Push {
                    protocol: PROTOCOL,
                    operation: id(operation),
                    version,
                    entity,
                })
                .unwrap(),
            )
            .await
            .map_err(network)?,
    )
}
#[tokio::test]
#[ignore = "scripts/i5-chat-sync.py provisions isolated real PostgreSQL"]
async fn real_http_chat_sync_and_isolation() {
    let admin = database(&std::env::var("CAIDEX_I5_ADMIN_DB").unwrap()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let trust = Trust::local_test(&format!(
        "http://localhost:{}/",
        listener.local_addr().unwrap().port()
    ))
    .unwrap();
    let service = Service::connect(
        &std::env::var("CAIDEX_I5_AUTH_DB").unwrap(),
        &std::env::var("CAIDEX_I5_DATA_DB").unwrap(),
        trust.clone(),
        "localhost",
        [42; 32],
        SandboxMail::default(),
    )
    .await
    .unwrap();
    let fault = Fault::default();
    let app = router(service.clone()).layer(axum::middleware::from_fn_with_state(
        fault.clone(),
        response_fault,
    ));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let user = Uuid::new_v4();
    let other = Uuid::new_v4();
    let a1 = fixture_session(&admin, &service, &trust, user, 1).await;
    let a2 = fixture_session(&admin, &service, &trust, user, 2).await;
    let b = fixture_session(&admin, &service, &trust, other, 3).await;
    let r1 = std::env::temp_dir().join(format!("caidex-i5-a1-{}", Uuid::new_v4()));
    let r2 = std::env::temp_dir().join(format!("caidex-i5-a2-{}", Uuid::new_v4()));
    let rb = std::env::temp_dir().join(format!("caidex-i5-b-{}", Uuid::new_v4()));
    let d1 = Arc::new(Chat::open(&r1, Namespace::account(&a1)).unwrap());
    let d2 = Arc::new(Chat::open(&r2, Namespace::account(&a2)).unwrap());
    let d3 = Chat::open(&rb, Namespace::account(&b)).unwrap();
    d1.connect_sync(a1.clone()).unwrap();
    d2.connect_sync(a2.clone()).unwrap();
    d3.connect_sync(b.clone()).unwrap();
    let c = d1
        .create_conversation(None, "shared conversation".into())
        .unwrap();
    let ignored = d1.create_conversation(None, "not selected".into()).unwrap();
    let file = d1
        .attach(
            c,
            "evidence.txt".into(),
            "text/plain".into(),
            b"shared attachment bytes".repeat(4000),
        )
        .unwrap();
    let initial = seed(
        &d1,
        c,
        None,
        "original complete user",
        Some(file),
        End::Complete,
    );
    check(
        "default_disabled_and_queues_retained",
        !d1.sync_status().unwrap().enabled
            && !d1.sync_once().await.unwrap().uploaded
            && d1.outbox(0, 100).unwrap().len() == 5,
    );
    check(
        "data_rejected_while_disabled",
        matches!(
            a1.chat_request("pull", json!({"protocol":PROTOCOL,"version":1,"after":0}))
                .await,
            Err(ChatError::Denied)
        ),
    );
    d1.set_chat_sync(true, &[c]).unwrap();
    // Lose the successful control response; retries must use its original operation and base.
    fault.mode.store(1, Ordering::SeqCst);
    check(
        "committed_control_response_lost_keeps_intent",
        matches!(d1.sync_once().await, Err(Error::SyncOffline))
            && d1.sync_status().unwrap().pending_control,
    );
    d1.sync_once().await.unwrap();
    check(
        "control_retry_commits_once",
        d1.sync_status().unwrap().version == 2 && !d1.sync_status().unwrap().pending_control,
    );
    // Lose a committed upload response, then close/reopen its original SQLite outbox.
    fault.mode.store(1, Ordering::SeqCst);
    check(
        "committed_upload_response_lost_keeps_outbox",
        matches!(d1.sync_once().await, Err(Error::SyncOffline)),
    );
    let queued = d1.outbox(0, 100).unwrap()[0].operation;
    drop(d1);
    let d1 = Arc::new(Chat::open(&r1, Namespace::account(&a1)).unwrap());
    d1.connect_sync(a1.clone()).unwrap();
    check(
        "restart_preserves_original_operation",
        d1.outbox(0, 100).unwrap()[0].operation == queued,
    );
    drain(&d1, &admin).await;
    drain(&d2, &admin).await;
    check(
        "two_clients_restore_complete_and_attachment",
        d2.messages(c, 0, 100).unwrap().len() == 2
            && d2.attachment(c, file).unwrap().bytes == b"shared attachment bytes".repeat(4000),
    );
    let mut data = database(&std::env::var("CAIDEX_I5_DATA_DB").unwrap()).await;
    let tx = data.transaction().await.unwrap();
    tx.query_one(
        "SELECT caidex.bind_session($1)",
        &[&base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([1u8; 32])],
    )
    .await
    .unwrap();
    let denial = tx
        .execute(
            "UPDATE caidex.messages SET body='overwrite' WHERE conversation_id=$1",
            &[&c],
        )
        .await
        .unwrap_err();
    check(
        "ordinary_data_role_cannot_mutate_immutable_messages",
        denial.code().is_some_and(|c| c.code() == "42501"),
    );
    tx.rollback().await.unwrap();
    let tx = data.transaction().await.unwrap();
    tx.query_one(
        "SELECT caidex.bind_session($1)",
        &[&base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([1u8; 32])],
    )
    .await
    .unwrap();
    let denial = tx
        .query_one("SELECT caidex.erase_chat_body($1)", &[&c])
        .await
        .unwrap_err();
    check(
        "erasure_function_refuses_active_conversation",
        denial.code().is_some_and(|c| c.code() == "42501"),
    );
    tx.rollback().await.unwrap();
    check(
        "unconfirmed_local_history_never_uploads",
        d2.conversations(None, 100)
            .unwrap()
            .iter()
            .all(|v| v.id != ignored),
    );
    check(
        "download_has_no_outbox_echo",
        d2.outbox(0, 100).unwrap().is_empty(),
    );
    check(
        "memory_authority_remains_disabled",
        !admin
            .query_one(
                "SELECT memory_enabled FROM caidex.users WHERE user_id=$1",
                &[&user],
            )
            .await
            .unwrap()
            .get::<_, bool>(0),
    );
    let left = seed(
        &d1,
        c,
        Some(initial.id),
        "left edit",
        None,
        End::Interrupted,
    );
    let right = seed(&d2, c, Some(initial.id), "right edit", None, End::Cancelled);
    drain(&d1, &admin).await;
    drain(&d2, &admin).await;
    drain(&d1, &admin).await;
    let messages = d1.messages(c, 0, 100).unwrap();
    check(
        "concurrent_branches_and_interrupted_endings_preserved",
        messages.len() == 6
            && messages
                .iter()
                .any(|m| m.id == left.id && m.end == End::Interrupted)
            && messages
                .iter()
                .any(|m| m.id == right.id && m.end == End::Cancelled)
            && messages
                .iter()
                .filter(|m| m.parent == Some(initial.id))
                .count()
                == 2,
    );
    d3.set_chat_sync(true, &[]).unwrap();
    drain(&d3, &admin).await;
    check(
        "other_account_download_empty",
        d3.conversations(None, 100).unwrap().is_empty(),
    );
    let forged = Entity::Message {
        id: id(Uuid::new_v4()),
        conversation: id(c),
        parent: None,
        role: "user".into(),
        text: "forged".into(),
        attachments: vec![],
        ending: "complete".into(),
        created_at_millis: 0,
    };
    check(
        "guessed_other_account_write_refused",
        matches!(
            send(&b, 2, Uuid::new_v4(), forged).await,
            Err(Error::SyncDenied)
        ),
    );
    let project = Uuid::new_v4();
    admin
        .execute(
            "INSERT INTO caidex.projects(project_id,owner_user_id,title) VALUES($1,$2,'private')",
            &[&project, &other],
        )
        .await
        .unwrap();
    check(
        "local_project_marker_does_not_grant_cloud_acl",
        matches!(
            send(
                &a1,
                2,
                Uuid::new_v4(),
                Entity::Conversation {
                    id: id(Uuid::new_v4()),
                    project_id: Some(id(project)),
                    title: "forged project".into()
                }
            )
            .await,
            Err(Error::SyncDenied)
        ),
    );
    let op = Uuid::new_v4();
    let repeat = Entity::Conversation {
        id: id(c),
        project_id: None,
        title: "shared conversation".into(),
    };
    let first = send(&a1, 2, op, repeat.clone()).await.unwrap();
    let again = send(&a1, 2, op, repeat.clone()).await.unwrap();
    check(
        "same_operation_same_payload_one_result",
        first.sequence == again.sequence && first.hash == again.hash,
    );
    let changed = Entity::Conversation {
        id: id(c),
        project_id: None,
        title: "different payload".into(),
    };
    check(
        "same_operation_different_payload_conflict",
        matches!(send(&a1, 2, op, changed).await, Err(Error::SyncConflict)),
    );
    check(
        "old_protocol_and_unknown_fields_refused",
        matches!(
            a1.chat_request("pull", json!({"protocol":99,"version":2,"after":0}))
                .await,
            Err(ChatError::Invalid)
        ) && matches!(
            a1.chat_request(
                "pull",
                json!({"protocol":1,"version":2,"after":0,"host_execute":true})
            )
            .await,
            Err(ChatError::Invalid)
        ),
    );
    check(
        "future_cursor_refused",
        matches!(
            a1.chat_request("pull", json!({"protocol":1,"version":2,"after":i64::MAX}))
                .await,
            Err(ChatError::Invalid)
        ),
    );
    reset_limits(&admin).await;
    // The data transaction rolls back its message when event persistence fails.
    admin.batch_execute("CREATE FUNCTION caidex.i5_injected_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.entity_kind='message' THEN RAISE EXCEPTION 'injected event failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER i5_failure BEFORE INSERT ON caidex.sync_events FOR EACH ROW EXECUTE FUNCTION caidex.i5_injected_failure();").await.unwrap();
    let failed = Uuid::new_v4();
    let entity = Entity::Message {
        id: id(failed),
        conversation: id(c),
        parent: Some(id(initial.id)),
        role: "user".into(),
        text: "rollback body".into(),
        attachments: vec![],
        ending: "complete".into(),
        created_at_millis: 0,
    };
    check(
        "event_failure_rolls_back_data",
        matches!(
            send(&a1, 2, Uuid::new_v4(), entity).await,
            Err(Error::SyncOffline)
        ) && admin
            .query_one(
                "SELECT count(*) FROM caidex.messages WHERE message_id=$1",
                &[&failed],
            )
            .await
            .unwrap()
            .get::<_, i64>(0)
            == 0,
    );
    admin.batch_execute("DROP TRIGGER i5_failure ON caidex.sync_events; DROP FUNCTION caidex.i5_injected_failure();").await.unwrap();
    // Hold a successful page after the service has committed, switch account, then release it.
    fault.mode.store(2, Ordering::SeqCst);
    let running = {
        let d2 = d2.clone();
        tokio::spawn(async move { d2.sync_once().await })
    };
    fault.entered.notified().await;
    d2.switch(Namespace::account(&b)).unwrap();
    fault.release.notify_one();
    check(
        "late_page_after_account_switch_refused",
        matches!(running.await.unwrap(), Err(Error::Cancelled))
            && d2.conversations(None, 100).unwrap().is_empty(),
    );
    d2.switch(Namespace::account(&a2)).unwrap();
    d2.connect_sync(a2.clone()).unwrap();
    // Stop before releasing an already successful enable control: the client stays stopped.
    d2.set_chat_sync(true, &[c]).unwrap();
    d1.set_chat_sync(false, &[]).unwrap();
    d1.sync_once().await.unwrap();
    check(
        "stale_control_cas_conflict_keeps_local_intent",
        matches!(d2.sync_once().await, Err(Error::SyncConflict))
            && d2.sync_status().unwrap().pending_control
            && !d2.sync_status().unwrap().enabled,
    );
    d2.discard_chat_control_conflict().unwrap();
    d2.sync_once().await.unwrap();
    check(
        "explicit_conflict_resolution_inherits_current_disabled",
        !d2.sync_status().unwrap().enabled && !d2.sync_status().unwrap().pending_control,
    );
    check(
        "confirmed_stop_rejects_new_data",
        matches!(
            a2.chat_request("pull", json!({"protocol":1,"version":2,"after":0}))
                .await,
            Err(ChatError::Denied)
        ),
    );
    d1.set_chat_sync(true, &[c]).unwrap();
    fault.mode.store(2, Ordering::SeqCst);
    let running = {
        let d1 = d1.clone();
        tokio::spawn(async move { d1.sync_once().await })
    };
    fault.entered.notified().await;
    d1.set_chat_sync(false, &[]).unwrap();
    fault.release.notify_one();
    running.await.unwrap().unwrap();
    check(
        "offline_stop_survives_late_enable_result",
        !d1.sync_status().unwrap().enabled && d1.sync_status().unwrap().pending_control,
    );
    d1.sync_once().await.unwrap();
    check(
        "stop_control_prioritized_before_any_data",
        !d1.sync_status().unwrap().enabled && !d1.sync_status().unwrap().pending_control,
    );
    d1.set_chat_sync(true, &[c]).unwrap();
    d1.sync_once().await.unwrap();
    drain(&d2, &admin).await;
    let own_project = Uuid::new_v4();
    admin.execute("INSERT INTO caidex.projects(project_id,owner_user_id,title) VALUES($1,$2,'owned private project')",&[&own_project,&user]).await.unwrap();
    let project_chat = d1
        .create_conversation(Some(id(own_project)), "project conversation".into())
        .unwrap();
    seed(
        &d1,
        project_chat,
        None,
        "project-specific user",
        None,
        End::Complete,
    );
    d1.set_chat_sync(true, &[project_chat]).unwrap();
    d1.sync_once().await.unwrap();
    drain(&d1, &admin).await;
    drain(&d2, &admin).await;
    check(
        "owned_project_scope_and_messages_restore",
        d2.conversations(None, 100)
            .unwrap()
            .iter()
            .any(|c| c.id == project_chat && c.project_id == Some(id(own_project)))
            && d2.messages(project_chat, 0, 100).unwrap().len() == 2,
    );
    let version = d1.sync_status().unwrap().version;
    check(
        "stale_settings_version_refused",
        matches!(
            send(&a1, 2, Uuid::new_v4(), repeat.clone()).await,
            Err(Error::SyncConflict)
        ),
    );
    let secret = Entity::Conversation {
        id: id(Uuid::new_v4()),
        project_id: None,
        title: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([1u8; 32]),
    };
    check(
        "known_account_token_in_payload_refused",
        matches!(
            send(&a1, version, Uuid::new_v4(), secret).await,
            Err(Error::Invalid)
        ),
    );
    let cross = Entity::Message {
        id: id(Uuid::new_v4()),
        conversation: id(project_chat),
        parent: Some(id(initial.id)),
        role: "user".into(),
        text: "cross scope".into(),
        attachments: vec![id(file)],
        ending: "complete".into(),
        created_at_millis: 0,
    };
    check(
        "same_account_cross_project_parent_and_attachment_refused",
        matches!(
            send(&a1, version, Uuid::new_v4(), cross).await,
            Err(Error::Invalid)
        ),
    );
    let memory = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let source = Uuid::new_v4();
    admin
        .execute(
            "INSERT INTO caidex.memories(memory_id,user_id,privacy) VALUES($1,$2,'sync_allowed')",
            &[&memory, &user],
        )
        .await
        .unwrap();
    admin.execute("INSERT INTO caidex.memory_revisions(revision_id,memory_id,user_id,author_user_id,content,kind) VALUES($1,$2,$3,$3,'independent approved memory','user_decision')",&[&revision,&memory,&user]).await.unwrap();
    admin.execute("INSERT INTO caidex.memory_sources(source_id,revision_id,user_id,message_id,source_kind,source_ref) VALUES($1,$2,$3,$4,'chat','immutable Chat source reference')",&[&source,&revision,&user,&initial.id]).await.unwrap();
    d1.delete_conversation(c).unwrap();
    drain(&d1, &admin).await;
    drain(&d2, &admin).await;
    check("deletion_purges_cloud_bodies_and_attachment_bytes",admin.query_one("SELECT (SELECT count(*) FROM caidex.messages WHERE conversation_id=$1 AND (body<>'' OR cardinality(attachment_ids)>0))+(SELECT count(*) FROM caidex.attachments WHERE conversation_id=$1)",&[&c]).await.unwrap().get::<_,i64>(0)==0 && admin.query_one("SELECT title FROM caidex.conversations WHERE conversation_id=$1",&[&c]).await.unwrap().get::<_,String>(0).is_empty());
    check(
        "chat_erasure_preserves_independent_memory_and_source_identity",
        admin
            .query_one(
                "SELECT content FROM caidex.memory_revisions WHERE revision_id=$1",
                &[&revision],
            )
            .await
            .unwrap()
            .get::<_, String>(0)
            == "independent approved memory"
            && admin
                .query_one(
                    "SELECT message_id FROM caidex.memory_sources WHERE source_id=$1",
                    &[&source],
                )
                .await
                .unwrap()
                .get::<_, Option<Uuid>>(0)
                == Some(initial.id),
    );
    check(
        "second_client_applies_deletion_without_resurrection",
        d2.messages(c, 0, 100).is_err()
            && d2
                .conversations(None, 100)
                .unwrap()
                .iter()
                .all(|v| v.id != c),
    );
    check(
        "old_device_new_operation_cannot_resurrect",
        matches!(
            send(&a2, version, Uuid::new_v4(), repeat).await,
            Err(Error::SyncConflict)
        ),
    );
    check(
        "old_confirmed_operation_returns_receipt_without_body",
        send(
            &a1,
            version,
            op,
            Entity::Conversation {
                id: id(c),
                project_id: None,
                title: "shared conversation".into(),
            },
        )
        .await
        .unwrap()
        .sequence
            == first.sequence,
    );
    check("minimum_permanent_tombstone_exists",admin.query_one("SELECT count(*) FROM caidex.deletion_tombstones WHERE entity_id=$1 AND retain_until='infinity'",&[&c]).await.unwrap().get::<_,i64>(0)==1);
    check("acknowledged_cursor_persisted_per_auth_session",admin.query_one("SELECT count(*) FROM caidex.sync_cursors WHERE user_id=$1 AND channel='chat' AND acknowledged_seq>0",&[&user]).await.unwrap().get::<_,i64>(0)==2);
    admin
        .execute(
            "UPDATE caidex.auth_sessions SET revoked_at=now() WHERE session_id=$1",
            &[&Uuid::parse_str(a1.session_id()).unwrap()],
        )
        .await
        .unwrap();
    check(
        "revoked_session_refused_without_queue_loss",
        matches!(d1.sync_once().await, Err(Error::SyncDenied)),
    );
    d2.logout().unwrap();
    check(
        "logout_locks_account_and_drops_sync_session",
        d2.conversations(None, 100).unwrap().is_empty()
            && matches!(d2.sync_once().await, Err(Error::SyncOffline)),
    );
    server.abort();
    drop(d1);
    drop(d2);
    drop(d3);
    drop(service);
    for root in [r1, r2, rb] {
        std::fs::remove_dir_all(root).unwrap();
    }
}
