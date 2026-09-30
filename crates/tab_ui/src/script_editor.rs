mod completions;
mod signature;
mod view;

#[cfg(test)]
mod completion_tests;

pub(crate) use view::{ScriptEditor, ScriptTarget, ScriptsChanged};
