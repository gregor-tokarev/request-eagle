use std::collections::HashMap;
use std::time::SystemTime;

use gpui_kit::component::{
    button::*,
    clipboard::Clipboard,
    input::{Editor, EditorState},
    notification::Notification,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::HttpRequest;

use super::draft::RequestDraft;

/// The request as a cURL command beside the request, like Postman's code
/// snippet panel. It follows edits to the request and fills in the variables
/// that resolve.
pub(crate) struct CodeSnippet {
    draft: WeakEntity<RequestDraft>,
    editor: Entity<EditorState>,
    /// The request the command was written for.
    request: HttpRequest,
    /// Variable values and the version of their sources they were read at.
    /// Reading the environment files on every edit would slow typing, so
    /// they are read again only when the variables change.
    values: (HashMap<String, String>, VariablesVersion),
    command: SharedString,
    /// Below the request in a narrow window, the snippet keeps its controls
    /// in one row to leave room for the response.
    pub(super) compact: bool,
    _subscriptions: [Subscription; 2],
}

impl CodeSnippet {
    /// Opened from within an update of `draft`, which `entity` refers to.
    pub(super) fn new(
        draft: &RequestDraft,
        entity: &Entity<RequestDraft>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let values = draft.variable_values(cx);
        let command = draft.request.curl_command(&values.0);
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("bash")
                .line_number(true)
                .folding(false)
                .soft_wrap(true)
                .default_value(command.clone())
        });
        let subscriptions = [
            cx.observe_in(entity, window, |this, draft, window, cx| {
                if this.request != draft.read(cx).request {
                    this.refresh(false, window, cx);
                }
            }),
            // The active environment or the collection's variables changed.
            cx.observe_in(&draft.variables, window, |this, _, window, cx| {
                this.refresh(true, window, cx)
            }),
        ];

        Self {
            draft: entity.downgrade(),
            editor,
            request: draft.request.clone(),
            values,
            command: command.into(),
            compact: false,
            _subscriptions: subscriptions,
        }
    }

    fn refresh(&mut self, variables_changed: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.upgrade() else {
            return;
        };
        let draft = draft.read(cx);

        // Scripts in other tabs of the collection change session values, and
        // the environment files can change outside the app.
        if variables_changed || draft.variables_version(cx) != self.values.1 {
            self.values = draft.variable_values(cx);
        }

        let command = draft.request.curl_command(&self.values.0);
        self.request = draft.request.clone();

        if command != self.command.as_ref() {
            self.command = command.clone().into();
            self.editor
                .update(cx, |editor, cx| editor.set_value(command, window, cx));
            cx.notify();
        }
    }
}

/// The session revision and the environment files' modification times.
type VariablesVersion = (u64, Vec<Option<SystemTime>>);

impl RequestDraft {
    fn variables_version(&self, cx: &App) -> VariablesVersion {
        let scope = self.variables.read(cx);
        (scope.session.revision(), scope.file_versions(cx))
    }

    /// The values of the variables that resolve, and the version of their
    /// sources they were read at.
    fn variable_values(&self, cx: &App) -> (HashMap<String, String>, VariablesVersion) {
        let version = self.variables_version(cx);
        let scope = self.variables.read(cx);
        // Without the environment files, session values still resolve.
        let values = scope
            .values(cx)
            .unwrap_or_else(|_| scope.session.values(HashMap::new(), HashMap::new()));

        (values, version)
    }

    /// Copies the request as a cURL command, with the variables that resolve
    /// filled in.
    pub fn copy_as_curl(&self, window: &mut Window, cx: &mut App) {
        let command = self.request.curl_command(&self.variable_values(cx).0);
        cx.write_to_clipboard(ClipboardItem::new_string(command));
        window.push_notification(Notification::success("Copied the request as cURL."), cx);
    }
}

impl Render for CodeSnippet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draft = self.draft.clone();
        let close = Button::new("close-code-snippet")
            .debug_selector(|| "close-code-snippet".into())
            .ghost()
            .small()
            .icon(IconName::Close)
            .accessibility_label("Close code snippet")
            .tooltip("Close")
            .on_click(move |_, window, cx| {
                let _ = draft.update(cx, |draft, cx| draft.toggle_code_snippet(window, cx));
            });
        let copy = div().debug_selector(|| "copy-code-snippet".into()).child(
            Clipboard::new("copy-code-snippet")
                .small()
                .tooltip("Copy snippet")
                .value(self.command.clone()),
        );
        let language = div().flex_1().font_weight(FontWeight::MEDIUM).child("cURL");

        v_flex()
            .debug_selector(|| "code-snippet".into())
            .size_full()
            .min_w_0()
            .px_4()
            .pb_2()
            .gap_2()
            .text_sm()
            .map(|this| {
                if self.compact {
                    this.child(
                        h_flex()
                            .flex_none()
                            .h_8()
                            .gap_2()
                            .child(language)
                            .child(copy)
                            .child(close),
                    )
                } else {
                    this.child(
                        h_flex()
                            .flex_none()
                            .h_10()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Code snippet"),
                            )
                            .child(close),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .h_8()
                            .gap_2()
                            .child(language)
                            .child(copy),
                    )
                }
            })
            .child(
                div()
                    .debug_selector(|| "code-snippet-command".into())
                    .flex_1()
                    .min_h_0()
                    .rounded(cx.theme().radius_tokens().lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .overflow_hidden()
                    .child(
                        Editor::new(&self.editor)
                            .h_full()
                            .readonly(true)
                            .appearance(false)
                            .bordered(false)
                            .bg(cx
                                .theme()
                                .highlight_theme
                                .style
                                .editor_background
                                .unwrap_or_else(|| cx.theme().input_background()))
                            .text_sm()
                            .aria_label("cURL command"),
                    ),
            )
    }
}
