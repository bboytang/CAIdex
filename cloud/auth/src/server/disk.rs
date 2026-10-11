use super::{Error, Result};
use std::path::PathBuf;

pub(super) struct Guard {
    pub path: PathBuf,
    pub reserve: u64,
}
impl Guard {
    pub fn check(&self) -> Result<()> {
        #[cfg(unix)]
        {
            use std::{ffi::CString, os::unix::ffi::OsStrExt};
            let path =
                CString::new(self.path.as_os_str().as_bytes()).map_err(|_| Error::Invalid)?;
            let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
            // SAFETY: path is NUL-terminated and stats points to writable storage.
            if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
                return Err(Error::Unavailable);
            }
            // SAFETY: a successful statvfs initialized the entire struct.
            let stats = unsafe { stats.assume_init() };
            let free = (stats.f_bavail as u128).saturating_mul(stats.f_frsize as u128);
            if free < u128::from(self.reserve) {
                return Err(Error::Unavailable);
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err(Error::Unavailable)
        }
    }
}
