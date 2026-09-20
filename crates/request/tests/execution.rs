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
        assert_eq!(response.metrics.request_header_bytes, 24);
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
        assert_eq!(metrics.request_header_bytes, 0);
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

        for method in [Method::Get, Method::Post, Method::Put, Method::Delete] {
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
            assert_eq!(response.body, b"{\"ok\":true}");
            report(&format!("{} /health", method.as_str()), &result);
        }
    });
}

#[test]
fn redirects_and_http_errors_are_inspectable_responses() {
    smol::block_on(async {
        for status in ["302 Found", "404 Not Found", "500 Internal Server Error"] {
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
    assert!(!preferences.ssl_certificate_verification);
}
