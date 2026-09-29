//! Tab content and its workspace interface. HTTP editing and response rendering
//! live here; the workspace owns tab selection, closing, and collection storage.
//! Collection, gRPC, and WebSocket pages implement `TabPage` independently.

mod actions;
mod collection_page;
mod page;
mod request_draft;
mod request_fields;
mod response_view;
mod script_editor;
mod script_intelligence;
mod section_count;
mod variable_input;
mod variables;
mod vim;

#[cfg(test)]
mod test_allocator;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use actions::SendRequest;
pub use collection_page::{CollectionPage, CollectionSettings, SaveCollection};
pub use page::{TabBadge, TabBadgeTone, TabPage, TabState, TabView};
pub use request_draft::RequestDraft;
