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

use crate::variables::VariableScope;

/// A draft whose request a code snippet shows as a command: cURL for HTTP
/// requests, grpcurl for gRPC calls.
pub(crate) trait SnippetDraft: Sized + 'static {
    /// What the command is written for. The command is written again only
    /// when it changes.
    type Request: Clone + PartialEq;
    /// The program that runs the command.
    const PROGRAM: &'static str;

    fn request(&self) -> &Self::Request;

    /// The command, with the variables `values` defines filled in.
    fn command(&self, values: &HashMap<String, String>) -> String;

    fn variables(&self) -> &Entity<VariableScope>;

    fn snippet_panel(&mut self) -> &mut SnippetPanel<Self>;

    /// Opens or closes the snippet with `toggle`, and redraws its button.
    fn toggle_code_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>);
}

/// A draft's code snippet, beside its request or below it in a narrow window.
pub(crate) struct SnippetPanel<D: SnippetDraft> {
    /// Shown while open.
    pub(crate) snippet: Option<Entity<CodeSnippet<D>>>,
    /// The snippet's height below the request, when the last frame was too
    /// narrow for it beside the request.
    below: Option<Pixels>,
}

impl<D: SnippetDraft> Default for SnippetPanel<D> {
    fn default() -> Self {
        Self {
            snippet: None,
            below: None,
        }
    }
}

/// Opens the draft's snippet, or closes it while open.
pub(crate) fn toggle<D: SnippetDraft>(draft: &mut D, window: &mut Window, cx: &mut Context<D>) {
    if draft.snippet_panel().snippet.take().is_some() {
        return;
    }

    let entity = cx.entity();
    let snippet = cx.new(|cx| CodeSnippet::new(draft, &entity, window, cx));
    let panel = draft.snippet_panel();
    // The layout was measured while the snippet was last open.
    let compact = panel.below.is_some();
    snippet.update(cx, |snippet, _| snippet.compact = compact);
    panel.snippet = Some(snippet);
}

/// Opens the request as a command, like Postman's `</>` button.
pub(crate) fn toggle_button<D: SnippetDraft>(open: bool, cx: &mut Context<D>) -> Button {
    Button::new("code-snippet")
        .debug_selector(|| "code-snippet-toggle".into())
        .ghost()
        .small()
        .flex_none()
        .icon(Icon::default().path("icons/code-xml.svg"))
        .selected(open)
        .accessibility_label(if open {
            "Hide code snippet"
        } else {
            "Show code snippet"
        })
        .tooltip("Code snippet")
        .on_click(cx.listener(|this: &mut D, _, window, cx| this.toggle_code_snippet(window, cx)))
}

/// Copies the request as a command, with the variables that resolve filled in.
pub(crate) fn copy<D: SnippetDraft>(draft: &D, window: &mut Window, cx: &mut App) {
    let command = draft.command(&variable_values(draft.variables(), cx).0);
    cx.write_to_clipboard(ClipboardItem::new_string(command));
    window.push_notification(
        Notification::success(format!("Copied the request as {}.", D::PROGRAM)),
        cx,
    );
}

/// The request beside its open snippet when both fit, and above it in a
/// narrow window, so the URL stays editable. Below, the snippet leaves the
/// request the height its configuration and response need, down to a few
/// lines of its own. The size is measured while drawing, so a change applies
/// from the next frame.
pub(crate) fn with_snippet<D: SnippetDraft>(
    draft: &mut D,
    request: AnyElement,
    cx: &mut Context<D>,
) -> AnyElement {
    let panel = draft.snippet_panel();
    let Some(snippet) = panel.snippet.clone() else {
        return request;
    };

    let entity = cx.entity().downgrade();
    let measure = canvas(
        move |bounds, window, cx| {
            let rem = |size: f32| rems(size).to_pixels(window.rem_size());
            let below = (bounds.size.width < rem(40.))
                .then(|| (bounds.size.height - rem(32.)).max(rem(7.)).min(rem(16.)));

            let _ = entity.update(cx, |draft, cx| {
                let panel = draft.snippet_panel();
                if panel.below == below {
                    return;
                }

                panel.below = below;
                // Notifying while drawing would not schedule the next frame,
                // so it waits until drawing is done.
                let draft = cx.entity();
                window.defer(cx, move |_, cx| {
                    draft.update(cx, |draft, cx| {
                        let panel = draft.snippet_panel();
                        let compact = panel.below.is_some();
                        if let Some(snippet) = &panel.snippet {
                            snippet.update(cx, |snippet, cx| {
                                snippet.compact = compact;
                                cx.notify();
                            });
                        }
                        cx.notify();
                    })
                });
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();

    if let Some(height) = panel.below {
        v_flex()
            .relative()
            .size_full()
            .child(measure)
            .child(div().flex_1().min_h_0().w_full().child(request))
            .child(
                div()
                    .flex_none()
                    .h(height)
                    .w_full()
                    .pt_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(snippet),
            )
            .into_any_element()
    } else {
        h_flex()
            .relative()
            .size_full()
            .child(measure)
            .child(div().flex_1().min_w_0().h_full().child(request))
            .child(
                div()
                    .flex_none()
                    .w(rems(24.))
                    .max_w(relative(0.4))
                    .h_full()
                    .border_l_1()
                    .border_color(cx.theme().border)
                    .child(snippet),
            )
            .into_any_element()
    }
}

/// The request as a command beside the request, like Postman's code snippet
/// panel. It follows edits to the request and fills in the variables that
/// resolve.
pub(crate) struct CodeSnippet<D: SnippetDraft> {
    draft: WeakEntity<D>,
    editor: Entity<EditorState>,
    /// The request the command was written for.
    request: D::Request,
    /// Variable values and the version of their sources they were read at.
    /// Reading the environment files on every edit would slow typing, so
    /// they are read again only when the variables change.
    values: (HashMap<String, String>, VariablesVersion),
    command: SharedString,
    /// Below the request in a narrow window, the snippet keeps its controls
    /// in one row to leave room for the response.
    compact: bool,
    _subscriptions: [Subscription; 2],
}

impl<D: SnippetDraft> CodeSnippet<D> {
    /// Opened from within an update of `draft`, which `entity` refers to.
    fn new(draft: &D, entity: &Entity<D>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let values = variable_values(draft.variables(), cx);
        let command = draft.command(&values.0);
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
                if this.request != *draft.read(cx).request() {
                    this.refresh(false, window, cx);
                }
            }),
            // The active environment or the collection's variables changed.
            cx.observe_in(draft.variables(), window, |this, _, window, cx| {
                this.refresh(true, window, cx)
            }),
        ];

        Self {
            draft: entity.downgrade(),
            editor,
            request: draft.request().clone(),
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
        if variables_changed || variables_version(draft.variables(), cx) != self.values.1 {
            self.values = variable_values(draft.variables(), cx);
        }

        let command = draft.command(&self.values.0);
        self.request = draft.request().clone();

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

fn variables_version(variables: &Entity<VariableScope>, cx: &App) -> VariablesVersion {
    let scope = variables.read(cx);
    (scope.session.revision(), scope.file_versions(cx))
}

/// The values of the variables that resolve, and the version of their
/// sources they were read at.
fn variable_values(
    variables: &Entity<VariableScope>,
    cx: &App,
) -> (HashMap<String, String>, VariablesVersion) {
    let version = variables_version(variables, cx);
    let scope = variables.read(cx);
    // Without the environment files, session values still resolve.
    let values = scope
        .values(cx)
        .unwrap_or_else(|_| scope.session.values(HashMap::new(), HashMap::new()));

    (values, version)
}

impl<D: SnippetDraft> Render for CodeSnippet<D> {
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
        let program = div()
            .flex_1()
            .font_weight(FontWeight::MEDIUM)
            .child(D::PROGRAM);

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
                            .child(program)
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
                            .child(program)
                            .child(copy),
                    )
                }
            })
            .child(
                div()
                    .debug_selector(|| "code-snippet-command".into())
                    .flex_1()
                    .min_h_0()
                    // The panel's own border frames the command. Themes whose
                    // editor color differs from the panel show it as a block.
                    .rounded(cx.theme().radius_tokens().lg)
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
                            .aria_label(format!("{} command", D::PROGRAM)),
                    ),
            )
    }
}
