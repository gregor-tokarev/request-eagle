use std::{net::TcpListener, sync::Arc};

use request::{
    ExecutionError, HttpError, HttpRequest, ProxyMode, ProxyPreferences, ProxyProtocol,
    RequestExecutor, RequestPreferences, Response,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{TlsAcceptor, rustls};

fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();

    (listener, port)
}

async fn read_head(stream: &mut (impl AsyncRead + Unpin)) -> String {
    let mut head = Vec::new();

    while !head.ends_with(b"\r\n\r\n") {
        head.push(stream.read_u8().await.unwrap());
        assert!(head.len() < 16 * 1024);
    }

    String::from_utf8(head).unwrap()
}

fn acceptor() -> TlsAcceptor {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["destination.invalid".into()]).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert.der().clone()], signing_key.into())
    .unwrap();

    TlsAcceptor::from(Arc::new(config))
}

fn custom(port: u16) -> ProxyPreferences {
    ProxyPreferences {
        mode: ProxyMode::Custom,
        host: "127.0.0.1".into(),
        port,
        authentication: true,
        username: "user".into(),
        password: "pass".into(),
        ..ProxyPreferences::default()
    }
}

async fn send(proxy: ProxyPreferences, url: String, override_host: bool) {
    let executor = RequestExecutor::new(&RequestPreferences {
        proxy,
        // The loopback TLS servers in these proxy tests use self-signed certificates.
        ssl_certificate_verification: false,
        timeout_ms: 2_000,
        ..RequestPreferences::default()
    })
    .unwrap();
    let result = executor
        .execute(HttpRequest {
            path: url,
            headers: if override_host {
                vec![("Host".into(), "virtual.invalid".into())]
            } else {
                vec![]
            },
            ..HttpRequest::default()
        })
        .await
        .unwrap();
    let Response::Http(response) = result.response;

    assert_eq!(response.body, b"ok");
}

const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

#[test]
fn full_proxy_urls_fill_endpoint_and_decoded_credentials() {
    let proxy =
        ProxyPreferences::from_url("  http://proxy-user:demo-password@proxy.example.com:20100  ")
            .unwrap();

    assert_eq!(proxy.mode, ProxyMode::Custom);
    assert_eq!(proxy.protocol, ProxyProtocol::Http);
    assert_eq!(proxy.host, "proxy.example.com");
    assert_eq!(proxy.port, 20100);
    assert!(proxy.authentication);
    assert_eq!(proxy.username, "proxy-user");
    assert_eq!(proxy.password, "demo-password");

    let proxy =
        ProxyPreferences::from_url("https://user%40team:p%3Ass%2Fword%25+@proxy.example:8443/")
            .unwrap();
    assert_eq!(proxy.protocol, ProxyProtocol::Https);
    assert_eq!(proxy.port, 8443);
    assert_eq!(proxy.username, "user@team");
    assert_eq!(proxy.password, "p:ss/word%+");

    let proxy = ProxyPreferences::from_url("http://proxy.example:80").unwrap();
    assert_eq!(proxy.port, 80);
    assert!(!proxy.authentication);
    assert!(proxy.username.is_empty());
    assert!(proxy.password.is_empty());

    let proxy = ProxyPreferences::from_url("https://[::1]").unwrap();
    assert_eq!(proxy.host, "[::1]");
    assert_eq!(proxy.port, 443);
}

#[test]
fn invalid_proxy_urls_are_rejected_without_echoing_credentials() {
    for value in [
        "http://user:demo-password@proxy.example:70000",
        "http://user:demo-password@proxy.example:0",
        "socks5://user:demo-password@proxy.example:1080",
        "http://user:demo-password@proxy.example/path",
        "http://user:demo-password@proxy.example?query=1",
        "http://user:demo-password@proxy.example#fragment",
        "http://user%3Aname:demo-password@proxy.example",
        "http://%FF:demo-password@proxy.example",
        "http://",
    ] {
        let error = ProxyPreferences::from_url(value).unwrap_err();
        assert!(!error.contains("demo-password"));
    }
}

#[test]
fn http_requests_use_custom_proxy_including_the_host_override_client() {
    smol::block_on(async {
        for override_host in [false, true] {
            let (listener, port) = listener();
            let server = reqwest_client::runtime().spawn(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let (mut stream, _) = listener.accept().await.unwrap();
                let head = read_head(&mut stream).await;
                stream.write_all(RESPONSE).await.unwrap();

                head
            });

            let mut proxy = custom(port);
            proxy.bypass = "other.invalid, tination.invalid".into();

            send(
                proxy,
                "http://destination.invalid/resource?x=1".into(),
                override_host,
            )
            .await;
            let head = server.await.unwrap().to_ascii_lowercase();

            assert!(head.starts_with("get http://destination.invalid/resource?x=1 http/1.1\r\n"));
            assert!(head.contains("\r\nproxy-authorization: basic dxnlcjpwyxnz\r\n"));

            if override_host {
                assert!(head.contains("\r\nhost: virtual.invalid\r\n"));
            }
        }
    });
}

#[test]
fn https_uses_connect_and_keeps_proxy_credentials_out_of_the_origin_request() {
    smol::block_on(async {
        let (listener, port) = listener();
        let tls = acceptor();
        let server = reqwest_client::runtime().spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (mut stream, _) = listener.accept().await.unwrap();
            let connect = read_head(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
            let mut stream = tls.accept(stream).await.unwrap();
            let head = read_head(&mut stream).await;
            stream.write_all(RESPONSE).await.unwrap();
            stream.shutdown().await.unwrap();

            (connect, head)
        });

        send(
            custom(port),
            "https://destination.invalid/secure".into(),
            false,
        )
        .await;
        let (connect, head) = server.await.unwrap();

        assert!(connect.starts_with("CONNECT destination.invalid:443 HTTP/1.1\r\n"));
        assert!(
            connect
                .to_ascii_lowercase()
                .contains("proxy-authorization: basic dxnlcjpwyxnz")
        );
        assert!(head.starts_with("GET /secure HTTP/1.1\r\n"));
        assert!(!head.to_ascii_lowercase().contains("proxy-authorization"));
    });
}

#[test]
fn https_proxy_protocol_encrypts_the_connection_to_the_proxy() {
    smol::block_on(async {
        let (listener, port) = listener();
        let tls = acceptor();
        let server = reqwest_client::runtime().spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = tls.accept(stream).await.unwrap();
            let head = read_head(&mut stream).await;
            stream.write_all(RESPONSE).await.unwrap();
            stream.shutdown().await.unwrap();

            head
        });
        let mut proxy = custom(port);
        proxy.protocol = ProxyProtocol::Https;
        proxy.authentication = false;

        send(proxy, "http://destination.invalid/resource".into(), false).await;
        let head = server.await.unwrap();

        assert!(head.starts_with("GET http://destination.invalid/resource HTTP/1.1\r\n"));
        assert!(!head.to_ascii_lowercase().contains("proxy-authorization"));
    });
}

#[test]
fn bypass_hosts_and_excluded_request_types_connect_directly_without_proxy_auth() {
    smol::block_on(async {
        for (host, bypass, mode, http) in [
            ("127.0.0.1", "127.0.0.1", ProxyMode::Custom, true),
            ("127.0.0.1", "127.0.0.0/8", ProxyMode::Custom, true),
            ("127.0.0.1", "*", ProxyMode::Custom, true),
            (
                "localhost",
                "example.com, *.LOCALHOST",
                ProxyMode::Custom,
                true,
            ),
            ("127.0.0.1", "", ProxyMode::Custom, false),
            ("127.0.0.1", "", ProxyMode::Disabled, true),
        ] {
            let (listener, port) = listener();
            let (unused_proxy, proxy_port) = self::listener();
            let server = reqwest_client::runtime().spawn(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let (mut stream, _) = listener.accept().await.unwrap();
                let head = read_head(&mut stream).await;
                stream.write_all(RESPONSE).await.unwrap();

                head
            });
            let mut proxy = custom(proxy_port);
            proxy.mode = mode;
            proxy.http = http;
            proxy.bypass = bypass.into();

            send(proxy, format!("http://{host}:{port}/direct"), false).await;
            let head = server.await.unwrap();

            assert!(head.starts_with("GET /direct HTTP/1.1\r\n"));
            assert!(!head.to_ascii_lowercase().contains("proxy-authorization"));
            assert_eq!(
                unused_proxy.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
    });
}

#[test]
fn invalid_custom_proxy_is_rejected_before_sending() {
    for host in [
        "",
        "http://proxy.example",
        "proxy.example:8080",
        "proxy.example/path",
        "user:pass@proxy.example",
    ] {
        let mut proxy = custom(8080);
        proxy.host = host.into();
        assert!(proxy.validate().is_err(), "{host}");
        assert!(matches!(
            RequestExecutor::new(&RequestPreferences {
                proxy,
                ..RequestPreferences::default()
            }),
            Err(ExecutionError::Http(HttpError::InvalidProxy(_)))
        ));
    }

    let mut proxy = custom(0);
    assert!(proxy.validate().is_err());
    proxy.port = 8080;
    proxy.http = false;
    proxy.https = false;
    assert!(proxy.validate().is_err());
    proxy.mode = ProxyMode::Disabled;
    assert!(proxy.validate().is_ok());

    for host in ["proxy.example", "127.0.0.1", "[::1]"] {
        let mut proxy = custom(8080);
        proxy.host = host.into();
        assert!(proxy.validate().is_ok(), "{host}");
    }
}

#[test]
fn inactive_proxy_settings_reject_credentials_in_the_host() {
    for mode in [ProxyMode::System, ProxyMode::Disabled] {
        let mut proxy = ProxyPreferences {
            mode,
            ..ProxyPreferences::default()
        };
        assert!(proxy.validate().is_ok());

        for host in [
            "user:password@proxy.example:3128",
            "http://user:password@proxy.example:3128",
        ] {
            proxy.host = host.into();
            assert!(proxy.validate().is_err());
        }
    }
}

#[test]
fn older_preferences_keep_system_proxy_defaults_and_debug_redacts_credentials() {
    let preferences: RequestPreferences = serde_json::from_str("{}").unwrap();
    assert_eq!(preferences.proxy, ProxyPreferences::default());

    let mut proxy = custom(8080);
    proxy.username = "private-username".into();
    proxy.password = "private-password".into();
    let debug = format!("{proxy:?}");
    assert!(!debug.contains("private-username"));
    assert!(!debug.contains("private-password"));
}

#[test]
fn system_proxy_is_used_only_in_system_mode() {
    smol::block_on(async {
        for mode in ["system", "custom", "disabled", "bypass", "excluded"] {
            let (destination, destination_port) = listener();
            let (proxy, proxy_port) = listener();
            let (unused, unused_port) = listener();
            let proxied = matches!(mode, "system" | "custom");
            let server_listener = if proxied { proxy } else { destination };
            let server = reqwest_client::runtime().spawn(async move {
                let listener = tokio::net::TcpListener::from_std(server_listener).unwrap();
                let (mut stream, _) = listener.accept().await.unwrap();
                let head = read_head(&mut stream).await;
                stream.write_all(RESPONSE).await.unwrap();

                head
            });
            let system_port = if mode == "system" {
                proxy_port
            } else {
                unused_port
            };
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "proxy_environment_child", "--nocapture"]);

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

            let output = command
                .env("HTTP_PROXY", format!("http://127.0.0.1:{system_port}"))
                .env("REQUEST_EAGLE_TEST_PROXY_MODE", mode)
                .env("REQUEST_EAGLE_TEST_PROXY_PORT", proxy_port.to_string())
                .env(
                    "REQUEST_EAGLE_TEST_DESTINATION_PORT",
                    destination_port.to_string(),
                )
                .output()
                .unwrap();

            assert!(
                output.status.success(),
                "{mode}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let head = server.await.unwrap();
            assert_eq!(head.starts_with("GET http://"), proxied, "{mode}");
            assert_eq!(
                unused.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
    });
}

#[test]
fn proxy_environment_child() {
    let Ok(mode) = std::env::var("REQUEST_EAGLE_TEST_PROXY_MODE") else {
        return;
    };
    let port = std::env::var("REQUEST_EAGLE_TEST_PROXY_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let destination_port = std::env::var("REQUEST_EAGLE_TEST_DESTINATION_PORT").unwrap();
    let mut proxy = custom(port);

    match mode.as_str() {
        "system" => proxy.mode = ProxyMode::System,
        "disabled" => proxy.mode = ProxyMode::Disabled,
        "bypass" => proxy.bypass = "127.0.0.1".into(),
        "excluded" => proxy.http = false,
        "custom" => {}
        _ => panic!("Unknown proxy test mode"),
    }

    smol::block_on(send(
        proxy,
        format!("http://127.0.0.1:{destination_port}/"),
        false,
    ));
}

#[test]
fn redirects_to_bypassed_hosts_do_not_forward_proxy_credentials() {
    smol::block_on(async {
        for override_host in [false, true] {
            let (proxy_listener, proxy_port) = listener();
            let (origin_listener, origin_port) = listener();
            let proxy_server = reqwest_client::runtime().spawn(async move {
                let listener = tokio::net::TcpListener::from_std(proxy_listener).unwrap();
                let (mut stream, _) = listener.accept().await.unwrap();
                let head = read_head(&mut stream).await;
                assert!(head.to_lowercase().contains("\r\nproxy-authorization: basic dxnlcjpwyxnz\r\n"));
                stream.write_all(format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{origin_port}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                ).as_bytes()).await.unwrap();
            });
            let origin_server = reqwest_client::runtime().spawn(async move {
                let listener = tokio::net::TcpListener::from_std(origin_listener).unwrap();
                let (mut stream, _) = listener.accept().await.unwrap();
                let head = read_head(&mut stream).await;
                assert!(head.starts_with("GET /final HTTP/1.1\r\n"));
                assert!(!head.to_lowercase().contains("proxy-authorization"));
                stream.write_all(RESPONSE).await.unwrap();
            });
            let mut proxy = custom(proxy_port);
            proxy.bypass = "127.0.0.1".into();
            send(
                proxy,
                "http://destination.invalid/redirect".into(),
                override_host,
            )
            .await;
            proxy_server.await.unwrap();
            origin_server.await.unwrap();
        }
    });
}

#[test]
fn same_port_redirects_to_direct_https_do_not_leak_proxy_credentials() {
    smol::block_on(async {
        for override_host in [false, true] {
            let (proxy_listener, proxy_port) = listener();
            let (origin_listener, origin_port) = listener();
            let tls = acceptor();
            let proxy_server = reqwest_client::runtime().spawn(async move {
                let listener = tokio::net::TcpListener::from_std(proxy_listener).unwrap();
                let (mut stream, _) = listener.accept().await.unwrap();
                let head = read_head(&mut stream).await;
                assert!(head.contains("\r\nproxy-authorization: Basic dXNlcjpwYXNz\r\n"));
                stream.write_all(format!(
                    "HTTP/1.1 302 Found\r\nLocation: https://127.0.0.1:{origin_port}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                ).as_bytes()).await.unwrap();
            });
            let origin_server = reqwest_client::runtime().spawn(async move {
                let listener = tokio::net::TcpListener::from_std(origin_listener).unwrap();
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = tls.accept(stream).await.unwrap();
                let head = read_head(&mut stream).await;
                stream.write_all(RESPONSE).await.unwrap();
                stream.shutdown().await.unwrap();

                head
            });
            let mut proxy = custom(proxy_port);
            proxy.https = false;

            send(
                proxy,
                format!("http://127.0.0.1:{origin_port}/redirect"),
                override_host,
            )
            .await;
            proxy_server.await.unwrap();
            let head = origin_server.await.unwrap().to_ascii_lowercase();
            assert!(head.starts_with("get /final http/1.1\r\n"));
            assert!(!head.contains("proxy-authorization"));
        }
    });
}

#[test]
fn cross_host_redirects_authenticate_each_request_to_the_proxy() {
    smol::block_on(async {
        for override_host in [false, true] {
            let (proxy_listener, proxy_port) = listener();
            let proxy_server = reqwest_client::runtime().spawn(async move {
                let listener = tokio::net::TcpListener::from_std(proxy_listener).unwrap();

                for host in ["first.invalid", "second.invalid"] {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let head = read_head(&mut stream).await;
                    assert!(head.starts_with(&format!("GET http://{host}/")));

                    if !head.contains("\r\nproxy-authorization: Basic dXNlcjpwYXNz\r\n") {
                        stream.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                        return;
                    }

                    if host == "first.invalid" {
                        stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://second.invalid/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                    } else {
                        stream.write_all(RESPONSE).await.unwrap();
                    }
                }
            });

            send(
                custom(proxy_port),
                "http://first.invalid/redirect".into(),
                override_host,
            )
            .await;
            proxy_server.await.unwrap();
        }
    });
}
