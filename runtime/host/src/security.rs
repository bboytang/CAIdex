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
$step = 10
try {
$p = $env:CAIDEX_PRIVATE_DIRECTORY
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
if (!(Test-Path -LiteralPath $p)) {
  $step = 11
  $acl = New-Object System.Security.AccessControl.DirectorySecurity
  $step = 12
  $acl.SetOwner($sid)
  $step = 13
  $acl.SetAccessRuleProtection($true, $false)
  $step = 14
  $rule = New-Object System.Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
  $step = 15
  $acl.AddAccessRule($rule)
  $step = 16
  New-Item -ItemType Directory -Path $p | Out-Null
  $step = 17
  Set-Acl -LiteralPath $p -AclObject $acl
}
$step = 18
$item = Get-Item -LiteralPath $p -Force
$step = 19
if (!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unsafe directory' }
$step = 20
$acl = Get-Acl -LiteralPath $p
$step = 21
if ($acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -ne $sid.Value) { throw 'Unsafe owner' }
$step = 22
if (!$acl.AreAccessRulesProtected) { throw 'Unsafe inheritance' }
$step = 23
$rules = @($acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))
$step = 24
if ($rules.Count -ne 1) { throw 'Unsafe ACL count' }
$step = 25
if ($rules[0].IdentityReference.Value -ne $sid.Value) { throw 'Unsafe ACL identity' }
$step = 26
if ($rules[0].AccessControlType -ne [System.Security.AccessControl.AccessControlType]::Allow) { throw 'Unsafe ACL type' }
$step = 27
if ($rules[0].FileSystemRights -ne [System.Security.AccessControl.FileSystemRights]::FullControl) { throw 'Unsafe ACL rights' }
$step = 28
$inherit = [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [System.Security.AccessControl.InheritanceFlags]::ObjectInherit
if ($rules[0].InheritanceFlags -ne $inherit) { throw 'Unsafe ACL inheritance' }
} catch { exit $step }
"#])
            .env("CAIDEX_PRIVATE_DIRECTORY", path)
            // Cargo can inherit PowerShell 7's incompatible module path; let
            // Windows PowerShell resolve its own built-in Get-Acl/Set-Acl module.
            .env_remove("PSModulePath")
            .output()?;
        if !output.status.success() {
            return Err(Error::WindowsAcl(output.status.code().unwrap_or(0)));
        }
    }
    Ok(())
}
