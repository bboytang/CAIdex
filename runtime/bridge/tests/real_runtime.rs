//! Real pinned app-server and scripted loopback Responses; no paid model or key.
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use caidex_runtime::{
    AppServer, ApprovalDecision, CODEX_VERSION, ClientOptions, Error, InteractionKind, Runtime,
    RuntimeEvent,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

const DEADLINE: Duration = Duration::from_secs(30);

struct TestDirectory(PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Harness {
    runtime: Runtime,
    provider: Child,
    directory: TestDirectory,
}

impl Harness {
    async fn start(mode: &str) -> Self {
        let binary = std::env::var_os("CAIDEX_CODEX_BIN").unwrap_or_else(|| "codex".into());
        let version = Command::new(&binary)
            .arg("--version")
            .kill_on_drop(true)
            .output()
            .await
            .unwrap();
        assert!(version.status.success());
        assert_eq!(
            String::from_utf8_lossy(&version.stdout).trim(),
            format!("codex-cli {CODEX_VERSION}")
        );
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = TestDirectory(
            std::env::temp_dir().join(format!("caidex-runtime-{}-{unique}", std::process::id())),
        );
        std::fs::create_dir(&directory.0).unwrap();
        let data = directory.0.join("data");
        let project = directory.0.join("project");
        std::fs::create_dir(&data).unwrap();
        std::fs::create_dir(&project).unwrap();
        let python = std::env::var_os("CAIDEX_TEST_PYTHON")
            .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into());
        let mut provider_command = isolated_command(&python);
        provider_command
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/responses_server.py"),
            )
            .arg(mode)
            .arg(directory.0.join("trace.json"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());
        let mut provider = provider_command.spawn().unwrap();
        let mut output = BufReader::new(provider.stdout.take().unwrap()).lines();
        let line = tokio::time::timeout(DEADLINE, output.next_line())
            .await
            .unwrap()
            .unwrap()
            .expect("local Responses server did not start");
        let port = serde_json::from_str::<Value>(&line).unwrap()["port"]
            .as_u64()
            .unwrap();
        std::fs::write(data.join("config.toml"), format!(
            "model = \"gpt-5.1-codex\"\nmodel_provider = \"caidex_fixture\"\n[model_providers.caidex_fixture]\nname = \"CAIdex local protocol fixture\"\nbase_url = \"http://127.0.0.1:{port}/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nrequest_max_retries = 0\nstream_max_retries = 0\n[analytics]\nenabled = false\n"
        )).unwrap();
        if mode == "mcp" {
            let fixture =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_server.py");
            let trace = directory.0.join("mcp-trace.json");
            let config = format!(
                "\n[mcp_servers.fixture]\ncommand = {}\nargs = [{}, {}]\nstartup_timeout_sec = 10\ntool_timeout_sec = 10\n",
                json!(python.to_string_lossy()),
                json!(fixture.to_string_lossy()),
                json!(trace.to_string_lossy())
            );
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(data.join("config.toml"))
                .unwrap()
                .write_all(config.as_bytes())
                .unwrap();
        }
        let mut command = isolated_command(&binary);
        command
            .env("CODEX_HOME", &data)
            .current_dir(&project)
            .args(["app-server", "--listen", "stdio://"]);
        let runtime = Runtime::connect(
            AppServer::spawn(command, 1024).unwrap(),
            ClientOptions {
                capabilities: json!({"experimentalApi": true}),
                ..Default::default()
            },
            DEADLINE,
            1024,
        )
        .await
        .unwrap();
        Self {
            runtime,
            provider,
            directory,
        }
    }

    async fn create_thread(&self) -> String {
        let result = self.runtime.client().start_thread(json!({
            "cwd": self.directory.0.join("project"), "approvalPolicy": "on-request", "approvalsReviewer": "user", "sandbox": "read-only"
        }), DEADLINE).await.unwrap();
        result["thread"]["id"].as_str().unwrap().into()
    }

    async fn next(&mut self) -> RuntimeEvent {
        tokio::time::timeout(DEADLINE, self.runtime.next_event())
            .await
            .unwrap()
            .expect("Runtime closed before expected event")
    }

    fn trace(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.directory.0.join("trace.json")).unwrap())
            .unwrap()
    }

    async fn shutdown(&mut self) {
        self.runtime.shutdown().await.unwrap();
        self.provider.kill().await.unwrap();
    }
}

fn isolated_command(binary: &std::ffi::OsStr) -> Command {
    let mut command = Command::new(binary);
    command.env_clear().kill_on_drop(true);
    for name in ["PATH", "HOME", "SystemRoot", "USERPROFILE", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; CI runs this explicitly"]
async fn real_turn_events_history_resume_and_fork() {
    let mut harness = Harness::start("message").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    let turn = client
        .start_turn(
            &thread,
            vec![json!({"type": "text", "text": "Local fixture message"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    let turn_id = turn["turn"]["id"].as_str().unwrap();
    let mut methods = vec![];
    loop {
        let RuntimeEvent::Notification(event) = harness.next().await else {
            panic!("message fixture requested unexpected interaction")
        };
        let completed = event.method == "turn/completed";
        if completed {
            assert_eq!(event.raw["params"]["turn"]["status"], "completed");
            assert_eq!(event.raw["params"]["turn"]["id"], turn_id);
        }
        methods.push(event.method);
        if completed {
            break;
        }
    }
    for method in [
        "turn/started",
        "item/started",
        "item/agentMessage/delta",
        "item/completed",
        "thread/tokenUsage/updated",
    ] {
        assert!(
            methods.iter().any(|seen| seen == method),
            "missing {method}: {methods:?}"
        );
    }
    let read = client.read_thread(&thread, true, DEADLINE).await.unwrap();
    assert_eq!(read["thread"]["turns"][0]["id"], turn_id);
    let resumed = client
        .resume_thread(&thread, json!({}), DEADLINE)
        .await
        .unwrap();
    assert_eq!(resumed["thread"]["id"], thread);
    let forked = client
        .fork_thread(&thread, json!({}), DEADLINE)
        .await
        .unwrap();
    assert_ne!(forked["thread"]["id"], thread);
    let trace = harness.trace();
    assert_eq!(trace["requests"], 1);
    assert_eq!(trace["authorizationSeen"], false);
    harness.shutdown().await;
}

async fn real_approval(decision: ApprovalDecision, expect_marker: bool) {
    let mut harness = Harness::start("approval").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    client
        .start_turn(
            &thread,
            vec![json!({"type": "text", "text": "Isolated local approval fixture"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    let mut approved = false;
    let mut command_completed = false;
    loop {
        match harness.next().await {
            RuntimeEvent::Interaction(request) => {
                assert_eq!(request.kind, InteractionKind::CommandApproval);
                assert_eq!(request.thread_id(), Some(thread.as_str()));
                assert!(
                    !harness
                        .directory
                        .0
                        .join("project/caidex-tool-marker.txt")
                        .exists()
                );
                let queued = client.call("thread/queue/add", Some(json!({"threadId": thread, "clientUserMessageId": "fixture-queue", "input": [{"type": "text", "text": "Queue only, do not start"}]})), DEADLINE).await.unwrap();
                let queue_id = queued["queuedSubmission"]["id"]
                    .as_str()
                    .expect("native queue returned no ID");
                let listed = client
                    .call(
                        "thread/queue/list",
                        Some(json!({"threadId": thread})),
                        DEADLINE,
                    )
                    .await
                    .unwrap();
                assert_eq!(listed["data"][0]["id"], queue_id);
                client.call("thread/queue/update", Some(json!({"threadId": thread, "queuedSubmissionId": queue_id, "input": [{"type": "text", "text": "Updated fixture queue"}]})), DEADLINE).await.unwrap();
                let deleted = client
                    .call(
                        "thread/queue/delete",
                        Some(json!({"threadId": thread, "queuedSubmissionId": queue_id})),
                        DEADLINE,
                    )
                    .await
                    .unwrap();
                assert_eq!(deleted["deleted"], true);

                client.decide_approval(&request.id, decision).await.unwrap();
                approved = true;
            }
            RuntimeEvent::Notification(event) => {
                if event.method == "item/completed"
                    && event.raw["params"]["item"]["type"] == "commandExecution"
                {
                    command_completed = true;
                }
                if event.method == "turn/completed" {
                    assert_eq!(
                        event.raw["params"]["turn"]["status"],
                        if expect_marker {
                            "completed"
                        } else {
                            "interrupted"
                        }
                    );
                    break;
                }
            }
        }
    }
    assert!(approved, "real Runtime never requested approval");
    assert!(command_completed, "missing real command completion event");
    assert_eq!(
        harness
            .directory
            .0
            .join("project/caidex-tool-marker.txt")
            .exists(),
        expect_marker
    );
    let trace = harness.trace();
    assert_eq!(trace["requests"], if expect_marker { 2 } else { 1 });
    assert_eq!(trace["authorizationSeen"], false);
    assert_eq!(
        trace["toolOutputs"].as_array().unwrap().len(),
        usize::from(expect_marker)
    );
    if expect_marker {
        assert_eq!(
            std::fs::read_to_string(harness.directory.0.join("project/caidex-tool-marker.txt"))
                .unwrap()
                .trim(),
            "CAIDEX_TOOL_EXECUTED"
        );
    }
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; CI runs this explicitly"]
async fn real_command_approval_cancel_prevents_execution_and_preserves_native_queue() {
    real_approval(ApprovalDecision::Cancel, false).await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires pinned Linux Codex and loopback; executes only a temp marker command"]
async fn real_command_approval_accept_executes_only_the_temp_project_marker() {
    real_approval(ApprovalDecision::Accept, true).await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; CI runs this explicitly"]
async fn real_plan_mode_user_input_returns_answer_to_runtime_tool_loop() {
    let mut harness = Harness::start("questions").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    client.start_turn(&thread, vec![json!({"type": "text", "text": "Local question fixture"})], json!({
        "collaborationMode": {"mode": "plan", "settings": {"model": "gpt-5.1-codex", "reasoning_effort": null, "developer_instructions": null}}
    }), DEADLINE).await.unwrap();
    let mut answered = false;
    loop {
        match harness.next().await {
            RuntimeEvent::Interaction(request) => {
                assert_eq!(request.kind, InteractionKind::UserInput);
                assert_eq!(request.thread_id(), Some(thread.as_str()));
                assert_eq!(request.event.raw["params"]["questions"][0]["id"], "choice");
                client
                    .answer_questions(
                        &request.id,
                        HashMap::from([("choice".into(), vec!["B".into()])]),
                    )
                    .await
                    .unwrap();
                answered = true;
            }
            RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                assert_eq!(event.raw["params"]["turn"]["status"], "completed");
                break;
            }
            _ => {}
        }
    }
    assert!(answered);
    let trace = harness.trace();
    assert_eq!(trace["requests"], 2);
    assert_eq!(trace["authorizationSeen"], false);
    assert!(trace["toolOutputs"][0].as_str().unwrap().contains("B"));
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; CI runs this explicitly"]
async fn real_steer_precondition_and_interrupt_revoke_waiting_approval() {
    let mut harness = Harness::start("approval").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    let result = client
        .start_turn(
            &thread,
            vec![json!({"type": "text", "text": "Local steer and interrupt fixture"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    let turn = result["turn"]["id"].as_str().unwrap();
    let request_id = loop {
        if let RuntimeEvent::Interaction(request) = harness.next().await {
            assert_eq!(request.kind, InteractionKind::CommandApproval);
            break request.id;
        }
    };
    assert!(matches!(
        client
            .steer_turn(
                &thread,
                "stale-turn",
                vec![json!({"type": "text", "text": "stale"})],
                DEADLINE
            )
            .await,
        Err(Error::Rpc(_, _, _))
    ));
    let steered = client
        .steer_turn(
            &thread,
            turn,
            vec![json!({"type": "text", "text": "Steer existing turn"})],
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(steered["turnId"], turn);
    client
        .interrupt_turn(&thread, turn, DEADLINE)
        .await
        .unwrap();
    loop {
        if let RuntimeEvent::Notification(event) = harness.next().await
            && event.method == "turn/completed"
        {
            assert_eq!(event.raw["params"]["turn"]["status"], "interrupted");
            break;
        }
    }
    assert!(matches!(
        client
            .decide_approval(&request_id, ApprovalDecision::Accept)
            .await,
        Err(Error::NotPending)
    ));
    assert!(
        !harness
            .directory
            .0
            .join("project/caidex-tool-marker.txt")
            .exists()
    );
    assert_eq!(harness.trace()["requests"], 1);
    harness.shutdown().await;
}

async fn real_patch_approval(decision: ApprovalDecision, accept: bool) {
    let mut harness = Harness::start("patch").await;
    let client = harness.runtime.client();
    let started = client.start_thread(json!({
        "cwd": harness.directory.0.join("project"), "approvalPolicy": "on-request", "approvalsReviewer": "user", "sandbox": "workspace-write",
        // Bundled classic Responses metadata explicitly declares freeform patch.
        "model": "gpt-5.5",
        "config": {"sandbox_workspace_write.exclude_tmpdir_env_var": true, "sandbox_workspace_write.exclude_slash_tmp": true}
    }), DEADLINE).await.unwrap();
    let thread = started["thread"]["id"].as_str().unwrap().to_owned();
    client
        .start_turn(
            &thread,
            vec![json!({"type": "text", "text": "Local patch approval fixture"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    let marker = harness.directory.0.join("caidex-patch-marker.txt");
    let mut requested = false;
    let mut patch_completed = false;
    loop {
        match harness.next().await {
            RuntimeEvent::Interaction(request) => {
                assert_eq!(request.kind, InteractionKind::FileApproval);
                assert_eq!(request.thread_id(), Some(thread.as_str()));
                assert!(!marker.exists(), "patch ran before approval");
                client.decide_approval(&request.id, decision).await.unwrap();
                requested = true;
            }
            RuntimeEvent::Notification(event) => {
                if event.method == "item/completed"
                    && event.raw["params"]["item"]["type"] == "fileChange"
                {
                    patch_completed = true;
                    assert!(
                        !event.raw["params"]["item"]["changes"]
                            .as_array()
                            .unwrap()
                            .is_empty()
                    );
                }
                if event.method == "turn/completed" {
                    assert_eq!(
                        event.raw["params"]["turn"]["status"],
                        if accept { "completed" } else { "interrupted" },
                        "turn result: {}; fixture: {}",
                        event.raw["params"]["turn"],
                        harness.trace()
                    );
                    break;
                }
            }
        }
    }
    assert!(requested);
    assert_eq!(patch_completed, accept);
    assert_eq!(marker.exists(), accept);
    if accept {
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap().trim(),
            "CAIDEX_PATCH_APPLIED"
        );
    }
    let trace = harness.trace();
    assert_eq!(trace["requests"], if accept { 2 } else { 1 });
    assert_eq!(trace["authorizationSeen"], false);
    assert_eq!(trace["tool"], "apply_patch");
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; CI runs this explicitly"]
async fn real_patch_cancel_leaves_target_unchanged() {
    real_patch_approval(ApprovalDecision::Cancel, false).await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; writes only an approved temporary fixture"]
async fn real_patch_accept_writes_only_approved_temp_target_and_emits_changes() {
    real_patch_approval(ApprovalDecision::Accept, true).await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and local MCP fixture; CI runs this explicitly"]
async fn real_mcp_discovery_resource_tool_and_form_elicitation() {
    let mut harness = Harness::start("mcp").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    let inventory = client
        .call(
            "mcpServerStatus/list",
            Some(json!({"threadId": thread, "serverName": "fixture"})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(inventory["data"][0]["name"], "fixture", "{inventory}");
    assert!(
        inventory["data"][0]["tools"]
            .as_object()
            .unwrap()
            .keys()
            .any(|name| name.contains("choose"))
    );
    let resource = client
        .call(
            "mcpServer/resource/read",
            Some(json!({"threadId": thread, "server": "fixture", "uri": "fixture://marker"})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(
        resource["contents"][0]["text"], "CAIdex local MCP resource",
        "{resource}"
    );
    let echoed = client
        .call(
            "mcpServer/tool/call",
            Some(json!({"threadId": thread, "server": "fixture", "tool": "echo", "arguments": {}})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(echoed["content"][0]["text"], "CAIdex local MCP tool");
    assert_eq!(echoed["structuredContent"]["fixture"], true);
    assert_eq!(echoed["_meta"]["fixtureOpaque"], "preserve");
    for action in ["accept", "decline", "cancel"] {
        let content = if action == "accept" {
            json!({"choice": "B"})
        } else {
            Value::Null
        };
        let pending = {
            let client = client.clone();
            let thread = thread.clone();
            tokio::spawn(async move {
                client.call("mcpServer/tool/call", Some(json!({"threadId": thread, "server": "fixture", "tool": "choose", "arguments": {}})), DEADLINE).await
            })
        };
        loop {
            if let RuntimeEvent::Interaction(request) = harness.next().await {
                assert_eq!(request.kind, InteractionKind::McpElicitation);
                assert_eq!(request.thread_id(), Some(thread.as_str()));
                assert_eq!(request.event.raw["params"]["serverName"], "fixture");
                assert_eq!(request.event.raw["params"]["mode"], "form");
                assert_eq!(
                    request.event.raw["params"]["requestedSchema"]["required"],
                    json!(["choice"])
                );
                assert!(!pending.is_finished(), "MCP request answered automatically");
                client
                    .reply(
                        &request.id,
                        Ok(json!({"action": action, "content": content})),
                    )
                    .await
                    .unwrap();
                break;
            }
        }
        let chosen = pending.await.unwrap().unwrap();
        assert_eq!(chosen["structuredContent"]["action"], action);
        assert_eq!(chosen["structuredContent"]["content"], content);
        assert_eq!(chosen["_meta"]["fixtureOpaque"], "preserve");
    }
    let trace: Value =
        serde_json::from_slice(&std::fs::read(harness.directory.0.join("mcp-trace.json")).unwrap())
            .unwrap();
    assert_eq!(trace["initialized"], true);
    assert_eq!(
        trace["toolCalls"],
        json!(["echo", "choose", "choose", "choose"])
    );
    assert_eq!(trace["elicitationReplies"][0]["content"]["choice"], "B");
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and native PTY; CI runs this explicitly"]
async fn real_pty_stream_stdin_resize_and_exit_preserve_utf8_bytes() {
    let mut harness = Harness::start("message").await;
    let client = harness.runtime.client();
    let python = std::env::var("CAIDEX_TEST_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into());
    let script = "import sys,os; print('TTY:'+str(sys.stdin.isatty()),flush=True); print('CAIDEX_READY',flush=True); line=sys.stdin.readline().strip(); size=os.get_terminal_size(); print('REPLY:'+line+':UTF8:你好:SIZE:'+str(size.columns)+':'+str(size.lines),flush=True)";
    client.call("process/spawn", Some(json!({"processHandle": "fixture-pty", "command": [python, "-c", script], "cwd": harness.directory.0.join("project"), "env": {"PYTHONUTF8": "1"}, "tty": true, "size": {"cols": 80, "rows": 24}})), DEADLINE).await.unwrap();
    let mut output = vec![];
    loop {
        if let RuntimeEvent::Notification(event) = harness.next().await {
            if event.method == "process/outputDelta" {
                assert_eq!(event.raw["params"]["processHandle"], "fixture-pty");
                assert_eq!(event.raw["params"]["capReached"], false);
                output.extend(
                    STANDARD
                        .decode(event.raw["params"]["deltaBase64"].as_str().unwrap())
                        .unwrap(),
                );
                if String::from_utf8_lossy(&output).contains("CAIDEX_READY") {
                    break;
                }
            } else if event.method == "process/exited" {
                panic!("PTY exited before ready: {}", event.raw);
            }
        }
    }
    assert!(String::from_utf8_lossy(&output).contains("TTY:True"));
    client
        .call(
            "process/resizePty",
            Some(json!({"processHandle": "fixture-pty", "size": {"cols": 100, "rows": 40}})),
            DEADLINE,
        )
        .await
        .unwrap();
    client.call("process/writeStdin", Some(json!({"processHandle": "fixture-pty", "deltaBase64": STANDARD.encode(b"fixture input\n")})), DEADLINE).await.unwrap();
    loop {
        if let RuntimeEvent::Notification(event) = harness.next().await {
            if event.method == "process/outputDelta" {
                output.extend(
                    STANDARD
                        .decode(event.raw["params"]["deltaBase64"].as_str().unwrap())
                        .unwrap(),
                );
            } else if event.method == "process/exited" {
                assert_eq!(event.raw["params"]["processHandle"], "fixture-pty");
                assert_eq!(event.raw["params"]["exitCode"], 0);
                assert_eq!(event.raw["params"]["stdout"], "");
                break;
            }
        }
    }
    let text = String::from_utf8(output).unwrap();
    assert!(
        text.contains("REPLY:fixture input:UTF8:你好:SIZE:100:40"),
        "{text}"
    );
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and local child process; CI runs this explicitly"]
async fn real_long_running_process_rejects_duplicate_handle_and_stops_on_explicit_kill() {
    let mut harness = Harness::start("message").await;
    let client = harness.runtime.client();
    let python = std::env::var("CAIDEX_TEST_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into());
    let params = json!({"processHandle": "fixture-running", "command": [python, "-c", "import sys; print('CAIDEX_PROCESS_READY',flush=True); sys.stdin.readline()"], "cwd": harness.directory.0.join("project"), "streamStdin": true, "streamStdoutStderr": true, "timeoutMs": 30000});
    client
        .call("process/spawn", Some(params.clone()), DEADLINE)
        .await
        .unwrap();
    let mut output = vec![];
    loop {
        if let RuntimeEvent::Notification(event) = harness.next().await
            && event.method == "process/outputDelta"
        {
            output.extend(
                STANDARD
                    .decode(event.raw["params"]["deltaBase64"].as_str().unwrap())
                    .unwrap(),
            );
            if String::from_utf8_lossy(&output).contains("CAIDEX_PROCESS_READY") {
                break;
            }
        }
    }
    assert!(matches!(
        client.call("process/spawn", Some(params), DEADLINE).await,
        Err(Error::Rpc(_, _, _))
    ));
    client
        .call(
            "process/kill",
            Some(json!({"processHandle": "fixture-running"})),
            DEADLINE,
        )
        .await
        .unwrap();
    loop {
        if let RuntimeEvent::Notification(event) = harness.next().await
            && event.method == "process/exited"
        {
            assert_eq!(event.raw["params"]["processHandle"], "fixture-running");
            assert_ne!(event.raw["params"]["exitCode"], 0);
            break;
        }
    }
    assert!(matches!(client.call("process/writeStdin", Some(json!({"processHandle": "fixture-running", "deltaBase64": STANDARD.encode(b"late\n")})), DEADLINE).await, Err(Error::Rpc(_, _, _))));
    harness.shutdown().await;
}
