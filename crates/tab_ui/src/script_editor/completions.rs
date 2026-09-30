use gpui_kit::component::input::{CompletionProvider, EditorState, HoverProvider, RopeExt};
use gpui_kit::{Action, App, Entity, Task, WeakEntity, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionResponse, CompletionTextEdit, Hover, Position,
    Range,
};
use request::ScriptPhase;
use ropey::{Rope, extra::esoterica::ropes_are_instances};

// GPUI Kit 0.6.2's editor positions count Unicode scalar values, while the
// language service returns standard LSP UTF-16 columns. Keep the conversion at
// the editor boundary so a preceding emoji cannot shift or corrupt an edit.
fn editor_range(range: Range, text: &Rope) -> Range {
    let position = |position: Position| {
        let line = text.slice_line(position.line as usize);
        let end = line.utf16_to_byte_idx((position.character as usize).min(line.len_utf16()));
        Position::new(position.line, line.slice(..end).len_chars() as u32)
    };
    Range::new(position(range.start), position(range.end))
}

fn editor_completions(mut items: Vec<CompletionItem>, text: &Rope) -> Vec<CompletionItem> {
    for item in &mut items {
        match &mut item.text_edit {
            Some(CompletionTextEdit::Edit(edit)) => edit.range = editor_range(edit.range, text),
            Some(CompletionTextEdit::InsertAndReplace(edit)) => {
                edit.insert = editor_range(edit.insert, text);
                edit.replace = editor_range(edit.replace, text);
            }
            None => {}
        }
    }
    items
}

pub(super) struct ScriptCompletions {
    phase: ScriptPhase,
    editor: WeakEntity<EditorState>,
}

impl ScriptCompletions {
    pub(super) fn new(phase: ScriptPhase, editor: &Entity<EditorState>) -> Self {
        Self {
            phase,
            editor: editor.downgrade(),
        }
    }
}

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
        let phase = self.phase;
        let editor = self.editor.clone();

        // Keep the current menu visible while TypeScript refreshes its items.
        // The patched native menu checks its document/caret snapshot before
        // accepting an item, so old edit ranges cannot modify newer text.

        cx.spawn(async move |cx| {
            let items = script_intelligence::completions(text.to_string(), offset, phase).await?;
            let current = editor
                .read_with(cx, |editor, _| {
                    editor.cursor() == offset
                        && (ropes_are_instances(editor.text(), &text) || editor.text() == &text)
                })
                .unwrap_or(false);

            Ok(CompletionResponse::Array(if current {
                editor_completions(items, &text)
            } else {
                Vec::new()
            }))
        })
    }

    fn is_completion_trigger(&self, _: usize, _: &str, _: &mut App) -> bool {
        // Also refresh after punctuation or deletion, clearing obsolete suggestions.
        true
    }
}

impl HoverProvider for ScriptCompletions {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Option<Hover>>> {
        let text = text.clone();
        let phase = self.phase;
        let editor = self.editor.clone();

        cx.spawn(async move |cx| {
            let mut hover = script_intelligence::hover(text.to_string(), offset, phase).await?;
            let current = editor
                .read_with(cx, |editor, _| {
                    ropes_are_instances(editor.text(), &text) || editor.text() == &text
                })
                .unwrap_or(false);

            if let Some(hover) = &mut hover {
                hover.range = hover.range.map(|range| editor_range(range, &text));
            }

            Ok(if current { hover } else { None })
        })
    }
}

#[cfg(test)]
pub(super) fn completion_items(
    text: &Rope,
    offset: usize,
    phase: ScriptPhase,
) -> Vec<lsp_types::CompletionItem> {
    let items = smol::block_on(script_intelligence::completions(
        text.to_string(),
        offset,
        phase,
    ))
    .unwrap();
    editor_completions(items, text)
}
