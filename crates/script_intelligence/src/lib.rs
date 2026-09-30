//! TypeScript completions, hover and signature help for request scripts. The
//! embedded TypeScript compiler runs in QuickJS on one shared worker thread.

mod compiler;
mod worker;

#[cfg(test)]
mod tests;

pub use worker::{completions, hover, signature_help, warm_up};
