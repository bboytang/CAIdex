use caidex_credentials::{
    Broker, CredentialRef, EnvironmentStore, Id, Secret, SecretKind, SecretStore,
};
#[cfg(unix)]
use std::path::Path;
use std::{
    collections::HashMap,
    ffi::OsString,
    io::{IsTerminal, Read},
};
use zeroize::Zeroizing;

const HELP: &str = "Manage executor-local credentials (no secret export).\n\ncaidex credentials <status|set|remove> --owner ID --provider ID --profile ID\n  --store system|file|env [--kind api-key|access-token|refresh-token|client-secret]\n  --directory /absolute/private/path   Required for file storage (Unix only)\n  --variable VARIABLE_NAME            Required for read-only environment storage\n  --stdin                             Required for set; pipe UTF-8 input, then EOF\n\nIDs: 1-64 lowercase ASCII letters, digits, '-' or '_'.\nFile storage is plaintext protected by 0700/0600 permissions, outside Git.\nSystem storage uses Windows Credential Manager or Linux Secret Service.\nOwner identifies this executor; this command does not manage a remote Host.\nNever pass secrets as arguments. Interactive secret entry is not available yet.";
const USAGE: &str = "invalid arguments; use 'caidex credentials --help'";

type Result<T> = std::result::Result<T, String>;

pub fn run(args: Vec<OsString>) -> Result<String> {
    if args.len() == 1 && args[0] == "--help" {
        return Ok(HELP.into());
    }
    let mut args = args.into_iter();
    let action = args.next().ok_or(USAGE)?;
    let action = action.to_str().ok_or(USAGE)?;
    if !matches!(action, "status" | "set" | "remove") {
        return Err(USAGE.into());
    }
    let mut options = HashMap::new();
    let mut stdin = false;
    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(USAGE)?;
        if flag == "--stdin" {
            if stdin {
                return Err(USAGE.into());
            }
            stdin = true;
            continue;
        }
        if !matches!(
            flag,
            "--owner"
                | "--provider"
                | "--profile"
                | "--kind"
                | "--store"
                | "--directory"
                | "--variable"
        ) {
            return Err(USAGE.into());
        }
        let value = args.next().ok_or(USAGE)?;
        if options.insert(flag.to_owned(), value).is_some() {
            return Err(USAGE.into());
        }
    }
    if stdin != (action == "set") {
        return Err(USAGE.into());
    }
    let option = |name: &str| -> Result<&str> {
        options
            .get(name)
            .and_then(|value| value.to_str())
            .ok_or_else(|| USAGE.into())
    };
    let id = |name| Id::new(option(name)?).map_err(|error| error.to_string());
    let kind = match options.get("--kind").map(|value| value.to_str()) {
        None | Some(Some("api-key")) => SecretKind::ApiKey,
        Some(Some("access-token")) => SecretKind::AccessToken,
        Some(Some("refresh-token")) => SecretKind::RefreshToken,
        Some(Some("client-secret")) => SecretKind::ClientSecret,
        _ => return Err(USAGE.into()),
    };
    let reference = CredentialRef {
        owner: id("--owner")?,
        provider: id("--provider")?,
        profile: id("--profile")?,
        kind,
    };
    match option("--store")? {
        "env" if !options.contains_key("--directory") => {
            let store = EnvironmentStore::new(HashMap::from([(
                reference.clone(),
                option("--variable")?.to_owned(),
            )]))
            .map_err(|error| error.to_string())?;
            execute(store, &reference, action)
        }
        "file" if !options.contains_key("--variable") => {
            let directory = options.get("--directory").ok_or(USAGE)?;
            #[cfg(unix)]
            {
                let store = caidex_credentials::ProtectedFileStore::open(Path::new(directory))
                    .map_err(|error| error.to_string())?;
                execute(store, &reference, action)
            }
            #[cfg(not(unix))]
            {
                let _ = directory;
                Err("file credential storage is available only on Unix".into())
            }
        }
        "system" if !options.contains_key("--variable") && !options.contains_key("--directory") => {
            #[cfg(any(windows, target_os = "linux"))]
            {
                execute(caidex_credentials::SystemStore, &reference, action)
            }
            #[cfg(not(any(windows, target_os = "linux")))]
            {
                Err("native credential storage is not available on this platform".into())
            }
        }
        _ => Err(USAGE.into()),
    }
}

fn execute<S: SecretStore>(store: S, reference: &CredentialRef, action: &str) -> Result<String> {
    if action != "status" && store.is_read_only() {
        return Err(caidex_credentials::Error::ReadOnly.to_string());
    }
    let broker = Broker::new(reference.owner.clone(), store);
    let report = match action {
        "status" => serde_json::to_value(
            broker
                .status(reference)
                .map_err(|error| error.to_string())?,
        ),
        "set" => {
            let input = std::io::stdin();
            if input.is_terminal() {
                return Err(
                    "pipe secret input via stdin; interactive input would expose it".into(),
                );
            }
            let mut bytes = Zeroizing::new(Vec::new());
            input
                .lock()
                .take(16 * 1024 + 3)
                .read_to_end(&mut bytes)
                .map_err(|_| "could not read secret input")?;
            // Remove one transport line ending; preserve spaces and all other bytes.
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
            }
            let value = std::str::from_utf8(&bytes).map_err(|_| "secret input must be UTF-8")?;
            let value = Secret::new(value.to_owned()).map_err(|error| error.to_string())?;
            broker
                .set(reference, value)
                .map_err(|error| error.to_string())?;
            Ok(serde_json::json!({"reference": reference, "saved": true}))
        }
        "remove" => Ok(serde_json::json!({
            "reference": reference,
            "removed": broker.remove(reference).map_err(|error| error.to_string())?
        })),
        _ => unreachable!("validated action"),
    };
    serde_json::to_string(&report.map_err(|_| "could not encode credential status")?)
        .map_err(|_| "could not encode credential status".into())
}
