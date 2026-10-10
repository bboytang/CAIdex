use super::*;
use caidex_model_core::{ModelMetadata, ModelProvider, ModelRouter};
use caidex_provider_custom::{ConfiguredModel, CustomResponsesProvider};
use caidex_provider_openrouter::{OpenRouterConfig, OpenRouterProvider};

const ROUTER_KEY: &str = "CAIDEX_SYNTHETIC_OPENROUTER_KEY";

async fn routed(
    native: &Fixture,
    custom: &Fixture,
) -> (RunningGateway, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let native_reads = Arc::new(AtomicUsize::new(0));
    let custom_reads = Arc::new(AtomicUsize::new(0));
    let native_broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store {
            value: Some(ROUTER_KEY),
            reads: native_reads.clone(),
        },
    ));
    let custom_broker = Arc::new(Broker::new(
        Id::new("executor").unwrap(),
        Store {
            value: Some(KEY),
            reads: custom_reads.clone(),
        },
    ));
    let mut credential = reference("executor");
    credential.provider = Id::new("openrouter").unwrap();
    let native_provider: Arc<dyn ModelProvider> = Arc::new(
        OpenRouterProvider::new(
            OpenRouterConfig::new(credential)
                .unwrap()
                .with_base_url(&format!("http://{}/v1/", native.address))
                .unwrap(),
            vec![ModelMetadata::configured(
                "native".into(),
                "native-router".into(),
                vec![ResponsesDialect::Classic],
            )],
            native_broker.clone(),
            Limits::default(),
        )
        .unwrap(),
    );
    let custom_provider: Arc<dyn ModelProvider> = Arc::new(
        CustomResponsesProvider::new(
            vec![
                ConfiguredModel::new(
                    "custom".into(),
                    "native-custom".into(),
                    vec![ResponsesDialect::Classic, ResponsesDialect::Lite],
                    CustomResponses::new(&custom.endpoint(), Some(reference("executor"))).unwrap(),
                )
                .unwrap(),
            ],
            custom_broker,
            Limits::default(),
        )
        .unwrap(),
    );
    let router = ModelRouter::new(vec![
        ("native".into(), native_provider),
        ("custom".into(), custom_provider),
    ])
    .unwrap();
    let gateway = caidex_model_gateway::start_with_provider(
        Arc::new(router),
        native_broker.redactor(),
        Limits::default(),
    )
    .await
    .unwrap();
    (gateway, native_reads, custom_reads)
}

#[tokio::test]
async fn gateway_router_selects_native_and_custom_adapters_with_isolated_credentials() {
    let output =
        json!({"id":"fixture","status":"completed","output":[],"future":18446744073709551616_u128});
    let scenario = Scenario::Reply {
        status: 200,
        content_type: "application/json",
        extra: String::new(),
        body: output.to_string().into_bytes(),
        fragmented: true,
    };
    let mut native = Fixture::start(scenario.clone()).await;
    let mut custom = Fixture::start(scenario).await;
    let (gateway, native_reads, custom_reads) = routed(&native, &custom).await;
    assert_eq!(native_reads.load(Ordering::SeqCst), 0);
    assert_eq!(custom_reads.load(Ordering::SeqCst), 0);
    for (id, fixture, key, native_model) in [
        ("native", &mut native, ROUTER_KEY, "native-router"),
        ("custom", &mut custom, KEY, "native-custom"),
    ] {
        let response = request(
            &client(),
            &gateway,
            json!({"model":id,"input":[],"stream":false}),
        )
        .send()
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<Value>(&response.bytes().await.unwrap()).unwrap(),
            output
        );
        let captured = fixture.request().await;
        assert_eq!(captured.body["model"], native_model);
        assert!(captured.headers.contains(key));
        assert!(!captured.headers.contains(gateway.token().expose()));
        assert!(
            !captured
                .headers
                .contains(if id == "native" { KEY } else { ROUTER_KEY })
        );
    }
    assert_eq!(native_reads.load(Ordering::SeqCst), 1);
    assert_eq!(custom_reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn gateway_router_streams_both_adapters_and_rejects_foreign_history_without_fallback() {
    let completed = json!({"type":"response.completed","response":{"id":"fixture","status":"completed","output":[]}});
    let scenario = sse(format!(
        "{CREATED}event: response.completed\ndata: {completed}\n\n"
    ));
    let mut native = Fixture::start(scenario.clone()).await;
    let mut custom = Fixture::start(scenario).await;
    let (gateway, native_reads, custom_reads) = routed(&native, &custom).await;
    for (id, fixture) in [("native", &mut native), ("custom", &mut custom)] {
        let response = request(
            &client(),
            &gateway,
            json!({"model":id,"input":[],"stream":true}),
        )
        .send()
        .await
        .unwrap();
        assert_eq!(wire(response).await.0, StreamState::Completed);
        fixture.request().await;
    }
    let before = (
        native_reads.load(Ordering::SeqCst),
        custom_reads.load(Ordering::SeqCst),
    );
    let response = request(&client(), &gateway, json!({"model":"native","input":[]}))
        .header("x-openai-internal-codex-responses-lite", "true")
        .send()
        .await
        .unwrap();
    error(response, StatusCode::BAD_REQUEST, "unsupported_dialect").await;
    let response = request(&client(), &gateway, json!({"model":"unknown","input":[]}))
        .send()
        .await
        .unwrap();
    error(response, StatusCode::NOT_FOUND, "unknown_model").await;
    let response=request(&client(),&gateway,json!({"model":"native","input":[{"type":"reasoning","encrypted_content":"foreign-signature"}]})).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        (
            native_reads.load(Ordering::SeqCst),
            custom_reads.load(Ordering::SeqCst)
        ),
        before
    );
    assert_eq!(native.accepted.load(Ordering::SeqCst), 1);
    assert_eq!(custom.accepted.load(Ordering::SeqCst), 1);
}
