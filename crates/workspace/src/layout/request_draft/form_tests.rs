use gpui_kit::{Modifiers, TestAppContext};
use request::{FormBody, HttpRequest, Method, MultipartField};

use super::{
    draft::RequestSection,
    execution::{generated_headers, outgoing_request},
    tests::{draft, element_bounds},
};

#[gpui_kit::test]
fn form_controls_preserve_modes_and_choose_upload_files(cx: &mut TestAppContext) {
    let (draft, cx) = draft(cx);
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_method(Method::Patch, cx);
            draft.section = RequestSection::Body;
            draft.request.body = Some(b"{\"raw\":true}".to_vec());
            draft.prepare(window, cx);
        })
    });

    let form = element_bounds(cx, "request-body-mode-urlencoded").unwrap();
    cx.simulate_click(form.center(), Modifiers::default());
    let name = element_bounds(cx, "form-name-0").unwrap();
    cx.simulate_click(name.center(), Modifiers::default());
    cx.simulate_input("message");
    let value = element_bounds(cx, "form-value-0").unwrap();
    cx.simulate_click(value.center(), Modifiers::default());
    cx.simulate_input("hello & goodbye");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.form,
            Some(FormBody::UrlEncoded(vec![(
                "message".into(),
                "hello & goodbye".into()
            )]))
        )
    });

    let multipart = element_bounds(cx, "request-body-mode-multipart").unwrap();
    cx.simulate_click(multipart.center(), Modifiers::default());
    let add = element_bounds(cx, "add-form-file").unwrap();
    cx.simulate_click(add.center(), Modifiers::default());
    let name = element_bounds(cx, "form-name-1").unwrap();
    cx.simulate_click(name.center(), Modifiers::default());
    cx.simulate_input("attachment");
    let browse = element_bounds(cx, "form-browse-1").unwrap();
    cx.simulate_click(browse.center(), Modifiers::default());
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files);
        assert!(!options.directories);
        assert!(!options.multiple);
        Some(vec!["/tmp/upload.bin".into()])
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.form,
            Some(FormBody::Multipart(vec![MultipartField::File {
                name: "attachment".into(),
                path: "/tmp/upload.bin".into(),
            }]))
        )
    });

    let raw = element_bounds(cx, "request-body-mode-raw").unwrap();
    cx.simulate_click(raw.center(), Modifiers::default());
    cx.read(|cx| {
        assert!(draft.read(cx).request.form.is_none());
        assert_eq!(
            draft.read(cx).request.body.as_deref(),
            Some(b"{\"raw\":true}".as_slice())
        );
    });
    let form = element_bounds(cx, "request-body-mode-urlencoded").unwrap();
    cx.simulate_click(form.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.form,
            Some(FormBody::UrlEncoded(vec![(
                "message".into(),
                "hello & goodbye".into()
            )]))
        )
    });
}

#[test]
fn form_preview_uses_encoded_length_and_does_not_default_to_json() {
    let request = HttpRequest {
        method: Method::Patch,
        path: "example.com".into(),
        headers: vec![
            ("Content-Type".into(), "application/json".into()),
            ("Content-Length".into(), "999".into()),
        ],
        body: Some(b"stale raw text".to_vec()),
        form: Some(FormBody::UrlEncoded(vec![("a b".into(), "c+d".into())])),
        ..HttpRequest::default()
    };
    let headers = generated_headers(&request);
    assert!(headers.contains(&(
        "Content-Type".into(),
        "application/x-www-form-urlencoded".into()
    )));
    assert!(headers.contains(&("Content-Length".into(), "9".into())));
    assert_eq!(outgoing_request(&request).form, request.form);

    for method in [Method::Get, Method::Head] {
        let mut request = request.clone();
        request.method = method;
        assert!(outgoing_request(&request).form.is_none());
        assert!(outgoing_request(&request).body.is_none());
    }
}

fn check_multiline_form(form: FormBody, selector: &'static str, cx: &mut TestAppContext) {
    let (draft, cx) = draft(cx);
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.request.method = Method::Post;
            draft.request.form = Some(form.clone());
            draft.saved_request = draft.request.clone();
            draft.section = RequestSection::Body;
            draft.prepare(window, cx);
        })
    });

    let mode = element_bounds(cx, selector).unwrap();
    cx.simulate_click(mode.center(), Modifiers::default());
    cx.read(|cx| {
        assert_eq!(draft.read(cx).request.form.as_ref(), Some(&form));
        assert!(!draft.read(cx).is_dirty());
    });

    let add = element_bounds(cx, "add-form-text").unwrap();
    cx.simulate_click(add.center(), Modifiers::default());
    let value = element_bounds(cx, "form-value-1").unwrap();
    cx.simulate_click(value.center(), Modifiers::default());
    cx.simulate_input("new\r\nmultiline\nvalue");

    let mut expected = form.clone();
    match &mut expected {
        FormBody::UrlEncoded(fields) => fields.push(("".into(), "new\r\nmultiline\nvalue".into())),
        FormBody::Multipart(fields) => fields.push(MultipartField::Text {
            name: "".into(),
            value: "new\r\nmultiline\nvalue".into(),
        }),
    }

    cx.read(|cx| assert_eq!(draft.read(cx).request.form.as_ref(), Some(&expected)));
    let raw = element_bounds(cx, "request-body-mode-raw").unwrap();
    cx.simulate_click(raw.center(), Modifiers::default());
    let mode = element_bounds(cx, selector).unwrap();
    cx.simulate_click(mode.center(), Modifiers::default());
    cx.read(|cx| assert_eq!(draft.read(cx).request.form.as_ref(), Some(&expected)));
}

#[gpui_kit::test]
fn urlencoded_form_preserves_saved_and_edited_line_breaks(cx: &mut TestAppContext) {
    check_multiline_form(
        FormBody::UrlEncoded(vec![(
            "field\r\nname".into(),
            "first\r\nsecond\nthird".into(),
        )]),
        "request-body-mode-urlencoded",
        cx,
    );
}

#[gpui_kit::test]
fn multipart_form_preserves_saved_and_edited_line_breaks(cx: &mut TestAppContext) {
    check_multiline_form(
        FormBody::Multipart(vec![MultipartField::Text {
            name: "field\r\nname".into(),
            value: "first\r\nsecond\nthird".into(),
        }]),
        "request-body-mode-multipart",
        cx,
    );
}
