use super::*;

impl<S: EmailSender> Service<S> {
    pub(super) async fn passkeys(
        &self,
        tx: &Transaction<'_>,
        action: &str,
        bearer: Option<&str>,
        body: &Value,
    ) -> Result<Value> {
        match action {
            "passkey-register" => {
                fields(body, &["proof"])?;
                let session = self.actor(tx, bearer).await?;
                self.proof(tx, &session, body).await?;
                let user: Uuid = session.get("user_id");
                lock_user(tx, user).await?;
                let address=tx.query_one("SELECT subject FROM caidex.user_identities WHERE user_id=$1 AND kind='email' AND state='verified'",&[&user]).await?.get::<_,String>(0);
                let keys = keys(tx, user).await?;
                let exclude = keys.iter().map(|(_, key)| key.cred_id().clone()).collect();
                let (options, state) = self
                    .webauthn
                    .start_passkey_registration(user, &address, &address, Some(exclude))
                    .map_err(|_| Error::Invalid)?;
                let request = challenge(
                    tx,
                    "passkey_register",
                    Some(user),
                    Some(session.get("session_id")),
                    "",
                    json_encode(state)?,
                    120,
                )
                .await?;
                Ok(json!({"request":request,"options":options,"expires_in":120}))
            }
            "passkey-register-finish" => {
                fields(body, &["request", "credential"])?;
                let session = self.actor(tx, bearer).await?;
                let user: Uuid = session.get("user_id");
                lock_user(tx, user).await?;
                let row = consume(
                    tx,
                    text(body, "request", 128)?,
                    "passkey_register",
                    Some(session.get("session_id")),
                )
                .await?;
                if row.get::<_, Option<Uuid>>("user_id") != Some(user) {
                    return Err(Error::Denied);
                }
                let state: PasskeyRegistration = json_decode(row.get("state"))?;
                let credential: RegisterPublicKeyCredential =
                    json_decode(body["credential"].clone())?;
                let passkey = self
                    .webauthn
                    .finish_passkey_registration(&credential, &state)
                    .map_err(|_| Error::Grant)?;
                let id: Vec<u8> = passkey.cred_id().as_ref().to_vec();
                // Duplicate device credential is a refusal, not a DB error carrying private metadata.
                if tx
                    .query_opt(
                        "SELECT credential_id FROM caidex.auth_credentials WHERE passkey_id=$1",
                        &[&id],
                    )
                    .await?
                    .is_some()
                {
                    return Err(Error::Grant);
                }
                let public =
                    serde_json::to_vec(passkey.get_public_key()).map_err(|_| Error::Unavailable)?;
                let encoded = json_encode(&passkey)?;
                let cid:Uuid=tx.query_one("INSERT INTO caidex.auth_credentials(user_id,kind,passkey_id,public_key,rp_id,algorithm_version,passkey_state) VALUES($1,'passkey',$2,$3,$4,'webauthn-rs-0.5.5',$5) RETURNING credential_id",&[&user,&id,&public,&self.rp_id,&encoded]).await?.get(0);
                self.event(tx, user, None, "identity_change").await?;
                self.notify(tx, user, "passkey_added").await?;
                Ok(json!({"status":"registered","credential_id":cid}))
            }
            "passkey-login" | "passkey-proof" => {
                fields(
                    body,
                    if action == "passkey-login" {
                        &["email"]
                    } else {
                        &[]
                    },
                )?;
                let session = if action == "passkey-proof" {
                    Some(self.actor(tx, bearer).await?)
                } else {
                    None
                };
                let user = if let Some(ref session) = session {
                    session.get("user_id")
                } else {
                    identity(tx, &email(body)?)
                        .await?
                        .ok_or(Error::Denied)?
                        .get("user_id")
                };
                let keys = keys(tx, user).await?;
                if keys.is_empty() {
                    return Err(Error::Denied);
                }
                let (options, state) = self
                    .webauthn
                    .start_passkey_authentication(
                        &keys.into_iter().map(|(_, key)| key).collect::<Vec<_>>(),
                    )
                    .map_err(|_| Error::Denied)?;
                let purpose = if action == "passkey-proof" {
                    "passkey_proof"
                } else {
                    "passkey_login"
                };
                let request = challenge(
                    tx,
                    purpose,
                    Some(user),
                    session.map(|s| s.get("session_id")),
                    "",
                    json_encode(state)?,
                    120,
                )
                .await?;
                Ok(json!({"request":request,"options":options,"expires_in":120}))
            }
            "passkey-login-finish" | "passkey-proof-finish" => {
                fields(
                    body,
                    &[
                        "request",
                        "credential",
                        "platform",
                        "installation_id",
                        "display_name",
                    ],
                )?;
                let session = if action == "passkey-proof-finish" {
                    Some(self.actor(tx, bearer).await?)
                } else {
                    None
                };
                let row = consume(
                    tx,
                    text(body, "request", 128)?,
                    if session.is_some() {
                        "passkey_proof"
                    } else {
                        "passkey_login"
                    },
                    session.as_ref().map(|s| s.get("session_id")),
                )
                .await?;
                let user = row.get::<_, Option<Uuid>>("user_id").ok_or(Error::Grant)?;
                lock_user(tx, user).await?;
                if session
                    .as_ref()
                    .is_some_and(|s| s.get::<_, Uuid>("user_id") != user)
                {
                    return Err(Error::Denied);
                }
                let state: PasskeyAuthentication = json_decode(row.get("state"))?;
                let credential: PublicKeyCredential = json_decode(body["credential"].clone())?;
                let result = self
                    .webauthn
                    .finish_passkey_authentication(&credential, &state)
                    .map_err(|_| Error::Grant)?;
                if !result.user_verified() {
                    return Err(Error::Grant);
                }
                let mut active = keys(tx, user).await?;
                let (id, key) = active
                    .iter_mut()
                    .find(|(_, key)| key.cred_id() == result.cred_id())
                    .ok_or(Error::Grant)?;
                key.update_credential(&result).ok_or(Error::Grant)?;
                tx.execute("UPDATE caidex.auth_credentials SET passkey_state=$2 WHERE credential_id=$1 AND revoked_at IS NULL",&[&*id,&json_encode(&*key)?]).await?;
                if let Some(session) = session {
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
                } else {
                    self.session(tx, user, "caidex-native", body).await
                }
            }
            "credentials" => {
                fields(body, &[])?;
                let session = self.actor(tx, bearer).await?;
                let rows=tx.query("SELECT credential_id,kind,rp_id FROM caidex.auth_credentials WHERE user_id=$1 AND revoked_at IS NULL ORDER BY credential_id",&[&session.get::<_,Uuid>("user_id")]).await?;
                Ok(
                    json!({"credentials":rows.iter().map(|r|json!({"credential_id":r.get::<_,Uuid>("credential_id"),"kind":r.get::<_,String>("kind"),"rp_id":r.get::<_,Option<String>>("rp_id")})).collect::<Vec<_>>() }),
                )
            }
            "credential-revoke" => {
                fields(body, &["proof", "credential_id"])?;
                let session = self.actor(tx, bearer).await?;
                let user: Uuid = session.get("user_id");
                lock_user(tx, user).await?;
                self.proof(tx, &session, body).await?;
                let count:i64=tx.query_one("SELECT count(*) FROM caidex.auth_credentials WHERE user_id=$1 AND revoked_at IS NULL",&[&user]).await?.get(0);
                if count <= 1 {
                    return Err(Error::Denied);
                }
                let cid = uuid(body, "credential_id")?;
                if tx.execute("UPDATE caidex.auth_credentials SET revoked_at=now() WHERE user_id=$1 AND credential_id=$2 AND revoked_at IS NULL",&[&user,&cid]).await?!=1 {return Err(Error::Denied);}
                self.event(tx, user, None, "identity_change").await?;
                self.notify(tx, user, "credential_revoked").await?;
                Ok(json!({"status":"revoked"}))
            }
            _ => Err(Error::Invalid),
        }
    }
}
async fn keys(tx: &Transaction<'_>, user: Uuid) -> Result<Vec<(Uuid, Passkey)>> {
    let rows=tx.query("SELECT credential_id,passkey_state FROM caidex.auth_credentials WHERE user_id=$1 AND kind='passkey' AND revoked_at IS NULL ORDER BY credential_id",&[&user]).await?;
    rows.into_iter()
        .map(|r| Ok((r.get("credential_id"), json_decode(r.get("passkey_state"))?)))
        .collect()
}
