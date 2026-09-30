use std::{ops::Range, time::Duration};

use gpui_kit::{
    component::{input::EditorState, *},
    prelude::FluentBuilder as _,
    *,
};
use lsp_types::{Documentation, ParameterLabel, SignatureHelp, SignatureInformation};
use request::ScriptPhase;
use ropey::{Rope, extra::esoterica::ropes_are_instances};

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
        // Debounce language-service work without hiding the current help. Replace
        // it only when the latest response arrives, including an empty response.
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

impl ScriptSignature {
    fn render_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
        // The caret's x-coordinate already includes horizontal scrolling.
        let origin = cursor.origin + point(px(0.), editor.scroll_offset().y);
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
            )
            .into_any_element()
    }
}

impl Render for ScriptSignature {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.help.is_none() {
            return Empty.into_any_element();
        }

        let signature = cx.entity();

        // Position after the editor has published this frame's caret geometry.
        deferred(
            canvas(
                move |_, window, cx| {
                    let mut popover =
                        signature.update(cx, |signature, cx| signature.render_popover(window, cx));
                    popover.prepaint_as_root(
                        Point::default(),
                        window.viewport_size().map(AvailableSpace::Definite),
                        window,
                        cx,
                    );
                    popover
                },
                |_, mut popover, window, cx| popover.paint(window, cx),
            )
            .absolute(),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_draft::{
        script_completion_tests::{script_editor, wait_for},
        tests::element_bounds,
    };
    use core::prelude::v1::test;
    use lsp_types::ParameterInformation;

    fn script_signature(
        draft: &Entity<crate::RequestDraft>,
        index: usize,
        cx: &App,
    ) -> Entity<ScriptSignature> {
        let scripts = draft.read(cx).scripts.as_ref().unwrap();
        scripts.read(cx).signatures[index].clone().unwrap()
    }

    #[gpui_kit::test]
    async fn signature_stays_visible_while_typing_and_updates_after_refresh(
        cx: &mut TestAppContext,
    ) {
        let (draft, editor, cx) = script_editor(cx, true);
        let signature = cx.read(|cx| script_signature(&draft, 1, cx));
        cx.simulate_input("pm.test('");
        wait_for(cx, |cx| {
            cx.read(|cx| signature.read(cx).help.is_some() && signature.read(cx).task.is_none())
        })
        .await;

        for text in ["s", "t", "a", "t", "u", "s", "', "] {
            cx.simulate_input(text);
            // The refresh is still debounced. No response can hide the gap.
            cx.read(|cx| assert!(signature.read(cx).task.is_some()));
            assert!(
                element_bounds(cx, "script-signature-help").is_some(),
                "typed {text}"
            );
            cx.executor().advance_clock(Duration::from_millis(25));
            cx.run_until_parked();
            assert!(element_bounds(cx, "script-signature-help").is_some());
        }
        cx.simulate_keystrokes("backspace");
        assert!(element_bounds(cx, "script-signature-help").is_some());
        wait_for(cx, |cx| cx.read(|cx| signature.read(cx).task.is_none())).await;
        cx.read(|cx| {
            let help = signature.read(cx).help.as_ref().unwrap();
            assert_eq!(help.active_parameter, Some(1));
        });

        // A real empty response still closes help when the call ends.
        cx.simulate_input("() => {});");
        wait_for(cx, |cx| cx.read(|cx| signature.read(cx).task.is_none())).await;
        cx.read(|cx| {
            assert!(signature.read(cx).help.is_none());
            assert_eq!(editor.read(cx).value(), "pm.test('status',() => {});");
        });
        assert!(element_bounds(cx, "script-signature-help").is_none());
    }

    #[gpui_kit::test]
    async fn signature_dismissal_cancels_pending_refresh(cx: &mut TestAppContext) {
        let (draft, editor, cx) = script_editor(cx, false);
        let signature = cx.read(|cx| script_signature(&draft, 0, cx));

        for dismiss in ["escape", "selection", "blur"] {
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.focus(window, cx);
                    editor.replace_all("", window, cx);
                    editor.lsp_mut().completion_provider = None;
                });
            });
            cx.simulate_input("pm.crypto.hmacSha256('");
            wait_for(cx, |cx| {
                cx.read(|cx| signature.read(cx).help.is_some() && signature.read(cx).task.is_none())
            })
            .await;
            cx.simulate_input("s");
            assert!(element_bounds(cx, "script-signature-help").is_some());

            match dismiss {
                "escape" => cx.simulate_keystrokes("escape"),
                "selection" => cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| editor.set_selected_range(0..2, cx));
                }),
                _ => cx.update(|window, cx| window.blur(cx)),
            }
            assert!(element_bounds(cx, "script-signature-help").is_none());
            cx.executor().advance_clock(Duration::from_secs(1));
            cx.run_until_parked();
            cx.read(|cx| {
                assert!(signature.read(cx).help.is_none(), "{dismiss}");
                assert!(signature.read(cx).task.is_none(), "{dismiss}");
            });
            assert!(element_bounds(cx, "script-signature-help").is_none());
        }
    }

    #[gpui_kit::test]
    async fn signature_moves_on_the_first_frame(cx: &mut TestAppContext) {
        let (_, editor, cx) = script_editor(cx, true);
        cx.simulate_resize(size(px(1440.), px(1100.)));
        editor.update(cx, |editor, _| editor.lsp_mut().completion_provider = None);

        for theme in ["Default Light", "Default Dark"] {
            for font_size in [12., 16., 24.] {
                cx.update(|_, cx| {
                    assert!(request_eagle_theme::apply(theme, cx));
                    Theme::global_mut(cx).font_size = px(font_size);
                    Theme::sync_base(cx);
                });
                cx.simulate_keystrokes("secondary-a");
                cx.simulate_input("\n\n\n\npm.test('");
                wait_for(cx, |cx| {
                    element_bounds(cx, "script-signature-help").is_some()
                })
                .await;

                for text in ["s", "t", "a", "t", "u", "s", "',\n"] {
                    cx.update(|window, cx| {
                        editor.update(cx, |editor, cx| {
                            editor.replace_text_in_range(None, text, window, cx);
                        });
                        window.refresh();
                        // Check the first draw, before observer notifications or a catch-up frame.
                        window.draw(cx).clear(cx);

                        let editor = editor.read(cx);
                        let (caret, _) = editor.cursor_layout().unwrap();
                        let expected = window.pixel_snap_point(
                            caret.origin + point(px(0.), editor.scroll_offset().y)
                                - point(px(0.), rems(0.25).to_pixels(window.rem_size())),
                        ).scale(window.scale_factor());
                        let background = Background::from(cx.theme().popover);
                        let popovers = window.painted_quads().iter()
                            .filter(|quad| quad.background == background)
                            .map(|quad| point(quad.bounds.left(), quad.bounds.bottom()))
                            .collect::<Vec<_>>();
                        assert!(popovers.iter().any(|origin| {
                            (origin.x - expected.x).0.abs() <= 1.
                                && (origin.y - expected.y).0.abs() <= 1.
                        }), "{theme}, {font_size}px, typed {text}: expected {expected:?}, got {popovers:?}");
                    });
                }

                cx.simulate_resize(size(px(1000.), px(800.)));
                // A smaller editor may clip the caret; help must then disappear.
                let bounds = element_bounds(cx, "script-signature-help");
                let visible = cx.read(|cx| {
                    let editor = editor.read(cx);
                    editor.cursor_layout().is_some_and(|(caret, height)| {
                        editor.input_bounds().contains(
                            &(caret.origin + point(px(0.), editor.scroll_offset().y + height / 2.)),
                        )
                    })
                });
                assert_eq!(bounds.is_some(), visible);
                if let Some(bounds) = bounds {
                    assert!(bounds.left() >= px(0.) && bounds.right() <= px(1000.));
                    assert!(bounds.top() >= px(0.) && bounds.bottom() <= px(800.));
                }
                cx.simulate_resize(size(px(1440.), px(1100.)));
            }
        }
    }

    #[gpui_kit::test]
    async fn signature_follows_horizontal_scroll_and_hides_offscreen(cx: &mut TestAppContext) {
        let (draft, editor, cx) = script_editor(cx, true);
        let signature = cx.read(|cx| script_signature(&draft, 1, cx));
        cx.simulate_resize(size(px(2400.), px(1000.)));
        let source = format!("{}pm.test('{}');", "// line\n".repeat(80), "s".repeat(500));
        let cursor = source.len() - 303;
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.lsp_mut().completion_provider = None;
                editor.set_soft_wrap(false, window, cx);
                editor.replace_all(&source, window, cx);
                editor.set_selected_range(cursor..cursor, cx);
            });
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                let height = editor.line_height().unwrap();
                editor.set_scroll_offset(point(-px(800.), -height * 76.), cx);
            });
        });
        wait_for(cx, |cx| {
            cx.read(|cx| signature.read(cx).help.is_some() && signature.read(cx).task.is_none())
        })
        .await;
        let bounds = element_bounds(cx, "script-signature-help").unwrap();
        cx.read(|cx| {
            let editor = editor.read(cx);
            assert!(editor.scroll_offset().x < px(0.));
            assert!(editor.scroll_offset().y < px(0.));
            let (caret, _) = editor.cursor_layout().unwrap();
            assert!((bounds.left() - caret.origin.x).abs() <= px(1.));
        });

        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_scroll_offset(Point::default(), cx)
            });
        });
        assert!(element_bounds(cx, "script-signature-help").is_none());
    }

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
