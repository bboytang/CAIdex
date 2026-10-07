use crate::{Failure, GatewayState};
use axum::{
    body::{Body, Bytes},
    http::StatusCode,
};
use caidex_credentials::SecretStore;
use caidex_model_core::{ResponsesStream, StreamEvent, StreamState, Usage};
use futures_util::Stream;
use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::{
    sync::{OwnedSemaphorePermit, mpsc, watch},
    task::JoinHandle,
    time::Instant,
};

pub(crate) async fn cancelled(mut shutdown: watch::Receiver<bool>) {
    let _ = shutdown.wait_for(|closed| *closed).await;
}

pub(crate) async fn guard<T>(
    operation: impl Future<Output = T>,
    shutdown: watch::Receiver<bool>,
    deadline: Instant,
) -> std::result::Result<T, Failure> {
    tokio::select! {
        biased;
        _ = cancelled(shutdown) => Err(Failure::new(StatusCode::SERVICE_UNAVAILABLE, "gateway_stopped")),
        _ = tokio::time::sleep_until(deadline) => Err(Failure::new(StatusCode::GATEWAY_TIMEOUT, "provider_timeout")),
        result = operation => Ok(result),
    }
}

struct Delivery {
    receiver: mpsc::Receiver<Bytes>,
    worker: JoinHandle<()>,
    _permit: OwnedSemaphorePermit,
}
impl Stream for Delivery {
    type Item = std::result::Result<Bytes, Infallible>;
    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(context).map(|value| value.map(Ok))
    }
}
impl Drop for Delivery {
    fn drop(&mut self) {
        // Dropping the HTTP body aborts the producer and drops its reqwest body,
        // including a pending read. It cannot keep generating after disconnect.
        self.worker.abort();
    }
}

pub(crate) fn stream<S: SecretStore + 'static>(
    upstream: reqwest::Response,
    state: Arc<GatewayState<S>>,
    deadline: Instant,
    permit: OwnedSemaphorePermit,
) -> Body {
    let (sender, receiver) = mpsc::channel(1);
    let worker = tokio::spawn(async move {
        if let Err(failure) = pump(upstream, &state, deadline, &sender).await {
            // If backpressure outlasted the deadline, close rather than wait
            // indefinitely to deliver a diagnostic. EOF still cannot be success.
            let error = Bytes::from(format!(
                "event: error\ndata: {}\n\n",
                serde_json::json!({"type":"error", "error":failure.wire()})
            ));
            if let Err(mpsc::error::TrySendError::Full(error)) = sender.try_send(error) {
                let _ = guard(sender.send(error), state.shutdown.clone(), deadline).await;
            }
        }
    });
    Body::from_stream(Delivery {
        receiver,
        worker,
        _permit: permit,
    })
}

fn encode<S: SecretStore>(mut event: StreamEvent, state: &GatewayState<S>) -> Bytes {
    // Only diagnostic subtrees are redacted. Opaque history/tool strings stay
    // unchanged; the whole provider wire is never treated as a log message.
    let mut wire = event.response.wire().clone();
    let mut diagnostic = false;
    if event.response.kind() == "error" {
        // Responses also puts code/message/param directly on the error event.
        wire = state.broker.redactor().json(&wire);
        diagnostic = true;
    }
    if event.response.kind() == "response.failed"
        && let Some(error) = wire
            .get_mut("response")
            .and_then(|response| response.get_mut("error"))
    {
        *error = state.broker.redactor().json(error);
        diagnostic = true;
    }
    if diagnostic {
        event.frame.data = wire.to_string();
    }
    let mut encoded = format!("event: {}\n", event.frame.event);
    encoded.push_str(&format!("id: {}\n", event.frame.id));
    if let Some(retry) = event.frame.retry_ms {
        encoded.push_str(&format!("retry: {retry}\n"));
    }
    for line in event.frame.data.split('\n') {
        encoded.push_str("data: ");
        encoded.push_str(line);
        encoded.push('\n');
    }
    encoded.push('\n');
    Bytes::from(encoded)
}

async fn pump<S: SecretStore>(
    mut upstream: reqwest::Response,
    state: &GatewayState<S>,
    deadline: Instant,
    sender: &mpsc::Sender<Bytes>,
) -> std::result::Result<(), Failure> {
    let mut parser = ResponsesStream::new(state.limits.frame_bytes).expect("validated limit");
    let mut last_delivery = Instant::now();
    loop {
        let chunk = guard(
            upstream.chunk(),
            state.shutdown.clone(),
            deadline.min(Instant::now() + state.limits.idle_timeout),
        )
        .await?
        .map_err(Failure::transport)?;
        let Some(chunk) = chunk else {
            return parser
                .finish()
                .map(|_| ())
                .map_err(|_| Failure::new(StatusCode::BAD_GATEWAY, "provider_stream_truncated"));
        };
        // Process one small slice at a time, even if transport coalesced frames.
        // The single-slot channel is the only producer/consumer queue.
        for slice in chunk.chunks(16 * 1024) {
            let events = parser
                .push(slice)
                .map_err(|_| Failure::new(StatusCode::BAD_GATEWAY, "provider_invalid_stream"))?;
            for event in events {
                guard(
                    sender.send(encode(event, state)),
                    state.shutdown.clone(),
                    deadline,
                )
                .await?
                .map_err(|_| Failure::new(StatusCode::BAD_GATEWAY, "client_disconnected"))?;
                last_delivery = Instant::now();
            }
        }
        if last_delivery.elapsed() >= std::time::Duration::from_secs(1) {
            // Preserve upstream byte activity for downstream idle timers without
            // forwarding unvalidated partial JSON or synthesizing model events.
            guard(
                sender.send(Bytes::from_static(b": caidex keepalive\n\n")),
                state.shutdown.clone(),
                deadline,
            )
            .await?
            .map_err(|_| Failure::new(StatusCode::BAD_GATEWAY, "client_disconnected"))?;
            last_delivery = Instant::now();
        }
        if parser.state() != StreamState::Open {
            // A terminal response ends this HTTP generation immediately; do not
            // wait for a provider that keeps the socket open after completion.
            return Ok(());
        }
    }
}

pub(crate) async fn json<S: SecretStore>(
    mut upstream: reqwest::Response,
    state: &GatewayState<S>,
    deadline: Instant,
) -> std::result::Result<Vec<u8>, Failure> {
    let invalid = || Failure::new(StatusCode::BAD_GATEWAY, "provider_invalid_response");
    let mut bytes = Vec::new();
    loop {
        let chunk = guard(
            upstream.chunk(),
            state.shutdown.clone(),
            deadline.min(Instant::now() + state.limits.idle_timeout),
        )
        .await?
        .map_err(Failure::transport)?;
        let Some(chunk) = chunk else {
            break;
        };
        if chunk.len() > state.limits.response_bytes.saturating_sub(bytes.len()) {
            return Err(Failure::new(
                StatusCode::BAD_GATEWAY,
                "provider_response_too_large",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let wire: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if !wire.is_object()
        || !wire
            .get("id")
            .and_then(|value| value.as_str())
            .is_some_and(|id| !id.is_empty())
        || !matches!(
            wire.get("status").and_then(|value| value.as_str()),
            Some("completed" | "incomplete" | "failed")
        )
        || !wire["output"].is_array()
    {
        return Err(invalid());
    }
    if let Some(usage) = wire.get("usage").filter(|value| !value.is_null()) {
        Usage::new(usage).map_err(|_| invalid())?;
    }
    if let Some(error) = wire.get("error").filter(|value| !value.is_null()) {
        let mut sanitized = wire.clone();
        sanitized["error"] = state.broker.redactor().json(error);
        return serde_json::to_vec(&sanitized).map_err(|_| invalid());
    }
    Ok(bytes)
}
