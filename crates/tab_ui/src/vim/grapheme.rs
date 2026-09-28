use gpui_kit::component::input::Rope;
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

pub(super) fn next(text: &Rope, offset: usize) -> usize {
    boundary(text, offset, true)
}

pub(super) fn previous(text: &Rope, offset: usize) -> usize {
    boundary(text, offset, false)
}

fn boundary(text: &Rope, offset: usize, forward: bool) -> usize {
    let mut cursor = GraphemeCursor::new(offset, text.len(), true);
    let (mut chunk, mut start) = text.chunk(offset);

    loop {
        let result = if forward {
            cursor.next_boundary(chunk, start)
        } else {
            cursor.prev_boundary(chunk, start)
        };

        match result {
            Ok(Some(boundary)) => return boundary,
            Ok(None) => return if forward { text.len() } else { 0 },
            Err(GraphemeIncomplete::NextChunk) => (chunk, start) = text.chunk(start + chunk.len()),
            Err(GraphemeIncomplete::PrevChunk) => (chunk, start) = text.chunk(start - 1),
            Err(GraphemeIncomplete::PreContext(end)) => {
                let (context, start) = text.chunk(end - 1);
                cursor.provide_context(&context[..end - start], start);
            }
            Err(GraphemeIncomplete::InvalidOffset) => {
                unreachable!("cursor is within its rope chunk")
            }
        }
    }
}
