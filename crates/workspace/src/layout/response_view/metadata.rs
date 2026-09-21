use std::time::Duration;

use gpui_kit::base::SelectableText;
use gpui_kit::component::{hover_card::HoverCard, *};
use gpui_kit::*;
use request::StatusCode;

use super::{
    content::size_label,
    timing::{duration_label, timing_details},
    view::ResponseView,
};

impl ResponseView {
    pub(super) fn metadata(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let content = self.content.as_ref().unwrap();
        let response = content.http();
        let status = response.status;
        let metrics = response.metrics;
        let body_bytes = response.body.len();
        let processing = content.processing;
        let elapsed = content.execution.elapsed + processing;
        let encoded = response
            .headers
            .get("content-encoding")
            .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"));
        let color = if status.is_success() {
            cx.theme().success
        } else if status.is_redirection() {
            cx.theme().warning
        } else if status.is_informational() {
            cx.theme().info
        } else {
            cx.theme().danger
        };
        let focus = self.focus.clone();
        let time_focus = focus.clone();
        let size_focus = focus.clone();
        let owner = cx.entity_id();

        h_flex()
            .debug_selector(|| "response-metadata".into())
            .gap_2()
            .text_size(px(12.))
            .text_color(cx.theme().muted_foreground)
            .child(
                HoverCard::new("response-status-details")
                    .anchor(Anchor::TopRight)
                    .open_delay(Duration::from_millis(250))
                    .trigger(div()
                        .debug_selector(|| "response-status".into())
                        .px_2().py_1().rounded(px(5.))
                        .bg(color.opacity(0.15)).text_color(color)
                        .font_weight(FontWeight::SEMIBOLD).cursor_text()
                        .child(SelectableText::new("response-status-text", status.to_string())))
                    .content(move |_, _, cx| {
                        panel("response-status-overlay", focus.clone(), owner, cx)
                            .w(px(340.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).text_size(px(15.))
                                .child(SelectableText::new("status-title", status.to_string())))
                            .child(SelectableText::new("status-description", status_description(status)).document_order(1))
                    }),
            )
            .child("•")
            .child(
                HoverCard::new("response-time-details")
                    .anchor(Anchor::TopRight)
                    .open_delay(Duration::from_millis(250))
                    .trigger(div().debug_selector(|| "response-time".into()).cursor_text()
                        .child(SelectableText::new("response-time-text", duration_label(elapsed)).document_order(1)))
                    .content(move |_, _, cx| {
                        panel("response-time-overlay", time_focus.clone(), owner, cx)
                            .w(px(580.))
                            .child(timing_details(metrics, processing, elapsed, cx))
                    }),
            )
            .child("•")
            .child(
                HoverCard::new("response-size-details")
                    .anchor(Anchor::TopRight)
                    .open_delay(Duration::from_millis(250))
                    .trigger(div().debug_selector(|| "response-size".into()).cursor_text()
                        .child(SelectableText::new("response-size-text", size_label(body_bytes + metrics.response_header_bytes)).document_order(2)))
                    .content(move |_, _, cx| {
                        panel("response-size-overlay", size_focus.clone(), owner, cx)
                            .w(px(370.))
                            .child(detail_row("response-total", 0, "Response size", bytes_label(body_bytes + metrics.response_header_bytes)).font_weight(FontWeight::SEMIBOLD))
                            .child(detail_row("response-headers", 1, "Headers (estimated)", bytes_label(metrics.response_header_bytes)))
                            .child(detail_row("response-body", 2, "Downloaded body", bytes_label(body_bytes)))
                            .child(detail_row("response-decoded", 3, "Uncompressed", if encoded { "Not decoded".to_owned() } else { bytes_label(body_bytes) }))
                            .child(div().h(px(1.)).my_1().bg(cx.theme().border))
                            .child(detail_row("request-total", 4, "Request size (known)", bytes_label(metrics.request_header_bytes + metrics.request_body_bytes)).font_weight(FontWeight::SEMIBOLD))
                            .child(detail_row("request-headers", 5, "Configured headers", bytes_label(metrics.request_header_bytes)))
                            .child(detail_row("request-body", 6, "Body", bytes_label(metrics.request_body_bytes)))
                            .child(div().pt_2().text_size(px(11.)).text_color(cx.theme().muted_foreground)
                                .child(SelectableText::new("size-note", "Header sizes use name: value text, excluding HTTP framing and header compression. Automatically added request headers are not included.").document_order(14)))
                    }),
            )
            .child("•")
            .child(div().cursor_text().child(SelectableText::new("response-version", format!("{:?}", response.version)).document_order(3)))
    }
}

fn panel(id: &'static str, focus: FocusHandle, owner: EntityId, cx: &App) -> Div {
    v_flex()
        .debug_selector(move || id.to_owned())
        .gap_3()
        .p_3()
        .text_size(px(13.))
        .text_color(cx.theme().foreground)
        .capture_any_mouse_down(move |event: &MouseDownEvent, window, cx| {
            if event.button == MouseButton::Left {
                window.focus(&focus, cx);
                cx.notify(owner);
            }
        })
        .on_mouse_move(move |event, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                cx.notify(owner);
            }
        })
}

fn detail_row(id: &'static str, order: u64, label: &'static str, value: String) -> Div {
    h_flex()
        .debug_selector(move || format!("detail-{id}"))
        .gap_3()
        .child(
            div()
                .flex_1()
                .cursor_text()
                .child(SelectableText::new((id, 0usize), label).document_order(order * 2)),
        )
        .child(
            div()
                .cursor_text()
                .child(SelectableText::new((id, 1usize), value).document_order(order * 2 + 1)),
        )
}

fn bytes_label(bytes: usize) -> String {
    if bytes < 1024 {
        size_label(bytes)
    } else {
        format!("{} ({bytes} B)", size_label(bytes))
    }
}

fn status_description(status: StatusCode) -> &'static str {
    match status.as_u16() {
        200 => "Request successful. The server returned the requested response.",
        201 => "Request successful. A new resource was created.",
        202 => "The request was accepted for processing, which may not be complete yet.",
        204 => "Request successful. The server returned no response body.",
        301 | 308 => "The resource has moved permanently. See the Location response header.",
        302 | 303 | 307 => "The server redirected this request. See the Location response header.",
        304 => "The resource has not changed. A cached representation can be used.",
        400 => "The server could not process the request because it was invalid.",
        401 => "Authentication is required or the supplied credentials were rejected.",
        403 => "The server refused access to the requested resource.",
        404 => "The server could not find the requested resource.",
        405 => "This HTTP method is not allowed for the resource. See the Allow header.",
        408 => "The server timed out waiting for the request.",
        409 => "The request conflicts with the current state of the resource.",
        413 => "The request body exceeds the server's size limit.",
        415 => "The server does not support this request's media type.",
        422 => "The server understood the request format but could not process its contents.",
        429 => "Too many requests. Check the Retry-After header before retrying.",
        500 => "The server encountered an unexpected error while processing the request.",
        502 => "The gateway received an invalid response from an upstream server.",
        503 => "The server is temporarily unable to handle the request.",
        504 => "The gateway timed out waiting for an upstream server.",
        _ if status.is_informational() => "An informational response from the server.",
        _ if status.is_success() => "The server successfully processed the request.",
        _ if status.is_redirection() => "The server returned a redirection response.",
        _ if status.is_client_error() => {
            "The server rejected the request. Inspect the response for details."
        }
        _ if status.is_server_error() => {
            "The server failed to complete the request. Inspect the response for details."
        }
        _ => "The server returned a non-standard HTTP status code.",
    }
}
