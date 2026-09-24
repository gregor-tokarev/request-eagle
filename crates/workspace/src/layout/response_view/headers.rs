use gpui_kit::base::SelectableText;
use gpui_kit::component::*;
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::view::ResponseView;

impl ResponseView {
    pub(super) fn headers(&self, cookies_only: bool, cx: &App) -> AnyElement {
        let content = self.content.as_ref().unwrap();
        let (rows, state) = if cookies_only {
            (content.cookies.clone(), self.cookies_list.clone())
        } else {
            (content.headers.clone(), self.headers_list.clone())
        };

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
            .child(
                h_flex()
                    .h_8()
                    .flex_none()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .w(rems(15.))
                            .flex_shrink_0()
                            .px_2()
                            .cursor_text()
                            .child(SelectableText::new(
                                "response-column-name",
                                if cookies_only { "Cookie" } else { "Header" },
                            )),
                    )
                    .child(div().flex_1().px_2().cursor_text().child(
                        SelectableText::new("response-column-value", "Value").document_order(1),
                    )),
            )
            // Variable-height virtualization keeps long values wrapped without
            // registering and laying out every selectable cell on each frame.
            .child(
                list(state, move |index, _, cx| {
                    let (name, value) = &rows[index];

                    h_flex()
                        .id(("response-header-row", index))
                        .w_full()
                        .items_start()
                        .flex_none()
                        .min_h_8()
                        .py_2()
                        .when(index + 1 < rows.len(), |row| row.border_b_1())
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .debug_selector(move || format!("response-header-name-{index}"))
                                .flex()
                                .flex_col()
                                .w(rems(15.))
                                .flex_shrink_0()
                                .px_2()
                                .font_family(cx.theme().mono_font_family.clone())
                                .cursor_text()
                                .child(
                                    SelectableText::new(
                                        ("response-header-name", index),
                                        name.clone(),
                                    )
                                    .document_order((index * 2 + 2) as u64),
                                ),
                        )
                        .child(
                            div()
                                .debug_selector(move || format!("response-header-value-{index}"))
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .px_2()
                                .cursor_text()
                                .child(
                                    SelectableText::new(
                                        ("response-header-value", index),
                                        value.clone(),
                                    )
                                    .document_order((index * 2 + 3) as u64),
                                ),
                        )
                        .into_any_element()
                })
                .flex_1()
                .min_h_0(),
            )
            .into_any_element()
    }
}
