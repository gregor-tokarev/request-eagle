//! What a tab quotes to the agent: the selection in a request or its
//! response, or the whole part that has focus.

use std::{ops::Range, path::PathBuf};

use gpui_kit::base::input;
use gpui_kit::component::{input::EditorState, native_menu::NativeMenu};
use gpui_kit::{Action, App, Entity, EntityId, Focusable as _, SharedString, Window, actions};
use request::{Body, Field, HttpRequest};

// Quotes the selection, or the field with focus, in the agent's message.
actions!(agent, [AddToAgent]);

/// A cite for the agent's message. Tabs dispatch it from where focus is; the
/// workspace hands it to the agent panel.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = agent, no_json)]
pub struct CiteToAgent(pub Cite);

/// The part of a tab that a cite quotes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CiteSource {
    Url,
    /// The request as written, with its method, URL, headers and body.
    Request,
    RequestBody,
    Script,
    /// A gRPC or WebSocket message being written.
    Message,
    ResponseBody,
    ResponseHeaders,
    ResponseCookies,
    /// Other text selected in the response, such as its status or timing.
    Response,
    /// The request as it went out, from the response's Request section.
    SentRequest,
}

impl CiteSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Url => "URL",
            Self::Request => "Request",
            Self::RequestBody => "Request body",
            Self::Script => "Script",
            Self::Message => "Message",
            Self::ResponseBody => "Response body",
            Self::ResponseHeaders => "Response headers",
            Self::ResponseCookies => "Response cookies",
            Self::Response => "Response",
            Self::SentRequest => "Sent request",
        }
    }
}

/// A quote from a request tab.
#[derive(Clone, Debug, PartialEq)]
pub struct Cite {
    pub source: CiteSource,
    pub text: String,
    /// The language of the text, such as `json`, when it is code.
    pub language: Option<&'static str>,
    /// The first and last line of a selection, from 1, when the text it was
    /// selected in has more than one line.
    pub lines: Option<(usize, usize)>,
    pub origin: CiteOrigin,
}

/// What a part of a tab quotes, before the tab says where it comes from.
pub(crate) struct Quote {
    pub(crate) source: CiteSource,
    pub(crate) text: String,
    pub(crate) language: Option<&'static str>,
    pub(crate) lines: Option<(usize, usize)>,
}

impl Quote {
    /// The selection in `text`, or all of it; see [`quote`].
    pub(crate) fn of(
        source: CiteSource,
        text: &str,
        selection: Range<usize>,
        language: Option<&'static str>,
    ) -> Option<Self> {
        let (text, lines) = quote(text, selection)?;

        Some(Self {
            source,
            text,
            language,
            lines,
        })
    }

    pub(crate) fn cite(self, origin: CiteOrigin) -> Cite {
        Cite {
            source: self.source,
            text: self.text,
            language: self.language,
            lines: self.lines,
            origin,
        }
    }
}

/// How many characters of a one-line quote its name shows.
const NAMED_QUOTE: usize = 24;

impl Cite {
    /// How the cite is named in the agent's message, such as
    /// `Response body · L12–14`, or `URL · /users/42` for a short quote.
    pub fn label(&self) -> String {
        let source = self.source.label();
        let text = self.text.trim();

        match self.lines {
            Some((first, last)) if first == last => format!("{source} · L{first}"),
            Some((first, last)) => format!("{source} · L{first}–{last}"),
            None if !text.contains('\n') && text.chars().count() <= NAMED_QUOTE => {
                format!("{source} · {text}")
            }
            None => source.to_owned(),
        }
    }
}

/// The request tab a cite comes from.
#[derive(Clone, Debug, PartialEq)]
pub struct CiteOrigin {
    /// The tab's page, to show it again.
    pub page: EntityId,
    pub name: SharedString,
    /// Such as `GET {{base_url}}/users/42`.
    pub request: String,
    /// The status of the response shown, such as `500 Internal Server Error`.
    pub status: Option<String>,
    /// The request's file, while it is saved.
    pub file: Option<PathBuf>,
    pub unsaved_changes: bool,
}

/// Whether an editor has focus, or its Vim mode does.
pub(crate) fn editor_focused(
    editor: &Entity<EditorState>,
    vim: Option<&Entity<crate::vim::Vim>>,
    window: &Window,
    cx: &App,
) -> bool {
    editor.focus_handle(cx).is_focused(window)
        || vim.is_some_and(|vim| vim.focus_handle(cx).contains_focused(window, cx))
}

/// What a text field offers on right-click: its editing commands, and
/// quoting its selection to the agent. `selected` enables Cut and Copy.
pub(crate) fn field_menu(menu: NativeMenu, editable: bool, selected: bool) -> NativeMenu {
    let menu = if editable {
        menu.menu_with_disabled("Cut", !selected, Box::new(input::Cut))
    } else {
        menu
    };
    let menu = menu.menu_with_disabled("Copy", !selected, Box::new(input::Copy));
    let menu = if editable {
        menu.menu("Paste", Box::new(input::Paste))
    } else {
        menu
    };

    menu.separator()
        .menu("Select All", Box::new(input::SelectAll))
        .separator()
        .menu("Add to Agent", Box::new(AddToAgent))
}

/// The right-click menu of an editable input or editor. The menu is built
/// while the input handles the click, so it cannot read the selection: Cut
/// and Copy do nothing without one.
pub(crate) fn editable_field_menu(menu: NativeMenu, _: &mut Window, _: &mut App) -> NativeMenu {
    field_menu(menu, true, true)
}

/// The selected part of `text`, or all of it when nothing is selected, with
/// the lines a selection spans. Nothing is quoted from empty text.
pub(crate) fn quote(
    text: &str,
    selection: Range<usize>,
) -> Option<(String, Option<(usize, usize)>)> {
    let selection = selection.start.min(text.len())..selection.end.min(text.len());
    if selection.is_empty()
        || !text.is_char_boundary(selection.start)
        || !text.is_char_boundary(selection.end)
    {
        return (!text.trim().is_empty()).then(|| (text.to_owned(), None));
    }

    let quoted = &text[selection.clone()];
    if quoted.trim().is_empty() {
        return None;
    }

    let lines = text.contains('\n').then(|| {
        let line_of = |offset: usize| text[..offset].matches('\n').count() + 1;
        // A selection that ends at the start of a line ends on the line before.
        let end = if quoted.ends_with('\n') {
            selection.end - 1
        } else {
            selection.end
        };
        (line_of(selection.start), line_of(end))
    });

    Some((quoted.to_owned(), lines))
}

/// An HTTP request as written, in the form of the message it sends, with its
/// `{{variables}}` as they are.
pub(crate) fn http_message(request: &HttpRequest) -> String {
    let mut message = format!("{} {}\n", request.method.as_str(), request.path);
    for (key, value) in Field::enabled(&request.headers) {
        message.push_str(&format!("{key}: {value}\n"));
    }

    match &request.body {
        None => {}
        Some(Body::Raw { text, .. }) => {
            message.push('\n');
            message.push_str(text);
        }
        Some(Body::UrlEncoded { fields }) => {
            let fields: Vec<String> = fields
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            message.push('\n');
            message.push_str(&fields.join("&"));
        }
        Some(Body::Multipart { parts }) => {
            message.push_str("\nmultipart/form-data:\n");
            for part in parts {
                let value = if part.file {
                    format!("file {}", part.value)
                } else {
                    part.value.clone()
                };
                message.push_str(&format!("{}: {value}\n", part.name));
            }
        }
        Some(Body::Binary { file }) => {
            message.push_str(&format!("\nThe body is the file {}", file.display()));
        }
    }

    message.trim_end().to_owned()
}
