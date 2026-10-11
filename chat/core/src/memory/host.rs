//! Memory-only client for the existing loopback Host transport (SSH/Relay come later).
use super::*;
use caidex_credentials::Secret;
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    net::{SocketAddr, TcpStream as BlockingStream},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

const DEADLINE: Duration = Duration::from_secs(5);
const MAX_FRAME: u64 = 65536;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostModelInfo {
    pub profile: String,
    pub purpose: Purpose,
    pub target: ProviderTarget,
    pub space: Option<Space>,
}
/// Includes the real Provider endpoint and native model, pinned to this Host/profile.
pub fn host_target(host: &str, profile: &str, target: &ProviderTarget) -> ProviderTarget {
    ProviderTarget {
        endpoint: format!("caidex-host:{host}:{profile}:{}", target.endpoint),
        native_model: target.native_model.clone(),
    }
}
#[derive(Clone)]
pub struct HostConnection {
    address: SocketAddr,
    host_id: String,
    token: Arc<Secret>,
    namespace: crate::Namespace,
    guard: Arc<Redactor>,
}
impl HostConnection {
    pub fn new(
        address: SocketAddr,
        host_id: String,
        token: Secret,
        namespace: crate::Namespace,
    ) -> Result<Self> {
        if !address.ip().is_loopback()
            || host_id.len() != 32
            || !host_id.bytes().all(|c| c.is_ascii_hexdigit())
            || token.expose().len() != 64
            || !token.expose().bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(Error::Invalid);
        }
        let guard = Arc::new(Redactor::default());
        guard.register(&token).map_err(|_| Error::Invalid)?;
        Ok(Self {
            address,
            host_id,
            token: Arc::new(token),
            namespace,
            guard,
        })
    }
    pub async fn models(&self) -> Result<Vec<HostModelInfo>> {
        let result = self.rpc(json!({"method":"memory/models"})).await?;
        serde_json::from_value(result).map_err(|_| Error::Invalid)
    }
    /// The descriptor is obtained over an authenticated RPC, then pinned in this session.
    pub async fn model(&self, profile: &str, scope: Scope) -> Result<HostModel> {
        let info = self
            .models()
            .await?
            .into_iter()
            .find(|i| i.profile == profile)
            .ok_or(Error::HostDenied)?;
        Ok(HostModel {
            connection: self.clone(),
            info,
            scope,
        })
    }
    async fn rpc(&self, mut request: Value) -> Result<Value> {
        request["id"] = json!(1);
        request["host_id"] = json!(self.host_id);
        let bytes = format!("{request}\n").into_bytes();
        if bytes.len() as u64 > MAX_FRAME {
            return Err(Error::Invalid);
        }
        tokio::time::timeout(DEADLINE, async {
            let socket = TcpStream::connect(self.address)
                .await
                .map_err(|_| Error::HostOffline)?;
            let (read, mut write) = socket.into_split();
            let mut read = BufReader::new(read);
            write
                .write_all(
                    format!("{}\n", json!({"protocol":1,"token":self.token.expose()})).as_bytes(),
                )
                .await
                .map_err(|_| Error::HostOffline)?;
            let auth = line(&mut read).await?;
            if auth["authorized"] != true {
                return Err(Error::HostDenied);
            }
            // Once the request write begins, delivery is uncertain; never classify it as safe to resend.
            write
                .write_all(&bytes)
                .await
                .map_err(|_| Error::HostUnknown)?;
            let reply = line(&mut read).await?;
            if reply["id"] != 1 {
                return Err(Error::HostUnknown);
            }
            if let Some(reason) = reply["error"].as_str() {
                return Err(if reason == "Memory operation unknown" {
                    Error::HostUnknown
                } else {
                    Error::HostDenied
                });
            }
            Ok(reply["result"].clone())
        })
        .await
        .map_err(|_| Error::HostUnknown)?
    }
    pub fn evidence(&self, scope: &Scope, reference: &str) -> Option<Vec<u8>> {
        let task = reference.strip_prefix(&format!("host:{}:", self.host_id))?;
        let mut socket = BlockingStream::connect_timeout(&self.address, DEADLINE).ok()?;
        socket.set_read_timeout(Some(DEADLINE)).ok()?;
        socket.set_write_timeout(Some(DEADLINE)).ok()?;
        writeln!(
            socket,
            "{}",
            json!({"protocol":1,"token":self.token.expose()})
        )
        .ok()?;
        let mut reader = std::io::BufReader::new(socket.try_clone().ok()?);
        if blocking_line(&mut reader)?["authorized"] != true {
            return None;
        }
        writeln!(socket, "{}", json!({"id":1,"method":"memory/evidence","host_id":self.host_id,"scope":scope,"task_id":task})).ok()?;
        let reply = blocking_line(&mut reader)?;
        if reply["id"] != 1 || reply.get("error").is_some() {
            return None;
        }
        let value = serde_json::to_vec(&reply["result"]).ok()?;
        if self
            .guard
            .contains_secret(std::str::from_utf8(&value).ok()?)
        {
            return None;
        }
        Some(value)
    }
}
async fn line(read: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Result<Value> {
    let mut bytes = Vec::new();
    read.take(MAX_FRAME + 1)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|_| Error::HostUnknown)?;
    if bytes.len() as u64 > MAX_FRAME || bytes.last() != Some(&b'\n') {
        return Err(Error::HostUnknown);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::HostUnknown)
}
fn blocking_line(read: &mut std::io::BufReader<BlockingStream>) -> Option<Value> {
    let mut bytes = Vec::new();
    Read::take(&mut *read, MAX_FRAME + 1)
        .read_until(b'\n', &mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_FRAME || bytes.last() != Some(&b'\n') {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}
impl EvidenceResolver for HostConnection {
    fn read(&self, scope: &Scope, origin: &Origin) -> Option<Vec<u8>> {
        if let Origin::Execution { reference } = origin {
            self.evidence(scope, reference)
        } else {
            None
        }
    }
}
#[derive(Clone)]
pub struct HostModel {
    connection: HostConnection,
    pub info: HostModelInfo,
    pub scope: Scope,
}
impl HostModel {
    pub(crate) fn validate(&self, namespace: &crate::Namespace) -> Result<()> {
        if &self.connection.namespace != namespace {
            return Err(Error::WrongCredentialOwner);
        }
        Ok(())
    }
    pub(crate) async fn ready(&self) -> Result<bool> {
        match self.connection.models().await {
            Ok(models) => Ok(models.contains(&self.info)),
            Err(Error::HostUnknown | Error::HostOffline) => Err(Error::HostOffline),
            Err(error) => Err(error),
        }
    }
    pub(crate) async fn infer(
        &self,
        operation: Uuid,
        request: InferenceRequest,
        cancel: CancellationToken,
        recover: bool,
    ) -> Result<InferenceOutput> {
        if request.draft.scope != self.scope
            || request.purpose() != self.info.purpose
            || request.profile != self.info.profile
            || request.target != self.info.target
            || request.space != self.info.space
        {
            return Err(Error::HostDenied);
        }
        request.validate()?;
        if self.connection.guard.contains_secret(&encode(&request)?) {
            return Err(Error::Invalid);
        }
        let operation_id = operation.to_string();
        let connection = self.connection.clone();
        let id = operation_id.clone();
        let watcher_cancel = cancel.clone();
        // The engine's Flight also cancels on future drop/account switch, independently of polling.
        let watcher = tokio::spawn(async move {
            watcher_cancel.cancelled().await;
            let _ = connection
                .rpc(json!({"method":"memory/cancel","operation_id":id}))
                .await;
        });
        let work = async {
            if !recover {
                self.connection.rpc(json!({"method":"memory/submit","operation_id":operation_id,"request":request})).await?;
            }
            loop {
                let result = self
                    .connection
                    .rpc(json!({"method":"memory/status","operation_id":operation_id}))
                    .await?;
                match result["state"].as_str() {
                    Some("running") => tokio::time::sleep(Duration::from_millis(50)).await,
                    Some("done") => {
                        let output: InferenceOutput =
                            serde_json::from_value(result["output"].clone())
                                .map_err(|_| Error::Invalid)?;
                        output.validate(&request)?;
                        if self.connection.guard.contains_secret(&encode(&output)?) {
                            return Err(Error::Invalid);
                        }
                        return Ok(output);
                    }
                    Some("cancelled") => return Err(Error::Cancelled),
                    Some("failed") => return Err(Error::ModelUnavailable),
                    _ => return Err(Error::HostUnknown),
                }
            }
        };
        let result =
            tokio::select! { biased; _=cancel.cancelled()=>Err(Error::Cancelled), r=work=>r };
        if result.is_ok() || !matches!(result, Err(Error::Cancelled)) {
            watcher.abort();
        }
        result
    }
}
#[derive(Clone)]
pub(crate) enum Executor {
    Local(LocalModel),
    Host(Box<HostModel>),
}
impl Executor {
    pub(crate) fn target(&self, embedding: bool) -> Result<ProviderTarget> {
        match self {
            Self::Local(m) => m
                .provider
                .memory_target(&m.model, embedding)
                .map_err(|_| Error::ModelUnavailable),
            Self::Host(m) => Ok(m.info.target.clone()),
        }
    }
    pub(crate) fn validate(&self, namespace: &crate::Namespace) -> Result<()> {
        match self {
            Self::Local(m) => crate::client::validate_model(m, namespace),
            Self::Host(m) => m.validate(namespace),
        }
    }
    pub(crate) fn profile(&self) -> String {
        match self {
            Self::Local(m) => m.model.clone(),
            Self::Host(m) => m.info.profile.clone(),
        }
    }
    pub(crate) async fn release(&self, id: Uuid) {
        if let Self::Host(m) = self {
            let _ = m
                .connection
                .rpc(json!({"method":"memory/release","operation_id":id.to_string()}))
                .await;
        }
    }
    pub(crate) async fn infer(
        &self,
        id: Uuid,
        request: InferenceRequest,
        cancel: CancellationToken,
        recover: bool,
    ) -> Result<InferenceOutput> {
        match self {
            Self::Local(m) if !recover => super::infer(m, &request, cancel).await,
            Self::Host(m) => m.infer(id, request, cancel, recover).await,
            _ => Err(Error::AlreadySubmitted),
        }
    }
}
impl MemoryEngine<'_> {
    pub fn configure_host_integration(&self, model: HostModel) -> Result<()> {
        let mut s = self.chat.state()?;
        model.validate(s.cache.namespace())?;
        if model.info.purpose != Purpose::Integration || model.info.space.is_some() {
            return Err(Error::Invalid);
        }
        store::authorize(&s, &model.scope)?;
        s.memory.guards.push(model.connection.guard.clone());
        s.memory.cancel_all();
        s.memory.integration = Some(Executor::Host(Box::new(model)));
        Ok(())
    }
    pub fn configure_host_embedding(&self, model: HostModel) -> Result<()> {
        let mut s = self.chat.state()?;
        model.validate(s.cache.namespace())?;
        let space = model.info.space.clone().ok_or(Error::Invalid)?;
        space.id()?;
        if model.info.purpose != Purpose::Embedding || space.target != model.info.target {
            return Err(Error::Invalid);
        }
        store::authorize(&s, &model.scope)?;
        s.memory.guards.push(model.connection.guard.clone());
        s.memory.cancel_all();
        s.memory.embedding = Some((Executor::Host(Box::new(model)), space));
        Ok(())
    }
}
