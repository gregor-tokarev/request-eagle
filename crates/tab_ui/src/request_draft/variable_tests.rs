use gpui_kit::{AppContext as _, Entity, Modifiers, TestAppContext, VisualTestContext};
use request::Method;

use super::{RequestDraft, draft::RequestSection};

fn setup(
    cx: &mut TestAppContext,
) -> (
    Entity<RequestDraft>,
    &mut VisualTestContext,
    tempfile::TempDir,
) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("environment.toml"),
        "base_url = 'https://example.com'\nmessage = 'hello'\n",
    )
    .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
    });
    let mut draft = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut draft = RequestDraft::new();
            draft.set_variable_environment(&directory.path().join("request.toml"), 0, cx);
            draft.prepare(window, cx);
            draft
        });
        draft = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    (draft.unwrap(), cx, directory)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, _| window.refresh());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
}

fn popup(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, _| window.refresh());
    cx.debug_bounds("variable-completions").is_some()
}

#[gpui_kit::test]
fn completion_excludes_environment_names_reserved_for_other_sources(cx: &mut TestAppContext) {
    let (_, cx, directory) = setup(cx);
    std::fs::write(
        directory.path().join("environment.toml"),
        r#"
        "$guid" = "reserved"
        "$unsupported" = "reserved"
        "!literal" = "reserved"
        " spaced" = "invalid"
    "#,
    )
    .unwrap();
    click(cx, "request-url");
    for (name, available) in [("$guid", true), ("$unsupported", false)] {
        cx.simulate_keystrokes("secondary-a");
        cx.simulate_input(&format!("{{{{{name}"));
        assert!(popup(cx));
        assert_eq!(
            cx.debug_bounds("variable-suggestion-0").is_some(),
            available,
            "{name}"
        );
        assert!(
            cx.debug_bounds("variable-suggestion-1").is_none(),
            "{name} must not have a duplicate Environment suggestion"
        );
    }
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("{{!message}}");
    assert!(
        !popup(cx),
        "escaped references do not open variable completion"
    );
}

#[gpui_kit::test]
fn variable_completion_filters_accepts_dismisses_and_supports_undo(cx: &mut TestAppContext) {
    let (draft, cx, _directory) = setup(cx);
    click(cx, "request-url");
    cx.simulate_input("{");
    assert!(!popup(cx));
    cx.simulate_input("{");
    assert!(popup(cx));
    cx.simulate_keystrokes("down enter");
    cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{message}}"));
    assert!(!popup(cx));
    cx.simulate_keystrokes("secondary-z");
    cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{"));
    cx.simulate_input("base");
    assert!(popup(cx));
    cx.simulate_keystrokes("tab");
    cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{base_url}}"));

    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("{{does_not_exist");
    assert!(popup(cx));
    cx.simulate_keystrokes("escape");
    assert!(!popup(cx));
    cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{does_not_exist"));
}

#[gpui_kit::test]
fn variable_completion_works_in_params_headers_and_json(cx: &mut TestAppContext) {
    let (draft, cx, _directory) = setup(cx);
    click(cx, "request-section-Params");
    click(cx, "params-key-0");
    cx.simulate_input("{{mess");
    cx.simulate_keystrokes("enter");
    click(cx, "params-value-0");
    cx.simulate_input("{{$guid");
    assert!(popup(cx));
    click(cx, "variable-suggestion-0");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.query.as_ref().unwrap()[0],
            ("{{message}}".into(), "{{$guid}}".into())
        )
    });

    click(cx, "request-section-Headers");
    click(cx, "headers-key-0");
    cx.simulate_input("X-{{mess");
    cx.simulate_keystrokes("tab");
    click(cx, "headers-value-0");
    cx.simulate_input("{{$time");
    cx.simulate_keystrokes("enter");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.headers[0],
            ("X-{{message}}".into(), "{{$timestamp}}".into())
        )
    });

    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_method(Method::Post, cx);
            draft.section = RequestSection::Body;
            draft.prepare(window, cx);
            draft.body.as_ref().unwrap().update(cx, |body, cx| {
                body.replace_all("{\"time\":\"{{$iso}}\"}", window, cx);
                body.set_selected_range(15..15, cx);
                body.focus(window, cx);
            });
        })
    });
    assert!(popup(cx));
    cx.simulate_keystrokes("enter");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.body.as_deref().unwrap(),
            br#"{"time":"{{$isoTimestamp}}"}"#
        )
    });
}

#[gpui_kit::test]
fn variable_completion_handles_unicode_blur_and_window_edges_at_all_scales(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{px, size};

    let (draft, cx, _directory) = setup(cx);
    for theme in ["Ayu Light", "Ayu Dark"] {
        for font_size in [12., 16., 24.] {
            cx.update(|window, cx| {
                request_eagle_theme::apply(theme, cx);
                gpui_kit::component::Theme::global_mut(cx).font_size = px(font_size);
                window.set_rem_size(px(font_size));
                window.refresh();
            });
            cx.simulate_resize(size(px(1024.), px(800.)));
            let text = format!(
                "https://example.com/{}/🦅/{{{{base}}}}/tail",
                "long-path/".repeat(20)
            );
            let caret = text.find("base}}").unwrap() + 4;
            cx.update(|window, cx| {
                draft.read(cx).url.clone().unwrap().update(cx, |input, cx| {
                    input.set_value(text.clone(), window, cx);
                    input.set_selected_range(caret..caret, cx);
                    input.focus(window, cx);
                });
            });
            assert!(popup(cx));
            let bounds = cx.debug_bounds("variable-completions").unwrap();
            assert!(bounds.left() >= px(0.) && bounds.right() <= px(1024.));
            assert!(bounds.top() >= px(0.) && bounds.bottom() <= px(800.));
            cx.simulate_keystrokes("enter");
            cx.read(|cx| {
                assert_eq!(
                    draft.read(cx).request.path,
                    text.replace("{{base}}", "{{base_url}}")
                )
            });

            cx.simulate_keystrokes("secondary-a");
            cx.simulate_input("{{");
            assert!(popup(cx));
            click(cx, "headers-key-0");
            assert!(!popup(cx));
        }
    }
}

#[gpui_kit::test]
async fn unresolved_variables_block_send_and_collection_scope_changes_with_the_request(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (draft, cx, _directory) = setup(cx);
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.request.path =
                format!("http://{}/{{{{missing}}}}", listener.local_addr().unwrap());
            draft.send(window, cx);
            assert!(draft.task.is_some());

            draft.set_variable_environment(
                std::path::Path::new("/tmp/variable-test/Collection/Folder/request.toml"),
                1,
                cx,
            );
            assert_eq!(
                draft.variables(cx).read(cx).path.as_deref(),
                Some(std::path::Path::new(
                    "/tmp/variable-test/Collection/environment.toml"
                ))
            );
            assert!(
                draft
                    .variables(cx)
                    .read(cx)
                    .values()
                    .unwrap()
                    .environment
                    .is_empty()
            );
        });
    });
    let started = std::time::Instant::now();
    while cx.read(|cx| draft.read(cx).task.is_some()) {
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        smol::Timer::after(std::time::Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    assert!(
        smol::future::poll_once(listener.accept()).await.is_none(),
        "unresolved variables must block HTTP dispatch"
    );
}

#[gpui_kit::test]
async fn unavailable_environment_only_blocks_requests_using_environment_variables(
    cx: &mut TestAppContext,
) {
    use smol::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use std::time::{Duration, Instant};

    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = smol::spawn(async move {
        for index in 0..3 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                head.push(byte[0]);
            }
            let head = String::from_utf8(head).unwrap();
            assert!(!head.contains("{{"));
            if index == 2 {
                assert!(head.starts_with("GET /fresh-from-file HTTP/1.1"));
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }
    });
    let (draft, cx, _directory) = setup(cx);
    std::fs::write(_directory.path().join("environment.toml"), "[invalid").unwrap();
    for path in ["literal", "{{$guid}}", "{{message}}"] {
        if path == "{{message}}" {
            std::fs::write(
                _directory.path().join("environment.toml"),
                "message = 'fresh-from-file'",
            )
            .unwrap();
        }
        cx.update(|window, cx| {
            draft.update(cx, |draft, cx| {
                draft.request.path = format!("http://{address}/{path}");
                draft.send(window, cx);
                assert!(draft.task.is_some(), "{path} should resolve and dispatch");
            });
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while cx.read(|cx| draft.read(cx).task.is_some()) {
            assert!(Instant::now() < deadline);
            smol::Timer::after(Duration::from_millis(10)).await;
            cx.run_until_parked();
        }
    }
    server.await;
    std::fs::write(_directory.path().join("environment.toml"), "[invalid").unwrap();
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.request.path = "{{base_url}}".into();
            draft.send(window, cx);
            assert!(draft.task.is_some());
        });
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while cx.read(|cx| draft.read(cx).task.is_some()) {
        assert!(Instant::now() < deadline);
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
}

#[test]
fn environment_errors_are_reported_only_for_environment_references() {
    use request::RequestVariables;
    let values = environment::VariableValues {
        environment: [("base_url".into(), "https://cached.example".into())].into(),
    };
    let mut request = request::HttpRequest {
        path: "{{ base_url }}".into(),
        ..Default::default()
    };
    assert_eq!(
        RequestVariables::new(values.clone(), Some("Invalid environment file".into()))
            .resolve(&request)
            .unwrap_err(),
        "Invalid environment file"
    );
    request.path = "http://example.com/{{$unsupported}}".into();
    assert!(
        RequestVariables::new(values, Some("Invalid environment file".into()))
            .resolve(&request)
            .unwrap_err()
            .contains("Unknown variable")
    );
}

#[gpui_kit::test]
async fn send_resolves_environment_and_script_variables_once(cx: &mut TestAppContext) {
    use smol::io::{AsyncReadExt, AsyncWriteExt};

    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = smol::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        assert!(head.starts_with("POST /created HTTP/1.1"), "{head}");
        assert!(head.contains("content-type: text/plain\r\n"), "{head}");
        assert!(!head.contains("application/json"), "{head}");
        let mut body = [0; 29];
        stream.read_exact(&mut body).await.unwrap();
        assert_eq!(&body, b"local/{{message}}/{{created}}");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });
    let (draft, cx, directory) = setup(cx);
    std::fs::write(directory.path().join("environment.toml"), format!("base_url = 'http://{address}'\nmessage = 'from file'\nliteral = '{{{{created}}}}'\nheader = 'Content-Type'\n")).unwrap();
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.request.method = Method::Post;
            draft.request.path = "{{base_url}}/{{created}}".into();
            draft.request.headers = vec![("{{header}}".into(), "text/plain".into())];
            draft.request.body = Some(b"{{message}}/{{!message}}/{{literal}}".to_vec());
            draft.request.scripts.pre_request = "pm.expect(pm.variables.get('message')).to.equal('from file'); pm.variables.set('created', 'created'); pm.variables.set('message', 'local');".into();
            draft.trusted_scripts = Some(draft.request.scripts.clone());
            draft.send(window, cx);
        });
    });
    let started = std::time::Instant::now();
    while cx.read(|cx| draft.read(cx).task.is_some()) {
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        smol::Timer::after(std::time::Duration::from_millis(10)).await;
        cx.run_until_parked();
    }
    server.await;
    assert!(
        std::fs::read_to_string(directory.path().join("environment.toml"))
            .unwrap()
            .contains("message = 'from file'")
    );
}

#[gpui_kit::test]
fn renaming_collections_back_to_an_old_path_reloads_environment_values(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("A");
    let b = directory.path().join("B");
    std::fs::create_dir(&a).unwrap();
    std::fs::write(a.join("environment.toml"), "base_url = 'original'").unwrap();
    let (draft, cx, _directory) = setup(cx);

    cx.update(|_, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_variable_environment(&a.join("request.toml"), 0, cx);
            std::fs::rename(&a, &b).unwrap();
            draft.set_variable_environment(&b.join("request.toml"), 0, cx);
            std::fs::write(b.join("environment.toml"), "base_url = 'updated'").unwrap();
            std::fs::rename(&b, &a).unwrap();
            draft.set_variable_environment(&a.join("request.toml"), 0, cx);

            assert_eq!(
                draft.variables(cx).read(cx).values().unwrap().environment["base_url"],
                "updated"
            );

            // Reopening a request must also refresh a reused path before preparation.
            std::fs::write(a.join("environment.toml"), "base_url = 'reopened'").unwrap();
            let mut reopened = RequestDraft::new();
            reopened.set_variable_environment(&a.join("request.toml"), 0, cx);
            assert!(reopened.variable_scope.is_none());
            assert_eq!(
                reopened
                    .variables(cx)
                    .read(cx)
                    .values()
                    .unwrap()
                    .environment["base_url"],
                "reopened"
            );
        })
    });
}

#[test]
fn environment_errors_are_reported_in_every_request_field() {
    use environment::VariableValues;
    use request::HttpRequest;
    use request::RequestVariables;

    let values = VariableValues {
        environment: [("message".into(), "cached value".into())].into(),
    };
    for field in 0..6 {
        let mut request = HttpRequest {
            method: Method::Post,
            path: "http://example.com".into(),
            ..Default::default()
        };
        let token = "{{ message }}".to_owned();
        match field {
            0 => request.path.push_str(&format!("/{token}")),
            1 => request.headers.push((token, "value".into())),
            2 => request.headers.push(("X-Message".into(), token)),
            3 => request.query = Some(vec![(token, "value".into())]),
            4 => request.query = Some(vec![("message".into(), token)]),
            _ => request.body = Some(token.into_bytes()),
        }
        assert!(
            RequestVariables::new(values.clone(), None)
                .resolve(&request)
                .is_ok()
        );
        assert_eq!(
            RequestVariables::new(values.clone(), Some("Invalid environment file".into()))
                .resolve(&request)
                .unwrap_err(),
            "Invalid environment file"
        );
    }

    let request = HttpRequest {
        path: "http://example.com".into(),
        body: Some(b"{{ message }}".to_vec()),
        ..Default::default()
    };
    assert!(
        RequestVariables::new(values, Some("Invalid environment file".into()))
            .resolve(&request.prepare_for_send())
            .is_ok(),
        "GET excludes the body before resolution"
    );
}

#[gpui_kit::test]
fn reopening_completion_reads_external_environment_changes(cx: &mut TestAppContext) {
    let (draft, cx, directory) = setup(cx);
    click(cx, "request-url");
    cx.simulate_input("{{new_name");
    assert!(popup(cx));
    assert!(cx.debug_bounds("variable-suggestion-0").is_none());
    cx.simulate_keystrokes("escape");
    std::fs::write(
        directory.path().join("environment.toml"),
        "new_name = 'new value'",
    )
    .unwrap();
    cx.simulate_keystrokes("backspace");
    cx.simulate_input("e");
    assert!(popup(cx));
    cx.simulate_keystrokes("enter");
    cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{new_name}}"));
}
