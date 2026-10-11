use oauth2::{PkceCodeChallenge, PkceCodeVerifier};
use serde::{Deserialize, Serialize};
use url::Url;

pub const AUDIENCE: &str = "caidex-data-v1";
pub const CLI_CLIENT: &str = "caidex-cli";

#[derive(Debug, thiserror::Error)]
#[error("Account authentication refused")]
pub struct Refused;

/// Construct only from the independent reviewed account configuration, never a model/Profile.
#[derive(Clone)]
pub struct Trust {
    issuer: Url,
    test: bool,
}

impl Trust {
    pub fn production(issuer: &str) -> Result<Self, Refused> {
        Self::new(issuer, false)
    }

    pub fn local_test(issuer: &str) -> Result<Self, Refused> {
        Self::new(issuer, true)
    }

    fn new(issuer: &str, test: bool) -> Result<Self, Refused> {
        let url = Url::parse(issuer).map_err(|_| Refused)?;
        if url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || (test && (url.scheme() != "http" || url.host_str() != Some("localhost")))
            || (!test
                && (url.scheme() != "https"
                    || url.host_str().is_none()
                    || matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
        {
            return Err(Refused);
        }
        Ok(Self { issuer: url, test })
    }

    pub fn issuer(&self) -> &str {
        self.issuer.as_str()
    }
    pub fn is_test(&self) -> bool {
        self.test
    }
    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.issuer())
    }

    /// No discovery redirect or arbitrary same-scheme URL may receive an account token.
    pub fn verify_metadata(&self, metadata: &serde_json::Value) -> Result<(), Refused> {
        let required = [
            ("issuer", self.issuer().to_owned()),
            ("authorization_endpoint", self.endpoint("authorize")),
            (
                "device_authorization_endpoint",
                self.endpoint("v1/auth/device"),
            ),
            ("token_endpoint", self.endpoint("v1/auth/token")),
            ("revocation_endpoint", self.endpoint("v1/auth/revoke")),
            ("verification_uri", self.endpoint("device")),
        ];
        if required
            .iter()
            .any(|(key, value)| metadata[*key].as_str() != Some(value))
        {
            return Err(Refused);
        }
        Ok(())
    }

    /// Token identity is confirmed online by the trusted backend, never parsed from opaque text.
    pub fn verify_session(&self, value: &serde_json::Value, client: &str) -> Result<(), Refused> {
        if value["issuer"].as_str() != Some(self.issuer())
            || value["audience"].as_str() != Some(AUDIENCE)
            || value["client_id"].as_str() != Some(client)
            || serde_json::from_value::<caidex_cloud_core::Id>(value["user_id"].clone()).is_err()
            || serde_json::from_value::<caidex_cloud_core::Id>(value["session_id"].clone()).is_err()
        {
            return Err(Refused);
        }
        Ok(())
    }
}

pub fn loopback_redirect(value: &str) -> Result<(), Refused> {
    let url = Url::parse(value).map_err(|_| Refused)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
        || url.port().is_none()
        || url.port() == Some(0)
        || url.path() != "/callback"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Refused);
    }
    Ok(())
}

/// Non-secret status only; credential material is held separately by the platform store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginState {
    Pending,
    Exchanging,
    Cancelled,
    CleanupRequired,
    Authenticated,
    Failed,
}

/// P connects this contract to the browser/listener and existing protected credential store.
pub struct LoginContext {
    state: String,
    redirect: String,
    verifier: PkceCodeVerifier,
    status: LoginState,
}

impl LoginContext {
    pub fn new(redirect: String) -> Result<(Self, String), Refused> {
        loopback_redirect(&redirect)?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let state = oauth2::CsrfToken::new_random_len(32);
        Ok((
            Self {
                state: state.secret().to_owned(),
                redirect,
                verifier,
                status: LoginState::Pending,
            },
            challenge.as_str().to_owned(),
        ))
    }
    pub fn state(&self) -> &str {
        &self.state
    }
    pub fn redirect(&self) -> &str {
        &self.redirect
    }
    pub fn callback(&mut self, state: &str, redirect: &str) -> Result<&str, Refused> {
        if self.status != LoginState::Pending || state != self.state || redirect != self.redirect {
            return Err(Refused);
        }
        self.status = LoginState::Exchanging;
        Ok(self.verifier.secret())
    }
    pub fn status(&self) -> &LoginState {
        &self.status
    }
    pub fn cancel(&mut self) {
        self.status = LoginState::Cancelled;
    }
    /// A late return or failed atomic secure save requires revoking only the newly issued session.
    fn token_returned(&mut self, verified: bool, securely_saved: bool) -> bool {
        if self.status != LoginState::Exchanging || !verified || !securely_saved {
            self.status = LoginState::CleanupRequired;
            return false;
        }
        self.status = LoginState::Authenticated;
        true
    }
    pub fn complete<S: caidex_credentials::SecretStore>(
        &mut self,
        store: &crate::AccountStore<S>,
        session: &crate::ConfirmedSession,
    ) -> bool {
        if session.client() != CLI_CLIENT || session.context() != Some(self.state.as_str()) {
            self.status = LoginState::Failed;
            return false;
        }
        if self.status != LoginState::Exchanging {
            return self.token_returned(true, false);
        }
        self.token_returned(true, store.save(session).is_ok())
    }
    pub fn cleanup_confirmed(&mut self) {
        self.status = LoginState::Failed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trust_and_redirect_reject_wrong_targets() {
        for issuer in [
            "http://accounts.example/",
            "https://user:secret@accounts.example/",
            "https://accounts.example/model?token=x",
            "https://localhost/",
        ] {
            assert!(Trust::production(issuer).is_err());
        }
        assert!(Trust::local_test("http://evil.example/").is_err());
        let trust = Trust::production("https://accounts.example/").unwrap();
        let mut metadata = serde_json::json!({"issuer":trust.issuer(),"authorization_endpoint":trust.endpoint("authorize"),"device_authorization_endpoint":trust.endpoint("v1/auth/device"),"token_endpoint":trust.endpoint("v1/auth/token"),"revocation_endpoint":trust.endpoint("v1/auth/revoke"),"verification_uri":trust.endpoint("device")});
        assert!(trust.verify_metadata(&metadata).is_ok());
        metadata["token_endpoint"] = "https://models.example/token".into();
        assert!(trust.verify_metadata(&metadata).is_err());
        for redirect in [
            "http://localhost:3333/callback",
            "http://127.0.0.1:0/callback",
            "http://127.0.0.1:3333/other",
            "https://evil.example/callback",
            "http://127.0.0.1:3333/callback?x=1",
        ] {
            assert!(loopback_redirect(redirect).is_err());
        }
        assert!(loopback_redirect("http://[::1]:3333/callback").is_ok());
    }
    #[test]
    fn cancelled_late_or_failed_store_never_logs_in() {
        let (mut login, _) = LoginContext::new("http://127.0.0.1:3333/callback".into()).unwrap();
        let state = login.state().to_owned();
        let redirect = login.redirect().to_owned();
        assert!(login.callback("forged", &redirect).is_err());
        assert!(login.callback(&state, &redirect).is_ok());
        assert!(login.callback(&state, &redirect).is_err());
        login.cancel();
        assert!(login.callback(&state, &redirect).is_err());
        assert!(!login.token_returned(true, true));
        assert_eq!(login.status, LoginState::CleanupRequired);
        login.cleanup_confirmed();
        let (mut login, _) = LoginContext::new("http://127.0.0.1:3333/callback".into()).unwrap();
        let state = login.state().to_owned();
        let redirect = login.redirect().to_owned();
        login.callback(&state, &redirect).unwrap();
        assert!(!login.token_returned(true, false));
        let (mut login, _) = LoginContext::new("http://127.0.0.1:3333/callback".into()).unwrap();
        let state = login.state().to_owned();
        let redirect = login.redirect().to_owned();
        login.callback(&state, &redirect).unwrap();
        assert!(login.token_returned(true, true));
        assert_eq!(login.status, LoginState::Authenticated);
    }
}
