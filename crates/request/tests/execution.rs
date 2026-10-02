use std::time::Duration;

use request::{
    EventStream, Execution, ExecutionError, HttpRequest, HttpSettings, HttpVersion, Method,
    Request, RequestExecutor, RequestPreferences, RequestVariables, Response, Version,
};
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use std::collections::HashMap;

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

fn no_variables() -> RequestVariables {
    RequestVariables::new(HashMap::new(), None)
}

#[test]
fn tells_whether_a_request_went_out_and_keeps_its_secrets_out_of_failures() {
    smol::block_on(async {
        let variables = RequestVariables::new(
            HashMap::from([("token".to_owned(), "s3cret".to_owned())]),
            None,
        );
        let (events, _updates, _stop) = EventStream::new();
        let dispatch = events.dispatch();
        let request = HttpRequest {
            path: "http://127.0.0.1:1/?token={{token}}".into(),
            ..Default::default()
        };

        let error = executor()
            .execute_streaming(request, variables, events)
            .await
            .unwrap_err();

        assert!(dispatch.started());
        assert!(error.to_string().contains("s3cret"), "{error}");
        assert!(!error.message_without_url().contains("s3cret"), "{error}");

        // An unknown variable stops the request before it goes out.
        let (events, _updates, _stop) = EventStream::new();
        let dispatch = events.dispatch();
        let request = HttpRequest {
            path: "http://127.0.0.1:1/{{missing}}".into(),
            ..Default::default()
        };

        executor()
            .execute_streaming(request, no_variables(), events)
            .await
            .unwrap_err();

        assert!(!dispatch.started());
    });
}

#[test]
fn generated_values_are_shared_by_scripts_and_wire_templates_for_one_send() {
    smol::block_on(async {
        let mut ids = std::collections::HashSet::new();
        for phase in ["post only", "both", "override"] {
            let (url, server) =
                serve(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec()).await;
            let mut pre = String::new();
            if phase != "post only" {
                pre.push_str(r#"
                        const id = pm.variables.replaceIn('{{$guid}}');
                        pm.expect(pm.variables.replaceIn('{{$guid}}/{{$guid}}')).to.equal(`${id}/${id}`);
                        pm.expect(pm.variables.has('$guid')).to.be.false;
                        pm.variables.set('$guid', 'temporary');
                        pm.expect(pm.variables.replaceIn('{{$guid}}')).to.equal('temporary');
                        pm.variables.unset('$guid');
                        pm.expect(pm.variables.replaceIn('{{$guid}}')).to.equal(id);
                        pm.variables.clear();
                        pm.expect(pm.variables.replaceIn('{{$guid}}')).to.equal(id);
                        pm.request.headers.upsert({key: 'X-Pre-Id', value: id});
                    "#);
            }
            if phase == "override" {
                pre.push_str("pm.variables.set('$guid', 'override');");
            }
            let request = HttpRequest {
                    method: Method::Post,
                    path: format!("{url}/{{{{$guid}}}}"),
                    headers: vec![
                        ("X-Id".into(), "{{$guid}}".into()),
                        ("X-Uuid".into(), "{{$randomUUID}}".into()),
                    ],
                    query: vec![("id".into(), "{{$guid}}".into())],
                    path_variables: Vec::new(),
                    body: Some(b"{{$guid}}/{{$guid}}".to_vec()),
                    scripts: request::RequestScripts {
                        pre_request: pre,
                        post_response: r#"
                            const sent = pm.request.headers.get('X-Id');
                            pm.expect(pm.variables.replaceIn('{{$guid}}')).to.equal(sent);
                            pm.expect(pm.variables.replaceIn('{{$randomUUID}}')).to.equal(pm.request.headers.get('X-Uuid'));
                            const original = pm.request.headers.get('X-Pre-Id') ?? sent;
                            pm.variables.unset('$guid');
                            pm.expect(pm.variables.replaceIn('{{$guid}}')).to.equal(original);
                            pm.variables.clear();
                            pm.expect(pm.variables.replaceIn('{{$guid}}')).to.equal(original);
                        "#.into(),
                    },
                    settings: HttpSettings::default(),
                };
            let execution = executor().execute(request, no_variables()).await.unwrap();
            for report in execution.scripts {
                assert!(report.error.is_none(), "{phase}: {report:?}");
            }
            let received = server.await;
            let id = String::from_utf8(received.body).unwrap();
            let (id, repeated) = id.split_once('/').unwrap();
            assert_eq!(id, repeated);
            assert!(
                received
                    .head
                    .starts_with(&format!("POST /{id}?id={id} HTTP/1.1\r\n"))
            );
            assert!(
                received
                    .head
                    .to_lowercase()
                    .contains(&format!("x-id: {id}\r\n"))
            );
            if phase == "override" {
                assert_eq!(id, "override");
            } else {
                uuid::Uuid::parse_str(id).unwrap();
                assert!(
                    ids.insert(id.to_owned()),
                    "a new send needs fresh generated values"
                );
            }
        }
    });
}

#[test]
fn post_response_scripts_share_uploads_and_read_the_sent_body() {
    smol::block_on(async {
        for (body, pre, post) in [
            (
                vec![b'x'; 40 * 1024 * 1024],
                "",
                "pm.response.to.have.status(200);",
            ),
            (
                vec![255; 40 * 1024 * 1024],
                "",
                "pm.response.to.have.status(200);",
            ),
            (
                b"draft".to_vec(),
                "pm.request.body.update('sent 🦅');",
                "pm.expect(pm.request.body.raw).to.equal('sent 🦅');",
            ),
        ] {
            let (url, server) =
                serve(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec()).await;
            let expected_len = if pre.contains("body.update") {
                "sent 🦅".len()
            } else {
                body.len()
            };
            let execution = executor()
                .execute(
                    HttpRequest {
                        method: Method::Post,
                        path: url,
                        body: Some(body),
                        scripts: request::RequestScripts {
                            pre_request: pre.into(),
                            post_response: post.into(),
                        },
                        ..Default::default()
                    },
                    no_variables(),
                )
                .await
                .unwrap();
            assert!(
                execution
                    .scripts
                    .iter()
                    .all(|report| report.error.is_none()),
                "{:?}",
                execution.scripts
            );
            let received = server.await;
            assert_eq!(received.body.len(), expected_len);
            let Response::Http(response) = execution.response;
            assert_eq!(response.metrics.request_body_bytes, expected_len);
            if pre.contains("body.update") {
                assert_eq!(received.body, "sent 🦅".as_bytes());
            } else {
                assert!(received.body.iter().all(|byte| *byte == received.body[0]));
            }
        }
    });
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
        let request = HttpRequest {
            method: Method::Post,
            path: format!("{url}/submit?tag=existing#ignored"),
            headers: vec![
                ("X-Tag".into(), "one".into()),
                ("X-Tag".into(), "two".into()),
            ],
            body: Some(vec![0, 255, 42]),
            scripts: Default::default(),
            query: vec![("tag".into(), "a & b".into()), ("tag".into(), "c+d".into())],
            path_variables: Vec::new(),
            settings: HttpSettings::default(),
        };
        let result = executor().execute(request, no_variables()).await.unwrap();
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
fn fills_path_variables_and_sends_query_params_moved_into_the_url_unchanged() {
    smol::block_on(async {
        let response = b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";
        let (url, server) = serve(response.to_vec()).await;
        let mut request = HttpRequest {
            path: format!("{url}/pets/:id/toys/:toy?tag=existing#ignored"),
            path_variables: vec![("id".into(), "{{pet}}".into())],
            query: vec![
                ("tag".into(), "a & b".into()),
                ("tag".into(), "c+d {{tag}}".into()),
            ],
            ..Default::default()
        };
        request.inline_query();
        assert!(request.query.is_empty());

        let variables = RequestVariables::new(
            HashMap::from([("pet".into(), "7".into()), ("tag".into(), "e".into())]),
            None,
        );
        executor().execute(request, variables).await.unwrap();

        // A path variable without a value is sent as written.
        let received = server.await;
        assert!(
            received.head.starts_with(
                "GET /pets/7/toys/:toy?tag=existing&tag=a+%26+b&tag=c%2Bd+e HTTP/1.1\r\n"
            ),
            "{}",
            received.head
        );
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
            .execute(
                HttpRequest {
                    path: url,
                    ..HttpRequest::default()
                },
                no_variables(),
            )
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
fn executes_all_existing_methods() {
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
            let request = HttpRequest {
                method,
                path: format!("{url}/health"),
                ..HttpRequest::default()
            };
            let result = executor.execute(request, no_variables()).await.unwrap();
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
                .execute(
                    HttpRequest {
                        path: url,
                        ..HttpRequest::default()
                    },
                    no_variables(),
                )
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
                    .execute(
                        HttpRequest {
                            method: Method::Post,
                            path: url,
                            body: Some(b"payload".to_vec()),
                            ..HttpRequest::default()
                        },
                        no_variables(),
                    )
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
fn request_settings_override_the_redirect_and_timeout_preferences() {
    smol::block_on(async {
        // Following is on in preferences and off for this request.
        let (url, server) = serve(
            b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        )
        .await;
        let execution = executor()
            .execute(
                HttpRequest {
                    path: url,
                    settings: HttpSettings {
                        follow_redirects: Some(false),
                        ..HttpSettings::default()
                    },
                    ..HttpRequest::default()
                },
                no_variables(),
            )
            .await
            .unwrap();
        server.await;
        let Response::Http(response) = execution.response;
        assert_eq!(response.status.as_u16(), 302);

        // Following is off in preferences and on for this request.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/start", listener.local_addr().unwrap());
        let server = smol::spawn(async move {
            for response in [
                &b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"[..],
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone",
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_request(&mut stream).await;
                stream.write_all(response).await.unwrap();
            }
        });
        let execution = RequestExecutor::new(&RequestPreferences {
            timeout_ms: 2_000,
            follow_all_redirects: false,
            ..RequestPreferences::default()
        })
        .unwrap()
        .execute(
            HttpRequest {
                path: url,
                settings: HttpSettings {
                    follow_redirects: Some(true),
                    ..HttpSettings::default()
                },
                ..HttpRequest::default()
            },
            no_variables(),
        )
        .await
        .unwrap();
        server.await;
        let Response::Http(response) = execution.response;
        assert_eq!(response.body, b"done");

        // A request's timeout replaces the preference, and zero turns it off.
        for (preference, setting) in [(0, Some(150)), (150, Some(0))] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = smol::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_request(&mut stream).await;
                smol::Timer::after(Duration::from_millis(400)).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nslow")
                    .await;
            });
            let result = RequestExecutor::new(&RequestPreferences {
                timeout_ms: preference,
                ..RequestPreferences::default()
            })
            .unwrap()
            .execute(
                HttpRequest {
                    path: url,
                    settings: HttpSettings {
                        timeout_ms: setting,
                        ..HttpSettings::default()
                    },
                    ..HttpRequest::default()
                },
                no_variables(),
            )
            .await;

            if setting == Some(0) {
                let Response::Http(response) = result.unwrap().response;
                assert_eq!(response.body, b"slow");
                server.await;
            } else {
                assert!(matches!(
                    result.unwrap_err(),
                    ExecutionError::Timeout { timeout } if timeout == Duration::from_millis(150)
                ));
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
            .execute(
                HttpRequest {
                    path: format!("{url}/0"),
                    ..HttpRequest::default()
                },
                no_variables(),
            )
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
                .execute(
                    HttpRequest {
                        path: url,
                        headers: vec![
                            ("Authorization".into(), "Bearer test-token".into()),
                            ("Cookie".into(), "session=test-session".into()),
                        ],
                        ..HttpRequest::default()
                    },
                    no_variables(),
                )
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
                    .execute(
                        HttpRequest {
                            method: Method::Post,
                            path: url,
                            headers: vec![
                                ("Host".into(), explicit_host),
                                ("Authorization".into(), "Bearer test-token".into()),
                                ("Cookie".into(), "session=test-session".into()),
                            ],
                            body: Some(b"payload".to_vec()),
                            ..HttpRequest::default()
                        },
                        no_variables(),
                    )
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
            .execute(
                HttpRequest {
                    path: url,
                    headers: vec![("Host".into(), "virtual.example".into())],
                    ..HttpRequest::default()
                },
                no_variables(),
            )
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
                .execute(
                    HttpRequest {
                        path: url,
                        headers: vec![("Host".into(), "virtual.example".into())],
                        ..HttpRequest::default()
                    },
                    no_variables(),
                )
                .await
                .unwrap_err();

            assert!(matches!(&error, ExecutionError::Transport(_)));
            assert!(error.to_string().contains("too many redirects"), "{error}");
            server.await;
        }
    });
}

#[test]
fn rejects_invalid_urls_schemes_and_headers_before_sending() {
    smol::block_on(async {
        for path in ["", "ws://localhost/socket", "file:///etc/hosts"] {
            let error = executor()
                .execute(
                    HttpRequest {
                        path: path.into(),
                        ..HttpRequest::default()
                    },
                    no_variables(),
                )
                .await
                .unwrap_err();
            println!("\n  {path:?} -> {error}");
            assert!(matches!(
                error,
                ExecutionError::InvalidUrl(_) | ExecutionError::UnsupportedScheme(_)
            ));
        }

        for header in [("bad name", "value"), ("X-Test", "value\r\ninjected: true")] {
            let error = executor()
                .execute(
                    HttpRequest {
                        path: "http://127.0.0.1:1".into(),
                        headers: vec![(header.0.into(), header.1.into())],
                        ..HttpRequest::default()
                    },
                    no_variables(),
                )
                .await
                .unwrap_err();
            println!("\n  Invalid header -> {error}");
            assert!(matches!(error, ExecutionError::InvalidRequest(_)));
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
                .execute(
                    HttpRequest {
                        path: "http://127.0.0.1:1".into(),
                        headers: vec![("hOsT".into(), value.into())],
                        ..HttpRequest::default()
                    },
                    no_variables(),
                )
                .await
                .unwrap_err();

            println!("\n  Host: {value:?} -> {error}");
            assert!(matches!(error, ExecutionError::InvalidHost));
            assert!(error.to_string().contains("User-Agent"));
        }

        let error = executor
            .execute(
                HttpRequest {
                    path: "http://127.0.0.1:1".into(),
                    headers: vec![
                        ("Host".into(), "example.com".into()),
                        ("host".into(), "example.com".into()),
                    ],
                    ..HttpRequest::default()
                },
                no_variables(),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ExecutionError::MultipleHosts));
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
                .execute(
                    HttpRequest {
                        path: url,
                        headers: vec![
                            ("Host".into(), host.into()),
                            ("User-Agent".into(), "requesteagleruntime/0.041".into()),
                        ],
                        ..HttpRequest::default()
                    },
                    no_variables(),
                )
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
        let headers = vec![("Content-Type".into(), "text/plain".into())];
        let preview = request::generated_headers(Method::Post, &path, &headers, 3);
        executor()
            .execute(
                HttpRequest {
                    method: Method::Post,
                    path,
                    headers,
                    body: Some(b"abc".to_vec()),
                    ..HttpRequest::default()
                },
                no_variables(),
            )
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
            6
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
                .execute(
                    HttpRequest {
                        path: url,
                        ..HttpRequest::default()
                    },
                    no_variables(),
                )
                .await
                .unwrap_err();
            server.await;
            println!("\n  Connection failure -> {error}");

            if response.is_empty() {
                assert!(matches!(error, ExecutionError::Transport(_)));
            } else {
                assert!(matches!(error, ExecutionError::ReadBody(_)));
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
                .execute(
                    HttpRequest {
                        path: url,
                        ..HttpRequest::default()
                    },
                    no_variables(),
                )
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
            .execute(
                HttpRequest {
                    path: url,
                    ..HttpRequest::default()
                },
                no_variables(),
            )
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
        for (send_headers, scripted) in [(false, false), (true, false), (false, true), (true, true)]
        {
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
                    scripts: request::RequestScripts {
                        pre_request: if scripted {
                            "console.log('prepared'); pm.test('pass', () => {}); pm.test('fail', () => pm.expect(1).to.equal(2));".into()
                        } else {
                            String::new()
                        },
                        ..Default::default()
                    },
                    ..HttpRequest::default()
                }, no_variables())
                .await
                .unwrap_err();
            println!(
                "\n  Stalled {} -> {error}",
                if send_headers { "body" } else { "headers" }
            );
            let error = if scripted {
                let ExecutionError::ScriptedRequest { source, reports } = error else {
                    panic!("timeout discarded pre-request diagnostics");
                };

                assert_eq!(reports.len(), 1);
                assert_eq!(reports[0].phase, request::ScriptPhase::PreRequest);
                assert_eq!(reports[0].logs.len(), 1);
                assert_eq!(reports[0].logs[0].message, "prepared");
                assert_eq!(reports[0].tests.len(), 2);
                assert_eq!(reports[0].tests[0].name, "pass");
                assert!(reports[0].tests[0].error.is_none());
                assert_eq!(reports[0].tests[1].name, "fail");
                assert!(reports[0].tests[1].error.is_some());
                assert!(reports[0].error.is_none());
                *source
            } else {
                error
            };

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
        let mut run = Box::pin(executor().execute(
            HttpRequest {
                path: url,
                ..HttpRequest::default()
            },
            no_variables(),
        ));
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

    // Settings are saved only when a request changes them.
    assert!(encoded.get("settings").is_none());
    let request: Request = serde_json::from_str(
        r#"{"type":"http","method":"GET","path":"https://example.test","settings":{"timeout_ms":0,"verify_certificates":false}}"#,
    )
    .unwrap();
    let Request::Http(http) = &request else {
        panic!("expected an HTTP request");
    };
    assert_eq!(
        http.settings,
        HttpSettings {
            timeout_ms: Some(0),
            follow_redirects: None,
            verify_certificates: Some(false),
        }
    );
    assert_eq!(
        serde_json::to_value(&request).unwrap()["settings"],
        serde_json::json!({"timeout_ms": 0, "verify_certificates": false})
    );

    let preferences: RequestPreferences =
        serde_json::from_str(r#"{"follow_all_redirects":false}"#).unwrap();
    assert!(!preferences.follow_all_redirects);
    assert_eq!(
        serde_json::to_value(&preferences).unwrap()["follow_all_redirects"],
        false
    );
}

#[test]
fn scripts_wrap_the_real_http_execution() {
    smol::block_on(async {
        let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"success\":true}";
        let (url, server) = serve(response.to_vec()).await;
        let request = HttpRequest {
            method: Method::Post,
            path: format!("{url}/{{{{resource}}}}"),
            body: Some(br#"{"name":"{{name}}"}"#.to_vec()),
            scripts: request::RequestScripts {
                pre_request: "pm.variables.set('resource', 'echo'); pm.variables.set('name', 'Eagle'); pm.request.headers.upsert({key: 'X-Script', value: 'ran'});".into(),
                post_response: "pm.test('status', () => pm.response.to.have.status(200)); pm.test('json', () => pm.expect(pm.response.json()).to.have.property('success', true)); pm.test('vars', () => pm.expect(pm.variables.get('name')).to.equal('Eagle'));".into(),
            },
            ..Default::default()
        };
        let execution = executor().execute(request, no_variables()).await.unwrap();
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
    });
}

#[test]
fn scripts_filter_bodies_and_logging_failures_do_not_block_http() {
    smol::block_on(async {
        for method in ["GET", "HEAD"] {
            let (url, server) = serve(
                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            )
            .await;
            let mut request = HttpRequest {
                method: Method::Post,
                path: url,
                body: Some(b"{{unclosed".to_vec()),
                ..Default::default()
            };
            request.scripts.pre_request = format!(
                r#"
                const cyclic = {{}}; cyclic.self = cyclic;
                console.log(1n);
                console.log(cyclic);
                console.log({{toJSON() {{ throw Error('json'); }}, toString() {{ throw Error('string'); }} }});
                console.log({{toJSON() {{ throw Error('json'); }}, toString() {{ return 'x'.repeat(10000); }} }});
                pm.variables.set('flag', pm.variables.replaceIn('{{{{$randomBoolean}}}}'));
                pm.request.headers.upsert({{key: 'X-Flag', value: '{{{{flag}}}}'}});
                pm.request.method = '{method}';
            "#
            );
            let execution = executor().execute(request, no_variables()).await.unwrap();
            let received = server.await;
            assert!(
                received
                    .head
                    .starts_with(&format!("{method} / HTTP/1.1\r\n"))
            );
            assert!(received.body.is_empty());
            assert!(
                received.head.contains("x-flag: true\r\n")
                    || received.head.contains("x-flag: false\r\n")
            );
            let logs = &execution.scripts[0].logs;
            assert_eq!(logs.len(), 4);
            assert_eq!(logs[0].message, "1");
            assert_eq!(logs[1].message, "[object Object]");
            assert_eq!(logs[2].message, "[Unserializable value]");
            assert_eq!(logs[3].message.len(), 4096);
        }
    });
}

#[test]
fn scripts_see_and_edit_query_rows() {
    smol::block_on(async {
        for mode in ["read", "edit", "clear", "replace", "post"] {
            let (url, server) = serve(
                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            )
            .await;
            let initial = format!(
                "{url}/path?tag=existing%20item&tag=a+%26+b&tag=c%2Bd&Case=keep&remove=yes"
            );
            let mutation = match mode {
                "read" | "post" => "",
                "clear" => "pm.request.url.query.clear();",
                "edit" => {
                    r#"
                    pm.request.url.query.remove('tag');
                    pm.request.url.query.remove('case');
                    pm.expect(pm.request.url.query.has('Case')).to.be.true;
                    pm.request.url.query.remove('remove');
                    pm.variables.set('value', '🦅 & +');
                    pm.request.url.query.add({key: 'x', value: 'old'});
                    pm.request.url.query.upsert({key: 'x', value: '{{value}}'});
                "#
                }
                _ => {
                    "pm.request.url = pm.request.url.toString().split('?')[0] + '?fresh=yes'; pm.expect(pm.request.url.query.toJSON()).to.deep.equal([{key: 'fresh', value: 'yes'}]);"
                }
            };
            let target = match mode {
                "read" | "post" => {
                    "/path?tag=existing%20item&tag=a+%26+b&tag=c%2Bd&Case=keep&remove=yes"
                }
                "clear" => "/path",
                "edit" => "/path?Case=keep&x=%F0%9F%A6%85+%26+%2B",
                _ => "/path?fresh=yes",
            };
            let request = HttpRequest {
                path: format!("{url}/path?tag=existing%20item#ignored"),
                query: vec![
                    ("tag".into(), "a & b".into()),
                    ("tag".into(), "c+d".into()),
                    ("Case".into(), "keep".into()),
                    ("remove".into(), "yes".into()),
                ],
                scripts: request::RequestScripts {
                    pre_request: format!(
                        "pm.expect(String(pm.request.url)).to.equal({initial:?}); pm.expect(pm.request.url.query.get('tag')).to.equal('existing item'); {mutation}"
                    ),
                    post_response: format!(
                        "pm.test('sent URL', () => pm.expect(pm.request.url.toString()).to.equal({:?}));",
                        format!("{url}{target}")
                    ),
                },
                ..Default::default()
            };
            let execution = executor().execute(request, no_variables()).await.unwrap();
            let received = server.await;
            assert!(
                received
                    .head
                    .starts_with(&format!("GET {target} HTTP/1.1\r\n")),
                "{}",
                received.head
            );
            assert!(
                execution.scripts.last().unwrap().tests[0].error.is_none(),
                "{:?}",
                execution.scripts.last().unwrap().tests[0]
            );
        }
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
        }, no_variables()).await.unwrap();
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
