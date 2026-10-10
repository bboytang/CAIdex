use crate::{ProviderState, transport};
use caidex_credentials::SecretStore;
use caidex_model_core::{
    CancellationToken, CanonicalResponse, ProviderError, ProviderResult, ProviderStream,
    ProviderStreamEvent, RequestContext, ResponseEvent, ResponsesStream, StreamEvent, StreamState,
};
use futures_util::Stream;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    sync::{OwnedSemaphorePermit, mpsc},
    task::JoinHandle,
    time::Instant,
};

pub(crate) async fn guard<T>(
    operation: impl Future<Output = T>,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> ProviderResult<T> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(ProviderError::new(503,"provider_cancelled")),
        _ = tokio::time::sleep_until(deadline) => Err(ProviderError::new(504,"provider_timeout")),
        result = operation => Ok(result),
    }
}

struct Delivery {
    receiver: mpsc::Receiver<ProviderStreamEvent>,
    worker: JoinHandle<()>,
    failure: Arc<Mutex<Option<ProviderError>>>,
    terminal: bool,
    _permit: OwnedSemaphorePermit,
}
impl Stream for Delivery {
    type Item = ProviderResult<ProviderStreamEvent>;
    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.receiver.poll_recv(context) {
            Poll::Ready(Some(event)) => {
                if let ProviderStreamEvent::Model(event) = &event
                    && event.response.terminal().is_some()
                {
                    self.terminal = true;
                }
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(None) if !self.terminal => {
                self.terminal = true;
                let failure = self
                    .failure
                    .lock()
                    .ok()
                    .and_then(|mut failure| failure.take())
                    .unwrap_or_else(|| ProviderError::new(502, "provider_stream_truncated"));
                Poll::Ready(Some(Err(failure)))
            }
            result => result.map(|event| event.map(Ok)),
        }
    }
}
impl Drop for Delivery {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

pub(crate) fn stream<S: SecretStore + 'static>(
    upstream: reqwest::Response,
    state: Arc<ProviderState<S>>,
    context: RequestContext,
    deadline: Instant,
    permit: OwnedSemaphorePermit,
) -> ProviderStream {
    let (sender, receiver) = mpsc::channel(1);
    let failure = Arc::new(Mutex::new(None));
    let worker_failure = failure.clone();
    let worker = tokio::spawn(async move {
        if let Err(error) = pump(upstream, &state, &context, deadline, &sender).await
            && let Ok(mut failure) = worker_failure.lock()
        {
            *failure = Some(error);
        }
        // Dropping the sender closes the single slot. Delivery reports the saved
        // failure after draining queued events, even if cancellation hit a full
        // queue. It never blocks trying to enqueue an error into backpressure.
    });
    Box::pin(Delivery {
        receiver,
        worker,
        failure,
        terminal: false,
        _permit: permit,
    })
}

fn sanitize<S: SecretStore>(mut event: StreamEvent, state: &ProviderState<S>) -> StreamEvent {
    let mut wire = event.response.wire().clone();
    let mut diagnostic = false;
    if event.response.kind() == "error" {
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
        event.response = ResponseEvent::new(wire).expect("redaction preserves event structure");
    }
    event
}

async fn pump<S: SecretStore>(
    mut upstream: reqwest::Response,
    state: &ProviderState<S>,
    context: &RequestContext,
    deadline: Instant,
    sender: &mpsc::Sender<ProviderStreamEvent>,
) -> ProviderResult<()> {
    let mut parser = ResponsesStream::new(state.limits.frame_bytes).expect("validated limit");
    let mut last_delivery = Instant::now();
    loop {
        let chunk = guard(
            upstream.chunk(),
            &context.cancellation,
            deadline.min(Instant::now() + state.limits.idle_timeout),
        )
        .await?
        .map_err(transport)?;
        let Some(chunk) = chunk else {
            return parser
                .finish()
                .map(|_| ())
                .map_err(|_| ProviderError::new(502, "provider_stream_truncated"));
        };
        for slice in chunk.chunks(16 * 1024) {
            for event in parser
                .push(slice)
                .map_err(|_| ProviderError::new(502, "provider_invalid_stream"))?
            {
                guard(
                    sender.send(ProviderStreamEvent::Model(sanitize(event, state))),
                    &context.cancellation,
                    deadline,
                )
                .await?
                .map_err(|_| ProviderError::new(502, "client_disconnected"))?;
                last_delivery = Instant::now();
            }
        }
        if last_delivery.elapsed() >= Duration::from_secs(1) {
            guard(
                sender.send(ProviderStreamEvent::Heartbeat),
                &context.cancellation,
                deadline,
            )
            .await?
            .map_err(|_| ProviderError::new(502, "client_disconnected"))?;
            last_delivery = Instant::now();
        }
        if parser.state() != StreamState::Open {
            return Ok(());
        }
    }
}

pub(crate) async fn json<S: SecretStore>(
    upstream: reqwest::Response,
    state: &ProviderState<S>,
    context: &RequestContext,
    deadline: Instant,
) -> ProviderResult<CanonicalResponse> {
    CanonicalResponse::new(value(upstream, state, context, deadline).await?)
        .map_err(|_| ProviderError::new(502, "provider_invalid_response"))
}

pub(crate) async fn value<S: SecretStore>(
    mut upstream: reqwest::Response,
    state: &ProviderState<S>,
    context: &RequestContext,
    deadline: Instant,
) -> ProviderResult<serde_json::Value> {
    let mut bytes = Vec::new();
    loop {
        let chunk = guard(
            upstream.chunk(),
            &context.cancellation,
            deadline.min(Instant::now() + state.limits.idle_timeout),
        )
        .await?
        .map_err(transport)?;
        let Some(chunk) = chunk else {
            break;
        };
        if chunk.len() > state.limits.response_bytes.saturating_sub(bytes.len()) {
            return Err(ProviderError::new(502, "provider_response_too_large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let mut wire: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| ProviderError::new(502, "provider_invalid_response"))?;
    if let Some(error) = wire.get_mut("error").filter(|value| !value.is_null()) {
        *error = state.broker.redactor().json(error);
    }
    Ok(wire)
}

/// Protocol-neutral framing; errors are classifications, never raw native data.
pub struct NativeStreamingResponse {
    pub events: Pin<Box<dyn Stream<Item = ProviderResult<caidex_model_core::SseEvent>> + Send>>,
    pub headers: caidex_model_core::ContextHeaders,
}

pub(crate) fn native_stream<S: SecretStore + 'static>(
    upstream: reqwest::Response,
    state: Arc<ProviderState<S>>,
    context: RequestContext,
    deadline: Instant,
    permit: OwnedSemaphorePermit,
) -> Pin<Box<dyn Stream<Item = ProviderResult<caidex_model_core::SseEvent>> + Send>> {
    struct NativeDelivery {
        receiver: mpsc::Receiver<caidex_model_core::SseEvent>,
        worker: JoinHandle<()>,
        failure: Arc<Mutex<Option<ProviderError>>>,
        cancellation: CancellationToken,
        deadline: Instant,
        closed: bool,
    }
    impl Stream for NativeDelivery {
        type Item = ProviderResult<caidex_model_core::SseEvent>;
        fn poll_next(
            mut self: Pin<&mut Self>,
            context: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            if self.closed {
                return Poll::Ready(None);
            }
            let failure = if self.cancellation.is_cancelled() {
                Some(ProviderError::new(503, "provider_cancelled"))
            } else if Instant::now() >= self.deadline {
                Some(ProviderError::new(504, "provider_timeout"))
            } else {
                self.failure
                    .lock()
                    .ok()
                    .and_then(|mut failure| failure.take())
            };
            if let Some(error) = failure {
                self.closed = true;
                self.worker.abort();
                return Poll::Ready(Some(Err(error)));
            }
            match self.receiver.poll_recv(context) {
                Poll::Ready(None) => {
                    self.closed = true;
                    Poll::Ready(None)
                }
                result => result.map(|event| event.map(Ok)),
            }
        }
    }
    impl Drop for NativeDelivery {
        fn drop(&mut self) {
            self.worker.abort();
        }
    }
    let (sender, receiver) = mpsc::channel(1);
    let failure = Arc::new(Mutex::new(None));
    let worker_failure = failure.clone();
    let cancellation = context.cancellation.clone();
    let worker_cancellation = cancellation.clone();
    let worker = tokio::spawn(async move {
        let _permit = permit;
        let result: ProviderResult<()> = async {
            let mut upstream = upstream;
            let mut parser = caidex_model_core::SseDecoder::new(state.limits.frame_bytes)
                .expect("validated limit");
            let mut bytes = 0usize;
            loop {
                let chunk = guard(
                    upstream.chunk(),
                    &worker_cancellation,
                    deadline.min(Instant::now() + state.limits.idle_timeout),
                )
                .await?
                .map_err(transport)?;
                let Some(chunk) = chunk else {
                    return if parser.has_pending_frame() {
                        Err(ProviderError::new(502, "provider_stream_truncated"))
                    } else {
                        Ok(())
                    };
                };
                if chunk.len() > state.limits.response_bytes.saturating_sub(bytes) {
                    return Err(ProviderError::new(502, "provider_response_too_large"));
                }
                bytes += chunk.len();
                for slice in chunk.chunks(16 * 1024) {
                    for frame in parser
                        .push(slice)
                        .map_err(|_| ProviderError::new(502, "provider_invalid_stream"))?
                    {
                        guard(sender.send(frame), &worker_cancellation, deadline)
                            .await?
                            .map_err(|_| ProviderError::new(502, "client_disconnected"))?;
                    }
                }
            }
        }
        .await;
        if let Err(error) = result
            && let Ok(mut failure) = worker_failure.lock()
        {
            *failure = Some(error);
        }
    });
    Box::pin(NativeDelivery {
        receiver,
        worker,
        failure,
        cancellation,
        deadline,
        closed: false,
    })
}
