//! TypeScript completions, hover and signature help for request scripts. The
//! embedded TypeScript compiler runs in QuickJS on one shared worker thread.
//! Members of `pm` complete from a table, without the compiler.

mod compiler;
mod members;
mod worker;

#[cfg(test)]
mod tests;

pub use worker::{completions, hover, is_running, signature_help};
