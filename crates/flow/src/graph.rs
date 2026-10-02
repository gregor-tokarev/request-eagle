//! How the blocks of a flow connect, worked out once before a run.

use std::collections::{HashMap, HashSet};

use crate::{BlockKind, Flow};

/// Frames of cycles have this bit set, which keeps them apart from loop runs.
pub(crate) const CYCLE: u64 = 1 << 63;

pub(crate) struct Target {
    pub block: usize,
    pub input: String,
    /// The connection leads back to an earlier block of a cycle, so what it
    /// carries starts the cycle's next pass.
    pub back: bool,
}

pub(crate) struct Graph {
    /// For each block, the inputs each output is connected to.
    pub targets: Vec<HashMap<String, Vec<Target>>>,
    /// For each block, its inputs that have a connection.
    pub connected: Vec<Vec<String>>,
    /// For each For and Repeat block, the Collect blocks that end its loop.
    pub collects: Vec<Vec<usize>>,
    /// For each block in a cycle, the frame that counts the cycle's passes.
    pub cycle: Vec<Option<u64>>,
}

impl Graph {
    pub fn new(flow: &Flow) -> Self {
        let count = flow.blocks.len();
        let index: HashMap<&str, usize> = flow
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.id.as_str(), index))
            .collect();
        let mut targets: Vec<HashMap<String, Vec<Target>>> =
            (0..count).map(|_| HashMap::new()).collect();
        let mut connected = vec![Vec::new(); count];
        let mut successors = vec![Vec::new(); count];

        for connection in &flow.connections {
            let (Some(&from), Some(&to)) = (
                index.get(connection.from.as_str()),
                index.get(connection.to.as_str()),
            ) else {
                continue;
            };

            targets[from]
                .entry(connection.output.clone())
                .or_default()
                .push(Target {
                    block: to,
                    input: connection.input.clone(),
                    back: false,
                });
            if !connected[to].contains(&connection.input) {
                connected[to].push(connection.input.clone());
            }
            if !successors[from].contains(&to) {
                successors[from].push(to);
            }
        }

        let cycle = cycles(&successors);
        for (from, back_to) in back_edges(&successors) {
            for target in targets[from].values_mut().flatten() {
                if target.block == back_to {
                    target.back = true;
                }
            }
        }

        let collects = (0..count)
            .map(|block| match flow.blocks[block].kind {
                BlockKind::For | BlockKind::Repeat => loop_collects(flow, &successors, block),
                _ => Vec::new(),
            })
            .collect();

        Self {
            targets,
            connected,
            collects,
            cycle,
        }
    }
}

/// The Collect blocks that close the loop of `start`, pairing loops and
/// Collect blocks like brackets, so a nested loop's Collect is passed by.
fn loop_collects(flow: &Flow, successors: &[Vec<usize>], start: usize) -> Vec<usize> {
    // Loops nest no deeper than there are loops, unless a cycle passes the
    // same loop again, which nests nothing more.
    let loops = flow
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::For | BlockKind::Repeat))
        .count();
    let mut collects = Vec::new();
    let mut visited = HashSet::new();
    let mut pending: Vec<(usize, usize)> =
        successors[start].iter().map(|&block| (block, 0)).collect();

    while let Some((block, depth)) = pending.pop() {
        if block == start || depth > loops || !visited.insert((block, depth)) {
            continue;
        }

        let depth = match flow.blocks[block].kind {
            BlockKind::For | BlockKind::Repeat => depth + 1,
            BlockKind::Collect if depth == 0 => {
                if !collects.contains(&block) {
                    collects.push(block);
                }
                continue;
            }
            BlockKind::Collect => depth - 1,
            _ => depth,
        };

        pending.extend(successors[block].iter().map(|&next| (next, depth)));
    }

    collects
}

/// The cycle frame of each block in a cycle: blocks of one strongly
/// connected component share it.
fn cycles(successors: &[Vec<usize>]) -> Vec<Option<u64>> {
    // Tarjan's algorithm, without recursion so large flows cannot overflow
    // the stack.
    let count = successors.len();
    let mut order = vec![usize::MAX; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut cycle = vec![None; count];
    let mut next_order = 0;
    let mut components = 0;

    for root in 0..count {
        if order[root] != usize::MAX {
            continue;
        }

        let mut path = vec![(root, 0)];
        order[root] = next_order;
        low[root] = next_order;
        next_order += 1;
        stack.push(root);
        on_stack[root] = true;

        while let Some(&mut (block, ref mut next)) = path.last_mut() {
            if let Some(&successor) = successors[block].get(*next) {
                *next += 1;
                if order[successor] == usize::MAX {
                    order[successor] = next_order;
                    low[successor] = next_order;
                    next_order += 1;
                    stack.push(successor);
                    on_stack[successor] = true;
                    path.push((successor, 0));
                } else if on_stack[successor] {
                    low[block] = low[block].min(order[successor]);
                }
                continue;
            }

            path.pop();
            if let Some(&(parent, _)) = path.last() {
                low[parent] = low[parent].min(low[block]);
            }

            if low[block] == order[block] {
                let mut members = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack[member] = false;
                    members.push(member);
                    if member == block {
                        break;
                    }
                }

                let looped = members.len() > 1 || successors[block].contains(&block);
                if looped {
                    for member in members {
                        cycle[member] = Some(CYCLE | components);
                    }
                    components += 1;
                }
            }
        }
    }

    cycle
}

/// The connections that lead back to a block whose data is still being
/// followed when exploring the flow from where data enters it. Each cycle
/// has at least one.
fn back_edges(successors: &[Vec<usize>]) -> Vec<(usize, usize)> {
    let count = successors.len();
    // 0: unvisited, 1: being explored, 2: done.
    let mut state = vec![0u8; count];
    let mut back = Vec::new();

    // Blocks nothing connects into come first, so a cycle's connection back
    // to where data enters it is the one found.
    let mut entered = vec![false; count];
    for &target in successors.iter().flatten() {
        entered[target] = true;
    }
    let (mut roots, rest): (Vec<usize>, Vec<usize>) =
        (0..count).partition(|&block| !entered[block]);
    roots.extend(rest);

    for root in roots {
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
                    1 => back.push((block, successor)),
                    _ => {}
                }
                continue;
            }

            state[block] = 2;
            path.pop();
        }
    }

    back
}
