use gpui_kit::component::input::Rope;

/// Repeat a counted insertion only while the edit remains a single insertion
/// at the original position and the caret is at its end.
pub(super) struct Insertion {
    original: Rope,
    start: usize,
    count: usize,
    separator: &'static str,
}

impl Insertion {
    pub(super) fn new(original: Rope, start: usize, count: usize, separator: &'static str) -> Self {
        Self {
            original,
            start,
            count,
            separator,
        }
    }

    pub(super) fn finish(self, text: &Rope, cursor: usize) -> Option<String> {
        let added = text.len().checked_sub(self.original.len())?;

        if cursor != self.start + added
            || text.slice(..self.start) != self.original.slice(..self.start)
            || text.slice(cursor..) != self.original.slice(self.start..)
        {
            return None;
        }

        let inserted = text.slice(self.start..cursor).to_string();
        Some(format!("{}{inserted}", self.separator).repeat(self.count - 1))
    }
}
