use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::{Editor, EditorState, Enter, Escape, InputEvent, MoveDown, MoveUp},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{RequestScripts, ScriptPhase};
use std::rc::Rc;

use super::{
    completions::{ScriptCompletions, capture_completion_action},
    signature::ScriptSignature,
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

/// Whose scripts the editor changes. This only affects its guidance text.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptTarget {
    Request,
    Collection,
}

impl ScriptTarget {
    fn description(self, phase: ScriptPhase) -> &'static str {
        match (self, phase) {
            (Self::Request, ScriptPhase::PreRequest) => "Runs before this request is sent.",
            (Self::Request, ScriptPhase::PostResponse) => "Runs after the response is received.",
            (Self::Collection, ScriptPhase::PreRequest) => {
                "Runs before every request in this collection, ahead of the request's own script."
            }
            (Self::Collection, ScriptPhase::PostResponse) => {
                "Runs after every response in this collection, ahead of the request's own script."
            }
        }
    }

    fn placeholder(self, phase: ScriptPhase) -> &'static str {
        match (self, phase) {
            (Self::Request, ScriptPhase::PreRequest) => {
                "// Write JavaScript to run before this request"
            }
            (Self::Request, ScriptPhase::PostResponse) => {
                "// Write tests to run after the response"
            }
            (Self::Collection, ScriptPhase::PreRequest) => {
                "// Write JavaScript to run before every request in this collection"
            }
            (Self::Collection, ScriptPhase::PostResponse) => {
                "// Write tests to run after every response in this collection"
            }
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Request => "Send to run scripts",
            Self::Collection => "Save, then send a request in this collection to run scripts",
        }
    }
}

/// The edited scripts, emitted after every change.
pub(crate) struct ScriptsChanged(pub RequestScripts);

/// Pre-request and post-response editors with completion, signature help and
/// Vim mode. Each phase's editor is created when it is first shown.
pub(crate) struct ScriptEditor {
    target: ScriptTarget,
    scripts: RequestScripts,
    pub(crate) phase: ScriptPhase,
    pub(crate) editors: [Option<Entity<EditorState>>; 2],
    vim: [Option<Entity<crate::vim::Vim>>; 2],
    pub(super) signatures: [Option<Entity<ScriptSignature>>; 2],
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ScriptsChanged> for ScriptEditor {}

impl ScriptEditor {
    pub(crate) fn new(scripts: RequestScripts, target: ScriptTarget) -> Self {
        Self {
            target,
            scripts,
            phase: ScriptPhase::PreRequest,
            editors: [None, None],
            vim: [None, None],
            signatures: [None, None],
            _subscriptions: Vec::new(),
        }
    }

    pub(crate) fn select_phase(
        &mut self,
        phase: ScriptPhase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.phase = phase;
        self.editor(window, cx);
        cx.notify();
    }

    /// The selected phase's editor.
    pub(crate) fn editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<EditorState> {
        let phase = self.phase;
        let index = usize::from(phase == ScriptPhase::PostResponse);

        if let Some(editor) = &self.editors[index] {
            return editor.clone();
        }

        let value = if index == 0 {
            &self.scripts.pre_request
        } else {
            &self.scripts.post_response
        };
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("javascript")
                .line_number(true)
                .soft_wrap(true)
                .placeholder(self.target.placeholder(phase))
                .default_value(value.clone())
        });
        let completions = Rc::new(ScriptCompletions::new(phase, &editor));
        editor.update(cx, |editor, _| {
            editor.lsp_mut().completion_provider = Some(completions.clone());
            editor.lsp_mut().hover_provider = Some(completions);
            editor.lsp_mut().completion_menu.max_width = rems(40.).to_pixels(window.rem_size());
        });
        self.signatures[index] =
            Some(cx.new(|cx| ScriptSignature::new(editor.clone(), phase, window, cx)));
        script_intelligence::warm_up();
        self._subscriptions.push(cx.subscribe(
            &editor,
            move |this, editor, event: &InputEvent, cx| {
                // Reload the compiler if it was released while scripts were idle.
                if matches!(event, InputEvent::Focus) {
                    script_intelligence::warm_up();
                }

                if matches!(event, InputEvent::Change) {
                    let value = editor.read(cx).value().to_string();
                    if phase == ScriptPhase::PreRequest {
                        this.scripts.pre_request = value;
                    } else {
                        this.scripts.post_response = value;
                    }
                    cx.emit(ScriptsChanged(this.scripts.clone()));
                    cx.notify();
                }
            },
        ));
        self.vim[index] = Some(cx.new(|cx| crate::vim::Vim::new(editor.clone(), cx)));
        self.editors[index] = Some(editor.clone());
        editor
    }
}

impl Render for ScriptEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor(window, cx);
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
        let phase = self.phase;
        let index = usize::from(phase == ScriptPhase::PostResponse);
        let signature = self.signatures[index].as_ref().unwrap().clone();
        let mouse_vim = self.vim[index].as_ref().unwrap().clone();
        let escape_editor = editor.clone();
        let escape_signature = signature.clone();
        let snippets = if phase == ScriptPhase::PreRequest {
            PRE_SNIPPETS
        } else {
            POST_SNIPPETS
        };
        let view = cx.entity().downgrade();
        let target = self.target;

        h_flex()
            .debug_selector(move || match target {
                ScriptTarget::Request => "request-scripts".into(),
                ScriptTarget::Collection => "collection-scripts".into(),
            })
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
                                    !self.scripts.pre_request.is_empty()
                                } else {
                                    !self.scripts.post_response.is_empty()
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
                                        this.select_phase(option, window, cx);
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
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.target.description(phase)),
                            )
                            .children(self.vim[index].clone())
                            .child("JavaScript"),
                    )
                    .child(
                        div()
                            .debug_selector(|| "script-editor".into())
                            .capture_any_mouse_down(move |_, _, cx| {
                                mouse_vim.update(cx, |vim, _| vim.mouse_down());
                            })
                            .relative()
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
                            .track_focus(&self.vim[index].as_ref().unwrap().focus_handle(cx))
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
                            .child(crate::vim::cursor(self.vim[index].as_ref().unwrap()))
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
                                    .child(self.target.hint()),
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
                                            let view = view.clone();
                                            menu = menu.item(PopupMenuItem::new(label).on_click(
                                                move |_, window, cx| {
                                                    let _ = view.update(cx, |view, cx| {
                                                        let editor = view.editor(window, cx);
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
    }
}
