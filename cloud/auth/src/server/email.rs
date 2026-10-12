//! Server-only transactional mail: one durable attempt, finite budgets, authenticated callbacks.
use super::{EmailSender, Error, Mail, Result, digest};
use axum::http::HeaderMap;
use base64::{Engine, engine::general_purpose::STANDARD};
use caidex_credentials::Secret;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

pub mod smtp;
use smtp::AliyunSmtpEmail;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Brevo,
    Resend,
    #[serde(rename = "aliyun_directmail")]
    AliyunDirectMail,
}
impl Provider {
    fn name(self) -> &'static str {
        match self {
            Self::Brevo => "brevo",
            Self::Resend => "resend",
            Self::AliyunDirectMail => "aliyun_directmail",
        }
    }
    fn endpoint(self) -> Option<&'static str> {
        match self {
            Self::Brevo => Some("https://api.brevo.com/v3/smtp/email"),
            Self::Resend => Some("https://api.resend.com/emails"),
            Self::AliyunDirectMail => None,
        }
    }
}

pub struct ProviderEmail {
    ledger: MailLedger,
    http: reqwest::Client,
    provider: Provider,
    endpoint: String,
    from: String,
    api_key: Secret,
    callback_key: Secret,
}
impl<S: EmailSender> EmailSender for Option<S> {
    fn enabled(&self) -> bool {
        self.is_some()
    }
    async fn send(&self, mail: Mail) -> Result<()> {
        match self {
            Some(sender) => sender.send(mail).await,
            None => Err(Error::Unavailable),
        }
    }
    async fn callback(&self, headers: &HeaderMap, body: &[u8]) -> Result<()> {
        match self {
            Some(sender) => sender.callback(headers, body).await,
            None => Err(Error::Denied),
        }
    }
}
impl ProviderEmail {
    pub async fn connect(
        dsn: &str,
        provider: Provider,
        from: String,
        api_key: Secret,
        callback_key: Secret,
        daily_budget: i64,
        monthly_budget: i64,
    ) -> Result<Self> {
        let endpoint = provider.endpoint().ok_or(Error::Invalid)?;
        if api_key.expose().len() < 20 || callback_key.expose().len() < 32 {
            return Err(Error::Invalid);
        }
        super::email(&json!({"email":from}))?;
        let ledger = MailLedger::connect(
            dsn,
            provider,
            daily_budget,
            monthly_budget,
            api_key.expose(),
        )
        .await?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|_| Error::Unavailable)?;
        Ok(Self {
            ledger,
            http,
            provider,
            endpoint: endpoint.into(),
            from,
            api_key,
            callback_key,
        })
    }
    /// Only synthetic test-auth builds can redirect mail to a loopback HTTP fixture.
    #[cfg(feature = "test-auth")]
    pub fn use_test_endpoint(&mut self, endpoint: &str) -> Result<()> {
        let url = url::Url::parse(endpoint).map_err(|_| Error::Invalid)?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(Error::Denied);
        }
        self.endpoint = endpoint.into();
        self.http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|_| Error::Unavailable)?;
        Ok(())
    }
    async fn deliver(&self, mail: &Mail, subject: &str, text: &str, html: &str) -> Result<String> {
        let id = mail.id.to_string();
        let (body, request) = match self.provider {
            Provider::Brevo => (
                json!({"sender":{"email":self.from,"name":"CAIdex"},"to":[{"email":mail.recipient}],"subject":subject,"textContent":text,"htmlContent":html,"headers":{"Idempotency-Key":id},"tags":[id]}),
                self.http
                    .post(&self.endpoint)
                    .header("api-key", self.api_key.expose()),
            ),
            Provider::AliyunDirectMail => return Err(Error::Invalid),
            Provider::Resend => (
                json!({"from":self.from,"to":[mail.recipient],"subject":subject,"text":text,"html":html,"tags":[{"name":"caidex_task","value":id}]}),
                self.http
                    .post(&self.endpoint)
                    .bearer_auth(self.api_key.expose())
                    .header("Idempotency-Key", id),
            ),
        };
        let response = request
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        if !response.status().is_success() {
            return Err(if response.status().is_client_error() {
                Error::Invalid
            } else {
                Error::Unavailable
            });
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unavailable)? {
            if bytes.len() + chunk.len() > 16384 {
                return Err(Error::Unavailable);
            }
            bytes.extend_from_slice(&chunk);
        }
        let reply: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Unavailable)?;
        let key = if self.provider == Provider::Brevo {
            "messageId"
        } else {
            "id"
        };
        reply[key]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control))
            .map(str::to_owned)
            .ok_or(Error::Unavailable)
    }
    fn verify_callback(&self, headers: &HeaderMap, body: &[u8]) -> Result<()> {
        if body.len() > 65536 {
            return Err(Error::Invalid);
        }
        let header = |key| {
            headers
                .get(key)
                .and_then(|value| value.to_str().ok())
                .ok_or(Error::Denied)
        };
        match self.provider {
            Provider::AliyunDirectMail => Err(Error::Denied),
            Provider::Brevo => {
                let token = header("authorization")?
                    .strip_prefix("Bearer ")
                    .ok_or(Error::Denied)?;
                if digest(token)
                    .ct_eq(&digest(self.callback_key.expose()))
                    .into()
                {
                    Ok(())
                } else {
                    Err(Error::Denied)
                }
            }
            Provider::Resend => {
                let id = header("svix-id")?;
                let timestamp = header("svix-timestamp")?;
                let signed = header("svix-signature")?;
                let seconds: u64 = timestamp.parse().map_err(|_| Error::Denied)?;
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| Error::Unavailable)?
                    .as_secs();
                if id.is_empty()
                    || id.len() > 128
                    || now.abs_diff(seconds) > 300
                    || signed.len() > 1024
                {
                    return Err(Error::Denied);
                }
                let key = STANDARD
                    .decode(
                        self.callback_key
                            .expose()
                            .strip_prefix("whsec_")
                            .ok_or(Error::Denied)?,
                    )
                    .map_err(|_| Error::Denied)?;
                let mut mac = Hmac::<Sha256>::new_from_slice(&key).map_err(|_| Error::Denied)?;
                mac.update(format!("{id}.{timestamp}.").as_bytes());
                mac.update(body);
                for signature in signed
                    .split_whitespace()
                    .filter_map(|part| part.strip_prefix("v1,"))
                {
                    if let Ok(bytes) = STANDARD.decode(signature)
                        && mac.clone().verify_slice(&bytes).is_ok()
                    {
                        return Ok(());
                    }
                }
                Err(Error::Denied)
            }
        }
    }
}
impl EmailSender for ProviderEmail {
    async fn send(&self, mail: Mail) -> Result<()> {
        let (subject, text, html) = template(&mail)?;
        if !self.ledger.reserve(&mail).await? {
            return Ok(());
        }
        // Durable unknown is committed before HTTP; cancellation/crash can never justify another send.
        let result = self.deliver(&mail, &subject, &text, &html).await.map(Some);
        self.ledger.complete(&mail.id, &result).await?;
        result.map(|_| ())
    }
    async fn callback(&self, headers: &HeaderMap, body: &[u8]) -> Result<()> {
        self.verify_callback(headers, body)?;
        let value: Value = serde_json::from_slice(body).map_err(|_| Error::Invalid)?;
        let (event, id) = match self.provider {
            Provider::AliyunDirectMail => return Err(Error::Denied),
            Provider::Brevo => (value["event"].as_str(), value["message-id"].as_str()),
            Provider::Resend => (value["type"].as_str(), value["data"]["email_id"].as_str()),
        };
        let status = match event.ok_or(Error::Invalid)? {
            "delivered" | "email.delivered" => "delivered",
            "hard_bounce" | "hardBounce" | "invalid" | "blocked" | "email.bounced" => "bounced",
            "spam" | "email.complained" => "complaint",
            _ => return Ok(()), // Opening/clicks/soft failures do not grant delivery or unblock suppression.
        };
        let id = id
            .filter(|s| !s.is_empty() && s.len() <= 512)
            .ok_or(Error::Invalid)?;
        let mut db = self.ledger.db.lock().await;
        let tx = db.transaction().await?;
        if let Some(row) = tx.query_opt("SELECT task_id,recipient_hash FROM caidex.mail_tasks WHERE provider=$1 AND provider_id=$2 FOR UPDATE", &[&self.provider.name(),&id]).await? {
            let task: Uuid = row.get(0);
            if status != "delivered" {
                tx.execute("INSERT INTO caidex.mail_suppressions(recipient_hash,reason) VALUES($1,$2) ON CONFLICT DO NOTHING", &[&row.get::<_,Vec<u8>>(1),&if status=="complaint" {"complaint"} else {"bounce"}]).await?;
            }
            tx.execute("UPDATE caidex.mail_tasks SET status=$2,updated_at=now() WHERE task_id=$1 AND (status NOT IN('bounced','complaint') OR $2='complaint')", &[&task,&status]).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

/// One durable budget/idempotency policy for the two HTTP providers and SMTP.
struct MailLedger {
    db: Mutex<Client>,
    provider: Provider,
    daily_budget: i64,
    monthly_budget: i64,
    payload_mac: Hmac<Sha256>,
}
impl MailLedger {
    async fn connect(
        dsn: &str,
        provider: Provider,
        daily_budget: i64,
        monthly_budget: i64,
        payload_key: &str,
    ) -> Result<Self> {
        if !(1..=10000).contains(&daily_budget)
            || !(1..=100000).contains(&monthly_budget)
            || daily_budget > monthly_budget
        {
            return Err(Error::Invalid);
        }
        let config: tokio_postgres::Config = dsn.parse().map_err(|_| Error::Invalid)?;
        if config.get_hosts().is_empty()
            || config
                .get_hosts()
                .iter()
                .any(|host| !matches!(host, tokio_postgres::config::Host::Unix(_)))
        {
            return Err(Error::Invalid);
        }
        let (db, connection) = tokio_postgres::connect(dsn, NoTls).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let denied: bool = db.query_one("SELECT rolsuper OR rolbypassrls OR rolcreatedb OR rolcreaterole OR has_schema_privilege(current_user,'caidex','CREATE') OR has_table_privilege(current_user,'caidex.messages','SELECT') FROM pg_roles WHERE rolname=current_user", &[]).await?.get(0);
        if denied {
            return Err(Error::Denied);
        }
        db.batch_execute("SET statement_timeout='3s'; SET lock_timeout='2s'; SET idle_in_transaction_session_timeout='3s'").await?;
        Ok(Self {
            db: Mutex::new(db),
            provider,
            daily_budget,
            monthly_budget,
            payload_mac: Hmac::<Sha256>::new_from_slice(payload_key.as_bytes())
                .map_err(|_| Error::Invalid)?,
        })
    }
    async fn reserve(&self, mail: &Mail) -> Result<bool> {
        let hash = payload_hash(mail, &self.payload_mac);
        let mut db = self.db.lock().await;
        let tx = db.transaction().await?;
        // All service instances/providers share one budget; reservations count even after a timeout.
        tx.execute("SELECT pg_advisory_xact_lock(714007)", &[])
            .await?;
        if let Some(row) = tx
            .query_opt(
                "SELECT payload_hash,status,provider FROM caidex.mail_tasks WHERE task_id=$1",
                &[&mail.id],
            )
            .await?
        {
            if row.get::<_, Vec<u8>>(0) != hash || row.get::<_, String>(2) != self.provider.name() {
                return Err(Error::Conflict);
            }
            let accepted = matches!(row.get::<_, String>(1).as_str(), "accepted" | "delivered");
            tx.commit().await?;
            return if accepted {
                Ok(false)
            } else {
                Err(Error::Unavailable)
            };
        }
        let recipient = digest(&mail.recipient);
        if tx
            .query_opt(
                "SELECT 1 FROM caidex.mail_suppressions WHERE recipient_hash=$1",
                &[&recipient],
            )
            .await?
            .is_some()
        {
            return Err(Error::Denied);
        }
        let counts = tx.query_one("SELECT count(*) FILTER(WHERE created_at>=date_trunc('day',now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'),count(*) FROM caidex.mail_tasks WHERE created_at>=date_trunc('month',now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'", &[]).await?;
        let daily: i64 = counts.get(0);
        let monthly: i64 = counts.get(1);
        let essential = matches!(mail.purpose.as_str(), "register" | "recover" | "email");
        if daily >= self.daily_budget
            || monthly >= self.monthly_budget
            || (!essential
                && (daily * 10 >= self.daily_budget * 9 || monthly * 10 >= self.monthly_budget * 9))
        {
            eprintln!("Account mail budget exhausted; no fallback or paid send");
            return Err(Error::Limited);
        }
        tx.execute("INSERT INTO caidex.mail_tasks(task_id,recipient_hash,payload_hash,purpose,provider,expires_at) VALUES($1,$2,$3,$4,$5,now()+($6::int*interval '1 second'))", &[&mail.id,&recipient,&hash,&mail.purpose,&self.provider.name(),&(mail.expires_in.max(60) as i32)]).await?;
        tx.commit().await?;
        Ok(true)
    }
    async fn complete(&self, task: &Uuid, result: &Result<Option<String>>) -> Result<()> {
        let (status, provider_id) = match result {
            Ok(id) => ("accepted", id.clone()),
            Err(Error::Invalid) => ("rejected", None),
            _ => ("unknown", None),
        };
        self.db.lock().await.execute("UPDATE caidex.mail_tasks SET status=$2,provider_id=$3,updated_at=now() WHERE task_id=$1 AND status='unknown'", &[task,&status,&provider_id]).await?;
        Ok(())
    }
}

fn payload_hash(mail: &Mail, mac: &Hmac<Sha256>) -> Vec<u8> {
    let payload = json!({"recipient":mail.recipient,"purpose":mail.purpose,"language":mail.language,"expires_in":mail.expires_in,"token":mail.verification_token}).to_string();
    if mail
        .verification_token
        .as_deref()
        .is_some_and(super::is_email_code)
    {
        // Six-digit codes must not be recoverable by enumerating an unkeyed ledger fingerprint.
        let mut mac = mac.clone();
        mac.update(b"caidex-mail-payload-v1\0");
        mac.update(payload.as_bytes());
        mac.finalize().into_bytes().to_vec()
    } else {
        digest(&payload)
    }
}

pub enum Sender {
    Http(ProviderEmail),
    Aliyun(AliyunSmtpEmail),
}
impl EmailSender for Sender {
    async fn send(&self, mail: Mail) -> Result<()> {
        match self {
            Self::Http(sender) => sender.send(mail).await,
            Self::Aliyun(sender) => sender.send(mail).await,
        }
    }
    async fn callback(&self, headers: &HeaderMap, body: &[u8]) -> Result<()> {
        match self {
            Self::Http(sender) => sender.callback(headers, body).await,
            Self::Aliyun(_) => Err(Error::Denied),
        }
    }
}

fn template(mail: &Mail) -> Result<(String, String, String)> {
    super::email(&json!({"email":mail.recipient}))?;
    if !matches!(mail.language, "zh-CN" | "en")
        || mail.purpose.is_empty()
        || mail.purpose.len() > 64
        || mail.expires_in > 600
    {
        return Err(Error::Invalid);
    }
    let title = match mail.purpose.as_str() {
        "register" => ("验证邮箱", "Verify your email"),
        "recover" => ("恢复账户", "Recover your account"),
        "email" => ("确认新邮箱", "Confirm your new email"),
        "security"
        | "email_change_requested"
        | "credential_added"
        | "credential_revoked"
        | "sessions_revoked"
        | "password_changed"
        | "account_disabled"
        | "new_session"
        | "refresh_replay" => ("账户安全通知", "Account security notice"),
        _ => return Err(Error::Invalid),
    };
    let subject = format!(
        "CAIdex — {}",
        if mail.language == "en" {
            title.1
        } else {
            title.0
        }
    );
    let text = if let Some(token) = &mail.verification_token {
        if !(super::is_email_code(token) || token.len() == 43)
            || mail.expires_in == 0
            || !matches!(mail.purpose.as_str(), "register" | "recover" | "email")
            || !token
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err(Error::Invalid);
        }
        if mail.language == "en" {
            format!(
                "{subject}\nVerification code: {token}\nExpires in {} seconds. Enter it in CAIdex. Ignore this email if you did not request it.",
                mail.expires_in
            )
        } else {
            format!(
                "{subject}\n验证码：{token}\n有效期 {} 秒，请在 CAIdex 中输入。若非本人请求请忽略。",
                mail.expires_in
            )
        }
    } else {
        if matches!(mail.purpose.as_str(), "register" | "recover" | "email") {
            return Err(Error::Invalid);
        }
        if mail.language == "en" {
            "Your CAIdex account security settings changed. Review Account and Security in CAIdex. No password or session token is included.".into()
        } else {
            "您的 CAIdex 账户安全设置有变更，请在 CAIdex 的账户与安全中核对。本邮件不含密码或会话令牌。".into()
        }
    };
    let chinese = mail.language == "zh-CN";
    let intro = if mail.verification_token.is_some() {
        match (mail.purpose.as_str(), chinese) {
            ("register", true) => "欢迎使用 CAIDEX。请输入下方验证码，确认你的邮箱。",
            ("recover", true) => "我们收到了账户密码找回请求。请使用下方验证码完成密码重设。",
            (_, true) => "请使用下方验证码确认新的邮箱地址。",
            ("register", false) => {
                "Welcome to CAIDEX. Enter the code below to verify your email address."
            }
            ("recover", false) => {
                "We received a password recovery request. Use the code below to reset your password."
            }
            (_, false) => "Use the code below to confirm your new email address.",
        }
    } else if chinese {
        "你的账户安全设置发生了变更。请在 CAIDEX 的账户与安全中查看；本邮件不含密码或会话令牌。"
    } else {
        "Your account security settings changed. Review Account and Security in CAIDEX. No password or session token is included."
    };
    let code_block = if let Some(code) = &mail.verification_token {
        let size = if code.len() == 6 { "36px" } else { "18px" };
        let expiry = if chinese {
            format!(
                "有效期 {} 分钟 · 仅可使用一次",
                mail.expires_in.div_ceil(60)
            )
        } else {
            format!(
                "Valid for {} minutes · Single use",
                mail.expires_in.div_ceil(60)
            )
        };
        format!(
            "<div style=\"background:#f5f6fc;border:1px solid #e7eafa;border-radius:10px;padding:24px 12px;text-align:center\"><div style=\"color:#3549bc;font-size:{size};font-family:Consolas,Menlo,monospace;font-weight:700;letter-spacing:5px;word-break:break-all\">{code}</div><div style=\"margin-top:12px;color:#8b92a6;font-size:12px\">{expiry}</div></div>"
        )
    } else {
        String::new()
    };
    let notice = match (mail.verification_token.is_some(), chinese) {
        (true, true) => "如非本人操作，请忽略此邮件。请勿向他人提供验证码。",
        (true, false) => {
            "If you did not request this, ignore this email. Never share your verification code."
        }
        (false, true) => "如非本人操作，请立即检查账户安全设置。",
        (false, false) => {
            "If you did not make this change, review your account security immediately."
        }
    };
    let html = include_str!("email.html")
        .replace("{{language}}", mail.language)
        .replace("{{subject}}", &subject)
        .replace(
            "{{eyebrow}}",
            if chinese {
                "账户与安全"
            } else {
                "ACCOUNT SECURITY"
            },
        )
        .replace("{{headline}}", if chinese { title.0 } else { title.1 })
        .replace("{{intro}}", intro)
        .replace("{{code_block}}", &code_block)
        .replace("{{notice}}", notice)
        .replace(
            "{{footer}}",
            if chinese {
                "此邮件由 CAIDEX 自动发送，请勿回复。"
            } else {
                "This is an automated CAIDEX message. Please do not reply."
            },
        );
    Ok((subject, text, html))
}

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn localized_html_and_plain_text_preserve_codes_and_security_notices() {
        for language in ["zh-CN", "en"] {
            for purpose in ["register", "recover", "email", "security"] {
                let mail = Mail {
                    id: Uuid::new_v4(),
                    recipient: "preview@example.test".into(),
                    purpose: purpose.into(),
                    language,
                    verification_token: (purpose != "security").then(|| "012345".into()),
                    expires_in: if purpose == "security" { 0 } else { 600 },
                };
                let (_, text, html) = template(&mail).unwrap();
                if purpose != "security" {
                    let key = Hmac::<Sha256>::new_from_slice(b"synthetic-provider-key").unwrap();
                    let other = Hmac::<Sha256>::new_from_slice(b"different-provider-key").unwrap();
                    assert_eq!(payload_hash(&mail, &key), payload_hash(&mail, &key));
                    assert_ne!(payload_hash(&mail, &key), payload_hash(&mail, &other));
                    let mut changed = Mail {
                        id: mail.id,
                        recipient: mail.recipient.clone(),
                        purpose: mail.purpose.clone(),
                        language,
                        expires_in: 600,
                        verification_token: Some("012346".into()),
                    };
                    assert_ne!(payload_hash(&mail, &key), payload_hash(&changed, &key));
                    changed.verification_token = Some("L".repeat(43));
                    assert_eq!(payload_hash(&changed, &key), payload_hash(&changed, &other));
                }
                assert!(html.contains("CAIDEX"));
                assert!(
                    !html.contains("{{") && !html.contains("<script") && !html.contains("<img")
                );
                if purpose == "security" {
                    assert!(!html.contains("012345"));
                    assert!(html.contains(if language == "en" {
                        "immediately"
                    } else {
                        "立即"
                    }));
                } else {
                    assert!(text.contains("012345") && html.contains("012345"));
                    assert!(html.contains(if language == "en" {
                        "10 minutes"
                    } else {
                        "10 分钟"
                    }));
                    if language == "zh-CN" && purpose == "register" {
                        std::fs::write(
                            std::env::temp_dir().join("caidex-email-preview.html"),
                            &html,
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
}
