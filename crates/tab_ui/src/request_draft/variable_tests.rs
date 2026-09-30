use gpui_kit::{AppContext as _, Entity, Modifiers, TestAppContext, VisualTestContext};
use request::Method;

use std::path::Path;

use super::{RequestDraft, RequestLocation, draft::RequestSection};

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
            let mut draft = super::tests::new_draft(cx);
            draft.set_location(location(&directory.path().join("request.toml"), 0), cx);
            draft.prepare(window, cx);
            draft
        });
        draft = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    (draft.unwrap(), cx, directory)
}

/// A saved request's location, `folders` deep in its collection.
fn location(path: &Path, folders: usize) -> RequestLocation {
    RequestLocation {
        path: path.to_path_buf(),
        id: "request".into(),
        name: "Request".into(),
        collection: "Collection".into(),
        folders: vec!["Folder".into(); folders],
    }
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
fn completion_virtualizes_large_environments_and_remeasures_at_each_zoom(cx: &mut TestAppContext) {
    use gpui_kit::{component::Theme, px, size};

    let (draft, cx, directory) = setup(cx);
    let variables = (0..1000)
        .map(|index| format!("variable_{index:04} = 'value'\n"))
        .collect::<String>();
    std::fs::write(directory.path().join("environment.toml"), variables).unwrap();

    for theme in ["Default Light", "Default Dark"] {
        for font_size in [12., 16., 24.] {
            cx.update(|_, cx| {
                assert!(request_eagle_theme::apply(theme, cx));
                Theme::global_mut(cx).font_size = px(font_size);
                Theme::sync_base(cx);
            });
            cx.simulate_resize(size(px(1100.), px(900.)));
            click(cx, "request-url");
            cx.simulate_keystrokes("secondary-a");
            cx.simulate_input("{{variable_");
            assert!(popup(cx));
            assert!(cx.debug_bounds("variable-suggestion-999").is_none());
            let first = cx.debug_bounds("variable-suggestion-0").unwrap();
            assert_eq!(first.size.height, px(font_size * 2.));

            // Up wraps from the first to the last model row, even though it
            // has no element until the virtual list scrolls it into view.
            cx.simulate_keystrokes("up");
            assert!(popup(cx));
            assert!(cx.debug_bounds("variable-suggestion-999").is_some());
            cx.simulate_keystrokes("enter");
            cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{variable_0999}}"));

            click(cx, "request-url");
            cx.simulate_keystrokes("secondary-a");
            cx.simulate_input("{{variable_0500");
            assert!(popup(cx));
            let row = cx.debug_bounds("variable-suggestion-0").unwrap();
            cx.simulate_click(row.center(), Modifiers::default());
            cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{variable_0500}}"));
        }
    }
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

            draft.set_location(
                location(
                    Path::new("/tmp/variable-test/Collection/Folder/request.toml"),
                    1,
                ),
                cx,
            );
            assert_eq!(
                draft.variables.read(cx).path.as_deref(),
                Some(std::path::Path::new(
                    "/tmp/variable-test/Collection/environment.toml"
                ))
            );
            assert!(
                draft
                    .variables
                    .read(cx)
                    .values(cx)
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
            draft.request.method = Method::Get;
            draft.request.path = "{{base_url}}/{{created}}".into();
            draft.request.headers = vec![("{{header}}".into(), "text/plain".into())];
            draft.request.body = Some(b"{{message}}/{{!message}}/{{literal}}".to_vec());
            draft.request.scripts.pre_request = "pm.request.method = 'POST'; pm.expect(pm.variables.get('message')).to.equal('from file'); pm.variables.set('created', 'created'); pm.variables.set('message', 'local');".into();
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
            draft.set_location(location(&a.join("request.toml"), 0), cx);
            std::fs::rename(&a, &b).unwrap();
            draft.set_location(location(&b.join("request.toml"), 0), cx);
            std::fs::write(b.join("environment.toml"), "base_url = 'updated'").unwrap();
            std::fs::rename(&b, &a).unwrap();
            draft.set_location(location(&a.join("request.toml"), 0), cx);

            assert_eq!(
                draft.variables.read(cx).values(cx).unwrap().environment["base_url"],
                "updated"
            );

            // Reopening a request must also refresh a reused path before preparation.
            std::fs::write(a.join("environment.toml"), "base_url = 'reopened'").unwrap();
            let reopened = cx.new(|cx| {
                RequestDraft::new(
                    Default::default(),
                    Some(location(&a.join("request.toml"), 0)),
                    Default::default(),
                    None,
                    cx,
                )
            });
            assert_eq!(
                reopened
                    .read(cx)
                    .variables
                    .read(cx)
                    .values(cx)
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

#[gpui_kit::test]
fn variable_completion_follows_the_caret_on_the_first_frame(cx: &mut TestAppContext) {
    use crate::variable_input::VariableTarget;
    use gpui_kit::{
        Background,
        component::{ActiveTheme as _, Theme},
        point, px, rems, size,
    };

    let (draft, cx, _directory) = setup(cx);
    cx.simulate_resize(size(px(1440.), px(900.)));

    for body in [false, true] {
        let target = cx.update(|window, cx| {
            draft.update(cx, |draft, cx| {
                if body {
                    draft.set_method(Method::Post, cx);
                    draft.section = RequestSection::Body;
                    let editor = draft.body_state(window, cx);
                    editor.update(cx, |editor, cx| editor.focus(window, cx));
                    cx.notify();
                    VariableTarget::Editor(editor)
                } else {
                    let input = draft.url.clone().unwrap();
                    input.update(cx, |input, cx| input.focus(window, cx));
                    VariableTarget::Input(input)
                }
            })
        });
        cx.simulate_input("{{base_url");

        for theme in ["Default Light", "Default Dark"] {
            for font_size in [12., 16., 24.] {
                cx.update(|_, cx| {
                    assert!(request_eagle_theme::apply(theme, cx));
                    Theme::global_mut(cx).font_size = px(font_size);
                    Theme::sync_base(cx);
                });
                for offset in [3, 7, 5, 10] {
                    cx.update(|window, cx| {
                        match &target {
                            VariableTarget::Input(input) => input.update(cx, |input, cx| {
                                input.set_selected_range(offset..offset, cx)
                            }),
                            VariableTarget::Editor(editor) => editor.update(cx, |editor, cx| {
                                editor.set_selected_range(offset..offset, cx)
                            }),
                        }
                        window.refresh();
                        // Inspect this draw before notifications can cause a catch-up frame.
                        window.draw(cx).clear(cx);

                        let (caret, height, scroll) = match &target {
                            VariableTarget::Input(input) => {
                                let input = input.read(cx);
                                let (caret, height) = input.cursor_layout().unwrap();
                                (caret, height, input.scroll_offset())
                            }
                            VariableTarget::Editor(editor) => {
                                let editor = editor.read(cx);
                                let (caret, height) = editor.cursor_layout().unwrap();
                                (caret, height, editor.scroll_offset())
                            }
                        };
                        let gap = rems(0.25).to_pixels(window.rem_size());
                        let expected = window
                            .pixel_snap_point(caret.origin + point(px(0.), scroll.y + height + gap))
                            .scale(window.scale_factor());
                        let background = Background::from(cx.theme().popover);
                        let popovers = window
                            .painted_quads()
                            .iter()
                            .filter(|quad| quad.background == background)
                            .map(|quad| quad.bounds.origin)
                            .collect::<Vec<_>>();
                        let at_caret = popovers.iter().any(|origin| {
                            (origin.x - expected.x).0.abs() <= 1.
                                && (origin.y - expected.y).0.abs() <= 1.
                        });
                        assert!(
                            at_caret,
                            "body={body}, {theme}, {font_size}px, cursor {offset}: expected {expected:?}, got {popovers:?}"
                        );
                    });
                }
            }
        }
    }
}

#[gpui_kit::test]
async fn response_token_is_reused_by_another_draft_and_appears_in_completion(
    cx: &mut TestAppContext,
) {
    use smol::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use std::time::{Duration, Instant};

    cx.executor().allow_parking();
    let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = smol::spawn(async move {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();

            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                head.push(byte[0]);
            }

            let head = String::from_utf8(head).unwrap();

            if index == 0 {
                assert!(head.starts_with("GET /login HTTP/1.1"), "{head}");
                let body = r#"{"token":"response-token"}"#;
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            } else {
                assert!(head.starts_with("GET /protected HTTP/1.1"), "{head}");
                assert!(
                    head.contains("authorization: Bearer response-token\r\n"),
                    "{head}"
                );
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
            }
        }
    });
    let (login, cx, directory) = setup(cx);
    let sessions = cx.read(|cx| login.read(cx).variable_sessions.clone());
    let original_file = std::fs::read_to_string(directory.path().join("environment.toml")).unwrap();
    cx.update(|window, cx| {
        login.update(cx, |draft, cx| {
            draft.request.path = format!("http://{address}/login");
            draft.request.scripts.post_response = "pm.environment.set('token', pm.response.json().token); pm.variables.set('scratch', 'local only');".into();
            draft.send(window, cx);
        });
    });

    let deadline = Instant::now() + Duration::from_secs(5);

    while cx.read(|cx| login.read(cx).is_sending()) {
        assert!(Instant::now() < deadline);
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }

    let protected = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut draft = RequestDraft::new(
                Default::default(),
                Some(location(&directory.path().join("nested/protected.toml"), 1)),
                sessions.clone(),
                None,
                cx,
            );
            draft.request.path = format!("http://{address}/protected");
            draft.request.headers = vec![("Authorization".into(), "Bearer {{token}}".into())];
            let scope = draft.variables.clone();
            let values = scope.read(cx).values(cx).unwrap();
            assert_eq!(values.environment["token"], "response-token");
            assert!(!values.environment.contains_key("scratch"));
            draft.send(window, cx);
            draft
        })
    });
    let deadline = Instant::now() + Duration::from_secs(5);

    while cx.read(|cx| protected.read(cx).is_sending()) {
        assert!(Instant::now() < deadline);
        smol::Timer::after(Duration::from_millis(10)).await;
        cx.run_until_parked();
    }

    smol::future::or(server, async {
        smol::Timer::after(Duration::from_secs(5)).await;
        panic!("both requests should reach the test server");
    })
    .await;
    cx.read(|cx| assert_eq!(protected.read(cx).request.headers[0].1, "Bearer {{token}}"));
    assert_eq!(
        std::fs::read_to_string(directory.path().join("environment.toml")).unwrap(),
        original_file
    );

    click(cx, "request-url");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("{{token");
    assert!(popup(cx));
    assert!(cx.debug_bounds("variable-suggestion-0").is_some());
    cx.simulate_keystrokes("enter");
    cx.read(|cx| assert_eq!(login.read(cx).request.path, "{{token}}"));
}

#[gpui_kit::test]
fn the_active_global_environment_overrides_collection_values(cx: &mut TestAppContext) {
    let (draft, cx, directory) = setup(cx);
    let catalog = environment::GlobalEnvironments::new(directory.path().join("environments"));
    catalog.create("Staging").unwrap();
    std::fs::write(
        catalog.path("Staging"),
        "base_url = 'https://staging.example.com'\ntoken = 'staging'\n",
    )
    .unwrap();

    cx.update(|_, cx| {
        let environments = cx.new(|_| crate::Environments::new(catalog, None));
        let scope = draft.read(cx).variables.clone();
        scope.update(cx, |scope, _| {
            scope.environments = Some(environments.clone())
        });

        let values = scope.read(cx).values(cx).unwrap();
        assert_eq!(values.environment["base_url"], "https://example.com");
        assert!(!values.environment.contains_key("token"));

        environments.update(cx, |environments, cx| {
            environments.set_active(Some("Staging".into()), cx)
        });
        let values = scope.read(cx).values(cx).unwrap();
        assert_eq!(
            values.environment["base_url"],
            "https://staging.example.com"
        );
        assert_eq!(values.environment["message"], "hello");
        assert_eq!(values.environment["token"], "staging");

        let request = request::HttpRequest {
            path: "{{base_url}}/users".into(),
            ..Default::default()
        };
        assert_eq!(
            scope
                .read(cx)
                .request_variables(cx)
                .resolve(&request)
                .unwrap()
                .path,
            "https://staging.example.com/users"
        );
    });
}
