use std::{error::Error as StdError, fmt, io, path::PathBuf, time::Duration};

use thiserror::Error;

use crate::ScriptReport;

const FALLBACK_TITLE: &str = "Request failed";

/// A user-facing explanation of a failed request.
///
/// Recognized transport failures lead with a short cause and recovery hint,
/// keeping the full diagnostic chain in `details` for inspection or copying.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorSummary {
    pub title: String,
    pub hint: String,
    pub details: Option<String>,
}

impl ErrorSummary {
    /// Summarize a saved failure message, such as one reopened from history.
    pub fn from_message(message: &str) -> Self {
        if let Some(duration) = message.strip_prefix("request timed out after ") {
            let duration = duration.trim();
            let hint = if duration.is_empty() {
                "The server did not respond in time. Check that it is reachable, or increase Request timeout in Settings.".to_owned()
            } else {
                format!(
                    "The server did not respond within {duration}. Check that it is reachable, or increase Request timeout in Settings."
                )
            };

            return Self {
                title: "Request timed out".into(),
                hint,
                details: Some(message.to_owned()),
            };
        }

        if message.starts_with("HTTP transport failed")
            || message.starts_with("could not read the HTTP response body")
        {
            let sanitized = strip_url_segment(message);

            if let Some((title, hint)) = classify_proxy_message(&sanitized)
                .or_else(|| classify_transport_message(&sanitized))
            {
                return Self {
                    title: title.into(),
                    hint: hint.into(),
                    details: Some(message.to_owned()),
                };
            }
        }

        Self::fallback(message.to_owned())
    }

    fn fallback(message: String) -> Self {
        Self {
            title: FALLBACK_TITLE.into(),
            hint: message,
            details: None,
        }
    }
}

/// A request that did not complete, with the reports of the scripts that ran
/// before it stopped. A pre-request script that failed or skipped the request
/// reports last.
#[derive(Debug)]
pub struct ExecutionFailure {
    pub error: ExecutionError,
    pub scripts: Vec<ScriptReport>,
}

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("Request skipped: {reason}")]
    Skipped { reason: String },

    #[error("pre-request script failed: {message}")]
    Script { message: String },

    #[error("{0}")]
    Variables(String),

    /// The request's authorization could not be added.
    #[error("{0}")]
    Auth(String),

    #[error("request timed out after {timeout:?}")]
    Timeout { timeout: Duration },

    #[error("response body exceeds the {limit_bytes}-byte limit")]
    ResponseTooLarge { limit_bytes: u64 },

    #[error("maximum response size is too large")]
    InvalidResponseLimit,

    #[error("invalid proxy settings: {0}")]
    InvalidProxy(&'static str),

    #[error("could not initialize the HTTP client: {0}")]
    Client(#[source] reqwest::Error),

    /// A certificate from Settings could not be used.
    #[error("{0}")]
    Certificate(String),

    #[error("invalid request URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("unsupported URL scheme: {0}; expected http or https")]
    UnsupportedScheme(String),

    #[error("unsupported URL scheme: {0}; expected ws or wss")]
    UnsupportedWebSocketScheme(String),

    #[error("the server did not accept the WebSocket connection: {status}")]
    WebSocketRejected {
        status: http_client::http::StatusCode,
    },

    #[error(
        "the server answered the WebSocket handshake with a Sec-WebSocket-Accept that does not match the request"
    )]
    WebSocketAccept,

    #[error("WebSocket connection failed: {0}")]
    WebSocket(#[source] tokio_tungstenite::tungstenite::Error),

    #[error("invalid HTTP request: {0}")]
    InvalidRequest(#[from] http_client::http::Error),

    #[error(
        "invalid Host header: expected a hostname and optional port (example.com:443). Use User-Agent for a client name/version"
    )]
    InvalidHost,

    #[error("multiple Host headers: keep only one, or remove them to use the URL's host")]
    MultipleHosts,

    #[error(
        "a custom Host that differs from the URL is not supported with forced HTTP/2. Choose Auto or HTTP/1.1 in request settings"
    )]
    Http2HostOverride,

    #[error("HTTP transport failed: {0:#}")]
    Transport(#[source] anyhow::Error),

    #[error("choose a file to send for {0}")]
    MissingFile(String),

    #[error("could not read {}: {source}", path.display())]
    BodyFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not read the HTTP response body: {0}")]
    ReadBody(#[source] io::Error),

    #[error("could not decode the gzip response body: {0}")]
    DecodeBody(#[source] io::Error),
}

impl ExecutionError {
    /// The message without the address the request was sent to. Once
    /// resolved, an address can hold secrets, such as a token in its query.
    pub fn message_without_url(&self) -> String {
        let message = self.to_string();
        let url = match self {
            Self::Transport(error) => error
                .chain()
                .find_map(|error| error.downcast_ref::<reqwest::Error>())
                .and_then(reqwest::Error::url),
            _ => None,
        };

        match url {
            Some(url) => message.replace(&format!(" for url ({url})"), ""),
            None => message,
        }
    }

    /// A readable summary of the failure, with the full diagnostic chain kept
    /// separately when a transport error is classified.
    pub fn summary(&self) -> ErrorSummary {
        match self {
            Self::Timeout { timeout } => ErrorSummary {
                title: "Request timed out".into(),
                hint: format!(
                    "The server did not respond within {timeout:?}. Check that it is reachable, or increase Request timeout in Settings."
                ),
                details: Some(self.to_string()),
            },
            Self::Transport(error) => {
                let raw = self.to_string();
                let sanitized = strip_url_segment(&self.message_without_url());

                match classify_error_chain(error.as_ref(), &sanitized) {
                    Some((title, hint)) => ErrorSummary {
                        title: title.into(),
                        hint: hint.into(),
                        details: Some(raw),
                    },
                    None => ErrorSummary::fallback(raw),
                }
            }
            Self::ReadBody(error) => {
                let raw = self.to_string();
                let sanitized = strip_url_segment(&raw);

                match classify_error_chain(error, &sanitized) {
                    Some((title, hint)) => ErrorSummary {
                        title: title.into(),
                        hint: hint.into(),
                        details: Some(raw),
                    },
                    None => ErrorSummary::fallback(raw),
                }
            }
            _ => ErrorSummary::fallback(self.to_string()),
        }
    }
}

/// Inspect typed causes in an error chain first so classification works across
/// operating systems and locales, then fall back to sanitized diagnostic text.
fn classify_error_chain(
    root: &(dyn StdError + 'static),
    sanitized: &str,
) -> Option<(&'static str, &'static str)> {
    if let Some(summary) = classify_proxy_message(sanitized) {
        return Some(summary);
    }

    if let Some(tls) = error_chain(root).find_map(find_rustls_error) {
        return Some(classify_rustls_error(tls));
    }

    if error_chain(root).any(|error| {
        error
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout)
            || error
                .downcast_ref::<io::Error>()
                .is_some_and(|io| io.kind() == io::ErrorKind::TimedOut)
    }) {
        return Some((
            "Request timed out",
            "The server did not respond in time. Check that it is reachable, or increase Request timeout in Settings.",
        ));
    }

    if let Some(summary) = error_chain(root)
        .filter_map(|error| error.downcast_ref::<io::Error>())
        .find_map(|io| classify_io_kind(io.kind()))
    {
        return Some(summary);
    }

    if let Some(summary) = classify_transport_message(sanitized) {
        return Some(summary);
    }

    if error_chain(root).any(|error| {
        error
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_connect)
    }) {
        return Some((
            "Connection failed",
            "Could not connect to the server. Check the URL, your network connection, and proxy settings.",
        ));
    }

    None
}

fn error_chain<'a>(
    root: &'a (dyn StdError + 'static),
) -> impl Iterator<Item = &'a (dyn StdError + 'static)> {
    std::iter::successors(Some(root), |error| (*error).source())
}

fn find_rustls_error<'a>(error: &'a (dyn StdError + 'static)) -> Option<&'a rustls::Error> {
    error.downcast_ref::<rustls::Error>().or_else(|| {
        error
            .downcast_ref::<io::Error>()
            .and_then(io::Error::get_ref)
            .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    })
}

fn classify_rustls_error(error: &rustls::Error) -> (&'static str, &'static str) {
    match error {
        rustls::Error::InvalidCertificate(
            rustls::CertificateError::Expired | rustls::CertificateError::ExpiredContext { .. },
        ) => (
            "Certificate expired",
            "The server's TLS certificate has expired. Renew the certificate on the server or check the system clock.",
        ),
        rustls::Error::InvalidCertificate(
            rustls::CertificateError::NotValidYet
            | rustls::CertificateError::NotValidYetContext { .. },
        ) => (
            "Certificate not valid yet",
            "The server's TLS certificate is not valid yet. Check the system clock or the certificate's validity dates.",
        ),
        rustls::Error::InvalidCertificate(
            rustls::CertificateError::NotValidForName
            | rustls::CertificateError::NotValidForNameContext { .. },
        ) => (
            "Certificate name mismatch",
            "The server's TLS certificate does not match this hostname. Check the URL hostname or the server's certificate.",
        ),
        rustls::Error::InvalidCertificate(rustls::CertificateError::Revoked) => (
            "Certificate revoked",
            "The server's TLS certificate has been revoked. Replace the certificate on the server before connecting.",
        ),
        rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer) => (
            "Certificate not trusted",
            "The server's TLS certificate is issued by an untrusted authority. Add its CA certificate in Settings, or turn off verification in this request's Settings tab.",
        ),
        rustls::Error::NoCertificatesPresented => (
            "No server certificate",
            "The server did not send a TLS certificate. Check the server's TLS configuration and URL.",
        ),
        rustls::Error::InvalidCertificate(_) | rustls::Error::InvalidCertRevocationList(_) => (
            "Invalid server certificate",
            "The server's TLS certificate could not be verified. Check the server's certificate chain and TLS configuration.",
        ),
        rustls::Error::InvalidMessage(rustls::InvalidMessage::InvalidContentType) => (
            "TLS handshake failed",
            "The server did not return a valid TLS response. Check the URL scheme, port, and server TLS configuration.",
        ),
        _ => (
            "TLS handshake failed",
            "Could not complete a secure TLS handshake. Check the URL scheme, port, and TLS or client certificate settings.",
        ),
    }
}

fn classify_io_kind(kind: io::ErrorKind) -> Option<(&'static str, &'static str)> {
    match kind {
        io::ErrorKind::ConnectionRefused => Some((
            "Connection refused",
            "No service accepted the connection. Check that the server is running and that the host and port are right.",
        )),
        io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::BrokenPipe => Some((
            "Connection reset",
            "The server closed the connection unexpectedly. Check the server logs or try sending the request again.",
        )),
        io::ErrorKind::UnexpectedEof => Some((
            "Connection closed",
            "The server closed the connection before finishing the response. Check the server logs or whether it expects HTTPS.",
        )),
        io::ErrorKind::NetworkUnreachable => Some((
            "Network unreachable",
            "No network route to the server is available. Check your internet connection, VPN, or proxy settings.",
        )),
        io::ErrorKind::HostUnreachable | io::ErrorKind::AddrNotAvailable => Some((
            "Host unreachable",
            "Could not reach the server. Check the host address, your network connection, or VPN settings.",
        )),
        _ => None,
    }
}

fn classify_proxy_message(sanitized: &str) -> Option<(&'static str, &'static str)> {
    let lower = sanitized.to_ascii_lowercase();

    if lower.contains("proxy authentication required") {
        return Some((
            "Proxy authentication required",
            "The proxy rejected the connection because credentials are missing or invalid. Check Proxy in Settings.",
        ));
    }

    if lower.contains("unsuccessful tunnel")
        || lower.contains("unexpected eof while tunneling")
        || lower.contains("proxy headers too long for tunnel")
    {
        return Some((
            "Proxy connection failed",
            "Could not tunnel through the proxy. Check your proxy address, port, and credentials in Settings.",
        ));
    }

    None
}

fn classify_transport_message(sanitized: &str) -> Option<(&'static str, &'static str)> {
    let lower = sanitized.to_ascii_lowercase();

    if lower.contains("connection refused") || lower.contains("actively refused") {
        return Some((
            "Connection refused",
            "No service accepted the connection. Check that the server is running and that the host and port are right.",
        ));
    }

    if lower.contains("dns error")
        || lower.contains("failed to lookup address")
        || lower.contains("name or service not known")
        || lower.contains("nodename nor servname")
        || lower.contains("no such host")
        || lower.contains("temporary failure in name resolution")
    {
        return Some((
            "Host not found",
            "Could not resolve the hostname. Check the spelling in the URL and your network or DNS settings.",
        ));
    }

    if lower.contains("timed out") || lower.contains("timeout elapsed") {
        return Some((
            "Request timed out",
            "The server did not respond in time. Check that it is reachable, or increase Request timeout in Settings.",
        ));
    }

    if let Some((_, peer_cert)) = lower.split_once("invalid peer certificate:") {
        return Some(classify_peer_certificate_detail(peer_cert.trim_start()));
    }

    if lower.contains("not valid for name") || lower.contains("notvalidforname") {
        return Some((
            "Certificate name mismatch",
            "The server's TLS certificate does not match this hostname. Check the URL hostname or the server's certificate.",
        ));
    }

    if lower.contains("certificate expired") {
        return Some((
            "Certificate expired",
            "The server's TLS certificate has expired. Renew the certificate on the server or check the system clock.",
        ));
    }

    if lower.contains("certificate not valid yet") {
        return Some((
            "Certificate not valid yet",
            "The server's TLS certificate is not valid yet. Check the system clock or the certificate's validity dates.",
        ));
    }

    if lower.contains("unknownissuer")
        || lower.contains("unknown issuer")
        || lower.contains("self-signed certificate")
        || lower.contains("self signed certificate")
        || lower.contains("unable to verify the first certificate")
    {
        return Some((
            "Certificate not trusted",
            "The server's TLS certificate is issued by an untrusted authority. Add its CA certificate in Settings, or turn off verification in this request's Settings tab.",
        ));
    }

    if lower.contains("peer sent no certificates") {
        return Some((
            "No server certificate",
            "The server did not send a TLS certificate. Check the server's TLS configuration and URL.",
        ));
    }

    if lower.contains("invalid certificate revocation list")
        || lower.contains("certificate verify failed")
    {
        return Some((
            "Invalid server certificate",
            "The server's TLS certificate could not be verified. Check the server's certificate chain and TLS configuration.",
        ));
    }

    if lower.contains("invalidcontenttype") || lower.contains("invalid content type") {
        return Some((
            "TLS handshake failed",
            "The server did not return a valid TLS response. Check the URL scheme, port, and server TLS configuration.",
        ));
    }

    if lower.contains("corrupt message")
        || lower.contains("fatal alert")
        || lower.contains("unexpected handshake")
        || lower.contains("handshake not complete")
        || lower.contains("peer is incompatible")
        || lower.contains("tls handshake")
        || lower.contains("ssl handshake")
    {
        return Some((
            "TLS handshake failed",
            "Could not complete a secure TLS handshake. Check the URL scheme, port, and TLS or client certificate settings.",
        ));
    }

    if lower.contains("network is unreachable") || lower.contains("network unreachable") {
        return Some((
            "Network unreachable",
            "No network route to the server is available. Check your internet connection, VPN, or proxy settings.",
        ));
    }

    if lower.contains("no route to host")
        || lower.contains("host is unreachable")
        || lower.contains("host unreachable")
    {
        return Some((
            "Host unreachable",
            "Could not reach the server. Check the host address, your network connection, or VPN settings.",
        ));
    }

    if lower.contains("connection reset")
        || lower.contains("forcibly closed")
        || lower.contains("connection aborted")
        || lower.contains("broken pipe")
    {
        return Some((
            "Connection reset",
            "The server closed the connection unexpectedly. Check the server logs or try sending the request again.",
        ));
    }

    if lower.contains("connection closed before message completed")
        || lower.contains("incomplete message")
        || lower.contains("unexpected eof")
        || lower.contains("unexpected end of file")
    {
        return Some((
            "Connection closed",
            "The server closed the connection before finishing the response. Check the server logs or whether it expects HTTPS.",
        ));
    }

    if lower.contains("too many redirects") {
        return Some((
            "Too many redirects",
            "The server redirected too many times. Check the URL, or turn off Follow redirects in request settings.",
        ));
    }

    if lower.contains("client error (connect)") || lower.contains("tcp connect error") {
        return Some((
            "Connection failed",
            "Could not connect to the server. Check the URL, your network connection, and proxy settings.",
        ));
    }

    None
}

/// Match the leading rustls `CertificateError` cause right after
/// `invalid peer certificate:` so hostnames or context details cannot
/// override the actual verification error.
fn classify_peer_certificate_detail(detail: &str) -> (&'static str, &'static str) {
    if detail.starts_with("certificate not valid for name")
        || starts_with_token(detail, "notvalidforname")
    {
        return (
            "Certificate name mismatch",
            "The server's TLS certificate does not match this hostname. Check the URL hostname or the server's certificate.",
        );
    }

    if detail.starts_with("certificate expired") || starts_with_token(detail, "expired") {
        return (
            "Certificate expired",
            "The server's TLS certificate has expired. Renew the certificate on the server or check the system clock.",
        );
    }

    if detail.starts_with("certificate not valid yet") || starts_with_token(detail, "notvalidyet") {
        return (
            "Certificate not valid yet",
            "The server's TLS certificate is not valid yet. Check the system clock or the certificate's validity dates.",
        );
    }

    if starts_with_token(detail, "revoked") {
        return (
            "Certificate revoked",
            "The server's TLS certificate has been revoked. Replace the certificate on the server before connecting.",
        );
    }

    if starts_with_token(detail, "unknownissuer") {
        return (
            "Certificate not trusted",
            "The server's TLS certificate is issued by an untrusted authority. Add its CA certificate in Settings, or turn off verification in this request's Settings tab.",
        );
    }

    (
        "Invalid server certificate",
        "The server's TLS certificate could not be verified. Check the server's certificate chain and TLS configuration.",
    )
}

fn starts_with_token(text: &str, token: &str) -> bool {
    text.strip_prefix(token)
        .is_some_and(|rest| rest.chars().next().is_none_or(|ch| !ch.is_alphanumeric()))
}

/// Remove ` for url (...)` clauses before text matching so path or query words
/// in the request URL cannot affect error classification.
fn strip_url_segment(message: &str) -> String {
    const MARKER: &str = " for url (";
    let mut text = message.to_owned();

    while let Some(start) = text.find(MARKER) {
        let content_start = start + MARKER.len();
        let rest = &text[content_start..];

        // Encoded URLs never contain spaces, so `): ` marks the exact end of
        // reqwest's URL clause even when the URL itself contains parentheses.
        if let Some(end) = rest.find("): ") {
            text.replace_range(start..content_start + end + 1, "");
        } else if rest.ends_with(')') {
            text.truncate(start);
        } else {
            break;
        }
    }

    text
}

/// A failure without script reports.
impl From<ExecutionError> for ExecutionFailure {
    fn from(error: ExecutionError) -> Self {
        Self {
            error,
            scripts: Vec::new(),
        }
    }
}

/// The error's message, without the scripts' reports.
impl fmt::Display for ExecutionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl StdError for ExecutionFailure {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.error.source()
    }
}
