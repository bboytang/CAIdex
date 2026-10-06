use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use caidex_runtime::{
    AppServer, CODEX_COMMIT, CODEX_VERSION, ClientOptions, Runtime, RuntimeEvent,
};
use serde_json::{Value, json};
use tokio::process::Command;

const DEADLINE: Duration = Duration::from_secs(15);

struct ProbeDirectory(PathBuf);

impl ProbeDirectory {
    fn create() -> Result<Self, Box<dyn std::error::Error>> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path =
            std::env::temp_dir().join(format!("caidex-doctor-{}-{unique}", std::process::id()));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub async fn run() -> Result<Value, Box<dyn std::error::Error>> {
    let version = tokio::time::timeout(
        DEADLINE,
        Command::new(super::codex_binary())
            .arg("--version")
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    let expected = format!("codex-cli {CODEX_VERSION}");
    if !version.status.success() || String::from_utf8_lossy(&version.stdout).trim() != expected {
        return Err(
            format!("expected {expected}; set CAIDEX_CODEX_BIN to the pinned executable").into(),
        );
    }
    let directory = ProbeDirectory::create()?;
    let data = directory.0.join("runtime");
    let project = directory.0.join("project");
    std::fs::create_dir(&data)?;
    std::fs::create_dir(&project)?;
    std::fs::write(
        data.join("config.toml"),
        concat!(
            "model = \"caidex-probe\"\nmodel_provider = \"caidex_probe\"\n",
            "[model_providers.caidex_probe]\nname = \"CAIdex offline probe\"\n",
            "base_url = \"http://127.0.0.1:9/v1\"\nwire_api = \"responses\"\n",
            "requires_openai_auth = false\n[analytics]\nenabled = false\n",
        ),
    )?;
    let mut command = Command::new(super::codex_binary());
    command.env_clear();
    for name in ["PATH", "HOME", "SystemRoot", "USERPROFILE", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    // Child-only Codex data isolation: never read/write the user's login or config.
    command
        .env("CODEX_HOME", &data)
        .current_dir(&project)
        .args(["app-server", "--listen", "stdio://"]);
    let mut runtime = Runtime::connect(
        AppServer::spawn(command, 128)?,
        ClientOptions::default(),
        DEADLINE,
        128,
    )
    .await?;
    let client = runtime.client();
    let report = async {
        let start = client.start_thread(json!({
            "cwd": project, "ephemeral": true, "approvalPolicy": "never", "sandbox": "read-only"
        }), DEADLINE).await?;
        let thread_id = start.pointer("/thread/id").and_then(Value::as_str)
            .ok_or("thread/start returned no thread ID")?;
        let loaded = client.call("thread/loaded/list", Some(json!({})), DEADLINE).await?;
        let threads = loaded["data"].as_array().ok_or("thread/loaded/list returned no data")?;
        if !threads.iter().any(|id| id.as_str() == Some(thread_id)) {
            return Err::<Value, Box<dyn std::error::Error>>("created thread was not loaded".into());
        }
        let started = tokio::time::timeout(DEADLINE, async {
            while let Some(event) = runtime.next_event().await {
                let RuntimeEvent::Notification(event) = event else { continue };
                if event.method == "thread/started" && event.raw.pointer("/params/thread/id")
                    .and_then(Value::as_str) == Some(thread_id) {
                    return true;
                }
            }
            false
        }).await?;
        if !started {
            return Err("thread/started notification missing".into());
        }
        let read = client.read_thread(thread_id, false, DEADLINE).await?;
        if read.pointer("/thread/id").and_then(Value::as_str) != Some(thread_id) {
            return Err("thread/read returned a different thread".into());
        }
        let init = &runtime.info().initialize_result;
        Ok(json!({
            "status": "ok", "codexVersion": CODEX_VERSION, "upstreamCommit": CODEX_COMMIT,
            "platformFamily": init["platformFamily"], "platformOs": init["platformOs"],
            "checks": ["initialize", "initialized", "thread/start", "thread/loaded/list", "thread/started", "thread/read"],
            "modelTurnStarted": false, "isolatedRuntimeData": true
        }))
    }.await;
    runtime.shutdown().await?;
    report
}
