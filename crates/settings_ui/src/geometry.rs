use gpui_kit::{Rems, Window, rems};

// Shared by the ordinary pages and the virtualized appearance/keybinding pages.
pub(crate) const PAGE_WIDTH: Rems = rems(55.);
pub(crate) const THEME_CARD_WIDTH: Rems = rems(12.);
pub(crate) const SIDEBAR_MIN: Rems = rems(14.);
pub(crate) const CONTENT_MIN: Rems = rems(28.);

pub(crate) fn is_narrow(window: &Window) -> bool {
    window.viewport_size().width < rems(42.5).to_pixels(window.rem_size())
}

pub(crate) fn page_inset(window: &Window) -> Rems {
    if is_narrow(window) {
        rems(1.)
    } else {
        rems(2.)
    }
}
