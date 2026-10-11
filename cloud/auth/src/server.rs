//! Linux account service. Migration/admin and ordinary data connections are separate.
mod grants;
mod passkeys;

use crate::{AUDIENCE, CLI_CLIENT, Trust};
use argon2::{
    Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{future::Future, net::SocketAddr, sync::Arc};
use tokio::sync::{Mutex, Semaphore};
use tokio_postgres::{Client, NoTls, Row, Transaction};
use uuid::Uuid;
use webauthn_rs::prelude::*;

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid_request")]
    Invalid,
    #[error("access_denied")]
    Denied,
    #[error("invalid_grant")]
    Grant,
    #[error("authorization_pending")]
    Pending,
    #[error("slow_down")]
    Slow,
    #[error("expired_token")]
    Expired,
    #[error("rate_limited")]
    Limited,
    #[error("temporarily_unavailable")]
    Unavailable,
}
impl From<tokio_postgres::Error> for Error {
    fn from(error: tokio_postgres::Error) -> Self {
        if let Some(code) = error.code() {
            eprintln!("Account database failure (SQLSTATE {})", code.code());
        }
        Self::Unavailable
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Limited => StatusCode::TOO_MANY_REQUESTS,
            Self::Denied => StatusCode::UNAUTHORIZED,
            _ => StatusCode::BAD_REQUEST,
        };
        (
            status,
            [("Cache-Control", "no-store"), ("Pragma", "no-cache")],
            Json(json!({"error":self.to_string()})),
        )
            .into_response()
    }
}

/// Transactions commit intentional rejection effects (attempt counts/replay revocation),
/// but DB/runtime failure rolls back every partial write. Raw DB errors never leave the service.
pub struct Service<S: EmailSender> {
    auth: Mutex<Client>,
    data: Mutex<Client>,
    hashes: Arc<Semaphore>,
    dummy_hash: String,
    trust: Trust,
    webauthn: Webauthn,
    rp_id: String,
    sender: S,
    code_key: [u8; 32],
}

/// Transport only; providers receive minimal recipient/template/expiry/idempotency data.
pub struct Mail {
    pub id: Uuid,
    pub recipient: String,
    pub purpose: String,
    pub language: &'static str,
    pub expires_in: u32,
    pub verification_token: Option<String>,
}
pub trait EmailSender: Send + Sync + 'static {
    fn send(&self, mail: Mail) -> impl Future<Output = Result<()>> + Send;
}

/// Process-private sandbox; it never sends mail or exposes an inbox HTTP endpoint.
#[derive(Clone, Default)]
pub struct SandboxMail(pub Arc<Mutex<Vec<Mail>>>);
impl EmailSender for SandboxMail {
    async fn send(&self, mail: Mail) -> Result<()> {
        self.0.lock().await.push(mail);
        Ok(())
    }
}

pub fn digest(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}
fn secret() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn fields(body: &Value, allowed: &[&str]) -> Result<()> {
    let object = body.as_object().ok_or(Error::Invalid)?;
    if object.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn text<'a>(body: &'a Value, key: &str, max: usize) -> Result<&'a str> {
    body[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= max)
        .ok_or(Error::Invalid)
}
fn uuid(body: &Value, key: &str) -> Result<Uuid> {
    Uuid::parse_str(text(body, key, 36)?).map_err(|_| Error::Invalid)
}
fn email(body: &Value) -> Result<String> {
    let value = text(body, "email", 254)?.trim();
    let (local, domain) = value.split_once('@').ok_or(Error::Invalid)?;
    if local.is_empty()
        || local.len() > 64
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&c))
        || domain.split('.').count() < 2
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || !label.as_bytes()[0].is_ascii_alphanumeric()
                || !label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                || !label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err(Error::Invalid);
    }
    // Domain case folds; local spelling, dots and + aliases are retained.
    Ok(format!("{local}@{}", domain.to_ascii_lowercase()))
}
fn password(body: &Value) -> Result<String> {
    let password = text(body, "password", 1024)?;
    if password.chars().count() < 15 {
        return Err(Error::Invalid);
    }
    Ok(password.to_owned())
}
fn json_encode<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(|_| Error::Unavailable)
}
fn json_decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Error::Invalid)
}

impl<S: EmailSender> Service<S> {
    /// No DB password is accepted in command arguments; the current deployment uses local Unix sockets.
    pub async fn connect(
        auth_dsn: &str,
        data_dsn: &str,
        trust: Trust,
        rp_id: &str,
        code_key: [u8; 32],
        sender: S,
    ) -> Result<Arc<Self>> {
        for dsn in [auth_dsn, data_dsn] {
            let config: tokio_postgres::Config = dsn.parse().map_err(|_| Error::Invalid)?;
            if config
                .get_hosts()
                .iter()
                .any(|h| !matches!(h, tokio_postgres::config::Host::Unix(_)))
                || config.get_hosts().is_empty()
            {
                return Err(Error::Invalid);
            }
        }
        let (auth, connection) = tokio_postgres::connect(auth_dsn, NoTls).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let (data, connection) = tokio_postgres::connect(data_dsn, NoTls).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        for client in [&auth, &data] {
            let row = client.query_one("SELECT rolsuper OR rolbypassrls OR rolcreaterole OR rolcreatedb FROM pg_roles WHERE rolname=current_user",&[]).await?;
            if row.get::<_, bool>(0) {
                return Err(Error::Denied);
            }
            client.batch_execute("SET statement_timeout='10s'; SET lock_timeout='5s'; SET idle_in_transaction_session_timeout='10s'").await?;
        }
        if auth
            .query_one("SELECT max(version) FROM caidex.schema_migrations", &[])
            .await?
            .get::<_, Option<i32>>(0)
            != Some(caidex_cloud_core::SCHEMA_VERSION as i32)
        {
            return Err(Error::Unavailable);
        }
        let check = auth.query_one("SELECT has_schema_privilege(current_user,'caidex','CREATE'), has_table_privilege(current_user,'caidex.messages','SELECT'), has_table_privilege(current_user,'caidex.auth_credentials','SELECT')",&[]).await?;
        if check.get::<_, bool>(0) || check.get::<_, bool>(1) || !check.get::<_, bool>(2) {
            return Err(Error::Denied);
        }
        let check = data
            .query_one(
                "SELECT has_table_privilege(current_user,'caidex.auth_credentials','SELECT')",
                &[],
            )
            .await?;
        if check.get::<_, bool>(0) {
            return Err(Error::Denied);
        }
        let origin = Url::parse(trust.issuer()).map_err(|_| Error::Invalid)?;
        let webauthn = WebauthnBuilder::new(rp_id, &origin)
            .map_err(|_| Error::Invalid)?
            .rp_name("CAIdex")
            .build()
            .map_err(|_| Error::Invalid)?;
        let dummy_hash = hash(secret()?).await?;
        Ok(Arc::new(Self {
            auth: Mutex::new(auth),
            data: Mutex::new(data),
            hashes: Arc::new(Semaphore::new(1)),
            dummy_hash,
            trust,
            webauthn,
            rp_id: rp_id.to_owned(),
            sender,
            code_key,
        }))
    }

    pub fn metadata(&self) -> Value {
        json!({"issuer":self.trust.issuer(),"authorization_endpoint":self.trust.endpoint("authorize"),
            "device_authorization_endpoint":self.trust.endpoint("v1/auth/device"),"token_endpoint":self.trust.endpoint("v1/auth/token"),
            "revocation_endpoint":self.trust.endpoint("v1/auth/revoke"),"verification_uri":self.trust.endpoint("device"),
            "code_challenge_methods_supported":["S256"],"grant_types_supported":["authorization_code","urn:ietf:params:oauth:grant-type:device_code","refresh_token"]})
    }

    pub async fn request(
        &self,
        action: &str,
        bearer: Option<&str>,
        peer: &str,
        body: Value,
    ) -> Result<Value> {
        if peer.len() > 128 {
            return Err(Error::Invalid);
        }
        // One bounded hash/request worker prevents unbounded memory or queued password copies.
        let _permit = self
            .hashes
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Limited)?;
        let mut auth = self.auth.lock().await;
        // Rate state commits independently, including failed attempts. No client-provided IP is trusted.
        let rate = auth.transaction().await?;
        let sensitive = matches!(action, "register" | "recover");
        let target = body.get("email").and_then(Value::as_str).unwrap_or("");
        let mut blocked = false;
        for (key, max, seconds, gap) in [
            (format!("ip:{peer}"), 120, 60, 0),
            ("global".into(), 600, 60, 0),
        ] {
            blocked |= !limit(&rate, &key, max, seconds, gap).await?;
        }
        if sensitive && !target.is_empty() {
            blocked |= !limit(&rate, &format!("mail:{}", email(&body)?), 5, 3600, 60).await?;
        }
        if matches!(action, "login" | "passkey-login" | "reauth") && !target.is_empty() {
            blocked |= !limit(&rate, &format!("login:{}", email(&body)?), 10, 600, 0).await?;
        }
        rate.commit().await?;
        if blocked {
            return Err(Error::Limited);
        }
        let tx = auth.transaction().await?;
        let outcome = self.operation(&tx, action, bearer, body).await;
        match outcome {
            Err(Error::Unavailable) => {
                tx.rollback().await?;
                Err(Error::Unavailable)
            }
            result => {
                tx.commit().await?;
                result
            }
        }
    }

    async fn operation(
        &self,
        tx: &Transaction<'_>,
        action: &str,
        bearer: Option<&str>,
        body: Value,
    ) -> Result<Value> {
        match action {
            "register" | "recover" => {
                fields(
                    &body,
                    if action == "register" {
                        &["email", "password"]
                    } else {
                        &["email"]
                    },
                )?;
                let address = email(&body)?;
                let existing = identity(tx, &address).await?;
                let state = if action == "register" {
                    json!({"password_hash":hash(password(&body)?).await?})
                } else {
                    json!({})
                };
                let user = existing.as_ref().map(|r| r.get::<_, Uuid>("user_id"));
                let token = challenge(tx, action, user, None, &address, state, 600).await?;
                if (action == "register" && user.is_none())
                    || (action == "recover" && user.is_some())
                {
                    self.send(&address, action, Some(token), 600).await?;
                }
                Ok(json!({"status":"accepted"}))
            }
            "verify" => {
                fields(&body, &["token", "purpose", "password"])?;
                let purpose = text(&body, "purpose", 16)?;
                if !matches!(purpose, "register" | "recover" | "email") {
                    return Err(Error::Invalid);
                }
                let new_hash = if purpose == "recover" {
                    Some(hash(password(&body)?).await?)
                } else {
                    None
                };
                let row = consume(tx, text(&body, "token", 128)?, purpose, None).await?;
                let address: String = row.get("target");
                let user = if purpose == "register" {
                    // Serialize identity creation and email replacement on the same namespace.
                    identity_lock(tx, &address).await?;
                    if tx.query_opt("SELECT identity_id FROM caidex.user_identities WHERE issuer='email' AND subject=$1",&[&address]).await?.is_some() {return Err(Error::Grant);}
                    let user=tx.query_one("INSERT INTO caidex.users(user_id) VALUES(gen_random_uuid()) RETURNING user_id",&[]).await?.get::<_,Uuid>(0);
                    let id=tx.query_one("INSERT INTO caidex.user_identities(user_id,kind,issuer,subject,state,verified_at) VALUES($1,'email','email',$2,'verified',now()) RETURNING identity_id",&[&user,&address]).await?.get::<_,Uuid>(0);
                    let state: Value = row.get("state");
                    let pwd = state["password_hash"].as_str().ok_or(Error::Unavailable)?;
                    tx.execute("INSERT INTO caidex.auth_credentials(user_id,identity_id,kind,password_hash,algorithm_version) VALUES($1,$2,'password',$3,'argon2id-v19-m65536-t3-p1')",&[&user,&id,&pwd]).await?;
                    user
                } else {
                    let user: Uuid = row.get::<_, Option<Uuid>>("user_id").ok_or(Error::Grant)?;
                    lock_user(tx, user).await?;
                    if purpose == "recover" {
                        if identity(tx, &address)
                            .await?
                            .map(|r| r.get::<_, Uuid>("user_id"))
                            != Some(user)
                        {
                            return Err(Error::Grant);
                        }
                        tx.execute("UPDATE caidex.auth_credentials SET revoked_at=now() WHERE user_id=$1 AND revoked_at IS NULL",&[&user]).await?;
                        let id = identity(tx, &address)
                            .await?
                            .ok_or(Error::Grant)?
                            .get::<_, Uuid>("identity_id");
                        tx.execute("INSERT INTO caidex.auth_credentials(user_id,identity_id,kind,password_hash,algorithm_version) VALUES($1,$2,'password',$3,'argon2id-v19-m65536-t3-p1')",&[&user,&id,&new_hash]).await?;
                    } else {
                        identity_lock(tx, &address).await?;
                        if identity(tx, &address).await?.is_some() {
                            return Err(Error::Grant);
                        }
                        tx.execute("UPDATE caidex.user_identities SET subject=$2,verified_at=now() WHERE user_id=$1 AND kind='email' AND state='verified'",&[&user,&address]).await?;
                    }
                    tx.execute("UPDATE caidex.auth_sessions SET revoked_at=now() WHERE user_id=$1 AND revoked_at IS NULL",&[&user]).await?;
                    tx.execute("UPDATE caidex.auth_challenges SET consumed_at=now() WHERE user_id=$1 AND consumed_at IS NULL",&[&user]).await?;
                    self.event(
                        tx,
                        user,
                        None,
                        if purpose == "recover" {
                            "recovery"
                        } else {
                            "identity_change"
                        },
                    )
                    .await?;
                    self.send(&address, "security", None, 0).await?;
                    user
                };
                Ok(json!({"status":"verified","user_id":user}))
            }
            "login" => {
                fields(
                    &body,
                    &[
                        "email",
                        "password",
                        "platform",
                        "installation_id",
                        "display_name",
                    ],
                )?;
                let address = email(&body)?;
                let candidate = identity(tx, &address).await?;
                if let Some(ref row) = candidate {
                    let user: Uuid = row.get("user_id");
                    // Serialize password verification with credential/identity changes and revocation.
                    lock_user(tx, user).await?;
                    if identity(tx, &address)
                        .await?
                        .map(|r| r.get::<_, Uuid>("user_id"))
                        != Some(user)
                    {
                        return Err(Error::Denied);
                    }
                }
                let pwd = text(&body, "password", 1024)?.to_owned();
                let stored = if let Some(ref row) = candidate {
                    password_row(tx, row.get("user_id")).await?
                } else {
                    None
                };
                let encoded = stored
                    .as_ref()
                    .map(|r| r.get::<_, String>("password_hash"))
                    .unwrap_or_else(|| self.dummy_hash.clone());
                if !verify(pwd, encoded).await? || candidate.is_none() || stored.is_none() {
                    return Err(Error::Denied);
                }
                let user: Uuid = candidate.unwrap().get("user_id");
                self.session(tx, user, "caidex-native", &body).await
            }
            "refresh" => {
                fields(&body, &["refresh_token", "client_id", "audience", "issuer"])?;
                self.refresh(tx, &body).await
            }
            "me" | "sessions" => {
                fields(
                    &body,
                    if action == "me" {
                        &[]
                    } else {
                        &["after_session_id"]
                    },
                )?;
                let after = if body.get("after_session_id").is_some() {
                    Some(uuid(&body, "after_session_id")?)
                } else {
                    None
                };
                let session = self.actor(tx, bearer).await?;
                let user: Uuid = session.get("user_id");
                let sid: Uuid = session.get("session_id");
                let context = authorization_context(tx, sid).await?;
                let mut data = self.data.lock().await;
                let scoped = data.transaction().await?;
                scoped
                    .query_one(
                        "SELECT caidex.bind_session($1)",
                        &[&bearer.ok_or(Error::Denied)?],
                    )
                    .await?;
                let value = if action == "me" {
                    let row=scoped.query_one("SELECT user_id,display_name,memory_enabled,chat_sync_enabled,settings_version,cloud_epoch,scope_version,allowed_scopes FROM caidex.users WHERE user_id=$1",&[&user]).await?;
                    json!({"user_id":user,"session_id":sid,"issuer":self.trust.issuer(),"client_id":session.get::<_,String>("client_id"),"audience":AUDIENCE,"authorization_context":context,"display_name":row.get::<_,String>("display_name"),"memory_sync":{"enabled":row.get::<_,bool>("memory_enabled"),"settings_version":row.get::<_,i64>("settings_version"),"cloud_epoch":row.get::<_,i64>("cloud_epoch"),"scope_version":row.get::<_,i64>("scope_version")},"chat_sync_enabled":row.get::<_,bool>("chat_sync_enabled"),"allowed_scopes":row.get::<_,Value>("allowed_scopes")})
                } else {
                    if let Some(cursor) = after
                        && scoped
                            .query_opt(
                                "SELECT session_id FROM caidex.auth_sessions WHERE session_id=$1",
                                &[&cursor],
                            )
                            .await?
                            .is_none()
                    {
                        return Err(Error::Denied);
                    }
                    let rows=scoped.query("SELECT session_id,client_id,platform,display_name,installation_id,created_at::text,last_active_at::text,revoked_at::text FROM caidex.auth_sessions WHERE $1::uuid IS NULL OR (created_at,session_id)>(SELECT created_at,session_id FROM caidex.auth_sessions WHERE session_id=$1) ORDER BY created_at,session_id LIMIT 201",&[&after]).await?;
                    let next = if rows.len() > 200 {
                        Some(rows[199].get::<_, Uuid>("session_id"))
                    } else {
                        None
                    };
                    json!({"next_cursor":next,"sessions":rows.iter().take(200).map(|r|json!({"session_id":r.get::<_,Uuid>("session_id"),"current":r.get::<_,Uuid>("session_id")==sid,"client_id":r.get::<_,String>("client_id"),"platform":r.get::<_,String>("platform"),"display_name":r.get::<_,String>("display_name"),"installation_id":r.get::<_,Option<Uuid>>("installation_id"),"created_at":r.get::<_,String>("created_at"),"last_active_at":r.get::<_,String>("last_active_at"),"revoked_at":r.get::<_,Option<String>>("revoked_at")})).collect::<Vec<_>>()})
                };
                scoped.commit().await?;
                Ok(value)
            }
            "reauth" => {
                fields(&body, &["password"])?;
                let session = self.actor(tx, bearer).await?;
                let user: Uuid = session.get("user_id");
                if !limit(tx, &format!("reauth:{user}"), 10, 600, 0).await? {
                    return Err(Error::Limited);
                }
                let row = password_row(tx, user).await?.ok_or(Error::Denied)?;
                if !verify(
                    text(&body, "password", 1024)?.to_owned(),
                    row.get("password_hash"),
                )
                .await?
                {
                    return Err(Error::Denied);
                }
                let proof = challenge(
                    tx,
                    "proof",
                    Some(user),
                    Some(session.get("session_id")),
                    "",
                    json!({}),
                    120,
                )
                .await?;
                Ok(json!({"proof":proof,"expires_in":120}))
            }
            "change-email" => {
                fields(&body, &["email", "proof"])?;
                let session = self.actor(tx, bearer).await?;
                self.proof(tx, &session, &body).await?;
                let address = email(&body)?;
                if !limit(tx, &format!("mail:{address}"), 5, 3600, 60).await? {
                    return Err(Error::Limited);
                }
                let user: Uuid = session.get("user_id");
                let token = challenge(
                    tx,
                    "email",
                    Some(user),
                    Some(session.get("session_id")),
                    &address,
                    json!({}),
                    600,
                )
                .await?;
                if identity(tx, &address).await?.is_none() {
                    self.send(&address, "email", Some(token), 600).await?;
                }
                self.notify(tx, user, "email_change_requested").await?;
                Ok(json!({"status":"accepted"}))
            }
            "change-password" => {
                fields(&body, &["password", "proof"])?;
                let session = self.actor(tx, bearer).await?;
                self.proof(tx, &session, &body).await?;
                let user: Uuid = session.get("user_id");
                lock_user(tx, user).await?;
                let encoded = hash(password(&body)?).await?;
                let changed=tx.execute("UPDATE caidex.auth_credentials SET password_hash=$2,algorithm_version='argon2id-v19-m65536-t3-p1' WHERE user_id=$1 AND kind='password' AND revoked_at IS NULL",&[&user,&encoded]).await?;
                if changed == 0 {
                    tx.execute("INSERT INTO caidex.auth_credentials(user_id,kind,password_hash,algorithm_version) VALUES($1,'password',$2,'argon2id-v19-m65536-t3-p1')",&[&user,&encoded]).await?;
                }
                tx.execute(
                    "UPDATE caidex.auth_sessions SET revoked_at=now() WHERE user_id=$1",
                    &[&user],
                )
                .await?;
                self.event(tx, user, None, "identity_change").await?;
                self.notify(tx, user, "password_changed").await?;
                Ok(json!({"status":"changed","reauthentication_required":true}))
            }
            "revoke" => {
                fields(&body, &["session_id", "others", "display_name"])?;
                if body.as_object().ok_or(Error::Invalid)?.len() > 1
                    || body.get("others").is_some_and(|v| v != true)
                {
                    return Err(Error::Invalid);
                }
                let session = self.actor(tx, bearer).await?;
                let user: Uuid = session.get("user_id");
                let current: Uuid = session.get("session_id");
                if let Some(name) = body.get("display_name") {
                    let name = name
                        .as_str()
                        .filter(|s| s.len() <= 256)
                        .ok_or(Error::Invalid)?;
                    tx.execute(
                        "UPDATE caidex.auth_sessions SET display_name=$2 WHERE session_id=$1",
                        &[&current, &name],
                    )
                    .await?;
                } else if body["others"] == true {
                    tx.execute("UPDATE caidex.auth_sessions SET revoked_at=now() WHERE user_id=$1 AND session_id<>$2 AND revoked_at IS NULL",&[&user,&current]).await?;
                } else {
                    let target = if body.get("session_id").is_some() {
                        uuid(&body, "session_id")?
                    } else {
                        current
                    };
                    let n=tx.execute("UPDATE caidex.auth_sessions SET revoked_at=now() WHERE user_id=$1 AND session_id=$2",&[&user,&target]).await?;
                    if n != 1 {
                        return Err(Error::Denied);
                    }
                }
                self.event(tx, user, Some(current), "revocation").await?;
                Ok(json!({"status":"confirmed"}))
            }
            "delete-account" => {
                fields(&body, &["proof"])?;
                let session = self.actor(tx, bearer).await?;
                self.proof(tx, &session, &body).await?;
                let user: Uuid = session.get("user_id");
                lock_user(tx, user).await?;
                tx.execute(
                    "UPDATE caidex.users SET state='deleted' WHERE user_id=$1",
                    &[&user],
                )
                .await?;
                tx.execute(
                    "UPDATE caidex.auth_sessions SET revoked_at=now() WHERE user_id=$1",
                    &[&user],
                )
                .await?;
                tx.execute(
                    "UPDATE caidex.auth_credentials SET revoked_at=now() WHERE user_id=$1",
                    &[&user],
                )
                .await?;
                tx.execute(
                    "UPDATE caidex.auth_challenges SET consumed_at=now() WHERE user_id=$1",
                    &[&user],
                )
                .await?;
                self.event(tx, user, None, "deletion").await?;
                self.notify(tx, user, "account_disabled").await?;
                Ok(json!({"status":"account_disabled","data_erasure":"pending_I6_I7"}))
            }
            _ if action.starts_with("passkey-")
                || matches!(action, "credentials" | "credential-revoke") =>
            {
                self.passkeys(tx, action, bearer, &body).await
            }
            _ if matches!(action, "pkce" | "device" | "approve" | "token" | "cancel") => {
                self.grants(tx, action, bearer, &body).await
            }
            _ => Err(Error::Invalid),
        }
    }

    async fn actor(&self, tx: &Transaction<'_>, token: Option<&str>) -> Result<Row> {
        let token = token
            .filter(|s| (32..=128).contains(&s.len()))
            .ok_or(Error::Denied)?;
        let row=tx.query_opt("SELECT s.* FROM caidex.auth_sessions s JOIN caidex.users u USING(user_id) WHERE s.access_hash=$1 AND s.issuer=$2 AND s.audience='caidex-data-v1' AND s.revoked_at IS NULL AND s.access_expires_at>statement_timestamp() AND s.refresh_expires_at>statement_timestamp() AND u.state='active'",&[&digest(token),&self.trust.issuer()]).await?.ok_or(Error::Denied)?;
        lock_user(tx, row.get("user_id")).await?;
        let row=tx.query_opt("SELECT *,revoked_at IS NULL AND access_expires_at>statement_timestamp() AND refresh_expires_at>statement_timestamp() AS valid FROM caidex.auth_sessions WHERE session_id=$1 FOR UPDATE",&[&row.get::<_,Uuid>("session_id")]).await?.ok_or(Error::Denied)?;
        if !row.get::<_, bool>("valid") {
            return Err(Error::Denied);
        }
        tx.execute(
            "UPDATE caidex.auth_sessions SET last_active_at=now() WHERE session_id=$1",
            &[&row.get::<_, Uuid>("session_id")],
        )
        .await?;
        Ok(row)
    }
    async fn proof(&self, tx: &Transaction<'_>, session: &Row, body: &Value) -> Result<()> {
        let row = consume(
            tx,
            text(body, "proof", 128)?,
            "proof",
            Some(session.get("session_id")),
        )
        .await?;
        if row.get::<_, Option<Uuid>>("user_id") != Some(session.get("user_id")) {
            return Err(Error::Denied);
        }
        Ok(())
    }
    async fn session(
        &self,
        tx: &Transaction<'_>,
        user: Uuid,
        client: &str,
        body: &Value,
    ) -> Result<Value> {
        lock_user(tx, user).await?;
        let access = secret()?;
        let refresh = secret()?;
        let platform = body
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let name = body
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        if platform.len() > 128 || name.len() > 256 {
            return Err(Error::Invalid);
        }
        let installation = body
            .get("installation_id")
            .map(|_| uuid(body, "installation_id"))
            .transpose()?;
        let sid:Uuid=tx.query_one("INSERT INTO caidex.auth_sessions(user_id,access_hash,refresh_hash,client_id,audience,platform,installation_id,display_name,access_expires_at,refresh_expires_at,issuer) VALUES($1,$2,$3,$4,'caidex-data-v1',$5,$6,$7,now()+interval '10 minutes',now()+interval '30 days',$8) RETURNING session_id",&[&user,&digest(&access),&digest(&refresh),&client,&platform,&installation,&name,&self.trust.issuer()]).await?.get(0);
        self.event(tx, user, Some(sid), "login").await?;
        self.notify(tx, user, "new_session").await?;
        let context = authorization_context(tx, sid).await?;
        Ok(
            json!({"access_token":access,"refresh_token":refresh,"token_type":"Bearer","expires_in":600,"session_id":sid,"user_id":user,"issuer":self.trust.issuer(),"client_id":client,"audience":AUDIENCE,"authorization_context":context}),
        )
    }
    async fn refresh(&self, tx: &Transaction<'_>, body: &Value) -> Result<Value> {
        let token = digest(text(body, "refresh_token", 128)?);
        let client = text(body, "client_id", 128)?;
        if body["audience"] != AUDIENCE || body["issuer"] != self.trust.issuer() {
            return Err(Error::Grant);
        }
        // The UNION finds consumed hashes too. Lock serializes concurrent refresh across processes.
        let sid=tx.query_opt("SELECT session_id FROM caidex.auth_sessions WHERE refresh_hash=$1 UNION SELECT session_id FROM caidex.refresh_history WHERE token_hash=$1",&[&token]).await?.ok_or(Error::Grant)?.get::<_,Uuid>(0);
        let user: Uuid = tx
            .query_one(
                "SELECT user_id FROM caidex.auth_sessions WHERE session_id=$1",
                &[&sid],
            )
            .await?
            .get(0);
        lock_user(tx, user).await?;
        let row=tx.query_one("SELECT s.*,s.refresh_expires_at>statement_timestamp() AS valid FROM caidex.auth_sessions s WHERE session_id=$1 FOR UPDATE",&[&sid]).await?;
        if row.get::<_, String>("client_id") != client
            || row.get::<_, String>("issuer") != self.trust.issuer()
        {
            return Err(Error::Grant);
        }
        if row.get::<_, Vec<u8>>("refresh_hash") != token {
            tx.execute("UPDATE caidex.auth_sessions SET revoked_at=coalesce(revoked_at,now()) WHERE refresh_family=$1",&[&row.get::<_,Uuid>("refresh_family")]).await?;
            self.event(tx, row.get("user_id"), Some(sid), "alert")
                .await?;
            self.notify(tx, row.get("user_id"), "refresh_replay")
                .await?;
            return Err(Error::Grant);
        }
        if row
            .get::<_, Option<std::time::SystemTime>>("revoked_at")
            .is_some()
            || !row.get::<_, bool>("valid")
        {
            return Err(Error::Grant);
        }
        let user: Uuid = row.get("user_id");
        lock_user(tx, user).await?;
        let access = secret()?;
        let refresh = secret()?;
        tx.execute(
            "INSERT INTO caidex.refresh_history(token_hash,session_id) VALUES($1,$2)",
            &[&token, &sid],
        )
        .await?;
        tx.execute("UPDATE caidex.auth_sessions SET access_hash=$2,refresh_hash=$3,rotation=rotation+1,access_expires_at=now()+interval '10 minutes',refresh_expires_at=now()+interval '30 days',last_active_at=now() WHERE session_id=$1",&[&sid,&digest(&access),&digest(&refresh)]).await?;
        let context = authorization_context(tx, sid).await?;
        Ok(
            json!({"access_token":access,"refresh_token":refresh,"token_type":"Bearer","expires_in":600,"session_id":sid,"user_id":user,"issuer":self.trust.issuer(),"client_id":client,"audience":AUDIENCE,"authorization_context":context}),
        )
    }
    async fn event(
        &self,
        tx: &Transaction<'_>,
        user: Uuid,
        session: Option<Uuid>,
        kind: &str,
    ) -> Result<()> {
        tx.execute("INSERT INTO caidex.security_events(user_id,session_id,kind,outcome) VALUES($1,$2,$3,'success')",&[&user,&session,&kind]).await?;
        Ok(())
    }
    async fn send(
        &self,
        address: &str,
        purpose: &str,
        token: Option<String>,
        expires: u32,
    ) -> Result<()> {
        // Mail failure must not disclose eligibility or roll back a replay/recovery revocation.
        if !matches!(
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.sender.send(Mail {
                    id: Uuid::new_v4(),
                    recipient: address.into(),
                    purpose: purpose.into(),
                    language: "zh-CN",
                    expires_in: expires,
                    verification_token: token,
                })
            )
            .await,
            Ok(Ok(()))
        ) {
            eprintln!("Account mail delivery not confirmed");
        }
        Ok(())
    }
    async fn notify(&self, tx: &Transaction<'_>, user: Uuid, purpose: &str) -> Result<()> {
        if let Some(row)=tx.query_opt("SELECT subject FROM caidex.user_identities WHERE user_id=$1 AND kind='email' AND state='verified'",&[&user]).await? {
            self.send(&row.get::<_,String>(0),purpose,None,0).await?;
        }
        Ok(())
    }
}

async fn authorization_context(tx: &Transaction<'_>, sid: Uuid) -> Result<Option<String>> {
    Ok(tx
        .query_opt(
            "SELECT grant_hash,login_state FROM caidex.auth_grants WHERE issued_session_id=$1",
            &[&sid],
        )
        .await?
        .map(|r| {
            r.get::<_, Option<String>>("login_state")
                .unwrap_or_else(|| URL_SAFE_NO_PAD.encode(r.get::<_, Vec<u8>>("grant_hash")))
        }))
}

async fn limit(tx: &Transaction<'_>, key: &str, max: i32, seconds: i32, gap: i32) -> Result<bool> {
    let row=tx.query_one("INSERT INTO caidex.auth_limits(bucket_hash,count,reset_at) VALUES($1,1,now()+($2::int * interval '1 second')) ON CONFLICT(bucket_hash) DO UPDATE SET count=CASE WHEN caidex.auth_limits.reset_at<=now() THEN 1 ELSE caidex.auth_limits.count+1 END, reset_at=CASE WHEN caidex.auth_limits.reset_at<=now() THEN now()+($2::int * interval '1 second') ELSE caidex.auth_limits.reset_at END RETURNING count,last_at<=now()-($3::int * interval '1 second') AS elapsed",&[&digest(key),&seconds,&gap]).await?;
    let count: i32 = row.get("count");
    let elapsed: bool = row.get("elapsed");
    if count <= max && (gap == 0 || count == 1 || elapsed) {
        tx.execute(
            "UPDATE caidex.auth_limits SET last_at=now() WHERE bucket_hash=$1",
            &[&digest(key)],
        )
        .await?;
        Ok(true)
    } else {
        Ok(false)
    }
}
async fn hash(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let mut salt = [0; 16];
        getrandom::fill(&mut salt).map_err(|_| Error::Unavailable)?;
        let salt = SaltString::encode_b64(&salt).map_err(|_| Error::Unavailable)?;
        let params = Params::new(65536, 3, 1, None).map_err(|_| Error::Unavailable)?;
        Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
            .hash_password(password.as_bytes(), &salt)
            .map(|v| v.to_string())
            .map_err(|_| Error::Unavailable)
    })
    .await
    .map_err(|_| Error::Unavailable)?
}
async fn verify(password: String, encoded: String) -> Result<bool> {
    tokio::task::spawn_blocking(move || {
        let hash = PasswordHash::new(&encoded).map_err(|_| Error::Unavailable)?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok())
    })
    .await
    .map_err(|_| Error::Unavailable)?
}
async fn identity(tx: &Transaction<'_>, address: &str) -> Result<Option<Row>> {
    Ok(tx.query_opt("SELECT i.* FROM caidex.user_identities i JOIN caidex.users u USING(user_id) WHERE i.kind='email' AND i.issuer='email' AND i.subject=$1 AND i.state='verified' AND u.state='active'",&[&address]).await?)
}
async fn password_row(tx: &Transaction<'_>, user: Uuid) -> Result<Option<Row>> {
    Ok(tx.query_opt("SELECT password_hash FROM caidex.auth_credentials WHERE user_id=$1 AND kind='password' AND revoked_at IS NULL",&[&user]).await?)
}
async fn identity_lock(tx: &Transaction<'_>, address: &str) -> Result<()> {
    tx.query_one(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        &[&format!("email:{address}")],
    )
    .await?;
    Ok(())
}
async fn lock_user(tx: &Transaction<'_>, user: Uuid) -> Result<()> {
    if tx
        .query_opt(
            "SELECT user_id FROM caidex.users WHERE user_id=$1 AND state='active' FOR UPDATE",
            &[&user],
        )
        .await?
        .is_none()
    {
        return Err(Error::Denied);
    }
    Ok(())
}
async fn challenge(
    tx: &Transaction<'_>,
    purpose: &str,
    user: Option<Uuid>,
    session: Option<Uuid>,
    target: &str,
    state: Value,
    seconds: i32,
) -> Result<String> {
    if let Some(user) = user {
        lock_user(tx, user).await?;
    }
    if matches!(purpose, "register" | "recover") {
        identity_lock(tx, target).await?;
        tx.execute("UPDATE caidex.auth_challenges SET consumed_at=now() WHERE purpose=$1 AND target=$2 AND consumed_at IS NULL",&[&purpose,&target]).await?;
    } else if purpose == "email" {
        tx.execute("UPDATE caidex.auth_challenges SET consumed_at=now() WHERE purpose='email' AND user_id=$1 AND consumed_at IS NULL",&[&user]).await?;
    }
    let token = secret()?;
    tx.execute("INSERT INTO caidex.auth_challenges(challenge_hash,purpose,user_id,session_id,target,state,expires_at) VALUES($1,$2,$3,$4,$5,$6,now()+($7::int*interval '1 second'))",&[&digest(&token),&purpose,&user,&session,&target,&state,&seconds]).await?;
    Ok(token)
}
async fn consume(
    tx: &Transaction<'_>,
    token: &str,
    purpose: &str,
    session: Option<Uuid>,
) -> Result<Row> {
    let pending = tx
        .query_opt(
            "SELECT user_id,target,purpose FROM caidex.auth_challenges WHERE challenge_hash=$1",
            &[&digest(token)],
        )
        .await?
        .ok_or(Error::Grant)?;
    if let Some(user) = pending.get::<_, Option<Uuid>>("user_id") {
        lock_user(tx, user).await?;
    }
    if matches!(
        pending.get::<_, String>("purpose").as_str(),
        "register" | "recover" | "email"
    ) {
        identity_lock(tx, &pending.get::<_, String>("target")).await?;
    }
    let row=tx.query_opt("SELECT *,expires_at>statement_timestamp() AS valid FROM caidex.auth_challenges WHERE challenge_hash=$1 FOR UPDATE",&[&digest(token)]).await?.ok_or(Error::Grant)?;
    if row.get::<_, String>("purpose") != purpose
        || (session.is_some() && row.get::<_, Option<Uuid>>("session_id") != session)
        || !row.get::<_, bool>("valid")
        || row.get::<_, i32>("attempts") >= 5
        || row
            .get::<_, Option<std::time::SystemTime>>("consumed_at")
            .is_some()
    {
        tx.execute("UPDATE caidex.auth_challenges SET attempts=least(5,attempts+1) WHERE challenge_hash=$1",&[&digest(token)]).await?;
        return Err(Error::Grant);
    }
    tx.execute(
        "UPDATE caidex.auth_challenges SET consumed_at=now() WHERE challenge_hash=$1",
        &[&digest(token)],
    )
    .await?;
    Ok(row)
}

pub fn router<S: EmailSender>(service: Arc<Service<S>>) -> Router {
    Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(discovery::<S>),
        )
        .route("/v1/auth/{action}", post(request::<S>))
        .route("/authorize", get(login_page))
        .route("/device", get(login_page))
        .route("/account.js", get(account_script))
        .layer(DefaultBodyLimit::max(32768))
        .with_state(service)
}
async fn discovery<S: EmailSender>(State(service): State<Arc<Service<S>>>) -> impl IntoResponse {
    Json(service.metadata())
}
async fn request<S: EmailSender>(
    State(service): State<Arc<Service<S>>>,
    Path(action): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response> {
    if let Some(origin) = headers.get("origin") {
        let expected = service.trust.issuer().trim_end_matches('/');
        if origin.to_str().ok() != Some(expected) {
            return Err(Error::Denied);
        }
    }
    let body: Value = match headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
    {
        Some("application/json") => serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?,
        Some("application/x-www-form-urlencoded")
            if matches!(action.as_str(), "token" | "device") =>
        {
            let pairs: Vec<(String, String)> =
                serde_urlencoded::from_bytes(&bytes).map_err(|_| Error::Invalid)?;
            let mut object = serde_json::Map::new();
            for (k, v) in pairs {
                if object.insert(k, Value::String(v)).is_some() {
                    return Err(Error::Invalid);
                }
            }
            Value::Object(object)
        }
        _ => return Err(Error::Invalid),
    };
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let result = service
        .request(&action, token, &peer.ip().to_string(), body)
        .await?;
    Ok((
        [("Cache-Control", "no-store"), ("Pragma", "no-cache")],
        Json(result),
    )
        .into_response())
}
async fn login_page() -> impl IntoResponse {
    (
        [
            (
                "Content-Security-Policy",
                "default-src 'none'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
            ),
            ("Cache-Control", "no-store"),
        ],
        Html(
            "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>CAIdex account authorization</title><h1>CAIdex CLI</h1><p>Approve only a CLI request you started. Account authorization grants no Host or model credential access.</p><p>Local I-2 protocol service; production login UI is not deployed.</p><label>Email <input id=\"email\" type=\"email\" autocomplete=\"username\"></label><label>Password <input id=\"password\" type=\"password\" autocomplete=\"current-password\"></label><button id=\"login\">Sign in</button><button id=\"passkey\">Use passkey</button><p><label>Device user code <input id=\"code\" maxlength=\"10\" autocomplete=\"off\"></label></p><button id=\"approve\" disabled>Approve my CLI request</button><button id=\"deny\" disabled>Deny</button><p id=\"status\" role=\"status\" aria-live=\"polite\"></p><script src=\"/account.js\" defer></script></html>",
        ),
    )
}
async fn account_script() -> impl IntoResponse {
    (
        [
            ("Content-Type", "text/javascript; charset=utf-8"),
            ("Cache-Control", "no-store"),
        ],
        include_str!("account.js"),
    )
}
