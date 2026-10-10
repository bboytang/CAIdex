use std::{path::PathBuf, time::Duration};

use caidex_host::{Journal, private_directory, serve};
use caidex_runtime::{AppServer, CODEX_VERSION, ClientOptions, Runtime};
use tokio::{net::TcpListener, process::Command};

const DEADLINE: Duration = Duration::from_secs(15);

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("CAIdex H-1 Host failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let action = args
        .next()
        .ok_or("use caidex-host run|inspect <private-directory>")?;
    let directory = PathBuf::from(args.next().ok_or("missing private Host directory")?);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    if action != "run" && action != "inspect" {
        return Err("use caidex-host run|inspect <private-directory>".into());
    }
    if action == "inspect" {
        if !directory.join("journal.sqlite3").is_file() {
            return Err("no existing journal".into());
        }
        println!("{}", serde_json::to_string(&Journal::inspect(&directory)?)?);
        return Ok(());
    }
    let token = std::env::var("CAIDEX_HOST_TOKEN").map_err(
        |_| "CAIDEX_HOST_TOKEN must be an independently generated 64-character hex token",
    )?;
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("CAIDEX_HOST_TOKEN must be 64 hex characters".into());
    }
    let binary = std::env::var_os("CAIDEX_CODEX_BIN")
        .ok_or("set CAIDEX_CODEX_BIN to the pinned executable")?;
    let version = tokio::time::timeout(
        DEADLINE,
        Command::new(&binary)
            .arg("--version")
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    if !version.status.success()
        || String::from_utf8_lossy(&version.stdout).trim() != format!("codex-cli {CODEX_VERSION}")
    {
        return Err("expected pinned Codex 0.160.1; no automatic upgrade".into());
    }
    let journal = Journal::open(&directory)?;
    // Keep an absolute path without Windows' verbatim prefix for PowerShell.
    let directory = std::path::absolute(directory)?;
    let data = directory.join("runtime");
    let project = directory.join("probe-project");
    private_directory(&data)?;
    private_directory(&project)?;
    if data.join("auth.json").exists() {
        return Err("Host probe directory contains authentication data; no import/read".into());
    }
    let config = data.join("config.toml");
    let expected = concat!(
        "model = \"caidex-h1-probe\"\nmodel_provider = \"caidex_h1_probe\"\n",
        "[model_providers.caidex_h1_probe]\nname = \"CAIdex H-1 offline probe\"\n",
        "base_url = \"http://127.0.0.1:9/v1\"\nwire_api = \"responses\"\n",
        "requires_openai_auth = false\n[analytics]\nenabled = false\n",
    );
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&config)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(expected.as_bytes())?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::symlink_metadata(&config)?.is_symlink()
                || std::fs::read_to_string(&config)? != expected
            {
                return Err("existing Host probe config differs; no overwrite/import".into());
            }
        }
        Err(error) => return Err(error.into()),
    }
    let mut command = Command::new(binary);
    command.env_clear();
    for name in ["PATH", "HOME", "SystemRoot", "USERPROFILE", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env("CODEX_HOME", &data)
        .current_dir(&project)
        .args(["app-server", "--listen", "stdio://"]);
    let runtime = Runtime::connect(
        AppServer::spawn(command, 256)?,
        ClientOptions::default(),
        DEADLINE,
        256,
    )
    .await?;
    serve(
        TcpListener::bind("127.0.0.1:0").await?,
        runtime,
        journal,
        token,
        project,
    )
    .await?;
    Ok(())
}
