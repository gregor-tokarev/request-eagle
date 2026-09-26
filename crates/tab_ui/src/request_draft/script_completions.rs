use gpui_kit::component::input::{CompletionProvider, EditorState, RopeExt};
use gpui_kit::{Action, App, Entity, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    TextEdit,
};
use request::ScriptPhase;
use ropey::Rope;
use tree_sitter::{Node, Parser};

pub(super) struct ScriptCompletions(pub ScriptPhase);

pub(super) fn capture_completion_action<A: Action>(
    editor: &Entity<EditorState>,
) -> impl Fn(&A, &mut Window, &mut App) + use<A> {
    let editor = editor.clone();

    move |action, window, cx| {
        let handled = editor.update(cx, |editor, cx| {
            editor.route_overlay_action(action.boxed_clone(), window, cx)
        });

        // GPUI Kit 0.6.2 propagates handled menu actions. Consume them here so
        // Enter does not also insert a newline and arrows do not move the caret.
        if handled {
            cx.stop_propagation();
        }
    }
}

impl CompletionProvider for ScriptCompletions {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let text = text.clone();
        let phase = self.0;

        cx.background_executor().spawn(async move {
            Ok(CompletionResponse::Array(completion_items(
                &text, offset, phase,
            )))
        })
    }

    fn is_completion_trigger(&self, _: usize, _: &str, _: &mut App) -> bool {
        // Refresh on deletion and punctuation too, so an old member list cannot
        // remain open after typing a space, entering a string, or changing scope.
        true
    }
}

/// Complete known sandbox objects without executing any part of the script.
/// Parsing a marker at the cursor distinguishes member access from strings,
/// comments and regex literals, and handles nested assertion arguments.
pub(super) fn completion_items(
    text: &Rope,
    offset: usize,
    phase: ScriptPhase,
) -> Vec<CompletionItem> {
    let source = text.to_string();
    if !source.is_char_boundary(offset) {
        return Vec::new();
    }

    let start = source[..offset]
        .char_indices()
        .rev()
        .find(|(_, ch)| !identifier_char(*ch))
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    let end = source[offset..]
        .char_indices()
        .find(|(_, ch)| !identifier_char(*ch))
        .map_or(source.len(), |(index, _)| offset + index);
    let prefix = &source[start..offset];
    let mut marked = source.clone();
    marked.replace_range(start..end, "__eagle_completion");

    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(&marked, None) else {
        return Vec::new();
    };
    let Some(node) = tree.root_node().descendant_for_byte_range(start, start + 1) else {
        return Vec::new();
    };
    let receiver = if node.kind() == "property_identifier" {
        let Some(parent) = node
            .parent()
            .filter(|node| node.kind() == "member_expression")
        else {
            return Vec::new();
        };
        let Some(object) = parent.child_by_field_name("object") else {
            return Vec::new();
        };
        let Some(receiver) = receiver(object, &marked, 0) else {
            return Vec::new();
        };
        receiver
    } else if node.kind() == "identifier" && !prefix.is_empty() {
        // Offer globals in expressions, not declaration names or object keys.
        let parent = node.parent();
        if parent.is_some_and(|parent| {
            matches!(parent.kind(), "formal_parameters" | "member_expression")
                || parent.child_by_field_name("name") == Some(node)
                || parent.child_by_field_name("key") == Some(node)
                || parent.child_by_field_name("parameter") == Some(node)
        }) {
            return Vec::new();
        }
        String::new()
    } else {
        return Vec::new();
    };

    if phase == ScriptPhase::PreRequest && receiver.starts_with("pm.response") {
        return Vec::new();
    }

    let range = lsp_types::Range::new(text.offset_to_position(start), text.offset_to_position(end));
    members(&receiver)
        .iter()
        .filter(|(name, _)| name.starts_with(prefix))
        .filter(|(name, _)| {
            !(phase == ScriptPhase::PreRequest && receiver == "pm" && *name == "response")
        })
        .map(|(name, detail)| CompletionItem {
            label: (*name).into(),
            detail: Some((*detail).into()),
            kind: Some(if detail.starts_with('(') {
                CompletionItemKind::METHOD
            } else {
                CompletionItemKind::PROPERTY
            }),
            // The native editor does not expand LSP snippets. Insert the name,
            // replacing the whole current member, including a suffix after the
            // caret. Show parameters in the menu instead of inserting placeholders.
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: (*name).into(),
            })),
            filter_text: Some(prefix.into()),
            ..Default::default()
        })
        .collect()
}

fn identifier_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '$')
}

fn receiver(node: Node<'_>, source: &str, depth: usize) -> Option<String> {
    if depth > 64 {
        return None;
    }

    match node.kind() {
        "identifier" => Some(source[node.byte_range()].to_owned()),
        "parenthesized_expression" => receiver(node.named_child(0)?, source, depth + 1),
        "member_expression" => {
            let object = receiver(node.child_by_field_name("object")?, source, depth + 1)?;
            let property = node.child_by_field_name("property")?;
            let name = &source[property.byte_range()];

            if object == "pm.expect()"
                && members("pm.expect()").iter().any(|(member, detail)| {
                    *member == name
                        && (!detail.starts_with('(')
                            || matches!(
                                name,
                                "a" | "an"
                                    | "include"
                                    | "includes"
                                    | "contain"
                                    | "contains"
                                    | "length"
                                    | "lengthOf"
                            ))
                })
            {
                Some(object)
            } else {
                Some(format!("{object}.{name}"))
            }
        }
        "call_expression" => {
            let function = receiver(node.child_by_field_name("function")?, source, depth + 1)?;
            if function == "pm.expect"
                || function == "pm.expect()"
                || function.strip_prefix("pm.expect().").is_some_and(|name| {
                    members("pm.expect()")
                        .iter()
                        .any(|(member, detail)| *member == name && detail.starts_with('('))
                })
            {
                Some("pm.expect()".into())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn members(receiver: &str) -> &'static [(&'static str, &'static str)] {
    match receiver {
        "" => &[
            ("pm", "Request scripting API"),
            ("console", "Script console"),
            ("JSON", "JSON utilities"),
        ],
        "pm" => &[
            ("request", "Outgoing request"),
            ("response", "Received response"),
            ("variables", "Variables for this run"),
            ("test", "(name, callback)"),
            ("expect", "(value, message?)"),
        ],
        "pm.variables" => &[
            ("get", "(name)"),
            ("set", "(name, value)"),
            ("has", "(name)"),
            ("unset", "(name)"),
            ("clear", "()"),
            ("toObject", "()"),
            ("replaceIn", "(text)"),
        ],
        "pm.request" => &[
            ("method", "HTTP method"),
            ("url", "URL string"),
            ("headers", "Request headers"),
            ("body", "Request body"),
        ],
        "pm.request.headers" => &[
            ("get", "(name)"),
            ("has", "(name)"),
            ("add", "({key, value})"),
            ("upsert", "({key, value})"),
            ("remove", "(name)"),
            ("toJSON", "()"),
        ],
        "pm.request.body" => &[
            ("mode", "Body mode"),
            ("raw", "Body text"),
            ("update", "(text)"),
        ],
        "pm.response" => &[
            ("code", "Status code"),
            ("status", "Status text"),
            ("responseTime", "Elapsed milliseconds"),
            ("headers", "Response headers"),
            ("text", "()"),
            ("json", "()"),
            ("to", "Response assertions"),
        ],
        "pm.response.headers" => &[("get", "(name)"), ("has", "(name)"), ("toJSON", "()")],
        "pm.response.to" => &[
            ("have", "Response assertions"),
            ("be", "Response assertions"),
        ],
        "pm.response.to.have" => &[
            ("status", "(codeOrReason)"),
            ("header", "(name, value?)"),
            ("body", "(textOrObjectOrPattern?)"),
            ("jsonBody", "(path?, value?)"),
        ],
        "pm.response.to.be" => &[
            ("ok", "Status 200"),
            ("success", "Status 2xx"),
            ("error", "Status 4xx or 5xx"),
            ("clientError", "Status 4xx"),
            ("serverError", "Status 5xx"),
            ("json", "Valid JSON body"),
        ],
        "pm.expect()" => &[
            ("to", "Chain"),
            ("be", "Chain"),
            ("been", "Chain"),
            ("is", "Chain"),
            ("that", "Chain"),
            ("which", "Chain"),
            ("and", "Chain"),
            ("has", "Chain"),
            ("have", "Chain"),
            ("with", "Chain"),
            ("at", "Chain"),
            ("of", "Chain"),
            ("same", "Chain"),
            ("not", "Negate"),
            ("deep", "Deep comparison"),
            ("nested", "Nested property path"),
            ("own", "Own properties"),
            ("any", "Any key"),
            ("all", "All keys"),
            ("ordered", "Ordered members"),
            ("equal", "(expected)"),
            ("equals", "(expected)"),
            ("eq", "(expected)"),
            ("eql", "(expected)"),
            ("include", "(expected)"),
            ("includes", "(expected)"),
            ("contain", "(expected)"),
            ("contains", "(expected)"),
            ("property", "(name, expected?)"),
            ("keys", "(...names)"),
            ("members", "(expected)"),
            ("oneOf", "(values)"),
            ("a", "(type)"),
            ("an", "(type)"),
            ("above", "(minimum)"),
            ("below", "(maximum)"),
            ("least", "(minimum)"),
            ("most", "(maximum)"),
            ("within", "(minimum, maximum)"),
            ("closeTo", "(expected, delta)"),
            ("greaterThan", "(minimum)"),
            ("lessThan", "(maximum)"),
            ("gte", "(minimum)"),
            ("lte", "(maximum)"),
            ("length", "(length)"),
            ("lengthOf", "(length)"),
            ("match", "(pattern)"),
            ("true", "Boolean assertion"),
            ("false", "Boolean assertion"),
            ("null", "Null assertion"),
            ("undefined", "Undefined assertion"),
            ("ok", "Truthy assertion"),
            ("empty", "Empty assertion"),
            ("exist", "Not null or undefined"),
            ("exists", "Not null or undefined"),
            ("NaN", "Not-a-number assertion"),
            ("finite", "Finite number"),
        ],
        "console" => &[
            ("log", "(...values)"),
            ("info", "(...values)"),
            ("warn", "(...values)"),
            ("error", "(...values)"),
            ("debug", "(...values)"),
        ],
        "JSON" => &[
            ("parse", "(text, reviver?)"),
            ("stringify", "(value, replacer?, space?)"),
        ],
        _ => &[],
    }
}
