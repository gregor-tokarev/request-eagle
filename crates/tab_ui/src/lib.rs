//! The pages shown in workspace tabs: HTTP and gRPC request drafts with their
//! responses, collection settings and global environment editors. The workspace owns tab
//! selection, closing and collection storage.
//!
//! `Environments` lives here rather than in the workspace because request drafts
//! resolve variables from the active global environment.

mod actions;
mod collection_page;
mod environment_editor;
mod environments;
mod grpc_draft;
mod grpc_response;
mod request_draft;
mod response_view;
mod script_editor;
mod section_count;
mod variable_input;
mod variable_table;
mod variables;
mod vim;

#[cfg(test)]
mod collection_page_tests;
#[cfg(test)]
mod environment_editor_tests;
#[cfg(test)]
mod test_allocator;
#[cfg(feature = "test-support")]
pub mod test_support;
#[cfg(test)]
mod variable_input_tests;

pub use actions::SendRequest;
pub use collection_page::{CollectionPage, CollectionSettings, SaveCollection};
pub use environment_editor::EnvironmentEditor;
pub use environments::{Environments, EnvironmentsEvent};
pub use grpc_draft::GrpcDraft;
pub use request_draft::{RequestDraft, RequestLocation};
