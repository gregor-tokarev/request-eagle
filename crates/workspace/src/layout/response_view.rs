mod body;
mod content;
mod headers;
mod metadata;
mod timing;
mod view;

#[cfg(test)]
mod performance;
#[cfg(test)]
mod tests;

pub(super) use content::ResponseContent;
pub(super) use view::ResponseView;
