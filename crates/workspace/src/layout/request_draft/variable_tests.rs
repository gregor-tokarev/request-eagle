use std::{
    cell::RefCell,
    fs,
    rc::Rc,
    time::{Duration, Instant},
};

use collection::{HttpRequest, Method};
use gpui_kit::TestAppContext;
use request::{Authentication, ExecutionError, FormBody, MultipartField, RequestExecutor};
use smol::io::{AsyncReadExt, AsyncWriteExt};

use super::{RequestDraft, execution::resolve_request};

#[test]
fn reports_variable_and_environment_file_errors_before_execution() {
    let directory = tempfile::tempdir().unwrap();
    let environment_path = directory.path().join("environment.toml");
    let template = HttpRequest {
        path: "{{base_url}}/account".into(),
        ..HttpRequest::default()
    };

    for path in [None, Some(environment_path.as_path())] {
        let error = resolve_request(&template, path).unwrap_err();

        assert!(matches!(error, ExecutionError::InvalidVariables(_)));
        assert!(error.to_string().contains("base_url"));
    }

    let literal = HttpRequest {
        path: "example.test/account".into(),
        ..HttpRequest::default()
    };
    assert_eq!(
        resolve_request(&literal, Some(environment_path.as_path()))
            .unwrap()
            .path,
        "https://example.test/account"
    );

    fs::write(environment_path.as_path(), "base_url = [").unwrap();
    let error = resolve_request(&template, Some(environment_path.as_path())).unwrap_err();
    assert!(matches!(error, ExecutionError::InvalidVariables(_)));
    assert!(error.to_string().contains("failed to parse"));
}

#[gpui_kit::test]
async fn sends_with_latest_environment_without_changing_draft_templates(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let environment_path = directory.path().join("environment.toml");
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        for value in ["first", "second"] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();

            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                head.push(byte[0]);
            }

            let head = String::from_utf8(head).unwrap();
            let length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;

                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            let mut body = vec![0; length];
            stream.read_exact(&mut body).await.unwrap();

            let method = if value == "first" { "POST" } else { "PATCH" };
            assert!(head.starts_with(&format!("{method} /{value}?account={value}+%26+eagle ")));
            assert!(head.contains(&format!("authorization: Bearer {value}-secret\r\n")));
            assert!(head.contains(&format!("x-account: {value}\r\n")));
            if value == "first" {
                assert!(head.contains("content-type: application/json\r\n"));
                assert_eq!(body, format!("{{\"account\":\"{value}\"}}").as_bytes());
            } else {
                assert!(head.contains("content-type: application/x-www-form-urlencoded\r\n"));
                assert_eq!(body, b"second=second+%26+eagle");
            }

            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
        }
    });

    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let (draft, cx) = cx.add_window_view(|window, cx| {
        let mut draft = RequestDraft::new();
        draft.request = HttpRequest {
            method: Method::Post,
            path: "{{base_url}}/{{account}}".into(),
            headers: vec![("{{header_name}}".into(), "{{account}}".into())],
            query: Some(vec![("account".into(), "{{account}} & eagle".into())]),
            body: Some(b"{\"account\":\"{{account}}\"}".to_vec()),
            authentication: Authentication::Bearer {
                token: "{{account}}-secret".into(),
            },
            ..HttpRequest::default()
        };
        draft.environment_path = Some(environment_path.as_path().to_owned());
        draft.prepare(window, cx);

        draft
    });
    let history = Rc::new(RefCell::new(Vec::new()));
    let _history_subscription = cx.update(|_, cx| {
        let history = history.clone();

        cx.subscribe(&draft, move |_, entry: &crate::history::HistoryEntry, _| {
            history.borrow_mut().push(entry.clone());
        })
    });

    for value in ["first", "second"] {
        fs::write(
            environment_path.as_path(),
            format!("base_url = \"{url}\"\naccount = \"{value}\"\nheader_name = \"X-Account\"\n"),
        )
        .unwrap();

        if value == "second" {
            cx.update(|_, cx| {
                draft.update(cx, |draft, cx| {
                    draft.request.form = Some(FormBody::UrlEncoded(vec![(
                        "{{account}}".into(),
                        "{{account}} & eagle".into(),
                    )]));
                    draft.set_method(Method::Patch, cx);
                })
            });
        }

        cx.update(|window, cx| draft.update(cx, |draft, cx| draft.send(window, cx)));
        let started = Instant::now();

        while cx.read(|cx| draft.read(cx).task.is_some()) {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "send did not finish"
            );
            smol::Timer::after(Duration::from_millis(10)).await;
            cx.run_until_parked();
        }

        cx.update(|window, _| window.refresh());
        assert!(cx.debug_bounds("response-status").is_some());
        cx.read(|cx| {
            let request = &draft.read(cx).request;

            assert_eq!(request.path, "{{base_url}}/{{account}}");
            assert_eq!(
                request.headers[0],
                ("{{header_name}}".into(), "{{account}}".into())
            );
            assert_eq!(
                request.body.as_deref(),
                Some(b"{\"account\":\"{{account}}\"}".as_slice())
            );
            assert_eq!(
                request.authentication,
                Authentication::Bearer {
                    token: "{{account}}-secret".into(),
                }
            );
        });
    }

    server.await;

    let history = history.borrow();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].request.method, Method::Post);
    assert_eq!(history[1].request.method, Method::Patch);
    assert_eq!(
        history[1].request.form,
        Some(FormBody::UrlEncoded(vec![(
            "{{account}}".into(),
            "{{account}} & eagle".into(),
        )]))
    );

    for entry in history.iter() {
        assert_eq!(entry.request.path, "{{base_url}}/{{account}}");
        assert_eq!(entry.environment_path.as_ref(), Some(&environment_path));
        assert_eq!(
            entry.request.authentication,
            Authentication::Bearer {
                token: "{{account}}-secret".into(),
            }
        );
    }
}

#[gpui_kit::test]
async fn missing_variables_prevent_network_and_record_the_attempt(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let environment_path = directory.path().join("environment.toml");
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/account", listener.local_addr().unwrap());

    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let (draft, cx) = cx.add_window_view(|window, cx| {
        let mut draft = RequestDraft::new();
        draft.request.path = url.clone();
        draft.request.authentication = Authentication::Bearer {
            token: "{{missing_token}}".into(),
        };
        draft.environment_path = Some(environment_path.clone());
        draft.prepare(window, cx);

        draft
    });
    let history = Rc::new(RefCell::new(Vec::new()));
    let _history_subscription = cx.update(|_, cx| {
        let history = history.clone();

        cx.subscribe(&draft, move |_, entry: &crate::history::HistoryEntry, _| {
            history.borrow_mut().push(entry.clone());
        })
    });
    cx.update(|window, cx| draft.update(cx, |draft, cx| draft.send(window, cx)));
    let started = Instant::now();

    while cx.read(|cx| draft.read(cx).task.is_some()) {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "send did not finish"
        );
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }

    let connected = smol::future::or(async { listener.accept().await.is_ok() }, async {
        smol::Timer::after(Duration::from_millis(20)).await;

        false
    })
    .await;
    assert!(
        !connected,
        "a missing variable must prevent the HTTP request"
    );

    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("response-status").is_none());
    assert!(cx.debug_bounds("response-empty").is_some());
    let history = history.borrow();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].request.path, url);
    assert_eq!(
        history[0].environment_path.as_ref(),
        Some(&environment_path)
    );
    assert_eq!(
        history[0].request.authentication,
        Authentication::Bearer {
            token: "{{missing_token}}".into(),
        }
    );
}

#[test]
fn patch_multipart_resolves_environment_and_uploads_original_file_bytes() {
    smol::block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let environment_path = directory.path().join("environment.toml");
        let upload_path = directory.path().join("example upload.bin");
        let upload = b"\0{{keep_file_contents_literal}}\xff";
        fs::write(&upload_path, upload).unwrap();

        let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/upload", listener.local_addr().unwrap());
        let server = smol::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();

            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                head.push(byte[0]);
            }

            let head = String::from_utf8(head).unwrap();
            let length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;

                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            let mut body = vec![0; length];
            stream.read_exact(&mut body).await.unwrap();

            assert!(head.starts_with("PATCH /upload "));
            assert!(head.contains("content-type: multipart/form-data; boundary="));
            assert!(head.contains("authorization: Bearer explicit-token\r\n"));
            assert!(body.windows(upload.len()).any(|bytes| bytes == upload));

            let text = String::from_utf8_lossy(&body);
            assert!(text.contains("name=\"account\"\r\n\r\neagle & bird"));
            assert!(text.contains("name=\"attachment\"; filename=\"example upload.bin\""));

            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
        });
        environment::Environment {
            path: environment_path.clone(),
            entries: std::collections::HashMap::from([
                ("url".into(), url),
                ("name".into(), "account".into()),
                ("value".into(), "eagle & bird".into()),
                ("file_field".into(), "attachment".into()),
                ("file_path".into(), upload_path.to_str().unwrap().into()),
            ]),
        }
        .save_file()
        .unwrap();
        let template = HttpRequest {
            method: Method::Patch,
            path: "{{url}}".into(),
            headers: vec![("Authorization".into(), "Bearer explicit-token".into())],
            body: Some(b"{{inactive_raw_body}}".to_vec()),
            form: Some(FormBody::Multipart(vec![
                MultipartField::Text {
                    name: "{{name}}".into(),
                    value: "{{value}}".into(),
                },
                MultipartField::File {
                    name: "{{file_field}}".into(),
                    path: "{{file_path}}".into(),
                },
            ])),
            authentication: Authentication::Bearer {
                token: "{{unused_inherited_token}}".into(),
            },
            ..HttpRequest::default()
        };
        let resolved = resolve_request(&template, Some(&environment_path)).unwrap();
        let execution = RequestExecutor::new(&request::RequestPreferences {
            timeout_ms: 2_000,
            ..Default::default()
        })
        .unwrap()
        .execute(resolved)
        .await
        .unwrap();
        server.await;

        let request::Response::Http(response) = execution.response;
        assert_eq!(response.status.as_u16(), 200);
        assert_eq!(template.path, "{{url}}");
        assert_eq!(fs::read(&upload_path).unwrap(), upload);
        assert!(matches!(
            template.form.unwrap(),
            FormBody::Multipart(fields)
                if fields[1] == MultipartField::File {
                    name: "{{file_field}}".into(),
                    path: "{{file_path}}".into(),
                }
        ));
    });
}

#[test]
fn inactive_bodies_and_form_framing_do_not_require_variables() {
    for method in [Method::Get, Method::Head] {
        let template = HttpRequest {
            method,
            path: "https://example.com/items".into(),
            body: Some(b"{{unused_raw}}".to_vec()),
            form: Some(FormBody::UrlEncoded(vec![(
                "name".into(),
                "{{unused_form}}".into(),
            )])),
            ..Default::default()
        };
        let outgoing = resolve_request(&template, None).unwrap();
        assert!(outgoing.body.is_none());
        assert!(outgoing.form.is_none());
        assert!(template.body.is_some());
        assert!(template.form.is_some());
    }

    let template = HttpRequest {
        method: Method::Post,
        path: "https://example.com/items".into(),
        headers: vec![
            ("Content-Type".into(), "{{stale_type}}".into()),
            ("Content-Length".into(), "{{stale_length}}".into()),
        ],
        form: Some(FormBody::UrlEncoded(vec![("name".into(), "value".into())])),
        ..Default::default()
    };
    assert!(resolve_request(&template, None).unwrap().headers.is_empty());
}
