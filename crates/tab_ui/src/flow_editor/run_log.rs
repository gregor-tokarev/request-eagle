use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    scroll::Scrollbar,
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{FlowEditor, run::LogKind};

impl FlowEditor {
    /// What the last run did, block by block, newest last.
    pub(super) fn render_run_log(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let count = self.run.log.len();

        v_flex()
            .debug_selector(|| "flow-run-log".into())
            .flex_none()
            .h(rems(13.))
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background)
            .child(
                h_flex()
                    .flex_none()
                    .h(rems(2.25))
                    .px_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Run log"),
                    )
                    .when(self.run.dropped > 0, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("{} earlier entries not shown", self.run.dropped)),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("flow-close-log")
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::Close).size_3())
                            .accessibility_label("Close the run log")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.log_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .when_some(self.run.error.clone(), |this, error| {
                this.child(
                    div()
                        .debug_selector(|| "flow-run-error".into())
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.danger)
                        .child(error),
                )
            })
            .child(if count == 0 {
                div()
                    .flex_1()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Run the flow to see what each block does.")
                    .into_any_element()
            } else {
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        uniform_list(
                            "flow-run-log-entries",
                            count,
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range.map(|index| this.log_row(index, cx)).collect()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.log_scroll),
                    )
                    .child(Scrollbar::vertical(&self.log_scroll))
                    .into_any_element()
            })
    }

    fn log_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let entry = &self.run.log[index];
        let title = self
            .flow
            .block(&entry.block)
            .map(|block| SharedString::from(block.title().to_owned()));
        let color = match entry.kind {
            LogKind::Ran => theme.foreground,
            LogKind::Logged => theme.info,
            LogKind::Notice => theme.muted_foreground,
            LogKind::Failed => theme.danger,
        };
        let block = entry.block.clone();

        h_flex()
            .id(("flow-log-entry", index))
            .h(rems(1.75))
            .px_3()
            .gap_3()
            .text_xs()
            .hover(|this| this.bg(theme.muted.opacity(0.5)))
            .child(
                div()
                    .flex_none()
                    .w(rems(4.5))
                    .text_color(theme.muted_foreground)
                    .font_family(theme.mono_font_family.clone())
                    .child(format!("{:.3}s", entry.at.as_secs_f64())),
            )
            .child(
                div()
                    .flex_none()
                    .w(rems(9.))
                    .text_ellipsis()
                    .font_weight(FontWeight::MEDIUM)
                    .children(title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .text_color(color)
                    .font_family(theme.mono_font_family.clone())
                    .child(entry.text.clone()),
            )
            .when(!block.is_empty(), |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.flow.block(&block).is_some() {
                            this.set_selection(vec![block.clone()], window, cx);
                            this.reveal(&block, cx);
                        }
                    }))
            })
            .into_any_element()
    }
}
