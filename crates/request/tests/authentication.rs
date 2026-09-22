use request::{
    ApiKeyLocation, Authentication, ExecutionError, HttpError, HttpRequest, HttpVersion, Method,
    Request, RequestExecutor, RequestPreferences,
};
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

async fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();

    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
        assert!(head.len() < 16 * 1024);
    }

    String::from_utf8(head).unwrap()
}

async fn serve(response: String) -> (String, smol::Task<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let head = read_head(&mut stream).await;
        stream.write_all(response.as_bytes()).await.unwrap();

        head
    });

    (url, server)
}

fn executor(http_version: HttpVersion) -> RequestExecutor {
    RequestExecutor::new(&RequestPreferences {
        http_version,
        timeout_ms: 2_000,
        ..RequestPreferences::default()
    })
    .unwrap()
}

fn header_values<'a>(head: &'a str, name: &str) -> Vec<&'a str> {
    head.lines()
        .skip(1)
        .filter_map(|line| {
            let (header, value) = line.split_once(':')?;

            header.eq_ignore_ascii_case(name).then(|| value.trim())
        })
        .collect()
}

#[test]
fn sends_basic_bearer_and_api_key_headers() {
    smol::block_on(async {
        for (authentication, name, value) in [
            (
                Authentication::Basic {
                    username: "Aladdin".into(),
                    password: "open sesame".into(),
                },
                "authorization",
                "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==",
            ),
            (
                Authentication::Bearer {
                    token: "test-token".into(),
                },
                "authorization",
                "Bearer test-token",
            ),
            (
                Authentication::ApiKey {
                    name: "X-Api-Key".into(),
                    value: "test-key".into(),
                    location: ApiKeyLocation::Header,
                },
                "x-api-key",
                "test-key",
            ),
        ] {
            let (url, server) = serve(OK.into()).await;
            executor(HttpVersion::Http1_1)
                .execute(HttpRequest {
                    path: format!("{url}/protected"),
                    authentication,
                    ..HttpRequest::default()
                })
                .await
                .unwrap();
            let received = server.await;

            assert_eq!(header_values(&received, name), vec![value]);
        }
    });
}

#[test]
fn bearer_authentication_and_explicit_headers_override_url_credentials() {
    smol::block_on(async {
        for http_version in [HttpVersion::Auto, HttpVersion::Http1_1] {
            for explicit_header in [false, true] {
                let (url, server) = serve(OK.into()).await;
                let path = url.replacen("http://", "http://user:password@", 1);
                let headers = if explicit_header {
                    vec![("aUtHoRiZaTiOn".into(), "Bearer explicit-token".into())]
                } else {
                    Vec::new()
                };

                executor(http_version)
                    .execute(HttpRequest {
                        path,
                        headers,
                        authentication: Authentication::Bearer {
                            token: "selected-token".into(),
                        },
                        ..HttpRequest::default()
                    })
                    .await
                    .unwrap();
                let received = server.await;
                let expected = if explicit_header {
                    "Bearer explicit-token"
                } else {
                    "Bearer selected-token"
                };

                assert_eq!(header_values(&received, "authorization"), vec![expected]);
            }
        }
    });
}

#[test]
fn explicit_headers_override_authentication_case_insensitively() {
    smol::block_on(async {
        for (authentication, name) in [
            (
                Authentication::Basic {
                    username: "generated-user".into(),
                    password: "generated-password".into(),
                },
                "aUtHoRiZaTiOn",
            ),
            (
                Authentication::Bearer {
                    token: "generated-token".into(),
                },
                "aUtHoRiZaTiOn",
            ),
            (
                Authentication::ApiKey {
                    name: "X-Api-Key".into(),
                    value: "generated-key".into(),
                    location: ApiKeyLocation::Header,
                },
                "x-aPi-kEy",
            ),
        ] {
            let (url, server) = serve(OK.into()).await;
            executor(HttpVersion::Http1_1)
                .execute(HttpRequest {
                    path: url,
                    headers: vec![(name.into(), "explicit-value".into())],
                    authentication,
                    ..HttpRequest::default()
                })
                .await
                .unwrap();
            let received = server.await;

            assert_eq!(header_values(&received, name), vec!["explicit-value"]);
        }
    });
}

#[test]
fn rejects_basic_usernames_with_colons_without_exposing_credentials() {
    smol::block_on(async {
        let error = executor(HttpVersion::Http1_1)
            .execute(HttpRequest {
                path: "http://127.0.0.1:1".into(),
                authentication: Authentication::Basic {
                    username: "private:user".into(),
                    password: "private-password".into(),
                },
                ..HttpRequest::default()
            })
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            ExecutionError::Http(HttpError::InvalidAuthentication(_))
        ));
        assert!(error.to_string().contains("colon"));

        for output in [error.to_string(), format!("{error:?}")] {
            assert!(!output.contains("private:user"));
            assert!(!output.contains("private-password"));
        }
    });
}

#[test]
fn rejects_blank_bearer_tokens_before_sending() {
    smol::block_on(async {
        for token in ["", " \t "] {
            let error = executor(HttpVersion::Http1_1)
                .execute(HttpRequest {
                    path: "http://127.0.0.1:1".into(),
                    authentication: Authentication::Bearer {
                        token: token.into(),
                    },
                    ..HttpRequest::default()
                })
                .await
                .unwrap_err();

            assert!(matches!(
                error,
                ExecutionError::Http(HttpError::InvalidAuthentication(_))
            ));
            assert!(error.to_string().contains("Bearer token"));
        }
    });
}

#[test]
fn encodes_api_key_query_names_and_values() {
    smol::block_on(async {
        let (url, server) = serve(OK.into()).await;
        executor(HttpVersion::Http1_1)
            .execute(HttpRequest {
                path: format!("{url}/protected?existing=1"),
                authentication: Authentication::ApiKey {
                    name: "api key".into(),
                    value: "a & b+=?/雪".into(),
                    location: ApiKeyLocation::Query,
                },
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        let received = server.await;

        assert_eq!(
            received.lines().next().unwrap(),
            "GET /protected?existing=1&api+key=a+%26+b%2B%3D%3F%2F%E9%9B%AA HTTP/1.1"
        );
        assert!(header_values(&received, "api key").is_empty());
    });
}

#[test]
fn existing_url_and_editor_query_parameters_override_api_keys() {
    smol::block_on(async {
        for from_editor in [false, true] {
            let (url, server) = serve(OK.into()).await;
            let (path, query, expected) = if from_editor {
                (
                    format!("{url}/protected"),
                    Some(vec![("api_key".into(), "from-editor".into())]),
                    "GET /protected?api_key=from-editor HTTP/1.1",
                )
            } else {
                (
                    format!("{url}/protected?api%5Fkey=from-url"),
                    None,
                    "GET /protected?api%5Fkey=from-url HTTP/1.1",
                )
            };

            executor(HttpVersion::Http1_1)
                .execute(HttpRequest {
                    path,
                    query,
                    authentication: Authentication::ApiKey {
                        name: "api_key".into(),
                        value: "generated-key".into(),
                        location: ApiKeyLocation::Query,
                    },
                    ..HttpRequest::default()
                })
                .await
                .unwrap();
            let received = server.await;

            assert_eq!(received.lines().next().unwrap(), expected);
            assert!(!received.contains("generated-key"));
        }
    });
}

#[test]
fn api_key_headers_follow_same_origin_redirects_but_stop_at_other_origins() {
    smol::block_on(async {
        for http_version in [HttpVersion::Auto, HttpVersion::Http1_1] {
            let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", origin.local_addr().unwrap());
            let (destination, destination_server) = serve(OK.into()).await;
            let origin_server = smol::spawn(async move {
                let mut requests = Vec::new();

                for location in ["/same".to_owned(), format!("{destination}/final")] {
                    let (mut stream, _) = origin.accept().await.unwrap();
                    requests.push(read_head(&mut stream).await);
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

                requests
            });

            executor(http_version)
                .execute(HttpRequest {
                    path: format!("{url}/start"),
                    authentication: Authentication::ApiKey {
                        name: "X-Api-Key".into(),
                        value: "private-key".into(),
                        location: ApiKeyLocation::Header,
                    },
                    ..HttpRequest::default()
                })
                .await
                .unwrap();

            for received in origin_server.await {
                assert_eq!(header_values(&received, "x-api-key"), vec!["private-key"]);
            }

            let received = destination_server.await;
            assert!(received.starts_with("GET /final HTTP/1.1\r\n"));
            assert!(header_values(&received, "x-api-key").is_empty());
        }
    });
}

#[test]
fn redirected_referers_exclude_api_key_query_values() {
    smol::block_on(async {
        for http_version in [HttpVersion::Auto, HttpVersion::Http1_1] {
            let (destination, destination_server) = serve(OK.into()).await;
            let (url, server) = serve(format!(
                "HTTP/1.1 302 Found\r\nLocation: {destination}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ))
            .await;

            executor(http_version)
                .execute(HttpRequest {
                    path: format!("{url}/start?visible=1"),
                    authentication: Authentication::ApiKey {
                        name: "api_key".into(),
                        value: "private-key".into(),
                        location: ApiKeyLocation::Query,
                    },
                    ..HttpRequest::default()
                })
                .await
                .unwrap();
            let initial = server.await;
            let redirected = destination_server.await;

            assert!(initial.starts_with("GET /start?visible=1&api_key=private-key HTTP/1.1\r\n"));
            assert!(redirected.starts_with("GET /final HTTP/1.1\r\n"));
            assert!(!redirected.contains("private-key"));

            for referer in header_values(&redirected, "referer") {
                let referer = url::Url::parse(referer).unwrap();
                assert!(!referer.query_pairs().any(|(name, _)| name == "api_key"));
            }
        }
    });
}

#[test]
fn saves_and_restores_every_authentication_type() {
    for authentication in [
        Authentication::None,
        Authentication::Basic {
            username: "saved-user".into(),
            password: "saved-password".into(),
        },
        Authentication::Bearer {
            token: "saved-token".into(),
        },
        Authentication::ApiKey {
            name: "X-Api-Key".into(),
            value: "saved-header-key".into(),
            location: ApiKeyLocation::Header,
        },
        Authentication::ApiKey {
            name: "api_key".into(),
            value: "saved-query-key".into(),
            location: ApiKeyLocation::Query,
        },
    ] {
        let expected = serde_json::to_value(&authentication).unwrap();
        let request = Request::Http(HttpRequest {
            method: Method::Post,
            path: "https://example.test/protected".into(),
            authentication,
            ..HttpRequest::default()
        });
        let encoded = serde_json::to_string(&request).unwrap();
        let Request::Http(restored) = serde_json::from_str(&encoded).unwrap();

        assert_eq!(
            serde_json::to_value(&restored.authentication).unwrap(),
            expected
        );
        assert_eq!(restored.method, Method::Post);
        assert_eq!(restored.path, "https://example.test/protected");
    }
}

#[test]
fn old_saved_requests_and_new_drafts_default_to_no_authentication() {
    let Request::Http(request) = serde_json::from_str::<Request>(
        r#"{"type":"http","method":"GET","path":"https://example.test"}"#,
    )
    .unwrap();

    assert!(matches!(request.authentication, Authentication::None));
    assert!(matches!(
        HttpRequest::default().authentication,
        Authentication::None
    ));
    assert!(matches!(ApiKeyLocation::default(), ApiKeyLocation::Header));
}

#[test]
fn debug_output_redacts_authentication_credentials() {
    for (authentication, credentials) in [
        (
            Authentication::Basic {
                username: "private-user".into(),
                password: "private-password".into(),
            },
            vec!["private-user", "private-password"],
        ),
        (
            Authentication::Bearer {
                token: "private-token".into(),
            },
            vec!["private-token"],
        ),
        (
            Authentication::ApiKey {
                name: "X-Api-Key".into(),
                value: "private-key".into(),
                location: ApiKeyLocation::Header,
            },
            vec!["private-key"],
        ),
    ] {
        let auth_debug = format!("{authentication:?}");
        let request_debug = format!(
            "{:?}",
            HttpRequest {
                authentication,
                ..HttpRequest::default()
            }
        );

        for credential in credentials {
            assert!(!auth_debug.contains(credential));
            assert!(!request_debug.contains(credential));
        }
    }
}
