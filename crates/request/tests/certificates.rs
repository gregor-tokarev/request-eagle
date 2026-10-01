//! Custom certificate authorities and client certificates (mutual TLS) over
//! HTTP, WebSocket and gRPC connections.

use std::{collections::HashMap, net::TcpListener, path::Path, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures::StreamExt as _;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
};
use request::{
    CertificateFiles, ClientCertificate, ExecutionError, GrpcClient, GrpcRequest, HttpRequest,
    HttpSettings, ProxyMode, RequestExecutor, RequestPreferences, RequestVariables, Response,
    WebSocketConnection, WebSocketEventKind, WebSocketRequest,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self, RootCertStore,
        pki_types::{CertificateDer, PrivatePkcs8KeyDer},
        server::WebPkiClientVerifier,
    },
};

struct Authority {
    issuer: Issuer<'static, KeyPair>,
    certificate: CertificateDer<'static>,
    pem: String,
}

fn authority(name: &str) -> Authority {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.distinguished_name.push(DnType::CommonName, name);
    let certificate = params.self_signed(&key).unwrap();

    Authority {
        pem: certificate.pem(),
        certificate: certificate.der().clone(),
        issuer: Issuer::new(params, key),
    }
}

/// A certificate for `localhost` signed by `authority`, and its key.
fn issue(
    authority: &Authority,
    purpose: ExtendedKeyUsagePurpose,
) -> (CertificateDer<'static>, KeyPair) {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    params.extended_key_usages = vec![purpose];
    let certificate = params.signed_by(&key, &authority.issuer).unwrap();

    (certificate.der().clone(), key)
}

fn pem(label: &str, der: &[u8]) -> String {
    let body = STANDARD.encode(der);
    let lines = body
        .as_bytes()
        .chunks(64)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

    format!("-----BEGIN {label}-----\n{lines}\n-----END {label}-----\n")
}

/// A TLS server for `localhost` signed by `server`, which requires a client
/// certificate signed by `clients` when given.
fn acceptor(server: &Authority, clients: Option<&Authority>) -> TlsAcceptor {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let (certificate, key) = issue(server, ExtendedKeyUsagePurpose::ServerAuth);
    let builder = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap();
    let builder = match clients {
        Some(clients) => {
            let mut roots = RootCertStore::empty();
            roots.add(clients.certificate.clone()).unwrap();
            let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider)
                .build()
                .unwrap();

            builder.with_client_cert_verifier(verifier)
        }
        None => builder.with_no_client_auth(),
    };

    TlsAcceptor::from(Arc::new(
        builder
            .with_single_cert(
                vec![certificate],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap(),
    ))
}

/// Answer one HTTPS request with the number of certificates the client
/// presented. A failed handshake closes the connection.
fn serve_https(acceptor: TlsAcceptor) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();

    reqwest_client::runtime().spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();

        while let Ok((stream, _)) = listener.accept().await {
            let Ok(mut stream) = acceptor.accept(stream).await else {
                continue;
            };
            let presented = stream.get_ref().1.peer_certificates().map_or(0, <[_]>::len);
            let mut head = Vec::new();

            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                if stream.read_exact(&mut byte).await.is_err() {
                    break;
                }
                head.push(byte[0]);
            }

            let body = format!("{presented} certificates");
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });

    port
}

fn preferences(ca: &Path) -> RequestPreferences {
    let mut preferences = RequestPreferences {
        timeout_ms: 5_000,
        ca_certificates: Some(ca.to_owned()),
        ..RequestPreferences::default()
    };
    preferences.proxy.mode = ProxyMode::Disabled;

    preferences
}

fn client_certificate(host: &str, files: CertificateFiles, passphrase: &str) -> ClientCertificate {
    ClientCertificate {
        id: host.to_owned(),
        host: host.to_owned(),
        files,
        has_passphrase: !passphrase.is_empty(),
        passphrase: passphrase.to_owned(),
        passphrase_unavailable: false,
    }
}

async fn send(preferences: &RequestPreferences, port: u16) -> Result<String, ExecutionError> {
    let execution = RequestExecutor::new(preferences)?
        .execute(
            HttpRequest {
                path: format!("https://localhost:{port}/"),
                ..HttpRequest::default()
            },
            RequestVariables::new(HashMap::new(), None),
        )
        .await?;
    let Response::Http(response) = execution.response;

    Ok(String::from_utf8(response.body).unwrap())
}

#[test]
fn trusts_a_custom_certificate_authority_in_addition_to_the_system() {
    let directory = tempfile::tempdir().unwrap();
    let ca = authority("Request Eagle Test CA");
    let ca_path = directory.path().join("ca.pem");
    std::fs::write(&ca_path, &ca.pem).unwrap();
    let port = serve_https(acceptor(&ca, None));

    smol::block_on(async {
        let mut system_only = preferences(&ca_path);
        system_only.ca_certificates = None;
        let error = send(&system_only, port).await.unwrap_err();
        assert!(matches!(error, ExecutionError::Transport(_)), "{error}");

        assert_eq!(
            send(&preferences(&ca_path), port).await.unwrap(),
            "0 certificates"
        );

        let missing = preferences(&directory.path().join("missing.pem"));
        let error = send(&missing, port).await.unwrap_err();
        assert!(matches!(error, ExecutionError::Certificate(_)), "{error}");
        assert!(
            error
                .to_string()
                .starts_with("CA certificates: could not read"),
            "{error}"
        );

        // Connections that check no certificates do not need the file.
        let executor = RequestExecutor::new(&missing).unwrap();
        let unchecked = HttpRequest {
            path: format!("https://localhost:{port}/"),
            settings: HttpSettings {
                verify_certificates: Some(false),
                ..HttpSettings::default()
            },
            ..HttpRequest::default()
        };
        executor
            .execute(unchecked, RequestVariables::new(HashMap::new(), None))
            .await
            .unwrap();

        let plain = serve_http();
        executor
            .execute(
                HttpRequest {
                    path: plain,
                    ..HttpRequest::default()
                },
                RequestVariables::new(HashMap::new(), None),
            )
            .await
            .unwrap();
    });
}

/// Answer one plain HTTP request.
fn serve_http() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());

    std::thread::spawn(move || {
        use std::io::{Read as _, Write as _};

        let (mut stream, _) = listener.accept().unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
    });

    url
}

#[test]
fn client_certificates_are_never_offered_to_a_proxy_reached_over_https() {
    let directory = tempfile::tempdir().unwrap();
    let ca = authority("Request Eagle Test CA");
    let ca_path = directory.path().join("ca.pem");
    std::fs::write(&ca_path, &ca.pem).unwrap();
    let clients = authority("Request Eagle Client CA");
    let (certificate, key) = issue(&clients, ExtendedKeyUsagePurpose::ClientAuth);
    let certificate_path = directory.path().join("client.pem");
    std::fs::write(
        &certificate_path,
        pem("CERTIFICATE", &certificate) + &key.serialize_pem(),
    )
    .unwrap();
    let port = serve_https(acceptor(&ca, Some(&clients)));

    let mut preferences = preferences(&ca_path);
    preferences.client_certificates = vec![client_certificate(
        "localhost",
        CertificateFiles::Pem {
            certificate: certificate_path,
            key: None,
        },
        "",
    )];
    // Nothing listens there: the request must fail before connecting.
    preferences.proxy.mode = ProxyMode::Custom;
    preferences.proxy.protocol = request::ProxyProtocol::Https;
    preferences.proxy.host = "127.0.0.1".into();
    preferences.proxy.port = 9;

    smol::block_on(async {
        let error = send(&preferences, port).await.unwrap_err();
        assert!(matches!(error, ExecutionError::Certificate(_)), "{error}");
        assert!(
            error.to_string().contains("proxy reached over HTTPS"),
            "{error}"
        );

        // A host that bypasses the proxy connects directly with its certificate.
        preferences.proxy.bypass = "localhost".into();
        assert_eq!(send(&preferences, port).await.unwrap(), "1 certificates");
    });
}

#[test]
fn presents_the_client_certificate_for_the_host_from_pem_files() {
    let directory = tempfile::tempdir().unwrap();
    let ca = authority("Request Eagle Test CA");
    let ca_path = directory.path().join("ca.pem");
    std::fs::write(&ca_path, &ca.pem).unwrap();
    let clients = authority("Request Eagle Client CA");
    let (certificate, key) = issue(&clients, ExtendedKeyUsagePurpose::ClientAuth);
    let certificate_path = directory.path().join("client.crt");
    let key_path = directory.path().join("client.key");
    let combined_path = directory.path().join("client.pem");
    std::fs::write(&certificate_path, pem("CERTIFICATE", &certificate)).unwrap();
    std::fs::write(&key_path, key.serialize_pem()).unwrap();
    std::fs::write(
        &combined_path,
        pem("CERTIFICATE", &certificate) + &key.serialize_pem(),
    )
    .unwrap();
    let port = serve_https(acceptor(&ca, Some(&clients)));
    let separate = CertificateFiles::Pem {
        certificate: certificate_path,
        key: Some(key_path),
    };

    smol::block_on(async {
        let mut preferences = preferences(&ca_path);
        assert!(
            send(&preferences, port).await.is_err(),
            "the server requires a certificate"
        );

        for host in [
            "localhost".to_owned(),
            format!("localhost:{port}"),
            "LOCALHOST.".into(),
        ] {
            preferences.client_certificates = vec![client_certificate(&host, separate.clone(), "")];
            assert_eq!(
                send(&preferences, port).await.unwrap(),
                "1 certificates",
                "{host}"
            );
        }

        preferences.client_certificates = vec![client_certificate(
            "localhost",
            CertificateFiles::Pem {
                certificate: combined_path,
                key: None,
            },
            "",
        )];
        assert_eq!(send(&preferences, port).await.unwrap(), "1 certificates");

        // A key from another certificate is rejected before connecting.
        let (_, other_key) = issue(&clients, ExtendedKeyUsagePurpose::ClientAuth);
        let other_key_path = directory.path().join("other.key");
        std::fs::write(&other_key_path, other_key.serialize_pem()).unwrap();
        let mismatched = client_certificate(
            "localhost",
            CertificateFiles::Pem {
                certificate: directory.path().join("client.crt"),
                key: Some(other_key_path),
            },
            "",
        );
        assert_eq!(
            mismatched.check().unwrap_err(),
            "the private key does not belong to the certificate"
        );
        preferences.client_certificates = vec![mismatched];
        let error = send(&preferences, port).await.unwrap_err();
        assert!(matches!(error, ExecutionError::Certificate(_)), "{error}");
        assert!(
            client_certificate("localhost", separate.clone(), "")
                .check()
                .is_ok()
        );

        // Certificates for other hosts and ports are not sent.
        for host in ["example.com", "localhost:1", "*.localhost"] {
            preferences.client_certificates = vec![client_certificate(host, separate.clone(), "")];
            assert!(send(&preferences, port).await.is_err(), "{host}");
        }
    });
}

#[test]
fn decrypts_encrypted_keys_and_pkcs12_files_with_their_passphrase() {
    let directory = tempfile::tempdir().unwrap();
    let ca = authority("Request Eagle Test CA");
    let ca_path = directory.path().join("ca.pem");
    std::fs::write(&ca_path, &ca.pem).unwrap();
    let clients = authority("Request Eagle Client CA");
    let (certificate, key) = issue(&clients, ExtendedKeyUsagePurpose::ClientAuth);
    let port = serve_https(acceptor(&ca, Some(&clients)));

    // An encrypted PKCS #8 key, as `openssl pkcs8 -topk8` writes it.
    let parameters = pkcs8::pkcs5::pbes2::Parameters::generate_pbkdf2_sha256_aes256cbc(
        2048,
        b"request-eagle-salt",
        [7; 16],
    )
    .unwrap();
    let encrypted = pkcs8::PrivateKeyInfoRef::try_from(key.serialize_der().as_slice())
        .unwrap()
        .encrypt_with_params(parameters, "open sesame")
        .unwrap();
    let certificate_path = directory.path().join("client.crt");
    let key_path = directory.path().join("client.key");
    std::fs::write(&certificate_path, pem("CERTIFICATE", &certificate)).unwrap();
    std::fs::write(
        &key_path,
        pem("ENCRYPTED PRIVATE KEY", encrypted.as_bytes()),
    )
    .unwrap();
    let encrypted_pem = CertificateFiles::Pem {
        certificate: certificate_path,
        key: Some(key_path),
    };

    // PKCS #12 files, with the current and the legacy encryption.
    let mut store = p12_keystore::KeyStore::new();
    store.add_entry(
        "client",
        p12_keystore::KeyStoreEntry::PrivateKeyChain(p12_keystore::PrivateKeyChain::new(
            vec![1],
            p12_keystore::PrivateKey::from_der(&key.serialize_der()).unwrap(),
            [p12_keystore::Certificate::from_der(&certificate).unwrap()],
        )),
    );
    let pkcs12 = |name: &str, algorithm| {
        let path = directory.path().join(name);
        let data = store
            .writer("open sesame")
            .encryption_algorithm(algorithm)
            .encryption_iterations(2048)
            .mac_iterations(2048)
            .write()
            .unwrap();
        std::fs::write(&path, data).unwrap();

        CertificateFiles::Pkcs12 { path }
    };
    let modern = pkcs12(
        "client.p12",
        p12_keystore::EncryptionAlgorithm::PbeWithHmacSha256AndAes256,
    );
    let legacy = pkcs12(
        "legacy.pfx",
        p12_keystore::EncryptionAlgorithm::PbeWithShaAnd3KeyTripleDesCbc,
    );

    smol::block_on(async {
        let mut preferences = preferences(&ca_path);

        for files in [&encrypted_pem, &modern, &legacy] {
            preferences.client_certificates = vec![client_certificate(
                "localhost",
                files.clone(),
                "open sesame",
            )];
            assert_eq!(
                send(&preferences, port).await.unwrap(),
                "1 certificates",
                "{files:?}"
            );

            preferences.client_certificates =
                vec![client_certificate("localhost", files.clone(), "wrong")];
            assert!(preferences.client_certificates[0].check().is_err());
            let error = send(&preferences, port).await.unwrap_err();
            assert!(matches!(error, ExecutionError::Certificate(_)), "{error}");
            assert!(
                error.to_string().contains("the passphrase is incorrect"),
                "{error}"
            );
        }

        let mut unavailable = client_certificate("localhost", modern.clone(), "");
        unavailable.passphrase_unavailable = true;
        preferences.client_certificates = vec![unavailable];
        let error = send(&preferences, port).await.unwrap_err();
        assert!(
            error.to_string().contains("passphrase is unavailable"),
            "{error}"
        );
    });
}

#[test]
fn websocket_handshakes_present_the_client_certificate() {
    let directory = tempfile::tempdir().unwrap();
    let ca = authority("Request Eagle Test CA");
    let ca_path = directory.path().join("ca.pem");
    std::fs::write(&ca_path, &ca.pem).unwrap();
    let clients = authority("Request Eagle Client CA");
    let (certificate, key) = issue(&clients, ExtendedKeyUsagePurpose::ClientAuth);
    let certificate_path = directory.path().join("client.pem");
    std::fs::write(
        &certificate_path,
        pem("CERTIFICATE", &certificate) + &key.serialize_pem(),
    )
    .unwrap();

    let acceptor = acceptor(&ca, Some(&clients));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    reqwest_client::runtime().spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let stream = acceptor.accept(stream).await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let _ = socket.close(None).await;
    });

    let mut preferences = preferences(&ca_path);
    preferences.client_certificates = vec![client_certificate(
        "localhost",
        CertificateFiles::Pem {
            certificate: certificate_path,
            key: None,
        },
        "",
    )];
    let (_connection, mut events) = WebSocketConnection::open(
        WebSocketRequest {
            url: format!("wss://localhost:{port}"),
            ..WebSocketRequest::default()
        },
        RequestVariables::new(HashMap::new(), None),
        &preferences,
    );

    smol::block_on(async {
        let event = smol::future::or(async { events.next().await.unwrap().kind }, async {
            smol::Timer::after(Duration::from_secs(10)).await;
            panic!("timed out waiting for the connection");
        })
        .await;

        assert!(
            matches!(event, WebSocketEventKind::Connected(_)),
            "{event:?}"
        );
    });
}

#[tokio::test]
async fn grpc_connections_present_the_client_certificate() {
    let directory = tempfile::tempdir().unwrap();
    let ca = authority("Request Eagle Test CA");
    let ca_path = directory.path().join("ca.pem");
    std::fs::write(&ca_path, &ca.pem).unwrap();
    let clients = authority("Request Eagle Client CA");
    let (certificate, key) = issue(&clients, ExtendedKeyUsagePurpose::ClientAuth);
    let certificate_path = directory.path().join("client.pem");
    std::fs::write(
        &certificate_path,
        pem("CERTIFICATE", &certificate) + &key.serialize_pem(),
    )
    .unwrap();

    // The handshake is enough to see the certificate; the server then hangs up.
    let acceptor = acceptor(&ca, Some(&clients));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (presented, mut handshakes) = futures::channel::mpsc::unbounded();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let presented = presented.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let certificates = acceptor
                    .accept(stream)
                    .await
                    .ok()
                    .and_then(|stream| Some(stream.get_ref().1.peer_certificates()?.len()));
                let _ = presented.unbounded_send(certificates);
            });
        }
    });

    let mut preferences = preferences(&ca_path);
    preferences.client_certificates = vec![client_certificate(
        "localhost",
        CertificateFiles::Pem {
            certificate: certificate_path,
            key: None,
        },
        "",
    )];
    let request = GrpcRequest {
        url: format!("grpcs://localhost:{port}"),
        method: "echo.v1.EchoService/Say".into(),
        ..GrpcRequest::default()
    };

    let _ = GrpcClient::new(&preferences)
        .load_definition(&request, &RequestVariables::new(HashMap::new(), None), None)
        .await;

    assert_eq!(handshakes.next().await, Some(Some(1)));
}

#[test]
fn https_proxies_from_the_environment_follow_the_transport_bypass_rules() {
    // A literal `*.localhost` does not bypass the proxy for the transport.
    for (no_proxy, direct) in [
        (None, false),
        (Some("localhost"), true),
        (Some("*.localhost"), false),
    ] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "system_proxy_child", "--nocapture"]);

        // Use a child process so environment changes cannot race other tests.
        for name in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
            "REQUEST_METHOD",
        ] {
            command.env_remove(name);
        }
        if let Some(no_proxy) = no_proxy {
            command.env("NO_PROXY", no_proxy);
        }

        let output = command
            .env("HTTPS_PROXY", "https://127.0.0.1:9")
            .env("REQUEST_EAGLE_TEST_DIRECT", direct.to_string())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{no_proxy:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn system_proxy_child() {
    let Ok(direct) = std::env::var("REQUEST_EAGLE_TEST_DIRECT") else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let ca = authority("Request Eagle Test CA");
    let ca_path = directory.path().join("ca.pem");
    std::fs::write(&ca_path, &ca.pem).unwrap();
    let clients = authority("Request Eagle Client CA");
    let (certificate, key) = issue(&clients, ExtendedKeyUsagePurpose::ClientAuth);
    let certificate_path = directory.path().join("client.pem");
    std::fs::write(
        &certificate_path,
        pem("CERTIFICATE", &certificate) + &key.serialize_pem(),
    )
    .unwrap();
    let port = serve_https(acceptor(&ca, Some(&clients)));

    let mut preferences = preferences(&ca_path);
    preferences.proxy.mode = ProxyMode::System;
    preferences.client_certificates = vec![client_certificate(
        "localhost",
        CertificateFiles::Pem {
            certificate: certificate_path,
            key: None,
        },
        "",
    )];

    smol::block_on(async {
        let result = send(&preferences, port).await;

        if direct == "true" {
            assert_eq!(result.unwrap(), "1 certificates");
        } else {
            let error = result.unwrap_err();
            assert!(
                error.to_string().contains("proxy reached over HTTPS"),
                "{error}"
            );
        }
    });
}
