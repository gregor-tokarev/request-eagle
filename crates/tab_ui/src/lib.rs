//! The pages shown in workspace tabs: HTTP request drafts with their responses,
//! collection settings and global environment editors. The workspace owns tab
//! selection, closing and collection storage.
//!
//! `Environments` lives here rather than in the workspace because request drafts
//! resolve variables from the active global environment.

mod actions;
mod collection_page;
mod environment_editor;
mod environments;
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
mod environment_editor_tests;
#[cfg(test)]
mod test_allocator;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use actions::SendRequest;
pub use collection_page::{CollectionPage, CollectionSettings, SaveCollection};
pub use environment_editor::EnvironmentEditor;
pub use environments::{Environments, EnvironmentsEvent};
pub use request_draft::RequestDraft;
