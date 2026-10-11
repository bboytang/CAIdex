//! Explicit Linux service entry. Binding is loopback; HTTPS and publication are external approvals.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use caidex_auth::{
    Trust,
    server::{
        Error, Service,
        email::{Provider, ProviderEmail, Sender, smtp::AliyunSmtpEmail},
        router,
    },
};
use caidex_credentials::{Broker, CredentialRef, Id, ProtectedFileStore, SecretKind};
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    port: u16,
    issuer: String,
    rp_id: String,
    auth_db: String,
    data_db: String,
    secrets: PathBuf,
    provider: Provider,
    #[serde(default)]
    mail_disabled: bool,
    from: String,
    daily_budget: i64,
    monthly_budget: i64,
    database_volume: PathBuf,
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
    if args.len() != 2 {
        eprintln!("Usage: caidex-auth-service <Git-external private configuration directory>");
        return Err(Error::Invalid);
    }
    let directory = PathBuf::from(&args[1]);
    caidex_credentials::private_directory(&directory).map_err(|_| Error::Denied)?;
    let directory = directory.canonicalize().map_err(|_| Error::Denied)?;
    if directory.ancestors().any(|p| p.join(".git").exists()) {
        return Err(Error::Denied);
    }
    let file = directory.join("config.json");
    let metadata = std::fs::symlink_metadata(&file).map_err(|_| Error::Denied)?;
    if !metadata.is_file() || metadata.is_symlink() || metadata.len() > 16384 {
        return Err(Error::Denied);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid takes no pointer arguments.
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(Error::Denied);
        }
    }
    let bytes = std::fs::read(&file).map_err(|_| Error::Denied)?;
    let config: Config = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
    // DB secrets are never in JSON or arguments; only local socket role/peer configuration is accepted.
    for dsn in [&config.auth_db, &config.data_db] {
        let parsed: tokio_postgres::Config = dsn.parse().map_err(|_| Error::Invalid)?;
        if parsed.get_password().is_some() {
            return Err(Error::Denied);
        }
    }
    let trust = Trust::production(&config.issuer).map_err(|_| Error::Invalid)?;
    if url::Url::parse(trust.issuer())
        .map_err(|_| Error::Invalid)?
        .host_str()
        != Some(&config.rp_id)
    {
        return Err(Error::Invalid);
    }
    let owner = Id::new("caidex-service").map_err(|_| Error::Invalid)?;
    let broker = Arc::new(Broker::new(
        owner.clone(),
        ProtectedFileStore::open(&config.secrets).map_err(|_| Error::Denied)?,
    ));
    let get = |profile: &str, kind: SecretKind| {
        broker
            .resolve(&CredentialRef {
                owner: owner.clone(),
                provider: Id::new("account-service").map_err(|_| Error::Invalid)?,
                profile: Id::new(profile).map_err(|_| Error::Invalid)?,
                kind,
            })
            .map_err(|_| Error::Denied)?
            .ok_or(Error::Unavailable)
    };
    let pepper = get("device-code-pepper", SecretKind::ClientSecret)?;
    let pepper: [u8; 32] = URL_SAFE_NO_PAD
        .decode(pepper.expose())
        .map_err(|_| Error::Invalid)?
        .try_into()
        .map_err(|_| Error::Invalid)?;
    let mail = if config.mail_disabled {
        eprintln!("Account mail is disabled; email verification and callbacks are unavailable");
        None
    } else {
        Some(match config.provider {
            Provider::AliyunDirectMail => Sender::Aliyun(
                AliyunSmtpEmail::connect(
                    &config.auth_db,
                    &config.from,
                    get("aliyun-smtp", SecretKind::ClientSecret)?,
                    config.daily_budget,
                    config.monthly_budget,
                )
                .await?,
            ),
            provider => Sender::Http(
                ProviderEmail::connect(
                    &config.auth_db,
                    provider,
                    config.from,
                    get("mail-api", SecretKind::ApiKey)?,
                    get("mail-webhook", SecretKind::ClientSecret)?,
                    config.daily_budget,
                    config.monthly_budget,
                )
                .await?,
            ),
        })
    };
    let service = Service::connect(
        &config.auth_db,
        &config.data_db,
        trust,
        &config.rp_id,
        pepper,
        mail,
    )
    .await?;
    service
        .guard_disk(config.database_volume, 128 * 1024 * 1024)
        .await?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, config.port))
        .await
        .map_err(|_| Error::Unavailable)?;
    println!(
        "CAIdex account service ready on loopback; HTTPS, domain, mail delivery and public deployment require separate verification"
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
