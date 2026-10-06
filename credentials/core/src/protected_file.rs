use crate::{CredentialRef, Error, Result, Secret, SecretStore};
use std::{
    ffi::CString,
    fs::{DirBuilder, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

fn io(error: std::io::Error) -> Error {
    Error::Io(error.kind())
}
fn in_git(path: &Path) -> bool {
    path.ancestors().any(|dir| dir.join(".git").exists())
}

/// Explicit Unix file backend for headless hosts. Secrets are plaintext at rest,
/// protected by owner-only modes (directory 0700, files 0600); never an implicit
/// fallback from a locked/unavailable native store. Keeps an open directory FD so
/// changing a pathname cannot redirect subsequent secret I/O.
pub struct ProtectedFileStore {
    directory: File,
}
impl ProtectedFileStore {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(Error::UnsafeStorage);
        }
        if in_git(path) {
            return Err(Error::GitWorktree);
        }
        match DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(io(error)),
        }
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| Error::UnsafeStorage)?;
        let canonical = std::fs::canonicalize(path).map_err(io)?;
        if in_git(&canonical) {
            return Err(Error::GitWorktree);
        }
        let metadata = directory.metadata().map_err(io)?;
        // SAFETY: geteuid has no pointer arguments or ownership effects.
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o777 != 0o700 {
            return Err(Error::UnsafeStorage);
        }
        Ok(Self { directory })
    }
    fn name(reference: &CredentialRef) -> String {
        format!("{}.caidex-secret", reference.storage_key())
    }
    fn open_file(&self, name: &str, flags: i32) -> std::io::Result<File> {
        self.validate_directory()
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
        let name = CString::new(name).expect("validated identifiers contain no NUL");
        // SAFETY: directory FD remains live for this call; name is a NUL-terminated
        // single component. The returned descriptor is owned exactly once by File.
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: openat succeeded and transferred a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn validate(&self, file: &File) -> Result<()> {
        let metadata = file.metadata().map_err(io)?;
        // SAFETY: geteuid has no pointer arguments or ownership effects.
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(Error::UnsafeStorage);
        }
        Ok(())
    }
    fn validate_directory(&self) -> Result<()> {
        let metadata = self.directory.metadata().map_err(io)?;
        // SAFETY: geteuid has no pointer arguments or ownership effects.
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o777 != 0o700 {
            return Err(Error::UnsafeStorage);
        }
        Ok(())
    }
    fn existing(&self, name: &str) -> Result<Option<File>> {
        match self.open_file(name, libc::O_RDONLY) {
            Ok(file) => {
                self.validate(&file)?;
                Ok(Some(file))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) if error.raw_os_error() == Some(libc::ELOOP) => Err(Error::UnsafeStorage),
            Err(error) => Err(io(error)),
        }
    }
    fn unlink(&self, name: &str) -> std::io::Result<()> {
        self.validate_directory()
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
        let name = CString::new(name).expect("validated single component");
        // SAFETY: both pointers and directory FD are valid; no directory recursion.
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

impl SecretStore for ProtectedFileStore {
    fn get(&self, reference: &CredentialRef) -> Result<Option<Secret>> {
        let Some(file) = self.existing(&Self::name(reference))? else {
            return Ok(None);
        };
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        let value = std::str::from_utf8(&bytes).map_err(|_| Error::InvalidSecret)?;
        Secret::new(value.to_owned()).map(Some)
    }
    fn set(&self, reference: &CredentialRef, value: &Secret) -> Result<()> {
        let name = Self::name(reference);
        self.existing(&name)?;
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let temporary = format!(
            "{}.{}.{}.caidex-credential-tmp",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let mut file = self
            .open_file(&temporary, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)
            .map_err(io)?;
        let result = (|| {
            self.validate(&file)?;
            file.write_all(value.expose().as_bytes()).map_err(io)?;
            file.sync_all().map_err(io)?;
            let from = CString::new(temporary.as_str()).unwrap();
            let to = CString::new(name).unwrap();
            // SAFETY: both names are single components, pointers and the held FD
            // remain valid. Atomic rename replaces a name, never follows a symlink.
            if unsafe {
                libc::renameat(
                    self.directory.as_raw_fd(),
                    from.as_ptr(),
                    self.directory.as_raw_fd(),
                    to.as_ptr(),
                )
            } < 0
            {
                return Err(io(std::io::Error::last_os_error()));
            }
            self.directory.sync_all().map_err(io)
        })();
        let _ = self.unlink(&temporary);
        result
    }
    fn remove(&self, reference: &CredentialRef) -> Result<bool> {
        let name = Self::name(reference);
        if self.existing(&name)?.is_none() {
            return Ok(false);
        }
        self.unlink(&name).map_err(io)?;
        self.directory.sync_all().map_err(io)?;
        Ok(true)
    }
}
