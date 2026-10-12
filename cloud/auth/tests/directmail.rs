#![cfg(feature = "test-auth")]
use base64::{Engine, engine::general_purpose::STANDARD};
use caidex_auth::{
    Trust,
    server::{
        EmailSender, Error, Mail, Service,
        email::{Provider, ProviderEmail, smtp::AliyunSmtpEmail},
        router,
    },
};
use caidex_credentials::Secret;
use openssl::{
    asn1::Asn1Time,
    bn::BigNum,
    ec::{EcGroup, EcKey},
    hash::MessageDigest,
    nid::Nid,
    pkey::PKey,
    x509::{
        X509, X509NameBuilder,
        extension::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName},
    },
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use tokio_postgres::Client;
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    },
};
use uuid::Uuid;

const PASSWORD: &str = "synthetic SMTP password never real";
const FROM: &str = "no-reply@caidex.cn";
#[derive(Clone, Copy)]
enum Mode {
    Accept,
    AuthReject,
    RecipientReject,
    DataReject,
    DropData,
    DelayData,
    DropAccepted,
    HugeReply,
}
struct Fixture {
    address: SocketAddr,
    ca: Vec<u8>,
    mode: Arc<Mutex<Mode>>,
    commands: Arc<Mutex<Vec<String>>>,
    messages: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn check(name: &str, passed: bool) {
    assert!(passed, "DirectMail check failed: {name}");
    println!("DIRECTMAIL_CHECK {}", json!({"name":name,"passed":true}));
}
fn tls(host: &str, expired: bool) -> (Vec<u8>, Arc<rustls::ServerConfig>) {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    let root_key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "Synthetic CAIdex test CA")
        .unwrap();
    let name = name.build();
    let mut root = X509::builder().unwrap();
    root.set_version(2).unwrap();
    root.set_serial_number(&BigNum::from_u32(1).unwrap().to_asn1_integer().unwrap())
        .unwrap();
    root.set_subject_name(&name).unwrap();
    root.set_issuer_name(&name).unwrap();
    root.set_pubkey(&root_key).unwrap();
    root.set_not_before(&Asn1Time::from_unix(1_700_000_000).unwrap())
        .unwrap();
    root.set_not_after(&Asn1Time::from_unix(2_100_000_000).unwrap())
        .unwrap();
    root.append_extension(BasicConstraints::new().critical().ca().build().unwrap())
        .unwrap();
    root.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()
            .unwrap(),
    )
    .unwrap();
    root.sign(&root_key, MessageDigest::sha256()).unwrap();
    let root = root.build();
    let key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
    let mut leaf = X509::builder().unwrap();
    leaf.set_version(2).unwrap();
    leaf.set_serial_number(&BigNum::from_u32(2).unwrap().to_asn1_integer().unwrap())
        .unwrap();
    let mut leaf_name = X509NameBuilder::new().unwrap();
    leaf_name.append_entry_by_text("CN", host).unwrap();
    leaf.set_subject_name(&leaf_name.build()).unwrap();
    leaf.set_issuer_name(root.subject_name()).unwrap();
    leaf.set_pubkey(&key).unwrap();
    leaf.set_not_before(&Asn1Time::from_unix(1_700_000_000).unwrap())
        .unwrap();
    leaf.set_not_after(
        &Asn1Time::from_unix(if expired {
            1_700_000_001
        } else {
            2_100_000_000
        })
        .unwrap(),
    )
    .unwrap();
    leaf.append_extension(BasicConstraints::new().critical().build().unwrap())
        .unwrap();
    leaf.append_extension(
        KeyUsage::new()
            .critical()
            .digital_signature()
            .build()
            .unwrap(),
    )
    .unwrap();
    leaf.append_extension(ExtendedKeyUsage::new().server_auth().build().unwrap())
        .unwrap();
    let san = SubjectAlternativeName::new()
        .dns(host)
        .build(&leaf.x509v3_context(Some(&root), None))
        .unwrap();
    leaf.append_extension(san).unwrap();
    leaf.sign(&root_key, MessageDigest::sha256()).unwrap();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![
                CertificateDer::from(leaf.build().to_der().unwrap()),
                CertificateDer::from(root.to_der().unwrap()),
            ],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                key.private_key_to_pkcs8().unwrap(),
            )),
        )
        .unwrap();
    (root.to_der().unwrap(), Arc::new(config))
}
impl Fixture {
    async fn new(host: &str, expired: bool) -> Self {
        let (ca, config) = tls(host, expired);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mode = Arc::new(Mutex::new(Mode::Accept));
        let commands = Arc::new(Mutex::new(vec![]));
        let messages = Arc::new(Mutex::new(vec![]));
        let (m, c, b) = (mode.clone(), commands.clone(), messages.clone());
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let (mode, commands, messages) = (m.clone(), c.clone(), b.clone());
                let acceptor = TlsAcceptor::from(config.clone());
                connections.spawn(async move {
                    let Ok(stream) = acceptor.accept(stream).await else {
                        return;
                    };
                    let (read, mut write) = tokio::io::split(stream);
                    let mut read = BufReader::new(read);
                    let selected = *mode.lock().unwrap();
                    if matches!(selected, Mode::HugeReply) {
                        let _ = write
                            .write_all(format!("220 {}\r\n", "x".repeat(32768)).as_bytes())
                            .await;
                        return;
                    }
                    if write.write_all(b"220 synthetic SMTP\r\n").await.is_err() {
                        return;
                    }
                    loop {
                        let mut line = String::new();
                        if read.read_line(&mut line).await.unwrap_or(0) == 0 {
                            return;
                        }
                        let command = line.trim_end().to_owned();
                        commands.lock().unwrap().push(command.clone());
                        let response = if command.starts_with("EHLO ") {
                            "250-synthetic\r\n250-AUTH LOGIN\r\n250 8BITMIME\r\n"
                        } else if command == "AUTH LOGIN" {
                            if write.write_all(b"334 VXNlcm5hbWU6\r\n").await.is_err() {
                                return;
                            }
                            let mut user = String::new();
                            if read.read_line(&mut user).await.unwrap_or(0) == 0 {
                                return;
                            }
                            if write.write_all(b"334 UGFzc3dvcmQ6\r\n").await.is_err() {
                                return;
                            }
                            let mut password = String::new();
                            if read.read_line(&mut password).await.unwrap_or(0) == 0 {
                                return;
                            }
                            // AUTH values stay in memory and are never put in commands/logs/reports.
                            if matches!(selected, Mode::AuthReject)
                                || STANDARD.decode(user.trim()).ok().as_deref()
                                    != Some(FROM.as_bytes())
                                || STANDARD.decode(password.trim()).ok().as_deref()
                                    != Some(PASSWORD.as_bytes())
                            {
                                "535 authentication rejected\r\n"
                            } else {
                                "235 authenticated\r\n"
                            }
                        } else if command.starts_with("MAIL FROM:") {
                            "250 sender ok\r\n"
                        } else if command.starts_with("RCPT TO:") {
                            if matches!(selected, Mode::RecipientReject) {
                                "550 recipient rejected\r\n"
                            } else {
                                "250 recipient ok\r\n"
                            }
                        } else if command == "DATA" {
                            if write.write_all(b"354 data\r\n").await.is_err() {
                                return;
                            }
                            let mut message = String::new();
                            loop {
                                let mut line = String::new();
                                if read.read_line(&mut line).await.unwrap_or(0) == 0 {
                                    return;
                                }
                                if line == ".\r\n" {
                                    break;
                                }
                                message.push_str(&line);
                            }
                            messages.lock().unwrap().push(message);
                            if matches!(selected, Mode::DropData) {
                                return;
                            }
                            if matches!(selected, Mode::DelayData) {
                                tokio::time::sleep(Duration::from_secs(4)).await;
                            }
                            if matches!(selected, Mode::DropAccepted) {
                                let _ = write.write_all(b"250 accepted\r\n").await;
                                return;
                            }
                            if matches!(selected, Mode::DataReject) {
                                "451 not accepted\r\n"
                            } else {
                                "250 accepted\r\n"
                            }
                        } else if command == "QUIT" {
                            let _ = write.write_all(b"221 bye\r\n").await;
                            return;
                        } else {
                            "500 unsupported\r\n"
                        };
                        if write.write_all(response.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        Self {
            address,
            ca,
            mode,
            commands,
            messages,
            task,
        }
    }
    async fn sender(&self, dsn: &str, daily: i64, monthly: i64) -> AliyunSmtpEmail {
        let mut sender = AliyunSmtpEmail::connect(
            dsn,
            FROM,
            Secret::new(PASSWORD.into()).unwrap(),
            daily,
            monthly,
        )
        .await
        .unwrap();
        sender
            .use_test_endpoint(
                self.address,
                &self.ca,
                Secret::new(PASSWORD.into()).unwrap(),
            )
            .unwrap();
        sender
    }
    fn count(&self) -> usize {
        self.messages.lock().unwrap().len()
    }
    fn body(&self, index: usize) -> String {
        self.part(index, "text/plain")
    }
    fn part(&self, index: usize, kind: &str) -> String {
        let message = self.messages.lock().unwrap()[index].clone();
        let boundary = message
            .split("boundary=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let delimiter = format!("--{boundary}");
        let part = message
            .split(&delimiter)
            .find(|part| part.contains(&format!("Content-Type: {kind}")))
            .unwrap();
        let (headers, body) = part.split_once("\r\n\r\n").unwrap();
        if headers
            .to_lowercase()
            .contains("content-transfer-encoding: base64")
        {
            String::from_utf8(
                STANDARD
                    .decode(body.split_whitespace().collect::<String>())
                    .unwrap(),
            )
            .unwrap()
        } else if headers
            .to_lowercase()
            .contains("content-transfer-encoding: quoted-printable")
        {
            String::from_utf8(
                quoted_printable::decode(
                    body.trim_end().as_bytes(),
                    quoted_printable::ParseMode::Strict,
                )
                .unwrap(),
            )
            .unwrap()
        } else {
            body.to_owned()
        }
    }
}
async fn db(dsn: &str) -> Client {
    let (db, connection) = tokio_postgres::connect(dsn, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    db
}
fn mail(id: Uuid) -> Mail {
    Mail {
        id,
        recipient: "smtp-synthetic@example.test".into(),
        purpose: "register".into(),
        language: "zh-CN",
        expires_in: 600,
        verification_token: Some("A".repeat(43)),
    }
}
async fn status(admin: &Client, id: Uuid) -> String {
    admin
        .query_one(
            "SELECT status FROM caidex.mail_tasks WHERE task_id=$1",
            &[&id],
        )
        .await
        .unwrap()
        .get(0)
}
#[tokio::test]
#[ignore = "scripts/i7-operations.py provides isolated PG and loopback TLS; never real SMTP"]
async fn directmail_tls_ledger_and_account_flow() {
    let admin = db(&std::env::var("CAIDEX_I7_ADMIN_DB").unwrap()).await;
    let dsn = std::env::var("CAIDEX_I7_AUTH_DB").unwrap();
    admin
        .batch_execute("DELETE FROM caidex.mail_suppressions;")
        .await
        .unwrap();
    let fixture = Fixture::new("smtpdm.aliyun.com", false).await;
    let sender = fixture.sender(&dsn, 100, 200).await;
    let id = Uuid::new_v4();
    sender.send(mail(id)).await.unwrap();
    sender.send(mail(id)).await.unwrap();
    check(
        "smtp_tls_auth_from_mime_and_same_task_only_once",
        fixture.count() == 1
            && fixture
                .commands
                .lock()
                .unwrap()
                .iter()
                .any(|s| s.contains(FROM))
            && fixture.body(0).contains(&"A".repeat(43)),
    );
    check(
        "smtp_acceptance_is_not_delivered_and_no_fake_provider_id",
        status(&admin, id).await == "accepted"
            && admin
                .query_one(
                    "SELECT provider_id IS NULL FROM caidex.mail_tasks WHERE task_id=$1",
                    &[&id],
                )
                .await
                .unwrap()
                .get::<_, bool>(0),
    );
    check(
        "smtp_stable_message_id",
        fixture.messages.lock().unwrap()[0].contains(&format!("<{id}@caidex.cn>")),
    );
    check(
        "smtp_multipart_html_and_plain_content_match",
        fixture.messages.lock().unwrap()[0].contains("multipart/alternative")
            && fixture.part(0, "text/html").contains(&"A".repeat(43))
            && fixture.part(0, "text/html").contains("CAIDEX"),
    );
    let mut changed = mail(id);
    changed.recipient = "changed@example.test".into();
    check(
        "smtp_same_task_changed_payload_rejected",
        matches!(sender.send(changed).await, Err(Error::Conflict)),
    );
    check(
        "smtp_callback_always_denied",
        matches!(
            sender
                .callback(&axum::http::HeaderMap::new(), b"{\"event\":\"delivered\"}")
                .await,
            Err(Error::Denied)
        ),
    );
    let http = ProviderEmail::connect(
        &dsn,
        Provider::Brevo,
        "account@example.test".into(),
        Secret::new("synthetic unrelated API key".into()).unwrap(),
        Secret::new("synthetic unrelated webhook password".into()).unwrap(),
        100,
        200,
    )
    .await
    .unwrap();
    check(
        "same_task_cannot_switch_to_http_provider",
        matches!(http.send(mail(id)).await, Err(Error::Conflict)),
    );
    let competing = fixture.sender(&dsn, 100, 200).await;
    let same = Uuid::new_v4();
    let before = fixture.count();
    let (a, b) = tokio::join!(sender.send(mail(same)), competing.send(mail(same)));
    check(
        "two_instances_concurrent_task_one_physical_send",
        (a.is_ok() || b.is_ok()) && fixture.count() == before + 1,
    );
    for (mode, name, expected) in [
        (Mode::AuthReject, "smtp_auth_failure", "rejected"),
        (
            Mode::RecipientReject,
            "smtp_recipient_rejection",
            "rejected",
        ),
        (
            Mode::DataReject,
            "smtp_temporary_data_rejection",
            "rejected",
        ),
        (Mode::DropData, "smtp_lost_data_ack", "unknown"),
        (Mode::DelayData, "smtp_late_data_ack", "unknown"),
        (
            Mode::DropAccepted,
            "smtp_final250_then_disconnect",
            "accepted",
        ),
        (Mode::HugeReply, "smtp_oversized_response", "unknown"),
    ] {
        *fixture.mode.lock().unwrap() = mode;
        let id = Uuid::new_v4();
        let result = sender.send(mail(id)).await;
        check(
            name,
            status(&admin, id).await == expected && (result.is_ok() == (expected == "accepted")),
        );
        let before = fixture.count();
        let _ = sender.send(mail(id)).await;
        check(&format!("{name}_no_retry"), fixture.count() == before);
    }
    tokio::time::sleep(Duration::from_millis(1200)).await;
    *fixture.mode.lock().unwrap() = Mode::DelayData;
    let id = Uuid::new_v4();
    let _ = tokio::time::timeout(Duration::from_millis(200), sender.send(mail(id))).await;
    let reopened = fixture.sender(&dsn, 100, 200).await;
    check(
        "cancelled_send_and_reopen_keep_unknown_no_retry",
        status(&admin, id).await == "unknown"
            && matches!(reopened.send(mail(id)).await, Err(Error::Unavailable)),
    );
    *fixture.mode.lock().unwrap() = Mode::Accept;
    for (host, expired, label) in [
        ("wrong.example.test", false, "wrong_tls_hostname"),
        ("smtpdm.aliyun.com", true, "expired_tls_certificate"),
    ] {
        let bad = Fixture::new(host, expired).await;
        let client = bad.sender(&dsn, 100, 200).await;
        check(
            label,
            client.send(mail(Uuid::new_v4())).await.is_err()
                && bad.commands.lock().unwrap().is_empty(),
        );
    }
    let alien = Fixture::new("smtpdm.aliyun.com", false).await;
    let mut untrusted = fixture.sender(&dsn, 100, 200).await;
    untrusted
        .use_test_endpoint(
            alien.address,
            &fixture.ca,
            Secret::new(PASSWORD.into()).unwrap(),
        )
        .unwrap();
    check(
        "untrusted_tls_ca_no_auth_or_data",
        untrusted.send(mail(Uuid::new_v4())).await.is_err()
            && alien.commands.lock().unwrap().is_empty(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let plaintext = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(b"220 plaintext SMTP\r\n").await.unwrap();
        let mut bytes = [0u8; 4096];
        let read = stream.read(&mut bytes).await.unwrap();
        bytes[..read].to_vec()
    });
    let mut client = fixture.sender(&dsn, 100, 200).await;
    client
        .use_test_endpoint(address, &fixture.ca, Secret::new(PASSWORD.into()).unwrap())
        .unwrap();
    let rejected = client.send(mail(Uuid::new_v4())).await.is_err();
    let observed = tokio::time::timeout(Duration::from_secs(2), plaintext)
        .await
        .unwrap()
        .unwrap();
    check(
        "plaintext_smtp_refused_without_auth_or_downgrade",
        rejected && observed.first() == Some(&0x16),
    );
    drop(client);
    let mut injected = mail(Uuid::new_v4());
    injected.recipient = "victim@example.test\r\nBcc: other@example.test".into();
    let before = fixture.count();
    check(
        "recipient_header_injection_refused",
        sender.send(injected).await.is_err() && fixture.count() == before,
    );
    admin.batch_execute("CREATE FUNCTION caidex.smtp_fail_result() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic result failure'; END $$; CREATE TRIGGER smtp_fail_result BEFORE UPDATE ON caidex.mail_tasks FOR EACH ROW EXECUTE FUNCTION caidex.smtp_fail_result();").await.unwrap();
    let id = Uuid::new_v4();
    let before = fixture.count();
    let result = sender.send(mail(id)).await;
    check(
        "accepted_then_database_failure_stays_unknown_without_resend",
        result.is_err()
            && status(&admin, id).await == "unknown"
            && fixture.count() == before + 1
            && matches!(sender.send(mail(id)).await, Err(Error::Unavailable)),
    );
    admin.batch_execute("DROP TRIGGER smtp_fail_result ON caidex.mail_tasks; DROP FUNCTION caidex.smtp_fail_result();").await.unwrap();
    admin.batch_execute("INSERT INTO caidex.mail_suppressions(recipient_hash,reason) VALUES(digest('smtp-synthetic@example.test','sha256'),'bounce');").await.unwrap();
    check(
        "smtp_cannot_bypass_existing_suppression",
        matches!(sender.send(mail(Uuid::new_v4())).await, Err(Error::Denied)),
    );
    admin
        .batch_execute("DELETE FROM caidex.mail_suppressions;")
        .await
        .unwrap();
    let used = admin
        .query_one("SELECT count(*) FROM caidex.mail_tasks", &[])
        .await
        .unwrap()
        .get::<_, i64>(0);
    let budget = fixture.sender(&dsn, used, 200).await;
    check(
        "smtp_budget_counts_all_prior_providers_and_failed_attempts",
        matches!(budget.send(mail(Uuid::new_v4())).await, Err(Error::Limited))
            && admin.query_one("SELECT EXISTS(SELECT FROM caidex.mail_tasks WHERE provider='brevo') AND EXISTS(SELECT FROM caidex.mail_tasks WHERE provider='aliyun_directmail' AND status='unknown')", &[]).await.unwrap().get::<_,bool>(0),
    );
    check("ledger_has_no_plain_smtp_secret_or_verification_body",admin.query_one("SELECT bool_and(to_jsonb(t)::text NOT LIKE '%synthetic SMTP password%' AND to_jsonb(t)::text NOT LIKE '%AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA%' AND to_jsonb(t)::text NOT LIKE '%smtp-synthetic@example.test%') FROM caidex.mail_tasks t",&[]).await.unwrap().get::<_,bool>(0));
    drop(http);
    drop(budget);
    drop(competing);
    drop(reopened);
    drop(untrusted);
    flow(&admin, &dsn, &fixture).await;
}

async fn flow(admin: &Client, dsn: &str, fixture: &Fixture) {
    let data = std::env::var("CAIDEX_I7_DATA_DB").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let trust = Trust::local_test(&format!("http://localhost:{}/", address.port())).unwrap();
    let service = Service::connect(
        dsn,
        &data,
        trust,
        "localhost",
        [42; 32],
        fixture.sender(dsn, 100, 200).await,
    )
    .await
    .unwrap();
    use std::future::IntoFuture;
    let task = tokio::spawn(
        axum::serve(
            listener,
            router(service).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .into_future(),
    );
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let call = |action: &str, body: Value| {
        http.post(format!("http://{address}/v1/auth/{action}"))
            .header("content-type", "application/json")
            .body(body.to_string())
    };
    let email = "smtp-flow@example.test";
    let password = "Oldpwd12";
    let updated = "Newpwd34";
    check(
        "smtp_api_seven_character_password_refused_without_mail",
        call(
            "register",
            json!({"email":"short-smtp@example.test","password":"1234567"}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 400,
    );
    let before = fixture.count();
    check(
        "smtp_api_registration_accepts_once",
        call("register", json!({"email":email,"password":password}))
            .send()
            .await
            .unwrap()
            .status()
            == 200
            && fixture.count() == before + 1,
    );
    check(
        "smtp_api_registration_rate_limit_no_extra_mail",
        call("register", json!({"email":email,"password":password}))
            .send()
            .await
            .unwrap()
            .status()
            == 429
            && fixture.count() == before + 1,
    );
    let body = fixture.body(before);
    let token = body
        .split("验证码：")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    check(
        "smtp_api_code_is_six_ascii_digits",
        token.len() == 6 && token.bytes().all(|c| c.is_ascii_digit()),
    );
    check(
        "smtp_api_code_requires_mailbox_context",
        call("verify", json!({"purpose":"register","token":token}))
            .send()
            .await
            .unwrap()
            .status()
            == 400,
    );
    check(
        "smtp_api_code_cannot_cross_mailbox",
        call(
            "verify",
            json!({"email":"other-smtp@example.test","purpose":"register","token":token}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 400,
    );
    check(
        "smtp_api_code_cannot_cross_purpose",
        call(
            "verify",
            json!({"email":email,"purpose":"recover","token":token,"password":updated}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 400,
    );
    let wrong = if token == "000000" {
        "111111"
    } else {
        "000000"
    };
    check(
        "smtp_api_bad_verification_code_refused",
        call(
            "verify",
            json!({"email":email,"purpose":"register","token":wrong}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 400,
    );
    admin.execute("UPDATE caidex.auth_challenges SET expires_at=now()-interval '1 second' WHERE purpose='register' AND target=$1", &[&email]).await.unwrap();
    check(
        "smtp_api_expired_verification_code_refused",
        call(
            "verify",
            json!({"email":email,"purpose":"register","token":token}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 400,
    );
    // Restore only this synthetic challenge's clock to finish the success path without an extra send.
    admin.execute("UPDATE caidex.auth_challenges SET expires_at=now()+interval '10 minutes' WHERE purpose='register' AND target=$1", &[&email]).await.unwrap();
    let verified = call(
        "verify",
        json!({"email":email,"purpose":"register","token":token}),
    )
    .send()
    .await
    .unwrap();
    check(
        "smtp_api_email_verification_succeeds",
        verified.status() == 200,
    );
    check(
        "smtp_api_verification_replay_refused",
        call(
            "verify",
            json!({"email":email,"purpose":"register","token":token}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 400,
    );
    let logged = call("login", json!({"email":email,"password":password}))
        .send()
        .await
        .unwrap();
    check(
        "smtp_api_password_login_after_verification",
        logged.status() == 200,
    );
    let session: Value = serde_json::from_str(&logged.text().await.unwrap()).unwrap();
    // Synthetic fixture only: age the existing address limiter instead of weakening production limits.
    admin.batch_execute("UPDATE caidex.auth_limits SET reset_at=now()-interval '2 hours',last_at=now()-interval '2 hours';").await.unwrap();
    let before = fixture.count();
    check(
        "smtp_api_recovery_emits_one_message",
        call("recover", json!({"email":email}))
            .send()
            .await
            .unwrap()
            .status()
            == 200
            && fixture.count() == before + 1,
    );
    check(
        "smtp_api_recovery_rate_limit_no_extra_mail",
        call("recover", json!({"email":email}))
            .send()
            .await
            .unwrap()
            .status()
            == 429
            && fixture.count() == before + 1,
    );
    let body = fixture.body(before);
    let token = body
        .split("验证码：")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    check(
        "smtp_api_recovery_new_password_verified",
        call(
            "verify",
            json!({"email":email,"purpose":"recover","token":token,"password":updated}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 200,
    );
    check(
        "smtp_api_old_session_revoked",
        call("me", json!({}))
            .bearer_auth(session["access_token"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .status()
            == 401,
    );
    check(
        "smtp_api_old_password_refused",
        call("login", json!({"email":email,"password":password}))
            .send()
            .await
            .unwrap()
            .status()
            == 401,
    );
    check(
        "smtp_api_recovered_password_login_succeeds",
        call("login", json!({"email":email,"password":updated}))
            .send()
            .await
            .unwrap()
            .status()
            == 200,
    );
    check(
        "smtp_api_recovery_code_replay_refused",
        call(
            "verify",
            json!({"email":email,"purpose":"recover","token":token,"password":updated}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 400,
    );
    let callback = http
        .post(format!("http://{address}/v1/mail/events"))
        .body("{\"event\":\"delivered\"}")
        .send()
        .await
        .unwrap();
    check(
        "smtp_http_webhook_never_grants_delivery",
        callback.status() == 401,
    );
    let legacy = "L".repeat(43);
    admin.execute("INSERT INTO caidex.auth_challenges(challenge_hash,purpose,target,state,expires_at) SELECT digest($1::text,'sha256'),'register','legacy-smtp@example.test',jsonb_build_object('password_hash',password_hash),now()+interval '10 minutes' FROM caidex.auth_credentials WHERE kind='password' AND revoked_at IS NULL LIMIT 1", &[&legacy]).await.unwrap();
    check(
        "legacy_already_issued_high_entropy_mail_code_still_verifies",
        call("verify", json!({"purpose":"register","token":legacy}))
            .send()
            .await
            .unwrap()
            .status()
            == 200,
    );
    admin.batch_execute("UPDATE caidex.auth_limits SET reset_at=now()-interval '1 second',last_at=now()-interval '2 hours';").await.unwrap();
    let before = fixture.count();
    check(
        "numeric_code_lockout_fixture_registration",
        call(
            "register",
            json!({"email":"locked-smtp@example.test","password":password}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 200,
    );
    let body = fixture.body(before);
    let locked = body
        .split("验证码：")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let wrong = if locked == "000000" {
        "111111"
    } else {
        "000000"
    };
    for _ in 0..5 {
        assert_eq!(
            call(
                "verify",
                json!({"email":"locked-smtp@example.test","purpose":"register","token":wrong})
            )
            .send()
            .await
            .unwrap()
            .status(),
            400
        );
    }
    check("five_wrong_codes_persistently_lock_challenge",admin.query_one("SELECT attempts=5 AND state ? 'email_code_mac' AND NOT (state ? 'verification_token') FROM caidex.auth_challenges WHERE purpose='register' AND target='locked-smtp@example.test'", &[]).await.unwrap().get::<_,bool>(0));
    check(
        "locked_code_and_sixth_guess_refused",
        call(
            "verify",
            json!({"email":"locked-smtp@example.test","purpose":"register","token":locked}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 429,
    );
    admin.batch_execute("UPDATE caidex.auth_limits SET reset_at=now()-interval '1 second',last_at=now()-interval '2 hours';").await.unwrap();
    check(
        "challenge_lock_survives_rate_window_reset",
        call(
            "verify",
            json!({"email":"locked-smtp@example.test","purpose":"register","token":locked}),
        )
        .send()
        .await
        .unwrap()
        .status()
            == 400,
    );
    task.abort();
    let _ = task.await;
}
