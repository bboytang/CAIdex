//! Internal I-4 reproduction, not the formal CLI/GUI or a commercial-model acceptance.
use caidex_chat::memory::*;
use caidex_chat::{Chat, Content, LocalModel, Namespace, Send};
use caidex_credentials::{Broker, EnvironmentStore};
use caidex_model_core::{ModelProvider, ModelRouter, ResponsesDialect};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider, Limits};
use serde_json::json;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use uuid::Uuid;
fn models(base: &str, ns: &Namespace) -> Arc<dyn ModelProvider> {
    let broker = Arc::new(Broker::new(
        ns.credential_owner(),
        EnvironmentStore::new(HashMap::new()).unwrap(),
    ));
    let configs = [
        ("chat", "chat-synthetic"),
        ("integration", "integration-synthetic"),
        ("embedding", "embedding-synthetic"),
    ]
    .into_iter()
    .map(|(id, native)| {
        ConfiguredModel::new(
            id.into(),
            native.into(),
            vec![ResponsesDialect::Classic],
            CustomResponses::new(&format!("{base}/responses"), None).unwrap(),
        )
        .unwrap()
        .with_embedding_endpoint(CustomResponses::new(&format!("{base}/embeddings"), None).unwrap())
        .unwrap()
    })
    .collect();
    let p: Arc<dyn ModelProvider> =
        Arc::new(CustomResponsesProvider::new(configs, broker, Limits::default()).unwrap());
    Arc::new(
        ModelRouter::new(
            ["chat", "integration", "embedding"]
                .into_iter()
                .map(|s| (s.into(), p.clone()))
                .collect(),
        )
        .unwrap(),
    )
}
fn local(p: &Arc<dyn ModelProvider>, name: &str) -> LocalModel {
    LocalModel {
        provider: p.clone(),
        model: name.into(),
    }
}
fn draft(text: &str) -> Draft {
    Draft {
        scope: Scope::Personal,
        text: text.into(),
        kind: Kind::Preference,
        privacy: Privacy::LocalOnly,
        tags: vec!["editor".into()],
        sources: vec![Source::user(Scope::Personal, text.into())],
        valid_until_millis: None,
    }
}
fn allow(
    e: &MemoryEngine<'_>,
    p: &Arc<dyn ModelProvider>,
    model: &str,
    purpose: Purpose,
) -> caidex_chat::Result<Uuid> {
    e.allow(Permission {
        target: p
            .memory_target(model, purpose == Purpose::Embedding)
            .unwrap(),
        purpose,
        scope: Scope::Personal,
        kinds: vec![Kind::Preference, Kind::AiInference],
        source_types: vec![SourceType::User, SourceType::Chat],
        allow_local_only: true,
    })
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 || !args[2].starts_with("http://127.0.0.1:") {
        return Err("usage: local_memory PRIVATE_ROOT LOOPBACK_URL".into());
    }
    let root = PathBuf::from(&args[1]);
    let ns = Namespace::guest();
    let p = models(&args[2], &ns);
    let chat = Chat::open(&root, ns.clone())?;
    let e = chat.memory();
    assert!(!e.automatic()?);
    let id = e.create(draft("Use vim for editing"))?;
    assert_eq!(e.search(&Scope::Personal, "vim", None, None, 10)?.len(), 1);
    let source = Source::user(Scope::Personal, "I prefer vim".into());
    let job = e.queue_integration(source, Privacy::LocalOnly)?;
    assert!(e.run(job).await?.is_empty());
    e.set_automatic(true)?;
    assert!(e.run(job).await?.is_empty());
    e.configure_integration(local(&p, "integration"))?;
    assert!(e.run(job).await?.is_empty());
    allow(&e, &p, "integration", Purpose::Integration)?;
    let candidates = e.run(job).await?;
    assert_eq!(candidates.len(), 1);
    let candidate = e.get(candidates[0])?;
    assert_eq!(candidate.state, "candidate");
    e.approve(candidate.id, candidate.head)?;
    let target = p.memory_target("embedding", true)?;
    let space = Space {
        target,
        model_revision: "fixture-v1".into(),
        dimensions: 2,
        preprocessing_version: "text-v1".into(),
        distance: Distance::Cosine,
    };
    e.configure_embedding(local(&p, "embedding"), space.clone())?;
    allow(&e, &p, "embedding", Purpose::Embedding)?;
    let embed = e.queue_embedding(id)?;
    e.run(embed).await?;
    assert_eq!(
        e.semantic_search(&Scope::Personal, "favorite editor", 10)
            .await?
            .len(),
        1
    );
    let mut newer = space.clone();
    newer.model_revision = "fixture-v2".into();
    e.configure_embedding(local(&p, "embedding"), newer)?;
    assert!(
        e.semantic_search(&Scope::Personal, "favorite editor", 10)
            .await?
            .is_empty()
    );
    let base = e.get(id)?.head;
    let first = e.correct(id, base, draft("Use helix"))?;
    let conflict = e.correct(id, base, draft("Use emacs"))?;
    let conflicted = e.get(id)?;
    assert_eq!(conflicted.state, "disputed");
    assert_eq!(conflicted.head, first);
    assert_eq!(conflicted.revisions.len(), 3);
    e.approve(id, conflict)?;
    chat.configure(local(&p, "chat"))?;
    allow(&e, &p, "chat", Purpose::Chat)?;
    let conv = chat.create_conversation(None, "memory injection fixture".into())?;
    let send = || Send {
        operation: Uuid::new_v4(),
        conversation: conv,
        parent: None,
        content: Content::text("Hello"),
    };
    chat.send(send()).await?;
    assert!(
        !chat
            .messages(conv, 0, 100)?
            .iter()
            .any(|m| m.content.text.contains("Use emacs"))
    );
    e.set_automatic(false)?;
    chat.send(send()).await?;
    let exported = e.export(&Scope::Personal)?;
    assert!(exported.contains("ai_inference") && exported.contains("Use emacs"));
    e.delete(id)?;
    assert!(
        e.search(&Scope::Personal, "emacs", None, None, 100)?
            .is_empty()
    );
    drop(chat);
    let chat = Chat::open(&root, ns)?;
    assert!(!chat.memory().automatic()?);
    assert_eq!(chat.memory().get(id)?.state, "deleted");
    println!(
        "{}",
        json!({"milestone":"I-4","synthetic_only":true,"automatic_default":false,"manual_without_models":true,"waiting_without_model_and_permission":true,"integration_candidates":1,"embedding_space_version_isolated":true,"cas_conflict_revisions":3,"transient_chat_injection":true,"export_provenance":true,"delete_and_reopen":true,"uploads":0})
    );
    Ok(())
}
