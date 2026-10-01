//! TLS for HTTP requests, WebSocket handshakes and gRPC calls: which server
//! certificates to trust, and the client certificate to present.

use std::{
    path::Path,
    sync::{Arc, OnceLock},
};

use rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::CryptoProvider,
    pki_types::{CertificateDer, ServerName, UnixTime},
};

use crate::ClientCertificate;

/// How a connection checks the server's certificate and identifies itself.
pub(crate) struct Tls<'a> {
    pub(crate) verify: bool,
    /// The name to check the certificate against instead of the host. The
    /// handshake still sends the host, so servers choose their usual certificate.
    pub(crate) server_name: Option<&'a str>,
    /// Certificate authorities trusted in addition to the system's.
    pub(crate) ca_certificates: Option<&'a Path>,
    pub(crate) client_certificate: Option<&'a ClientCertificate>,
}

impl Tls<'_> {
    /// Read the certificate files into a configuration that offers the `alpn`
    /// protocols.
    pub(crate) fn config(&self, alpn: &[&[u8]]) -> Result<ClientConfig, String> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|error| error.to_string())?;

        let builder = if self.verify {
            let roots = match self.ca_certificates {
                Some(path) => {
                    let mut roots = native_roots().clone();
                    let certificates = crate::certificates::ca_certificates(path)
                        .map_err(|error| format!("CA certificates: {error}"))?;

                    for certificate in certificates {
                        roots.add(certificate).map_err(|error| {
                            format!(
                                "could not trust a certificate in {}: {error}",
                                path.display()
                            )
                        })?;
                    }

                    roots
                }
                None => native_roots().clone(),
            };

            match self.server_name {
                None => builder.with_root_certificates(roots),
                Some(name) => {
                    let name = ServerName::try_from(name.to_owned())
                        .map_err(|error| format!("invalid server name: {error}"))?;
                    let verifier =
                        WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
                            .build()
                            .map_err(|error| error.to_string())?;

                    builder
                        .dangerous()
                        .with_custom_certificate_verifier(Arc::new(VerifyAs { name, verifier }))
                }
            }
        } else {
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate(provider)))
        };

        let mut config = match self.client_certificate {
            Some(certificate) => {
                let identity = certificate.load().map_err(|error| {
                    format!("client certificate for {}: {error}", certificate.host)
                })?;

                builder
                    .with_client_auth_cert(identity.chain, identity.key)
                    .map_err(|error| {
                        format!(
                            "client certificate for {}: {}",
                            certificate.host,
                            crate::certificates::key_error(error)
                        )
                    })?
            }
            None => builder.with_no_client_auth(),
        };
        config.alpn_protocols = alpn.iter().map(|protocol| protocol.to_vec()).collect();

        Ok(config)
    }
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
