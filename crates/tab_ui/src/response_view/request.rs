use gpui_kit::base::SelectableText;
use gpui_kit::component::{scroll::ScrollableElement as _, *};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{Body, Field, HttpRequest};

use super::content::ResponseContent;
use super::view::ResponseView;

impl ResponseView {
    /// The request as it went out: its address, headers and body.
    pub(super) fn sent_request(&self, content: &ResponseContent, cx: &App) -> AnyElement {
        let Some(sent) = content.execution.sent.as_ref() else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("The request was not kept")
                .into_any_element();
        };

        let metrics = content.http().metrics;
        let url = sent_url(sent);

        v_flex()
            .id("sent-request")
            .debug_selector(|| "sent-request".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .px_2()
            .pb_3()
            .gap_4()
            .child(section(
                "General",
                vec![
                    ("URL".into(), url.clone().into()),
                    ("Method".into(), sent.method.as_str().into()),
                ],
                0,
                cx,
            ))
            .child(section(
                "Request Headers",
                // The request's own headers, then those sending adds. The
                // cookie jar's Cookie header is left out.
                Field::enabled(&sent.headers)
                    .map(|(name, value)| (name.to_owned(), value.to_owned()))
                    .chain(request::generated_headers(
                        sent.method,
                        &url,
                        &sent.headers,
                        metrics.request_body_bytes,
                    ))
                    .map(|(name, value)| (name.into(), value.into()))
                    .collect(),
                1,
                cx,
            ))
            .child(body(sent, metrics.request_body_bytes, cx))
            .into_any_element()
    }
}

/// Where the request went, with any parameters a script added after the URL.
pub(crate) fn sent_url(request: &HttpRequest) -> String {
    let mut request = request.clone();
    request.inline_query();

    request.path
}

fn title(text: &'static str, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

/// Named values, one per row, as headers are written.
fn section(
    heading: &'static str,
    rows: Vec<(SharedString, SharedString)>,
    order: usize,
    cx: &App,
) -> Div {
    v_flex()
        .gap_1()
        .child(title(heading, cx))
        .when(rows.is_empty(), |section| {
            section.child(div().text_color(cx.theme().muted_foreground).child("None"))
        })
        .children(rows.into_iter().enumerate().map(|(index, (name, value))| {
            h_flex()
                .items_start()
                .gap_2()
                .font_family(cx.theme().mono_font_family.clone())
                .child(
                    div()
                        .flex_none()
                        .text_color(cx.theme().muted_foreground)
                        .child(SelectableText::new(
                            ("sent-request-name", order * 1000 + index),
                            format!("{name}:"),
                        )),
                )
                .child(div().flex_1().min_w_0().child(SelectableText::new(
                    ("sent-request-value", order * 1000 + index),
                    value,
                )))
        }))
}

/// `size` is how many bytes the body had; the execution keeps only the
/// start of a large raw body.
fn body(request: &HttpRequest, size: usize, cx: &App) -> Div {
    match &request.body {
        None => v_flex().gap_1().child(title("Request Body", cx)).child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child("This request has no body"),
        ),
        Some(Body::Raw { text, .. }) => v_flex()
            .gap_1()
            .child(title("Request Body", cx))
            .child(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(SelectableText::new("sent-request-body", text.clone())),
            )
            .when(text.len() < size, |body| {
                body.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "Showing the first {} of {}.",
                            super::metadata::size_label(text.len()),
                            super::metadata::size_label(size)
                        )),
                )
            }),
        Some(Body::UrlEncoded { fields }) => section(
            "Request Body",
            fields
                .iter()
                .map(|(name, value)| (name.clone().into(), value.clone().into()))
                .collect(),
            2,
            cx,
        ),
        Some(Body::Multipart { parts }) => section(
            "Request Body",
            parts
                .iter()
                .map(|part| {
                    let value = if part.file {
                        format!("File {}", part.value)
                    } else {
                        part.value.clone()
                    };
                    (part.name.clone().into(), value.into())
                })
                .collect(),
            2,
            cx,
        ),
        Some(Body::Binary { file }) => section(
            "Request Body",
            vec![("File".into(), file.to_string_lossy().into_owned().into())],
            2,
            cx,
        ),
    }
}
