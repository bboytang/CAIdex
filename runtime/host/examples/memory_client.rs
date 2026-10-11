//! Internal interactive reproduction driver; not the formal CLI or an account/Key importer.
use caidex_chat::{Chat, Namespace, memory::*};
use caidex_credentials::Secret;
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
fn reply(value: Value) {
    println!("{value}");
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("memory_client PRIVATE_ROOT HOST_ADDRESS HOST_ID".into());
    }
    let connection = HostConnection::new(
        args[2].parse()?,
        args[3].clone(),
        Secret::new(std::env::var("CAIDEX_I42_PAIR_TOKEN")?)?,
        Namespace::guest(),
    )?;
    let chat = Chat::open(&PathBuf::from(&args[1]), Namespace::guest())?;
    let e = chat.memory();
    assert!(!e.automatic()?);
    for profile in ["integration", "embedding"] {
        let model = connection.model(profile, Scope::Personal).await?;
        if model.info.purpose == Purpose::Integration {
            e.configure_host_integration(model.clone())?;
        } else {
            e.configure_host_embedding(model.clone())?;
        }
        e.allow(Permission {
            target: model.info.target,
            purpose: model.info.purpose,
            scope: Scope::Personal,
            kinds: vec![Kind::AiInference, Kind::Preference, Kind::ExecutionResult],
            source_types: vec![SourceType::User, SourceType::Execution],
            allow_local_only: true,
        })?;
    }
    e.configure_host_resolver(connection.clone())?;
    reply(json!({"ready":true,"automatic_default":false}));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut waiting = None;
    let mut last = None;
    while let Some(line) = lines.next_line().await? {
        let input: Value = serde_json::from_str(&line)?;
        match input["action"].as_str().ok_or("action required")? {
            "enable" => {
                e.set_automatic(true)?;
                reply(json!({"enabled":true}));
            }
            "run" => {
                let source = Source::user(
                    Scope::Personal,
                    input["text"].as_str().ok_or("text required")?.into(),
                );
                let job = e.queue_integration(source, Privacy::LocalOnly)?;
                waiting = Some(job);
                reply(json!({"operation_id":job}));
                match e.run(job).await {
                    Ok(ids) => {
                        last = ids.first().copied().or(last);
                        reply(
                            json!({"state":e.jobs(&Scope::Personal,100)?.into_iter().find(|j|j.id==job).ok_or("job missing")?.state,"memories":ids}),
                        );
                    }
                    Err(error) => reply(json!({"error":error.to_string()})),
                }
            }
            "resume" => {
                let ids = e.run(waiting.ok_or("no waiting job")?).await?;
                last = ids.first().copied();
                reply(json!({"resumed":ids.len()}));
            }
            "recover" => match e.recover_host(waiting.ok_or("no original job")?).await {
                Ok(ids) => {
                    last = ids.first().copied().or(last);
                    reply(json!({"recovered":ids.len()}));
                }
                Err(error) => reply(json!({"error":error.to_string()})),
            },
            "embedding" => {
                let id = last.ok_or("no candidate")?;
                let m = e.get(id)?;
                e.approve(id, m.head)?;
                let job = e.queue_embedding(id)?;
                e.run(job).await?;
                let count = e
                    .semantic_search(&Scope::Personal, "preferred editor", 10)
                    .await?
                    .len();
                reply(json!({"semantic_results":count}));
            }
            "source" => {
                let reference = format!(
                    "host:{}:{}",
                    args[3],
                    input["task_id"].as_str().ok_or("task required")?
                );
                let bytes = connection
                    .evidence(&Scope::Personal, &reference)
                    .ok_or("source denied or offline")?;
                let excerpt = String::from_utf8(bytes.clone())?;
                let source = Source::evidence(
                    Scope::Personal,
                    Origin::Execution { reference },
                    &bytes,
                    excerpt.clone(),
                );
                let id = e.create(Draft {
                    scope: Scope::Personal,
                    text: excerpt,
                    kind: Kind::ExecutionResult,
                    privacy: Privacy::LocalOnly,
                    tags: vec![],
                    sources: vec![source.clone()],
                    valid_until_millis: None,
                })?;
                let job = e.queue_integration(source, Privacy::LocalOnly)?;
                let ids = e.run(job).await?;
                assert_eq!(ids.len(), 1);
                reply(
                    json!({"execution_memory":id,"source_current":e.get(id)?.revisions[0].source_current,"candidate_count":ids.len()}),
                );
            }
            "source-denied" => {
                let reference = format!(
                    "host:{}:{}",
                    args[3],
                    input["task_id"].as_str().ok_or("task required")?
                );
                assert!(connection.evidence(&Scope::Personal, &reference).is_none());
                reply(json!({"source_denied":true}));
            }
            "disable" => {
                e.set_automatic(false)?;
                reply(json!({"disabled":true}));
            }
            "exit" => break,
            _ => return Err("unknown action".into()),
        }
    }
    Ok(())
}
