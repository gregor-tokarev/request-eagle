use gpui_kit::component::*;
use gpui_kit::*;

use super::view::ResponseView;

impl ResponseView {
    pub(super) fn headers(&self, cookies_only: bool, cx: &App) -> AnyElement {
        let headers = &self.content.as_ref().unwrap().http().headers;
        let rows: Vec<_> = headers
            .iter()
            .filter(|(name, _)| !cookies_only || name.as_str() == "set-cookie")
            .collect();

        if rows.is_empty() {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(if cookies_only {
                    "No cookies in this response"
                } else {
                    "No response headers"
                })
                .into_any_element();
        }

        v_flex()
            .id("response-header-table")
            .debug_selector(|| "response-header-table".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                h_flex()
                    .h(px(32.))
                    .flex_none()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .w(px(240.))
                            .flex_shrink_0()
                            .px_2()
                            .child(if cookies_only { "Cookie" } else { "Header" }),
                    )
                    .child(div().flex_1().px_2().child("Value")),
            )
            .children(rows.into_iter().map(|(name, value)| {
                let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
                let (name, value) = if cookies_only {
                    value
                        .split_once('=')
                        .map(|(name, value)| (name.to_owned(), value.to_owned()))
                        .unwrap_or(("set-cookie".into(), value))
                } else {
                    (name.to_string(), value)
                };

                h_flex()
                    .items_start()
                    .flex_none()
                    .min_h(px(32.))
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .w(px(240.))
                            .flex_shrink_0()
                            .px_2()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(name),
                    )
                    .child(div().flex_1().min_w_0().px_2().child(value))
            }))
            .into_any_element()
    }
}
