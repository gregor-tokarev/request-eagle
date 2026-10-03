use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    scroll::Scrollbar,
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::{
    FlowEditor,
    run::{LogKind, RunData},
};

impl FlowEditor {
    /// The entries the run log shows: all of them, or the selected block's.
    pub(super) fn log_entries(&self) -> Vec<usize> {
        let filter = match self.selection.as_slice() {
            [block] if self.log_filtered => Some(block),
            _ => None,
        };

        (0..self.run.log.len())
            .filter(|&index| filter.is_none_or(|block| self.run.log[index].block == *block))
            .collect()
    }

    /// What the last run did, block by block, newest last.
    pub(super) fn render_run_log(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entries = self.log_entries();
        let count = entries.len();
        let filterable = self.selection.len() == 1;

        v_flex()
            .debug_selector(|| "flow-run-log".into())
            .size_full()
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
                    .when(filterable, |this| {
                        this.child(
                            Button::new("flow-log-filter")
                                .debug_selector(|| "flow-log-filter".into())
                                .xsmall()
                                .ghost()
                                .selected(self.log_filtered)
                                .label("Selected block only")
                                .tooltip("Show only the entries of the selected block")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.log_filtered = !this.log_filtered;
                                    cx.notify();
                                })),
                        )
                    })
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
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .filter_map(|row| entries.get(row).copied())
                                    .map(|index| this.log_row(index, cx))
                                    .collect()
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
            .map(|block| self.block_title(block, cx));
        // A block that ran more than once, such as in a loop, numbers its runs.
        let number = (entry.index > 0
            && self
                .run
                .blocks
                .get(&entry.block)
                .is_some_and(|status| status.runs > 1))
        .then(|| format!("#{}", entry.index));
        let color = match entry.kind {
            LogKind::Ran => theme.foreground,
            LogKind::Logged => theme.info,
            LogKind::Notice => theme.muted_foreground,
            LogKind::Failed => theme.danger,
        };
        let block = entry.block.clone();
        let (run, released) = match &entry.run {
            RunData::Kept(run, _) => (Some(run.clone()), false),
            // A block's last run stays with its status after its entry lets
            // go of it.
            RunData::Released => match self
                .run
                .blocks
                .get(&entry.block)
                .filter(|status| status.runs == entry.index)
                .and_then(|status| status.last.clone())
            {
                Some(last) => (Some(last), false),
                None => (None, true),
            },
            RunData::None => (None, false),
        };
        let at = entry.at;

        h_flex()
            .id(("flow-log-entry", index))
            .debug_selector(move || format!("flow-log-entry-{index}"))
            .w_full()
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
                h_flex()
                    .flex_none()
                    .w(rems(11.))
                    .gap_1()
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .font_weight(FontWeight::MEDIUM)
                            .children(title),
                    )
                    .children(number.map(|number| {
                        div()
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .child(number)
                    })),
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
                            match &run {
                                Some(run) => this.show_run(&block, run.clone(), at, window, cx),
                                None if released => this.show_released(&block, at, cx),
                                None => {}
                            }
                        }
                    }))
            })
            .into_any_element()
    }
}
