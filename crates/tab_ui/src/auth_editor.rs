mod editor;
mod fields;
mod token;
mod view;

#[cfg(test)]
mod tests;

pub(crate) use editor::{AuthChanged, AuthEditor, AuthTarget, Inherited};
