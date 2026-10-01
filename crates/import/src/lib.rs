//! Reads collections exported by other API clients: Postman collections,
//! including the folders Postman writes in Git-connected workspaces, and
//! OpenAPI specifications.

mod body;
mod document;
mod openapi;
mod postman;
mod postman_v3;

#[cfg(test)]
mod document_tests;
#[cfg(test)]
mod openapi_tests;
#[cfg(test)]
mod postman_tests;
#[cfg(test)]
mod postman_v3_tests;

pub use document::{Import, ImportError, parse, read};
