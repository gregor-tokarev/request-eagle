//! Edits to a flow that do not depend on the canvas: copying and pasting
//! blocks, and arranging them.

use std::collections::{HashMap, HashSet};

use flow::{Block, Flow};
use gpui_kit::{Point, Size};
use serde_json::{Value, json};

/// Marks flow blocks on the clipboard, which can hold any text.
const CLIPBOARD_KEY: &str = "request-eagle-flow-blocks";

/// The blocks with these IDs and the connections between them.
pub(super) fn copy(flow: &Flow, ids: &[String]) -> Flow {
    Flow {
        blocks: flow
            .blocks
            .iter()
            .filter(|block| ids.contains(&block.id))
            .cloned()
            .collect(),
        connections: flow
            .connections
            .iter()
            .filter(|connection| ids.contains(&connection.from) && ids.contains(&connection.to))
            .cloned()
            .collect(),
    }
}

pub(super) fn to_clipboard(copied: &Flow) -> String {
    json!({ CLIPBOARD_KEY: copied }).to_string()
}

/// Blocks copied from a flow, if the clipboard holds some.
pub(super) fn from_clipboard(text: &str) -> Option<Flow> {
    let mut value: Value = serde_json::from_str(text).ok()?;
    let copied: Flow = serde_json::from_value(value.get_mut(CLIPBOARD_KEY)?.take()).ok()?;
    (!copied.blocks.is_empty()).then_some(copied)
}

/// Add copied blocks with new IDs, moved by `offset`, keeping the
/// connections between them. Returns the new blocks' IDs.
pub(super) fn paste(flow: &mut Flow, copied: Flow, offset: Point<f32>) -> Vec<String> {
    let mut ids = HashMap::new();

    for mut block in copied.blocks {
        let id = flow.next_block_id();
        ids.insert(block.id.clone(), id.clone());
        block.id = id;
        block.x += offset.x;
        block.y += offset.y;
        flow.blocks.push(block);
    }

    for mut connection in copied.connections {
        if let (Some(from), Some(to)) = (ids.get(&connection.from), ids.get(&connection.to)) {
            connection.from = from.clone();
            connection.to = to.clone();
            flow.connect(connection);
        }
    }

    let mut pasted: Vec<String> = ids.into_values().collect();
    pasted.sort_by_key(|id| flow.blocks.iter().position(|block| &block.id == id));
    pasted
}

/// Place blocks in columns by how far they are from where data starts, so
/// connections run left to right. Connections that close a cycle are left
/// out when counting.
pub(super) fn arrange(flow: &mut Flow, size: impl Fn(&Block) -> Size<f32>) {
    const COLUMN_GAP: f32 = 96.;
    const ROW_GAP: f32 = 40.;

    let index: HashMap<&str, usize> = flow
        .blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id.as_str(), index))
        .collect();
    let mut successors = vec![Vec::new(); flow.blocks.len()];
    for connection in &flow.connections {
        if let (Some(&from), Some(&to)) = (
            index.get(connection.from.as_str()),
            index.get(connection.to.as_str()),
        ) && !successors[from].contains(&to)
        {
            successors[from].push(to);
        }
    }

    // Depth-first order, skipping connections back to a block still being
    // explored, gives the longest path to each block without cycles.
    let mut order = Vec::new();
    let mut state = vec![0u8; flow.blocks.len()];
    let mut back = HashSet::new();
    for root in roots_first(&successors) {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        let mut path = vec![(root, 0)];
        while let Some(&mut (block, ref mut next)) = path.last_mut() {
            if let Some(&successor) = successors[block].get(*next) {
                *next += 1;
                match state[successor] {
                    0 => {
                        state[successor] = 1;
                        path.push((successor, 0));
                    }
                    1 => {
                        back.insert((block, successor));
                    }
                    _ => {}
                }
                continue;
            }
            state[block] = 2;
            order.push(block);
            path.pop();
        }
    }

    let mut column = vec![0usize; flow.blocks.len()];
    for &block in order.iter().rev() {
        for &successor in &successors[block] {
            if !back.contains(&(block, successor)) {
                column[successor] = column[successor].max(column[block] + 1);
            }
        }
    }

    let columns = column.iter().copied().max().map_or(0, |last| last + 1);
    let mut x = 0.;
    for current in 0..columns {
        let members: Vec<usize> = (0..flow.blocks.len())
            .filter(|&block| column[block] == current)
            .collect();
        let width = members
            .iter()
            .map(|&block| size(&flow.blocks[block]).width)
            .fold(0., f32::max);

        let mut y = 0.;
        for block in members {
            let height = size(&flow.blocks[block]).height;
            flow.blocks[block].x = x;
            flow.blocks[block].y = y;
            y += height + ROW_GAP;
        }
        x += width + COLUMN_GAP;
    }
}

/// Every block, those that nothing connects into first, so exploring from
/// them finds a cycle's connection back to where data enters it.
pub(super) fn roots_first(successors: &[Vec<usize>]) -> Vec<usize> {
    let mut entered = vec![false; successors.len()];
    for targets in successors {
        for &target in targets {
            entered[target] = true;
        }
    }

    let (mut roots, rest): (Vec<usize>, Vec<usize>) =
        (0..successors.len()).partition(|&block| !entered[block]);
    roots.extend(rest);
    roots
}
