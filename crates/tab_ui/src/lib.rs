//! The pages shown in workspace tabs: HTTP and gRPC request drafts with their
//! responses, WebSocket drafts with their message logs, flow canvases,
//! collection settings, the Collection Runner, global environment editors and
//! the cookie jar. The workspace owns tab selection, closing and collection
//! storage.
//!
//! `Environments` lives here rather than in the workspace because request drafts
//! resolve variables from the active global environment. `Cookies` lives here
//! for the same reason: requests store and send the jar's cookies.

mod actions;
mod auth_editor;
mod code_snippet;
mod collection_page;
mod collection_runner;
mod cookie_page;
mod cookies;
mod environment_editor;
mod environments;
mod flow_editor;
mod grpc_draft;
mod grpc_response;
mod request_draft;
mod request_sent;
mod request_settings;
mod response_view;
mod script_editor;
mod section_count;
mod variable_input;
mod variable_table;
mod variables;
mod vim;
mod websocket_draft;

#[cfg(test)]
mod variable_input_tests;

pub use actions::SendRequest;
pub use collection_page::{CollectionPage, CollectionSettings, RunCollection, SaveCollection};
pub use collection_runner::CollectionRunner;
pub use cookie_page::CookiePage;
pub use cookies::Cookies;
pub use environment_editor::EnvironmentEditor;
pub use environments::{Environments, EnvironmentsEvent};
pub use flow_editor::{
    AddBlock, ArrangeBlocks, CopyBlocks, DeleteSelection, DuplicateBlocks, FlowEditor, FlowRequest,
    FlowRequests, PasteBlocks, RedoFlowEdit, SelectAllBlocks, StopFlow, UndoFlowEdit, ZoomIn,
    ZoomOut, ZoomToFit,
};
pub use grpc_draft::GrpcDraft;
pub use request_draft::{RequestDraft, RequestLocation};
pub use request_sent::RequestSent;
pub use websocket_draft::WebSocketDraft;
