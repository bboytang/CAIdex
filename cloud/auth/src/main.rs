//! Explicit local protocol service, never a production deployment command.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use caidex_auth::{
    Trust,
    server::{EmailSender, Error, Mail, Service, router},
};
use caidex_credentials::{Broker, CredentialRef, Id, ProtectedFileStore, Secret, SecretKind};
use std::{path::Path, sync::Arc};

struct FileMail(Arc<Broker<ProtectedFileStore>>);
impl EmailSender for FileMail {
    async fn send(&self, mail: Mail) -> Result<(), Error> {
        let reference = reference(&mail.id.to_string())?;
        let body = serde_json::json!({"id":mail.id,"recipient":mail.recipient,"purpose":mail.purpose,"language":mail.language,"expires_in":mail.expires_in,"verification_token":mail.verification_token});
        let secret = Secret::new(body.to_string()).map_err(|_| Error::Unavailable)?;
        self.0
            .set(&reference, secret)
            .map_err(|_| Error::Unavailable)
    }
}
fn reference(profile: &str) -> Result<CredentialRef, Error> {
    Ok(CredentialRef {
        owner: Id::new("caidex-test-service").map_err(|_| Error::Invalid)?,
        provider: Id::new("account-test-auth").map_err(|_| Error::Invalid)?,
        profile: Id::new(profile).map_err(|_| Error::Invalid)?,
        kind: SecretKind::ClientSecret,
    })
}
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: caidex-auth-local <port> <new Git-external private sandbox directory>");
        return Err(Error::Invalid);
    }
    let port: u16 = args[1].parse().map_err(|_| Error::Invalid)?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|_| Error::Unavailable)?;
    let port = listener
        .local_addr()
        .map_err(|_| Error::Unavailable)?
        .port();
    let trust =
        Trust::local_test(&format!("http://localhost:{port}/")).map_err(|_| Error::Invalid)?;
    let store = ProtectedFileStore::open(Path::new(&args[2])).map_err(|_| Error::Denied)?;
    let broker = Arc::new(Broker::new(
        Id::new("caidex-test-service").map_err(|_| Error::Invalid)?,
        store,
    ));
    let key_ref = reference("device-code-pepper")?;
    let key = if let Some(key) = broker.resolve(&key_ref).map_err(|_| Error::Denied)? {
        key
    } else {
        let mut bytes = [0; 32];
        getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
        let key = Secret::new(URL_SAFE_NO_PAD.encode(bytes)).map_err(|_| Error::Unavailable)?;
        broker
            .set(
                &key_ref,
                Secret::new(key.expose().into()).map_err(|_| Error::Unavailable)?,
            )
            .map_err(|_| Error::Denied)?;
        key
    };
    let key: [u8; 32] = URL_SAFE_NO_PAD
        .decode(key.expose())
        .map_err(|_| Error::Denied)?
        .try_into()
        .map_err(|_| Error::Denied)?;
    let auth = std::env::var("CAIDEX_AUTH_DB").map_err(|_| Error::Invalid)?;
    let data = std::env::var("CAIDEX_DATA_DB").map_err(|_| Error::Invalid)?;
    let service = Service::connect(&auth, &data, trust, "localhost", key, FileMail(broker)).await?;
    println!(
        "CAIdex local account protocol service: http://localhost:{port}/ (test only; mail in protected sandbox)"
    );
    axum::serve(
        listener,
        router(service).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .map_err(|_| Error::Unavailable)
}
