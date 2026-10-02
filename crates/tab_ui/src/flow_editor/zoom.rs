use gpui_kit::*;

/// Lays out and paints its child with another interface font size, which
/// scales everything the child measures in rems. The canvas zooms its blocks
/// this way, so their text and controls stay sharp at every zoom.
pub(super) struct Zoom {
    rem_size: Pixels,
    child: AnyElement,
}

impl Zoom {
    pub fn new(rem_size: Pixels, child: impl IntoElement) -> Self {
        Self {
            rem_size,
            child: child.into_any_element(),
        }
    }
}

impl IntoElement for Zoom {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Zoom {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout = window.with_rem_size(Some(self.rem_size), |window| {
            self.child.request_layout(window, cx)
        });

        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_rem_size(Some(self.rem_size), |window| {
            self.child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_rem_size(Some(self.rem_size), |window| {
            self.child.paint(window, cx);
        });
    }
}
