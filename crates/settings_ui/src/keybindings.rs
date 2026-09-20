mod keycaps;
mod page;
mod recorder;
mod row;
mod search;

use keycaps::{shortcut, shortcut_keycaps};
use page::matches_search;

#[cfg(test)]
mod tests;

pub(super) use page::KeybindingsPage;
