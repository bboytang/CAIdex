use crate::{Refused, Trust};
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Confirmation must come from the authenticated /me response at the trusted issuer.
/// Opaque token text is never identity evidence. This type intentionally has no Debug/Serialize.
pub struct ConfirmedSession {
    tokens: Tokens,
    test: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tokens {
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: u32,
    user_id: String,
    session_id: String,
    issuer: String,
    client_id: String,
    audience: String,
    #[serde(default)]
    authorization_context: Option<String>,
}
impl Drop for Tokens {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}
impl ConfirmedSession {
    pub fn confirm(
        trust: &Trust,
        client: &str,
        token_reply: serde_json::Value,
        online_me: &serde_json::Value,
    ) -> Result<Self, Refused> {
        trust.verify_session(&token_reply, client)?;
        trust.verify_session(online_me, client)?;
        if token_reply["user_id"] != online_me["user_id"]
            || token_reply["session_id"] != online_me["session_id"]
            || token_reply["authorization_context"] != online_me["authorization_context"]
        {
            return Err(Refused);
        }
        let tokens: Tokens = serde_json::from_value(token_reply).map_err(|_| Refused)?;
        validate_tokens(&tokens)?;
        Ok(Self {
            tokens,
            test: trust.is_test(),
        })
    }
    /// Read-only guard for local data boundaries; opaque credentials never become source identity.
    pub fn credential_guard(&self) -> std::sync::Arc<caidex_credentials::Redactor> {
        let guard = std::sync::Arc::new(caidex_credentials::Redactor::default());
        for token in [&self.tokens.access_token, &self.tokens.refresh_token] {
            guard
                .register(&Secret::new(token.clone()).expect("validated account token"))
                .expect("new guard mutex");
        }
        guard
    }
    pub fn user_id(&self) -> &str {
        &self.tokens.user_id
    }
    pub fn issuer(&self) -> &str {
        &self.tokens.issuer
    }
    pub fn is_test(&self) -> bool {
        self.test
    }
    pub fn session_id(&self) -> &str {
        &self.tokens.session_id
    }
    pub(crate) fn context(&self) -> Option<&str> {
        self.tokens.authorization_context.as_deref()
    }
    pub(crate) fn client(&self) -> &str {
        &self.tokens.client_id
    }
}
fn validate_tokens(tokens: &Tokens) -> Result<(), Refused> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    if tokens.token_type != "Bearer"
        || !(1..=600).contains(&tokens.expires_in)
        || [&tokens.access_token, &tokens.refresh_token]
            .iter()
            .any(|t| {
                t.len() != 43
                    || URL_SAFE_NO_PAD
                        .decode(t.as_bytes())
                        .map_or(true, |bytes| bytes.len() != 32)
            })
    {
        return Err(Refused);
    }
    Ok(())
}

/// One protected entry contains the pair: no half access/refresh write or plaintext fallback.
/// Reopening an entry gives SavedUnverified; it never resumes login or sync by itself.
pub struct AccountStore<S: SecretStore> {
    broker: Broker<S>,
    owner: Id,
}
impl<S: SecretStore> AccountStore<S> {
    pub fn new(installation_id: uuid::Uuid, store: S) -> Self {
        let owner = Id::new(format!("account-{installation_id}")).expect("UUID namespace");
        Self {
            broker: Broker::new(owner.clone(), store),
            owner,
        }
    }
    fn reference(&self, test: bool, issuer: &str, client: &str, user: &str) -> CredentialRef {
        let key = format!("{issuer}\0{client}\0{user}");
        let hash = Sha256::digest(key.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        // The store's owner is an installation namespace, not user_id or execution CredentialRef.owner.
        CredentialRef {
            owner: self.owner.clone(),
            provider: Id::new(if test { "account-test" } else { "account" }).unwrap(),
            profile: Id::new(hash).unwrap(),
            kind: SecretKind::AccessToken,
        }
    }
    pub fn save(&self, session: &ConfirmedSession) -> Result<(), Refused> {
        let reference = self.reference(
            session.test,
            &session.tokens.issuer,
            &session.tokens.client_id,
            &session.tokens.user_id,
        );
        let value = serde_json::to_string(&session.tokens).map_err(|_| Refused)?;
        let secret = Secret::new(value).map_err(|_| Refused)?;
        self.broker.set(&reference, secret).map_err(|_| Refused)?;
        let readback = self
            .broker
            .resolve(&reference)
            .map_err(|_| Refused)?
            .ok_or(Refused)?;
        let tokens: Tokens = serde_json::from_str(readback.expose()).map_err(|_| Refused)?;
        if tokens.access_token != session.tokens.access_token
            || tokens.refresh_token != session.tokens.refresh_token
        {
            return Err(Refused);
        }
        Ok(())
    }
    pub fn saved(
        &self,
        trust: &Trust,
        client: &str,
        user: &str,
    ) -> Result<Option<SavedSession>, Refused> {
        let reference = self.reference(trust.is_test(), trust.issuer(), client, user);
        let Some(secret) = self.broker.resolve(&reference).map_err(|_| Refused)? else {
            return Ok(None);
        };
        let tokens: Tokens = serde_json::from_str(secret.expose()).map_err(|_| Refused)?;
        validate_tokens(&tokens)?;
        let value = serde_json::to_value(&tokens).map_err(|_| Refused)?;
        trust.verify_session(&value, client)?;
        if tokens.user_id != user {
            return Err(Refused);
        }
        Ok(Some(SavedSession { tokens }))
    }
    pub fn remove(&self, trust: &Trust, client: &str, user: &str) -> Result<bool, Refused> {
        self.broker
            .remove(&self.reference(trust.is_test(), trust.issuer(), client, user))
            .map_err(|_| Refused)
    }
}
/// A stored token can be used for a trusted verification request; this is not an online login state.
pub struct SavedSession {
    tokens: Tokens,
}
impl SavedSession {
    pub fn status(&self) -> &'static str {
        "saved_unverified"
    }
    pub fn session_id(&self) -> &str {
        &self.tokens.session_id
    }
    pub fn access_token(&self) -> Secret {
        Secret::new(self.tokens.access_token.clone()).expect("validated account token")
    }
    pub fn refresh_token(&self) -> Secret {
        Secret::new(self.tokens.refresh_token.clone()).expect("validated account token")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<BTreeMap<String, String>>>, bool);
    impl SecretStore for Memory {
        fn get(&self, r: &CredentialRef) -> caidex_credentials::Result<Option<Secret>> {
            self.0
                .lock()
                .unwrap()
                .get(&serde_json::to_string(r).unwrap())
                .map(|s| Secret::new(s.clone()))
                .transpose()
        }
        fn set(&self, r: &CredentialRef, v: &Secret) -> caidex_credentials::Result<()> {
            if self.1 {
                return Err(caidex_credentials::Error::BackendUnavailable);
            }
            self.0
                .lock()
                .unwrap()
                .insert(serde_json::to_string(r).unwrap(), v.expose().into());
            Ok(())
        }
        fn remove(&self, r: &CredentialRef) -> caidex_credentials::Result<bool> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .remove(&serde_json::to_string(r).unwrap())
                .is_some())
        }
    }
    fn fixture(trust: &Trust) -> (serde_json::Value, serde_json::Value) {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let value = serde_json::json!({"access_token":URL_SAFE_NO_PAD.encode([1;32]),"refresh_token":URL_SAFE_NO_PAD.encode([2;32]),"token_type":"Bearer","expires_in":600,"issuer":trust.issuer(),"client_id":crate::CLI_CLIENT,"audience":crate::AUDIENCE,"user_id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa","session_id":"bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb","authorization_context":"synthetic-context-of-thirty-two-bytes"});
        (value.clone(), value)
    }
    #[test]
    fn pair_storage_is_separate_and_reopening_is_unverified() {
        let trust = Trust::local_test("http://localhost:3333/").unwrap();
        let (reply, online) = fixture(&trust);
        let confirmed =
            ConfirmedSession::confirm(&trust, crate::CLI_CLIENT, reply, &online).unwrap();
        let memory = Memory::default();
        let installation = uuid::Uuid::new_v4();
        let store = AccountStore::new(installation, memory.clone());
        store.save(&confirmed).unwrap();
        let saved = store
            .saved(
                &trust,
                crate::CLI_CLIENT,
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            )
            .unwrap()
            .unwrap();
        assert_eq!(saved.status(), "saved_unverified");
        assert!(format!("{:?}", saved.access_token()).contains("REDACTED"));
        let production = Trust::production("https://accounts.example/").unwrap();
        assert!(
            store
                .saved(
                    &production,
                    crate::CLI_CLIENT,
                    "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
                )
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .saved(
                    &trust,
                    crate::CLI_CLIENT,
                    "cccccccc-cccc-cccc-cccc-cccccccccccc"
                )
                .unwrap()
                .is_none()
        );
        assert!(
            AccountStore::new(uuid::Uuid::new_v4(), memory)
                .saved(
                    &trust,
                    crate::CLI_CLIENT,
                    "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
                )
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn failed_save_and_late_return_never_activate_login() {
        let trust = Trust::local_test("http://localhost:3333/").unwrap();
        let fail = AccountStore::new(uuid::Uuid::new_v4(), Memory(Arc::default(), true));
        let (mut login, _) =
            crate::LoginContext::new("http://127.0.0.1:3333/callback".into()).unwrap();
        let state = login.state().to_owned();
        let redirect = login.redirect().to_owned();
        login.callback(&state, &redirect).unwrap();
        let (mut reply, _) = fixture(&trust);
        reply["authorization_context"] = login.state().into();
        let session =
            ConfirmedSession::confirm(&trust, crate::CLI_CLIENT, reply.clone(), &reply).unwrap();
        assert!(!login.complete(&fail, &session));
        assert_eq!(login.status(), &crate::LoginState::CleanupRequired);
        let store = AccountStore::new(uuid::Uuid::new_v4(), Memory::default());
        let (mut login, _) =
            crate::LoginContext::new("http://127.0.0.1:3333/callback".into()).unwrap();
        let (mut reply, _) = fixture(&trust);
        reply["authorization_context"] = login.state().into();
        let session =
            ConfirmedSession::confirm(&trust, crate::CLI_CLIENT, reply.clone(), &reply).unwrap();
        login.cancel();
        assert!(!login.complete(&store, &session));
        assert!(
            store
                .saved(
                    &trust,
                    crate::CLI_CLIENT,
                    "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
                )
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn token_metadata_without_matching_online_identity_is_refused() {
        let trust = Trust::local_test("http://localhost:3333/").unwrap();
        let (reply, mut online) = fixture(&trust);
        online["session_id"] = "cccccccc-cccc-cccc-cccc-cccccccccccc".into();
        assert!(ConfirmedSession::confirm(&trust, crate::CLI_CLIENT, reply, &online).is_err());
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("Chat service unavailable; keep the original operation")]
    Offline,
    #[error("Chat authentication or synchronization permission refused")]
    Denied,
    #[error("Chat synchronization version or operation conflict")]
    Conflict,
    #[error("Invalid Chat protocol response")]
    Invalid,
}
impl ConfirmedSession {
    /// Account transport is pinned to the reviewed issuer. No redirect receives a bearer.
    pub async fn chat_request(
        &self,
        action: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ChatError> {
        if !matches!(action, "status" | "control" | "push" | "pull") {
            return Err(ChatError::Invalid);
        }
        let bytes = serde_json::to_vec(&body).map_err(|_| ChatError::Invalid)?;
        if bytes.len() > caidex_cloud_core::chat::MAX_WIRE {
            return Err(ChatError::Invalid);
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|_| ChatError::Offline)?;
        let mut reply = client
            .post(format!("{}v1/chat/{action}", self.issuer()))
            .header("content-type", "application/json")
            .bearer_auth(&self.tokens.access_token)
            .body(bytes)
            .send()
            .await
            .map_err(|_| ChatError::Offline)?;
        match reply.status().as_u16() {
            200 => (),
            401 | 403 => return Err(ChatError::Denied),
            409 => return Err(ChatError::Conflict),
            429 | 500..=599 => return Err(ChatError::Offline),
            _ => return Err(ChatError::Invalid),
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = reply.chunk().await.map_err(|_| ChatError::Offline)? {
            if bytes.len() + chunk.len() > caidex_cloud_core::chat::MAX_WIRE {
                return Err(ChatError::Invalid);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| ChatError::Invalid)
    }
}
