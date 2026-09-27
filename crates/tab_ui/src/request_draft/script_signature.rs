use std::{ops::Range, time::Duration};

use gpui_kit::{
    component::{input::EditorState, *},
    prelude::FluentBuilder as _,
    *,
};
use lsp_types::{Documentation, ParameterLabel, SignatureHelp, SignatureInformation};
use request::ScriptPhase;
use ropey::{Rope, extra::esoterica::ropes_are_instances};

use crate::script_intelligence;

pub(super) struct ScriptSignature {
    editor: Entity<EditorState>,
    phase: ScriptPhase,
    snapshot: Option<(Rope, usize)>,
    help: Option<SignatureHelp>,
    task: Option<Task<()>>,
    _subscription: Subscription,
}

impl ScriptSignature {
    pub(super) fn new(
        editor: Entity<EditorState>,
        phase: ScriptPhase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe_in(&editor, window, |this, _, window, cx| {
            this.refresh(window, cx);
        });

        Self {
            editor,
            phase,
            snapshot: None,
            help: None,
            task: None,
            _subscription: subscription,
        }
    }

    pub(super) fn dismiss(&mut self, cx: &mut Context<Self>) -> bool {
        let visible = self.help.take().is_some() || self.task.is_some();
        self.task = None;
        cx.notify();
        visible
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        if !editor.focus_handle(cx).is_focused(window) || !editor.selected_range().is_empty() {
            self.snapshot = None;
            self.help = None;
            self.task = None;
            cx.notify();
            return;
        }

        let offset = editor.cursor();
        if self.snapshot.as_ref().is_some_and(|(text, cursor)| {
            *cursor == offset && ropes_are_instances(text, editor.text())
        }) {
            // The source is unchanged, but scrolling can move the popup's anchor.
            cx.notify();
            return;
        }

        let text = editor.text().clone();
        self.snapshot = Some((text.clone(), offset));
        self.help = None;
        let phase = self.phase;
        self.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(75))
                .await;
            let help = script_intelligence::signature_help(text.to_string(), offset, phase).await;
            let _ = this.update(cx, |this, cx| {
                let current = this.snapshot.as_ref().is_some_and(|(current, cursor)| {
                    *cursor == offset && ropes_are_instances(current, &text)
                });
                if current {
                    this.help = help.ok().flatten();
                    this.task = None;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }
}

fn active_parameter_range(signature: &SignatureInformation, active: usize) -> Option<Range<usize>> {
    let parameters = signature.parameters.as_ref()?;
    let parameter = parameters.get(active.min(parameters.len().checked_sub(1)?))?;
    match &parameter.label {
        ParameterLabel::Simple(label) => {
            let start = signature.label.find(label)?;
            Some(start..start + label.len())
        }
        ParameterLabel::LabelOffsets([start, end]) => {
            let byte_offset = |offset: u32| {
                let mut utf16 = 0;
                for (byte, ch) in signature.label.char_indices() {
                    if utf16 == offset {
                        return Some(byte);
                    }
                    utf16 += ch.len_utf16() as u32;
                }
                (utf16 == offset).then_some(signature.label.len())
            };
            Some(byte_offset(*start)?..byte_offset(*end)?)
        }
    }
}

impl Render for ScriptSignature {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.read(cx);
        let Some(help) = &self.help else {
            return Empty.into_any_element();
        };
        if !editor.focus_handle(cx).is_focused(window) || !editor.selected_range().is_empty() {
            return Empty.into_any_element();
        }
        let Some(signature) = help
            .signatures
            .get(help.active_signature.unwrap_or(0) as usize)
        else {
            return Empty.into_any_element();
        };
        let Some((cursor, line_height)) = editor.cursor_layout() else {
            return Empty.into_any_element();
        };
        let origin = cursor.origin + editor.scroll_offset();
        if !editor
            .input_bounds()
            .contains(&(origin + point(px(0.), line_height / 2.)))
        {
            return Empty.into_any_element();
        }

        let active = signature
            .active_parameter
            .or(help.active_parameter)
            .unwrap_or(0) as usize;
        let highlights = active_parameter_range(signature, active)
            .into_iter()
            .map(|range| {
                (
                    range,
                    HighlightStyle {
                        color: Some(cx.theme().primary),
                        font_weight: Some(FontWeight::BOLD),
                        ..Default::default()
                    },
                )
            })
            .collect::<Vec<_>>();
        let documentation = signature
            .parameters
            .as_ref()
            .and_then(|parameters| parameters.get(active.min(parameters.len().saturating_sub(1))))
            .and_then(|parameter| parameter.documentation.as_ref())
            .or(signature.documentation.as_ref())
            .map(|documentation| match documentation {
                Documentation::String(value) => value.clone(),
                Documentation::MarkupContent(value) => value.value.clone(),
            })
            .filter(|documentation| !documentation.is_empty());
        let margin = rems(0.5).to_pixels(window.rem_size());
        let width = rems(36.)
            .to_pixels(window.rem_size())
            .min((window.bounds().size.width - margin * 2.).max(px(0.)));

        deferred(
            anchored()
                .anchor(Anchor::BottomLeft)
                .position(origin - point(px(0.), rems(0.25).to_pixels(window.rem_size())))
                .snap_to_window_with_margin(margin)
                .child(
                    v_flex()
                        .id("script-signature-help")
                        .debug_selector(|| "script-signature-help".into())
                        .role(Role::Tooltip)
                        .aria_label("Function parameters")
                        .w(width)
                        .max_h(rems(12.))
                        .overflow_y_scroll()
                        .p_2()
                        .gap_1()
                        .text_xs()
                        .bg(cx.theme().popover)
                        .text_color(cx.theme().popover_foreground)
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius_tokens().lg)
                        .shadow_md()
                        .occlude()
                        .child(
                            div()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(
                                    StyledText::new(signature.label.clone())
                                        .with_highlights(highlights),
                                ),
                        )
                        .when_some(documentation, |view, documentation| {
                            view.child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(documentation),
                            )
                        }),
                ),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::ParameterInformation;

    #[core::prelude::v1::test]
    fn parameter_highlight_uses_utf16_offsets_without_splitting_unicode() {
        let label = "f(🦅: string, value: number)";
        let start = label.find("value").unwrap();
        let end = label.find(')').unwrap();
        let signature = SignatureInformation {
            label: label.into(),
            documentation: None,
            parameters: Some(vec![ParameterInformation {
                label: ParameterLabel::LabelOffsets([
                    label[..start].encode_utf16().count() as u32,
                    label[..end].encode_utf16().count() as u32,
                ]),
                documentation: None,
            }]),
            active_parameter: None,
        };
        assert_eq!(active_parameter_range(&signature, 0), Some(start..end));
    }
}
