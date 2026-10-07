use crate::{
    ContentEvent, ContentStream, Limits, NativeStreamResponse,
    client::{guard, transport},
};
use caidex_model_core::{ContextHeaders, ProviderError, ProviderResult, RequestContext};
use futures_util::Stream;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::{
    sync::{OwnedSemaphorePermit, mpsc},
    task::JoinHandle,
    time::Instant,
};

/// Inference data only. Errors are static classifications, never raw native
/// error frames or diagnostics containing a credential, URL or response body.
#[derive(Debug)]
pub enum NativeStreamEvent {
    Event(ContentEvent),
    Completed(NativeStreamResponse),
}
pub struct NativeStreamingResponse {
    headers: ContextHeaders,
    receiver: mpsc::Receiver<NativeStreamEvent>,
    worker: JoinHandle<()>,
    failure: Arc<Mutex<Option<ProviderError>>>,
    terminal: bool,
}
impl NativeStreamingResponse {
    pub fn headers(&self) -> &ContextHeaders {
        &self.headers
    }
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
                    .unwrap_or_else(|| ProviderError::new(502, "google_stream_truncated"));
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
pub(crate) fn stream(
    upstream: reqwest::Response,
    context: RequestContext,
    deadline: Instant,
    limits: Limits,
    permit: OwnedSemaphorePermit,
    expected_candidates: usize,
    headers: ContextHeaders,
) -> NativeStreamingResponse {
    let (sender, receiver) = mpsc::channel(1);
    let failure = Arc::new(Mutex::new(None));
    let worker_failure = failure.clone();
    let worker = tokio::spawn(async move {
        // Timeout/cancellation releases the slot even without receiver polls.
        let _permit = permit;
        if let Err(error) = pump(
            upstream,
            &context,
            deadline,
            limits,
            &sender,
            expected_candidates,
        )
        .await
            && let Ok(mut failure) = worker_failure.lock()
        {
            *failure = Some(error);
        }
        // A full data slot must not block delivery of the terminal error.
    });
    NativeStreamingResponse {
        headers,
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
    expected_candidates: usize,
) -> ProviderResult<()> {
    let mut parser = ContentStream::new(
        limits.frame_bytes,
        limits.response_bytes,
        expected_candidates,
    )?;
    loop {
        let chunk = guard(
            upstream.chunk(),
            context,
            deadline.min(Instant::now() + limits.idle_timeout),
        )
        .await?
        .map_err(transport)?;
        let Some(chunk) = chunk else {
            // Only a normal HTTP body EOF may complete candidate history.
            parser.finish()?;
            let complete = parser
                .take_completed_response()
                .expect("completed native stream");
            drop(upstream);
            guard(
                sender.send(NativeStreamEvent::Completed(complete)),
                context,
                deadline,
            )
            .await?
            .map_err(|_| ProviderError::new(503, "provider_cancelled"))?;
            return Ok(());
        };
        for slice in chunk.chunks(16 * 1024) {
            for event in parser.push(slice)? {
                guard(
                    sender.send(NativeStreamEvent::Event(event)),
                    context,
                    deadline,
                )
                .await?
                .map_err(|_| ProviderError::new(503, "provider_cancelled"))?;
            }
        }
    }
}
