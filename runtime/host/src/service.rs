use std::{path::PathBuf, time::Duration};

use caidex_runtime::{Runtime, RuntimeEvent};
use serde::Deserialize;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc, oneshot},
    task::JoinSet,
};

use crate::{Error, Event, Journal, Result, random_id};

const DEADLINE: Duration = Duration::from_secs(15);
const WRITE_DEADLINE: Duration = Duration::from_secs(5);
const MAX_CLIENTS: usize = 64;
const EVENT_CAPACITY: usize = 128;
const MAX_FRAME: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Auth {
    protocol: u32,
    token: String,
}

#[derive(Deserialize)]
#[serde(tag = "method", deny_unknown_fields)]
enum Request {
    #[serde(rename = "snapshot")]
    Snapshot { id: u64 },
    #[serde(rename = "attach")]
    Attach {
        id: u64,
        host_id: String,
        after: Option<i64>,
    },
    #[serde(rename = "detach")]
    Detach { id: u64 },
    #[serde(rename = "probe")]
    Probe { id: u64 },
    #[serde(rename = "shutdown")]
    Shutdown { id: u64 },
}

enum ClientCommand {
    Snapshot,
    Attach { host_id: String, after: Option<i64> },
    Detach,
    Probe,
    Shutdown,
}

impl Request {
    fn into_parts(self) -> (u64, ClientCommand) {
        match self {
            Self::Snapshot { id } => (id, ClientCommand::Snapshot),
            Self::Attach { id, host_id, after } => (id, ClientCommand::Attach { host_id, after }),
            Self::Detach { id } => (id, ClientCommand::Detach),
            Self::Probe { id } => (id, ClientCommand::Probe),
            Self::Shutdown { id } => (id, ClientCommand::Shutdown),
        }
    }
}

enum Reply {
    Json(Value),
    Attached(Value, broadcast::Receiver<Event>),
}

type ReplySender = oneshot::Sender<std::result::Result<Reply, &'static str>>;

enum Message {
    Client(ClientCommand, ReplySender),
    ProbeResult(
        String,
        std::result::Result<Value, caidex_runtime::Error>,
        ReplySender,
    ),
}

fn publish(
    journal: &mut Journal,
    events: &broadcast::Sender<Event>,
    method: &str,
    data: Value,
) -> Result<Event> {
    let event = journal.append(method, data)?;
    let _ = events.send(event.clone());
    Ok(event)
}

/// The independently launched process owns Runtime, not a client connection.
/// A journal error is fatal: no further Runtime calls or uncommitted broadcasts.
pub async fn serve(
    listener: TcpListener,
    mut runtime: Runtime,
    mut journal: Journal,
    token: String,
    project: PathBuf,
) -> Result<()> {
    if !listener.local_addr()?.ip().is_loopback()
        || token.len() != 64
        || !token.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::Refused(
            "Host requires loopback and a 256-bit authorization token",
        ));
    }
    let (events, _) = broadcast::channel(EVENT_CAPACITY);
    let (send, mut messages) = mpsc::channel(MAX_CLIENTS);
    let mut connections = JoinSet::new();
    let mut probes = JoinSet::new();
    let mut probe_pending = false;
    let result = async {
        publish(&mut journal, &events, "host/started", json!({"codex_version": caidex_runtime::CODEX_VERSION}))?;
        println!("{}", json!({"protocol": 1, "address": listener.local_addr()?, "host_id": journal.snapshot().host_id, "pid": std::process::id()}));
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (socket, _) = accepted?;
                    if connections.len() < MAX_CLIENTS {
                        let send = send.clone();
                        let token = token.clone();
                        connections.spawn(async move { let _ = connection(socket, token, send).await; });
                    }
                }
                _ = connections.join_next(), if !connections.is_empty() => (),
                _ = probes.join_next(), if !probes.is_empty() => (),
                event = runtime.next_event() => {
                    match event {
                        Some(RuntimeEvent::Notification(event)) => {
                            // H-1 has no account/auth entry points. Never journal
                            // authentication payloads, including future extensions.
                            if event.method.starts_with("account/") {
                                publish(&mut journal, &events, "host/runtimeUnavailable", json!({"reason": "authentication event outside H-1"}))?;
                                return Err(Error::Refused("authentication event outside H-1"));
                            }
                            publish(&mut journal, &events, &event.method, event.raw)?;
                        }
                        Some(RuntimeEvent::Interaction(_)) => {
                            publish(&mut journal, &events, "host/runtimeUnavailable", json!({"reason": "interaction outside H-1"}))?;
                            return Err(Error::Refused("Runtime interaction outside H-1; no automatic answer"));
                        }
                        None => {
                            publish(&mut journal, &events, "host/runtimeUnavailable", json!({"reason": "Runtime disconnected or event queue failed"}))?;
                            return Err(Error::Refused("Runtime disconnected; captured journal is not a complete Runtime history"));
                        }
                    }
                }
                message = messages.recv() => {
                    match message.expect("service retains a sender") {
                        Message::Client(ClientCommand::Snapshot, reply) => {
                            let _ = reply.send(Ok(Reply::Json(json!(journal.snapshot()))));
                        }
                        Message::Client(ClientCommand::Attach { host_id, after }, reply) => {
                            // No await between subscribe and watermark/replay: the
                            // service is the sole journal writer, so there is no gap.
                            let receiver = events.subscribe();
                            let recovery = match journal.recovery(&host_id, after) {
                                Ok(value) => Ok(Reply::Attached(value, receiver)),
                                Err(Error::Refused(reason)) => Err(reason),
                                Err(error) => return Err(error),
                            };
                            let _ = reply.send(recovery);
                        }
                        Message::Client(ClientCommand::Detach, reply) => {
                            let _ = reply.send(Ok(Reply::Json(json!({"detached": true}))));
                        }
                        Message::Client(ClientCommand::Probe, reply) => {
                            if probe_pending {
                                let _ = reply.send(Err("probe already in flight"));
                                continue;
                            }
                            let id = random_id()?;
                            publish(&mut journal, &events, "host/probeStarted", json!({"probe_id": id}))?;
                            probe_pending = true;
                            let client = runtime.client();
                            let send = send.clone();
                            let project = project.clone();
                            probes.spawn(async move {
                                let result = client.start_thread(json!({"cwd": project, "ephemeral": true, "approvalPolicy": "never", "sandbox": "read-only"}), DEADLINE).await;
                                // Dropped clients do not cancel a submitted probe.
                                let _ = send.send(Message::ProbeResult(id, result, reply)).await;
                            });
                        }
                        Message::ProbeResult(id, result, reply) => {
                            probe_pending = false;
                            let data = match &result {
                                Ok(value) => json!({"probe_id": id, "outcome": "confirmed", "result": value}),
                                Err(caidex_runtime::Error::Rpc(_, _, _)) => json!({"probe_id": id, "outcome": "rejected"}),
                                Err(_) => {
                                    // Intent stays unresolved; neither retry nor
                                    // classify a lost response as a failed action.
                                    publish(&mut journal, &events, "host/runtimeUnavailable", json!({"reason": "probe outcome unknown"}))?;
                                    let _ = reply.send(Err("probe outcome unknown; never automatically retry"));
                                    return Err(Error::Refused("probe outcome unknown"));
                                }
                            };
                            publish(&mut journal, &events, "host/probeResult", data.clone())?;
                            let _ = reply.send(Ok(Reply::Json(data)));
                        }
                        Message::Client(ClientCommand::Shutdown, reply) => {
                            publish(&mut journal, &events, "host/stopping", json!({"reason": "explicit owner shutdown"}))?;
                            runtime.shutdown().await?;
                            publish(&mut journal, &events, "host/stopped", json!({"reason": "explicit owner shutdown"}))?;
                            let _ = reply.send(Ok(Reply::Json(json!({"stopped": true}))));
                            // Let the initiating connection flush its response.
                            return Ok(());
                        }
                    }
                }
            }
        }
    }.await;
    probes.abort_all();
    drop(messages);
    // Closing the service senders makes idle client handlers finish as well.
    if result.is_ok() {
        let _ = tokio::time::timeout(WRITE_DEADLINE, async {
            while connections.join_next().await.is_some() {}
        })
        .await;
    }
    connections.abort_all();
    drop(runtime);
    // Release ownership before Tokio publishes this future's completion.
    drop(journal);
    result
}

async fn frame(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    bytes: &mut Vec<u8>,
) -> Result<Option<Vec<u8>>> {
    // Retain partial frames across select cancellation while broadcasts arrive.
    let remaining = (MAX_FRAME + 1).saturating_sub(bytes.len() as u64);
    let size = reader.take(remaining).read_until(b'\n', bytes).await?;
    if size == 0 && bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() as u64 > MAX_FRAME || bytes.last() != Some(&b'\n') {
        return Err(Error::Refused("invalid or oversized client frame"));
    }
    Ok(Some(std::mem::take(bytes)))
}

async fn write(writer: &mut tokio::net::tcp::OwnedWriteHalf, value: Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    tokio::time::timeout(WRITE_DEADLINE, writer.write_all(&bytes))
        .await
        .map_err(|_| Error::Refused("client write timed out"))??;
    Ok(())
}

async fn connection(socket: TcpStream, token: String, send: mpsc::Sender<Message>) -> Result<()> {
    let (read, mut writer) = socket.into_split();
    let mut reader = BufReader::new(read);
    let mut buffer = Vec::new();
    let bytes = tokio::time::timeout(DEADLINE, frame(&mut reader, &mut buffer))
        .await
        .map_err(|_| Error::Refused("authorization timed out"))??
        .ok_or(Error::Refused("missing authorization"))?;
    let auth: Auth = serde_json::from_slice(&bytes)?;
    if auth.protocol != 1 || !bool::from(auth.token.as_bytes().ct_eq(token.as_bytes())) {
        write(
            &mut writer,
            json!({"error": "authorization/protocol rejected"}),
        )
        .await?;
        return Ok(());
    }
    write(&mut writer, json!({"authorized": true, "protocol": 1})).await?;
    let mut subscriber: Option<broadcast::Receiver<Event>> = None;
    let mut last_seq = 0;
    loop {
        tokio::select! {
            _ = send.closed() => return Ok(()),
            bytes = frame(&mut reader, &mut buffer) => {
                let Some(bytes) = bytes? else { return Ok(()); };
                let request: Request = match serde_json::from_slice(&bytes) {
                    Ok(value) => value,
                    Err(_) => { write(&mut writer, json!({"error": "invalid or unavailable H-1 request"})).await?; continue; }
                };
                let (id, command) = request.into_parts();
                let detach = matches!(command, ClientCommand::Detach);
                let shutdown = matches!(command, ClientCommand::Shutdown);
                let (reply, receive) = oneshot::channel();
                send.send(Message::Client(command, reply)).await.map_err(|_| Error::Refused("Host stopped"))?;
                let response = receive.await.map_err(|_| Error::Refused("Host stopped; response outcome unknown"))?;
                match response {
                    Ok(Reply::Json(value)) => {
                        if detach { subscriber = None; }
                        write(&mut writer, json!({"id": id, "result": value})).await?;
                        if shutdown { return Ok(()); }
                    }
                    Ok(Reply::Attached(value, receiver)) => {
                        last_seq = value["through_seq"].as_i64().or_else(|| value.pointer("/snapshot/seq").and_then(Value::as_i64)).expect("recovery watermark");
                        subscriber = Some(receiver);
                        write(&mut writer, json!({"id": id, "result": value})).await?;
                    }
                    Err(reason) => write(&mut writer, json!({"id": id, "error": reason})).await?,
                }
            }
            event = async { subscriber.as_mut().expect("guarded subscription").recv().await }, if subscriber.is_some() => {
                match event {
                    Ok(event) => {
                        last_seq = event.seq;
                        write(&mut writer, json!({"event": event})).await?;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        write(&mut writer, json!({"gap": true, "after": last_seq, "reconnect": true})).await?;
                        return Ok(());
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lagged_subscriber_gets_explicit_gap_and_reconnects_to_consistent_snapshot() {
        let directory = std::env::temp_dir().join(format!(
            "caidex-h1-lag-{}-{}",
            std::process::id(),
            random_id().unwrap()
        ));
        let mut journal = Journal::open(&directory).unwrap();
        journal.append("host/started", json!({})).unwrap();
        let host = journal.snapshot().host_id;
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let (send, mut messages) = mpsc::channel(4);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let task = tokio::spawn(connection(server, "f".repeat(64), send));
        let (read, mut write) = socket.into_split();
        let mut lines = BufReader::new(read).lines();
        write
            .write_all(format!("{}\n", json!({"protocol": 1, "token": "f".repeat(64)})).as_bytes())
            .await
            .unwrap();
        lines.next_line().await.unwrap().unwrap();
        write
            .write_all(
                format!(
                    "{}\n",
                    json!({"id": 1, "method": "attach", "host_id": host})
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let Message::Client(_, reply) = messages.recv().await.unwrap() else {
            panic!("attach")
        };
        reply
            .send(Ok(Reply::Attached(
                journal.recovery(&host, None).unwrap(),
                events.subscribe(),
            )))
            .ok()
            .unwrap();
        lines.next_line().await.unwrap().unwrap();
        // Keep the handler awaiting an actor response while committed events fill
        // its ring. This deterministically exercises the slow-consumer gap path.
        write
            .write_all(b"{\"id\":2,\"method\":\"snapshot\"}\n")
            .await
            .unwrap();
        let Message::Client(_, reply) = messages.recv().await.unwrap() else {
            panic!("snapshot")
        };
        for index in 0..=EVENT_CAPACITY {
            publish(
                &mut journal,
                &events,
                "future/event",
                json!({"index": index}),
            )
            .unwrap();
        }
        reply
            .send(Ok(Reply::Json(json!(journal.snapshot()))))
            .ok()
            .unwrap();
        lines.next_line().await.unwrap().unwrap();
        let gap: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(gap["gap"], true);
        assert_eq!(gap["after"], 1);
        assert!(task.await.unwrap().is_ok());
        let recovery = journal.recovery(&host, Some(1)).unwrap();
        assert_eq!(recovery["mode"], "snapshot");
        assert_eq!(recovery["snapshot"]["seq"], 130);
        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
