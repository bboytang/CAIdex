#![cfg(any(windows, all(target_os = "linux", feature = "linux-secret-service")))]
use caidex_credentials::{Broker, CredentialRef, Id, Secret, SecretKind, SecretStore, SystemStore};
use std::time::{SystemTime, UNIX_EPOCH};

struct Cleanup(CredentialRef);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = SystemStore.remove(&self.0);
    }
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "requires an isolated unlocked Secret Service; never use a user's keyring"
)]
fn native_store_round_trip_update_delete_and_missing_entry() {
    let owner = Id::new(format!(
        "fixture-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
    .unwrap();
    let reference = CredentialRef {
        owner: owner.clone(),
        provider: Id::new("synthetic").unwrap(),
        profile: Id::new("test").unwrap(),
        kind: SecretKind::ApiKey,
    };
    let _cleanup = Cleanup(reference.clone());
    let broker = Broker::new(owner, SystemStore);
    assert!(!broker.status(&reference).unwrap().configured);
    broker
        .set(
            &reference,
            Secret::new("fixture-native-旧-value".into()).unwrap(),
        )
        .unwrap();
    assert_eq!(
        broker.resolve(&reference).unwrap().unwrap().expose(),
        "fixture-native-旧-value"
    );
    broker
        .set(
            &reference,
            Secret::new("fixture-native-new-value".into()).unwrap(),
        )
        .unwrap();
    assert_eq!(
        broker.resolve(&reference).unwrap().unwrap().expose(),
        "fixture-native-new-value"
    );
    #[cfg(windows)]
    {
        // Windows stores passwords as UTF-16; its blob limit is lower than the
        // core's 16 KiB UTF-8 input cap. A failed replacement preserves the key.
        assert!(matches!(
            broker.set(&reference, Secret::new("x".repeat(3000)).unwrap()),
            Err(caidex_credentials::Error::SecretTooLong(_))
        ));
        assert_eq!(
            broker.resolve(&reference).unwrap().unwrap().expose(),
            "fixture-native-new-value"
        );
    }
    assert!(broker.remove(&reference).unwrap());
    assert!(!broker.remove(&reference).unwrap());
    assert!(!broker.status(&reference).unwrap().configured);
}
