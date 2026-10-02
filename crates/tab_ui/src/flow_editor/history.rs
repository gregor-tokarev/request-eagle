use flow::Flow;

/// The most edits that can be undone.
const LIMIT: usize = 200;

/// Snapshots of the flow before each edit. Flows are small, so whole
/// snapshots are simpler than recording each change.
#[derive(Default)]
pub(super) struct History {
    undo: Vec<Flow>,
    redo: Vec<Flow>,
    /// What the last edit changed. Typing into one setting is one edit until
    /// something else changes.
    last: Option<String>,
}

impl History {
    /// Remember `flow` before an edit. Edits with the same key as the
    /// previous one join it, so undo reverts all of them at once.
    pub fn record(&mut self, flow: &Flow, key: Option<String>) {
        if key.is_some() && key == self.last {
            return;
        }

        self.last = key;
        self.redo.clear();
        self.undo.push(flow.clone());
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
    }

    /// Restore the flow before the last edit. Returns whether there was one.
    pub fn undo(&mut self, flow: &mut Flow) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };

        self.last = None;
        self.redo.push(std::mem::replace(flow, previous));
        true
    }

    pub fn redo(&mut self, flow: &mut Flow) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };

        self.last = None;
        self.undo.push(std::mem::replace(flow, next));
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// End the current edit, so the next one is undone on its own.
    pub fn seal(&mut self) {
        self.last = None;
    }
}
