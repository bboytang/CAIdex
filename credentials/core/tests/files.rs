#![cfg(unix)]
use caidex_credentials::{
    Broker, CredentialRef, Error, Id, ProtectedFileStore, Secret, SecretKind, SecretStore,
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "caidex-credential-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn reference() -> CredentialRef {
    CredentialRef {
        owner: Id::new("local").unwrap(),
        provider: Id::new("custom").unwrap(),
        profile: Id::new("main").unwrap(),
        kind: SecretKind::ApiKey,
    }
}
fn filename() -> &'static str {
    "local.custom.main.api-key.caidex-secret"
}
fn secret(value: &str) -> Secret {
    Secret::new(value.into()).unwrap()
}

#[test]
fn file_store_is_private_persistent_atomic_and_keeps_profiles_separate() {
    let directory = Directory::new();
    let root = directory.0.join("secrets");
    let reference = reference();
    let alternate = CredentialRef {
        profile: Id::new("testing").unwrap(),
        ..reference.clone()
    };
    let broker = Broker::new(
        Id::new("local").unwrap(),
        ProtectedFileStore::open(&root).unwrap(),
    );
    broker
        .set(&reference, secret("fixture-before-replacement"))
        .unwrap();
    broker
        .set(&alternate, secret("fixture-alternate-profile"))
        .unwrap();
    broker
        .set(&reference, secret("fixture-after-replacement-你好"))
        .unwrap();
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for entry in fs::read_dir(&root).unwrap() {
        let entry = entry.unwrap();
        assert_eq!(
            entry.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!entry.file_name().to_string_lossy().ends_with("-tmp"));
    }
    let reopened = ProtectedFileStore::open(&root).unwrap();
    assert_eq!(
        reopened.get(&reference).unwrap().unwrap().expose(),
        "fixture-after-replacement-你好"
    );
    assert_eq!(
        reopened.get(&alternate).unwrap().unwrap().expose(),
        "fixture-alternate-profile"
    );
    assert!(broker.remove(&reference).unwrap());
    assert!(reopened.get(&reference).unwrap().is_none());
    assert!(reopened.get(&alternate).unwrap().is_some());
}

#[test]
fn broad_permissions_and_permission_changes_fail_closed() {
    let directory = Directory::new();
    let root = directory.0.join("secrets");
    let store = ProtectedFileStore::open(&root).unwrap();
    store
        .set(&reference(), &secret("fixture-private-file"))
        .unwrap();
    fs::set_permissions(root.join(filename()), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(store.get(&reference()).unwrap_err(), Error::UnsafeStorage);
    assert_eq!(
        store.set(&reference(), &secret("fixture-rejected")),
        Err(Error::UnsafeStorage)
    );
    assert_eq!(store.remove(&reference()), Err(Error::UnsafeStorage));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(store.get(&reference()).is_err());
    assert!(ProtectedFileStore::open(&root).is_err());
}

#[test]
fn symlink_hardlink_and_directory_entries_never_read_write_or_delete_targets() {
    let directory = Directory::new();
    let root = directory.0.join("secrets");
    let store = ProtectedFileStore::open(&root).unwrap();
    let target = directory.0.join("outside");
    fs::write(&target, "fixture-outside-target").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    let entry = root.join(filename());
    symlink(&target, &entry).unwrap();
    for operation in 0..3 {
        let result = match operation {
            0 => store.get(&reference()).map(|_| ()),
            1 => store.set(&reference(), &secret("fixture-rejected")),
            _ => store.remove(&reference()).map(|_| ()),
        };
        assert_eq!(result, Err(Error::UnsafeStorage));
    }
    fs::remove_file(&entry).unwrap();
    fs::hard_link(&target, &entry).unwrap();
    assert_eq!(store.get(&reference()).unwrap_err(), Error::UnsafeStorage);
    assert_eq!(
        store.set(&reference(), &secret("fixture-rejected")),
        Err(Error::UnsafeStorage)
    );
    fs::remove_file(&entry).unwrap();
    fs::create_dir(&entry).unwrap();
    assert!(store.get(&reference()).is_err());
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "fixture-outside-target"
    );
    assert!(ProtectedFileStore::open(&directory.0.join("relative-does-not-exist/child")).is_err());
}

#[test]
fn held_directory_descriptor_prevents_path_replacement_from_redirecting_secrets() {
    let directory = Directory::new();
    let root = directory.0.join("secrets");
    let moved = directory.0.join("held-secrets");
    let store = ProtectedFileStore::open(&root).unwrap();
    store
        .set(&reference(), &secret("fixture-held-value"))
        .unwrap();
    fs::rename(&root, &moved).unwrap();
    fs::create_dir(&root).unwrap();
    store
        .set(&reference(), &secret("fixture-held-replacement"))
        .unwrap();
    assert!(!root.join(filename()).exists());
    assert_eq!(
        fs::read_to_string(moved.join(filename())).unwrap(),
        "fixture-held-replacement"
    );
    assert_eq!(
        store.get(&reference()).unwrap().unwrap().expose(),
        "fixture-held-replacement"
    );
}

#[test]
fn git_worktrees_root_symlinks_and_invalid_contents_are_rejected() {
    let directory = Directory::new();
    let root = directory.0.join("secrets");
    let store = ProtectedFileStore::open(&root).unwrap();
    let alias = directory.0.join("alias");
    symlink(&root, &alias).unwrap();
    assert!(ProtectedFileStore::open(&alias).is_err());
    fs::write(root.join(filename()), [0xff, 0xfe]).unwrap();
    fs::set_permissions(root.join(filename()), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(store.get(&reference()).unwrap_err(), Error::InvalidSecret);
    let project = directory.0.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join(".git"), "gitdir: ../synthetic-git").unwrap();
    assert!(matches!(
        ProtectedFileStore::open(&project.join("secrets")),
        Err(Error::GitWorktree)
    ));
    assert!(matches!(
        ProtectedFileStore::open(std::path::Path::new("relative")),
        Err(Error::UnsafeStorage)
    ));
}
