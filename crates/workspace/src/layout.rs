pub(crate) mod bottom_panel;
mod history;
pub(crate) mod main_view;
pub(crate) mod recovery;
pub(crate) mod request_draft;
mod request_fields;
mod response_view;
pub(crate) mod save_request;
pub(crate) mod top_panel;
pub(crate) mod workflow;

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod main_view_tests;

#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod save_request_tests;

#[cfg(test)]
mod workflow_tests;
