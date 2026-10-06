use crate::{CredentialRef, Error, Result, Secret, SecretStore};

fn safe_error(error: keyring::Error) -> Error {
    match error {
        keyring::Error::NoStorageAccess(_) => Error::StorageAccessDenied,
        keyring::Error::BadEncoding(bytes) => {
            drop(zeroize::Zeroizing::new(bytes));
            Error::InvalidSecret
        }
        keyring::Error::TooLong(name, limit)
            if matches!(
                name.as_str(),
                "secret" | "password" | "password encoded as UTF-16"
            ) =>
        {
            Error::SecretTooLong(limit)
        }
        _ => Error::BackendUnavailable,
    }
}

/// Keyring is built with an explicit native backend. No mock default, fallback to
/// files, global builder replacement, or logging of third-party error payloads.
#[derive(Default)]
pub struct SystemStore;
impl SystemStore {
    fn entry(reference: &CredentialRef) -> Result<keyring::Entry> {
        keyring::Entry::new("org.caidex.credentials.v1", &reference.storage_key())
            .map_err(safe_error)
    }
}
impl SecretStore for SystemStore {
    fn get(&self, reference: &CredentialRef) -> Result<Option<Secret>> {
        match Self::entry(reference)?.get_password() {
            Ok(value) => Secret::new(value).map(Some),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(safe_error(error)),
        }
    }
    fn set(&self, reference: &CredentialRef, value: &Secret) -> Result<()> {
        Self::entry(reference)?
            .set_password(value.expose())
            .map_err(safe_error)
    }
    fn remove(&self, reference: &CredentialRef) -> Result<bool> {
        match Self::entry(reference)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(error) => Err(safe_error(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_errors_are_classified_without_exposing_attached_payloads() {
        let payload = "fixture-error-secret";
        for error in [
            keyring::Error::PlatformFailure(Box::new(std::io::Error::other(payload))),
            keyring::Error::NoStorageAccess(Box::new(std::io::Error::other(payload))),
            keyring::Error::BadEncoding(payload.as_bytes().to_vec()),
            keyring::Error::Invalid("password".into(), payload.into()),
            keyring::Error::TooLong(payload.into(), 2560),
        ] {
            let safe = safe_error(error);
            assert!(!format!("{safe:?}: {safe}").contains(payload));
        }
        assert_eq!(
            safe_error(keyring::Error::NoStorageAccess(Box::new(
                std::io::Error::other(payload)
            ))),
            Error::StorageAccessDenied
        );
        assert_eq!(
            safe_error(keyring::Error::TooLong(
                "password encoded as UTF-16".into(),
                2560
            )),
            Error::SecretTooLong(2560)
        );
    }
}
