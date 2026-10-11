use crate::{Error, Result};
use std::path::Path;

/// Reuse the shared platform boundary without changing Host error contracts.
pub fn private_directory(path: &Path) -> Result<()> {
    caidex_credentials::private_directory(path).map_err(|error| match error {
        caidex_credentials::PrivateDirectoryError::Io(kind) => {
            Error::Io(std::io::Error::from(kind))
        }
        caidex_credentials::PrivateDirectoryError::Unsafe => {
            Error::Refused("Host directory must be owner-only (0700)")
        }
        caidex_credentials::PrivateDirectoryError::WindowsAcl(code) => Error::WindowsAcl(code),
    })
}
