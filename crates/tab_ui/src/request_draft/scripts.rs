use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::{Editor, EditorState, InputEvent},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::ScriptPhase;

use super::RequestDraft;

const PRE_SNIPPETS: &[(&str, &str)] = &[
    ("Set a variable", "pm.variables.set(\"name\", \"value\");"),
    (
        "Set a request header",
        "pm.request.headers.upsert({\n    key: \"X-Request-Id\",\n    value: String(Date.now())\n});",
    ),
    (
        "Log a message",
        "console.log(\"Sending\", pm.request.method, pm.request.url);",
    ),
];
const POST_SNIPPETS: &[(&str, &str)] = &[
    (
        "Status code is 200",
        "pm.test(\"Status code is 200\", function () {\n    pm.response.to.have.status(200);\n});",
    ),
    (
        "Check a JSON value",
        "pm.test(\"Response has the expected value\", function () {\n    const data = pm.response.json();\n    pm.expect(data).to.have.property(\"success\", true);\n});",
    ),
    (
        "Response time is below 1 second",
        "pm.test(\"Response time is below 1 second\", function () {\n    pm.expect(pm.response.responseTime).to.be.below(1000);\n});",
    ),
    (
        "Check a response header",
        "pm.test(\"Content-Type is JSON\", function () {\n    pm.expect(pm.response.headers.get(\"content-type\")).to.include(\"application/json\");\n});",
    ),
    (
        "Log the response",
        "console.log(pm.response.code, pm.response.json());",
    ),
];

impl RequestDraft {
    pub(super) fn script_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EditorState> {
        let phase = self.script_phase;
        let index = if phase == ScriptPhase::PreRequest {
            0
        } else {
            1
        };

        if let Some(editor) = &self.script_editors[index] {
            return editor.clone();
        }

        let value = if index == 0 {
            &self.request.scripts.pre_request
        } else {
            &self.request.scripts.post_response
        };
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("javascript")
                .line_number(true)
                .soft_wrap(true)
                .placeholder(if index == 0 {
                    "// Write JavaScript to run before this request"
                } else {
                    "// Write tests to run after the response"
                })
                .default_value(value.clone())
        });
        self._subscriptions.push(cx.subscribe(
            &editor,
            move |this, editor, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = editor.read(cx).value().to_string();
                    if phase == ScriptPhase::PreRequest {
                        this.request.scripts.pre_request = value;
                    } else {
                        this.request.scripts.post_response = value;
                    }
                    cx.notify();
                }
            },
        ));
        self.script_editors[index] = Some(editor.clone());
        editor
    }

    pub(super) fn scripts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let editor = self.script_state(window, cx);
        let phase = self.script_phase;
        let snippets = if phase == ScriptPhase::PreRequest {
            PRE_SNIPPETS
        } else {
            POST_SNIPPETS
        };
        let draft = cx.entity().downgrade();

        h_flex()
            .debug_selector(|| "request-scripts".into())
            .size_full()
            .min_h_0()
            .min_w_0()
            .items_stretch()
            .child(
                Tabs::new("script-phases")
                    .flex()
                    .flex_col()
                    .flex_none()
                    .w(rems(9.))
                    .pr_2()
                    .gap_1()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .children(
                        [ScriptPhase::PreRequest, ScriptPhase::PostResponse]
                            .into_iter()
                            .map(|option| {
                                let present = if option == ScriptPhase::PreRequest {
                                    !self.request.scripts.pre_request.is_empty()
                                } else {
                                    !self.request.scripts.post_response.is_empty()
                                };
                                Tab::new(option.label())
                                    .debug_selector(move || {
                                        format!("script-phase-{}", option.label())
                                    })
                                    .selected(phase == option)
                                    .accessibility_label(option.label())
                                    .h_8()
                                    .px_2()
                                    .gap_2()
                                    .rounded(cx.theme().radius_tokens().md)
                                    .text_color(cx.theme().muted_foreground)
                                    .when(phase == option, |tab| {
                                        tab.bg(cx.theme().muted).text_color(cx.theme().foreground)
                                    })
                                    .child(option.label())
                                    .when(present, |tab| tab.child(div().text_xs().child("•")))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.script_phase = option;
                                        this.script_state(window, cx);
                                        cx.notify();
                                    }))
                            }),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .pl_3()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_none()
                            .min_w_0()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(div().flex_1().min_w_0().child(
                                if phase == ScriptPhase::PreRequest {
                                    "Runs before this request is sent."
                                } else {
                                    "Runs after the response is received."
                                },
                            ))
                            .child("JavaScript"),
                    )
                    .child(
                        div()
                            .debug_selector(|| "script-editor".into())
                            .flex_1()
                            .min_h_0()
                            .child(
                                Editor::new(&editor)
                                    .h_full()
                                    .appearance(false)
                                    .bordered(false)
                                    .bg(cx
                                        .theme()
                                        .highlight_theme
                                        .style
                                        .editor_background
                                        .unwrap_or_else(|| cx.theme().input_background()))
                                    .text_sm()
                                    .aria_label(format!("{} script", phase.label())),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .min_w_0()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Send to run scripts"),
                            )
                            .child(
                                Button::new("script-snippets")
                                    .debug_selector(|| "script-snippets".into())
                                    .ghost()
                                    .small()
                                    .label("Snippets")
                                    .icon(IconName::ChevronDown)
                                    .dropdown_menu(move |mut menu, _, _| {
                                        for &(label, code) in snippets {
                                            let draft = draft.clone();
                                            menu = menu.item(PopupMenuItem::new(label).on_click(
                                                move |_, window, cx| {
                                                    let _ = draft.update(cx, |draft, cx| {
                                                        let editor = draft.script_state(window, cx);
                                                        editor.update(cx, |editor, cx| {
                                                            let existing = editor.value();
                                                            let text = if existing.is_empty() {
                                                                code.to_owned()
                                                            } else {
                                                                format!("{existing}\n\n{code}")
                                                            };
                                                            editor.replace_all(text, window, cx);
                                                            window.focus(
                                                                &editor.focus_handle(cx),
                                                                cx,
                                                            );
                                                        });
                                                    });
                                                },
                                            ));
                                        }
                                        menu
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
}
