//! Real HTTP and ordinary PostgreSQL roles; synthetic identities, no commercial API or email.
use super::*;
use crate::sync::http_tests::{Fault, database, fixture_session, reset_limits, response_fault};
use base64::Engine;
use caidex_auth::{
    Trust,
    server::{SandboxMail, Service},
};
use serde_json::json;
use std::sync::atomic::Ordering;
use tokio_postgres::Client;

fn check(name: &str, passed: bool) {
    assert!(passed, "I6 {name}");
    println!("I6_CHECK {}", json!({"name":name,"passed":true}));
}
fn draft(scope: Scope, text: &str) -> Draft {
    Draft {
        scope: scope.clone(),
        text: text.into(),
        kind: Kind::Preference,
        privacy: Privacy::Shareable,
        tags: vec!["sync".into()],
        sources: vec![Source::user(scope, text.into())],
        valid_until_millis: None,
    }
}
async fn drain(chat: &Chat, admin: &Client) {
    for _ in 0..80 {
        reset_limits(admin).await;
        let r = chat.memory().sync_once(None).await.unwrap();
        assert!(!r.needs_epoch_choice);
        if !r.uploaded
            && !r.downloaded
            && !r.control
            && !r.more
            && !chat.memory().sync_status().unwrap().pending_control
        {
            return;
        }
    }
    panic!("Memory sync failed to drain");
}
async fn ctrl(s: &ConfirmedSession) -> Control {
    reply(
        s.memory_request("status", json!({"protocol":PROTOCOL}))
            .await
            .unwrap(),
    )
    .unwrap()
}
async fn scope(s: &ConfirmedSession, p: Option<Uuid>) -> Permit {
    reply(
        s.memory_request("scope", json!({"protocol":PROTOCOL,"project":p}))
            .await
            .unwrap(),
    )
    .unwrap()
}
async fn send(
    s: &ConfirmedSession,
    p: Permit,
    operation: Uuid,
    change: Change,
) -> std::result::Result<serde_json::Value, ChatError> {
    s.memory_request(
        "push",
        value(&Push {
            protocol: PROTOCOL,
            operation,
            version: ctrl(s).await.version,
            permit: p,
            change,
        })
        .unwrap(),
    )
    .await
}
async fn pull(
    s: &ConfirmedSession,
    p: Permit,
) -> std::result::Result<serde_json::Value, ChatError> {
    s.memory_request(
        "pull",
        value(&Pull {
            protocol: PROTOCOL,
            version: ctrl(s).await.version,
            permit: p,
            after: 0,
        })
        .unwrap(),
    )
    .await
}
async fn direct_count(db: &mut Client, n: u8, owner: Uuid, p: Option<Uuid>) -> i64 {
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([n; 32]);
    let tx = db.transaction().await.unwrap();
    tx.execute("SELECT caidex.bind_session($1)", &[&token])
        .await
        .unwrap();
    let count=tx.query_one("SELECT count(*) FROM caidex.memories WHERE user_id=$1 AND scope_key=coalesce($2::uuid::text,'personal')",&[&owner,&p]).await.unwrap().get(0);
    tx.commit().await.unwrap();
    count
}
#[tokio::test]
#[ignore = "scripts/i6-memory-sync.py provisions isolated real PostgreSQL"]
async fn real_http_memory_sync_and_isolation() {
    let admin = database(&std::env::var("CAIDEX_I6_ADMIN_DB").unwrap()).await;
    let mut ordinary = database(&std::env::var("CAIDEX_I6_DATA_DB").unwrap()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let trust = Trust::local_test(&format!(
        "http://localhost:{}/",
        listener.local_addr().unwrap().port()
    ))
    .unwrap();
    let mail = SandboxMail::default();
    let service = Service::connect(
        &std::env::var("CAIDEX_I6_AUTH_DB").unwrap(),
        &std::env::var("CAIDEX_I6_DATA_DB").unwrap(),
        trust.clone(),
        "localhost",
        [42; 32],
        mail.clone(),
    )
    .await
    .unwrap();
    let password = "Synthetic-I6-test-password-2026!";
    service
        .request(
            "register",
            None,
            "fixture",
            json!({"email":"i6-owner@example.test","password":password}),
        )
        .await
        .unwrap();
    let token = mail
        .0
        .lock()
        .await
        .last()
        .unwrap()
        .verification_token
        .clone()
        .unwrap();
    let verified = service
        .request(
            "verify",
            None,
            "fixture",
            json!({"email":"i6-owner@example.test","token":token,"purpose":"register"}),
        )
        .await
        .unwrap();
    let owner: Uuid = reply(verified["user_id"].clone()).unwrap();
    let member = Uuid::new_v4();
    let writer = Uuid::new_v4();
    let a1 = fixture_session(&admin, &service, &trust, owner, 11).await;
    let a2 = fixture_session(&admin, &service, &trust, owner, 12).await;
    let b = fixture_session(&admin, &service, &trust, member, 13).await;
    let c = fixture_session(&admin, &service, &trust, writer, 14).await;
    let fault = Fault::default();
    let app = caidex_auth::server::router(service.clone()).layer(
        axum::middleware::from_fn_with_state(fault.clone(), response_fault),
    );
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let root = std::env::temp_dir().join(format!("caidex-i6-{}", Uuid::new_v4()));
    caidex_credentials::private_directory(&root).unwrap();
    let d1 = Arc::new(Chat::open(&root.join("a1"), Namespace::account(&a1)).unwrap());
    let d2 = Arc::new(Chat::open(&root.join("a2"), Namespace::account(&a2)).unwrap());
    let dm = Chat::open(&root.join("member"), Namespace::account(&b)).unwrap();
    let dw = Chat::open(&root.join("writer"), Namespace::account(&c)).unwrap();
    d1.connect_sync(a1.clone()).unwrap();
    d2.connect_sync(a2.clone()).unwrap();
    dm.connect_sync(b.clone()).unwrap();
    dw.connect_sync(c.clone()).unwrap();
    let id = d1
        .memory()
        .create(draft(Scope::Personal, "private selected memory"))
        .unwrap();
    let unselected = d1
        .memory()
        .create(draft(Scope::Personal, "unselected local history"))
        .unwrap();
    drain(&d1, &admin).await;
    check(
        "default_off_local_memory_and_no_cloud_upload",
        !ctrl(&a1).await.enabled
            && admin
                .query_one(
                    "SELECT count(*) FROM caidex.memories WHERE user_id=$1",
                    &[&owner],
                )
                .await
                .unwrap()
                .get::<_, i64>(0)
                == 0
            && d1.memory().get(id).is_ok(),
    );
    check(
        "guest_cannot_enable",
        Chat::open(&root.join("guest"), Namespace::guest())
            .unwrap()
            .memory()
            .set_sync(Mode::Enable, &[Scope::Personal], &[])
            .is_err(),
    );
    check(
        "chat_auto_provider_permissions_independent",
        !d1.sync_status().unwrap().enabled
            && !d1.memory().automatic().unwrap()
            && d1
                .memory()
                .permissions(&Scope::Personal)
                .unwrap()
                .is_empty(),
    );
    d1.memory()
        .set_sync(Mode::Enable, &[Scope::Personal], &[id])
        .unwrap();
    fault.mode.store(1, Ordering::SeqCst);
    check(
        "lost_control_response_retains_original_intent",
        matches!(d1.memory().sync_once(None).await, Err(Error::SyncOffline))
            && d1.memory().sync_status().unwrap().pending_control,
    );
    drain(&d1, &admin).await;
    drain(&d2, &admin).await;
    check(
        "inherit_enabled_download_without_upload_consent",
        d2.memory().get(id).unwrap().revisions[0].draft.text == "private selected memory"
            && d2
                .state()
                .unwrap()
                .cache
                .db
                .query_row("SELECT count(*) FROM memory_sync_uploads", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap()
                == 0,
    );
    check(
        "first_upload_exact_selection",
        admin
            .query_opt(
                "SELECT 1 FROM caidex.memories WHERE memory_id=$1",
                &[&unselected],
            )
            .await
            .unwrap()
            .is_none(),
    );
    let base = d1.memory().get(id).unwrap().head;
    let r1 = d1
        .memory()
        .correct(id, base, draft(Scope::Personal, "device one revision"))
        .unwrap();
    let r2 = d2
        .memory()
        .correct(id, base, draft(Scope::Personal, "device two revision"))
        .unwrap();
    d2.memory().select_uploads(&[id]).unwrap();
    fault.mode.store(1, Ordering::SeqCst);
    check(
        "lost_upload_response_retains_stable_operation",
        matches!(d1.memory().sync_once(None).await, Err(Error::SyncOffline)),
    );
    drain(&d1, &admin).await;
    drain(&d2, &admin).await;
    drain(&d1, &admin).await;
    let merged = d1.memory().get(id).unwrap();
    check(
        "two_offline_revisions_preserved_disputed",
        merged.state == "disputed"
            && merged.revisions.iter().any(|r| r.id == r1)
            && merged.revisions.iter().any(|r| r.id == r2)
            && admin
                .query_one(
                    "SELECT count(*) FROM caidex.memory_revisions WHERE memory_id=$1",
                    &[&id],
                )
                .await
                .unwrap()
                .get::<_, i64>(0)
                == 3,
    );
    d1.memory().approve(id, r2).unwrap();
    drain(&d1, &admin).await;
    drain(&d2, &admin).await;
    check(
        "explicit_generation_checked_conflict_resolution",
        d2.memory().get(id).unwrap().head == r2 && d2.memory().get(id).unwrap().state == "active",
    );
    let grant = scope(&a1, None).await;
    let operation = Uuid::new_v4();
    let stable = Change::Upsert {
        id: Uuid::new_v4(),
        revision: Uuid::new_v4(),
        base: None,
        created: 10,
        draft: draft(Scope::Personal, "idempotent content"),
        recreate: false,
        candidate: false,
    };
    let first = send(&a1, grant.clone(), operation, stable.clone())
        .await
        .unwrap();
    let second = send(&a1, grant.clone(), operation, stable.clone())
        .await
        .unwrap();
    check(
        "same_operation_same_receipt_once",
        first == second
            && admin
                .query_one(
                    "SELECT count(*) FROM caidex.sync_events WHERE operation_id=$1",
                    &[&operation],
                )
                .await
                .unwrap()
                .get::<_, i64>(0)
                == 1,
    );
    let mut changed = stable.clone();
    if let Change::Upsert { draft, .. } = &mut changed {
        draft.text = "changed payload".into();
    }
    check(
        "same_operation_changed_payload_rejected",
        matches!(
            send(&a1, grant.clone(), operation, changed).await,
            Err(ChatError::Conflict)
        ),
    );
    reset_limits(&admin).await;
    let p = Uuid::new_v4();
    let otherp = Uuid::new_v4();
    admin.execute("INSERT INTO caidex.projects(project_id,owner_user_id,title) VALUES($1,$2,'owner project'),($3,$4,'member independent project')",&[&p,&owner,&otherp,&member]).await.unwrap();
    admin.execute("INSERT INTO caidex.project_members(project_id,user_id,role,granted_by) VALUES($1,$2,'reader',$4),($1,$3,'writer',$4)",&[&p,&member,&writer,&owner]).await.unwrap();
    let project_scope = Scope::Project {
        project_id: Id::try_from(p.to_string()).unwrap(),
    };
    let other_scope = Scope::Project {
        project_id: Id::try_from(otherp.to_string()).unwrap(),
    };
    d1.memory()
        .grant_project(&Id::try_from(p.to_string()).unwrap())
        .unwrap();
    dm.memory()
        .grant_project(&Id::try_from(p.to_string()).unwrap())
        .unwrap();
    dm.memory()
        .grant_project(&Id::try_from(otherp.to_string()).unwrap())
        .unwrap();
    dw.memory()
        .grant_project(&Id::try_from(p.to_string()).unwrap())
        .unwrap();
    let project_id = d1
        .memory()
        .create(draft(project_scope.clone(), "shared project memory"))
        .unwrap();
    let member_id = dm
        .memory()
        .create(draft(Scope::Personal, "member private memory"))
        .unwrap();
    let other_id = dm
        .memory()
        .create(draft(other_scope.clone(), "member other project"))
        .unwrap();
    d1.memory()
        .set_sync(
            Mode::Enable,
            &[Scope::Personal, project_scope.clone()],
            &[project_id],
        )
        .unwrap();
    dm.memory()
        .set_sync(
            Mode::Enable,
            &[Scope::Personal, project_scope.clone(), other_scope.clone()],
            &[member_id, other_id],
        )
        .unwrap();
    dw.memory()
        .set_sync(Mode::Enable, std::slice::from_ref(&project_scope), &[])
        .unwrap();
    drain(&d1, &admin).await;
    drain(&dm, &admin).await;
    drain(&dw, &admin).await;
    drain(&d2, &admin).await;
    check(
        "reader_real_rls_can_read_owner_project",
        direct_count(&mut ordinary, 13, owner, Some(p)).await == 1
            && dm.memory().get(project_id).unwrap().owner_user_id
                == Some(Id::try_from(owner.to_string()).unwrap()),
    );
    let member_grant = scope(&b, Some(p)).await;
    let writer_grant = scope(&c, Some(p)).await;
    let update = Change::Upsert {
        id: project_id,
        revision: Uuid::new_v4(),
        base: Some(d1.memory().get(project_id).unwrap().head),
        created: 20,
        draft: draft(project_scope.clone(), "writer change"),
        recreate: false,
        candidate: false,
    };
    check(
        "reader_cannot_write_project",
        matches!(
            send(&b, member_grant.clone(), Uuid::new_v4(), update.clone()).await,
            Err(ChatError::Denied)
        ),
    );
    check(
        "writer_real_http_can_write",
        send(&c, writer_grant.clone(), Uuid::new_v4(), update)
            .await
            .is_ok(),
    );
    drain(&dm, &admin).await;
    drain(&d1, &admin).await;
    let old = scope(&a1, None).await;
    d1.memory().set_sync(Mode::Keep, &[], &[]).unwrap();
    check(
        "offline_stop_immediately_blocks_local_without_claiming_global",
        !d1.memory().sync_status().unwrap().enabled && ctrl(&a2).await.enabled,
    );
    // Reconfigure the same namespace: pending stop is not overwritten by login.
    d1.connect_sync(a1.clone()).unwrap();
    drain(&d1, &admin).await;
    check("A_retains_cloud_body_and_acl",admin.query_one("SELECT count(*) FROM caidex.memory_revisions WHERE user_id=$1 AND content<>''",&[&owner]).await.unwrap().get::<_,i64>(0)>0&&admin.query_one("SELECT count(*) FROM caidex.project_members WHERE project_id=$1 AND revoked_at IS NULL",&[&p]).await.unwrap().get::<_,i64>(0)==2);
    check(
        "owner_off_all_members_rls_access_paused",
        direct_count(&mut ordinary, 13, owner, Some(p)).await == 0
            && direct_count(&mut ordinary, 14, owner, Some(p)).await == 0
            && !scope(&b, Some(p)).await.enabled,
    );
    check(
        "A_rejects_old_upload_download_and_body_export",
        matches!(pull(&a1, old.clone()).await, Err(ChatError::Denied))
            && matches!(
                send(&a1, old, Uuid::new_v4(), stable.clone()).await,
                Err(ChatError::Denied)
            )
            && a1
                .memory_request("export", json!({"protocol":PROTOCOL}))
                .await
                .is_err(),
    );
    check(
        "local_export_keeps_working_while_cloud_off",
        d1.memory()
            .export(&Scope::Personal)
            .unwrap()
            .contains("device two revision"),
    );
    d1.memory()
        .set_sync(Mode::Enable, &[Scope::Personal], &[])
        .unwrap();
    drain(&d1, &admin).await;
    check(
        "reenable_excluded_project_remains_denied",
        direct_count(&mut ordinary, 13, owner, Some(p)).await == 0,
    );
    d1.memory()
        .set_sync(Mode::Enable, &[Scope::Personal, project_scope.clone()], &[])
        .unwrap();
    drain(&d1, &admin).await;
    check(
        "reenable_current_scope_and_acl_restores_access",
        direct_count(&mut ordinary, 13, owner, Some(p)).await == 1,
    );
    dm.memory().set_sync(Mode::Keep, &[], &[]).unwrap();
    drain(&dm, &admin).await;
    check(
        "member_own_sync_off_cannot_bypass_project",
        direct_count(&mut ordinary, 13, owner, Some(p)).await == 0,
    );
    dm.memory()
        .set_sync(
            Mode::Enable,
            &[Scope::Personal, project_scope.clone(), other_scope.clone()],
            &[],
        )
        .unwrap();
    drain(&dm, &admin).await;
    // Two independent service workers exercise actual owner locks, beyond the single-worker gate.
    reset_limits(&admin).await;
    let closer = Service::connect(
        &std::env::var("CAIDEX_I6_AUTH_DB").unwrap(),
        &std::env::var("CAIDEX_I6_DATA_DB").unwrap(),
        trust.clone(),
        "localhost",
        [42; 32],
        mail.clone(),
    )
    .await
    .unwrap();
    let observer = database(&std::env::var("CAIDEX_I6_ADMIN_DB").unwrap()).await;
    let mut blocker = database(&std::env::var("CAIDEX_I6_ADMIN_DB").unwrap()).await;
    let pg = scope(&c, Some(p)).await;
    let cv = ctrl(&c).await.version;
    let ov = ctrl(&a1).await.version;
    let base: Uuid = admin
        .query_one(
            "SELECT current_revision_id FROM caidex.memories WHERE memory_id=$1",
            &[&project_id],
        )
        .await
        .unwrap()
        .get(0);
    let delayed_revision = Uuid::new_v4();
    let upload = Push {
        protocol: PROTOCOL,
        operation: Uuid::new_v4(),
        version: cv,
        permit: pg.clone(),
        change: Change::Upsert {
            id: project_id,
            revision: delayed_revision,
            base: Some(base),
            created: 25,
            draft: draft(
                project_scope.clone(),
                "ordered member commit before owner close",
            ),
            recreate: false,
            candidate: false,
        },
    };
    let lock = blocker.transaction().await.unwrap();
    lock.batch_execute("LOCK caidex.memory_revisions IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let ws = c.clone();
    let writer_request =
        tokio::spawn(async move { ws.memory_request("push", value(&upload).unwrap()).await });
    let mut entered = false;
    for _ in 0..150 {
        entered=observer.query_one("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE relation='caidex.memory_revisions'::regclass AND NOT granted)",&[]).await.unwrap().get(0);
        if entered {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        entered,
        "real member transaction did not reach locked revisions"
    );
    let close = SetControl {
        protocol: PROTOCOL,
        operation: Uuid::new_v4(),
        base_version: ov,
        mode: Mode::Keep,
        scopes: vec![],
    };
    let access_for_close = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([11; 32]);
    let closed = tokio::spawn(async move {
        closer
            .request(
                "memory-control",
                Some(&access_for_close),
                "fixture-close",
                value(&close).unwrap(),
            )
            .await
    });
    let mut waiting = false;
    for _ in 0..150 {
        waiting=observer.query_one("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE wait_event_type='Lock' AND query LIKE '%users%' AND query LIKE '%FOR NO KEY UPDATE%')",&[]).await.unwrap().get(0);
        if waiting {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    check(
        "owner_close_waits_for_actual_member_transaction",
        waiting && !closed.is_finished(),
    );
    lock.commit().await.unwrap();
    let written = writer_request.await.unwrap().unwrap();
    assert!(written["sequence"].as_i64().unwrap() > 0);
    let stopped: Control = reply(closed.await.unwrap().unwrap()).unwrap();
    check(
        "member_commit_ordered_before_owner_close_confirmation",
        !stopped.enabled
            && admin
                .query_opt(
                    "SELECT 1 FROM caidex.memory_revisions WHERE revision_id=$1",
                    &[&delayed_revision],
                )
                .await
                .unwrap()
                .is_some()
            && pull(&c, pg).await.is_err(),
    );
    d1.memory().sync_once(None).await.unwrap();
    d1.memory()
        .set_sync(Mode::Enable, &[Scope::Personal, project_scope.clone()], &[])
        .unwrap();
    drain(&d1, &admin).await;
    admin.execute("UPDATE caidex.project_members SET revoked_at=now(),authorization_version=authorization_version+1 WHERE project_id=$1 AND user_id=$2",&[&p,&writer]).await.unwrap();
    check(
        "revoked_writer_cannot_commit_or_read",
        send(
            &c,
            writer_grant,
            Uuid::new_v4(),
            Change::Delete {
                id: project_id,
                sources: vec![],
            },
        )
        .await
        .is_err()
            && direct_count(&mut ordinary, 14, owner, Some(p)).await == 0,
    );
    reset_limits(&admin).await;
    let before = ctrl(&a1).await;
    let stale = SetControl {
        protocol: PROTOCOL,
        operation: Uuid::new_v4(),
        base_version: before.version - 1,
        mode: Mode::Keep,
        scopes: vec![],
    };
    check(
        "stale_control_CAS_rejected",
        matches!(
            a2.memory_request("control", value(&stale).unwrap()).await,
            Err(ChatError::Conflict)
        ),
    );
    let secret = draft(
        Scope::Personal,
        "api_key=sk-proj-SyntheticSecretNeverSyncI6",
    );
    let sc = Change::Upsert {
        id: Uuid::new_v4(),
        revision: Uuid::new_v4(),
        base: None,
        created: 30,
        draft: secret,
        recreate: false,
        candidate: false,
    };
    check(
        "known_secret_boundary_rejected",
        send(&a1, scope(&a1, None).await, Uuid::new_v4(), sc)
            .await
            .is_err(),
    );
    // Keep an independent Chat and cloud vector/job so B must erase all Memory bytes only.
    let conversation = d1
        .create_conversation(None, "Chat must survive B".into())
        .unwrap();
    admin.execute("INSERT INTO caidex.conversations(user_id,conversation_id,title) VALUES($1,$2,'cloud Chat survives')",&[&owner,&conversation]).await.unwrap();
    let space = Uuid::new_v4();
    let current = ctrl(&a1).await;
    admin.execute("INSERT INTO caidex.embedding_spaces(embedding_space_id,user_id,provider_ref,endpoint_ref,model_ref,model_revision,preprocessing_version,dimensions,distance) VALUES($1,$2,'synthetic','local','test','v1',1,2,'cosine')",&[&space,&owner]).await.unwrap();
    admin.execute("INSERT INTO caidex.memory_embeddings(user_id,revision_id,embedding_space_id,dimensions,embedding) VALUES($1,$2,$3,2,'[1,0]')",&[&owner,&r2,&space]).await.unwrap();
    let job = Uuid::new_v4();
    admin.execute("INSERT INTO caidex.memory_jobs(job_id,user_id,base_revision_id,kind,execution_owner_ref,operation_id,cloud_epoch,settings_version) VALUES($1,$2,$3,'embed','explicit offline host',$4,$5,$6)",&[&job,&owner,&r2,&Uuid::new_v4(),&current.epoch,&current.version]).await.unwrap();
    let unsent = d2
        .memory()
        .create(draft(Scope::Personal, "unsent device two before B"))
        .unwrap();
    d2.memory().select_uploads(&[unsent]).unwrap();
    let old_grant = scope(&a1, None).await;
    d1.memory().set_sync(Mode::Delete, &[], &[]).unwrap();
    check(
        "B_requires_fresh_reauthentication",
        matches!(d1.memory().sync_once(None).await, Err(Error::SyncDenied))
            && !d1.memory().sync_status().unwrap().enabled,
    );
    let access = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([11; 32]);
    let proof = service
        .request(
            "reauth",
            Some(&access),
            "fixture",
            json!({"password":password}),
        )
        .await
        .unwrap()["proof"]
        .as_str()
        .unwrap()
        .to_owned();
    let intent: String = d1
        .state()
        .unwrap()
        .cache
        .db
        .query_row("SELECT intent FROM memory_sync", [], |r| r.get(0))
        .unwrap();
    let mut wrong: serde_json::Value = serde_json::from_str(&intent).unwrap();
    wrong["proof"] = proof.clone().into();
    check(
        "B_proof_bound_to_session",
        a2.memory_request("control", wrong).await.is_err(),
    );
    fault.mode.store(1, Ordering::SeqCst);
    check(
        "B_lost_barrier_response_original_retry_without_proof",
        matches!(
            d1.memory().sync_once(Some(&proof)).await,
            Err(Error::SyncOffline)
        ),
    );
    d1.memory().sync_once(None).await.unwrap();
    let stopped = ctrl(&a1).await;
    check(
        "B_barrier_committed_before_cleanup",
        !stopped.enabled
            && stopped.delete_pending
            && stopped.epoch == current.epoch + 1
            && admin
                .query_one(
                    "SELECT count(*) FROM caidex.memory_revisions WHERE user_id=$1",
                    &[&owner],
                )
                .await
                .unwrap()
                .get::<_, i64>(0)
                > 0,
    );
    check(
        "B_rejects_old_requests_and_member_access",
        pull(&a1, old_grant.clone()).await.is_err()
            && send(&a1, old_grant, Uuid::new_v4(), stable.clone())
                .await
                .is_err()
            && direct_count(&mut ordinary, 13, owner, Some(p)).await == 0,
    );
    admin.batch_execute("CREATE FUNCTION caidex.fixture_fail_erasure() RETURNS trigger LANGUAGE plpgsql AS $$BEGIN RAISE EXCEPTION 'fixture erase fault'; END$$; CREATE TRIGGER fixture_fail BEFORE DELETE ON caidex.memory_revisions FOR EACH ROW EXECUTE FUNCTION caidex.fixture_fail_erasure();").await.unwrap();
    check(
        "cleanup_failure_keeps_disabled_durable_barrier",
        d1.memory().sync_once(None).await.is_err()
            && ctrl(&a1).await.delete_pending
            && admin
                .query_opt(
                    "SELECT 1 FROM caidex.memory_embeddings WHERE revision_id=$1",
                    &[&r2],
                )
                .await
                .unwrap()
                .is_some(),
    );
    admin.batch_execute("DROP TRIGGER fixture_fail ON caidex.memory_revisions; DROP FUNCTION caidex.fixture_fail_erasure();").await.unwrap();
    admin.execute("INSERT INTO caidex.memories(user_id,memory_id,state) SELECT $1,gen_random_uuid(),'candidate' FROM generate_series(1,40)", &[&owner]).await.unwrap();
    let remaining = admin
        .query_one(
            "SELECT count(*) FROM caidex.memories WHERE user_id=$1",
            &[&owner],
        )
        .await
        .unwrap()
        .get::<_, i64>(0);
    d1.memory().sync_once(None).await.unwrap();
    check(
        "B_bounded_cleanup_commits_progress_and_keeps_barrier",
        ctrl(&a1).await.delete_pending
            && admin
                .query_one(
                    "SELECT count(*) FROM caidex.memories WHERE user_id=$1",
                    &[&owner],
                )
                .await
                .unwrap()
                .get::<_, i64>(0)
                == remaining - 32,
    );
    for _ in 0..4 {
        if !ctrl(&a1).await.delete_pending {
            break;
        }
        d1.memory().sync_once(None).await.unwrap();
    }
    check("B_actual_body_revision_source_vector_job_erased",admin.query_one("SELECT (SELECT count(*) FROM caidex.memories WHERE user_id=$1)+(SELECT count(*) FROM caidex.memory_revisions WHERE user_id=$1)+(SELECT count(*) FROM caidex.memory_sources WHERE user_id=$1)+(SELECT count(*) FROM caidex.memory_embeddings WHERE user_id=$1)+(SELECT count(*) FROM caidex.memory_jobs WHERE user_id=$1)+(SELECT count(*) FROM caidex.embedding_spaces WHERE user_id=$1)",&[&owner]).await.unwrap().get::<_,i64>(0)==0&&!ctrl(&a1).await.delete_pending);
    check(
        "B_preserves_member_private_other_project_and_Chat",
        admin
            .query_opt(
                "SELECT 1 FROM caidex.memories WHERE memory_id=$1",
                &[&member_id],
            )
            .await
            .unwrap()
            .is_some()
            && admin
                .query_opt(
                    "SELECT 1 FROM caidex.memories WHERE memory_id=$1",
                    &[&other_id],
                )
                .await
                .unwrap()
                .is_some()
            && admin
                .query_opt(
                    "SELECT 1 FROM caidex.conversations WHERE conversation_id=$1",
                    &[&conversation],
                )
                .await
                .unwrap()
                .is_some()
            && d1
                .conversations(None, 10)
                .unwrap()
                .iter()
                .any(|c| c.id == conversation),
    );
    check("permanent_entity_and_source_tombstones",admin.query_one("SELECT count(*) FROM caidex.deletion_tombstones WHERE user_id=$1 AND entity_kind IN('memory','source') AND retain_until='infinity'",&[&owner]).await.unwrap().get::<_,i64>(0)>2);
    d1.memory().reconcile_deleted_epoch(true).unwrap();
    d2.memory().sync_once(None).await.unwrap();
    d2.memory().reconcile_deleted_epoch(false).unwrap();
    check(
        "B_local_retain_detached_and_other_device_discards",
        d1.memory().get(id).unwrap().revisions.len() == 3
            && d2.memory().get(id).unwrap().revisions.is_empty()
            && d2.memory().get(id).unwrap().state == "deleted"
            && d1
                .state()
                .unwrap()
                .cache
                .db
                .query_row(
                    "SELECT count(*) FROM memory_sync_uploads WHERE memory=?1",
                    [id.to_string()],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
                == 0,
    );
    check(
        "B_freezes_unsubmitted_other_device_queue",
        d2.memory().get(unsent).unwrap().state == "deleted"
            && d2
                .memory()
                .sync_operations(0, 100)
                .unwrap()
                .iter()
                .all(|(op, _)| op.entity != unsent),
    );
    check(
        "retained_copies_cannot_implicitly_reselect_old_ids",
        d1.memory()
            .set_sync(Mode::Enable, &[Scope::Personal], &[id])
            .is_err(),
    );
    // Member receives safe epoch metadata while the owner's data plane remains closed.
    for _ in 0..4 {
        dm.memory().sync_once(None).await.unwrap();
    }
    dm.memory()
        .reconcile_project_epoch(&project_scope, true)
        .unwrap();
    d1.memory()
        .set_sync(Mode::Enable, &[Scope::Personal, project_scope.clone()], &[])
        .unwrap();
    drain(&d1, &admin).await;
    drain(&dm, &admin).await;
    check(
        "retained_local_body_survives_cloud_tombstone_rebase",
        d1.memory().get(id).unwrap().revisions.len() == 3
            && !dm.memory().get(project_id).unwrap().revisions.is_empty(),
    );
    let new_grant = scope(&a1, None).await;
    let mut replay = stable.clone();
    if let Change::Upsert { id, revision, .. } = &mut replay {
        *id = Uuid::new_v4();
        *revision = Uuid::new_v4();
    }
    check(
        "new_id_old_source_cannot_resurrect",
        matches!(
            send(&a1, new_grant.clone(), Uuid::new_v4(), replay.clone()).await,
            Err(ChatError::Conflict)
        ),
    );
    if let Change::Upsert { recreate, .. } = &mut replay {
        *recreate = true;
    }
    check(
        "explicit_new_authorization_recreation_allowed",
        send(&a1, new_grant.clone(), Uuid::new_v4(), replay)
            .await
            .is_ok(),
    );
    check(
        "old_epoch_receipt_cannot_ack_deleted_payload",
        matches!(
            send(&a1, new_grant.clone(), operation, stable.clone()).await,
            Err(ChatError::Conflict)
        ),
    );
    check(
        "old_entity_id_replay_after_reenable_refused",
        matches!(
            send(&a1, new_grant, Uuid::new_v4(), stable).await,
            Err(ChatError::Conflict)
        ),
    );
    // A raw ordinary role cannot revive a queued result with stale control/epoch.
    let tx = ordinary.transaction().await.unwrap();
    tx.execute("SELECT caidex.bind_session($1)", &[&access])
        .await
        .unwrap();
    let rejected=tx.execute("INSERT INTO caidex.memory_jobs(job_id,user_id,kind,execution_owner_ref,operation_id,cloud_epoch,settings_version) VALUES($1,$2,'embed','old offline host',$3,$4,$5)",&[&Uuid::new_v4(),&owner,&Uuid::new_v4(),&current.epoch,&current.version]).await.is_err();
    tx.rollback().await.unwrap();
    check("ordinary_RLS_stale_job_commit_denied", rejected);
    reset_limits(&admin).await;
    fault.mode.store(2, Ordering::SeqCst);
    let old_chat = d1.clone();
    let flight = tokio::spawn(async move { old_chat.memory().sync_once(None).await });
    fault.entered.notified().await;
    d1.switch(Namespace::account(&b)).unwrap();
    fault.release.notify_one();
    check(
        "late_reply_after_account_switch_rejected",
        matches!(flight.await.unwrap(), Err(Error::Cancelled)) && d1.memory().get(id).is_err(),
    );
    admin
        .execute(
            "UPDATE caidex.auth_sessions SET revoked_at=now() WHERE session_id=$1",
            &[&Uuid::parse_str(a2.session_id()).unwrap()],
        )
        .await
        .unwrap();
    check(
        "revoked_session_cannot_status_or_commit",
        a2.memory_request("status", json!({"protocol":PROTOCOL}))
            .await
            .is_err(),
    );
    server.abort();
    drop(d1);
    drop(d2);
    drop(dm);
    drop(dw);
    std::fs::remove_dir_all(root).unwrap();
}
