#![cfg(feature = "test-auth")]

use caidex_auth::{
    AUDIENCE, AccountStore, CLI_CLIENT, ConfirmedSession, LoginContext, Trust,
    server::{SandboxMail, Service, digest, router},
};
use oauth2::{
    AuthorizationCode, ClientId, PkceCodeVerifier, RedirectUrl, TokenResponse, TokenUrl,
    basic::BasicClient,
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::*;

const A: &str = "alice+project@example.test";
const B: &str = "bob@example.test";
const PASSWORD: &str = "synthetic correct horse battery staple";
const RECOVERED: &str = "synthetic recovered horse battery staple";
struct UnavailableMail;
impl caidex_auth::server::EmailSender for UnavailableMail {
    async fn send(&self, _: caidex_auth::server::Mail) -> Result<(), caidex_auth::server::Error> {
        Err(caidex_auth::server::Error::Unavailable)
    }
}
struct HangingMail;
impl caidex_auth::server::EmailSender for HangingMail {
    async fn send(&self, _: caidex_auth::server::Mail) -> Result<(), caidex_auth::server::Error> {
        std::future::pending().await
    }
}
static CHECKS: AtomicUsize = AtomicUsize::new(0);
fn check(name: &str, passed: bool) {
    assert!(passed, "I-2 check failed: {name}");
    CHECKS.fetch_add(1, Ordering::Relaxed);
    println!("I2_CHECK {}", json!({"name":name,"passed":true}));
}
async fn connection(dsn: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(dsn, NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}
async fn api(
    client: &reqwest::Client,
    base: &str,
    action: &str,
    token: Option<&str>,
    body: Value,
) -> (u16, Value) {
    let mut request = client
        .post(format!("{base}v1/auth/{action}"))
        .header("content-type", "application/json")
        .body(body.to_string());
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    let bytes = response.bytes().await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn access(session: &Value) -> &str {
    session["access_token"].as_str().unwrap()
}
async fn mail_token(mail: &SandboxMail, email: &str, purpose: &str) -> String {
    mail.0
        .lock()
        .await
        .iter()
        .rev()
        .find(|m| m.recipient == email && m.purpose == purpose)
        .unwrap()
        .verification_token
        .clone()
        .unwrap()
}
async fn advance_limits(admin: &Client) {
    // Expiry/resend boundary is moved in the isolated DB, never by a client clock or production config.
    admin.batch_execute("UPDATE caidex.auth_limits SET reset_at=now()-interval '1 second',last_at=now()-interval '1 hour'").await.unwrap();
}
fn public(trust: &Trust) -> Value {
    json!({"client_id":CLI_CLIENT,"issuer":trust.issuer(),"audience":AUDIENCE})
}

#[tokio::test]
#[ignore = "scripts/i2-auth.py provisions a private real PostgreSQL cluster"]
async fn account_http_database_security_and_public_clients() {
    let auth_dsn = std::env::var("CAIDEX_I2_AUTH_DB").unwrap();
    let data_dsn = std::env::var("CAIDEX_I2_DATA_DB").unwrap();
    let admin = connection(&std::env::var("CAIDEX_I2_ADMIN_DB").unwrap()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let trust = Trust::local_test(&format!(
        "http://localhost:{}/",
        listener.local_addr().unwrap().port()
    ))
    .unwrap();
    let mail = SandboxMail::default();
    let service = Service::connect(
        &auth_dsn,
        &data_dsn,
        trust.clone(),
        "localhost",
        [42; 32],
        mail.clone(),
    )
    .await
    .unwrap();
    let other = Service::connect(
        &auth_dsn,
        &data_dsn,
        trust.clone(),
        "localhost",
        [42; 32],
        mail.clone(),
    )
    .await
    .unwrap();
    let server = tokio::spawn(
        axum::serve(
            listener,
            router(service.clone()).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .into_future(),
    );
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap();
    let base = trust.issuer();
    let discovery = http
        .get(trust.endpoint(".well-known/oauth-authorization-server"))
        .send()
        .await
        .unwrap();
    let metadata: Value = serde_json::from_slice(&discovery.bytes().await.unwrap()).unwrap();
    check(
        "trusted_discovery_and_endpoints",
        trust.verify_metadata(&metadata).is_ok(),
    );
    let mut bad = metadata.clone();
    bad["issuer"] = "https://models.example/".into();
    check(
        "wrong_issuer_discovery_refused",
        trust.verify_metadata(&bad).is_err(),
    );
    let page = http.get(trust.endpoint("device")).send().await.unwrap();
    let csp = page
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let html = page.text().await.unwrap();
    check(
        "existing_account_authorization_page_has_confirmation_and_no_registration",
        html.contains("Approve only a CLI request you started")
            && !html.contains("register")
            && csp.contains("frame-ancestors 'none'"),
    );
    check(
        "no_context_and_fake_account_bearer_refused",
        api(&http, base, "me", None, json!({})).await.0 == 401
            && api(
                &http,
                base,
                "me",
                Some("synthetic-gateway-not-account"),
                json!({}),
            )
            .await
            .0 == 401,
    );

    let start = std::time::Instant::now();
    let response = api(
        &http,
        base,
        "register",
        None,
        json!({"email":A,"password":PASSWORD}),
    )
    .await;
    check(
        "register_generic_without_token_or_user_id",
        response.0 == 200 && response.1 == json!({"status":"accepted"}),
    );
    let token = mail_token(&mail, A, "register").await;
    check(
        "short_password_and_untrusted_fields_refused",
        api(
            &http,
            base,
            "register",
            None,
            json!({"email":"short@example.test","password":"short"}),
        )
        .await
        .0 == 400
            && api(
                &http,
                base,
                "register",
                None,
                json!({"email":"extra@example.test","password":PASSWORD,"user_id":Uuid::new_v4()}),
            )
            .await
            .0 == 400,
    );
    check(
        "unverified_identity_cannot_login",
        api(
            &http,
            base,
            "login",
            None,
            json!({"email":A,"password":PASSWORD}),
        )
        .await
        .0 == 401,
    );
    check(
        "challenge_wrong_purpose_and_missing_token_refused",
        api(
            &http,
            base,
            "verify",
            None,
            json!({"email":A,"token":token,"purpose":"recover","password":RECOVERED}),
        )
        .await
        .0 == 400
            && api(
                &http,
                base,
                "verify",
                None,
                json!({"token":"unknown-synthetic-token","purpose":"register"}),
            )
            .await
            .0 == 400,
    );
    let (left, right) = tokio::join!(
        service.request(
            "verify",
            None,
            "verify-a",
            json!({"email":A,"token":token,"purpose":"register"})
        ),
        other.request(
            "verify",
            None,
            "verify-b",
            json!({"email":A,"token":token,"purpose":"register"})
        )
    );
    check(
        "verification_concurrent_consume_exactly_once",
        left.is_ok() != right.is_ok(),
    );
    check(
        "verification_replay_refused",
        api(
            &http,
            base,
            "verify",
            None,
            json!({"email":A,"token":token,"purpose":"register"}),
        )
        .await
        .0 == 400,
    );
    let a=api(&http,base,"login",None,json!({"email":A,"password":PASSWORD,"platform":"Windows fixture","installation_id":Uuid::new_v4(),"display_name":"D1 synthetic"})).await;
    check(
        "verified_password_login_creates_independent_session",
        a.0 == 200 && trust.verify_session(&a.1, "caidex-native").is_ok(),
    );
    let a = a.1;
    let uid: Uuid = serde_json::from_value(a["user_id"].clone()).unwrap();
    let stored=admin.query_one("SELECT password_hash FROM caidex.auth_credentials WHERE user_id=$1 AND revoked_at IS NULL",&[&uid]).await.unwrap().get::<_,String>(0);
    check(
        "argon2id_salt_and_versioned_64mib_parameters_no_plaintext",
        stored.starts_with("$argon2id$v=19$m=65536,t=3,p=1$") && !stored.contains(PASSWORD),
    );
    let challenge_row=admin.query_one("SELECT state::text,octet_length(challenge_hash) FROM caidex.auth_challenges WHERE challenge_hash=$1",&[&digest(&token)]).await.unwrap();
    check(
        "verification_material_digest_not_raw_password_or_token",
        challenge_row.get::<_, i32>(1) == 32
            && !challenge_row.get::<_, String>(0).contains(PASSWORD)
            && !challenge_row.get::<_, String>(0).contains(&token),
    );
    println!(
        "I2_MEASUREMENT {}",
        json!({"register_verify_login_elapsed_ms":start.elapsed().as_millis(),"argon2_memory_kib":65536,"hash_concurrency":1,"two_core_two_gib_load_test":false})
    );
    let me = api(&http, base, "me", Some(access(&a)), json!({})).await;
    check(
        "new_account_settings_default_off_and_no_host_authority",
        me.0 == 200
            && me.1["user_id"] == a["user_id"]
            && me.1["memory_sync"]["enabled"] == false
            && me.1["chat_sync_enabled"] == false
            && me.1.get("host_execute").is_none(),
    );
    check(
        "client_supplied_actor_refused",
        api(
            &http,
            base,
            "me",
            Some(access(&a)),
            json!({"user_id":Uuid::new_v4()}),
        )
        .await
        .0 == 400,
    );
    check(
        "resend_cooldown_real_account",
        api(
            &http,
            base,
            "register",
            None,
            json!({"email":A,"password":PASSWORD}),
        )
        .await
        .0 == 429,
    );
    advance_limits(&admin).await;
    let before = mail.0.lock().await.len();
    let duplicate = api(
        &http,
        base,
        "register",
        None,
        json!({"email":A,"password":PASSWORD}),
    )
    .await;
    check(
        "duplicate_registration_generic_no_second_identity_or_email",
        duplicate.0 == 200
            && duplicate.1 == json!({"status":"accepted"})
            && mail.0.lock().await.len() == before
            && admin
                .query_one(
                    "SELECT count(*) FROM caidex.user_identities WHERE subject=$1",
                    &[&A],
                )
                .await
                .unwrap()
                .get::<_, i64>(0)
                == 1,
    );
    check(
        "unknown_recovery_generic",
        api(
            &http,
            base,
            "recover",
            None,
            json!({"email":"unknown@example.test"}),
        )
        .await
        .1 == json!({"status":"accepted"}),
    );
    check(
        "unknown_address_same_resend_limit",
        api(
            &http,
            base,
            "recover",
            None,
            json!({"email":"unknown@example.test"}),
        )
        .await
        .0 == 429,
    );
    api(
        &http,
        base,
        "register",
        None,
        json!({"email":B,"password":PASSWORD}),
    )
    .await;
    let bcode = mail_token(&mail, B, "register").await;
    check(
        "second_account_verified",
        api(
            &http,
            base,
            "verify",
            None,
            json!({"email":B,"token":bcode,"purpose":"register"}),
        )
        .await
        .0 == 200,
    );
    let b=api(&http,base,"login",None,json!({"email":B,"password":PASSWORD,"installation_id":a.get("installation_id").cloned().unwrap_or(json!(Uuid::new_v4()))})).await.1;
    let bme = api(&http, base, "me", Some(access(&b)), json!({})).await;
    check(
        "reused_data_connection_switches_account_without_leak",
        bme.1["user_id"] == b["user_id"]
            && api(&http, base, "me", Some(access(&a)), json!({})).await.1["user_id"]
                == a["user_id"],
    );
    let db = connection(&data_dsn).await;
    check(
        "data_role_cannot_read_auth_credentials_or_grant_state",
        db.query("SELECT password_hash FROM caidex.auth_credentials", &[])
            .await
            .is_err()
            && db
                .query("SELECT * FROM caidex.auth_grants", &[])
                .await
                .is_err(),
    );
    let authdb = connection(&auth_dsn).await;
    check(
        "auth_role_cannot_read_chat_memory_projects_or_set_owner",
        authdb
            .query("SELECT body FROM caidex.messages", &[])
            .await
            .is_err()
            && authdb
                .query("SELECT content FROM caidex.memory_revisions", &[])
                .await
                .is_err()
            && authdb
                .query("SELECT * FROM caidex.projects", &[])
                .await
                .is_err()
            && authdb.batch_execute("SET ROLE caidex_owner").await.is_err(),
    );
    check(
        "auth_role_controls_memory_sync_but_cannot_rewrite_user_id",
        authdb
            .query_one("SELECT has_column_privilege(current_user,'caidex.users','memory_enabled','UPDATE')", &[])
            .await
            .unwrap()
            .get::<_, bool>(0)
            && authdb
                .execute("UPDATE caidex.users SET user_id=gen_random_uuid()", &[])
                .await
                .is_err(),
    );
    check(
        "privileged_db_principal_rejected_at_service_start",
        Service::connect(
            &std::env::var("CAIDEX_I2_ADMIN_DB").unwrap(),
            &data_dsn,
            trust.clone(),
            "localhost",
            [42; 32],
            mail.clone(),
        )
        .await
        .is_err(),
    );
    let mut data = connection(&data_dsn).await;
    let tx = data.transaction().await.unwrap();
    tx.query_one("SELECT caidex.bind_session($1)", &[&access(&a)])
        .await
        .unwrap();
    check(
        "parameterized_transaction_actor_is_authenticated_user",
        tx.query_one("SELECT caidex.actor()", &[])
            .await
            .unwrap()
            .get::<_, Uuid>(0)
            == uid,
    );
    tx.rollback().await.unwrap();
    check(
        "real_reused_connection_rollback_clears_context",
        data.query_one("SELECT caidex.actor() IS NULL", &[])
            .await
            .unwrap()
            .get::<_, bool>(0),
    );

    let mut controller = connection(&std::env::var("CAIDEX_I2_ADMIN_DB").unwrap()).await;
    let locked = controller.transaction().await.unwrap();
    locked
        .query_one(
            "SELECT user_id FROM caidex.users WHERE user_id=$1 FOR UPDATE",
            &[&uid],
        )
        .await
        .unwrap();
    let parallel = other.clone();
    let attempt = tokio::spawn(async move {
        parallel
            .request(
                "login",
                None,
                "credential-race",
                json!({"email":A,"password":PASSWORD}),
            )
            .await
    });
    let mut waiting = false;
    for _ in 0..100 {
        waiting = admin.query_one("SELECT EXISTS(SELECT FROM pg_stat_activity WHERE usename='caidex_auth_test' AND wait_event_type='Lock' AND query LIKE 'SELECT user_id FROM caidex.users%')",&[]).await.unwrap().get(0);
        if waiting {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(waiting, "login did not reach controlled user lock");
    locked.execute("UPDATE caidex.auth_credentials SET revoked_at=now() WHERE user_id=$1 AND kind='password' AND revoked_at IS NULL", &[&uid]).await.unwrap();
    locked.commit().await.unwrap();
    check(
        "password_revocation_committed_before_login_cannot_issue_session",
        attempt.await.unwrap().is_err(),
    );
    admin.execute("UPDATE caidex.auth_credentials SET revoked_at=NULL WHERE user_id=$1 AND kind='password'", &[&uid]).await.unwrap();

    let d2=api(&http,base,"login",None,json!({"email":A,"password":PASSWORD,"platform":"iOS fixture","display_name":"D2 synthetic"})).await.1;
    let sessions = api(&http, base, "sessions", Some(access(&a)), json!({}))
        .await
        .1;
    check(
        "device_list_current_display_and_no_secret_columns",
        sessions["sessions"].as_array().unwrap().len() == 2
            && sessions["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["current"] == true)
            && !sessions.to_string().contains("access_hash")
            && !sessions.to_string().contains(access(&b)),
    );
    check(
        "foreign_session_revoke_refused",
        api(
            &http,
            base,
            "revoke",
            Some(access(&a)),
            json!({"session_id":b["session_id"]}),
        )
        .await
        .0 == 401,
    );
    check(
        "ambiguous_or_nonboolean_revoke_cannot_log_out_current_session",
        api(
            &http,
            base,
            "revoke",
            Some(access(&a)),
            json!({"others":"true"}),
        )
        .await
        .0 == 400
            && api(
                &http,
                base,
                "revoke",
                Some(access(&a)),
                json!({"others":true,"session_id":d2["session_id"]}),
            )
            .await
            .0 == 400
            && api(&http, base, "me", Some(access(&a)), json!({})).await.0 == 200,
    );
    admin.execute("INSERT INTO caidex.auth_sessions(user_id,access_hash,refresh_hash,client_id,audience,platform,display_name,access_expires_at,refresh_expires_at) SELECT $1,decode(md5('pagination-access-'||n)||md5('access-'||n),'hex'),decode(md5('pagination-refresh-'||n)||md5('refresh-'||n),'hex'),'caidex-native','caidex-data-v1','fixture','pagination-fixture',now()+interval '10 minutes',now()+interval '30 days' FROM generate_series(1,201) n", &[&uid]).await.unwrap();
    let page = api(&http, base, "sessions", Some(access(&a)), json!({}))
        .await
        .1;
    let next = api(
        &http,
        base,
        "sessions",
        Some(access(&a)),
        json!({"after_session_id":page["next_cursor"]}),
    )
    .await
    .1;
    let ids: std::collections::HashSet<String> = page["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .chain(next["sessions"].as_array().unwrap())
        .map(|s| s["session_id"].as_str().unwrap().to_owned())
        .collect();
    check(
        "unlimited_device_list_pages_without_duplicates_or_foreign_cursor",
        page["sessions"].as_array().unwrap().len() == 200
            && ids.len() == 203
            && next["next_cursor"].is_null()
            && api(
                &http,
                base,
                "sessions",
                Some(access(&a)),
                json!({"after_session_id":b["session_id"]}),
            )
            .await
            .0 == 401,
    );
    admin.execute("DELETE FROM caidex.auth_sessions WHERE user_id=$1 AND display_name='pagination-fixture'", &[&uid]).await.unwrap();
    check(
        "device_name_is_display_only",
        api(
            &http,
            base,
            "revoke",
            Some(access(&a)),
            json!({"display_name":"pretend-owner"}),
        )
        .await
        .0 == 200
            && api(&http, base, "me", Some(access(&a)), json!({})).await.1["user_id"]
                == a["user_id"],
    );
    check(
        "single_device_revoke_confirmed",
        api(
            &http,
            base,
            "revoke",
            Some(access(&a)),
            json!({"session_id":d2["session_id"]}),
        )
        .await
        .0 == 200,
    );
    check(
        "revoked_access_rejected_before_natural_expiry",
        api(&http, base, "me", Some(access(&d2)), json!({})).await.0 == 401
            && api(&http, base, "me", Some(access(&a)), json!({})).await.0 == 200,
    );
    let mut refresh = json!({"refresh_token":a["refresh_token"],"client_id":"caidex-native","audience":AUDIENCE,"issuer":trust.issuer()});
    let mut wrong = refresh.clone();
    wrong["issuer"] = "https://wrong.example/".into();
    check(
        "refresh_wrong_issuer_or_audience_refused",
        api(&http, base, "refresh", None, wrong).await.0 == 400,
    );
    let fresh = api(&http, base, "refresh", None, refresh.clone()).await;
    check(
        "refresh_rotates_access_and_refresh_atomically",
        fresh.0 == 200
            && fresh.1["access_token"] != a["access_token"]
            && fresh.1["refresh_token"] != a["refresh_token"]
            && fresh.1["session_id"] == a["session_id"],
    );
    check(
        "rotation_invalidates_old_access",
        api(&http, base, "me", Some(access(&a)), json!({})).await.0 == 401,
    );
    check(
        "lost_response_old_refresh_replay_refused",
        api(&http, base, "refresh", None, refresh.clone()).await.0 == 400,
    );
    check(
        "refresh_replay_revokes_successor_family",
        api(&http, base, "me", Some(access(&fresh.1)), json!({}))
            .await
            .0
            == 401
            && api(&http, base, "me", Some(access(&b)), json!({})).await.0 == 200,
    );
    advance_limits(&admin).await;
    let a = api(
        &http,
        base,
        "login",
        None,
        json!({"email":A,"password":PASSWORD}),
    )
    .await
    .1;
    refresh["refresh_token"] = a["refresh_token"].clone();
    let (r1, r2) = tokio::join!(
        service.request("refresh", None, "refresh-1", refresh.clone()),
        other.request("refresh", None, "refresh-2", refresh)
    );
    println!(
        "I2_DIAGNOSTIC {}",
        json!({"concurrent_refresh_errors":[r1.as_ref().err().map(ToString::to_string),r2.as_ref().err().map(ToString::to_string)]})
    );
    check(
        "two_backend_instances_concurrent_refresh_one_wins",
        r1.is_ok() != r2.is_ok(),
    );
    let rotated = if let Ok(v) = r1 { v } else { r2.unwrap() };
    check(
        "concurrent_old_token_replay_blocks_winner_family",
        api(&http, base, "me", Some(access(&rotated)), json!({}))
            .await
            .0
            == 401,
    );
    let a = api(
        &http,
        base,
        "login",
        None,
        json!({"email":A,"password":PASSWORD}),
    )
    .await
    .1;

    let proof = api(
        &http,
        base,
        "reauth",
        Some(access(&a)),
        json!({"password":PASSWORD}),
    )
    .await
    .1;
    check(
        "password_reauthentication_issues_narrow_one_use_proof",
        proof["proof"].as_str().is_some() && proof["expires_in"] == 120,
    );
    check(
        "passkey_enrollment_without_proof_refused",
        api(&http, base, "passkey-register", Some(access(&a)), json!({}))
            .await
            .0
            == 400,
    );
    let begin = api(
        &http,
        base,
        "passkey-register",
        Some(access(&a)),
        json!({"proof":proof["proof"]}),
    )
    .await;
    check(
        "passkey_server_challenge_and_uv_required",
        begin.0 == 200
            && begin.1["options"]["publicKey"]["authenticatorSelection"]["userVerification"]
                == "required",
    );
    check(
        "reauthentication_proof_replay_refused",
        api(
            &http,
            base,
            "passkey-register",
            Some(access(&a)),
            json!({"proof":proof["proof"]}),
        )
        .await
        .0 == 400,
    );
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let origin = Url::parse(base).unwrap();
    let credential = authenticator
        .do_registration(
            origin.clone(),
            serde_json::from_value(begin.1["options"].clone()).unwrap(),
        )
        .unwrap();
    let registered = api(
        &http,
        base,
        "passkey-register-finish",
        Some(access(&a)),
        json!({"request":begin.1["request"],"credential":credential}),
    )
    .await;
    check(
        "real_webauthn_software_registration_signature_verified",
        registered.0 == 200,
    );
    check(
        "passkey_registration_replay_refused",
        api(
            &http,
            base,
            "passkey-register-finish",
            Some(access(&a)),
            json!({"request":begin.1["request"],"credential":credential}),
        )
        .await
        .0 == 400,
    );
    let begin = api(&http, base, "passkey-login", None, json!({"email":A}))
        .await
        .1;
    let signed = authenticator
        .do_authentication(
            origin.clone(),
            serde_json::from_value(begin["options"].clone()).unwrap(),
        )
        .unwrap();
    let pk = api(
        &http,
        base,
        "passkey-login-finish",
        None,
        json!({"request":begin["request"],"credential":signed}),
    )
    .await;
    check(
        "real_webauthn_software_login_signature_and_uv_verified",
        pk.0 == 200 && pk.1["user_id"] == a["user_id"],
    );
    check(
        "passkey_authentication_replay_refused",
        api(
            &http,
            base,
            "passkey-login-finish",
            None,
            json!({"request":begin["request"],"credential":signed}),
        )
        .await
        .0 == 400,
    );
    let begin = api(&http, base, "passkey-login", None, json!({"email":A}))
        .await
        .1;
    let signed = authenticator
        .do_authentication(
            Url::parse("http://localhost:1/").unwrap(),
            serde_json::from_value(begin["options"].clone()).unwrap(),
        )
        .unwrap();
    check(
        "wrong_origin_valid_software_signature_refused",
        api(
            &http,
            base,
            "passkey-login-finish",
            None,
            json!({"request":begin["request"],"credential":signed}),
        )
        .await
        .0 == 400,
    );
    let begin = api(&http, base, "passkey-login", None, json!({"email":A}))
        .await
        .1;
    let mut signed = serde_json::to_value(
        authenticator
            .do_authentication(
                origin.clone(),
                serde_json::from_value(begin["options"].clone()).unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    signed["response"]["signature"] = "AAAA".into();
    check(
        "invalid_passkey_signature_refused",
        api(
            &http,
            base,
            "passkey-login-finish",
            None,
            json!({"request":begin["request"],"credential":signed}),
        )
        .await
        .0 == 400,
    );
    let row=admin.query_one("SELECT public_key,passkey_state::text FROM caidex.auth_credentials WHERE credential_id=$1",&[&Uuid::parse_str(registered.1["credential_id"].as_str().unwrap()).unwrap()]).await.unwrap();
    check(
        "passkey_persists_public_metadata_without_private_key",
        !row.get::<_, Vec<u8>>(0).is_empty() && !row.get::<_, String>(1).contains("PRIVATE KEY"),
    );
    check(
        "fresh_backend_reads_persisted_passkey_public_state",
        other
            .request("passkey-login", None, "restart", json!({"email":A}))
            .await
            .is_ok(),
    );
    let begin = api(&http, base, "passkey-proof", Some(access(&a)), json!({}))
        .await
        .1;
    let signed = authenticator
        .do_authentication(
            origin.clone(),
            serde_json::from_value(begin["options"].clone()).unwrap(),
        )
        .unwrap();
    let passproof = api(
        &http,
        base,
        "passkey-proof-finish",
        Some(access(&a)),
        json!({"request":begin["request"],"credential":signed}),
    )
    .await;
    check(
        "passkey_can_reauthenticate_sensitive_operation",
        passproof.0 == 200 && passproof.1["proof"].as_str().is_some(),
    );
    check(
        "cross_session_sensitive_proof_refused",
        api(
            &http,
            base,
            "credential-revoke",
            Some(access(&pk.1)),
            json!({"proof":passproof.1["proof"],"credential_id":registered.1["credential_id"]}),
        )
        .await
        .0 == 400,
    );
    check(
        "credential_revoke_preserves_password_login_method",
        api(
            &http,
            base,
            "credential-revoke",
            Some(access(&a)),
            json!({"proof":passproof.1["proof"],"credential_id":registered.1["credential_id"]}),
        )
        .await
        .0 == 200,
    );
    check(
        "revoked_passkey_no_new_authentication",
        api(&http, base, "passkey-login", None, json!({"email":A}))
            .await
            .0
            == 401,
    );
    let credentials = api(&http, base, "credentials", Some(access(&a)), json!({}))
        .await
        .1;
    let proof = api(
        &http,
        base,
        "reauth",
        Some(access(&a)),
        json!({"password":PASSWORD}),
    )
    .await
    .1;
    check("last_authentication_method_cannot_be_removed",api(&http,base,"credential-revoke",Some(access(&a)),json!({"proof":proof["proof"],"credential_id":credentials["credentials"][0]["credential_id"]})).await.0==401);

    let redirect = "http://127.0.0.1:33333/callback";
    let (mut login_context, challenge) = LoginContext::new(redirect.into()).unwrap();
    let mut request = public(&trust);
    request["redirect_uri"] = redirect.into();
    request["code_challenge"] = challenge.into();
    request["code_challenge_method"] = "S256".into();
    request["state"] = login_context.state().into();
    let mut bad = request.clone();
    bad["code_challenge_method"] = "plain".into();
    check(
        "public_client_plain_pkce_refused",
        api(&http, base, "pkce", None, bad).await.0 == 400,
    );
    let mut bad = request.clone();
    bad["redirect_uri"] = "http://evil.example/callback".into();
    check(
        "public_client_nonloopback_redirect_refused",
        api(&http, base, "pkce", None, bad).await.0 == 400,
    );
    let mut bad = request.clone();
    bad["client_secret"] = "synthetic-secret".into();
    check(
        "public_client_secret_and_unknown_fields_refused",
        api(&http, base, "pkce", None, bad).await.0 == 400,
    );
    let grant_reply = api(&http, base, "pkce", None, request.clone()).await;
    check(
        "pkce_high_entropy_context_accepted",
        grant_reply.0 == 200 && grant_reply.1["request"].as_str().is_some(),
    );
    let grant = grant_reply.1;
    check(
        "pkce_existing_account_confirmation_required",
        api(
            &http,
            base,
            "approve",
            None,
            json!({"request":grant["request"],"approve":true}),
        )
        .await
        .0 == 401,
    );
    let approved = api(
        &http,
        base,
        "approve",
        Some(access(&a)),
        json!({"request":grant["request"],"approve":true}),
    )
    .await;
    check(
        "pkce_redirect_code_and_state_exactly_bound",
        approved.0 == 200
            && approved.1["redirect_uri"]
                .as_str()
                .unwrap()
                .starts_with(redirect)
            && approved.1["redirect_uri"]
                .as_str()
                .unwrap()
                .contains("state="),
    );
    let redirect_uri = Url::parse(approved.1["redirect_uri"].as_str().unwrap()).unwrap();
    let state = redirect_uri
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .to_string();
    let verifier = PkceCodeVerifier::new(login_context.callback(&state, redirect).unwrap().into());
    let mut exchange = public(&trust);
    exchange["grant_type"] = "authorization_code".into();
    exchange["code"] = grant["request"].clone();
    exchange["redirect_uri"] = redirect.into();
    exchange["code_verifier"] = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into();
    check(
        "pkce_wrong_verifier_refused_without_consuming_valid_grant",
        api(&http, base, "token", None, exchange.clone()).await.0 == 400,
    );
    // Mature OAuth client builds the actual standard form request; the adapter uses the existing HTTP library.
    let oauth = BasicClient::new(ClientId::new(CLI_CLIENT.into()))
        .set_token_uri(TokenUrl::new(trust.endpoint("v1/auth/token")).unwrap())
        .set_redirect_uri(RedirectUrl::new(redirect.into()).unwrap());
    let wire = std::sync::Arc::new(tokio::sync::Mutex::new(None));
    let transport = |request: oauth2::HttpRequest| {
        let wire = wire.clone();
        let http = http.clone();
        async move {
            let (parts, body) = request.into_parts();
            let response = http
                .request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .body(body)
                .send()
                .await?;
            let mut reply = oauth2::http::Response::builder().status(response.status());
            *reply.headers_mut().unwrap() = response.headers().clone();
            let body = response.bytes().await?.to_vec();
            *wire.lock().await = Some(serde_json::from_slice::<Value>(&body).unwrap());
            Ok::<_, reqwest::Error>(reply.body(body).unwrap())
        }
    };
    let exchanged = oauth
        .exchange_code(AuthorizationCode::new(
            grant["request"].as_str().unwrap().into(),
        ))
        .set_pkce_verifier(PkceCodeVerifier::new(verifier.secret().into()))
        .add_extra_param("audience", AUDIENCE)
        .add_extra_param("issuer", base)
        .request_async(&transport)
        .await
        .unwrap();
    let cli_access = exchanged.access_token().secret();
    let cli = api(&http, base, "me", Some(cli_access), json!({})).await;
    check(
        "mature_oauth_library_pkce_form_exchange_independent_cli_session",
        cli.0 == 200
            && trust.verify_session(&cli.1, CLI_CLIENT).is_ok()
            && cli.1["session_id"] != a["session_id"]
            && cli.1["memory_sync"]["enabled"] == false,
    );
    let confirmed = ConfirmedSession::confirm(
        &trust,
        CLI_CLIENT,
        wire.lock().await.take().unwrap(),
        &cli.1,
    )
    .unwrap();
    let vault = std::env::temp_dir().join(format!("account-store-{}", Uuid::new_v4()));
    let protected = caidex_credentials::ProtectedFileStore::open(&vault).unwrap();
    let account_store = AccountStore::new(Uuid::new_v4(), protected);
    check(
        "real_protected_pair_save_and_current_pkce_context_confirmed",
        login_context.complete(&account_store, &confirmed),
    );
    let stored = account_store
        .saved(&trust, CLI_CLIENT, cli.1["user_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    check(
        "reopened_pair_unverified_and_keystore_private",
        stored.status() == "saved_unverified"
            && stored.access_token().expose() == cli_access
            && std::fs::read_dir(&vault).unwrap().count() == 1,
    );
    use std::os::unix::fs::PermissionsExt;
    check(
        "account_store_real_owner_only_directory_and_file",
        std::fs::metadata(&vault).unwrap().permissions().mode() & 0o777 == 0o700
            && std::fs::read_dir(&vault)
                .unwrap()
                .all(|p| p.unwrap().metadata().unwrap().permissions().mode() & 0o777 == 0o600),
    );
    drop(account_store);
    std::fs::remove_dir_all(&vault).unwrap();
    exchange["code_verifier"] = verifier.secret().as_str().into();
    check(
        "authorization_code_replay_refused",
        api(&http, base, "token", None, exchange).await.0 == 400,
    );
    check(
        "cancel_after_token_return_revokes_only_new_session",
        api(
            &http,
            base,
            "cancel",
            None,
            json!({"cancel_secret":grant["cancel_secret"]}),
        )
        .await
        .0 == 200
            && api(&http, base, "me", Some(cli_access), json!({})).await.0 == 401
            && api(&http, base, "me", Some(access(&a)), json!({})).await.0 == 200,
    );
    let grant = api(&http, base, "pkce", None, request).await.1;
    check(
        "cancel_before_approval_blocks_late_approval",
        api(
            &http,
            base,
            "cancel",
            None,
            json!({"cancel_secret":grant["cancel_secret"]}),
        )
        .await
        .0 == 200
            && api(
                &http,
                base,
                "approve",
                Some(access(&a)),
                json!({"request":grant["request"],"approve":true}),
            )
            .await
            .0 == 400,
    );
    let device = api(&http, base, "device", None, public(&trust)).await.1;
    let mut poll = public(&trust);
    poll["grant_type"] = "urn:ietf:params:oauth:grant-type:device_code".into();
    poll["device_code"] = device["device_code"].clone();
    check(
        "device_pending_and_default_interval",
        device["interval"] == 5
            && device["expires_in"] == 600
            && api(&http, base, "token", None, poll.clone()).await.1["error"]
                == "authorization_pending",
    );
    check(
        "device_early_poll_slow_down_persists",
        api(&http, base, "token", None, poll.clone()).await.1["error"] == "slow_down"
            && admin
                .query_one(
                    "SELECT interval_seconds FROM caidex.auth_grants WHERE grant_hash=$1",
                    &[&digest(device["device_code"].as_str().unwrap())],
                )
                .await
                .unwrap()
                .get::<_, i32>(0)
                == 10,
    );
    check(
        "device_wrong_user_code_refused",
        api(
            &http,
            base,
            "approve",
            Some(access(&a)),
            json!({"user_code":"AAAAAAAAAA","approve":true}),
        )
        .await
        .0 == 400,
    );
    check(
        "device_explicit_existing_account_approval",
        api(
            &http,
            base,
            "approve",
            Some(access(&a)),
            json!({"user_code":device["user_code"],"approve":true}),
        )
        .await
        .0 == 200,
    );
    admin.execute("UPDATE caidex.auth_grants SET next_poll_at=now()-interval '1 second' WHERE grant_hash=$1",&[&digest(device["device_code"].as_str().unwrap())]).await.unwrap();
    let device_session = api(&http, base, "token", None, poll.clone()).await;
    check(
        "device_once_creates_independent_cli_session",
        device_session.0 == 200
            && device_session.1["client_id"] == CLI_CLIENT
            && device_session.1["session_id"] != a["session_id"],
    );
    check(
        "device_code_replay_refused",
        api(&http, base, "token", None, poll).await.0 == 400,
    );
    let device = api(&http, base, "device", None, public(&trust)).await.1;
    api(
        &http,
        base,
        "approve",
        Some(access(&a)),
        json!({"user_code":device["user_code"],"approve":false}),
    )
    .await;
    let mut poll = public(&trust);
    poll["grant_type"] = "urn:ietf:params:oauth:grant-type:device_code".into();
    poll["device_code"] = device["device_code"].clone();
    check(
        "device_denial_terminal",
        api(&http, base, "token", None, poll.clone()).await.1["error"] == "access_denied",
    );
    let device = api(&http, base, "device", None, public(&trust)).await.1;
    admin.execute("UPDATE caidex.auth_grants SET expires_at=now()-interval '1 second' WHERE grant_hash=$1",&[&digest(device["device_code"].as_str().unwrap())]).await.unwrap();
    poll["device_code"] = device["device_code"].clone();
    check(
        "device_expiry_and_late_approval_refused",
        api(&http, base, "token", None, poll).await.1["error"] == "expired_token"
            && api(
                &http,
                base,
                "approve",
                Some(access(&a)),
                json!({"user_code":device["user_code"],"approve":true}),
            )
            .await
            .0 == 400,
    );
    let hostile = http
        .post(format!("{base}v1/auth/me"))
        .header("origin", "https://models.example")
        .header("content-type", "application/json")
        .bearer_auth(access(&a))
        .body("{}")
        .send()
        .await
        .unwrap();
    check(
        "hostile_browser_origin_refused_without_cors",
        hostile.status() == 401
            && hostile
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
    );
    let too_large = http
        .post(format!("{base}v1/auth/login"))
        .header("content-type", "application/json")
        .body(" ".repeat(32769))
        .send()
        .await
        .unwrap();
    check("http_body_bound", too_large.status() == 413);
    let no_store = http
        .post(format!("{base}v1/auth/me"))
        .header("content-type", "application/json")
        .bearer_auth(access(&a))
        .body("{}")
        .send()
        .await
        .unwrap();
    check(
        "protected_and_token_responses_no_store",
        no_store.headers().get("cache-control").unwrap() == "no-store",
    );

    advance_limits(&admin).await;
    check(
        "sensitive_email_change_requires_fresh_proof",
        api(
            &http,
            base,
            "change-email",
            Some(access(&a)),
            json!({"email":"new@example.test","proof":"not-a-proof"}),
        )
        .await
        .0 == 400,
    );
    let proof = api(
        &http,
        base,
        "reauth",
        Some(access(&a)),
        json!({"password":PASSWORD}),
    )
    .await
    .1;
    check(
        "new_email_verification_required",
        api(
            &http,
            base,
            "change-email",
            Some(access(&a)),
            json!({"email":"new@example.test","proof":proof["proof"]}),
        )
        .await
        .0 == 200
            && api(
                &http,
                base,
                "login",
                None,
                json!({"email":"new@example.test","password":PASSWORD}),
            )
            .await
            .0 == 401,
    );
    let email_code = mail_token(&mail, "new@example.test", "email").await;
    check(
        "verified_email_changes_identity_not_user_id",
        api(
            &http,
            base,
            "verify",
            None,
            json!({"email":"new@example.test","token":email_code,"purpose":"email"}),
        )
        .await
        .1["user_id"]
            == a["user_id"],
    );
    check(
        "old_email_and_prechange_sessions_refused",
        api(
            &http,
            base,
            "login",
            None,
            json!({"email":A,"password":PASSWORD}),
        )
        .await
        .0 == 401
            && api(&http, base, "me", Some(access(&a)), json!({})).await.0 == 401,
    );
    let new_a = api(
        &http,
        base,
        "login",
        None,
        json!({"email":"new@example.test","password":PASSWORD}),
    )
    .await
    .1;
    check(
        "new_email_retains_account_and_defaults",
        new_a["user_id"] == a["user_id"]
            && api(&http, base, "me", Some(access(&new_a)), json!({}))
                .await
                .1["memory_sync"]["enabled"]
                == false,
    );
    advance_limits(&admin).await;
    check(
        "recovery_generic_and_sandbox_notice",
        api(
            &http,
            base,
            "recover",
            None,
            json!({"email":"new@example.test"}),
        )
        .await
        .1 == json!({"status":"accepted"}),
    );
    let recovery = mail_token(&mail, "new@example.test", "recover").await;
    check(
        "password_recovery_valid_once_preserves_user_id",
        api(
            &http,
            base,
            "verify",
            None,
            json!({"email":"new@example.test","token":recovery,"purpose":"recover","password":RECOVERED}),
        )
        .await
        .1["user_id"]
            == a["user_id"]
            && api(
                &http,
                base,
                "verify",
                None,
                json!({"email":"new@example.test","token":recovery,"purpose":"recover","password":RECOVERED}),
            )
            .await
            .0 == 400,
    );
    check(
        "recovery_blocks_old_password_sessions_and_old_grants",
        api(
            &http,
            base,
            "login",
            None,
            json!({"email":"new@example.test","password":PASSWORD}),
        )
        .await
        .0 == 401
            && api(&http, base, "me", Some(access(&new_a)), json!({}))
                .await
                .0
                == 401
            && api(
                &http,
                base,
                "me",
                Some(access(&device_session.1)),
                json!({}),
            )
            .await
            .0 == 401,
    );
    let recovered = api(
        &http,
        base,
        "login",
        None,
        json!({"email":"new@example.test","password":RECOVERED}),
    )
    .await
    .1;
    check("recovered_password_login_and_security_event",recovered["user_id"]==a["user_id"] && admin.query_one("SELECT count(*) FROM caidex.security_events WHERE user_id=$1 AND kind='recovery'",&[&uid]).await.unwrap().get::<_,i64>(0)==1);
    check(
        "security_mail_contains_no_password_or_account_tokens",
        mail.0
            .lock()
            .await
            .iter()
            .any(|m| m.purpose == "security" && m.verification_token.is_none()),
    );
    let d2 = api(
        &http,
        base,
        "login",
        None,
        json!({"email":"new@example.test","password":RECOVERED,"installation_id":Uuid::new_v4()}),
    )
    .await
    .1;
    check(
        "new_device_does_not_require_old_device_approval",
        d2["user_id"] == recovered["user_id"] && d2["session_id"] != recovered["session_id"],
    );
    check(
        "logout_others_confirmed_preserves_current_and_other_account",
        api(
            &http,
            base,
            "revoke",
            Some(access(&recovered)),
            json!({"others":true}),
        )
        .await
        .0 == 200
            && api(&http, base, "me", Some(access(&d2)), json!({})).await.0 == 401
            && api(&http, base, "me", Some(access(&recovered)), json!({}))
                .await
                .0
                == 200
            && api(&http, base, "me", Some(access(&b)), json!({})).await.0 == 200,
    );
    let proof = api(
        &http,
        base,
        "reauth",
        Some(access(&recovered)),
        json!({"password":RECOVERED}),
    )
    .await
    .1;
    check(
        "account_disable_requires_proof_and_confirms_erasure_pending",
        api(
            &http,
            base,
            "delete-account",
            Some(access(&recovered)),
            json!({"proof":proof["proof"]}),
        )
        .await
        .1 == json!({"status":"account_disabled","data_erasure":"pending_I6_I7"}),
    );
    check(
        "disabled_account_tokens_rejected_other_account_untouched",
        api(&http, base, "me", Some(access(&recovered)), json!({}))
            .await
            .0
            == 401
            && api(
                &http,
                base,
                "login",
                None,
                json!({"email":"new@example.test","password":RECOVERED}),
            )
            .await
            .0 == 401
            && api(&http, base, "me", Some(access(&b)), json!({})).await.0 == 200,
    );
    advance_limits(&admin).await;
    let outage = Service::connect(
        &auth_dsn,
        &data_dsn,
        trust.clone(),
        "localhost",
        [42; 32],
        UnavailableMail,
    )
    .await
    .unwrap();
    let known = outage
        .request("recover", None, "mail-down-1", json!({"email":B}))
        .await
        .unwrap();
    let unknown = outage
        .request(
            "recover",
            None,
            "mail-down-2",
            json!({"email":"mail-down@example.test"}),
        )
        .await
        .unwrap();
    check(
        "mail_outage_does_not_enumerate_recovery_eligibility",
        known == unknown && known == json!({"status":"accepted"}),
    );
    let login = outage
        .request(
            "login",
            None,
            "mail-down-3",
            json!({"email":B,"password":PASSWORD}),
        )
        .await
        .unwrap();
    let mut request = json!({"refresh_token":login["refresh_token"],"client_id":"caidex-native","audience":AUDIENCE,"issuer":trust.issuer()});
    let successor = outage
        .request("refresh", None, "mail-down-4", request.clone())
        .await
        .unwrap();
    check(
        "mail_outage_cannot_undo_refresh_replay_revocation",
        outage
            .request("refresh", None, "mail-down-5", request.clone())
            .await
            .is_err()
            && api(&http, base, "me", Some(access(&successor)), json!({}))
                .await
                .0
                == 401,
    );
    request["refresh_token"] = successor["refresh_token"].clone();
    check(
        "revoked_family_remains_closed_when_notifier_unavailable",
        outage
            .request("refresh", None, "mail-down-6", request)
            .await
            .is_err(),
    );
    let hanging = Service::connect(
        &auth_dsn,
        &data_dsn,
        trust.clone(),
        "localhost",
        [42; 32],
        HangingMail,
    )
    .await
    .unwrap();
    check(
        "mail_transport_timeout_does_not_hold_authentication_indefinitely",
        matches!(tokio::time::timeout(std::time::Duration::from_secs(10),hanging.request("recover",None,"mail-timeout",json!({"email":"mail-timeout@example.test"}))).await, Ok(Ok(value)) if value==json!({"status":"accepted"})),
    );
    advance_limits(&admin).await;
    for _ in 0..10 {
        api(
            &http,
            base,
            "login",
            None,
            json!({"email":"guess@example.test","password":PASSWORD}),
        )
        .await;
    }
    check(
        "account_address_failed_login_limit",
        api(
            &http,
            base,
            "login",
            None,
            json!({"email":"guess@example.test","password":PASSWORD}),
        )
        .await
        .0 == 429,
    );
    admin.execute("INSERT INTO caidex.auth_limits(bucket_hash,count,reset_at) VALUES($1,120,now()+interval '1 minute') ON CONFLICT(bucket_hash) DO UPDATE SET count=120,reset_at=excluded.reset_at",&[&digest("ip:127.0.0.1")]).await.unwrap();
    check(
        "peer_ip_limit_not_client_forwarded_identity",
        api(&http, base, "me", Some(access(&b)), json!({})).await.0 == 429,
    );
    advance_limits(&admin).await;
    admin.execute("UPDATE caidex.auth_limits SET count=600,reset_at=now()+interval '1 minute' WHERE bucket_hash=$1",&[&digest("global")]).await.unwrap();
    check(
        "global_limit",
        api(&http, base, "me", Some(access(&b)), json!({})).await.0 == 429,
    );
    server.abort();
    println!(
        "I2_SUMMARY {}",
        json!({"passed":CHECKS.load(Ordering::Relaxed),"failed":0})
    );
}
