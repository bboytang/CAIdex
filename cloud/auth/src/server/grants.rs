use super::*;
use crate::loopback_redirect;
use hmac::{Hmac, Mac};
use oauth2::{PkceCodeChallenge, PkceCodeVerifier};
use subtle::ConstantTimeEq;

impl<S: EmailSender> Service<S> {
    fn user_code_hash(&self, code: &str) -> Vec<u8> {
        // Short user codes require a server secret against offline DB enumeration.
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.code_key).expect("32-byte HMAC key");
        mac.update(code.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }
    fn public_client(&self, body: &Value) -> Result<()> {
        if body["client_id"] != CLI_CLIENT
            || body["audience"] != AUDIENCE
            || body["issuer"] != self.trust.issuer()
        {
            return Err(Error::Grant);
        }
        Ok(())
    }
    pub(super) async fn grants(
        &self,
        tx: &Transaction<'_>,
        action: &str,
        bearer: Option<&str>,
        body: &Value,
    ) -> Result<Value> {
        match action {
            "pkce" | "device" => {
                fields(
                    body,
                    if action == "pkce" {
                        &[
                            "client_id",
                            "audience",
                            "issuer",
                            "redirect_uri",
                            "code_challenge",
                            "code_challenge_method",
                            "state",
                        ]
                    } else {
                        &["client_id", "audience", "issuer"]
                    },
                )?;
                self.public_client(body)?;
                let (redirect, challenge, state) = if action == "pkce" {
                    let redirect = text(body, "redirect_uri", 256)?;
                    loopback_redirect(redirect).map_err(|_| Error::Invalid)?;
                    let challenge = text(body, "code_challenge", 43)?;
                    if body["code_challenge_method"] != "S256"
                        || URL_SAFE_NO_PAD
                            .decode(challenge)
                            .map_err(|_| Error::Invalid)?
                            .len()
                            != 32
                    {
                        return Err(Error::Invalid);
                    }
                    let state = text(body, "state", 128)?;
                    if state.len() < 32 {
                        return Err(Error::Invalid);
                    }
                    (Some(redirect), Some(challenge), Some(state))
                } else {
                    (None, None, None)
                };
                let code = secret()?;
                let cancel = secret()?;
                let mut bytes = [0; 10];
                getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
                let alphabet = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
                let user_code: String = bytes
                    .iter()
                    .map(|b| alphabet[(b & 31) as usize] as char)
                    .collect();
                let user_hash = if action == "device" {
                    Some(self.user_code_hash(&user_code))
                } else {
                    None
                };
                tx.execute("INSERT INTO caidex.auth_grants(grant_hash,cancel_hash,user_code_hash,kind,client_id,audience,issuer,redirect_uri,pkce_challenge,login_state,expires_at) VALUES($1,$2,$3,$4,'caidex-cli','caidex-data-v1',$5,$6,$7,$8,now()+interval '10 minutes')",&[&digest(&code),&digest(&cancel),&user_hash,&action,&self.trust.issuer(),&redirect,&challenge,&state]).await?;
                if action == "device" {
                    Ok(
                        json!({"device_code":code,"cancel_secret":cancel,"user_code":user_code,"verification_uri":self.trust.endpoint("device"),"expires_in":600,"interval":5}),
                    )
                } else {
                    let mut uri = Url::parse(&self.trust.endpoint("authorize"))
                        .map_err(|_| Error::Unavailable)?;
                    uri.query_pairs_mut().append_pair("request", &code);
                    Ok(
                        json!({"request":code,"cancel_secret":cancel,"authorization_uri":uri.as_str(),"expires_in":600}),
                    )
                }
            }
            "approve" => {
                fields(body, &["request", "user_code", "approve"])?;
                if body.get("request").is_some() == body.get("user_code").is_some() {
                    return Err(Error::Invalid);
                }
                let approve = body["approve"].as_bool().ok_or(Error::Invalid)?;
                let row = if body.get("request").is_some() {
                    tx.query_opt("SELECT *,expires_at>statement_timestamp() AS valid FROM caidex.auth_grants WHERE grant_hash=$1 AND kind='pkce'",&[&digest(text(body,"request",128)?)]).await?
                } else {
                    let code=text(body,"user_code",10)?;
                    if code.len()!=10 || !code.bytes().all(|b|b.is_ascii_uppercase()||b.is_ascii_digit()) {return Err(Error::Invalid);}
                    tx.query_opt("SELECT *,expires_at>statement_timestamp() AS valid FROM caidex.auth_grants WHERE user_code_hash=$1 AND kind='device'",&[&self.user_code_hash(code)]).await?
                }.ok_or(Error::Grant)?;
                if row.get::<_, String>("status") != "pending" {
                    return Err(Error::Grant);
                }
                let session = self.actor(tx, bearer).await?;
                let user: Uuid = session.get("user_id");
                let row=tx.query_one("SELECT *,expires_at>statement_timestamp() AS valid FROM caidex.auth_grants WHERE grant_hash=$1 FOR UPDATE",&[&row.get::<_,Vec<u8>>("grant_hash")]).await?;
                if !row.get::<_, bool>("valid") {
                    return Err(Error::Expired);
                }
                if row.get::<_, String>("status") != "pending" {
                    return Err(Error::Grant);
                }
                tx.execute("UPDATE caidex.auth_grants SET status=$2,user_id=$3,source_session_id=$4 WHERE grant_hash=$1",&[&row.get::<_,Vec<u8>>("grant_hash"),&if approve {"approved"} else {"denied"},&user,&session.get::<_,Uuid>("session_id")]).await?;
                if let Some(redirect) = row.get::<_, Option<String>>("redirect_uri") {
                    let mut uri = Url::parse(&redirect).map_err(|_| Error::Unavailable)?;
                    uri.query_pairs_mut()
                        .append_pair("state", &row.get::<_, String>("login_state"));
                    if approve {
                        uri.query_pairs_mut()
                            .append_pair("code", text(body, "request", 128)?);
                    } else {
                        uri.query_pairs_mut().append_pair("error", "access_denied");
                    }
                    Ok(
                        json!({"status":if approve {"approved"} else {"denied"},"redirect_uri":uri.as_str()}),
                    )
                } else {
                    Ok(json!({"status":if approve {"approved"} else {"denied"}}))
                }
            }
            "token" => {
                fields(
                    body,
                    &[
                        "grant_type",
                        "client_id",
                        "audience",
                        "issuer",
                        "code",
                        "code_verifier",
                        "redirect_uri",
                        "device_code",
                        "refresh_token",
                    ],
                )?;
                self.public_client(body)?;
                let kind = match text(body, "grant_type", 64)? {
                    "authorization_code" => "pkce",
                    "urn:ietf:params:oauth:grant-type:device_code" => "device",
                    "refresh_token" => return self.refresh(tx, body).await,
                    _ => return Err(Error::Invalid),
                };
                let code = text(
                    body,
                    if kind == "pkce" {
                        "code"
                    } else {
                        "device_code"
                    },
                    128,
                )?;
                let peek = tx
                    .query_opt(
                        "SELECT user_id,status FROM caidex.auth_grants WHERE grant_hash=$1",
                        &[&digest(code)],
                    )
                    .await?
                    .ok_or(Error::Grant)?;
                let owner = peek.get::<_, Option<Uuid>>("user_id");
                let owner_locked = peek.get::<_, String>("status") == "approved";
                if owner_locked {
                    lock_user(tx, owner.ok_or(Error::Grant)?).await?;
                }
                let row=tx.query_opt("SELECT *,expires_at>statement_timestamp() AS valid,next_poll_at<=statement_timestamp() AS poll_allowed FROM caidex.auth_grants WHERE grant_hash=$1 FOR UPDATE",&[&digest(code)]).await?.ok_or(Error::Grant)?;
                if row.get::<_, String>("kind") != kind
                    || row.get::<_, String>("client_id") != CLI_CLIENT
                    || row.get::<_, String>("issuer") != self.trust.issuer()
                {
                    return Err(Error::Grant);
                }
                if !row.get::<_, bool>("valid") {
                    return Err(Error::Expired);
                }
                let status: String = row.get("status");
                if matches!(status.as_str(), "denied" | "cancelled") {
                    return Err(Error::Denied);
                }
                if status == "consumed" {
                    return Err(Error::Grant);
                }
                if kind == "pkce" {
                    let verifier = text(body, "code_verifier", 128)?;
                    if verifier.len() < 43
                        || !verifier
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"-._~".contains(&c))
                    {
                        return Err(Error::Grant);
                    }
                    let computed = PkceCodeChallenge::from_code_verifier_sha256(
                        &PkceCodeVerifier::new(verifier.into()),
                    );
                    let expected: String = row.get("pkce_challenge");
                    if computed
                        .as_str()
                        .as_bytes()
                        .ct_eq(expected.as_bytes())
                        .unwrap_u8()
                        != 1
                        || body["redirect_uri"].as_str()
                            != row.get::<_, Option<String>>("redirect_uri").as_deref()
                    {
                        return Err(Error::Grant);
                    }
                } else {
                    let interval: i32 = row.get("interval_seconds");
                    if !row.get::<_, bool>("poll_allowed") {
                        tx.execute("UPDATE caidex.auth_grants SET interval_seconds=least(60,interval_seconds+5),next_poll_at=now()+((least(60,interval_seconds+5)) * interval '1 second') WHERE grant_hash=$1",&[&digest(code)]).await?;
                        return Err(Error::Slow);
                    }
                    tx.execute("UPDATE caidex.auth_grants SET next_poll_at=now()+($2::int * interval '1 second') WHERE grant_hash=$1",&[&digest(code),&interval]).await?;
                }
                if status == "approved" && !owner_locked {
                    return Err(Error::Pending);
                }
                if status != "approved" {
                    return Err(Error::Pending);
                }
                let user: Uuid = row.get::<_, Option<Uuid>>("user_id").ok_or(Error::Grant)?;
                let source: Uuid = row
                    .get::<_, Option<Uuid>>("source_session_id")
                    .ok_or(Error::Grant)?;
                if tx.query_opt("SELECT session_id FROM caidex.auth_sessions WHERE session_id=$1 AND user_id=$2 AND issuer=$3 AND revoked_at IS NULL AND access_expires_at>statement_timestamp() AND refresh_expires_at>statement_timestamp() FOR UPDATE",&[&source,&user,&self.trust.issuer()]).await?.is_none() {return Err(Error::Grant);}
                let mut token = self
                    .session(tx, user, CLI_CLIENT, &json!({"platform":"CLI"}))
                    .await?;
                token["authorization_context"] = row
                    .get::<_, Option<String>>("login_state")
                    .unwrap_or_else(|| URL_SAFE_NO_PAD.encode(row.get::<_, Vec<u8>>("grant_hash")))
                    .into();
                let sid = uuid(&token, "session_id")?;
                tx.execute("UPDATE caidex.auth_grants SET status='consumed',issued_session_id=$2 WHERE grant_hash=$1",&[&digest(code),&sid]).await?;
                Ok(token)
            }
            "cancel" => {
                fields(body, &["cancel_secret"])?;
                let cancel_hash = digest(text(body, "cancel_secret", 128)?);
                if let Some(user) = tx
                    .query_opt(
                        "SELECT user_id FROM caidex.auth_grants WHERE cancel_hash=$1",
                        &[&cancel_hash],
                    )
                    .await?
                    .ok_or(Error::Grant)?
                    .get::<_, Option<Uuid>>(0)
                {
                    tx.query_one(
                        "SELECT user_id FROM caidex.users WHERE user_id=$1 FOR UPDATE",
                        &[&user],
                    )
                    .await?;
                }
                let row = tx
                    .query_opt(
                        "SELECT * FROM caidex.auth_grants WHERE cancel_hash=$1 FOR UPDATE",
                        &[&digest(text(body, "cancel_secret", 128)?)],
                    )
                    .await?
                    .ok_or(Error::Grant)?;
                tx.execute(
                    "UPDATE caidex.auth_grants SET status='cancelled' WHERE grant_hash=$1",
                    &[&row.get::<_, Vec<u8>>("grant_hash")],
                )
                .await?;
                if let Some(sid) = row.get::<_, Option<Uuid>>("issued_session_id") {
                    tx.execute("UPDATE caidex.auth_sessions SET revoked_at=coalesce(revoked_at,now()) WHERE session_id=$1",&[&sid]).await?;
                    self.event(
                        tx,
                        row.get::<_, Option<Uuid>>("user_id")
                            .ok_or(Error::Unavailable)?,
                        Some(sid),
                        "revocation",
                    )
                    .await?;
                }
                Ok(json!({"status":"confirmed"}))
            }
            _ => Err(Error::Invalid),
        }
    }
}
