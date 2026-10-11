//! Internal I-4.2 Host reproduction. Only numeric loopback fixtures and a dedicated synthetic key.
use caidex_chat::{LocalModel, memory::*};
use caidex_credentials::{Broker, CredentialRef, EnvironmentStore, Id, SecretKind};
use caidex_host::{
    HostPolicy, Journal, MemoryGrant, MemoryProfile, MemoryService, private_directory,
    serve_with_memory,
};
use caidex_model_core::{ModelProvider, ModelRouter, ResponsesDialect};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider, Limits};
use caidex_runtime::{AppServer, ClientOptions, Runtime};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{net::TcpListener, process::Command};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 5 {
        return Err("memory_host PRIVATE_ROOT MEMORY_FIXTURE RUNTIME_FIXTURE PORT".into());
    }
    for endpoint in [&args[2], &args[3]] {
        let port = endpoint
            .strip_prefix("http://127.0.0.1:")
            .ok_or("numeric loopback fixture required")?
            .parse::<u16>()?;
        if port == 0 {
            return Err("nonzero fixture port required".into());
        }
    }
    let directory = std::path::absolute(PathBuf::from(&args[1]))?;
    let journal = Journal::open(&directory)?;
    let host = journal.snapshot().host_id;
    let owner = Id::new("host-i42")?;
    let reference = CredentialRef {
        owner: owner.clone(),
        provider: Id::new("synthetic")?,
        profile: Id::new("i42")?,
        kind: SecretKind::ApiKey,
    };
    let broker = Arc::new(Broker::new(
        owner.clone(),
        EnvironmentStore::new(HashMap::from([(
            reference.clone(),
            "CAIDEX_I42_SYNTHETIC_KEY".into(),
        )]))?,
    ));
    let mut configs = vec![];
    for id in ["integration", "embedding"] {
        configs.push(
            ConfiguredModel::new(
                id.into(),
                format!("{id}-synthetic"),
                vec![ResponsesDialect::Classic],
                CustomResponses::new(&format!("{}/responses", args[2]), Some(reference.clone()))?,
            )?
            .with_embedding_endpoint(CustomResponses::new(
                &format!("{}/embeddings", args[2]),
                Some(reference.clone()),
            )?)?,
        );
    }
    let provider: Arc<dyn ModelProvider> = Arc::new(CustomResponsesProvider::new(
        configs,
        broker,
        Limits::default(),
    )?);
    let provider: Arc<dyn ModelProvider> = Arc::new(ModelRouter::new(
        ["integration", "embedding"]
            .into_iter()
            .map(|id| (id.into(), provider.clone()))
            .collect(),
    )?);
    let mut profiles = vec![];
    let mut permissions = vec![];
    for (id, purpose) in [
        ("integration", Purpose::Integration),
        ("embedding", Purpose::Embedding),
    ] {
        let target = provider.memory_target(id, purpose == Purpose::Embedding)?;
        let space = (purpose == Purpose::Embedding).then(|| Space {
            target: target.clone(),
            model_revision: "fixture-v1".into(),
            dimensions: 2,
            preprocessing_version: "text-v1".into(),
            distance: Distance::Cosine,
        });
        profiles.push(MemoryProfile {
            id: id.into(),
            model: LocalModel {
                provider: provider.clone(),
                model: id.into(),
            },
            purpose,
            space,
        });
        permissions.push(Permission {
            target: host_target(&host, id, &target),
            purpose,
            scope: Scope::Personal,
            kinds: vec![Kind::AiInference, Kind::Preference, Kind::ExecutionResult],
            source_types: vec![SourceType::User, SourceType::Execution],
            allow_local_only: true,
        });
    }
    let memory = MemoryService::open(
        &directory,
        owner,
        profiles,
        vec![MemoryGrant {
            client_id: "memory-demo".into(),
            permissions,
            evidence: BTreeMap::new(),
        }],
    )?;
    let project = directory.join("probe-project");
    let runtime_root = directory.join("runtime");
    private_directory(&project)?;
    private_directory(&runtime_root)?;
    if runtime_root.join("auth.json").exists() {
        return Err("no authentication import permitted".into());
    }
    let config = format!(
        "model = \"gpt-5.5\"\nmodel_provider = \"caidex_h2_a\"\n[analytics]\nenabled = false\n[model_providers.caidex_h2_a]\nname = \"I42 Runtime fixture\"\nbase_url = \"{}/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = false\n",
        args[3]
    );
    let path = runtime_root.join("config.toml");
    if path.exists() {
        if std::fs::symlink_metadata(&path)?.is_symlink()
            || std::fs::read_to_string(&path)? != config
        {
            return Err("existing config differs".into());
        }
    } else {
        use std::io::Write;
        let mut f = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(path)?;
        f.write_all(config.as_bytes())?;
        f.sync_all()?;
    }
    let binary = std::env::var_os("CAIDEX_CODEX_BIN").ok_or("pinned Codex executable required")?;
    let mut command = Command::new(binary);
    command.env_clear();
    for name in ["PATH", "HOME", "SystemRoot", "USERPROFILE", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env("CODEX_HOME", runtime_root)
        .current_dir(&project)
        .args(["app-server", "--listen", "stdio://"]);
    let runtime = Runtime::connect(
        AppServer::spawn(command, 256)?,
        ClientOptions::default(),
        Duration::from_secs(15),
        256,
    )
    .await?;
    serve_with_memory(
        TcpListener::bind(format!("127.0.0.1:{}", args[4])).await?,
        runtime,
        journal,
        std::env::var("CAIDEX_HOST_TOKEN")?,
        project,
        HostPolicy::offline(),
        memory,
    )
    .await?;
    Ok(())
}
