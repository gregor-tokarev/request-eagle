use std::{
    fs,
    io::ErrorKind,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use request::{
    ApiKeyLocation, Authentication, ExecutionError, FormBody, HttpError, HttpRequest, HttpVersion,
    Method, MultipartField, Request, RequestExecutor, RequestPreferences, Response,
};
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

struct ReceivedRequest {
    head: String,
    body: Vec<u8>,
}

impl ReceivedRequest {
    fn headers(&self, name: &str) -> Vec<&str> {
        self.head
            .lines()
            .filter_map(|line| {
                let (header, value) = line.split_once(':')?;

                header.eq_ignore_ascii_case(name).then_some(value.trim())
            })
            .collect()
    }
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

async fn serve() -> (String, smol::Task<ReceivedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let received = read_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();

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

struct UploadFile {
    directory: PathBuf,
    path: PathBuf,
}

impl UploadFile {
    fn new(name: &str, bytes: &[u8]) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        let directory = loop {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "request-eagle-upload-test-{}-{id}",
                std::process::id()
            ));

            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create upload fixture: {error}"),
            }
        };
        let path = directory.join(name);
        let upload = Self { directory, path };
        fs::write(&upload.path, bytes).unwrap();

        upload
    }
}

impl Drop for UploadFile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn sends_urlencoded_duplicate_unicode_and_empty_fields() {
    smol::block_on(async {
        let (url, server) = serve().await;
        let form = FormBody::UrlEncoded(vec![
            ("tag".into(), "a b".into()),
            ("tag".into(), "c+d&=".into()),
            ("city".into(), "東京".into()),
            ("empty".into(), String::new()),
            (String::new(), "blank name".into()),
        ]);
        let expected = b"tag=a+b&tag=c%2Bd%26%3D&city=%E6%9D%B1%E4%BA%AC&empty=&=blank+name";
        assert_eq!(form.content_type(), "application/x-www-form-urlencoded");
        assert_eq!(form.encoded_len(), Some(expected.len()));

        let execution = executor()
            .execute(HttpRequest {
                method: Method::Post,
                path: format!("{url}/form"),
                body: Some(b"stale raw body".to_vec()),
                form: Some(form),
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        let received = server.await;

        assert!(received.head.starts_with("POST /form HTTP/1.1\r\n"));
        assert_eq!(received.body, expected);
        assert_eq!(
            received.headers("content-type"),
            vec!["application/x-www-form-urlencoded"]
        );

        let Response::Http(response) = execution.response;
        assert_eq!(response.metrics.request_body_bytes, expected.len());
    });
}

#[test]
fn sends_multipart_text_and_binary_file_with_matching_boundary() {
    smol::block_on(async {
        let bytes = b"\x00\xff\x01\r\nraw bytes";
        let upload = UploadFile::new("payload.bin", bytes);
        let form = FormBody::Multipart(vec![
            MultipartField::Text {
                name: "description".into(),
                value: "東京\r\nrequest-eagle-boundary-0\r\nsecond line".into(),
            },
            MultipartField::File {
                name: "attachment".into(),
                path: upload.path.clone(),
            },
        ]);
        assert_eq!(form.content_type(), "multipart/form-data");
        assert_eq!(form.encoded_len(), None);

        let (url, server) = serve().await;
        let execution = executor()
            .execute(HttpRequest {
                method: Method::Post,
                path: url,
                form: Some(form),
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        let received = server.await;
        let content_types = received.headers("content-type");
        assert_eq!(content_types.len(), 1);

        let boundary = content_types[0]
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        assert!(!boundary.is_empty());
        assert_ne!(boundary, "request-eagle-boundary-0");

        let mut expected = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"description\"\r\n\r\n東京\r\nrequest-eagle-boundary-0\r\nsecond line\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"payload.bin\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .into_bytes();
        expected.extend_from_slice(bytes);
        expected.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        assert_eq!(received.body, expected);
        assert_eq!(
            received.headers("content-length"),
            vec![expected.len().to_string()]
        );

        let Response::Http(response) = execution.response;
        assert_eq!(response.metrics.request_body_bytes, expected.len());
    });
}

#[test]
fn multipart_infers_file_content_types_and_preserves_binary_bytes() {
    smol::block_on(async {
        let bytes = b"\x00\xff\r\nfile contents";

        for (filename, content_type) in [
            ("notes.txt", "text/plain"),
            ("image.png", "image/png"),
            (
                "data.unknown-request-eagle-extension",
                "application/octet-stream",
            ),
        ] {
            let upload = UploadFile::new(filename, bytes);
            let (url, server) = serve().await;
            executor()
                .execute(HttpRequest {
                    method: Method::Post,
                    path: url,
                    form: Some(FormBody::Multipart(vec![MultipartField::File {
                        name: "attachment".into(),
                        path: upload.path.clone(),
                    }])),
                    ..HttpRequest::default()
                })
                .await
                .unwrap();
            let received = server.await;
            let content_types = received.headers("content-type");
            assert_eq!(content_types.len(), 1);

            let boundary = content_types[0]
                .strip_prefix("multipart/form-data; boundary=")
                .unwrap();
            let mut expected = format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"{filename}\"\r\n\
                 Content-Type: {content_type}\r\n\r\n"
            )
            .into_bytes();
            expected.extend_from_slice(bytes);
            expected.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

            assert_eq!(received.body, expected, "upload {filename}");
        }
    });
}

#[test]
fn forms_override_raw_body_and_conflicting_entity_headers() {
    smol::block_on(async {
        for form in [
            FormBody::UrlEncoded(vec![("field".into(), "value".into())]),
            FormBody::Multipart(vec![MultipartField::Text {
                name: "field".into(),
                value: "value".into(),
            }]),
        ] {
            let (url, server) = serve().await;
            let expected_content_type = form.content_type();
            executor()
                .execute(HttpRequest {
                    method: Method::Post,
                    path: url,
                    headers: vec![
                        ("Content-Type".into(), "application/json".into()),
                        ("CONTENT-TYPE".into(), "text/plain".into()),
                        ("Content-Length".into(), "1".into()),
                        ("CONTENT-LENGTH".into(), "0".into()),
                        ("Transfer-Encoding".into(), "chunked".into()),
                        ("TRANSFER-ENCODING".into(), "identity".into()),
                        ("X-Custom".into(), "retained".into()),
                    ],
                    body: Some(b"old raw body".to_vec()),
                    form: Some(form),
                    ..HttpRequest::default()
                })
                .await
                .unwrap();
            let received = server.await;
            let content_types = received.headers("content-type");

            assert_eq!(content_types.len(), 1);
            assert!(content_types[0].starts_with(expected_content_type));
            assert_eq!(
                received.headers("content-length"),
                vec![received.body.len().to_string()]
            );
            assert!(received.headers("transfer-encoding").is_empty());
            assert_eq!(received.headers("x-custom"), vec!["retained"]);
            assert!(String::from_utf8_lossy(&received.body).contains("value"));
            assert!(!String::from_utf8_lossy(&received.body).contains("old raw body"));
        }
    });
}

#[test]
fn forms_ignore_api_key_header_helpers_that_override_body_framing() {
    smol::block_on(async {
        for multipart in [false, true] {
            for (name, value) in [
                ("cOnTeNt-LeNgTh", "1"),
                ("tRaNsFeR-EnCoDiNg", "chunked"),
                ("cOnTeNt-TyPe", "application/bogus"),
            ] {
                let form = if multipart {
                    FormBody::Multipart(vec![MultipartField::Text {
                        name: "name".into(),
                        value: "value".into(),
                    }])
                } else {
                    FormBody::UrlEncoded(vec![("name".into(), "value".into())])
                };
                let (url, server) = serve().await;

                executor()
                    .execute(HttpRequest {
                        method: Method::Post,
                        path: format!("{url}/form"),
                        form: Some(form),
                        authentication: Authentication::ApiKey {
                            name: name.into(),
                            value: value.into(),
                            location: ApiKeyLocation::Header,
                        },
                        ..HttpRequest::default()
                    })
                    .await
                    .unwrap();
                let received = server.await;
                let content_types = received.headers("content-type");
                assert_eq!(content_types.len(), 1);

                let expected = if multipart {
                    let boundary = content_types[0]
                        .strip_prefix("multipart/form-data; boundary=")
                        .unwrap();
                    assert!(!boundary.is_empty());

                    format!(
                        "--{boundary}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nvalue\r\n--{boundary}--\r\n"
                    )
                    .into_bytes()
                } else {
                    assert_eq!(content_types, vec!["application/x-www-form-urlencoded"]);

                    b"name=value".to_vec()
                };

                assert!(received.head.starts_with("POST /form HTTP/1.1\r\n"));
                assert_eq!(received.body, expected, "API key header {name}");
                assert_eq!(
                    received.headers("content-length"),
                    vec![expected.len().to_string()]
                );
                assert!(received.headers("transfer-encoding").is_empty());
            }
        }
    });
}

#[test]
fn forms_preserve_api_key_queries_and_unrelated_headers() {
    smol::block_on(async {
        for multipart in [false, true] {
            for (name, location, target) in [
                (
                    "Content-Length",
                    ApiKeyLocation::Query,
                    "/form?Content-Length=actual-key",
                ),
                ("X-Key", ApiKeyLocation::Header, "/form"),
            ] {
                let form = if multipart {
                    FormBody::Multipart(vec![MultipartField::Text {
                        name: "name".into(),
                        value: "value".into(),
                    }])
                } else {
                    FormBody::UrlEncoded(vec![("name".into(), "value".into())])
                };
                let (url, server) = serve().await;

                executor()
                    .execute(HttpRequest {
                        method: Method::Post,
                        path: format!("{url}/form"),
                        form: Some(form),
                        authentication: Authentication::ApiKey {
                            name: name.into(),
                            value: "actual-key".into(),
                            location,
                        },
                        ..HttpRequest::default()
                    })
                    .await
                    .unwrap();
                let received = server.await;

                assert!(
                    received
                        .head
                        .starts_with(&format!("POST {target} HTTP/1.1\r\n"))
                );

                if location == ApiKeyLocation::Header {
                    assert_eq!(received.headers("x-key"), vec!["actual-key"]);
                } else {
                    assert!(received.headers("x-key").is_empty());
                }

                let content_types = received.headers("content-type");
                assert_eq!(content_types.len(), 1);
                let expected = if multipart {
                    let boundary = content_types[0]
                        .strip_prefix("multipart/form-data; boundary=")
                        .unwrap();
                    assert!(!boundary.is_empty());

                    format!(
                        "--{boundary}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nvalue\r\n--{boundary}--\r\n"
                    )
                    .into_bytes()
                } else {
                    assert_eq!(content_types, vec!["application/x-www-form-urlencoded"]);

                    b"name=value".to_vec()
                };

                assert_eq!(received.body, expected);
                assert_eq!(
                    received.headers("content-length"),
                    vec![expected.len().to_string()]
                );
                assert!(received.headers("transfer-encoding").is_empty());
            }
        }
    });
}

#[test]
fn multipart_escapes_names_and_file_names_without_changing_values() {
    smol::block_on(async {
        let upload = UploadFile::new("quo\"ted\r\n.bin", b"file contents");
        let (url, server) = serve().await;
        executor()
            .execute(HttpRequest {
                method: Method::Post,
                path: url,
                form: Some(FormBody::Multipart(vec![
                    MultipartField::Text {
                        name: "text\"\r\nInjected: yes".into(),
                        value: "unescaped\"\r\nvalue".into(),
                    },
                    MultipartField::File {
                        name: "file\"\r\nInjected: yes".into(),
                        path: upload.path.clone(),
                    },
                ])),
                ..HttpRequest::default()
            })
            .await
            .unwrap();
        let received = server.await;
        let body = String::from_utf8(received.body).unwrap();

        assert!(body.contains("name=\"text%22%0D%0AInjected: yes\"\r\n\r\n"));
        assert!(body.contains("name=\"file%22%0D%0AInjected: yes\";"));
        assert!(body.contains("filename=\"quo%22ted%0D%0A.bin\"\r\n"));
        assert!(body.contains("\r\n\r\nunescaped\"\r\nvalue\r\n"));
        assert!(!body.contains("\r\nInjected:"));
        assert!(!body.contains(upload.directory.to_str().unwrap()));
    });
}

#[test]
fn missing_upload_reports_the_file_path_before_connecting() {
    smol::block_on(async {
        let upload = UploadFile::new("missing.bin", b"deleted before execution");
        fs::remove_file(&upload.path).unwrap();

        let error = executor()
            .execute(HttpRequest {
                method: Method::Post,
                path: "http://127.0.0.1:1/upload".into(),
                form: Some(FormBody::Multipart(vec![MultipartField::File {
                    name: "attachment".into(),
                    path: upload.path.clone(),
                }])),
                ..HttpRequest::default()
            })
            .await
            .unwrap_err();

        match error {
            ExecutionError::Http(HttpError::ReadUpload { path, source }) => {
                assert_eq!(path, upload.path);
                assert_eq!(source.kind(), ErrorKind::NotFound);
            }
            error => panic!("expected upload read failure, got {error:?}"),
        }
    });
}

#[test]
fn deserializes_existing_raw_requests_without_form_data() {
    let saved = serde_json::json!({
        "type": "http",
        "method": "POST",
        "path": "https://example.com/raw",
        "headers": [["Content-Type", "application/octet-stream"]],
        "body": [0, 255, 42],
        "query": [["tag", "existing"]]
    });
    let Request::Http(request) = serde_json::from_value::<Request>(saved).unwrap();

    assert_eq!(request.method, Method::Post);
    assert_eq!(request.body, Some(vec![0, 255, 42]));
    assert_eq!(request.form, None);
    assert_eq!(request.query, Some(vec![("tag".into(), "existing".into())]));
    assert_eq!(
        request.headers,
        vec![("Content-Type".into(), "application/octet-stream".into())]
    );
}

#[test]
fn round_trips_both_form_modes_in_saved_requests() {
    for encoded_form in [
        serde_json::json!({
            "type": "url_encoded",
            "fields": [["tag", "first"], ["tag", "東京"]]
        }),
        serde_json::json!({
            "type": "multipart",
            "fields": [
                {"type": "text", "name": "caption", "value": "a picture"},
                {"type": "file", "name": "upload", "path": "/tmp/photo.jpg"}
            ]
        }),
    ] {
        let form = serde_json::from_value::<FormBody>(encoded_form.clone()).unwrap();
        let saved = Request::Http(HttpRequest {
            method: Method::Patch,
            path: "https://example.com/form".into(),
            form: Some(form.clone()),
            ..HttpRequest::default()
        });
        let encoded_request = serde_json::to_value(&saved).unwrap();

        assert_eq!(encoded_request["form"], encoded_form);
        assert_eq!(encoded_request["method"], "PATCH");

        let Request::Http(decoded) = serde_json::from_value(encoded_request).unwrap();
        assert_eq!(decoded.method, Method::Patch);
        assert_eq!(decoded.form, Some(form));
    }
}

#[test]
fn executes_patch_head_and_options_from_saved_requests() {
    smol::block_on(async {
        let executor = executor();

        for method in [Method::Patch, Method::Head, Method::Options] {
            let (url, server) = serve().await;
            let request = Request::Http(HttpRequest {
                method,
                path: format!("{url}/resource"),
                ..HttpRequest::default()
            });
            let encoded = serde_json::to_vec(&request).unwrap();
            let restored: Request = serde_json::from_slice(&encoded).unwrap();
            let execution = executor.execute(restored).await.unwrap();
            let received = server.await;

            assert!(
                received
                    .head
                    .starts_with(&format!("{} /resource HTTP/1.1\r\n", method.as_str()))
            );

            let Response::Http(response) = execution.response;
            assert_eq!(response.status.as_u16(), 204);
            assert!(response.body.is_empty());
        }
    });
}
