use request::{Body, Field, FormPart, HttpRequest, Method, RawLanguage};

use gpui_kit::EntityId;

use crate::cite::{Cite, CiteOrigin, CiteSource, http_message, quote};

#[test]
fn quotes_the_selection_with_the_lines_it_spans() {
    let text = "{\n  \"error\": {\n    \"code\": \"INVALID_ID\"\n  }\n}";
    let start = text.find("\"code\"").unwrap();
    let end = start + "\"code\": \"INVALID_ID\"".len();

    assert_eq!(
        quote(text, start..end),
        Some(("\"code\": \"INVALID_ID\"".to_owned(), Some((3, 3))))
    );
    assert_eq!(
        quote(text, 2..end),
        Some((text[2..end].to_owned(), Some((2, 3))))
    );
}

#[test]
fn a_selection_ending_at_a_line_start_ends_on_the_line_before() {
    let text = "one\ntwo\nthree";

    assert_eq!(quote(text, 4..8), Some(("two\n".to_owned(), Some((2, 2)))));
}

#[test]
fn quotes_everything_without_a_selection_and_nothing_from_blank_text() {
    assert_eq!(
        quote("https://api.example.com", 5..5),
        Some(("https://api.example.com".to_owned(), None))
    );
    assert_eq!(quote("  \n ", 0..0), None);
    assert_eq!(quote("a   b", 1..4), None);
}

#[test]
fn single_line_text_has_no_lines() {
    assert_eq!(
        quote("{{base_url}}/users", 0..12),
        Some(("{{base_url}}".to_owned(), None))
    );
}

#[test]
fn writes_a_request_as_the_message_it_sends() {
    let request = HttpRequest {
        method: Method::Post,
        path: "{{base_url}}/users".into(),
        headers: vec![
            Field::new("Authorization", "Bearer {{token}}"),
            Field {
                enabled: false,
                ..Field::new("X-Debug", "1")
            },
        ],
        body: Some(Body::Raw {
            language: RawLanguage::Json,
            text: "{\"name\": \"Ada\"}".into(),
        }),
        ..HttpRequest::default()
    };

    assert_eq!(
        http_message(&request),
        "POST {{base_url}}/users\nAuthorization: Bearer {{token}}\n\n{\"name\": \"Ada\"}"
    );
}

#[test]
fn names_the_files_a_form_sends() {
    let request = HttpRequest {
        method: Method::Put,
        path: "https://example.com/avatar".into(),
        body: Some(Body::Multipart {
            parts: vec![
                FormPart {
                    name: "user".into(),
                    value: "42".into(),
                    file: false,
                },
                FormPart {
                    name: "avatar".into(),
                    value: "photo.png".into(),
                    file: true,
                },
            ],
        }),
        ..HttpRequest::default()
    };

    assert_eq!(
        http_message(&request),
        "PUT https://example.com/avatar\n\nmultipart/form-data:\nuser: 42\navatar: file photo.png"
    );
}

fn cite(source: CiteSource, text: &str, lines: Option<(usize, usize)>) -> Cite {
    Cite {
        source,
        text: text.to_owned(),
        language: None,
        lines,
        origin: CiteOrigin {
            page: EntityId::from(1u64),
            name: "Get user".into(),
            request: "GET {{base_url}}/users/42".into(),
            status: None,
            file: None,
            unsaved_changes: false,
        },
    }
}

#[test]
fn names_a_cite_by_its_lines_or_its_short_text() {
    assert_eq!(
        cite(CiteSource::ResponseBody, "\"code\": 1", Some((3, 4))).label(),
        "Response body · L3–4"
    );
    assert_eq!(
        cite(CiteSource::Script, "pm.test()", Some((7, 7))).label(),
        "Script · L7"
    );
    assert_eq!(
        cite(CiteSource::Url, "/users/42", None).label(),
        "URL · /users/42"
    );
    assert_eq!(
        cite(CiteSource::Url, "{{base_url}}/users/42?expand=orders", None).label(),
        "URL"
    );
    assert_eq!(
        cite(CiteSource::Request, "GET /\nAccept: */*", None).label(),
        "Request"
    );
}
