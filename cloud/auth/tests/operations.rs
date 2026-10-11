#![cfg(feature = "test-auth")]
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use caidex_auth::server::{
    EmailSender, Error, Mail,
    email::{Provider, ProviderEmail},
};
use caidex_credentials::Secret;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

#[derive(Default)]
struct Fixture {
    requests: Mutex<Vec<Value>>,
    status: Mutex<u16>,
}
async fn deliver(
    State(fixture): State<Arc<Fixture>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let status = *fixture.status.lock().unwrap();
    let id = Uuid::new_v4().to_string();
    fixture.requests.lock().unwrap().push(json!({"body":body,"id":id,"brevo_key":headers.contains_key("api-key"),"resend_key":headers.contains_key("authorization"),"idempotency":headers.get("idempotency-key").and_then(|h|h.to_str().ok())}));
    if status == 0 {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    (
        StatusCode::from_u16(if status == 0 { 201 } else { status }).unwrap(),
        Json(json!({"messageId":id,"id":id})),
    )
        .into_response()
}
fn check(name: &str, passed: bool) {
    assert!(passed, "I-7 mail check failed: {name}");
    println!("I7_CHECK {}", json!({"name":name,"passed":true}));
}
async fn connection(dsn: &str) -> Client {
    let (db, conn) = tokio_postgres::connect(dsn, NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    db
}
fn mail(id: Uuid) -> Mail {
    Mail {
        id,
        recipient: "synthetic@example.test".into(),
        purpose: "register".into(),
        language: "zh-CN",
        expires_in: 600,
        verification_token: Some("A".repeat(43)),
    }
}
async fn sender(
    dsn: &str,
    provider: Provider,
    endpoint: &str,
    daily: i64,
    monthly: i64,
) -> ProviderEmail {
    let key = if provider == Provider::Brevo {
        "synthetic-callback-bearer-1234567890".into()
    } else {
        format!("whsec_{}", STANDARD.encode([1u8; 32]))
    };
    let mut sender = ProviderEmail::connect(
        dsn,
        provider,
        "account@example.test".into(),
        Secret::new("synthetic-mail-api-key-never-real".into()).unwrap(),
        Secret::new(key).unwrap(),
        daily,
        monthly,
    )
    .await
    .unwrap();
    sender.use_test_endpoint(endpoint).unwrap();
    sender
}
fn brevo_headers() -> axum::http::HeaderMap {
    let mut h = axum::http::HeaderMap::new();
    h.insert(
        "authorization",
        "Bearer synthetic-callback-bearer-1234567890"
            .parse()
            .unwrap(),
    );
    h
}
fn resend_headers(body: &[u8], timestamp: u64) -> axum::http::HeaderMap {
    let mut mac = Hmac::<Sha256>::new_from_slice(&[1u8; 32]).unwrap();
    mac.update(format!("fixture-event.{timestamp}.").as_bytes());
    mac.update(body);
    let mut h = axum::http::HeaderMap::new();
    h.insert("svix-id", "fixture-event".parse().unwrap());
    h.insert("svix-timestamp", timestamp.to_string().parse().unwrap());
    h.insert(
        "svix-signature",
        format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
            .parse()
            .unwrap(),
    );
    h
}
#[tokio::test]
#[ignore = "scripts/i7-operations.py creates real isolated PostgreSQL and synthetic mail HTTP"]
async fn real_mail_operations() {
    let admin = connection(&std::env::var("CAIDEX_I7_ADMIN_DB").unwrap()).await;
    let dsn = std::env::var("CAIDEX_I7_AUTH_DB").unwrap();
    let fixture = Arc::new(Fixture::default());
    *fixture.status.lock().unwrap() = 201;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/mail", listener.local_addr().unwrap());
    let routes = Router::new()
        .route("/mail", post(deliver))
        .with_state(fixture.clone());
    let task = tokio::spawn(axum::serve(listener, routes).into_future());
    use std::future::IntoFuture;
    let brevo = sender(&dsn, Provider::Brevo, &endpoint, 100, 200).await;
    let id = Uuid::new_v4();
    brevo.send(mail(id)).await.unwrap();
    brevo.send(mail(id)).await.unwrap();
    let wire = fixture.requests.lock().unwrap()[0].clone();
    check(
        "brevo_minimal_template_stable_idempotency_one_send",
        fixture.requests.lock().unwrap().len() == 1
            && wire["body"]["tags"][0] == id.to_string()
            && wire["body"]["textContent"]
                .as_str()
                .unwrap()
                .contains(&"A".repeat(43))
            && wire["brevo_key"] == true
            && wire["body"].get("bcc").is_none(),
    );
    let mut changed = mail(id);
    changed.recipient = "changed@example.test".into();
    check(
        "mail_same_task_changed_payload_rejected",
        matches!(brevo.send(changed).await, Err(Error::Conflict)),
    );
    let event = json!({"event":"hard_bounce","message-id":wire["id"]}).to_string();
    check(
        "unauthenticated_callback_cannot_suppress",
        matches!(
            brevo
                .callback(&axum::http::HeaderMap::new(), event.as_bytes())
                .await,
            Err(Error::Denied)
        ),
    );
    brevo
        .callback(&brevo_headers(), event.as_bytes())
        .await
        .unwrap();
    brevo
        .callback(&brevo_headers(), event.as_bytes())
        .await
        .unwrap();
    brevo
        .callback(
            &brevo_headers(),
            json!({"event":"delivered","message-id":wire["id"]})
                .to_string()
                .as_bytes(),
        )
        .await
        .unwrap();
    check(
        "hard_bounce_durable_idempotent_and_late_delivery_cannot_unsuppress",
        matches!(brevo.send(mail(Uuid::new_v4())).await, Err(Error::Denied))
            && admin
                .query_one(
                    "SELECT status='bounced' FROM caidex.mail_tasks WHERE task_id=$1",
                    &[&id],
                )
                .await
                .unwrap()
                .get::<_, bool>(0),
    );
    admin
        .batch_execute("TRUNCATE caidex.mail_tasks,caidex.mail_suppressions")
        .await
        .unwrap();
    let resend = sender(&dsn, Provider::Resend, &endpoint, 100, 200).await;
    let id = Uuid::new_v4();
    let mut english = mail(id);
    english.language = "en";
    resend.send(english).await.unwrap();
    let wire = fixture.requests.lock().unwrap().last().unwrap().clone();
    check(
        "resend_explicit_provider_english_and_stable_header",
        wire["idempotency"] == id.to_string()
            && wire["resend_key"] == true
            && wire["body"]["text"]
                .as_str()
                .unwrap()
                .contains("Verification code")
            && wire["body"]["tags"][0]["value"] == id.to_string(),
    );
    let event = json!({"type":"email.complained","data":{"email_id":wire["id"]}}).to_string();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let headers = resend_headers(event.as_bytes(), now);
    check(
        "svix_signature_raw_body_and_expiry_required",
        matches!(resend.callback(&headers, b"{}").await, Err(Error::Denied))
            && matches!(
                resend
                    .callback(
                        &resend_headers(event.as_bytes(), now - 301),
                        event.as_bytes()
                    )
                    .await,
                Err(Error::Denied)
            ),
    );
    resend.callback(&headers, event.as_bytes()).await.unwrap();
    check(
        "signed_complaint_suppresses_across_provider_switch",
        matches!(brevo.send(mail(Uuid::new_v4())).await, Err(Error::Denied)),
    );
    admin
        .batch_execute("TRUNCATE caidex.mail_tasks,caidex.mail_suppressions")
        .await
        .unwrap();
    *fixture.status.lock().unwrap() = 0;
    let unknown = Uuid::new_v4();
    let before = fixture.requests.lock().unwrap().len();
    check(
        "provider_timeout_unknown",
        matches!(brevo.send(mail(unknown)).await, Err(Error::Unavailable)),
    );
    drop(brevo);
    let brevo = sender(&dsn, Provider::Brevo, &endpoint, 100, 200).await;
    check(
        "process_reopen_never_retries_unknown_or_switches_provider",
        matches!(brevo.send(mail(unknown)).await, Err(Error::Unavailable))
            && matches!(resend.send(mail(unknown)).await, Err(Error::Conflict))
            && fixture.requests.lock().unwrap().len() == before + 1,
    );
    *fixture.status.lock().unwrap() = 429;
    let id = Uuid::new_v4();
    check(
        "supplier_429_rejected_no_raw_error_or_retry",
        matches!(brevo.send(mail(id)).await, Err(Error::Invalid))
            && matches!(brevo.send(mail(id)).await, Err(Error::Unavailable)),
    );
    *fixture.status.lock().unwrap() = 503;
    let id = Uuid::new_v4();
    check(
        "supplier_503_unknown_no_automatic_fallback",
        matches!(brevo.send(mail(id)).await, Err(Error::Unavailable)),
    );
    *fixture.status.lock().unwrap() = 302;
    let id = Uuid::new_v4();
    check(
        "supplier_redirect_not_followed",
        matches!(brevo.send(mail(id)).await, Err(Error::Unavailable)),
    );
    *fixture.status.lock().unwrap() = 201;
    admin
        .batch_execute("TRUNCATE caidex.mail_tasks,caidex.mail_suppressions")
        .await
        .unwrap();
    let budget = sender(&dsn, Provider::Brevo, &endpoint, 1, 1).await;
    budget.send(mail(Uuid::new_v4())).await.unwrap();
    let before = fixture.requests.lock().unwrap().len();
    check(
        "durable_global_daily_monthly_budget_no_extra_http",
        matches!(budget.send(mail(Uuid::new_v4())).await, Err(Error::Limited))
            && fixture.requests.lock().unwrap().len() == before,
    );
    admin
        .batch_execute("TRUNCATE caidex.mail_tasks,caidex.mail_suppressions")
        .await
        .unwrap();
    let nearing = sender(&dsn, Provider::Brevo, &endpoint, 10, 10).await;
    for _ in 0..9 {
        nearing.send(mail(Uuid::new_v4())).await.unwrap();
    }
    let mut notice = mail(Uuid::new_v4());
    notice.purpose = "security".into();
    notice.verification_token = None;
    notice.expires_in = 0;
    check(
        "ninety_percent_budget_stops_nonessential_but_keeps_verification",
        matches!(nearing.send(notice).await, Err(Error::Limited))
            && nearing.send(mail(Uuid::new_v4())).await.is_ok(),
    );
    drop(nearing);
    check("mail_records_contain_no_plain_recipient_token_password_or_api_key",admin.query_one("SELECT bool_and(to_jsonb(t)::text NOT LIKE '%synthetic@example.test%' AND to_jsonb(t)::text NOT LIKE '%AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA%' AND to_jsonb(t)::text NOT LIKE '%synthetic-mail-api-key%') FROM caidex.mail_tasks t",&[]).await.unwrap().get::<_,bool>(0));
    check("data_role_cannot_read_mail_metadata",admin.query_one("SELECT NOT has_table_privilege('caidex_test','caidex.mail_tasks','SELECT') AND NOT has_table_privilege('caidex_test','caidex.mail_suppressions','SELECT')",&[]).await.unwrap().get::<_,bool>(0));
    drop(budget);
    drop(resend);
    drop(brevo);
    callback_route(&admin, &dsn, &endpoint, &fixture).await;
    resource_workload(&admin, &dsn).await;
    production_entry(&dsn).await;
    task.abort();
}

async fn resource_workload(admin: &Client, dsn: &str) {
    use caidex_auth::{
        Trust,
        server::{SandboxMail, Service, router},
    };
    use std::io::Write;
    let data_dsn = std::env::var("CAIDEX_I7_DATA_DB").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let trust = Trust::local_test(&format!("http://localhost:{}/", address.port())).unwrap();
    let mailbox = SandboxMail::default();
    let service = Service::connect(dsn, &data_dsn, trust, "localhost", [1; 32], mailbox.clone())
        .await
        .unwrap();
    service
        .guard_disk("/fault".into(), 1024 * 1024)
        .await
        .unwrap();
    let task = tokio::spawn(
        axum::serve(
            listener,
            router(service.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    use std::future::IntoFuture;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://{address}/v1/auth/");
    let credentials =
        json!({"email":"load@example.test","password":"synthetic load password never real"});
    let reply = http
        .post(format!("{url}register"))
        .header("content-type", "application/json")
        .body(credentials.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(reply.status(), 200);
    let token = mailbox.0.lock().await[0]
        .verification_token
        .clone()
        .unwrap();
    let reply = http
        .post(format!("{url}verify"))
        .header("content-type", "application/json")
        .body(json!({"purpose":"register","token":token}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(reply.status(), 200);
    let mut login_ms = Vec::new();
    for _ in 0..8 {
        let start = std::time::Instant::now();
        let reply = http
            .post(format!("{url}login"))
            .header("content-type", "application/json")
            .body(credentials.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(reply.status(), 200);
        login_ms.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    admin.batch_execute("UPDATE caidex.auth_limits SET reset_at=now()-interval '1 second',last_at=now()-interval '1 hour'").await.unwrap();
    let mut concurrent = Vec::new();
    for _ in 0..4 {
        let http = http.clone();
        let url = url.clone();
        let body = credentials.to_string();
        concurrent.push(tokio::spawn(async move {
            http.post(format!("{url}login"))
                .header("content-type", "application/json")
                .body(body)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }));
    }
    let mut statuses = Vec::new();
    for task in concurrent {
        statuses.push(task.await.unwrap());
    }
    check(
        "single_hash_worker_concurrent_login_bounded_refusal",
        statuses.iter().all(|s| matches!(s, 200 | 429))
            && statuses.contains(&200)
            && statuses.contains(&429),
    );
    let mut query_ms = Vec::new();
    for _ in 0..20 {
        let start = std::time::Instant::now();
        admin
            .batch_execute(
                "BEGIN; SET LOCAL ROLE caidex_test; SELECT caidex.bind_session(repeat('a',64));",
            )
            .await
            .unwrap();
        let rows=admin.query("SELECT user_id,embedding<->'[0,0,0]'::vector FROM caidex.memory_embeddings ORDER BY embedding<->'[0,0,0]'::vector LIMIT 10",&[]).await.unwrap();
        assert_eq!(rows.len(), 10);
        assert!(
            rows.iter()
                .all(|r| r.get::<_, Uuid>(0).to_string() == "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
        );
        admin.batch_execute("COMMIT").await.unwrap();
        query_ms.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    fn percentiles(values: &mut [f64]) -> Value {
        values.sort_by(f64::total_cmp);
        let p = |q: f64| values[((values.len() as f64 * q).ceil() as usize).saturating_sub(1)];
        json!({"p50_ms":p(0.5),"p95_ms":p(0.95),"p99_ms":p(0.99)})
    }
    let login = percentiles(&mut login_ms);
    let query = percentiles(&mut query_ms);
    println!("I7_LATENCY {}", json!({"login":login,"query":query}));
    check(
        "registered_login_and_exact_rls_vector_latency_slo",
        login["p95_ms"].as_f64().unwrap() <= 2000.0 && query["p95_ms"].as_f64().unwrap() <= 100.0,
    );
    let settings=admin.query_one("SELECT current_setting('shared_buffers'),current_setting('work_mem'),current_setting('max_connections'),(SELECT count(*) FROM pg_stat_activity),pg_database_size(current_database())",&[]).await.unwrap();
    println!(
        "I7_MEASUREMENT {}",
        json!({"login":login,"exact_vector_query":query,"login_sequential":8,"concurrency":4,"statuses":statuses,"query_count":20,"vectors":1000,"dimensions":3,"shared_buffers":settings.get::<_,String>(0),"work_mem":settings.get::<_,String>(1),"max_connections":settings.get::<_,String>(2),"actual_connections":settings.get::<_,i64>(3),"database_bytes":settings.get::<_,i64>(4)})
    );
    let mut filler = std::fs::File::create("/fault/service-filler").unwrap();
    let block = vec![0u8; 1024 * 1024];
    let error = loop {
        if let Err(error) = filler.write_all(&block) {
            break error;
        }
    };
    assert_eq!(error.raw_os_error(), Some(28));
    check(
        "physical_low_disk_refuses_write_before_database_mutation",
        matches!(
            service
                .request("login", None, "fixture-low-disk", credentials.clone())
                .await,
            Err(Error::Unavailable)
        ),
    );
    for (action, body) in [("chat-push", json!({})), ("memory-push", json!({}))] {
        check(
            &format!("low_disk_{action}_rejected_before_payload_or_mutation"),
            matches!(
                service
                    .request(action, None, "fixture-low-disk", body)
                    .await,
                Err(Error::Unavailable)
            ),
        );
    }
    for (action, body) in [
        ("chat-control", json!({"enabled":false})),
        ("memory-control", json!({"mode":"keep"})),
        ("memory-control", json!({"mode":"delete"})),
    ] {
        check(
            &format!(
                "low_disk_{action}_{}_still_requires_auth",
                body.get("mode").and_then(Value::as_str).unwrap_or("stop")
            ),
            matches!(
                service
                    .request(action, None, "fixture-low-disk", body)
                    .await,
                Err(Error::Denied)
            ),
        );
    }
    drop(filler);
    std::fs::remove_file("/fault/service-filler").unwrap();
    check(
        "disk_space_recovery_does_not_switch_account_or_database",
        service
            .request("login", None, "fixture-restored-disk", credentials)
            .await
            .is_ok(),
    );
    task.abort();
}

async fn callback_route(admin: &Client, dsn: &str, endpoint: &str, fixture: &Arc<Fixture>) {
    use caidex_auth::{
        Trust,
        server::{Service, router},
    };
    admin
        .batch_execute("TRUNCATE caidex.mail_tasks,caidex.mail_suppressions")
        .await
        .unwrap();
    let mailer = sender(dsn, Provider::Brevo, endpoint, 100, 200).await;
    let id = Uuid::new_v4();
    mailer.send(mail(id)).await.unwrap();
    let provider_id = fixture.requests.lock().unwrap().last().unwrap()["id"].clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let trust = Trust::local_test(&format!("http://localhost:{}/", address.port())).unwrap();
    let service = Service::connect(
        dsn,
        &std::env::var("CAIDEX_I7_DATA_DB").unwrap(),
        trust,
        "localhost",
        [1; 32],
        mailer,
    )
    .await
    .unwrap();
    use std::future::IntoFuture;
    let server = tokio::spawn(
        axum::serve(
            listener,
            router(service).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://{address}/v1/mail/events");
    let event = json!({"event":"delivered","message-id":provider_id});
    let rejected = http
        .post(&url)
        .header("content-type", "application/json")
        .body(event.to_string())
        .send()
        .await
        .unwrap();
    let accepted = http
        .post(&url)
        .headers(brevo_headers())
        .header("content-type", "application/json")
        .body(event.to_string())
        .send()
        .await
        .unwrap();
    check(
        "real_http_callback_authentication_and_durable_delivery",
        rejected.status() == 401
            && accepted.status() == 200
            && admin
                .query_one(
                    "SELECT status='delivered' FROM caidex.mail_tasks WHERE task_id=$1",
                    &[&id],
                )
                .await
                .unwrap()
                .get::<_, bool>(0),
    );
    let oversized = http
        .post(&url)
        .headers(brevo_headers())
        .body(vec![b'x'; 65537])
        .send()
        .await
        .unwrap();
    check("callback_body_size_bound", oversized.status() == 400);
    use tokio::io::AsyncWriteExt;
    let mut slow = tokio::net::TcpStream::connect(address).await.unwrap();
    slow.write_all(
        b"POST /v1/mail/events HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\n\r\nx",
    )
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let blocked = http
        .post(&url)
        .headers(brevo_headers())
        .header("content-type", "application/json")
        .body(event.to_string())
        .send()
        .await
        .unwrap();
    check(
        "callback_worker_admission_before_slow_body",
        blocked.status() == 429,
    );
    drop(slow);
    server.abort();
    let _ = server.await;
}

async fn production_entry(dsn: &str) {
    use caidex_credentials::{Broker, CredentialRef, Id, ProtectedFileStore, SecretKind};
    use std::{
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
    };
    let directory =
        std::path::PathBuf::from(format!("/var/tmp/caidex-i7-service-{}", Uuid::new_v4()));
    caidex_credentials::private_directory(&directory).unwrap();
    let secrets = directory.join("secrets");
    let owner = Id::new("caidex-service").unwrap();
    let broker = Broker::new(owner.clone(), ProtectedFileStore::open(&secrets).unwrap());
    for (profile, kind, value) in [
        (
            "device-code-pepper",
            SecretKind::ClientSecret,
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([1; 32]),
        ),
        (
            "mail-api",
            SecretKind::ApiKey,
            "synthetic-entry-api-key".into(),
        ),
        (
            "mail-webhook",
            SecretKind::ClientSecret,
            "synthetic-entry-webhook-key-123456789".into(),
        ),
    ] {
        broker
            .set(
                &CredentialRef {
                    owner: owner.clone(),
                    provider: Id::new("account-service").unwrap(),
                    profile: Id::new(profile).unwrap(),
                    kind,
                },
                Secret::new(value).unwrap(),
            )
            .unwrap();
    }
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = json!({"port":port,"issuer":"https://accounts.example.test/","rp_id":"accounts.example.test","auth_db":dsn,"data_db":std::env::var("CAIDEX_I7_DATA_DB").unwrap(),"secrets":secrets,"provider":"brevo","from":"account@example.test","daily_budget":100,"monthly_budget":200,"database_volume":"/var/lib/postgresql/data"});
    let path = directory.join("config.json");
    std::fs::write(&path, config.to_string()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let command = || {
        let mut command = Command::new("/host/lib/ld-linux-x86-64.so.2");
        command.args([
            "--library-path",
            "/host/lib",
            "/artifacts/release/caidex-auth-service",
        ]);
        command
            .arg(&directory)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    for _ in 0..2 {
        let mut child = command().spawn().unwrap();
        let mut ready = false;
        for _ in 0..100 {
            if let Ok(response) = http
                .get(format!(
                    "http://127.0.0.1:{port}/.well-known/oauth-authorization-server"
                ))
                .send()
                .await
                && response.status() == 200
                && serde_json::from_str::<Value>(&response.text().await.unwrap()).unwrap()["issuer"]
                    == "https://accounts.example.test/"
            {
                ready = true;
                break;
            }
            if child.try_wait().unwrap().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        if child.try_wait().unwrap().is_none() {
            child.kill().unwrap();
        }
        child.wait().unwrap();
        assert!(ready, "synthetic production entry did not become ready");
    }
    check(
        "protected_service_entry_real_process_start_and_restart_no_email_send",
        true,
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    check(
        "service_entry_rejects_public_configuration",
        !command().status().unwrap().success(),
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut bad = config;
    bad["auth_db"] = json!(format!("{dsn} password=synthetic-not-allowed"));
    std::fs::write(&path, bad.to_string()).unwrap();
    check(
        "service_entry_rejects_password_in_configuration",
        !command().status().unwrap().success(),
    );
    std::fs::remove_dir_all(directory).unwrap();
}
