use crate::{
    Limits, MessageEvent, MessageStream, NativeMessage, NativeStreamState, client::transport,
};
use caidex_model_core::{ProviderError, ProviderResult, RequestContext};
use futures_util::Stream;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::{
    sync::{OwnedSemaphorePermit, mpsc},
    task::JoinHandle,
    time::Instant,
};

/// Only inference data is delivered. Native error messages are replaced by
/// static classifications, never echoed as SSE frames or diagnostic strings.
#[derive(Debug)]
pub enum NativeStreamEvent {
    Event(MessageEvent),
    Completed(NativeMessage),
}
pub struct NativeStreamingResponse {
    receiver: mpsc::Receiver<NativeStreamEvent>,
    worker: JoinHandle<()>,
    failure: Arc<Mutex<Option<ProviderError>>>,
    terminal: bool,
}
impl Stream for NativeStreamingResponse {
    type Item = ProviderResult<NativeStreamEvent>;
    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.receiver.poll_recv(context) {
            Poll::Ready(Some(event)) => {
                if matches!(event, NativeStreamEvent::Completed(_)) {
                    self.terminal = true;
                }
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(None) if !self.terminal => {
                self.terminal = true;
                let error = self
                    .failure
                    .lock()
                    .ok()
                    .and_then(|mut error| error.take())
                    .unwrap_or_else(|| ProviderError::new(502, "anthropic_stream_truncated"));
                Poll::Ready(Some(Err(error)))
            }
            result => result.map(|event| event.map(Ok)),
        }
    }
}
impl Drop for NativeStreamingResponse {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
pub(crate) async fn guard<T>(
    operation: impl Future<Output = T>,
    context: &RequestContext,
    deadline: Instant,
) -> ProviderResult<T> {
    tokio::select! {
        biased;
        _ = context.cancellation.cancelled() => Err(ProviderError::new(503, "provider_cancelled")),
        _ = tokio::time::sleep_until(deadline) => Err(ProviderError::new(504, "provider_timeout")),
        result = operation => Ok(result),
    }
}
pub(crate) fn stream(
    upstream: reqwest::Response,
    context: RequestContext,
    deadline: Instant,
    limits: Limits,
    permit: OwnedSemaphorePermit,
) -> NativeStreamingResponse {
    let (sender, receiver) = mpsc::channel(1);
    let failure = Arc::new(Mutex::new(None));
    let worker_failure = failure.clone();
    let worker = tokio::spawn(async move {
        // The worker owns the permit: timeout/cancellation also releases it when
        // delivery remains unpolled, rather than blocking a subsequent request.
        let _permit = permit;
        if let Err(error) = pump(upstream, &context, deadline, limits, &sender).await
            && let Ok(mut failure) = worker_failure.lock()
        {
            *failure = Some(error);
        }
        // Save failure out of band. A full data slot cannot hide cancellation
        // or deadline by making the worker wait to enqueue its error.
    });
    NativeStreamingResponse {
        receiver,
        worker,
        failure,
        terminal: false,
    }
}
async fn pump(
    mut upstream: reqwest::Response,
    context: &RequestContext,
    deadline: Instant,
    limits: Limits,
    sender: &mpsc::Sender<NativeStreamEvent>,
) -> ProviderResult<()> {
    let mut parser = MessageStream::new(limits.frame_bytes, limits.response_bytes)?;
    loop {
        let chunk = guard(
            upstream.chunk(),
            context,
            deadline.min(Instant::now() + limits.idle_timeout),
        )
        .await?
        .map_err(transport)?;
        let Some(chunk) = chunk else {
            parser.finish()?;
            return Err(ProviderError::new(502, "anthropic_stream_truncated"));
        };
        for slice in chunk.chunks(16 * 1024) {
            for event in parser.push(slice)? {
                if event.kind() == "error" {
                    return Err(native_error(event.wire()));
                }
                guard(
                    sender.send(NativeStreamEvent::Event(event)),
                    context,
                    deadline,
                )
                .await?
                .map_err(|_| ProviderError::new(503, "provider_cancelled"))?;
            }
        }
        if parser.state() == NativeStreamState::Completed {
            let message = parser
                .completed_message()
                .expect("completed message")
                .clone();
            guard(
                sender.send(NativeStreamEvent::Completed(message)),
                context,
                deadline,
            )
            .await?
            .map_err(|_| ProviderError::new(503, "provider_cancelled"))?;
            return Ok(());
        }
    }
}
fn native_error(wire: &serde_json::Value) -> ProviderError {
    let (status, code) = match wire["error"]["type"].as_str() {
        Some("authentication_error") => (401, "provider_authentication_failed"),
        Some("permission_error") => (403, "provider_authentication_failed"),
        Some("rate_limit_error") => (429, "provider_rate_limited"),
        Some("overloaded_error") => (529, "provider_unavailable"),
        Some("invalid_request_error") => (400, "provider_request_rejected"),
        Some("api_error") => (500, "provider_unavailable"),
        _ => (502, "provider_stream_error"),
    };
    ProviderError::new(status, code)
}
