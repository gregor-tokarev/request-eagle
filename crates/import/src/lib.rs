//! Reads collections exported by other API clients: Postman collections and
//! OpenAPI specifications.

mod body;
mod document;
mod openapi;
mod postman;

#[cfg(test)]
mod document_tests;
#[cfg(test)]
mod openapi_tests;
#[cfg(test)]
mod postman_tests;

pub use document::{Import, ImportError, parse};
