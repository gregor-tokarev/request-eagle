//! What a message can mention: saved requests, flows and collections, found
//! by typing `@` and part of a name.

use std::{ops::Range, path::PathBuf};

use gpui_kit::SharedString;

/// A saved request, flow or collection named in a message.
#[derive(Clone, Debug, PartialEq)]
pub struct Mention {
    pub kind: MentionKind,
    pub name: SharedString,
    /// The file a request or flow is saved in, or a collection's folder.
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MentionKind {
    Request {
        /// Its HTTP method, or `gRPC` or `WS`.
        method: &'static str,
        /// Where it is sent, as written.
        url: String,
        collection: String,
        /// The folders between the collection and the request, outermost
        /// first.
        folders: Vec<String>,
    },
    Flow,
    Collection,
}

impl Mention {
    /// Where the menu says it is: a request's collection and folders, or
    /// what it is.
    pub(crate) fn location(&self) -> String {
        match &self.kind {
            MentionKind::Request {
                collection,
                folders,
                ..
            } => std::iter::once(collection.as_str())
                .chain(folders.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" › "),
            MentionKind::Flow => "Flow".to_owned(),
            MentionKind::Collection => "Collection".to_owned(),
        }
    }
}

/// The `@` before `cursor` and what is typed after it, while that is a
/// mention being written: the `@` starts the text or follows a space, no
/// space is between it and the cursor, and it is not in one of `tokens`.
pub(crate) fn typed_mention(
    text: &str,
    cursor: usize,
    tokens: &[Range<usize>],
) -> Option<Range<usize>> {
    let before = text.get(..cursor)?;
    let start = before.rfind('@')?;

    let starts_word = before[..start]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace);
    let one_word = !before[start..].contains(char::is_whitespace);
    let in_token = tokens.iter().any(|token| token.contains(&start));

    (starts_word && one_word && !in_token).then_some(start..cursor)
}

/// The mentions that match `query`, by their index, best first: names that
/// start with it, then names with a word that starts with it, then names
/// that have it, spaces aside, then those whose method, URL, collection or
/// folder has it. Each group keeps the order of `mentions`. Case does not
/// matter, and an empty query matches all of them.
pub(crate) fn matching(mentions: &[Mention], query: &str) -> Vec<usize> {
    let query: String = query
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_lowercase();

    let mut ranked: Vec<(usize, usize)> = mentions
        .iter()
        .enumerate()
        .filter_map(|(index, mention)| Some((rank(mention, &query)?, index)))
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);

    ranked.into_iter().map(|(_, index)| index).collect()
}

fn rank(mention: &Mention, query: &str) -> Option<usize> {
    let name = mention.name.to_lowercase();
    let compact: String = name
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    if compact.starts_with(query) {
        return Some(0);
    }
    if name
        .split(|character: char| !character.is_alphanumeric())
        .any(|word| word.starts_with(query))
    {
        return Some(1);
    }
    if compact.contains(query) {
        return Some(2);
    }

    let mut details = mention.location();
    if let MentionKind::Request { method, url, .. } = &mention.kind {
        details.push_str(&format!(" {method} {url}"));
    }
    details.to_lowercase().contains(query).then_some(3)
}
