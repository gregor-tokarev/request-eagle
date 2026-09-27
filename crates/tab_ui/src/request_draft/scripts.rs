use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::{Editor, EditorState, Enter, Escape, InputEvent, MoveDown, MoveUp},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::ScriptPhase;
use std::rc::Rc;

use super::{
    RequestDraft,
    script_completions::{ScriptCompletions, capture_completion_action},
    script_signature::ScriptSignature,
};

const PRE_SNIPPETS: &[(&str, &str)] = &[
    ("Set a variable", "pm.variables.set(\"name\", \"value\");"),
    (
        "Generate a request ID",
        "pm.variables.set(\"requestId\", pm.variables.replaceIn(\"{{$guid}}\"));\npm.request.headers.upsert({key: \"X-Request-Id\", value: pm.variables.get(\"requestId\")});",
    ),
    (
        "Set a request header",
        "pm.request.headers.upsert({\n    key: \"X-Request-Id\",\n    value: String(Date.now())\n});",
    ),
    (
        "Fetch an access token",
        "const auth = await pm.sendRequest({\n    url: \"{{base_url}}/login\",\n    method: \"POST\"\n});\npm.expect(auth.code).to.equal(200);\npm.request.headers.upsert({\n    key: \"Authorization\",\n    value: \"Bearer \" + auth.json().token\n});",
    ),
    (
        "Sign the body with HMAC",
        "const signature = pm.crypto.hmacSha256(\n    pm.environment.get(\"signing_key\"),\n    pm.variables.replaceIn(pm.request.body.raw || \"\")\n);\npm.request.headers.upsert({key: \"X-Signature\", value: signature});",
    ),
    (
        "Skip when a variable is missing",
        "if (!pm.environment.get(\"token\")) {\n    pm.execution.skipRequest(\"No access token configured\");\n}",
    ),
    (
        "Log a message",
        "console.log(\"Sending\", pm.request.method, pm.request.url);",
    ),
];
const POST_SNIPPETS: &[(&str, &str)] = &[
    (
        "Save a token for later requests",
        "pm.environment.set(\"token\", pm.response.json().token);",
    ),
    (
        "Status code is 200",
        "pm.test(\"Status code is 200\", function () {\n    pm.response.to.have.status(200);\n});",
    ),
    (
        "Check a JSON value",
        "pm.test(\"Response has the expected value\", function () {\n    const data = pm.response.json();\n    pm.expect(data).to.have.property(\"success\", true);\n});",
    ),
    (
        "Status is one of the expected codes",
        "pm.test(\"Status is expected\", function () {\n    pm.expect(pm.response.code).to.be.oneOf([200, 201, 202]);\n});",
    ),
    (
        "Check a nested JSON property",
        "pm.test(\"Response has a user ID\", function () {\n    pm.expect(pm.response.json()).to.have.nested.property(\"data.user.id\");\n});",
    ),
    (
        "Check required JSON keys",
        "pm.test(\"Response has the required keys\", function () {\n    pm.expect(pm.response.json()).to.include.all.keys(\"id\", \"name\");\n});",
    ),
    (
        "Validate a response schema",
        "pm.test(\"Response matches the schema\", function () {\n    pm.response.to.have.jsonSchema({\n        type: \"object\",\n        required: [\"id\"],\n        properties: {id: {type: \"integer\"}}\n    });\n});",
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
        let completions = Rc::new(ScriptCompletions::new(phase, &editor));
        editor.update(cx, |editor, _| {
            editor.lsp_mut().completion_provider = Some(completions.clone());
            editor.lsp_mut().hover_provider = Some(completions);
            editor.lsp_mut().completion_menu.max_width = rems(40.).to_pixels(window.rem_size());
        });
        self.script_signatures[index] =
            Some(cx.new(|cx| ScriptSignature::new(editor.clone(), phase, window, cx)));
        crate::script_intelligence::warm_up();
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
        editor.update(cx, |editor, _| {
            // GPUI's completion popover uses a local cursor x-coordinate when
            // limiting its width. Account for the editor's window position.
            let available = editor
                .cursor_layout()
                .map(|(cursor, _)| {
                    window.bounds().size.width
                        - cursor.origin.x
                        - editor.scroll_offset().x
                        - rems(0.5).to_pixels(window.rem_size())
                })
                .unwrap_or(window.bounds().size.width);
            editor.lsp_mut().completion_menu.max_width = rems(40.)
                .to_pixels(window.rem_size())
                .min(available.max(px(120.)));
        });
        let phase = self.script_phase;
        let index = usize::from(phase == ScriptPhase::PostResponse);
        let signature = self.script_signatures[index].as_ref().unwrap().clone();
        let escape_editor = editor.clone();
        let escape_signature = signature.clone();
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
                            .capture_action(capture_completion_action::<Enter>(&editor))
                            .capture_action(move |action: &Escape, window, cx| {
                                let handled = escape_editor.update(cx, |editor, cx| {
                                    editor.route_overlay_action(action.boxed_clone(), window, cx)
                                });
                                if handled
                                    || escape_signature
                                        .update(cx, |signature, cx| signature.dismiss(cx))
                                {
                                    cx.stop_propagation();
                                }
                            })
                            .capture_action(capture_completion_action::<MoveUp>(&editor))
                            .capture_action(capture_completion_action::<MoveDown>(&editor))
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
                            )
                            .child(signature),
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
