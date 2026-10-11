//! Internal I-3 demonstration client, not the formal CLI or GUI.
use caidex_auth::Trust;
use caidex_chat::{Chat, Content, End, LocalModel, Namespace, Send};
use caidex_cloud_core::Id;
use caidex_credentials::{Broker, EnvironmentStore};
use caidex_model_core::{ModelProvider, ModelRouter, ResponsesDialect};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider, Limits};
use serde_json::json;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use uuid::Uuid;

fn model(endpoint: &str, namespace: &Namespace) -> LocalModel {
    let broker = Arc::new(Broker::new(
        namespace.credential_owner(),
        EnvironmentStore::new(HashMap::new()).unwrap(),
    ));
    let config = ConfiguredModel::new(
        "local-fixture".into(),
        "synthetic".into(),
        vec![ResponsesDialect::Classic],
        CustomResponses::new(endpoint, None).unwrap(),
    )
    .unwrap();
    let provider: Arc<dyn ModelProvider> =
        Arc::new(CustomResponsesProvider::new(vec![config], broker, Limits::default()).unwrap());
    LocalModel {
        provider: Arc::new(ModelRouter::new(vec![("local-fixture".into(), provider)]).unwrap()),
        model: "local-fixture".into(),
    }
}
fn request(conversation: Uuid, parent: Option<Uuid>, text: &str) -> Send {
    Send {
        operation: Uuid::new_v4(),
        conversation,
        parent,
        content: Content::text(text),
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: local_chat PRIVATE_ROOT LOOPBACK_RESPONSES_URL".into());
    }
    let root = PathBuf::from(&args[1]);
    let endpoint = &args[2];
    // This example only talks to a local synthetic server and never reads an env key.
    if !endpoint.starts_with("http://127.0.0.1:") {
        return Err("fixture requires literal loopback HTTP".into());
    }
    let guest = Namespace::guest();
    let trust = Trust::local_test("http://localhost:3333/")?;
    let a = Namespace::offline_account(
        &trust,
        &Id::try_from("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned())?,
    );
    let b = Namespace::offline_account(
        &trust,
        &Id::try_from("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_owned())?,
    );
    let chat = Chat::open(&root, guest.clone())?;
    chat.configure(model(endpoint, &guest))?;
    let g = chat.create_conversation(None, "Guest local".into())?;
    let attachment = chat.attach(
        g,
        "fixture.txt".into(),
        "text/plain".into(),
        b"local synthetic attachment".to_vec(),
    )?;
    let mut first = request(g, None, "Hello fixture");
    first.content.attachments.push(attachment);
    let reply = chat.send(first).await?;
    assert_eq!(reply.end, End::Complete);
    assert_eq!(reply.content.text, "Synthetic Chat reply");
    let (left, right) = tokio::join!(
        chat.send(request(g, Some(reply.id), "left branch")),
        chat.send(request(g, Some(reply.id), "right branch"))
    );
    assert_ne!(left?.parent, right?.parent);
    chat.switch(a.clone())?;
    assert!(chat.conversations(None, 100)?.is_empty());
    assert!(chat.outbox(0, 100)?.is_empty());
    let ac = chat.create_conversation(None, "A private".into())?;
    chat.configure(model(endpoint, &a))?;
    chat.send(request(ac, None, "A only")).await?;
    chat.switch(b.clone())?;
    assert!(chat.messages(ac, 0, 100).is_err());
    assert!(chat.attachment(g, attachment).is_err());
    assert!(chat.outbox(0, 100)?.is_empty());
    let bc = chat.create_conversation(None, "B private".into())?;
    assert!(
        chat.send(request(bc, None, "model must be separately configured"))
            .await
            .is_err()
    );
    chat.logout()?;
    assert_eq!(chat.conversations(None, 100)?[0].id, g);
    assert_eq!(chat.messages(g, 0, 100)?.len(), 6);
    drop(chat);
    let reopened = Chat::open(&root, a)?;
    assert_eq!(reopened.messages(ac, 0, 100)?.len(), 2);
    println!(
        "{}",
        json!({"milestone":"I-3","transport":"loopback HTTP SSE via CustomResponsesProvider + ModelRouter","commercial_api":false,"authenticated_cloud":false,"guest_messages":6,"account_a_messages":2,"attachments_persisted":true,"concurrent_branches":true,"account_b_cannot_read_a":true,"guest_not_adopted":true,"switch_clears_model":true,"restart_retains_original_scope":true,"outbox_uploads":0,"formal_gui_cli":false})
    );
    Ok(())
}
