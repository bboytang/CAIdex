use std::path::Path;

use crate::{Error, Result};

/// Create a private Host directory, or validate it without repairing user files.
pub fn private_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        match std::fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        let metadata = std::fs::symlink_metadata(path)?;
        // SAFETY: geteuid takes no pointers and transfers no ownership.
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o700
        {
            return Err(Error::Refused("Host directory must be owner-only (0700)"));
        }
    }
    #[cfg(windows)]
    {
        // Windows PowerShell/.NET is a platform facility; no path interpolation.
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", r#"
$ErrorActionPreference = 'Stop'
$p = $env:CAIDEX_PRIVATE_DIRECTORY
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
if (!(Test-Path -LiteralPath $p)) {
  $acl = New-Object System.Security.AccessControl.DirectorySecurity
  $acl.SetOwner($sid)
  $acl.SetAccessRuleProtection($true, $false)
  $rule = New-Object System.Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
  $acl.AddAccessRule($rule)
  New-Item -ItemType Directory -Path $p | Out-Null
  Set-Acl -LiteralPath $p -AclObject $acl
}
$item = Get-Item -LiteralPath $p -Force
if (!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unsafe directory' }
$acl = Get-Acl -LiteralPath $p
if ($acl.GetOwner([System.Security.Principal.SecurityIdentifier]) -ne $sid -or !$acl.AreAccessRulesProtected) { throw 'Unsafe owner/inheritance' }
$rules = @($acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))
if ($rules.Count -ne 1 -or $rules[0].IdentityReference -ne $sid -or $rules[0].AccessControlType -ne 'Allow' -or $rules[0].FileSystemRights -ne 'FullControl' -or $rules[0].InheritanceFlags -ne 'ContainerInherit,ObjectInherit') { throw 'Unsafe ACL' }
"#])
            .env("CAIDEX_PRIVATE_DIRECTORY", path)
            .output()?;
        if !output.status.success() {
            return Err(Error::Refused(
                "Host directory requires an owner-only Windows ACL",
            ));
        }
    }
    Ok(())
}
