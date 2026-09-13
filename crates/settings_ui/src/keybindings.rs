mod page;
mod recorder;
mod search;

use page::{matches_search, shortcut_keycaps};

#[cfg(test)]
mod tests;

pub(super) use page::KeybindingsPage;
