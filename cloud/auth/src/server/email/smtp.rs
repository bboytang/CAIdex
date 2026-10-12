//! DirectMail submission only; no unverified delivery events or automatic retries.
use super::{EmailSender, Error, Mail, MailLedger, Provider, Result, template};
use caidex_credentials::Secret;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{MultiPart, SinglePart, header::ContentType},
    transport::smtp::{
        authentication::{Credentials, Mechanism},
        client::{Tls, TlsParameters, TlsVersion},
        extension::ClientId,
    },
};
use std::time::Duration;

const HOST: &str = "smtpdm.aliyun.com";
const FROM: &str = "no-reply@caidex.cn";

pub struct AliyunSmtpEmail {
    ledger: MailLedger,
    transport: AsyncSmtpTransport<Tokio1Executor>,
}
impl AliyunSmtpEmail {
    pub async fn connect(
        dsn: &str,
        from: &str,
        password: Secret,
        daily: i64,
        monthly: i64,
    ) -> Result<Self> {
        if from != FROM {
            return Err(Error::Invalid);
        }
        let tls = TlsParameters::builder(HOST.into())
            .set_min_tls_version(TlsVersion::Tlsv12)
            .build()
            .map_err(|_| Error::Unavailable)?;
        let ledger = MailLedger::connect(
            dsn,
            Provider::AliyunDirectMail,
            daily,
            monthly,
            password.expose(),
        )
        .await?;
        let transport = Self::transport(HOST, 465, tls, password)?;
        Ok(Self { ledger, transport })
    }
    fn transport(
        host: &str,
        port: u16,
        tls: TlsParameters,
        password: Secret,
    ) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
        // A password is never embedded in an SMTP URL or error/debug output.
        Ok(
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
                .port(port)
                .tls(Tls::Wrapper(tls))
                .hello_name(ClientId::Domain("caidex.cn".into()))
                .authentication(vec![Mechanism::Login])
                .credentials(Credentials::new(FROM.into(), password.expose().to_owned()))
                .timeout(Some(Duration::from_secs(2)))
                .build(),
        )
    }
    /// Test builds alone can use a loopback TLS fixture and its synthetic CA; verification stays on.
    #[cfg(feature = "test-auth")]
    pub fn use_test_endpoint(
        &mut self,
        address: std::net::SocketAddr,
        ca: &[u8],
        password: Secret,
    ) -> Result<()> {
        if !address.ip().is_loopback() {
            return Err(Error::Denied);
        }
        let certificate = lettre::transport::smtp::client::Certificate::from_der(ca.to_vec())
            .map_err(|_| Error::Invalid)?;
        let tls = TlsParameters::builder(HOST.into())
            .set_min_tls_version(TlsVersion::Tlsv12)
            .add_root_certificate(certificate)
            .build()
            .map_err(|_| Error::Invalid)?;
        self.transport = Self::transport(&address.ip().to_string(), address.port(), tls, password)?;
        Ok(())
    }
}
impl EmailSender for AliyunSmtpEmail {
    async fn send(&self, mail: Mail) -> Result<()> {
        let (subject, text, html) = template(&mail)?;
        let message = Message::builder()
            .from(FROM.parse().map_err(|_| Error::Invalid)?)
            .to(mail.recipient.parse().map_err(|_| Error::Invalid)?)
            .message_id(Some(format!("<{}@caidex.cn>", mail.id)))
            .subject(subject)
            .multipart(
                MultiPart::alternative()
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_PLAIN)
                            .body(text),
                    )
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_HTML)
                            .body(html),
                    ),
            )
            .map_err(|_| Error::Invalid)?;
        if !self.ledger.reserve(&mail).await? {
            return Ok(());
        }
        // Unknown is committed before opening SMTP. Dropping the unpooled send cannot schedule a retry.
        let result = match tokio::time::timeout(
            Duration::from_secs(3),
            self.transport.send(message),
        )
        .await
        {
            Ok(Ok(reply)) if reply.code().to_string() == "250" => Ok(None),
            Ok(Err(error)) if error.is_permanent() || error.is_transient() => Err(Error::Invalid),
            _ => Err(Error::Unavailable),
        };
        self.ledger.complete(&mail.id, &result).await?;
        result.map(|_| ())
    }
}
