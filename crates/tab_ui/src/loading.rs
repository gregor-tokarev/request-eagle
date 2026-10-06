use std::time::Duration;

use gpui_kit::component::Icon;
use gpui_kit::*;

/// Each frame of an animation redraws the window, and a request can wait for
/// its response for as long as the server takes. A smoothly turning icon kept
/// the app busy for all that time, so loading icons turn in coarse steps.
const FRAMES_PER_SECOND: f32 = 12.;

/// Turns `icon` once a second, to show that something is loading. The icon
/// should not look the same after a step, as `IconName::Loader` does.
pub(crate) fn spinning(icon: Icon, id: impl Into<ElementId>) -> impl IntoElement {
    icon.with_animation(
        id,
        Animation::new(Duration::from_secs(1))
            .repeat()
            .with_max_fps(FRAMES_PER_SECOND),
        |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
    )
}
