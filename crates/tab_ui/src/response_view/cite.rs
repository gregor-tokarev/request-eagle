use gpui_kit::base::TextSelection;
use gpui_kit::*;

use super::body::{Body, BodyMode};
use super::view::{ResponseState, ResponseView, Section};
use crate::cite::{CiteSource, Quote, http_message};

impl ResponseView {
    /// What the response quotes to the agent: the selection in the section
    /// shown, or the section's text when nothing is selected in it.
    pub(crate) fn quote(&self, window: &mut Window, cx: &mut App) -> Option<Quote> {
        let ResponseState::Received { content, body, .. } = &self.state else {
            return None;
        };

        let source = match self.section {
            Section::Body => CiteSource::ResponseBody,
            Section::Headers => CiteSource::ResponseHeaders,
            Section::Cookies => CiteSource::ResponseCookies,
            Section::Request => CiteSource::SentRequest,
            Section::Tests | Section::Console => CiteSource::Response,
        };
        let language = Some(content.language).filter(|language| *language != "text");

        // The body's text and selection, in the viewer that shows it.
        let body_text = match (self.section, body) {
            (Section::Body, Body::Pretty(editor)) => {
                let editor = editor.read(cx).0.read(cx);
                Some((editor.value(), editor.selected_range(), language))
            }
            (Section::Body, Body::Raw { view, .. }) => {
                let view = view.read(cx);
                let language = language.filter(|_| self.mode != BodyMode::Hex);
                Some((view.source.clone(), view.selection.clone(), language))
            }
            _ => None,
        };
        if let Some((text, selection, language)) = &body_text
            && !selection.is_empty()
        {
            return Quote::of(source, text, selection.clone(), *language);
        }

        // Headers, cookies, the status and timing are selectable text, which
        // the window selects across.
        if TextSelection::has_selection(window, cx)
            && let Some(quote) = Quote::of(
                source,
                &TextSelection::selected_text(window, cx),
                0..0,
                None,
            )
        {
            return Some(quote);
        }

        match self.section {
            Section::Body => match (body_text, body) {
                (Some((text, _, language)), _) => Quote::of(source, &text, 0..0, language),
                // A page's preview quotes its source. Images and documents
                // have no text to quote.
                (None, Body::Html(_)) => Quote::of(source, &content.raw, 0..0, Some("html")),
                (None, _) => None,
            },
            Section::Headers | Section::Cookies => {
                let rows = if self.section == Section::Headers {
                    &content.headers
                } else {
                    &content.cookies
                };
                let text: Vec<String> = rows
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}"))
                    .collect();

                Quote::of(source, &text.join("\n"), 0..0, None)
            }
            Section::Request => {
                let sent = content.execution.sent.as_ref()?;

                Quote::of(source, &http_message(sent), 0..0, None)
            }
            Section::Tests | Section::Console => None,
        }
    }

    pub(crate) fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
    }

    /// The status of the response shown, such as `500 Internal Server Error`.
    pub(crate) fn status(&self) -> Option<String> {
        match &self.state {
            ResponseState::Received { content, .. } => Some(content.http().status.to_string()),
            _ => None,
        }
    }
}
