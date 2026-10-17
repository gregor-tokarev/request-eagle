//! What a message sends the agent: its text, each cite and mention as
//! context the agent reads where it is in the message, and, at the start of a
//! thread, what the agent should know about Request Eagle.

use std::{io::Write as _, path::PathBuf};

use tempfile::TempPath;

use agent::acp::schema::v1::{
    ContentBlock, EmbeddedResource, EmbeddedResourceResource, TextContent, TextResourceContents,
};
use gpui_kit::component::input::InputContent;
use tab_ui::Cite;

use crate::mention::{Mention, MentionKind};
use crate::thread::Reference;

/// How much of a cite goes into the prompt. A longer one is saved to a
/// file the agent can read, such as a large response body.
const CITE_LIMIT: usize = 40_000;

/// Where the app keeps its data, for the agent to find and change it.
pub(crate) struct Briefing {
    pub(crate) data_directory: PathBuf,
    /// The Request Eagle CLI, when it is installed.
    pub(crate) cli: Option<PathBuf>,
}

impl Briefing {
    pub(crate) fn text(&self) -> String {
        let data = self.data_directory.display();
        let tools = match &self.cli {
            Some(cli) => format!(
                "Use the Request Eagle CLI at {} to list, read, edit and run saved requests \
                 and flows; `request-eagle-cli schema` describes its commands.",
                cli.display()
            ),
            None => "The Request Eagle CLI isn't installed, so edit the collections' TOML files \
                     directly, following the files already there. The user can install the CLI \
                     from https://github.com/gregor-tokarev/request-eagle/blob/main/docs/cli.md#install."
                .to_owned(),
        };

        format!(
            "You are in the agent panel of Request Eagle, an API client like Postman.\n\
             - Saved requests are in collections: folders of TOML files in {data}/collections \
             and folders opened as collections elsewhere. Flows are in {data}/flows.\n\
             - {tools}\n\
             - The app reloads collections and flows after each of your turns, so the user sees \
             your changes at once. The app does not need to be closed first.\n\
             - Cites quote what the user selected in a request or its response, as the tab shows \
             it. Variables such as {{{{token}}}} are as written, not resolved. A tab can have \
             changes that are not saved to its file yet; its cites say so.\n\
             - Mentions name a saved request, flow or collection and the file or folder it is \
             saved in. They refer to it as saved: changes in a tab that are not saved yet are \
             not in the file."
        )
    }
}

/// The prompt for a message, with `briefing` first when the thread starts.
/// `embedded` is whether the agent reads cites and mentions as embedded
/// resources; otherwise they go in the text. Long cites are saved to `files`,
/// which the thread keeps until it ends.
pub(crate) fn prompt(
    content: &InputContent,
    references: &[(String, Reference)],
    briefing: Option<String>,
    embedded: bool,
    files: &mut Vec<TempPath>,
) -> Vec<ContentBlock> {
    let mut blocks: Vec<ContentBlock> = briefing
        .map(|briefing| ContentBlock::Text(TextContent::new(briefing)))
        .into_iter()
        .collect();

    let text = content.text();
    let mut written = 0;
    for (number, span) in content.tokens().iter().enumerate() {
        let range = span.range();
        push_text(&mut blocks, &text[written..range.start]);
        written = range.end;

        let Some((_, reference)) = references
            .iter()
            .find(|(id, _)| id == span.token().id().as_ref())
        else {
            push_text(&mut blocks, &text[range]);
            continue;
        };
        let (kind, context) = match reference {
            Reference::Cite(cite) => ("cite", quote(cite, files)),
            Reference::Mention(mention) => ("mention", describe(mention)),
        };
        let uri = format!("request-eagle:{kind}/{}", number + 1);
        blocks.push(if embedded {
            ContentBlock::Resource(EmbeddedResource::new(
                EmbeddedResourceResource::TextResourceContents(TextResourceContents::new(
                    context, uri,
                )),
            ))
        } else {
            ContentBlock::Text(TextContent::new(format!(
                "\n<{kind} ref=\"{uri}\">\n{context}\n</{kind}>\n"
            )))
        });
    }
    push_text(&mut blocks, &text[written..]);

    blocks
}

/// Text between cites. A message that is only cites has none.
fn push_text(blocks: &mut Vec<ContentBlock>, text: &str) {
    if !text.trim().is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(text.to_owned())));
    }
}

/// A cite as the agent reads it: what it quotes and from where, then the
/// text. A long cite is saved whole to one of `files`.
pub(crate) fn quote(cite: &Cite, files: &mut Vec<TempPath>) -> String {
    let origin = &cite.origin;
    let mut about = format!(
        "{} of \"{}\" ({})",
        cite.source.label(),
        origin.name,
        origin.request
    );
    if let Some(status) = &origin.status {
        about.push_str(&format!(", which responded {status}"));
    }
    match cite.lines {
        Some((first, last)) if first == last => about.push_str(&format!(", line {first}")),
        Some((first, last)) => about.push_str(&format!(", lines {first}–{last}")),
        None => {}
    }
    about.push('.');
    if let Some(file) = &origin.file {
        about.push_str(&format!("\nSaved in {}", file.display()));
        if origin.unsaved_changes {
            about.push_str(", with changes in the tab that are not saved yet");
        }
        about.push('.');
    } else {
        about.push_str("\nThe request is not saved to a file.");
    }

    let mut text = cite.text.as_str();
    if text.len() > CITE_LIMIT {
        let mut end = CITE_LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        match save(&cite.text) {
            Some(file) => {
                about.push_str(&format!(
                    "\nIt is long, so this is its start. All of it is in {}.",
                    file.display()
                ));
                files.push(file);
            }
            None => about.push_str("\nIt is long, so this is its start."),
        }
        text = &text[..end];
    }

    let fence = "`".repeat(longest_backtick_run(text).max(2) + 1);
    format!(
        "{about}\n\n{fence}{}\n{text}\n{fence}",
        cite.language.unwrap_or("")
    )
}

/// A mention as the agent reads it: what it names, where it is, and the file
/// or folder it is saved in.
pub(crate) fn describe(mention: &Mention) -> String {
    let name = &mention.name;
    let path = mention.path.display();

    match &mention.kind {
        MentionKind::Request {
            method,
            url,
            collection,
            folders,
        } => {
            let mut text =
                format!("Request \"{name}\" ({method} {url}) in the collection \"{collection}\"");
            if !folders.is_empty() {
                text.push_str(&format!(", folder \"{}\"", folders.join("/")));
            }
            text.push_str(&format!(".\nSaved in {path}."));
            text
        }
        MentionKind::Flow => format!("Flow \"{name}\".\nSaved in {path}."),
        MentionKind::Collection => format!("Collection \"{name}\", the folder {path}."),
    }
}

/// Keep a long cite where the agent can read all of it. Dropping the path
/// deletes the file.
fn save(text: &str) -> Option<TempPath> {
    let mut file = tempfile::Builder::new()
        .prefix("request-eagle-cite-")
        .suffix(".txt")
        .tempfile()
        .ok()?;
    file.write_all(text.as_bytes()).ok()?;

    Some(file.into_temp_path())
}

fn longest_backtick_run(text: &str) -> usize {
    text.split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
}
