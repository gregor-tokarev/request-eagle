//! Completions for the members of `pm` and of the objects it holds, from a
//! table TypeScript produced. `pm.` chains are what scripts complete most, and
//! answering them without the compiler lets it load later and unload sooner.

use std::{collections::HashMap, sync::OnceLock};

use lsp_types::{CompletionItem, CompletionTextEdit, Position, Range, TextEdit};
use request::ScriptPhase;

use super::compiler::interface_name;

/// For each phase's `pm` interface, the members TypeScript completes after
/// each `pm` path, such as `pm.request.headers`. Their edits are left out.
type Table = HashMap<String, HashMap<String, Vec<CompletionItem>>>;

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();

    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("pm_members.json")).expect("pm member table")
    })
}

/// Whether `ch` can continue a JavaScript identifier.
fn is_identifier(ch: char) -> bool {
    unicode_ident::is_xid_continue(ch) || matches!(ch, '$' | '\u{200c}' | '\u{200d}')
}

/// The members to complete at `offset`, when it follows a `pm` path the table
/// has, as in `pm.request.he|`. `None` leaves the position to TypeScript.
pub(crate) fn completions(
    source: &str,
    offset: usize,
    phase: ScriptPhase,
) -> Option<Vec<CompletionItem>> {
    let before = &source[..offset];
    if !scan_code(before, |_, _| {}) || !pm_is_global(source) {
        return None;
    }

    let typed_start = before
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_identifier(*ch))
        .last()
        .map_or(offset, |(index, _)| index);
    let typed = &before[typed_start..];
    let path = before[..typed_start].strip_suffix('.')?;

    let path_start = path
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_identifier(*ch) || *ch == '.')
        .last()
        .map_or(path.len(), |(index, _)| index);
    let path = &path[path_start..];
    if path != "pm" && !path.starts_with("pm.") {
        return None;
    }

    let members = table().get(interface_name(phase))?.get(path)?;
    let after = source[offset..]
        .find(|ch| !is_identifier(ch))
        .map_or(source.len(), |length| offset + length);
    let range = Range::new(position(source, typed_start), position(source, after));

    Some(
        members
            .iter()
            .filter(|member| member.label.starts_with(typed))
            .take(100)
            .map(|member| {
                let mut item = member.clone();
                let new_text = item
                    .insert_text
                    .take()
                    .unwrap_or_else(|| item.label.clone());
                item.filter_text = Some(typed.to_owned());
                item.text_edit = Some(CompletionTextEdit::Edit(TextEdit { range, new_text }));
                item
            })
            .collect(),
    )
}

/// An LSP position: the line counts line feeds only, as the editor does, and
/// the character counts UTF-16 code units.
fn position(source: &str, offset: usize) -> Position {
    let before = &source[..offset];
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);

    Position::new(
        before.matches('\n').count() as u32,
        before[line_start..].encode_utf16().count() as u32,
    )
}

/// Whether every `pm` in the code of `source` starts a member access, as in
/// `pm.request`. Otherwise `pm` may name a parameter or a variable, and
/// TypeScript tells which.
fn pm_is_global(source: &str) -> bool {
    // Whether the identifier from `start` to `end`, between the code `before`
    // and `next` to it, is a `pm` that does not start a member access.
    let bound = |start: usize, end: usize, before: Option<char>, next: Option<char>| {
        &source[start..end] == "pm" && before != Some('.') && next != Some('.')
    };
    let mut global = true;
    let mut previous = None;
    // Where the current identifier starts, and the code before it.
    let mut word = None;

    let complete = scan_code(source, |index, ch| {
        if is_identifier(ch) {
            word.get_or_insert((index, previous));
        } else if let Some((start, before)) = word.take() {
            global &= !bound(start, index, before, Some(ch));
        }
        previous = Some(ch);
    });
    if let Some((start, before)) = word {
        global &= !bound(start, source.len(), before, None);
    }

    complete && global
}

/// Calls `visit` with each character of `source` that is code, rather than a
/// comment or a string, and returns whether `source` ends in code. After a `/`
/// that may start a regular expression, it stops and returns false, leaving
/// the rest to TypeScript.
fn scan_code(source: &str, mut visit: impl FnMut(usize, char)) -> bool {
    enum State {
        Code,
        LineComment,
        BlockComment,
        Quoted(char),
    }

    let mut state = State::Code;
    // Open template literals, each with the depth of braces in its current
    // `${…}` substitution.
    let mut templates: Vec<usize> = Vec::new();
    let mut in_template_text = false;
    let mut chars = source.char_indices().peekable();

    while let Some((index, ch)) = chars.next() {
        let next = chars.peek().map(|(_, next)| *next);

        if in_template_text {
            match ch {
                '\\' => {
                    chars.next();
                }
                '`' => {
                    templates.pop();
                    in_template_text = false;
                }
                '$' if next == Some('{') => {
                    chars.next();
                    in_template_text = false;
                }
                _ => {}
            }
            continue;
        }

        match state {
            State::Code => match ch {
                '/' if next == Some('/') => state = State::LineComment,
                '/' if next == Some('*') => {
                    chars.next();
                    state = State::BlockComment;
                }
                '/' => return false,
                '"' | '\'' => state = State::Quoted(ch),
                '`' => {
                    templates.push(0);
                    in_template_text = true;
                }
                '{' => {
                    if let Some(depth) = templates.last_mut() {
                        *depth += 1;
                    }
                    visit(index, ch);
                }
                '}' => match templates.last_mut() {
                    Some(0) => in_template_text = true,
                    Some(depth) => {
                        *depth -= 1;
                        visit(index, ch);
                    }
                    None => visit(index, ch),
                },
                _ => visit(index, ch),
            },
            State::LineComment => {
                if ch == '\n' {
                    state = State::Code;
                }
            }
            State::BlockComment => {
                if ch == '*' && next == Some('/') {
                    chars.next();
                    state = State::Code;
                }
            }
            State::Quoted(quote) => match ch {
                '\\' => {
                    chars.next();
                }
                '\n' => state = State::Code,
                _ if ch == quote => state = State::Code,
                _ => {}
            },
        }
    }

    matches!(state, State::Code) && !in_template_text
}
