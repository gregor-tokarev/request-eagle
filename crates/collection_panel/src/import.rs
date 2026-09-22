use collection::{CollectionEditError, ImportedFile, ImportedRequest};
use gpui_kit::{Context, ScrollStrategy, Window};

use super::CollectionPanel;

impl CollectionPanel {
    pub fn import_requests(
        &mut self,
        name: &str,
        requests: Vec<ImportedRequest>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Vec<ImportedFile>, CollectionEditError> {
        let imported = self.collections.import_requests(name, requests)?;
        self.rename = None;
        self.pending_delete = None;
        self.error = None;
        self.query.clear();
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.rebuild_tree(imported.first().map(|file| file.path.as_path()), None, cx);

        if let Some(row) = self.selected_row {
            self.scroll_handle
                .scroll_to_item(row, ScrollStrategy::Nearest);
        }

        cx.notify();
        Ok(imported)
    }
}
