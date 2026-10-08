use crate::Failure;
use axum::{
    body::{Body, Bytes},
    http::{HeaderValue, StatusCode},
    response::Response,
};
use caidex_model_core::{CancellationToken, ContextHeaders, ProviderStream, ProviderStreamEvent};
use futures_util::StreamExt;
use std::{convert::Infallible, future::Future};
use tokio::{sync::OwnedSemaphorePermit, time::Instant};

pub(crate) async fn guard<T>(
    operation: impl Future<Output = T>,
    cancellation: CancellationToken,
    deadline: Instant,
) -> Result<T, Failure> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(Failure::new(StatusCode::SERVICE_UNAVAILABLE,"gateway_stopped")),
        _ = tokio::time::sleep_until(deadline) => Err(Failure::new(StatusCode::GATEWAY_TIMEOUT,"provider_timeout")),
        result = operation => Ok(result),
    }
}

pub(crate) fn headers(response: &mut Response, context: &ContextHeaders) -> Result<(), Failure> {
    for (name, value) in context.iter() {
        if !caidex_model_core::RESPONSE_HEADERS.contains(&name) {
            return Err(Failure::new(
                StatusCode::BAD_GATEWAY,
                "provider_invalid_context_header",
            ));
        }
        let mut value = HeaderValue::from_str(value).expect("validated header");
        value.set_sensitive(true);
        response.headers_mut().insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).expect("allowed name"),
            value,
        );
    }
    Ok(())
}

pub(crate) fn stream(
    events: ProviderStream,
    cancellation: CancellationToken,
    deadline: Instant,
    permit: OwnedSemaphorePermit,
) -> Body {
    Body::from_stream(futures_util::stream::unfold(
        (Some(events), cancellation, deadline, permit),
        |(events, cancellation, deadline, permit)| async move {
            let mut events = events?;
            let next = guard(events.next(), cancellation.clone(), deadline).await;
            let (bytes, events) = match next {
                Ok(Some(Ok(ProviderStreamEvent::Heartbeat))) => {
                    (Bytes::from_static(b": caidex keepalive\n\n"), Some(events))
                }
                Ok(Some(Ok(ProviderStreamEvent::Model(event)))) => {
                    let mut data =
                        format!("event: {}\nid: {}\n", event.frame.event, event.frame.id);
                    if let Some(retry) = event.frame.retry_ms {
                        data.push_str(&format!("retry: {retry}\n"));
                    }
                    for line in event.frame.data.split('\n') {
                        data.push_str("data: ");
                        data.push_str(line);
                        data.push('\n');
                    }
                    data.push('\n');
                    (Bytes::from(data), Some(events))
                }
                Ok(None) => return None,
                error => {
                    let error = match error {
                        Ok(Some(Err(error))) => error.wire(),
                        Err(error) => error.wire(),
                        _ => unreachable!("exhaustive provider events"),
                    };
                    (
                        Bytes::from(format!(
                            "event: response.failed\ndata: {}\n\n",
                            // Fixed Runtime ignores generic error events except
                            // flex-unavailable. Keep static errors observable.
                            serde_json::json!({"type":"response.failed","response":{"status":"failed","error":error}})
                        )),
                        None,
                    )
                }
            };
            Some((
                Ok::<_, Infallible>(bytes),
                (events, cancellation, deadline, permit),
            ))
        },
    ))
}
