use std::time::Duration;

use request::{
    Execution, ExecutionError, HttpError, HttpRequest, HttpVersion, Method, Request,
    RequestExecutor, RequestPreferences, Response, Version,
};
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

struct ReceivedRequest {
    head: String,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> ReceivedRequest {
    let mut head = Vec::new();

    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
        assert!(head.len() < 16 * 1024);
    }

    let head = String::from_utf8(head).unwrap();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;

            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await.unwrap();

    ReceivedRequest { head, body }
}

async fn serve(response: Vec<u8>) -> (String, smol::Task<ReceivedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let received = read_request(&mut stream).await;
        // Oversized responses may be cancelled before the server finishes writing.
        let _ = stream.write_all(&response).await;

        received
    });

    (url, server)
}

fn executor() -> RequestExecutor {
    RequestExecutor::new(&RequestPreferences {
        timeout_ms: 2_000,
        http_version: HttpVersion::Http1_1,
        ssl_certificate_verification: true,
        ..RequestPreferences::default()
    })
    .unwrap()
}

fn report(label: &str, execution: &Execution) {
    let Response::Http(response) = &execution.response;

    println!("\n  {label}");
    println!(
        "  <- {:?} {} ({:.2?})",
        response.version, response.status, execution.elapsed
    );

    for (name, value) in &response.headers {
        println!("     {name}: {}", String::from_utf8_lossy(value.as_bytes()));
    }

    if response.body.len() <= 128 {
        println!(
            "     body ({} bytes): {:?}",
            response.body.len(),
            String::from_utf8_lossy(&response.body)
        );
    } else {
        println!("     body: {} bytes", response.body.len());
    }
}

#[test]
fn sends_a_snapshot_with_encoded_query_repeated_headers_and_binary_body() {
    smol::block_on(async {
        let response = b"HTTP/1.1 201 Created\r\nContent-Length: 3\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nX-Raw: \xff\r\nConnection: close\r\n\r\n\x00\xff\x01";
        let (url, server) = serve(response.to_vec()).await;
        let mut draft = HttpRequest {
            method: Method::Post,
            path: format!("{url}/submit?tag=existing#ignored"),
            headers: vec![
                ("X-Tag".into(), "one".into()),
                ("X-Tag".into(), "two".into()),
            ],
            body: Some(vec![0, 255, 42]),
            scripts: Default::default(),
            query: Some(vec![
                ("tag".into(), "a & b".into()),
                ("tag".into(), "c+d".into()),
            ]),
        };
        let run = executor().execute(&draft);
        draft.path = "http://unused.invalid".into();
        draft.body = None;

        let result = run.await.unwrap();
        let received = server.await;
        println!("\n  -> {}", received.head.lines().next().unwrap());
        println!("     request body bytes: {:?}", received.body);
        report("Binary response and repeated headers", &result);

        assert!(
            received
                .head
                .starts_with("POST /submit?tag=existing&tag=a+%26+b&tag=c%2Bd HTTP/1.1\r\n")
        );
        assert_eq!(received.head.to_lowercase().matches("x-tag:").count(), 2);
        assert_eq!(received.body, vec![0, 255, 42]);

        let Response::Http(response) = result.response;
        assert_eq!(response.status.as_u16(), 201);
        assert_eq!(response.version, Version::HTTP_11);
        assert_eq!(response.body, vec![0, 255, 1]);
        assert_eq!(response.headers.get_all("set-cookie").iter().count(), 2);
        assert_eq!(response.headers["x-raw"].as_bytes(), b"\xff");
        assert_eq!(response.metrics.request_body_bytes, 3);
        let sent_header_bytes: usize = received
            .head
            .lines()
            .skip(1)
            .filter(|line| !line.is_empty())
            .map(|line| line.len() + 2)
            .sum();
        assert_eq!(response.metrics.request_header_bytes, sent_header_bytes);
    });
}

#[test]
fn measures_waiting_and_download_separately() {
    smol::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = smol::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            smol::Timer::after(Duration::from_millis(30)).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            smol::Timer::after(Duration::from_millis(60)).await;
            stream.write_all(b"abc").await.unwrap();
        });
        let execution = executor()
            .execute(HttpRequest {
                path: url,
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        server.await;
        let Response::Http(response) = &execution.response;
        let metrics = response.metrics;
        assert!(metrics.waiting >= Duration::from_millis(30));
        assert!(metrics.download >= Duration::from_millis(50));
        assert!(metrics.prepare + metrics.waiting + metrics.download <= execution.elapsed);
        assert!(
            metrics.request_header_bytes > 0,
            "include generated request headers"
        );
        assert_eq!(metrics.request_body_bytes, 0);
        assert_eq!(
            metrics.response_header_bytes,
            b"content-length: 3\r\nconnection: close\r\n".len()
        );
        assert_eq!(response.body, b"abc");
    });
}

#[test]
fn executes_all_existing_methods_from_saved_requests() {
    smol::block_on(async {
        let executor = executor();

        for method in [
            Method::Get,
            Method::Post,
            Method::Put,
            Method::Patch,
            Method::Head,
            Method::Options,
            Method::Delete,
        ] {
            let (url, server) = serve(
                b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}"
                    .to_vec(),
            )
            .await;
            let request = Request::Http(HttpRequest {
                method,
                path: format!("{url}/health"),
                ..HttpRequest::default()
            });
            let result = executor.execute(&request).await.unwrap();
            let received = server.await;

            assert!(
                received
                    .head
                    .starts_with(&format!("{} /health HTTP/1.1", method.as_str()))
            );
            let Response::Http(response) = &result.response;
            if method == Method::Head {
                assert!(response.body.is_empty());
            } else {
                assert_eq!(response.body, b"{\"ok\":true}");
            }
            report(&format!("{} /health", method.as_str()), &result);
        }
    });
}

#[test]
fn http_errors_are_inspectable_responses() {
    smol::block_on(async {
        for status in ["404 Not Found", "500 Internal Server Error"] {
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: 7\r\nLocation: http://unused.invalid/\r\nConnection: close\r\n\r\ndetails"
            );
            let (url, server) = serve(response.into_bytes()).await;
            let result = executor()
                .execute(HttpRequest {
                    path: url,
                    ..HttpRequest::default()
                })
                .await
                .unwrap();
            server.await;
            report("HTTP status is returned with its body", &result);

            let Response::Http(response) = result.response;
            assert_eq!(
                response.status.as_u16(),
                status[..3].parse::<u16>().unwrap()
            );
            assert_eq!(response.body, b"details");
        }
    });
}

#[test]
fn redirects_follow_the_setting_and_preserve_http_method_semantics() {
    smol::block_on(async {
        for status in [301, 302, 303, 307, 308] {
            for follow in [true, false] {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let url = format!("http://{}/start", listener.local_addr().unwrap());
                let server = smol::spawn(async move {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let received = read_request(&mut stream).await;
                    assert!(received.head.starts_with("POST /start HTTP/1.1\r\n"));
                    assert_eq!(received.body, b"payload");

                    stream
                        .write_all(
                            format!(
                                "HTTP/1.1 {status} Redirect\r\nLocation: /final\r\nContent-Length: 5\r\nConnection: close\r\n\r\nmoved"
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                    drop(stream);

                    if follow {
                        let (mut stream, _) = listener.accept().await.unwrap();
                        let received = read_request(&mut stream).await;

                        if matches!(status, 307 | 308) {
                            assert!(received.head.starts_with("POST /final HTTP/1.1\r\n"));
                            assert_eq!(received.body, b"payload");
                        } else {
                            assert!(received.head.starts_with("GET /final HTTP/1.1\r\n"));
                            assert!(received.body.is_empty());
                        }

                        stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone")
                            .await
                            .unwrap();
                    }
                });
                let mut preferences = RequestPreferences {
                    timeout_ms: 2_000,
                    ..RequestPreferences::default()
                };

                if !follow {
                    preferences.follow_all_redirects = false;
                }

                let result = RequestExecutor::new(&preferences)
                    .unwrap()
                    .execute(HttpRequest {
                        method: Method::Post,
                        path: url,
                        body: Some(b"payload".to_vec()),
                        ..HttpRequest::default()
                    })
                    .await
                    .unwrap();
                server.await;

                let Response::Http(response) = result.response;

                if follow {
                    assert_eq!(response.status.as_u16(), 200);
                    assert_eq!(response.body, b"done");
                } else {
                    assert_eq!(response.status.as_u16(), status);
                    assert_eq!(response.headers["location"], "/final");
                    assert_eq!(response.body, b"moved");
                }
            }
        }
    });
}

#[test]
fn follows_redirect_chains_by_default() {
    smol::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = smol::spawn(async move {
            for (step, status) in [301, 302, 303, 307, 308, 200].into_iter().enumerate() {
                let (mut stream, _) = listener.accept().await.unwrap();
                let received = read_request(&mut stream).await;
                assert!(
                    received
                        .head
                        .starts_with(&format!("GET /{step} HTTP/1.1\r\n"))
                );

                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status} Response\r\nLocation: /{}\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone",
                            step + 1,
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let result = executor()
            .execute(HttpRequest {
                path: format!("{url}/0"),
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        server.await;

        let Response::Http(response) = result.response;
        assert_eq!(response.status.as_u16(), 200);
        assert_eq!(response.body, b"done");
    });
}

#[test]
fn cross_host_redirects_update_host_and_strip_sensitive_headers() {
    smol::block_on(async {
        for http_version in [HttpVersion::Auto, HttpVersion::Http1_1] {
            let (destination, destination_server) = serve(
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone".to_vec(),
            )
            .await;
            let expected_host = destination.strip_prefix("http://").unwrap().to_owned();
            let (url, server) = serve(
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: {destination}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .into_bytes(),
            )
            .await;
            let executor = RequestExecutor::new(&RequestPreferences {
                http_version,
                timeout_ms: 2_000,
                ..RequestPreferences::default()
            })
            .unwrap();
            let result = executor
                .execute(HttpRequest {
                    path: url,
                    headers: vec![
                        ("Authorization".into(), "Bearer test-token".into()),
                        ("Cookie".into(), "session=test-session".into()),
                    ],
                    ..HttpRequest::default()
                })
                .await
                .unwrap();
            server.await;
            let received = destination_server.await;

            assert!(received.head.starts_with("GET /final HTTP/1.1\r\n"));
            assert!(
                received
                    .head
                    .contains(&format!("\r\nhost: {expected_host}\r\n"))
            );
            assert!(!received.head.to_lowercase().contains("\r\nauthorization:"));
            assert!(!received.head.to_lowercase().contains("\r\ncookie:"));

            let Response::Http(response) = result.response;
            assert_eq!(response.status.as_u16(), 200);
            assert_eq!(response.body, b"done");
        }
    });
}

#[test]
fn explicit_host_overrides_only_follow_redirects_on_the_same_authority() {
    smol::block_on(async {
        for (http_version, custom_host) in [
            (HttpVersion::Auto, true),
            (HttpVersion::Http1_1, true),
            (HttpVersion::Http1_1, false),
        ] {
            for status in [301, 302, 303, 307, 308] {
                let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let origin_host = origin.local_addr().unwrap().to_string();
                let destination_host = destination.local_addr().unwrap().to_string();
                let url = format!("http://{origin_host}/start");
                let explicit_host = if custom_host {
                    "virtual.example".to_owned()
                } else {
                    origin_host.clone()
                };
                let expected_explicit_host = explicit_host.clone();
                let server = smol::spawn(async move {
                    for (listener, path, expected_host, location) in [
                        (
                            &origin,
                            "/start",
                            expected_explicit_host.as_str(),
                            Some("/same".to_owned()),
                        ),
                        (
                            &origin,
                            "/same",
                            expected_explicit_host.as_str(),
                            Some(format!("http://{destination_host}/final")),
                        ),
                        (
                            &destination,
                            "/final",
                            destination_host.as_str(),
                            Some(format!("http://{origin_host}/back")),
                        ),
                        (&origin, "/back", origin_host.as_str(), None),
                    ] {
                        let (mut stream, _) = listener.accept().await.unwrap();
                        let received = read_request(&mut stream).await;
                        let preserves_body = path == "/start" || matches!(status, 307 | 308);
                        let method = if preserves_body { "POST" } else { "GET" };

                        assert!(
                            received
                                .head
                                .starts_with(&format!("{method} {path} HTTP/1.1\r\n"))
                        );
                        assert!(
                            received
                                .head
                                .contains(&format!("\r\nhost: {expected_host}\r\n"))
                        );
                        assert_eq!(
                            received.body,
                            if preserves_body {
                                b"payload".as_slice()
                            } else {
                                b""
                            }
                        );

                        if matches!(path, "/start" | "/same") {
                            assert!(
                                received
                                    .head
                                    .contains("\r\nauthorization: Bearer test-token\r\n")
                            );
                            assert!(
                                received
                                    .head
                                    .contains("\r\ncookie: session=test-session\r\n")
                            );
                        } else {
                            assert!(!received.head.contains("\r\nauthorization:"));
                            assert!(!received.head.contains("\r\ncookie:"));
                        }

                        let response = match location {
                            Some(location) => format!(
                                "HTTP/1.1 {status} Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            ),
                            None => "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone".to_owned(),
                        };
                        stream.write_all(response.as_bytes()).await.unwrap();
                    }
                });
                let executor = RequestExecutor::new(&RequestPreferences {
                    http_version,
                    timeout_ms: 2_000,
                    ..RequestPreferences::default()
                })
                .unwrap();
                let result = executor
                    .execute(HttpRequest {
                        method: Method::Post,
                        path: url,
                        headers: vec![
                            ("Host".into(), explicit_host),
                            ("Authorization".into(), "Bearer test-token".into()),
                            ("Cookie".into(), "session=test-session".into()),
                        ],
                        body: Some(b"payload".to_vec()),
                        ..HttpRequest::default()
                    })
                    .await
                    .unwrap();
                server.await;

                let Response::Http(response) = result.response;
                assert_eq!(response.status.as_u16(), 200);
                assert_eq!(response.body, b"done");
            }
        }
    });
}

#[test]
fn explicit_host_does_not_follow_redirects_when_disabled() {
    smol::block_on(async {
        let (url, server) = serve(
            b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 5\r\nConnection: close\r\n\r\nmoved".to_vec(),
        )
        .await;
        let executor = RequestExecutor::new(&RequestPreferences {
            follow_all_redirects: false,
            timeout_ms: 2_000,
            ..RequestPreferences::default()
        })
        .unwrap();
        let result = executor
            .execute(HttpRequest {
                path: url,
                headers: vec![("Host".into(), "virtual.example".into())],
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        let received = server.await;
        assert!(received.head.contains("\r\nhost: virtual.example\r\n"));

        let Response::Http(response) = result.response;
        assert_eq!(response.status.as_u16(), 302);
        assert_eq!(response.headers["location"], "/final");
        assert_eq!(response.body, b"moved");
    });
}

#[test]
fn explicit_host_redirects_share_the_transport_redirect_limit() {
    smol::block_on(async {
        for cross_authority in [false, true] {
            let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/0", origin.local_addr().unwrap());
            let destination_url = format!("http://{}", destination.local_addr().unwrap());
            let server = smol::spawn(async move {
                for hop in 0..100 {
                    let listener = if cross_authority && hop >= 2 {
                        &destination
                    } else {
                        &origin
                    };
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let received = read_request(&mut stream).await;
                    assert!(
                        received
                            .head
                            .starts_with(&format!("GET /{hop} HTTP/1.1\r\n"))
                    );
                    let location = if cross_authority && hop == 1 {
                        format!("{destination_url}/{}", hop + 1)
                    } else {
                        format!("/{}", hop + 1)
                    };

                    stream
                        .write_all(
                            format!(
                                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                }
            });
            let executor = RequestExecutor::new(&RequestPreferences {
                timeout_ms: 5_000,
                ..RequestPreferences::default()
            })
            .unwrap();
            let error = executor
                .execute(HttpRequest {
                    path: url,
                    headers: vec![("Host".into(), "virtual.example".into())],
                    ..HttpRequest::default()
                })
                .await
                .unwrap_err();

            assert!(matches!(
                &error,
                ExecutionError::Http(HttpError::Transport(_))
            ));
            assert!(error.to_string().contains("too many redirects"), "{error}");
            server.await;
        }
    });
}

#[test]
fn rejects_invalid_urls_schemes_and_headers_before_sending() {
    smol::block_on(async {
        for path in [
            "",
            "/relative/path",
            "ws://localhost/socket",
            "file:///etc/hosts",
        ] {
            let error = executor()
                .execute(HttpRequest {
                    path: path.into(),
                    ..HttpRequest::default()
                })
                .await
                .unwrap_err();
            println!("\n  {path:?} -> {error}");
            assert!(matches!(
                error,
                ExecutionError::Http(HttpError::InvalidUrl(_) | HttpError::UnsupportedScheme(_))
            ));
        }

        for header in [("bad name", "value"), ("X-Test", "value\r\ninjected: true")] {
            let error = executor()
                .execute(HttpRequest {
                    path: "http://127.0.0.1:1".into(),
                    headers: vec![(header.0.into(), header.1.into())],
                    ..HttpRequest::default()
                })
                .await
                .unwrap_err();
            println!("\n  Invalid header -> {error}");
            assert!(matches!(
                error,
                ExecutionError::Http(HttpError::InvalidRequest(_))
            ));
        }
    });
}

#[test]
fn rejects_invalid_and_duplicate_host_headers_before_sending() {
    smol::block_on(async {
        let executor = executor();

        for value in [
            "requesteagleruntime/0.041",
            "",
            "https://example.com",
            "example.com/path",
            "example.com?query",
            "example.com#fragment",
            "user@example.com",
            "bad host",
            ":8080",
            "example.com:invalid",
            "example.com:65536",
            "::1",
            "[not-ipv6]",
            "[::1]suffix",
        ] {
            let error = executor
                .execute(HttpRequest {
                    path: "http://127.0.0.1:1".into(),
                    headers: vec![("hOsT".into(), value.into())],
                    ..HttpRequest::default()
                })
                .await
                .unwrap_err();

            println!("\n  Host: {value:?} -> {error}");
            assert!(matches!(
                error,
                ExecutionError::Http(HttpError::InvalidHost)
            ));
            assert!(error.to_string().contains("User-Agent"));
        }

        let error = executor
            .execute(HttpRequest {
                path: "http://127.0.0.1:1".into(),
                headers: vec![
                    ("Host".into(), "example.com".into()),
                    ("host".into(), "example.com".into()),
                ],
                ..HttpRequest::default()
            })
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            ExecutionError::Http(HttpError::MultipleHosts)
        ));
    });
}

#[test]
fn preserves_valid_host_overrides_and_user_agent() {
    smol::block_on(async {
        let executor = executor();

        for host in [
            "example.com",
            "example.com:8080",
            "example.com:",
            "localhost",
            "127.0.0.1:8080",
            "[::1]",
            "[2001:db8::1]:8080",
        ] {
            let (url, server) = serve(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
            )
            .await;
            let result = executor
                .execute(HttpRequest {
                    path: url,
                    headers: vec![
                        ("Host".into(), host.into()),
                        ("User-Agent".into(), "requesteagleruntime/0.041".into()),
                    ],
                    ..HttpRequest::default()
                })
                .await
                .unwrap();
            let received = server.await;

            assert!(received.head.contains(&format!("\r\nhost: {host}\r\n")));
            assert!(
                received
                    .head
                    .contains("\r\nuser-agent: requesteagleruntime/0.041\r\n")
            );
            let Response::Http(response) = result.response;
            assert_eq!(response.status.as_u16(), 200);
        }
    });
}

#[test]
fn generated_preview_matches_headers_received_by_the_server() {
    smol::block_on(async {
        let (url, server) =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec())
                .await;
        let path = url.replacen("http://", "http://user:p%40ss@", 1);
        let preview = request::generated_headers(Method::Post, &path, &[], 3);
        executor()
            .execute(HttpRequest {
                method: Method::Post,
                path,
                body: Some(b"abc".to_vec()),
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        let received = server.await;

        for (name, value) in preview {
            assert!(
                received
                    .head
                    .contains(&format!("\r\n{}: {value}\r\n", name.to_lowercase())),
                "{}",
                received.head
            );
        }
        assert_eq!(
            received
                .head
                .lines()
                .skip(1)
                .filter(|line| !line.is_empty())
                .count(),
            5
        );
        assert_eq!(received.body, b"abc");
    });
}

#[test]
fn reports_transport_and_truncated_body_errors() {
    smol::block_on(async {
        for response in [
            b"".as_slice(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nshort".as_slice(),
        ] {
            let (url, server) = serve(response.to_vec()).await;
            let error = executor()
                .execute(HttpRequest {
                    path: url,
                    ..HttpRequest::default()
                })
                .await
                .unwrap_err();
            server.await;
            println!("\n  Connection failure -> {error}");

            if response.is_empty() {
                assert!(matches!(
                    error,
                    ExecutionError::Http(HttpError::Transport(_))
                ));
            } else {
                assert!(matches!(
                    error,
                    ExecutionError::Http(HttpError::ReadBody(_))
                ));
            }
        }
    });
}

#[test]
fn enforces_size_limit_for_content_length_and_chunked_responses() {
    smol::block_on(async {
        let limit = 1024 * 1024;
        let executor = RequestExecutor::new(&RequestPreferences {
            max_response_size_mb: 1,
            timeout_ms: 2_000,
            ..RequestPreferences::default()
        })
        .unwrap();

        for (size, chunked) in [(limit, false), (limit + 1, false), (limit + 1, true)] {
            let head = if chunked {
                format!(
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{size:x}\r\n"
                )
            } else {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n")
            };
            let mut response = head.into_bytes();
            response.extend(vec![b'x'; size]);

            if chunked {
                response.extend_from_slice(b"\r\n0\r\n\r\n");
            }

            let (url, server) = serve(response).await;
            let result = executor
                .execute(HttpRequest {
                    path: url,
                    ..HttpRequest::default()
                })
                .await;
            server.await;

            if size == limit {
                let result = result.unwrap();
                report("Exactly 1 MiB: allowed", &result);
                let Response::Http(response) = result.response;
                assert_eq!(response.body.len(), limit);
            } else {
                let error = result.unwrap_err();
                println!("\n  {size} bytes, chunked={chunked} -> {error}");
                assert!(matches!(
                    error,
                    ExecutionError::ResponseTooLarge {
                        limit_bytes: 1_048_576
                    }
                ));
            }
        }
    });
}

#[test]
fn zero_size_and_timeout_preferences_disable_the_limits() {
    smol::block_on(async {
        let size = 1024 * 1024 + 1;
        let mut response =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n")
                .into_bytes();
        response.extend(vec![b'x'; size]);
        let (url, server) = serve(response).await;
        let executor = RequestExecutor::new(&RequestPreferences {
            timeout_ms: 0,
            max_response_size_mb: 0,
            ..RequestPreferences::default()
        })
        .unwrap();
        let result = executor
            .execute(HttpRequest {
                path: url,
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        server.await;
        report("Limits disabled", &result);

        let Response::Http(response) = result.response;
        assert_eq!(response.body.len(), size);
    });
}

#[test]
fn timeout_covers_waiting_for_headers_and_reading_the_body() {
    smol::block_on(async {
        for send_headers in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = smol::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_request(&mut stream).await;

                if send_headers {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n")
                        .await
                        .unwrap();
                }

                let mut byte = [0];
                stream.read(&mut byte).await.unwrap()
            });
            let executor = RequestExecutor::new(&RequestPreferences {
                timeout_ms: 150,
                ..RequestPreferences::default()
            })
            .unwrap();
            let error = executor
                .execute(HttpRequest {
                    path: url,
                    ..HttpRequest::default()
                })
                .await
                .unwrap_err();
            println!(
                "\n  Stalled {} -> {error}",
                if send_headers { "body" } else { "headers" }
            );
            assert!(
                matches!(error, ExecutionError::Timeout { timeout } if timeout == Duration::from_millis(150))
            );

            let closed = smol::future::or(async { Some(server.await) }, async {
                smol::Timer::after(Duration::from_secs(2)).await;
                None
            })
            .await;
            assert_eq!(closed, Some(0), "timeout must close the pending connection");
        }
    });
}

#[test]
fn dropping_an_in_flight_future_cancels_the_connection() {
    smol::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (accepted, waiting) = smol::channel::bounded(1);
        let server = smol::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            accepted.send(()).await.unwrap();
            let mut byte = [0];

            stream.read(&mut byte).await.unwrap()
        });
        let mut run = Box::pin(executor().execute(HttpRequest {
            path: url,
            ..HttpRequest::default()
        }));
        smol::future::or(
            async {
                let result = run.as_mut().await;
                panic!("request completed before cancellation: {result:?}");
            },
            async {
                waiting.recv().await.unwrap();
            },
        )
        .await;
        drop(run);

        let closed = smol::future::or(async { Some(server.await) }, async {
            smol::Timer::after(Duration::from_secs(2)).await;
            None
        })
        .await;
        assert_eq!(closed, Some(0));
        println!("\n  Dropped execution future -> server observed connection close");
    });
}

#[test]
fn rejects_response_limit_overflow() {
    let error = RequestExecutor::new(&RequestPreferences {
        max_response_size_mb: u64::MAX,
        ..RequestPreferences::default()
    })
    .err()
    .unwrap();
    assert!(matches!(error, ExecutionError::InvalidResponseLimit));
    println!("\n  Overflowing response limit -> {error}");
}

#[test]
fn preserves_serialized_request_and_preference_formats() {
    let request: Request =
        serde_json::from_str(r#"{"type":"http","method":"POST","path":"https://example.test"}"#)
            .unwrap();
    let encoded = serde_json::to_value(request).unwrap();
    assert_eq!(encoded["type"], "http");
    assert_eq!(encoded["method"], "POST");
    assert_eq!(encoded["headers"], serde_json::json!([]));

    let preferences: RequestPreferences =
        serde_json::from_str(r#"{"http_version":"http2","timeout_ms":250}"#).unwrap();
    assert_eq!(preferences.http_version, HttpVersion::Http2);
    assert_eq!(preferences.timeout_ms, 250);
    assert_eq!(preferences.max_response_size_mb, 50);
    assert!(preferences.ssl_certificate_verification);
    assert!(preferences.follow_all_redirects);

    let preferences: RequestPreferences =
        serde_json::from_str(r#"{"follow_all_redirects":false}"#).unwrap();
    assert!(!preferences.follow_all_redirects);
    assert_eq!(
        serde_json::to_value(&preferences).unwrap()["follow_all_redirects"],
        false
    );
}

#[test]
fn scripts_wrap_the_real_http_execution_and_keep_the_draft_unchanged() {
    smol::block_on(async {
        let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"success\":true}";
        let (url, server) = serve(response.to_vec()).await;
        let draft = HttpRequest {
            method: Method::Post,
            path: format!("{url}/{{{{resource}}}}"),
            body: Some(br#"{"name":"{{name}}"}"#.to_vec()),
            scripts: request::RequestScripts {
                pre_request: "pm.variables.set('resource', 'echo'); pm.variables.set('name', 'Eagle'); pm.request.headers.upsert({key: 'X-Script', value: 'ran'});".into(),
                post_response: "pm.test('status', () => pm.response.to.have.status(200)); pm.test('json', () => pm.expect(pm.response.json()).to.have.property('success', true)); pm.test('vars', () => pm.expect(pm.variables.get('name')).to.equal('Eagle'));".into(),
            },
            ..Default::default()
        };
        let execution = executor().execute(&draft).await.unwrap();
        let received = server.await;
        assert!(received.head.starts_with("POST /echo HTTP/1.1"));
        assert!(received.head.to_lowercase().contains("x-script: ran"));
        assert_eq!(received.body, br#"{"name":"Eagle"}"#);
        assert_eq!(execution.scripts.len(), 2);
        assert_eq!(execution.scripts[1].tests.len(), 3);
        assert!(
            execution.scripts[1]
                .tests
                .iter()
                .all(|test| test.error.is_none())
        );
        assert!(draft.path.ends_with("{{resource}}"));
        assert!(draft.headers.is_empty());
    });
}

#[test]
fn request_timeout_does_not_discard_a_response_during_its_post_response_script() {
    smol::block_on(async {
        let (url, server) =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec())
                .await;
        let executor = RequestExecutor::new(&RequestPreferences {
            timeout_ms: 250,
            ..Default::default()
        })
        .unwrap();
        let execution = executor.execute(HttpRequest {
            path: url,
            scripts: request::RequestScripts {
                post_response: "const start = Date.now(); while (Date.now() - start < 350) {} throw new Error('script failed after response');".into(),
                ..Default::default()
            },
            ..Default::default()
        }).await.unwrap();
        server.await;
        let Response::Http(response) = execution.response;
        assert_eq!(response.body, b"ok");
        assert!(
            execution.scripts[0]
                .error
                .as_ref()
                .unwrap()
                .contains("script failed after response")
        );
    });
}
