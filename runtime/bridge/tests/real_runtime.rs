//! Real pinned app-server and scripted loopback Responses; no paid model or key.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
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

impl TestDirectory {
    fn create(timestamp: u128) -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "caidex-runtime-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        // Acquire ownership only after creation succeeds; a failed create must
        // never run Drop against another fixture's existing directory.
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

#[test]
fn runtime_fixture_directories_are_independent_even_at_the_same_clock_tick() {
    let first = TestDirectory::create(0);
    let second = TestDirectory::create(0);
    assert_ne!(first.0, second.0);
    drop(first);
    assert!(second.0.is_dir());
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Harness {
    runtime: Runtime,
    provider: Child,
    directory: TestDirectory,
    gateway: Option<caidex_model_gateway::RunningGateway>,
    credential_reads: Arc<AtomicU64>,
}

struct GatewayFixtureStore(Arc<AtomicU64>);
impl caidex_credentials::SecretStore for GatewayFixtureStore {
    fn get(
        &self,
        _: &caidex_credentials::CredentialRef,
    ) -> caidex_credentials::Result<Option<caidex_credentials::Secret>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        caidex_credentials::Secret::new("CAIDEX_GATEWAY_PROVIDER_TEST_KEY".into()).map(Some)
    }
    fn set(
        &self,
        _: &caidex_credentials::CredentialRef,
        _: &caidex_credentials::Secret,
    ) -> caidex_credentials::Result<()> {
        unreachable!("fixture is read-only")
    }
    fn remove(&self, _: &caidex_credentials::CredentialRef) -> caidex_credentials::Result<bool> {
        unreachable!("fixture is read-only")
    }
}

impl Harness {
    async fn start(mode: &str) -> Self {
        let through_gateway = mode.starts_with("gateway-");
        let fixture_mode = match mode {
            "gateway-google-basic-classic" => "native-google-basic-classic",
            "gateway-google-basic-lite" => "native-google-basic-lite",
            "gateway-google-classic" => "native-google-classic",
            "gateway-google-history-classic" | "gateway-google-history-lite" => {
                "native-google-history"
            }
            "gateway-google-tools-lite" => "native-google-tools-lite",
            "gateway-google-multi-lite" => "native-google-multi-lite",
            "gateway-google-mcp-classic" => "native-google-mcp",
            "gateway-google-lite" => "native-google-lite",
            "gateway-google-stall-classic" | "gateway-google-stall-lite" => "native-google-stall",
            "gateway-anthropic-classic" => "native-anthropic-classic",
            "gateway-anthropic-discovery-classic" => "native-anthropic-discovery",
            "gateway-anthropic-lite" => "native-anthropic-lite",
            "gateway-anthropic-tools-lite" => "native-anthropic-tools-lite",
            "gateway-anthropic-stall-classic" | "gateway-anthropic-stall-lite" => {
                "native-anthropic-stall"
            }
            "gateway-classic" | "gateway-openai-classic" => "wire-classic",
            "gateway-lite" | "gateway-openai-lite" => "wire-lite",
            "gateway-stall-classic"
            | "gateway-stall-lite"
            | "gateway-openai-stall-classic"
            | "gateway-openai-stall-lite" => "wire-stall",
            _ => mode,
        };
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
        let directory = TestDirectory::create(unique);
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
            .arg(fixture_mode)
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
        let google_catalog = matches!(
            mode,
            "gateway-google-basic-classic"
                | "gateway-google-basic-lite"
                | "gateway-google-history-classic"
                | "gateway-google-history-lite"
                | "gateway-google-tools-lite"
                | "gateway-google-multi-lite"
                | "gateway-google-mcp-classic"
                | "gateway-google-stall-lite"
                | "gateway-google-stall-classic"
        );
        let model = if google_catalog {
            if mode.ends_with("-lite") {
                "caidex-google-lite-fixture"
            } else {
                "caidex-google-classic-fixture"
            }
        } else {
            match (fixture_mode, mode) {
                (
                    "wire-lite"
                    | "wire-anthropic-lite"
                    | "native-anthropic-lite"
                    | "native-anthropic-tools-lite"
                    | "native-google-lite",
                    _,
                )
                | (
                    _,
                    "gateway-stall-lite"
                    | "gateway-openai-stall-lite"
                    | "gateway-anthropic-stall-lite",
                ) => "gpt-6.1-sol",
                (
                    "wire-classic"
                    | "wire-stall"
                    | "wire-anthropic-classic"
                    | "native-anthropic-classic"
                    | "native-anthropic-discovery"
                    | "native-anthropic-stall"
                    | "native-google-classic",
                    _,
                ) => "gpt-5.5",
                _ => "gpt-5.1-codex",
            }
        };
        let credential_reads = Arc::new(AtomicU64::new(0));
        let gateway = if through_gateway {
            use caidex_credentials::{Broker, CredentialRef, Id, SecretKind};
            use caidex_model_core::ResponsesDialect;
            use caidex_model_gateway::{CustomResponses, Limits, ModelRoute};
            let native_openai = mode.starts_with("gateway-openai-");
            let native_anthropic = mode.starts_with("gateway-anthropic-");
            let native_google = mode.starts_with("gateway-google-");
            let owner = Id::new("fixture-host").unwrap();
            let credential = CredentialRef {
                owner: owner.clone(),
                provider: Id::new(if native_openai {
                    "openai"
                } else if native_anthropic {
                    "anthropic"
                } else if native_google {
                    "google"
                } else {
                    "custom"
                })
                .unwrap(),
                profile: Id::new("fixture").unwrap(),
                kind: SecretKind::ApiKey,
            };
            let broker = Arc::new(Broker::new(
                owner,
                GatewayFixtureStore(credential_reads.clone()),
            ));
            if native_google {
                use caidex_provider_google::{
                    GeminiClient, GeminiConfig, GeminiModel, GeminiProvider, ReasoningMapping,
                    SummaryMapping, ThinkingContext, VerbosityMapping,
                };
                let config = GeminiConfig::new(credential)
                    .unwrap()
                    .with_base_url(&format!("http://127.0.0.1:{port}/v1beta"))
                    .unwrap()
                    .with_local_runtime_context();
                let client = GeminiClient::new(config, broker.clone(), Limits::default()).unwrap();
                let mut profile = GeminiModel::new(
                    caidex_model_core::ModelMetadata::configured(
                        model.into(),
                        "models/native-fixture".into(),
                        vec![ResponsesDialect::Classic, ResponsesDialect::Lite],
                    ),
                    4096,
                    100,
                );
                profile.retain_runtime_metadata = true;
                profile.enforce_single_tool_call = matches!(
                    mode,
                    "gateway-google-history-lite"
                        | "gateway-google-tools-lite"
                        | "gateway-google-multi-lite"
                        | "gateway-google-stall-lite"
                );
                profile.verbosity_mappings = vec![
                    VerbosityMapping::new(
                        "low".into(),
                        "Keep user-facing answers concise while preserving required detail.".into(),
                    )
                    .unwrap(),
                ];
                profile.thinking_context = Some(ThinkingContext::AllTurns);
                profile.summary_mappings = vec![SummaryMapping::new("auto".into(), true).unwrap()];
                profile.reasoning_mappings = ["low", "medium", "high", "xhigh"]
                    .into_iter()
                    .map(|effort| {
                        ReasoningMapping::new(effort.into(), json!({"thinkingBudget":1024}))
                            .unwrap()
                    })
                    .collect();
                let provider = Arc::new(GeminiProvider::new(client, vec![profile], 10).unwrap());
                Some(
                    caidex_model_gateway::start_with_provider(
                        provider,
                        broker.redactor(),
                        Limits::default(),
                    )
                    .await
                    .unwrap(),
                )
            } else if native_anthropic {
                use caidex_provider_anthropic::{
                    AnthropicClient, AnthropicConfig, AnthropicModel, AnthropicProvider,
                    ReasoningMapping, SummaryMapping, ThinkingContext, VerbosityMapping,
                };
                let mut config = AnthropicConfig::new(credential)
                    .unwrap()
                    .with_base_url(&format!("http://127.0.0.1:{port}/v1"))
                    .unwrap()
                    .with_local_runtime_context()
                    .with_thinking_binding_controls()
                    .with_expected_organization("org-fixture")
                    .unwrap();
                if mode == "gateway-anthropic-discovery-classic" {
                    config = config.with_inline_tools();
                }
                let client =
                    AnthropicClient::new(config, broker.clone(), Limits::default()).unwrap();
                let mut profile = AnthropicModel::new(
                    caidex_model_core::ModelMetadata::configured(
                        model.into(),
                        "native-fixture".into(),
                        vec![ResponsesDialect::Classic, ResponsesDialect::Lite],
                    ),
                    4096,
                    100,
                );
                profile.retain_runtime_metadata = true;
                profile.verbosity_mappings = vec![
                    VerbosityMapping::new(
                        "low".into(),
                        "Keep user-facing answers concise while preserving required detail.".into(),
                    )
                    .unwrap(),
                ];
                profile.supports_system_messages = true;
                profile.supports_tool_discovery = mode == "gateway-anthropic-discovery-classic";
                profile.thinking_context = Some(ThinkingContext::AllTurns);
                profile.summary_mappings =
                    vec![SummaryMapping::new("auto".into(), "summarized".into()).unwrap()];
                profile.reasoning_mappings = ["low","medium","high","xhigh"].into_iter().map(|effort|ReasoningMapping::new(effort.into(),Some(effort.into()),Some(json!({"type":"adaptive","block_binding":{"prefix_mismatch_behavior":"error"}}))).unwrap()).collect();
                let provider = Arc::new(AnthropicProvider::new(client, vec![profile], 10).unwrap());
                Some(
                    caidex_model_gateway::start_with_provider(
                        provider,
                        broker.redactor(),
                        Limits::default(),
                    )
                    .await
                    .unwrap(),
                )
            } else if native_openai {
                use caidex_provider_openai::{OpenAiConfig, OpenAiProvider};
                let config = OpenAiConfig::new(credential)
                    .unwrap()
                    .with_base_url(&format!("http://127.0.0.1:{port}/v1"))
                    .unwrap()
                    .with_scope(Some("org-fixture"), Some("proj-fixture"))
                    .unwrap();
                let provider = Arc::new(
                    OpenAiProvider::new(
                        config,
                        vec![caidex_model_core::ModelMetadata::configured(
                            model.into(),
                            model.into(),
                            vec![ResponsesDialect::Classic, ResponsesDialect::Lite],
                        )],
                        broker.clone(),
                        Limits::default(),
                    )
                    .unwrap(),
                );
                Some(
                    caidex_model_gateway::start_with_provider(
                        provider,
                        broker.redactor(),
                        Limits::default(),
                    )
                    .await
                    .unwrap(),
                )
            } else {
                let adapter = CustomResponses::new(
                    &format!("http://127.0.0.1:{port}/v1/responses"),
                    Some(credential),
                )
                .unwrap();
                Some(
                    caidex_model_gateway::start(
                        vec![
                            ModelRoute::new(
                                model.into(),
                                model.into(),
                                vec![ResponsesDialect::Classic, ResponsesDialect::Lite],
                                adapter,
                            )
                            .unwrap(),
                        ],
                        broker,
                        Limits::default(),
                    )
                    .await
                    .unwrap(),
                )
            }
        } else {
            None
        };
        let port = gateway
            .as_ref()
            .map_or(port, |gateway| u64::from(gateway.address().port()));
        let authentication = if through_gateway {
            "env_key = \"CAIDEX_GATEWAY_TEST_TOKEN\"\n"
        } else {
            ""
        };
        // Explicit fixture scope: native Anthropic has no verified equivalent
        // for Codex cached web search. Never filter it inside the Gateway.
        let web_search = if matches!(
            mode,
            "gateway-anthropic-discovery-classic"
                | "gateway-google-history-classic"
                | "gateway-google-history-lite"
                | "gateway-google-tools-lite"
                | "gateway-google-multi-lite"
                | "gateway-google-mcp-classic"
                | "gateway-google-stall-lite"
                | "gateway-google-stall-classic"
        ) {
            "web_search = \"disabled\"\n"
        } else {
            ""
        };
        // Executor-owned catalog declares unsupported client tool search. This
        // uses the fixed Runtime's public config, never strips Gateway tools.
        let google_summary = if mode.starts_with("gateway-google-") {
            "model_reasoning_summary = \"auto\"\n"
        } else {
            ""
        };
        let catalog = if google_catalog {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/google_model_catalog.json");
            format!("model_catalog_json = {}\n", json!(path))
        } else {
            String::new()
        };
        std::fs::write(data.join("config.toml"), format!(
            "{catalog}{google_summary}model = \"{model}\"\nmodel_provider = \"caidex_fixture\"\n{web_search}[model_providers.caidex_fixture]\nname = \"CAIdex local protocol fixture\"\nbase_url = \"http://127.0.0.1:{port}/v1\"\nwire_api = \"responses\"\n{authentication}requires_openai_auth = false\nrequest_max_retries = 0\nstream_max_retries = 0\n[analytics]\nenabled = false\n"
        )).unwrap();
        if mode.starts_with("goal-") {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(data.join("config.toml"))
                .unwrap()
                .write_all(b"\n[features]\ngoals = true\n")
                .unwrap();
        }
        if matches!(
            mode,
            "mcp" | "gateway-anthropic-discovery-classic" | "gateway-google-mcp-classic"
        ) {
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
        if let Some(gateway) = &gateway {
            command.env("CAIDEX_GATEWAY_TEST_TOKEN", gateway.token().expose());
        }
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
            gateway,
            credential_reads,
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
        if let Some(gateway) = self.gateway.take() {
            gateway.shutdown().await.unwrap();
        }
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

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; CI runs this explicitly"]
async fn real_queue_reorder_busy_start_and_interrupt_preserve_native_identity() {
    let mut harness = Harness::start("queue").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    let first_turn = client
        .start_turn(
            &thread,
            vec![json!({"type": "text", "text": "Hold queue fixture at approval"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    loop {
        if let RuntimeEvent::Interaction(request) = harness.next().await {
            assert_eq!(request.kind, InteractionKind::CommandApproval);
            break;
        }
    }
    let mut submissions = vec![];
    for name in ["A", "B", "C"] {
        let added = client.call("thread/queue/add", Some(json!({"threadId": thread, "clientUserMessageId": name, "input": [{"type": "text", "text": name}]})), DEADLINE).await.unwrap();
        submissions.push(added["queuedSubmission"].clone());
    }
    assert!(matches!(
        client
            .call(
                "thread/queue/reorder",
                Some(json!({"threadId": thread, "queuedSubmissionIds": [submissions[0]["id"]]})),
                DEADLINE
            )
            .await,
        Err(Error::Rpc(_, _, _))
    ));
    let order = vec![
        submissions[2].clone(),
        submissions[0].clone(),
        submissions[1].clone(),
    ];
    client.call("thread/queue/reorder", Some(json!({"threadId": thread, "queuedSubmissionIds": order.iter().map(|v| v["id"].clone()).collect::<Vec<_>>()})), DEADLINE).await.unwrap();
    let page = client
        .call(
            "thread/queue/list",
            Some(json!({"threadId": thread, "limit": 1})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(page["data"], json!([order[0]]));
    assert!(page["nextCursor"].is_string());
    let rest = client
        .call(
            "thread/queue/list",
            Some(json!({"threadId": thread, "cursor": page["nextCursor"], "limit": 2})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(rest["data"], json!([order[1], order[2]]));
    assert_eq!(rest["nextCursor"], Value::Null);
    assert!(matches!(
        client
            .call(
                "thread/queue/start",
                Some(json!({"threadId": thread, "queuedSubmissionId": submissions[1]["id"]})),
                DEADLINE
            )
            .await,
        Err(Error::Rpc(_, _, _))
    ));
    client
        .interrupt_turn(
            &thread,
            first_turn["turn"]["id"].as_str().unwrap(),
            DEADLINE,
        )
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
    let preserved = client
        .call(
            "thread/queue/list",
            Some(json!({"threadId": thread})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(preserved["data"], json!(order));
    // Explicitly start B (non-head), interrupt it, then start C (head, no ID).
    for (selected, remaining, explicit) in [
        (&order[2], vec![order[0].clone(), order[1].clone()], true),
        (&order[0], vec![order[1].clone()], false),
    ] {
        let mut params = json!({"threadId": thread});
        if explicit {
            params["queuedSubmissionId"] = selected["id"].clone();
        }
        let started = client
            .call("thread/queue/start", Some(params), DEADLINE)
            .await
            .unwrap();
        let mut user_message = false;
        loop {
            match harness.next().await {
                RuntimeEvent::Notification(event) => {
                    if event.method == "item/started"
                        && event.raw["params"]["item"]["type"] == "userMessage"
                    {
                        assert_eq!(
                            event.raw["params"]["item"]["clientId"],
                            selected["clientUserMessageId"]
                        );
                        user_message = true;
                    }
                }
                RuntimeEvent::Interaction(request) => {
                    assert_eq!(request.kind, InteractionKind::CommandApproval);
                    assert_eq!(request.turn_id(), started["turn"]["id"].as_str());
                    break;
                }
            }
        }
        assert!(
            user_message,
            "queued client message identity was not emitted"
        );
        let listed = client
            .call(
                "thread/queue/list",
                Some(json!({"threadId": thread})),
                DEADLINE,
            )
            .await
            .unwrap();
        assert_eq!(listed["data"], json!(remaining));
        client
            .interrupt_turn(&thread, started["turn"]["id"].as_str().unwrap(), DEADLINE)
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
    }
    assert_eq!(harness.trace()["requests"], 3);
    assert!(
        !harness
            .directory
            .0
            .join("project/caidex-tool-marker.txt")
            .exists()
    );
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; CI runs this explicitly"]
async fn real_goal_pause_resume_budget_and_clear_are_runtime_owned() {
    let mut harness = Harness::start("goal-budget").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    let created = client.call("thread/goal/set", Some(json!({"threadId": thread, "objective": "Isolated budget fixture", "status": "paused", "tokenBudget": 10})), DEADLINE).await.unwrap();
    assert_eq!(created["goal"]["status"], "paused");
    assert_eq!(created["goal"]["tokensUsed"], 0);
    assert!(matches!(
        client
            .call(
                "thread/goal/set",
                Some(json!({"threadId": thread, "tokenBudget": -1})),
                DEADLINE
            )
            .await,
        Err(Error::Rpc(_, _, _))
    ));
    let paused = client
        .call(
            "thread/goal/get",
            Some(json!({"threadId": thread})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(paused["goal"], created["goal"]);
    let active = client
        .call(
            "thread/goal/set",
            Some(json!({"threadId": thread, "status": "active"})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(active["goal"]["objective"], "Isolated budget fixture");
    assert_eq!(active["goal"]["tokenBudget"], 10);
    let mut completed = false;
    let mut limited = false;
    while !completed || !limited {
        if let RuntimeEvent::Notification(event) = harness.next().await {
            if event.method == "turn/completed" {
                assert_eq!(event.raw["params"]["turn"]["status"], "completed");
                completed = true;
            }
            if event.method == "thread/goal/updated"
                && event.raw["params"]["goal"]["status"] == "budgetLimited"
            {
                limited = true;
            }
        }
    }
    let budget = client
        .call(
            "thread/goal/get",
            Some(json!({"threadId": thread})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(budget["goal"]["status"], "budgetLimited");
    assert!(budget["goal"]["tokensUsed"].as_i64().unwrap() >= 10);
    let cleared = client
        .call(
            "thread/goal/clear",
            Some(json!({"threadId": thread})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(cleared["cleared"], true);
    loop {
        if let RuntimeEvent::Notification(event) = harness.next().await
            && event.method == "thread/goal/cleared"
        {
            assert_eq!(event.raw["params"]["threadId"], thread);
            break;
        }
    }
    let absent = client
        .call(
            "thread/goal/get",
            Some(json!({"threadId": thread})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(absent["goal"], Value::Null);
    assert_eq!(harness.trace()["requests"], 1);
    assert_eq!(harness.trace()["authorizationSeen"], false);
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; CI runs this explicitly"]
async fn real_goal_empty_continuations_block_without_client_retry() {
    let mut harness = Harness::start("goal-empty").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    client
        .call(
            "thread/goal/set",
            Some(json!({"threadId": thread, "objective": "Isolated empty response fixture"})),
            DEADLINE,
        )
        .await
        .unwrap();
    let mut completed = 0;
    let mut blocked = false;
    while completed < 3 || !blocked {
        if let RuntimeEvent::Notification(event) = harness.next().await {
            if event.method == "turn/completed" {
                assert_eq!(event.raw["params"]["turn"]["status"], "completed");
                completed += 1;
            }
            if event.method == "thread/goal/updated"
                && event.raw["params"]["goal"]["status"] == "blocked"
            {
                blocked = true;
            }
        }
    }
    assert_eq!(completed, 3);
    let goal = client
        .call(
            "thread/goal/get",
            Some(json!({"threadId": thread})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(goal["goal"]["status"], "blocked");
    assert_eq!(harness.trace()["requests"], 3);
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; CI runs this explicitly"]
async fn real_manual_compaction_emits_lifecycle_and_carries_summary_forward() {
    let mut harness = Harness::start("compact").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    client
        .start_turn(
            &thread,
            vec![json!({"type": "text", "text": "Seed local compaction history"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    loop {
        if let RuntimeEvent::Notification(event) = harness.next().await
            && event.method == "turn/completed"
        {
            assert_eq!(event.raw["params"]["turn"]["status"], "completed");
            break;
        }
    }
    let result = client
        .call(
            "thread/compact/start",
            Some(json!({"threadId": thread})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(result, json!({}));
    let mut started = None;
    let mut finished = None;
    loop {
        if let RuntimeEvent::Notification(event) = harness.next().await {
            if event.raw["params"]["item"]["type"] == "contextCompaction" {
                if event.method == "item/started" {
                    started = Some(event.raw["params"]["item"]["id"].clone());
                }
                if event.method == "item/completed" {
                    finished = Some(event.raw["params"]["item"]["id"].clone());
                }
            }
            if event.method == "turn/completed" {
                assert_eq!(event.raw["params"]["turn"]["status"], "completed");
                break;
            }
        }
    }
    assert!(started.is_some());
    assert_eq!(started, finished);
    client
        .start_turn(
            &thread,
            vec![json!({"type": "text", "text": "Continue after compaction"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    loop {
        if let RuntimeEvent::Notification(event) = harness.next().await
            && event.method == "turn/completed"
        {
            assert_eq!(event.raw["params"]["turn"]["status"], "completed");
            break;
        }
    }
    let trace = harness.trace();
    assert_eq!(trace["requests"], 3);
    assert_eq!(trace["summarySeen"], json!([false, false, true]));
    assert_eq!(trace["authorizationSeen"], false);
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; CI runs this explicitly"]
async fn real_idle_queue_add_starts_a_turn_and_preserves_client_identity() {
    let mut harness = Harness::start("message").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    let added = client.call("thread/queue/add", Some(json!({"threadId": thread, "clientUserMessageId": "fixture-auto-queue", "input": [{"type": "text", "text": "Start via native queue"}]})), DEADLINE).await.unwrap();
    assert_eq!(
        added["queuedSubmission"]["clientUserMessageId"],
        "fixture-auto-queue"
    );
    let mut started = None;
    let mut user_message = false;
    loop {
        let RuntimeEvent::Notification(event) = harness.next().await else {
            panic!("message-only queue fixture requested interaction")
        };
        if event.method == "turn/started" {
            started = Some(event.raw["params"]["turn"]["id"].clone());
        }
        if event.method == "item/started" && event.raw["params"]["item"]["type"] == "userMessage" {
            assert_eq!(
                event.raw["params"]["item"]["clientId"],
                "fixture-auto-queue"
            );
            assert_eq!(
                event.raw["params"]["item"]["content"][0]["text"],
                "Start via native queue"
            );
            user_message = true;
        }
        if event.method == "turn/completed" {
            assert_eq!(event.raw["params"]["turn"]["status"], "completed");
            assert_eq!(Some(event.raw["params"]["turn"]["id"].clone()), started);
            break;
        }
    }
    assert!(user_message);
    let queue = client
        .call(
            "thread/queue/list",
            Some(json!({"threadId": thread})),
            DEADLINE,
        )
        .await
        .unwrap();
    assert_eq!(queue["data"], json!([]));
    assert_eq!(harness.trace()["requests"], 1);
    harness.shutdown().await;
}

async fn real_native_anthropic_history(mode: &str) {
    use caidex_provider_anthropic::NativeMessage;
    let mut harness = Harness::start(mode).await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    for text in [
        "Offline native history fixture",
        "Continue exact signed native history",
    ] {
        client
            .start_turn(
                &thread,
                vec![json!({"type":"text","text":text})],
                json!({}),
                DEADLINE,
            )
            .await
            .unwrap();
        loop {
            let RuntimeEvent::Notification(event) = harness.next().await else {
                panic!("native history fixture does not execute tools")
            };
            if event.method == "turn/completed" {
                assert_eq!(event.raw["params"]["turn"]["status"], "completed");
                break;
            }
        }
    }
    let trace = harness.trace();
    assert_eq!(trace["requests"], 2);
    for request in trace["wireRequests"].as_array().unwrap() {
        if mode == "wire-anthropic-lite" {
            assert_eq!(request["body"]["model"], "gpt-6.1-sol");
            assert_eq!(request["liteHeader"], "true");
            assert!(request["body"].get("instructions").is_none());
            assert!(request["body"].get("tools").is_none());
        } else {
            assert_eq!(request["body"]["model"], "gpt-5.5");
            assert_eq!(request["liteHeader"], Value::Null);
        }
    }
    let original: Vec<_> = trace["wireResponses"][0]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["type"] == "response.output_item.done")
        .map(|event| event["item"].clone())
        .collect();
    let native =
        NativeMessage::from_responses_output(&original, "native-fixture", 128 * 1024).unwrap();
    let followup = trace["wireRequests"][1]["body"]["input"]
        .as_array()
        .unwrap();
    let index = followup
        .iter()
        .position(|item| {
            item["type"] == "reasoning"
                && item["encrypted_content"] == original[0]["encrypted_content"]
        })
        .unwrap();
    let replay = NativeMessage::from_responses_output(
        &followup[index..index + original.len()],
        "native-fixture",
        128 * 1024,
    )
    .unwrap();
    assert_eq!(replay.wire(), native.wire());
    assert_eq!(replay.content()[0]["signature"], "signed+/==\n");
    assert_eq!(replay.content()[1]["data"], "opaque+/==\n");
    assert_eq!(
        replay.content()[3]["number"].to_string(),
        "18446744073709551616"
    );
    // Validate the Python fixture against the actual Rust projection contract.
    assert!(
        NativeMessage::from_responses_output(
            native.to_responses(128 * 1024).unwrap().output(),
            "native-fixture",
            128 * 1024
        )
        .is_ok()
    );
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; native carrier replay, not Anthropic Gateway integration"]
async fn real_classic_runtime_preserves_native_anthropic_signed_history_carrier() {
    real_native_anthropic_history("wire-anthropic-classic").await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; Lite native carrier replay, not Code Mode tool execution"]
async fn real_lite_runtime_preserves_native_anthropic_signed_history_carrier() {
    real_native_anthropic_history("wire-anthropic-lite").await;
}

async fn real_anthropic_adapter(mode: &str) {
    let mut harness = Harness::start(mode).await;
    let thread = harness.create_thread().await;
    for text in [
        "Offline native adapter fixture",
        "Continue exact native history",
    ] {
        harness
            .runtime
            .client()
            .start_turn(
                &thread,
                vec![json!({"type":"text","text":text})],
                json!({}),
                DEADLINE,
            )
            .await
            .unwrap();
        loop {
            let RuntimeEvent::Notification(event) = harness.next().await else {
                panic!("history fixture has no executable tools")
            };
            if event.method == "turn/completed" {
                assert_eq!(
                    event.raw["params"]["turn"]["status"], "completed",
                    "{}",
                    event.raw
                );
                break;
            }
        }
    }
    let trace = harness.trace();
    assert_eq!(trace["requests"], 2);
    assert_eq!(trace["organizationLookups"], 2);
    assert_eq!(trace["gatewayCredentialMatched"], true);
    assert_eq!(trace["authorizationSeen"], false);
    let requests = trace["nativeRequests"].as_array().unwrap();
    let first = &requests[0];
    let second = &requests[1];
    assert_eq!(first["model"], "native-fixture");
    assert_eq!(second["system"], first["system"]);
    assert_eq!(second["tools"], first["tools"]);
    let prefix = first["messages"].as_array().unwrap();
    let messages = second["messages"].as_array().unwrap();
    assert_eq!(&messages[..prefix.len()], prefix);
    assert_eq!(messages[prefix.len()]["role"], "assistant");
    assert_eq!(
        messages[prefix.len()]["content"],
        trace["nativeResponses"][0]["content"]
    );
    assert_eq!(
        messages[prefix.len()]["content"][0]["signature"],
        "signed+/==\n"
    );
    assert_eq!(messages[prefix.len()]["content"][1]["data"], "opaque+/==\n");
    assert_eq!(
        messages[prefix.len()]["content"][3]["number"].to_string(),
        "18446744073709551616"
    );
    for request in requests {
        for key in ["include", "client_metadata", "prompt_cache_key", "binding"] {
            assert!(request.get(key).is_none());
        }
        assert_eq!(request["thinking"]["type"], "adaptive");
    }
    // The real Runtime persisted the v3 output it received from the Gateway.
    let read = harness
        .runtime
        .client()
        .read_thread(&thread, true, DEADLINE)
        .await
        .unwrap();
    assert!(read.to_string().contains("CAIdex local fixture complete"));
    let path = PathBuf::from(
        read["thread"]["path"]
            .as_str()
            .expect("persisted rollout path"),
    );
    assert!(
        path.canonicalize()
            .unwrap()
            .starts_with(harness.directory.0.join("data").canonicalize().unwrap()),
        "rollout must remain inside the isolated CODEX_HOME: {}",
        path.display()
    );
    let rollout = std::fs::read_to_string(path).unwrap();
    let histories: Vec<Value> = rollout
        .lines()
        .filter_map(|line| {
            let entry: Value = serde_json::from_str(line).unwrap();
            let item = &entry["payload"];
            (entry["type"] == "response_item" && item["type"] == "reasoning").then(|| item.clone())
        })
        .collect();
    assert_eq!(histories.len(), 2);
    for item in histories {
        let capsule = item["encrypted_content"].as_str().unwrap();
        let envelope: Value = serde_json::from_str(
            capsule
                .strip_prefix("caidex.anthropic.native-message.v3:")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(envelope["binding"]["organization"], "org-fixture");
        assert_eq!(
            envelope["message"]["content"][0]["signature"],
            "signed+/==\n"
        );
        assert_eq!(
            envelope["message"]["content"][3]["number"].to_string(),
            "18446744073709551616"
        );
    }
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; unsupported cached web search must fail before native authentication"]
async fn real_classic_native_anthropic_rejects_cached_web_search_before_authentication() {
    let mut harness = Harness::start("gateway-anthropic-classic").await;
    let thread = harness.create_thread().await;
    harness
        .runtime
        .client()
        .start_turn(
            &thread,
            vec![json!({"type":"text","text":"Offline native adapter fixture"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    loop {
        match harness.next().await {
            RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                assert_eq!(event.raw["params"]["turn"]["status"], "failed");
                assert!(
                    event.raw["params"]["turn"]["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("unsupported_anthropic_web_search")
                );
                break;
            }
            RuntimeEvent::Interaction(request) => {
                panic!("unsupported request executed a tool: {}", request.event.raw)
            }
            _ => (),
        }
    }
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 0);
    assert_eq!(harness.trace()["requests"], 0);
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and local MCP; explicit web_search=disabled, not full classic compatibility"]
async fn real_classic_native_anthropic_discovers_and_executes_mcp_tools() {
    let mut harness = Harness::start("gateway-anthropic-discovery-classic").await;
    let thread = harness.create_thread().await;
    harness
        .runtime
        .client()
        .start_turn(
            &thread,
            vec![json!({"type":"text","text":"Find the local fixture echo tool and invoke it"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    loop {
        match harness.next().await {
            RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                assert_eq!(
                    event.raw["params"]["turn"]["status"], "completed",
                    "{}",
                    event.raw
                );
                break;
            }
            RuntimeEvent::Interaction(request) => {
                panic!("unexpected interaction: {}", request.event.raw)
            }
            _ => (),
        }
    }
    let trace = harness.trace();
    assert_eq!(trace["requests"], 3);
    assert_eq!(trace["organizationLookups"], 3);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 3);
    let requests = trace["nativeRequests"].as_array().unwrap();
    for request in &requests[1..] {
        assert_eq!(request["tools"], requests[0]["tools"]);
        assert_eq!(request["system"], requests[0]["system"]);
    }
    let results: Vec<_> = requests[2]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|message| message["content"].as_array().unwrap())
        .filter(|block| block["type"] == "tool_result")
        .collect();
    assert_eq!(results.len(), 2);
    let discovery: Value =
        serde_json::from_str(results[0]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(discovery["type"], "tool_search_output");
    assert_eq!(discovery["execution"], "client");
    assert!(discovery["tools"].to_string().contains("mcp__fixture"));
    // Fixed Runtime formats structuredContent as model-facing JSON instead of
    // the MCP display text; preserve the actual Runtime result verbatim.
    let result_text = results[1]["content"][0]["text"].as_str().unwrap();
    let structured: Value =
        serde_json::from_str(result_text.split_once("\nOutput:\n").unwrap().1).unwrap();
    assert_eq!(structured, json!({"fixture":true}));
    let mcp: Value =
        serde_json::from_slice(&std::fs::read(harness.directory.0.join("mcp-trace.json")).unwrap())
            .unwrap();
    assert_eq!(mcp["toolCalls"], json!(["echo"]));
    let read = harness
        .runtime
        .client()
        .read_thread(&thread, true, DEADLINE)
        .await
        .unwrap();
    let rollout = std::fs::read_to_string(read["thread"]["path"].as_str().unwrap()).unwrap();
    let output = rollout
        .lines()
        .filter_map(|line| {
            let entry: Value = serde_json::from_str(line).unwrap();
            let item = &entry["payload"];
            (entry["type"] == "response_item"
                && item["type"] == "function_call_output"
                && item["call_id"] == "native-discovery-echo")
                .then(|| item["output"].clone())
        })
        .next()
        .unwrap();
    assert_eq!(output, result_text);
    assert!(rollout.contains("caidex.anthropic.native-message.v4:"));
    assert!(rollout.contains("tool_search_call"));
    assert!(rollout.contains("tool_search_output"));
    assert_eq!(trace["authorizationSeen"], false);
    // Restart the actual app-server so resume must load persisted history,
    // instead of reusing the first process's in-memory discovery state.
    harness.runtime.shutdown().await.unwrap();
    let binary = std::env::var_os("CAIDEX_CODEX_BIN").unwrap_or_else(|| "codex".into());
    let mut command = isolated_command(&binary);
    command
        .env("CODEX_HOME", harness.directory.0.join("data"))
        .env(
            "CAIDEX_GATEWAY_TEST_TOKEN",
            harness.gateway.as_ref().unwrap().token().expose(),
        )
        .current_dir(harness.directory.0.join("project"))
        .args(["app-server", "--listen", "stdio://"]);
    harness.runtime = Runtime::connect(
        AppServer::spawn(command, 1024).unwrap(),
        ClientOptions {
            capabilities: json!({"experimentalApi":true}),
            ..Default::default()
        },
        DEADLINE,
        1024,
    )
    .await
    .unwrap();
    let resumed = harness
        .runtime
        .client()
        .resume_thread(&thread, json!({}), DEADLINE)
        .await
        .unwrap();
    assert_eq!(resumed["thread"]["id"], thread);
    harness.runtime.client().start_turn(&thread,
        vec![json!({"type":"text","text":"Continue exact discovered-tool history after restart"})],
        json!({}),DEADLINE).await.unwrap();
    loop {
        match harness.next().await {
            RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                assert_eq!(
                    event.raw["params"]["turn"]["status"], "completed",
                    "{}",
                    event.raw
                );
                break;
            }
            RuntimeEvent::Interaction(request) => {
                panic!("resume repeated a tool: {}", request.event.raw)
            }
            _ => (),
        }
    }
    let resumed_trace = harness.trace();
    assert_eq!(resumed_trace["requests"], 4);
    assert_eq!(resumed_trace["organizationLookups"], 4);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 4);
    let fourth = &resumed_trace["nativeRequests"][3];
    assert_eq!(fourth["tools"], requests[0]["tools"]);
    assert_eq!(fourth["system"], requests[0]["system"]);
    let third_prefix = requests[2]["messages"].as_array().unwrap();
    assert_eq!(
        &fourth["messages"].as_array().unwrap()[..third_prefix.len()],
        third_prefix
    );
    let mcp: Value =
        serde_json::from_slice(&std::fs::read(harness.directory.0.join("mcp-trace.json")).unwrap())
            .unwrap();
    assert_eq!(
        mcp["toolCalls"],
        json!([]),
        "restarted MCP server must not repeat echo"
    );
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; real Anthropic Gateway Lite two-turn, no commercial model"]
async fn real_lite_runtime_via_native_anthropic_adapter() {
    real_anthropic_adapter("gateway-anthropic-lite").await;
}

async fn real_model_wire(mode: &str, dialect: caidex_model_core::ResponsesDialect) {
    use caidex_model_core::{
        CanonicalRequest, ResponseItem, ResponsesDialect, ResponsesStream, StreamState,
    };
    let mut harness = Harness::start(mode).await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    for text in [
        "Offline model boundary fixture",
        "Continue with opaque reasoning history",
    ] {
        client
            .start_turn(
                &thread,
                vec![json!({"type":"text", "text":text})],
                json!({}),
                DEADLINE,
            )
            .await
            .unwrap();
        loop {
            let RuntimeEvent::Notification(event) = harness.next().await else {
                panic!("wire fixture must not request a tool or permission")
            };
            if event.method == "turn/completed" {
                assert_eq!(event.raw["params"]["turn"]["status"], "completed");
                break;
            }
        }
    }
    let trace = harness.trace();
    assert_eq!(trace["requests"], 2);
    assert_eq!(trace["authorizationSeen"], mode.starts_with("gateway-"));
    assert_eq!(
        trace["gatewayCredentialMatched"],
        mode.starts_with("gateway-")
    );
    let requests = trace["wireRequests"].as_array().unwrap();
    for request in requests {
        assert_eq!(request["accept"], "text/event-stream");
        if mode.starts_with("gateway-openai-") {
            assert_eq!(request["organization"], "org-fixture");
            assert_eq!(request["project"], "proj-fixture");
        }
        let body = &request["body"];
        let normalized = CanonicalRequest::new(body.clone(), dialect).unwrap();
        assert!(normalized.is_streaming());
        assert_eq!(serde_json::to_value(&normalized).unwrap(), *body);
        assert_eq!(body["store"], false);
        assert!(
            body["include"]
                .as_array()
                .unwrap()
                .contains(&json!("reasoning.encrypted_content"))
        );
        match dialect {
            ResponsesDialect::Classic => {
                assert_eq!(normalized.model(), "gpt-5.5");
                assert_eq!(request["liteHeader"], Value::Null);
                assert!(body["instructions"].is_string());
                assert!(body["tools"].is_array());
            }
            ResponsesDialect::Lite => {
                assert_eq!(normalized.model(), "gpt-6.1-sol");
                assert_eq!(request["liteHeader"], "true");
                assert!(body.get("instructions").is_none());
                assert!(body.get("tools").is_none());
                assert_eq!(body["parallel_tool_calls"], false);
                assert_eq!(body["reasoning"]["context"], "all_turns");
                assert_eq!(body["input"][0]["type"], "additional_tools");
                assert_eq!(body["input"][0]["role"], "developer");
                assert!(body["input"][0]["id"].as_str().unwrap().starts_with("at_"));
                assert!(body["input"][0]["tools"].is_array());
                assert!(body["input"][1]["id"].as_str().unwrap().starts_with("msg_"));
            }
        }
    }
    let followup = &requests[1]["body"]["input"];
    assert!(
        followup
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "reasoning"
                && item["encrypted_content"] == "CAIDEX_OPAQUE_REASONING+/==")
    );
    if dialect == ResponsesDialect::Lite {
        assert_eq!(&requests[0]["body"]["input"][0], &followup[0]);
        assert_eq!(&requests[0]["body"]["input"][1], &followup[1]);
    }
    for response in trace["wireResponses"].as_array().unwrap() {
        let mut stream = ResponsesStream::new(16 * 1024).unwrap();
        let mut decoded = Vec::new();
        for raw in response.as_array().unwrap() {
            let frame = format!("event: {}\ndata: {raw}\n\n", raw["type"].as_str().unwrap());
            for chunk in frame.as_bytes().chunks(3) {
                decoded.extend(stream.push(chunk).unwrap());
            }
        }
        assert_eq!(stream.finish().unwrap(), StreamState::Completed);
        assert_eq!(decoded.len(), response.as_array().unwrap().len());
        for (event, raw) in decoded.iter().zip(response.as_array().unwrap()) {
            assert_eq!(event.response.wire(), raw);
        }
        let reasoning = decoded
            .iter()
            .find_map(|event| {
                event
                    .response
                    .item()
                    .unwrap()
                    .filter(|item| item.kind() == "reasoning")
            })
            .unwrap();
        assert_eq!(
            reasoning.wire()["provider_signature"],
            "CAIDEX_FUTURE_SIGNATURE=="
        );
        assert_eq!(
            serde_json::to_value(ResponseItem::new(reasoning.wire().clone()).unwrap()).unwrap(),
            *reasoning.wire()
        );
    }
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; validates classic wire with synthetic reasoning"]
async fn real_classic_model_wire_matches_core_and_replays_opaque_reasoning() {
    real_model_wire("wire-classic", caidex_model_core::ResponsesDialect::Classic).await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; validates Lite wire, not Code Mode tool execution"]
async fn real_lite_model_wire_matches_core_and_keeps_stable_prefix() {
    real_model_wire("wire-lite", caidex_model_core::ResponsesDialect::Lite).await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; classic two-turn via authenticated Gateway"]
async fn real_classic_runtime_via_gateway_with_executor_credential() {
    real_model_wire(
        "gateway-classic",
        caidex_model_core::ResponsesDialect::Classic,
    )
    .await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; Lite two-turn via authenticated Gateway"]
async fn real_lite_runtime_via_gateway_with_executor_credential() {
    real_model_wire("gateway-lite", caidex_model_core::ResponsesDialect::Lite).await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; native OpenAI adapter, synthetic two-turn classic fixture"]
async fn real_classic_runtime_via_native_openai_adapter() {
    real_model_wire(
        "gateway-openai-classic",
        caidex_model_core::ResponsesDialect::Classic,
    )
    .await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; native OpenAI adapter, synthetic two-turn Lite fixture"]
async fn real_lite_runtime_via_native_openai_adapter() {
    real_model_wire(
        "gateway-openai-lite",
        caidex_model_core::ResponsesDialect::Lite,
    )
    .await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and loopback; interrupt closes classic/Lite Gateway upstream socket"]
async fn real_runtime_interrupt_via_gateway_closes_provider_socket() {
    interrupt_gateway(&["gateway-stall-classic", "gateway-stall-lite"]).await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; native OpenAI classic/Lite interrupt closes actual socket"]
async fn real_runtime_interrupt_via_native_openai_closes_provider_socket() {
    interrupt_gateway(&["gateway-openai-stall-classic", "gateway-openai-stall-lite"]).await;
}

async fn interrupt_gateway(modes: &[&str]) {
    for mode in modes {
        let mut harness = Harness::start(mode).await;
        let thread = harness.create_thread().await;
        let client = harness.runtime.client();
        let turn = client
            .start_turn(
                &thread,
                vec![json!({"type":"text", "text":"Offline cancellable Gateway fixture"})],
                json!({}),
                DEADLINE,
            )
            .await
            .unwrap();
        // Markers are written after trace updates, so polling never reads a
        // partially written JSON file or relies on a speculative timer.
        let streaming = harness.directory.0.join("gateway-streaming");
        tokio::time::timeout(DEADLINE, async {
            while !streaming.exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(harness.trace()["gatewayCredentialMatched"], true);
        client
            .interrupt_turn(&thread, turn["turn"]["id"].as_str().unwrap(), DEADLINE)
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
        let disconnected = harness.directory.0.join("gateway-disconnected");
        tokio::time::timeout(DEADLINE, async {
            while !disconnected.exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(harness.trace()["gatewayDisconnected"], true);
        assert_eq!(harness.trace()["requests"], 1);
        harness.shutdown().await;
    }
}

#[tokio::test]
#[ignore = "requires pinned Codex; native Anthropic Lite Code Mode tool with approved temp marker"]
async fn real_lite_native_anthropic_code_mode_executes_tool_and_replays_result() {
    let mut harness = Harness::start("gateway-anthropic-tools-lite").await;
    let thread = harness.create_thread().await;
    let client = harness.runtime.client();
    client
        .start_turn(
            &thread,
            vec![json!({"type":"text","text":"Offline native Code Mode tool fixture"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    let marker = harness.directory.0.join("project/caidex-native-marker.txt");
    let mut approved = false;
    loop {
        match harness.next().await {
            RuntimeEvent::Interaction(request) => {
                assert_eq!(request.kind, InteractionKind::CommandApproval);
                assert!(!marker.exists());
                assert!(!approved);
                client
                    .decide_approval(&request.id, ApprovalDecision::Accept)
                    .await
                    .unwrap();
                approved = true;
            }
            RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                assert_eq!(
                    event.raw["params"]["turn"]["status"], "completed",
                    "{}",
                    event.raw
                );
                break;
            }
            _ => (),
        }
    }
    assert!(
        approved,
        "Code Mode must reach the real Runtime command approval"
    );
    assert_eq!(
        std::fs::read_to_string(marker).unwrap().trim(),
        "CAIDEX_NATIVE_CODE_MODE"
    );
    let trace = harness.trace();
    assert_eq!(trace["requests"], 2);
    assert_eq!(trace["organizationLookups"], 2);
    let messages = trace["nativeRequests"][1]["messages"].as_array().unwrap();
    let assistant = messages
        .iter()
        .find(|message| message["role"] == "assistant")
        .unwrap();
    assert_eq!(assistant["content"], trace["nativeResponses"][0]["content"]);
    let results: Vec<_> = messages
        .iter()
        .flat_map(|message| message["content"].as_array().unwrap())
        .filter(|block| block["type"] == "tool_result")
        .collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["tool_use_id"], "native-code-mode-one");
    assert!(
        results[0]["content"]
            .to_string()
            .contains("CAIDEX_NATIVE_CODE_MODE")
    );
    assert_eq!(trace["authorizationSeen"], false);
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; native Anthropic Lite interrupt closes actual upstream socket"]
async fn real_lite_native_anthropic_interrupt_closes_provider_socket() {
    interrupt_gateway(&["gateway-anthropic-stall-lite"]).await;
}

// Catches accepting fixed Runtime defaults that the native compiler cannot
// represent, or reading credentials/POSTing before rejecting those defaults.
#[tokio::test]
#[ignore = "requires pinned Codex; unsupported Gemini classic/Lite defaults reject before Key"]
async fn real_google_runtime_rejects_unsupported_defaults_before_authentication() {
    for (mode, code) in [
        (
            "gateway-google-classic",
            "unsupported_google_tool_discovery",
        ),
        (
            "gateway-google-lite",
            "unsupported_google_parallel_tool_calls",
        ),
        (
            "gateway-google-basic-classic",
            "unsupported_google_web_search",
        ),
        (
            "gateway-google-basic-lite",
            "unsupported_google_parallel_tool_calls",
        ),
    ] {
        let mut harness = Harness::start(mode).await;
        let thread = harness.create_thread().await;
        harness
            .runtime
            .client()
            .start_turn(
                &thread,
                vec![json!({"type":"text","text":"Offline native Gemini defaults fixture"})],
                json!({}),
                DEADLINE,
            )
            .await
            .unwrap();
        loop {
            match harness.next().await {
                RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                    assert_eq!(event.raw["params"]["turn"]["status"], "failed");
                    assert!(
                        event.raw["params"]["turn"]["error"]["message"]
                            .as_str()
                            .unwrap()
                            .contains(code),
                        "{}",
                        event.raw
                    );
                    break;
                }
                RuntimeEvent::Interaction(request) => panic!(
                    "unsupported native request executed a tool: {}",
                    request.event.raw
                ),
                _ => {}
            }
        }
        assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 0);
        assert_eq!(harness.trace()["requests"], 0);
        harness.shutdown().await;
    }
}

// Catches missing actual Runtime adapter/context integration, native signed
// Parts reconstructed from visible text, or a lost persisted v2 capsule.
#[tokio::test]
#[ignore = "requires pinned Codex; explicit web_search disabled, synthetic Gemini signed history"]
async fn real_classic_runtime_via_native_google_preserves_signed_history() {
    real_google_history("gateway-google-history-classic").await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; explicit Gemini single-call profile, Lite signed history and restart"]
async fn real_lite_runtime_via_native_google_preserves_signed_history() {
    real_google_history("gateway-google-history-lite").await;
}

async fn restart_google_runtime(harness: &mut Harness, thread: &str) {
    harness.runtime.shutdown().await.unwrap();
    let binary = std::env::var_os("CAIDEX_CODEX_BIN").unwrap_or_else(|| "codex".into());
    let mut command = isolated_command(&binary);
    command
        .env("CODEX_HOME", harness.directory.0.join("data"))
        .env(
            "CAIDEX_GATEWAY_TEST_TOKEN",
            harness.gateway.as_ref().unwrap().token().expose(),
        )
        .current_dir(harness.directory.0.join("project"))
        .args(["app-server", "--listen", "stdio://"]);
    harness.runtime = Runtime::connect(
        AppServer::spawn(command, 1024).unwrap(),
        ClientOptions {
            capabilities: json!({"experimentalApi":true}),
            ..Default::default()
        },
        DEADLINE,
        1024,
    )
    .await
    .unwrap();
    let resumed = harness
        .runtime
        .client()
        .resume_thread(thread, json!({}), DEADLINE)
        .await
        .unwrap();
    assert_eq!(resumed["thread"]["id"], thread);
}

async fn real_google_history(mode: &str) {
    let mut harness = Harness::start(mode).await;
    let thread = harness.create_thread().await;
    for (i, text) in [
        "Offline native Gemini fixture",
        "Continue exact native history",
        "Continue from disk after app-server restart",
    ]
    .into_iter()
    .enumerate()
    {
        if i == 2 {
            restart_google_runtime(&mut harness, &thread).await;
        }
        harness
            .runtime
            .client()
            .start_turn(
                &thread,
                vec![json!({"type":"text","text":text})],
                json!({}),
                DEADLINE,
            )
            .await
            .unwrap();
        let mut visible = String::new();
        loop {
            match harness.next().await {
                RuntimeEvent::Notification(event) if event.method == "item/agentMessage/delta" => {
                    visible.push_str(event.raw["params"]["delta"].as_str().unwrap());
                }
                RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                    assert_eq!(
                        event.raw["params"]["turn"]["status"], "completed",
                        "{}",
                        event.raw
                    );
                    break;
                }
                RuntimeEvent::Interaction(request) => panic!(
                    "history fixture has no executable tools: {}",
                    request.event.raw
                ),
                _ => {}
            }
        }
        assert_eq!(visible, "CAIdex local fixture complete");
    }
    let trace = harness.trace();
    assert_eq!(trace["requests"], 3);
    assert_eq!(trace["gatewayCredentialMatched"], true);
    assert_eq!(trace["authorizationSeen"], false);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 3);
    let requests = trace["nativeRequests"].as_array().unwrap();
    assert_eq!(
        requests[1]["systemInstruction"],
        requests[0]["systemInstruction"]
    );
    assert_eq!(requests[1]["tools"], requests[0]["tools"]);
    assert_eq!(requests[2]["tools"], requests[0]["tools"]);
    assert_eq!(
        requests[2]["systemInstruction"],
        requests[0]["systemInstruction"]
    );
    let second_prefix = requests[1]["contents"].as_array().unwrap();
    let resumed_contents = requests[2]["contents"].as_array().unwrap();
    assert_eq!(&resumed_contents[..second_prefix.len()], second_prefix);
    assert_eq!(
        resumed_contents[second_prefix.len()],
        trace["nativeResponses"][1]["candidates"][0]["content"]
    );

    let prefix = requests[0]["contents"].as_array().unwrap();
    let contents = requests[1]["contents"].as_array().unwrap();
    assert_eq!(&contents[..prefix.len()], prefix);
    assert_eq!(
        contents[prefix.len()],
        trace["nativeResponses"][0]["candidates"][0]["content"]
    );
    assert_eq!(
        contents[prefix.len()]["parts"][0]["thoughtSignature"],
        "signed+/==\n"
    );
    assert_eq!(
        contents[prefix.len()]["parts"][3]["futurePart"]["number"].to_string(),
        "18446744073709551616"
    );
    for request in requests {
        assert_eq!(request["generationConfig"]["maxOutputTokens"], 4096);
        assert_eq!(
            request["generationConfig"]["thinkingConfig"],
            json!({"thinkingBudget":1024,"includeThoughts":true})
        );
        for key in ["model", "client_metadata", "prompt_cache_key", "include"] {
            assert!(request.get(key).is_none());
        }
    }
    let read = harness
        .runtime
        .client()
        .read_thread(&thread, true, DEADLINE)
        .await
        .unwrap();
    assert!(read.to_string().contains("CAIdex local "));
    assert!(read.to_string().contains("fixture complete"));
    let path = PathBuf::from(read["thread"]["path"].as_str().unwrap());
    assert!(
        path.canonicalize()
            .unwrap()
            .starts_with(harness.directory.0.join("data").canonicalize().unwrap())
    );
    let rollout = std::fs::read_to_string(path).unwrap();
    let histories: Vec<Value> = rollout
        .lines()
        .filter_map(|line| {
            let entry: Value = serde_json::from_str(line).unwrap();
            let item = &entry["payload"];
            (entry["type"] == "response_item" && item["type"] == "reasoning").then(|| item.clone())
        })
        .collect();
    assert_eq!(histories.len(), 3);
    for (i, item) in histories.iter().enumerate() {
        let envelope: Value = serde_json::from_str(
            item["encrypted_content"]
                .as_str()
                .unwrap()
                .strip_prefix("caidex.google.native-history.v2:")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(envelope["request"], requests[i]);
        assert_eq!(envelope["response"], trace["nativeResponses"][i]);
        assert_eq!(envelope["chunks"], trace["nativeChunks"][i]);
        assert_eq!(envelope["model"], "models/native-fixture");
    }
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; explicit Gemini single-call profile, approved temp marker and disk resume"]
async fn real_lite_native_google_code_mode_executes_tool_and_replays_result() {
    real_google_tool("gateway-google-tools-lite", true).await;
}

#[tokio::test]
#[ignore = "requires pinned Codex and local MCP; static tools, no web/discovery, exact Gemini disk resume"]
async fn real_classic_native_google_executes_static_mcp_and_replays_result() {
    real_google_tool("gateway-google-mcp-classic", false).await;
}

async fn real_google_tool(mode: &str, lite: bool) {
    let mut harness = Harness::start(mode).await;
    let thread = harness.create_thread().await;
    let marker = harness.directory.0.join("project/caidex-native-marker.txt");
    let mut approved = false;
    for i in 0..2 {
        if i == 1 {
            restart_google_runtime(&mut harness, &thread).await;
        }
        let client = harness.runtime.client();
        client.start_turn(&thread, vec![json!({"type":"text","text":"Offline Gemini tool fixture; continue from disk on second turn"})], json!({}), DEADLINE).await.unwrap();
        let mut visible = String::new();
        loop {
            match harness.next().await {
                RuntimeEvent::Interaction(request) => {
                    assert!(
                        lite && i == 0 && !approved,
                        "unexpected/repeated tool: {}",
                        request.event.raw
                    );
                    assert_eq!(request.kind, InteractionKind::CommandApproval);
                    assert!(!marker.exists());
                    client
                        .decide_approval(&request.id, ApprovalDecision::Accept)
                        .await
                        .unwrap();
                    approved = true;
                }
                RuntimeEvent::Notification(event) if event.method == "item/agentMessage/delta" => {
                    visible.push_str(event.raw["params"]["delta"].as_str().unwrap());
                }
                RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                    assert_eq!(
                        event.raw["params"]["turn"]["status"], "completed",
                        "{}",
                        event.raw
                    );
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(visible, "CAIdex local fixture complete");
        if lite {
            assert!(approved);
            // Removing it before restart detects an unnoticed repeated write.
            if i == 0 {
                assert_eq!(
                    std::fs::read_to_string(&marker).unwrap().trim(),
                    "CAIDEX_NATIVE_CODE_MODE"
                );
                std::fs::remove_file(&marker).unwrap();
            }
        } else {
            let mcp: Value = serde_json::from_slice(
                &std::fs::read(harness.directory.0.join("mcp-trace.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                mcp["toolCalls"],
                if i == 0 { json!(["echo"]) } else { json!([]) }
            );
        }
        if i == 1 && lite {
            assert!(!marker.exists());
        }
    }
    let trace = harness.trace();
    assert_eq!(trace["requests"], 3);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 3);
    assert_eq!(trace["authorizationSeen"], false);
    let requests = trace["nativeRequests"].as_array().unwrap();
    let prefix = requests[0]["contents"].as_array().unwrap();
    let contents = requests[1]["contents"].as_array().unwrap();
    assert_eq!(&contents[..prefix.len()], prefix);
    let signed = &trace["nativeResponses"][0]["candidates"][0]["content"];
    assert_eq!(contents[prefix.len()], *signed);
    assert_eq!(signed["parts"][1]["thoughtSignature"], "tool-signed+/==\n");
    assert_eq!(signed["parts"][1]["functionCall"]["id"], "google-tool-one");
    let results: Vec<_> = contents
        .iter()
        .flat_map(|c| c["parts"].as_array().unwrap())
        .filter_map(|p| p.get("functionResponse"))
        .collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["id"], "google-tool-one");
    assert_eq!(
        results[0]["name"],
        signed["parts"][1]["functionCall"]["name"]
    );
    if lite {
        assert!(
            results[0]["response"]["output"]
                .to_string()
                .contains("CAIDEX_NATIVE_CODE_MODE")
        );
    } else {
        let output = results[0]["response"]["output"].as_str().unwrap();
        let structured: Value =
            serde_json::from_str(output.split_once("\nOutput:\n").unwrap().1).unwrap();
        assert_eq!(structured, json!({"fixture":true}));
    }
    let second_prefix = requests[1]["contents"].as_array().unwrap();
    let resumed = requests[2]["contents"].as_array().unwrap();
    assert_eq!(&resumed[..second_prefix.len()], second_prefix);
    assert_eq!(
        resumed[second_prefix.len()],
        trace["nativeResponses"][1]["candidates"][0]["content"]
    );
    for request in &requests[1..] {
        assert_eq!(request["tools"], requests[0]["tools"]);
        assert_eq!(
            request["systemInstruction"],
            requests[0]["systemInstruction"]
        );
    }
    let read = harness
        .runtime
        .client()
        .read_thread(&thread, true, DEADLINE)
        .await
        .unwrap();
    let rollout = std::fs::read_to_string(read["thread"]["path"].as_str().unwrap()).unwrap();
    let items: Vec<Value> = rollout
        .lines()
        .filter_map(|line| {
            let entry: Value = serde_json::from_str(line).unwrap();
            (entry["type"] == "response_item").then(|| entry["payload"].clone())
        })
        .collect();
    let result = items
        .iter()
        .find(|item| {
            item["call_id"] == "google-tool-one"
                && item["type"]
                    == if lite {
                        "custom_tool_call_output"
                    } else {
                        "function_call_output"
                    }
        })
        .unwrap();
    assert_eq!(result["output"], results[0]["response"]["output"]);
    let call = items
        .iter()
        .find(|item| {
            item["call_id"] == "google-tool-one"
                && item["type"]
                    == if lite {
                        "custom_tool_call"
                    } else {
                        "function_call"
                    }
        })
        .unwrap();
    assert_eq!(call["name"], if lite { "exec" } else { "echo" });
    assert_eq!(
        call["namespace"],
        if lite { "functions" } else { "mcp__fixture" }
    );
    if lite {
        assert_eq!(
            call["input"],
            signed["parts"][1]["functionCall"]["args"]["input"]
        );
    } else {
        assert_eq!(call["arguments"], "{}");
    }
    assert_eq!(
        items
            .iter()
            .filter(|item| item["type"] == "reasoning")
            .count(),
        3
    );
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; Gemini local single-call policy rejects two calls before approval/execution"]
async fn real_lite_native_google_rejects_multiple_calls_before_execution() {
    let mut harness = Harness::start("gateway-google-multi-lite").await;
    let thread = harness.create_thread().await;
    harness
        .runtime
        .client()
        .start_turn(
            &thread,
            vec![json!({"type":"text","text":"Offline Gemini double-call negative fixture"})],
            json!({}),
            DEADLINE,
        )
        .await
        .unwrap();
    loop {
        match harness.next().await {
            RuntimeEvent::Interaction(request) => panic!(
                "rejected generation requested approval: {}",
                request.event.raw
            ),
            RuntimeEvent::Notification(event) if event.method == "turn/completed" => {
                assert_eq!(event.raw["params"]["turn"]["status"], "failed");
                assert!(
                    event.raw["params"]["turn"]["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("google_tool_call_limit_exceeded"),
                    "{}",
                    event.raw
                );
                break;
            }
            _ => {}
        }
    }
    assert_eq!(harness.trace()["requests"], 1);
    assert_eq!(harness.credential_reads.load(Ordering::SeqCst), 1);
    assert!(
        !harness
            .directory
            .0
            .join("project/caidex-native-marker.txt")
            .exists()
    );
    let read = harness
        .runtime
        .client()
        .read_thread(&thread, true, DEADLINE)
        .await
        .unwrap();
    let rollout = std::fs::read_to_string(read["thread"]["path"].as_str().unwrap()).unwrap();
    for line in rollout.lines() {
        let entry: Value = serde_json::from_str(line).unwrap();
        if entry["type"] == "response_item" {
            assert!(!matches!(
                entry["payload"]["type"].as_str(),
                Some("custom_tool_call" | "function_call" | "reasoning")
            ));
        }
    }
    harness.shutdown().await;
}

#[tokio::test]
#[ignore = "requires pinned Codex; Gemini classic/Lite interrupt closes actual native streaming socket"]
async fn real_runtime_interrupt_via_native_google_closes_socket() {
    interrupt_gateway(&["gateway-google-stall-classic", "gateway-google-stall-lite"]).await;
}
