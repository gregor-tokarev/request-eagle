use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use http_client::http::Uri;
use hyper_util::rt::TokioIo;
use rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::CryptoProvider,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, client::TlsStream};
use tonic::{
    Status,
    transport::{Channel, Endpoint},
};

use super::GrpcError;

/// Connection setup gives up here unless the request timeout is shorter.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long to wait for the handshake that checks whether a server uses TLS.
const TLS_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Where to connect: the host and port from a request URL, and how to
/// check the server's certificate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) tls: bool,
    pub(crate) verify_certificates: bool,
    /// The certificate name to expect instead of the host. The handshake
    /// still sends the host, so servers choose their usual certificate.
    pub(crate) server_name: Option<String>,
}

impl Target {
    /// Accept `host`, `host:port` and URLs with a gRPC or HTTP scheme. Without
    /// a port, both TLS and plaintext connect to 443, as in Postman.
    pub(crate) fn parse(url: &str, tls: bool) -> Result<Self, GrpcError> {
        let url = url.trim();

        if url.is_empty() {
            return Err(GrpcError::MissingUrl);
        }

        let (tls, rest) = match url.split_once("://") {
            Some(("grpcs" | "https", rest)) => (true, rest),
            Some(("grpc" | "http", rest)) => (false, rest),
            Some((scheme, _)) => {
                return Err(GrpcError::InvalidUrl(format!(
                    "unsupported scheme {scheme}://; use grpc:// or grpcs://"
                )));
            }
            None => (tls, url),
        };
        let authority = rest.strip_suffix('/').unwrap_or(rest);

        if authority.contains(['/', '?', '#']) {
            return Err(GrpcError::InvalidUrl(
                "enter only a host and port; the method selects the path".into(),
            ));
        }

        let parsed = url::Url::parse(&format!("http://{authority}"))
            .ok()
            .filter(|parsed| parsed.username().is_empty() && parsed.password().is_none())
            .ok_or_else(|| GrpcError::InvalidUrl(format!("{authority} is not a host and port")))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| GrpcError::InvalidUrl(format!("{authority} has no host")))?
            .to_owned();
        // `Url` drops the default port of its http scheme, so read it back.
        let port = match parsed.port() {
            Some(port) => port,
            None if authority.ends_with(":80") => 80,
            None => 443,
        };

        Ok(Self {
            host,
            port,
            tls,
            verify_certificates: true,
            server_name: None,
        })
    }

    fn uri(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };

        format!("{scheme}://{}:{}", self.host, self.port)
    }
}

/// Open an HTTP/2 connection. TLS negotiates `h2` with ALPN.
pub(crate) async fn connect(
    target: &Target,
    timeout: Option<Duration>,
) -> Result<Channel, GrpcError> {
    let endpoint = Endpoint::from_shared(target.uri())
        .map_err(|error| GrpcError::InvalidUrl(error.to_string()))?
        .connect_timeout(timeout.map_or(CONNECT_TIMEOUT, |timeout| timeout.min(CONNECT_TIMEOUT)))
        .tcp_nodelay(true);

    let channel = if target.tls {
        let connector = TlsConnector::from(tls_config(target)?);
        let server_name = server_name(target)?;
        let target = target.clone();

        endpoint
            .connect_with_connector(tower::service_fn(move |_: Uri| {
                let handshake = handshake(connector.clone(), target.clone(), server_name.clone());

                async move { Ok::<_, std::io::Error>(TokioIo::new(handshake.await?)) }
            }))
            .await
    } else {
        endpoint.connect().await
    };

    channel.map_err(|error| {
        if answered_without_tls(&error) {
            GrpcError::TlsUnsupported
        } else {
            GrpcError::Connect(error_chain(&error))
        }
    })
}

/// Whether a server reached without TLS completes a TLS handshake. Such a
/// server resets the connection without saying why, so this explains it.
pub(crate) async fn expects_tls(target: &Target) -> bool {
    if target.tls {
        return false;
    }

    // Any certificate will do: the handshake only shows the server uses TLS.
    let target = Target {
        tls: true,
        verify_certificates: false,
        ..target.clone()
    };
    let (Ok(config), Ok(server_name)) = (tls_config(&target), server_name(&target)) else {
        return false;
    };
    let handshake = handshake(TlsConnector::from(config), target, server_name);

    matches!(
        tokio::time::timeout(TLS_CHECK_TIMEOUT, handshake).await,
        Ok(Ok(_))
    )
}

/// The connection error behind a status, when a call failed in the
/// connection rather than with a status from the server.
pub(crate) fn connection_error(status: &Status) -> Option<String> {
    std::error::Error::source(status)?
        .downcast_ref::<tonic::transport::Error>()
        .map(|error| error_chain(error))
}

fn server_name(target: &Target) -> Result<ServerName<'static>, GrpcError> {
    ServerName::try_from(target.host.trim_matches(['[', ']']).to_owned())
        .map_err(|error| GrpcError::InvalidUrl(error.to_string()))
}

/// Connect over TCP and complete the TLS handshake.
async fn handshake(
    connector: TlsConnector,
    target: Target,
    server_name: ServerName<'static>,
) -> std::io::Result<TlsStream<TcpStream>> {
    let tcp = TcpStream::connect((target.host.trim_matches(['[', ']']), target.port)).await?;
    tcp.set_nodelay(true)?;

    connector.connect(server_name, tcp).await
}

/// Whether a TLS handshake failed because the server answered without TLS.
fn answered_without_tls(error: &(dyn std::error::Error + 'static)) -> bool {
    std::iter::successors(Some(error), |error| error.source()).any(|error| {
        let tls_error = error
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
            .and_then(|error| error.downcast_ref::<rustls::Error>());

        matches!(
            tls_error,
            Some(rustls::Error::InvalidMessage(
                rustls::InvalidMessage::InvalidContentType
            ))
        )
    })
}

/// Include each source, since transport errors wrap the useful cause.
pub(crate) fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();

    while let Some(cause) = source {
        let cause = cause.to_string();

        if !message.contains(&cause) {
            message.push_str(": ");
            message.push_str(&cause);
        }

        source = source.and_then(std::error::Error::source);
    }

    message
}

fn tls_config(target: &Target) -> Result<Arc<ClientConfig>, GrpcError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|error| GrpcError::Connect(error.to_string()))?;
    let mut config = match (&target.server_name, target.verify_certificates) {
        (_, false) => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate(provider)))
            .with_no_client_auth(),
        (None, true) => builder
            .with_root_certificates(native_roots().clone())
            .with_no_client_auth(),
        // The handshake still names the host; only the check uses the override.
        (Some(name), true) => {
            let name = ServerName::try_from(name.clone())
                .map_err(|error| GrpcError::InvalidUrl(format!("invalid server name: {error}")))?;
            let verifier = WebPkiServerVerifier::builder_with_provider(
                Arc::new(native_roots().clone()),
                provider,
            )
            .build()
            .map_err(|error| GrpcError::Connect(error.to_string()))?;

            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(VerifyAs { name, verifier }))
                .with_no_client_auth()
        }
    };
    config.alpn_protocols = vec![b"h2".to_vec()];

    Ok(Arc::new(config))
}

/// The platform's trusted roots, read once per process.
fn native_roots() -> &'static RootCertStore {
    static ROOTS: OnceLock<RootCertStore> = OnceLock::new();

    ROOTS.get_or_init(|| {
        let mut roots = RootCertStore::empty();
        roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);

        roots
    })
}

/// Checks the certificate against another name than the one connected to.
#[derive(Debug)]
struct VerifyAs {
    name: ServerName<'static>,
    verifier: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for VerifyAs {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.verifier
            .verify_server_cert(end_entity, intermediates, &self.name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.verifier
            .verify_tls12_signature(message, certificate, signature)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.verifier
            .verify_tls13_signature(message, certificate, signature)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.verifier.supported_verify_schemes()
    }
}

/// Skips certificate checks when verification is turned off in settings.
/// Handshake signatures are still checked, so the connection is encrypted.
#[derive(Debug)]
struct AcceptAnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
