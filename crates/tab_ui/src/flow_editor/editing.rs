//! Edits to a flow that do not depend on the canvas: copying and pasting
//! blocks, and arranging them.

use std::collections::{HashMap, HashSet};

use flow::{Block, BlockKind, Flow};
use gpui_kit::{Bounds, Point, Size, point, size};
use serde_json::{Value, json};

use super::geometry;

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

/// The space kept between a placed block and the others.
const PLACEMENT_GAP: f32 = 16.;
/// How far apart the places tried for a block are, and how many are tried.
const PLACEMENT_STEP: f32 = 24.;
const PLACEMENT_TRIES: usize = 80;

/// Where a block of `size` can go at or below `position` without covering
/// any of `taken`.
pub(super) fn free_spot(
    taken: &[Bounds<f32>],
    size: Size<f32>,
    position: Point<f32>,
) -> Point<f32> {
    let group = [Bounds {
        origin: position,
        size,
    }];
    let offset = free_offset(taken, &group, point(0., 0.));
    point(position.x + offset.x, position.y + offset.y)
}

/// How far to move `group` from where it is, starting at `start` and going
/// further down, for none of it to cover any of `taken`. Past the places
/// tried, it goes below every block.
pub(super) fn free_offset(
    taken: &[Bounds<f32>],
    group: &[Bounds<f32>],
    start: Point<f32>,
) -> Point<f32> {
    let covers = |offset: Point<f32>| {
        group.iter().any(|bounds| {
            let moved = Bounds {
                origin: point(
                    bounds.origin.x + offset.x - PLACEMENT_GAP,
                    bounds.origin.y + offset.y - PLACEMENT_GAP,
                ),
                size: size(
                    bounds.size.width + PLACEMENT_GAP * 2.,
                    bounds.size.height + PLACEMENT_GAP * 2.,
                ),
            };
            taken
                .iter()
                .any(|other| geometry::intersects(&moved, other))
        })
    };

    (0..PLACEMENT_TRIES)
        .map(|step| point(start.x, start.y + PLACEMENT_STEP * step as f32))
        .find(|offset| !covers(*offset))
        .unwrap_or_else(|| {
            let bottom = taken
                .iter()
                .map(|bounds| bounds.origin.y + bounds.size.height)
                .fold(f32::MIN, f32::max);
            let top = group
                .iter()
                .map(|bounds| bounds.origin.y)
                .fold(f32::MAX, f32::min);
            point(start.x, start.y.max(bottom + PLACEMENT_GAP * 2. - top))
        })
}

/// Place blocks in columns by how far they are from where data starts, so
/// connections run left to right. Connections that close a cycle are left
/// out when counting. The blocks a Note frames are arranged together inside
/// it, and the Note takes their place among the other blocks, growing or
/// shrinking to fit them.
pub(super) fn arrange(flow: &mut Flow, measure: impl Fn(&Block) -> Size<f32>) {
    /// Room around the blocks a Note frames, and above them for its text.
    const FRAME_PADDING: f32 = 24.;
    const FRAME_TEXT: f32 = 64.;

    let count = flow.blocks.len();
    let is_note = |index: usize| matches!(flow.blocks[index].kind, BlockKind::Note { .. });
    let bounds = |index: usize| Bounds {
        origin: point(flow.blocks[index].x, flow.blocks[index].y),
        size: measure(&flow.blocks[index]),
    };
    let index: HashMap<&str, usize> = flow
        .blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id.as_str(), index))
        .collect();
    let connections: Vec<(usize, usize)> = flow
        .connections
        .iter()
        .filter_map(|connection| {
            Some((
                *index.get(connection.from.as_str())?,
                *index.get(connection.to.as_str())?,
            ))
        })
        .collect();

    // A block belongs to the smallest Note around it.
    let frame: Vec<Option<usize>> = (0..count)
        .map(|block| {
            if is_note(block) {
                return None;
            }
            (0..count)
                .filter(|&other| {
                    is_note(other) && geometry::contains(&bounds(other), &bounds(block))
                })
                .min_by(|&a, &b| {
                    let area = |note: usize| bounds(note).size.width * bounds(note).size.height;
                    area(a).total_cmp(&area(b))
                })
        })
        .collect();

    // Arrange each Note's blocks within it.
    let mut inside = vec![point(0., 0.); count];
    let mut sizes: Vec<Size<f32>> = (0..count)
        .map(|block| measure(&flow.blocks[block]))
        .collect();
    for note in (0..count).filter(|&block| is_note(block)) {
        let members: Vec<usize> = (0..count)
            .filter(|&block| frame[block] == Some(note))
            .collect();
        if members.is_empty() {
            continue;
        }

        let local: HashMap<usize, usize> = members
            .iter()
            .enumerate()
            .map(|(local, &block)| (block, local))
            .collect();
        let member_connections: Vec<(usize, usize)> = connections
            .iter()
            .filter_map(|(from, to)| Some((*local.get(from)?, *local.get(to)?)))
            .collect();
        let member_sizes: Vec<Size<f32>> = members.iter().map(|&block| sizes[block]).collect();
        let positions = columns(&member_sizes, &member_connections);

        let mut content: Size<f32> = size(0., 0.);
        for (local, &block) in members.iter().enumerate() {
            inside[block] = positions[local];
            content.width = content
                .width
                .max(positions[local].x + member_sizes[local].width);
            content.height = content
                .height
                .max(positions[local].y + member_sizes[local].height);
        }
        sizes[note] = size(
            content.width + FRAME_PADDING * 2.,
            content.height + FRAME_TEXT + FRAME_PADDING,
        );
    }

    // Then the Notes and the blocks outside them, as one block each.
    let units: Vec<usize> = (0..count).filter(|&block| frame[block].is_none()).collect();
    let unit: HashMap<usize, usize> = units
        .iter()
        .enumerate()
        .map(|(position, &block)| (block, position))
        .collect();
    let of = |block: usize| unit[&frame[block].unwrap_or(block)];
    let unit_connections: Vec<(usize, usize)> = connections
        .iter()
        .map(|&(from, to)| (of(from), of(to)))
        .filter(|(from, to)| from != to)
        .collect();
    let unit_sizes: Vec<Size<f32>> = units.iter().map(|&block| sizes[block]).collect();
    let positions = columns(&unit_sizes, &unit_connections);

    for index in 0..count {
        let at = match frame[index] {
            Some(note) => {
                let origin = positions[unit[&note]];
                point(
                    origin.x + FRAME_PADDING + inside[index].x,
                    origin.y + FRAME_TEXT + inside[index].y,
                )
            }
            None => positions[unit[&index]],
        };
        let framing = frame.contains(&Some(index));
        let measured = sizes[index];
        let block = &mut flow.blocks[index];
        block.x = at.x;
        block.y = at.y;
        // A Note that frames blocks fits around them.
        if framing && let BlockKind::Note { width, height, .. } = &mut block.kind {
            *width = Some(measured.width);
            *height = Some(measured.height);
        }
    }
}

/// Where items of these sizes go in columns, so connections between them
/// run left to right. Connections that close a cycle are left out when
/// counting.
fn columns(sizes: &[Size<f32>], connections: &[(usize, usize)]) -> Vec<Point<f32>> {
    const COLUMN_GAP: f32 = 96.;
    const ROW_GAP: f32 = 40.;

    let count = sizes.len();
    let mut successors = vec![Vec::new(); count];
    for &(from, to) in connections {
        if !successors[from].contains(&to) {
            successors[from].push(to);
        }
    }

    // Depth-first order, skipping connections back to an item still being
    // explored, gives the longest path to each item without cycles.
    let mut order = Vec::new();
    let mut state = vec![0u8; count];
    let mut back = HashSet::new();
    for root in roots_first(&successors) {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        let mut path = vec![(root, 0)];
        while let Some(&mut (item, ref mut next)) = path.last_mut() {
            if let Some(&successor) = successors[item].get(*next) {
                *next += 1;
                match state[successor] {
                    0 => {
                        state[successor] = 1;
                        path.push((successor, 0));
                    }
                    1 => {
                        back.insert((item, successor));
                    }
                    _ => {}
                }
                continue;
            }
            state[item] = 2;
            order.push(item);
            path.pop();
        }
    }

    let mut column = vec![0usize; count];
    for &item in order.iter().rev() {
        for &successor in &successors[item] {
            if !back.contains(&(item, successor)) {
                column[successor] = column[successor].max(column[item] + 1);
            }
        }
    }

    let mut positions = vec![point(0., 0.); count];
    let columns = column.iter().copied().max().map_or(0, |last| last + 1);
    let mut x = 0.;
    for current in 0..columns {
        let members: Vec<usize> = (0..count).filter(|&item| column[item] == current).collect();
        let width = members
            .iter()
            .map(|&item| sizes[item].width)
            .fold(0., f32::max);

        let mut y = 0.;
        for item in members {
            positions[item] = point(x, y);
            y += sizes[item].height + ROW_GAP;
        }
        x += width + COLUMN_GAP;
    }

    positions
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
