use gpui_kit::{AppContext as _, Modifiers, MouseButton, TestAppContext, point, px};
use request::{
    ApiKeyLocation, Authentication, FormBody, HttpRequest, HttpVersion, Method, MultipartField,
    RequestExecutor, RequestPreferences,
};
use smol::io::{AsyncReadExt, AsyncWriteExt};

use super::{
    RequestDraft,
    draft::RequestSection,
    execution::{generated_headers, outgoing_request, resolve_request},
};

#[test]
fn form_owned_headers_skip_helpers_in_preview_and_variable_resolution() {
    for form in [
        FormBody::UrlEncoded(vec![("name".into(), "value".into())]),
        FormBody::Multipart(vec![MultipartField::Text {
            name: "name".into(),
            value: "value".into(),
        }]),
    ] {
        for name in ["cOnTeNt-TyPe", "cOnTeNt-LeNgTh", "tRaNsFeR-EnCoDiNg"] {
            let request = HttpRequest {
                method: Method::Post,
                path: "https://example.test/form".into(),
                form: Some(form.clone()),
                authentication: Authentication::ApiKey {
                    name: name.into(),
                    value: "{{unused_helper}}".into(),
                    location: ApiKeyLocation::Header,
                },
                ..Default::default()
            };
            let headers = generated_headers(&request);

            assert!(headers.contains(&("Content-Type".into(), form.content_type().into())));
            assert!(headers.iter().all(|(name, value)| {
                !name.eq_ignore_ascii_case("transfer-encoding") && !value.contains("[hidden]")
            }));
            let lengths = headers
                .iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                lengths,
                form.encoded_len()
                    .map(|length| length.to_string())
                    .into_iter()
                    .collect::<Vec<_>>()
            );
            let outgoing = resolve_request(&request, None).unwrap();
            assert_eq!(outgoing.authentication, request.authentication);
            assert_eq!(outgoing.form, request.form);
            assert!(request.headers.is_empty());
        }
    }
}

#[test]
fn raw_and_inactive_form_framing_helpers_remain_active() {
    for (method, form) in [
        (Method::Post, None),
        (
            Method::Get,
            Some(FormBody::UrlEncoded(vec![("name".into(), "value".into())])),
        ),
    ] {
        for name in ["Content-Length", "Transfer-Encoding"] {
            let request = HttpRequest {
                method,
                path: "https://example.test/form".into(),
                body: Some(b"raw body".to_vec()),
                form: form.clone(),
                authentication: Authentication::ApiKey {
                    name: name.into(),
                    value: "{{active_helper}}".into(),
                    location: ApiKeyLocation::Header,
                },
                ..Default::default()
            };
            assert!(generated_headers(&request).contains(&(name.into(), "[hidden]".into())));
            assert!(
                resolve_request(&request, None)
                    .unwrap_err()
                    .to_string()
                    .contains("active_helper")
            );
        }
    }
}

#[test]
fn bearer_preview_masks_the_selected_token_and_execution_overrides_url_credentials() {
    smol::block_on(async {
        for explicit in [false, true] {
            let listener = smol::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let request = HttpRequest {
                path: format!("http://sam:pass@{}/", listener.local_addr().unwrap()),
                authentication: Authentication::Bearer {
                    token: "actual-token".into(),
                },
                headers: if explicit {
                    vec![("aUtHoRiZaTiOn".into(), "Bearer explicit-token".into())]
                } else {
                    Vec::new()
                },
                ..HttpRequest::default()
            };
            let preview = generated_headers(&request);
            let authorization = preview
                .iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                authorization,
                if explicit {
                    vec![]
                } else {
                    vec!["Bearer [hidden]"]
                }
            );
            assert!(!format!("{preview:?}").contains("actual-token"));
            assert!(!format!("{preview:?}").contains("c2FtOnBhc3M="));

            let server = smol::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut head = Vec::new();

                while !head.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).await.unwrap();
                    head.push(byte[0]);
                    assert!(head.len() < 16 * 1024);
                }

                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();

                String::from_utf8(head).unwrap()
            });
            RequestExecutor::new(&RequestPreferences {
                http_version: HttpVersion::Http1_1,
                timeout_ms: 2_000,
                ..RequestPreferences::default()
            })
            .unwrap()
            .execute(outgoing_request(&request))
            .await
            .unwrap();
            let received = server.await;
            let authorization = received
                .lines()
                .skip(1)
                .filter_map(|line| {
                    let (name, value) = line.split_once(':')?;

                    name.eq_ignore_ascii_case("authorization")
                        .then(|| value.trim())
                })
                .collect::<Vec<_>>();
            let expected = if explicit {
                "Bearer explicit-token"
            } else {
                "Bearer actual-token"
            };

            assert!(received.starts_with("GET / HTTP/1.1\r\n"));
            assert_eq!(authorization, vec![expected]);
            assert!(!received.contains("Basic "));
            assert!(!received.contains("[hidden]"));
            assert_eq!(
                request.authentication,
                Authentication::Bearer {
                    token: "actual-token".into()
                }
            );
        }
    });
}

#[test]
fn auth_previews_mask_helper_values_and_preserve_url_basic_fallback() {
    for (authentication, expected) in [
        (
            Authentication::Basic {
                username: "helper-user".into(),
                password: "helper-password".into(),
            },
            vec![("Authorization", "Basic [hidden]")],
        ),
        (
            Authentication::ApiKey {
                name: "X-Api-Key".into(),
                value: "helper-secret".into(),
                location: ApiKeyLocation::Header,
            },
            vec![
                ("Authorization", "Basic c2FtOnBhc3M="),
                ("X-Api-Key", "[hidden]"),
            ],
        ),
        (
            Authentication::ApiKey {
                name: "api_key".into(),
                value: "helper-secret".into(),
                location: ApiKeyLocation::Query,
            },
            vec![("Authorization", "Basic c2FtOnBhc3M=")],
        ),
        (
            Authentication::None,
            vec![("Authorization", "Basic c2FtOnBhc3M=")],
        ),
    ] {
        let request = HttpRequest {
            path: "http://sam:pass@127.0.0.1:8080/".into(),
            authentication,
            ..HttpRequest::default()
        };
        let preview = generated_headers(&request);
        let auth_headers = preview
            .iter()
            .filter(|(name, _)| {
                name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("x-api-key")
            })
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(auth_headers, expected);
        assert!(preview.iter().all(|(name, _)| name != "api_key"));

        for secret in ["helper-user", "helper-password", "helper-secret"] {
            assert!(!format!("{preview:?}").contains(secret));
        }
    }
}

#[test]
fn explicit_headers_suppress_auth_helper_preview_rows_case_insensitively() {
    for (authentication, name) in [
        (
            Authentication::Basic {
                username: "helper-user".into(),
                password: "helper-password".into(),
            },
            "aUtHoRiZaTiOn",
        ),
        (
            Authentication::Bearer {
                token: "helper-token".into(),
            },
            "aUtHoRiZaTiOn",
        ),
        (
            Authentication::ApiKey {
                name: "X-Api-Key".into(),
                value: "helper-key".into(),
                location: ApiKeyLocation::Header,
            },
            "x-aPi-kEy",
        ),
    ] {
        let request = HttpRequest {
            path: "http://sam:pass@127.0.0.1:8080/".into(),
            authentication,
            headers: vec![(name.into(), "explicit-value".into())],
            ..HttpRequest::default()
        };
        let preview = generated_headers(&request);

        assert!(
            preview
                .iter()
                .all(|(key, _)| !key.eq_ignore_ascii_case(name))
        );
        assert!(!format!("{preview:?}").contains("explicit-value"));
        assert_eq!(outgoing_request(&request).headers, request.headers);
    }
}

#[test]
fn auth_preview_preserves_unresolved_templates_and_hides_credentials() {
    for (authentication, name, expected) in [
        (
            Authentication::Bearer {
                token: "{{token}}".into(),
            },
            "Authorization",
            "Bearer [hidden]",
        ),
        (
            Authentication::Basic {
                username: "{{username}}".into(),
                password: "{{password}}".into(),
            },
            "Authorization",
            "Basic [hidden]",
        ),
        (
            Authentication::ApiKey {
                name: "{{header_name}}".into(),
                value: "{{secret}}".into(),
                location: ApiKeyLocation::Header,
            },
            "{{header_name}}",
            "[hidden]",
        ),
    ] {
        let request = HttpRequest {
            path: "https://{{host}}/{{path}}".into(),
            authentication: authentication.clone(),
            ..HttpRequest::default()
        };
        let preview = generated_headers(&request);

        assert!(preview.contains(&(name.into(), expected.into())));
        assert_eq!(request.authentication, authentication);
        assert_eq!(request.path, "https://{{host}}/{{path}}");
        assert!(request.headers.is_empty());
        assert_eq!(outgoing_request(&request).authentication, authentication);

        for secret in ["{{token}}", "{{username}}", "{{password}}", "{{secret}}"] {
            assert!(!format!("{preview:?}").contains(secret));
        }
    }
}

#[gpui_kit::test]
fn generated_headers_update_count_respect_overrides_and_are_selectable(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let mut draft = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        // The initial untitled tab can render before prepare/focus is called.
        let view = cx.new(|_| RequestDraft::new());
        draft = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    let draft = draft.unwrap();
    let refresh = |cx: &mut gpui_kit::VisualTestContext| cx.update(|window, _| window.refresh());

    refresh(cx);
    assert!(cx.debug_bounds("request-section-Headers-count-2").is_some());
    assert!(cx.debug_bounds("headers-generated-value-0").is_some());
    let url = cx.debug_bounds("request-url").unwrap();
    cx.simulate_click(url.center(), Modifiers::default());
    cx.simulate_input("example.com:8443/path");
    refresh(cx);
    assert!(cx.debug_bounds("request-section-Headers-count-3").is_some());
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).generated_headers[0],
            ("Host".into(), "example.com:8443".into())
        );
        assert!(draft.read(cx).request.headers.is_empty());
    });

    let key = cx.debug_bounds("headers-key-0").unwrap();
    cx.simulate_click(key.center(), Modifiers::default());
    cx.simulate_input("HOST");
    refresh(cx);
    let value = cx.debug_bounds("headers-value-0").unwrap();
    cx.simulate_click(value.center(), Modifiers::default());
    cx.simulate_input("virtual.example");
    cx.read(|cx| {
        assert!(
            draft
                .read(cx)
                .generated_headers
                .iter()
                .all(|(name, _)| name != "Host")
        )
    });

    refresh(cx);
    let enabled = cx.debug_bounds("headers-enabled-0").unwrap();
    cx.simulate_click(enabled.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(draft.read(cx).generated_headers[0].1, "example.com:8443");
        assert!(draft.read(cx).request.headers.is_empty());
    });

    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_method(Method::Post, cx);
            draft.section = RequestSection::Body;
            draft.prepare(window, cx);
            draft.body.as_ref().unwrap().update(cx, |body, cx| {
                body.replace_all("{\"bird\":\"🦅\"}", window, cx)
            });
        })
    });
    cx.read(|cx| {
        let headers = &draft.read(cx).generated_headers;
        assert_eq!(headers[3], ("Content-Length".into(), "15".into()));
        assert_eq!(
            headers[4],
            ("Content-Type".into(), "application/json".into())
        );
    });
    refresh(cx);
    assert!(cx.debug_bounds("request-section-Headers-count-5").is_some());
    let tab = cx.debug_bounds("request-section-Headers").unwrap();
    cx.simulate_click(tab.center(), Modifiers::default());
    refresh(cx);
    let cell = cx.debug_bounds("headers-generated-value-0").unwrap();
    let start = point(cell.left() + px(8.), cell.center().y);
    let end = point(cell.right() - px(8.), cell.center().y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    cx.simulate_keystrokes("secondary-c");
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().unwrap(),
        "example.com:8443"
    );

    cx.update(|_, cx| draft.update(cx, |draft, cx| draft.set_method(Method::Get, cx)));
    cx.read(|cx| {
        assert_eq!(draft.read(cx).generated_headers.len(), 3);
        assert!(
            draft.read(cx).request.body.is_some(),
            "preserve the draft body while GET excludes it"
        );
    });
}
