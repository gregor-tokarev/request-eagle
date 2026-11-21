use std::{collections::HashMap, io, net::TcpListener, time::Duration};

use crate::{
    ErrorSummary, ExecutionError, HttpRequest, ProxyMode, RequestExecutor, RequestPreferences,
    RequestVariables,
};

#[test]
fn typed_io_transport_errors_lead_with_cause_and_preserve_raw_chain() {
    let cases = [
        (io::ErrorKind::ConnectionRefused, "Connection refused"),
        (io::ErrorKind::ConnectionReset, "Connection reset"),
        (io::ErrorKind::ConnectionAborted, "Connection reset"),
        (io::ErrorKind::BrokenPipe, "Connection reset"),
        (io::ErrorKind::UnexpectedEof, "Connection closed"),
        (io::ErrorKind::TimedOut, "Request timed out"),
        (io::ErrorKind::NetworkUnreachable, "Network unreachable"),
        (io::ErrorKind::HostUnreachable, "Host unreachable"),
        (io::ErrorKind::AddrNotAvailable, "Host unreachable"),
    ];

    for (kind, expected_title) in cases {
        let error = ExecutionError::Transport(io::Error::new(kind, "raw transport cause").into());
        let summary = error.summary();

        assert_eq!(summary.title, expected_title, "{kind:?}");
        assert!(!summary.hint.is_empty(), "{kind:?}");
        assert_eq!(
            summary.details.as_deref(),
            Some("HTTP transport failed: raw transport cause"),
            "{kind:?}"
        );
    }
}

#[test]
fn typed_rustls_errors_distinguish_certificates_from_handshake_failures() {
    let cases = [
        (
            rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer),
            "Certificate not trusted",
            "CA certificate",
        ),
        (
            rustls::Error::InvalidCertificate(rustls::CertificateError::Expired),
            "Certificate expired",
            "expired",
        ),
        (
            rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidYet),
            "Certificate not valid yet",
            "not valid yet",
        ),
        (
            rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidForName),
            "Certificate name mismatch",
            "hostname",
        ),
        (
            rustls::Error::InvalidCertificate(rustls::CertificateError::Revoked),
            "Certificate revoked",
            "Replace the certificate",
        ),
        (
            rustls::Error::NoCertificatesPresented,
            "No server certificate",
            "did not send a TLS certificate",
        ),
        (
            rustls::Error::InvalidCertificate(rustls::CertificateError::BadSignature),
            "Invalid server certificate",
            "certificate chain",
        ),
        (
            rustls::Error::InvalidMessage(rustls::InvalidMessage::InvalidContentType),
            "TLS handshake failed",
            "URL scheme, port",
        ),
        (
            rustls::Error::AlertReceived(rustls::AlertDescription::CertificateRequired),
            "TLS handshake failed",
            "client certificate",
        ),
        (
            rustls::Error::HandshakeNotComplete,
            "TLS handshake failed",
            "TLS handshake",
        ),
    ];

    for (tls_error, expected_title, expected_hint_fragment) in cases {
        let io_error = io::Error::new(io::ErrorKind::InvalidData, tls_error);
        let error = ExecutionError::Transport(io_error.into());
        let summary = error.summary();

        assert_eq!(summary.title, expected_title);
        assert!(
            summary.hint.contains(expected_hint_fragment),
            "{:?} did not contain {:?}",
            summary.hint,
            expected_hint_fragment
        );
        assert!(summary.details.is_some());
    }
}

#[test]
fn read_body_and_timeout_errors_are_summarized() {
    let timeout = ExecutionError::Timeout {
        timeout: Duration::from_secs(30),
    }
    .summary();
    assert_eq!(timeout.title, "Request timed out");
    assert!(timeout.hint.contains("30s"));
    assert_eq!(
        timeout.details.as_deref(),
        Some("request timed out after 30s")
    );

    let reset =
        ExecutionError::ReadBody(io::Error::new(io::ErrorKind::ConnectionReset, "peer reset"))
            .summary();
    assert_eq!(reset.title, "Connection reset");
    assert_eq!(
        reset.details.as_deref(),
        Some("could not read the HTTP response body: peer reset")
    );

    let other_read =
        ExecutionError::ReadBody(io::Error::other("unexpected disk failure")).summary();
    assert_eq!(other_read.title, "Request failed");
    assert_eq!(
        other_read.hint,
        "could not read the HTTP response body: unexpected disk failure"
    );
    assert_eq!(other_read.details, None);
}

#[test]
fn message_classification_covers_common_connection_failures_and_history_strings() {
    let cases = [
        (
            "HTTP transport failed: error sending request for url (http://127.0.0.1:1/): client error (Connect): tcp connect error: Connection refused (os error 111)",
            "Connection refused",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): tcp connect error: Connection refused (os error 111)",
            "Connection refused",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): tcp connect error: No connection could be made because the target machine actively refused it. (os error 10061)",
            "Connection refused",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): dns error: failed to lookup address information: Name or service not known",
            "Host not found",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): dns error: failed to lookup address information: nodename nor servname provided, or not known",
            "Host not found",
        ),
        (
            "HTTP transport failed: error sending request: operation timed out",
            "Request timed out",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: UnknownIssuer",
            "Certificate not trusted",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: Revoked",
            "Certificate revoked",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): peer sent no certificates",
            "No server certificate",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: certificate revocation list expired: verification time 100",
            "Invalid server certificate",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: ExpiredRevocationList",
            "Invalid server certificate",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: Expired",
            "Certificate expired",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: certificate expired: verification time 100",
            "Certificate expired",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: certificate not valid for name \"example.com\"",
            "Certificate name mismatch",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: certificate not valid for name \"expired.example.com\"; certificate is only valid for example.com",
            "Certificate name mismatch",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: certificate not valid for name \"revoked.example.com\"; certificate is only valid for example.com",
            "Certificate name mismatch",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): received corrupt message of type InvalidContentType",
            "TLS handshake failed",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): received fatal alert: HandshakeFailure",
            "TLS handshake failed",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): received fatal alert: CertificateRequired",
            "TLS handshake failed",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): received corrupt message of type InvalidCertificateStatusType",
            "TLS handshake failed",
        ),
        (
            "HTTP transport failed: error sending request: connection closed before message completed",
            "Connection closed",
        ),
        (
            "HTTP transport failed: error sending request: client error (SendRequest): connection reset by peer",
            "Connection reset",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): tcp connect error: Network is unreachable (os error 101)",
            "Network unreachable",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): tcp connect error: No route to host (os error 113)",
            "Host unreachable",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): proxy authentication required",
            "Proxy authentication required",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): unexpected eof while tunneling",
            "Proxy connection failed",
        ),
        (
            "HTTP transport failed: too many redirects",
            "Too many redirects",
        ),
        (
            "HTTP transport failed: error sending request: client error (Connect): custom socket error",
            "Connection failed",
        ),
    ];

    for (raw, expected_title) in cases {
        let summary = ErrorSummary::from_message(raw);

        assert_eq!(summary.title, expected_title, "{raw}");
        assert_eq!(summary.details.as_deref(), Some(raw), "{raw}");
        assert!(!summary.hint.is_empty(), "{raw}");
    }

    let history_timeout = ErrorSummary::from_message("request timed out after 30s");
    assert_eq!(history_timeout.title, "Request timed out");
    assert!(history_timeout.hint.contains("30s"));
    assert_eq!(
        history_timeout.details.as_deref(),
        Some("request timed out after 30s")
    );

    // Expired, not-yet-valid, hostname-mismatch, revoked, missing, and other
    // invalid server certificate hints should not tell users to turn off
    // certificate verification.
    for error in [
        rustls::Error::InvalidCertificate(rustls::CertificateError::Expired),
        rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidYet),
        rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidForName),
        rustls::Error::InvalidCertificate(rustls::CertificateError::Revoked),
        rustls::Error::InvalidCertificate(rustls::CertificateError::BadSignature),
        rustls::Error::NoCertificatesPresented,
    ] {
        let summary =
            ExecutionError::Transport(io::Error::new(io::ErrorKind::InvalidData, error).into())
                .summary();
        assert!(!summary.hint.contains("turn off"), "{}", summary.hint);
    }

    for message in [
        "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: Revoked",
        "HTTP transport failed: error sending request: client error (Connect): peer sent no certificates",
        "HTTP transport failed: error sending request: client error (Connect): invalid peer certificate: BadSignature",
    ] {
        let summary = ErrorSummary::from_message(message);
        assert!(!summary.hint.contains("turn off"), "{}", summary.hint);
    }
}

#[test]
fn url_keywords_and_non_transport_errors_are_not_misclassified() {
    // URL paths containing error keywords must not override the real failure.
    let raw = "HTTP transport failed: error sending request for url (http://127.0.0.1:1/certificate/dns-error(timed-out)): client error (Connect): tcp connect error: Connection refused (os error 111)";
    let summary = ErrorSummary::from_message(raw);
    assert_eq!(summary.title, "Connection refused");
    assert_eq!(summary.details.as_deref(), Some(raw));

    // A URL with keywords and an unknown non-connect error must fall back cleanly.
    let unknown_with_url = "HTTP transport failed: error sending request for url (https://example.com/certificate?ref=connection-refused): unknown protocol framing error";
    let fallback = ErrorSummary::from_message(unknown_with_url);
    assert_eq!(fallback.title, "Request failed");
    assert_eq!(fallback.hint, unknown_with_url);
    assert_eq!(fallback.details, None);

    // Non-transport errors stay verbatim even when they mention transport terms.
    for message in [
        "Unknown variable {{certificate}}.",
        "pre-request script failed: connection refused",
        "invalid request URL: relative URL without a base",
        "Request cancelled",
    ] {
        let summary = ErrorSummary::from_message(message);
        assert_eq!(summary.title, "Request failed", "{message}");
        assert_eq!(summary.hint, message, "{message}");
        assert_eq!(summary.details, None, "{message}");
    }
}

#[test]
fn real_refused_connection_from_executor_classifies_as_connection_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let executor = RequestExecutor::new(&RequestPreferences {
        timeout_ms: 2_000,
        proxy: crate::ProxyPreferences {
            mode: ProxyMode::Disabled,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();

    smol::block_on(async {
        let error = executor
            .execute(
                HttpRequest {
                    path: format!("http://127.0.0.1:{port}/certificate"),
                    ..Default::default()
                },
                RequestVariables::new(HashMap::new(), None),
            )
            .await
            .unwrap_err()
            .error;
        let summary = error.summary();

        assert_eq!(summary.title, "Connection refused");
        assert!(summary.hint.contains("host and port"));
        assert_eq!(summary.details.as_deref(), Some(error.to_string().as_str()));
    });
}
