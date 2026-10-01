use std::collections::HashMap;

use gpui_kit::component::{
    button::*,
    clipboard::Clipboard,
    input::{Editor, EditorState},
    notification::Notification,
    *,
};
use gpui_kit::*;
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
    command: SharedString,
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
        let command = draft.curl_command(cx);
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
                    this.refresh(window, cx);
                }
            }),
            // The active environment or the collection's variables changed.
            cx.observe_in(&draft.variables, window, |this, _, window, cx| {
                this.refresh(window, cx)
            }),
        ];

        Self {
            draft: entity.downgrade(),
            editor,
            request: draft.request.clone(),
            command: command.into(),
            _subscriptions: subscriptions,
        }
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.upgrade() else {
            return;
        };
        let draft = draft.read(cx);
        let command = draft.curl_command(cx);
        self.request = draft.request.clone();

        if command != self.command.as_ref() {
            self.command = command.clone().into();
            self.editor
                .update(cx, |editor, cx| editor.set_value(command, window, cx));
            cx.notify();
        }
    }
}

impl RequestDraft {
    /// The request as a cURL command, with the variables that resolve filled in.
    pub(crate) fn curl_command(&self, cx: &App) -> String {
        let scope = self.variables.read(cx);
        // Without the environment files, session values still resolve.
        let values = scope
            .values(cx)
            .unwrap_or_else(|_| scope.session.values(HashMap::new()));

        self.request.curl_command(&values)
    }

    pub fn copy_as_curl(&self, window: &mut Window, cx: &mut App) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.curl_command(cx)));
        window.push_notification(Notification::success("Copied the request as cURL."), cx);
    }
}

impl Render for CodeSnippet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let command = self.command.clone();
        let draft = self.draft.clone();

        v_flex()
            .debug_selector(|| "code-snippet".into())
            .size_full()
            .min_w_0()
            .px_4()
            .pb_2()
            .gap_2()
            .text_sm()
            .child(
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
                    .child(
                        Button::new("close-code-snippet")
                            .debug_selector(|| "close-code-snippet".into())
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .accessibility_label("Close code snippet")
                            .tooltip("Close")
                            .on_click(move |_, window, cx| {
                                let _ = draft
                                    .update(cx, |draft, cx| draft.toggle_code_snippet(window, cx));
                            }),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .h_8()
                    .gap_2()
                    .child(div().flex_1().font_weight(FontWeight::MEDIUM).child("cURL"))
                    .child(
                        div().debug_selector(|| "copy-code-snippet".into()).child(
                            Clipboard::new("copy-code-snippet")
                                .small()
                                .tooltip("Copy snippet")
                                .value(command),
                        ),
                    ),
            )
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
