use std::{net::TcpListener, sync::Arc};

use request::{
    ExecutionError, HttpError, HttpRequest, HttpVersion, RequestExecutor, RequestPreferences,
    Response, Version,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{TlsAcceptor, rustls};

fn tls_config() -> rustls::ServerConfig {
    let certificate =
        rustls_pemfile::certs(&mut include_bytes!("fixtures/localhost.pem").as_slice())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
    let key =
        rustls_pemfile::private_key(&mut include_bytes!("fixtures/localhost-key.pem").as_slice())
            .unwrap()
            .unwrap();

    rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certificate, key)
        .unwrap()
}

#[test]
fn certificate_verification_can_be_enabled_or_disabled() {
    smol::block_on(async {
        let acceptor = TlsAcceptor::from(Arc::new(tls_config()));

        for verify in [true, false] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!("https://{}", listener.local_addr().unwrap());
            let acceptor = acceptor.clone();
            let server = reqwest_client::runtime().spawn(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let Ok(mut stream) = acceptor.accept(stream).await else {
                    return;
                };
                let mut head = Vec::new();

                while !head.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).await.unwrap();
                    head.push(byte[0]);
                }

                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecure",
                    )
                    .await
                    .unwrap();
                stream.shutdown().await.unwrap();
            });
            let executor = RequestExecutor::new(&RequestPreferences {
                ssl_certificate_verification: verify,
                timeout_ms: 2_000,
                ..RequestPreferences::default()
            })
            .unwrap();
            let result = executor
                .execute(HttpRequest {
                    path: url,
                    ..HttpRequest::default()
                })
                .await;

            if verify {
                let error = result.unwrap_err();
                println!("\n  Verify TLS = true, self-signed server -> {error}");
                assert!(matches!(
                    error,
                    ExecutionError::Http(HttpError::Transport(_))
                ));
            } else {
                let execution = result.unwrap();
                let Response::Http(response) = execution.response;
                println!(
                    "\n  Verify TLS = false, self-signed server -> {}",
                    response.status
                );
                println!("     body: {:?}", String::from_utf8_lossy(&response.body));
                assert_eq!(response.status.as_u16(), 200);
                assert_eq!(response.body, b"secure");
            }

            server.await.unwrap();
        }
    });
}

#[test]
fn selected_http2_version_is_used_on_the_wire() {
    assert_http2_request(false);
}

#[test]
fn explicit_host_and_user_agent_work_over_http2() {
    assert_http2_request(true);
}

fn assert_http2_request(explicit_host: bool) {
    smol::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/h2", listener.local_addr().unwrap());
        let host = listener.local_addr().unwrap().to_string();
        let expected_host = host.clone();
        let server = reqwest_client::runtime().spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let mut connection = h2::server::handshake(stream).await.unwrap();
            let (request, mut respond) = connection.accept().await.unwrap().unwrap();
            assert_eq!(request.version(), Version::HTTP_2);
            assert_eq!(request.uri().path(), "/h2");

            if explicit_host {
                assert_eq!(request.uri().authority().unwrap().as_str(), expected_host);
                assert!(!request.headers().contains_key("host"));
                assert_eq!(request.headers()["user-agent"], "requesteagleruntime/0.041");
            }

            let response = http_client::Response::builder()
                .status(200)
                .body(())
                .unwrap();
            respond
                .send_response(response, false)
                .unwrap()
                .send_data("hello h2".into(), true)
                .unwrap();

            // Drive the connection until the client releases its pool.
            while connection.accept().await.is_some() {}
        });
        let executor = RequestExecutor::new(&RequestPreferences {
            http_version: HttpVersion::Http2,
            timeout_ms: 2_000,
            ..RequestPreferences::default()
        })
        .unwrap();
        let execution = executor
            .execute(HttpRequest {
                path: url,
                headers: if explicit_host {
                    vec![
                        ("Host".into(), host),
                        ("User-Agent".into(), "requesteagleruntime/0.041".into()),
                    ]
                } else {
                    Vec::new()
                },
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        let Response::Http(response) = execution.response;
        println!(
            "\n  Forced HTTP/2 -> {:?} {}",
            response.version, response.status
        );
        println!("     body: {:?}", String::from_utf8_lossy(&response.body));
        assert_eq!(response.version, Version::HTTP_2);
        assert_eq!(response.body, b"hello h2");
        drop(executor);
        server.await.unwrap();
    });
}

#[test]
fn forced_http2_explains_unsupported_host_overrides_before_sending() {
    smol::block_on(async {
        let executor = RequestExecutor::new(&RequestPreferences {
            http_version: HttpVersion::Http2,
            timeout_ms: 2_000,
            ..RequestPreferences::default()
        })
        .unwrap();
        let error = executor
            .execute(HttpRequest {
                path: "http://127.0.0.1:1".into(),
                headers: vec![("Host".into(), "virtual.example".into())],
                ..HttpRequest::default()
            })
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            ExecutionError::Http(HttpError::Http2HostOverride)
        ));
    });
}

#[test]
fn auto_host_override_uses_http1_preserving_destination_and_tls_name() {
    smol::block_on(async {
        let mut config = tls_config();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!(
            "https://localhost:{}/override",
            listener.local_addr().unwrap().port()
        );
        let server = reqwest_client::runtime().spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();

            assert_eq!(
                stream.get_ref().1.alpn_protocol(),
                Some(b"http/1.1".as_slice())
            );
            assert_eq!(stream.get_ref().1.server_name(), Some("localhost"));

            let mut head = Vec::new();

            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                head.push(byte[0]);
                assert!(head.len() < 16 * 1024);
            }

            let head = String::from_utf8(head).unwrap();
            assert!(head.starts_with("GET /override HTTP/1.1\r\n"));
            assert!(head.contains("\r\nhost: virtual.example:8443\r\n"));

            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
            stream.shutdown().await.unwrap();
        });
        let executor = RequestExecutor::new(&RequestPreferences {
            http_version: HttpVersion::Auto,
            timeout_ms: 2_000,
            ssl_certificate_verification: false,
            ..RequestPreferences::default()
        })
        .unwrap();
        let execution = executor
            .execute(HttpRequest {
                path: url,
                headers: vec![("Host".into(), "virtual.example:8443".into())],
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        let Response::Http(response) = execution.response;

        assert_eq!(response.version, Version::HTTP_11);
        assert_eq!(response.body, b"ok");
        server.await.unwrap();
    });
}
