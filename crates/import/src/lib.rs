//! Reads collections exported by other API clients: Postman collections,
//! including the folders Postman writes in Git-connected workspaces, and
//! OpenAPI specifications. Also reads single requests written as cURL commands.

mod body;
mod curl;
mod document;
mod openapi;
mod postman;
mod postman_v3;

#[cfg(test)]
mod curl_tests;
#[cfg(test)]
mod document_tests;
#[cfg(test)]
mod openapi_tests;
#[cfg(test)]
mod postman_tests;
#[cfg(test)]
mod postman_v3_tests;

pub use curl::{CurlError, is_curl, parse_curl};
pub use document::{Import, ImportError, parse, read};
