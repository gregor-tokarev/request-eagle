use gpui_kit::base::{Tab, Tabs};
use gpui_kit::component::{
    button::*,
    input::{Editor, EditorState, Enter, Escape, InputEvent, MoveDown, MoveUp},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{GrpcScripts, RequestScripts, ScriptPhase};
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
        "Format a date with Moment",
        "const moment = require(\"moment\");\npm.variables.set(\"date\", moment().utc().format(\"YYYY-MM-DD\"));",
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
        "Save a response cookie",
        "pm.environment.set(\"session\", pm.cookies.get(\"session\"));",
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

const BEFORE_INVOKE_SNIPPETS: &[(&str, &str)] = &[
    ("Set a variable", "pm.variables.set(\"name\", \"value\");"),
    (
        "Add metadata",
        "pm.request.metadata.upsert({\n    key: \"x-request-id\",\n    value: pm.variables.replaceIn(\"{{$guid}}\")\n});",
    ),
    (
        "Fetch an access token",
        "const auth = await pm.sendRequest({\n    url: \"{{base_url}}/login\",\n    method: \"POST\"\n});\npm.expect(auth.code).to.equal(200);\npm.request.metadata.upsert({\n    key: \"authorization\",\n    value: \"Bearer \" + auth.json().token\n});",
    ),
    (
        "Change the message",
        "const message = JSON.parse(pm.request.message || \"{}\");\nmessage.requestId = pm.variables.replaceIn(\"{{$guid}}\");\npm.request.message = message;",
    ),
    (
        "Skip when a variable is missing",
        "if (!pm.environment.get(\"token\")) {\n    pm.execution.skipRequest(\"No access token configured\");\n}",
    ),
    (
        "Log the call",
        "console.log(\"Invoking\", pm.request.methodPath, \"on\", pm.request.url);",
    ),
];
const ON_MESSAGE_SNIPPETS: &[(&str, &str)] = &[
    (
        "Check each message",
        "pm.test(\"Message has an ID\", function () {\n    pm.expect(pm.message.data).to.have.property(\"id\");\n});",
    ),
    (
        "Count messages",
        "pm.variables.set(\"messages\", Number(pm.variables.get(\"messages\") ?? 0) + 1);",
    ),
    (
        "Save a value from a message",
        "pm.environment.set(\"lastId\", pm.message.data.id);",
    ),
    (
        "Log each message",
        "console.log(pm.message.timestamp.toISOString(), pm.message.data);",
    ),
];
const AFTER_RESPONSE_SNIPPETS: &[(&str, &str)] = &[
    (
        "Status is OK",
        "pm.test(\"Status is OK\", function () {\n    pm.response.to.have.status(\"OK\");\n});",
    ),
    (
        "Save a value from the response",
        "pm.environment.set(\"token\", pm.response.messages.idx(0).data.token);",
    ),
    (
        "A message has the expected fields",
        "pm.test(\"A message has the expected fields\", function () {\n    pm.response.messages.to.include({success: true});\n});",
    ),
    (
        "Every message has a property",
        "pm.test(\"Every message has an ID\", function () {\n    pm.response.messages.to.have.property(\"id\");\n});",
    ),
    (
        "Validate messages with a schema",
        "pm.test(\"Messages match the schema\", function () {\n    pm.response.messages.to.have.jsonSchema({\n        type: \"object\",\n        required: [\"id\"],\n        properties: {id: {type: \"string\"}}\n    });\n});",
    ),
    (
        "Check response metadata",
        "pm.test(\"Content type is gRPC\", function () {\n    pm.response.to.have.metadata(\"content-type\", \"application/grpc\");\n});",
    ),
    (
        "Count received messages",
        "pm.test(\"Received 3 messages\", function () {\n    pm.expect(pm.response.messages.count()).to.equal(3);\n});",
    ),
    (
        "Response time is below 1 second",
        "pm.test(\"Response time is below 1 second\", function () {\n    pm.expect(pm.response.responseTime).to.be.below(1000);\n});",
    ),
    (
        "Log the response",
        "console.log(pm.response.status, pm.response.messages.all());",
    ),
];

fn snippets(phase: ScriptPhase) -> &'static [(&'static str, &'static str)] {
    match phase {
        ScriptPhase::PreRequest => PRE_SNIPPETS,
        ScriptPhase::PostResponse => POST_SNIPPETS,
        ScriptPhase::BeforeInvoke => BEFORE_INVOKE_SNIPPETS,
        ScriptPhase::OnMessage => ON_MESSAGE_SNIPPETS,
        ScriptPhase::AfterResponse => AFTER_RESPONSE_SNIPPETS,
    }
}

/// Whose scripts the editor changes. This only affects its guidance text.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptTarget {
    Request,
    Collection,
}

impl ScriptTarget {
    fn description(self, phase: ScriptPhase) -> &'static str {
        match (self, phase) {
            (Self::Collection, ScriptPhase::PreRequest) => {
                "Runs before every request in this collection, ahead of the request's own script."
            }
            (Self::Collection, ScriptPhase::PostResponse) => {
                "Runs after every response in this collection, ahead of the request's own script."
            }
            (_, ScriptPhase::PreRequest) => "Runs before this request is sent.",
            (_, ScriptPhase::PostResponse) => "Runs after the response is received.",
            (_, ScriptPhase::BeforeInvoke) => "Runs before the method is invoked.",
            (_, ScriptPhase::OnMessage) => "Runs for each message the server sends.",
            (_, ScriptPhase::AfterResponse) => "Runs after the server ends the call.",
        }
    }

    fn placeholder(self, phase: ScriptPhase) -> &'static str {
        match (self, phase) {
            (Self::Collection, ScriptPhase::PreRequest) => {
                "// Write JavaScript to run before every request in this collection"
            }
            (Self::Collection, ScriptPhase::PostResponse) => {
                "// Write tests to run after every response in this collection"
            }
            (_, ScriptPhase::PreRequest) => "// Write JavaScript to run before this request",
            (_, ScriptPhase::PostResponse) => "// Write tests to run after the response",
            (_, ScriptPhase::BeforeInvoke) => {
                "// Write JavaScript to run before invoking the method"
            }
            (_, ScriptPhase::OnMessage) => "// Write tests to run for each received message",
            (_, ScriptPhase::AfterResponse) => "// Write tests to run after the call ends",
        }
    }

    fn hint(self, phase: ScriptPhase) -> &'static str {
        match self {
            Self::Collection => "Save, then send a request in this collection to run scripts",
            Self::Request if phase.is_grpc() => "Invoke to run scripts",
            Self::Request => "Send to run scripts",
        }
    }
}

/// The scripts an editor changes, one for each phase.
pub(crate) trait Scripts: Clone + 'static {
    /// The phases in the order the editor lists them.
    const PHASES: &'static [ScriptPhase];

    fn script(&self, phase: ScriptPhase) -> &str;

    fn script_mut(&mut self, phase: ScriptPhase) -> &mut String;
}

impl Scripts for RequestScripts {
    const PHASES: &'static [ScriptPhase] = &[ScriptPhase::PreRequest, ScriptPhase::PostResponse];

    fn script(&self, phase: ScriptPhase) -> &str {
        match phase {
            ScriptPhase::PreRequest => &self.pre_request,
            _ => &self.post_response,
        }
    }

    fn script_mut(&mut self, phase: ScriptPhase) -> &mut String {
        match phase {
            ScriptPhase::PreRequest => &mut self.pre_request,
            _ => &mut self.post_response,
        }
    }
}

impl Scripts for GrpcScripts {
    const PHASES: &'static [ScriptPhase] = &[
        ScriptPhase::BeforeInvoke,
        ScriptPhase::OnMessage,
        ScriptPhase::AfterResponse,
    ];

    fn script(&self, phase: ScriptPhase) -> &str {
        match phase {
            ScriptPhase::BeforeInvoke => &self.before_invoke,
            ScriptPhase::OnMessage => &self.on_message,
            _ => &self.after_response,
        }
    }

    fn script_mut(&mut self, phase: ScriptPhase) -> &mut String {
        match phase {
            ScriptPhase::BeforeInvoke => &mut self.before_invoke,
            ScriptPhase::OnMessage => &mut self.on_message,
            _ => &mut self.after_response,
        }
    }
}

/// The edited scripts, emitted after every change.
pub(crate) struct ScriptsChanged<S = RequestScripts>(pub S);

/// An editor for each script phase with completion, signature help and Vim
/// mode. Each phase's editor is created when it is first shown.
pub(crate) struct ScriptEditor<S: Scripts = RequestScripts> {
    target: ScriptTarget,
    scripts: S,
    pub(crate) phase: ScriptPhase,
    /// In the order of `S::PHASES`, like the other per-phase views.
    pub(crate) editors: Vec<Option<Entity<EditorState>>>,
    vim: Vec<Option<Entity<crate::vim::Vim>>>,
    pub(super) signatures: Vec<Option<Entity<ScriptSignature>>>,
    _subscriptions: Vec<Subscription>,
}

impl<S: Scripts> EventEmitter<ScriptsChanged<S>> for ScriptEditor<S> {}

impl<S: Scripts> ScriptEditor<S> {
    pub(crate) fn new(scripts: S, target: ScriptTarget) -> Self {
        Self {
            target,
            scripts,
            phase: S::PHASES[0],
            editors: vec![None; S::PHASES.len()],
            vim: vec![None; S::PHASES.len()],
            signatures: vec![None; S::PHASES.len()],
            _subscriptions: Vec::new(),
        }
    }

    fn index(&self) -> usize {
        S::PHASES
            .iter()
            .position(|phase| *phase == self.phase)
            .expect("the editor's phase")
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
        let index = self.index();

        if let Some(editor) = &self.editors[index] {
            return editor.clone();
        }

        let value = self.scripts.script(phase);
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("javascript")
                .line_number(true)
                .soft_wrap(true)
                .placeholder(self.target.placeholder(phase))
                .default_value(value.to_owned())
        });
        let completions = Rc::new(ScriptCompletions::new(phase, &editor));
        editor.update(cx, |editor, _| {
            editor.lsp_mut().completion_provider = Some(completions.clone());
            editor.lsp_mut().hover_provider = Some(completions);
            editor.lsp_mut().completion_menu.max_width = rems(40.).to_pixels(window.rem_size());
        });
        self.signatures[index] =
            Some(cx.new(|cx| ScriptSignature::new(editor.clone(), phase, window, cx)));
        self._subscriptions.push(cx.subscribe(
            &editor,
            move |this, editor, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    *this.scripts.script_mut(phase) = editor.read(cx).value().to_string();
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

impl<S: Scripts> Render for ScriptEditor<S> {
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
        let index = self.index();
        let signature = self.signatures[index].as_ref().unwrap().clone();
        let mouse_vim = self.vim[index].as_ref().unwrap().clone();
        let escape_editor = editor.clone();
        let escape_signature = signature.clone();
        let snippets = snippets(phase);
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
                    .children(S::PHASES.iter().copied().map(|option| {
                        let present = !self.scripts.script(option).is_empty();
                        Tab::new(option.label())
                            .debug_selector(move || format!("script-phase-{}", option.label()))
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
                    })),
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
                                    .child(self.target.hint(phase)),
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
