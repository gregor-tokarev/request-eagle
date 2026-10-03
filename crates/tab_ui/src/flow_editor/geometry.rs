//! Where the canvas draws blocks, ports and connections. Canvas positions
//! are pixels at 100% zoom with the default 16 px interface font, so the
//! canvas scales with the interface font like the rest of the app.

use flow::{BlockKind, BlockType};
use gpui_kit::{Bounds, Modifiers, Pixels, Point, ScrollDelta, Size, point, px, size};

/// The interface font size that canvas positions are measured at.
pub(super) const BASE_REM: f32 = 16.;
pub(super) const HEADER: f32 = 36.;
pub(super) const ROW: f32 = 24.;
/// The diameter of a port's dot.
pub(super) const PORT: f32 = 10.;
/// How close a pointer must be to a port or connection to grab it, in
/// screen pixels.
pub(super) const GRAB: f32 = 12.;
pub(super) const MIN_ZOOM: f32 = 0.25;
pub(super) const MAX_ZOOM: f32 = 2.;
/// How much a notch of a mouse wheel zooms, as a natural logarithm: about
/// 16%.
const WHEEL_ZOOM: f32 = 0.15;
/// The lines a mouse wheel scrolls for each notch by default.
pub(super) const NOTCH_LINES: f32 = if cfg!(target_os = "macos") { 1. } else { 3. };

pub(super) fn width(kind: &BlockKind) -> f32 {
    match kind.block_type() {
        BlockType::Start
        | BlockType::HttpRequest
        | BlockType::Evaluate
        | BlockType::If
        | BlockType::Condition
        | BlockType::Validate
        | BlockType::Record
        | BlockType::List
        | BlockType::Template
        | BlockType::Display
        | BlockType::Note => 288.,
        _ => 208.,
    }
}

/// The height below the ports: a block's summary, or a Display block's data.
pub(super) fn body_height(kind: &BlockKind) -> f32 {
    match kind.block_type() {
        BlockType::Display => 176.,
        BlockType::Note => 112.,
        BlockType::Evaluate
        | BlockType::If
        | BlockType::Condition
        | BlockType::Template
        | BlockType::Validate
        | BlockType::Record
        | BlockType::List => 56.,
        _ => 36.,
    }
}

/// A block's size with `rows` rows of ports.
pub(super) fn block_size(kind: &BlockKind, rows: usize) -> Size<f32> {
    size(width(kind), HEADER + ROW * rows as f32 + body_height(kind))
}

/// How far down a block the port of a row is.
pub(super) fn port_offset(row: usize) -> f32 {
    HEADER + ROW * row as f32 + ROW / 2.
}

/// The part of the canvas a view shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Viewport {
    /// The canvas position at the view's top-left corner.
    pub origin: Point<f32>,
    pub zoom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            origin: point(-48., -48.),
            zoom: 1.,
        }
    }
}

impl Viewport {
    /// Screen pixels for each canvas pixel.
    pub fn scale(&self, rem: Pixels) -> f32 {
        self.zoom * f32::from(rem) / BASE_REM
    }

    /// Where a canvas position is, relative to the view's top-left corner.
    pub fn to_view(self, position: Point<f32>, rem: Pixels) -> Point<Pixels> {
        let scale = self.scale(rem);
        point(
            px((position.x - self.origin.x) * scale),
            px((position.y - self.origin.y) * scale),
        )
    }

    /// The canvas position at a point relative to the view's top-left corner.
    pub fn to_canvas(self, position: Point<Pixels>, rem: Pixels) -> Point<f32> {
        let scale = self.scale(rem);
        point(
            self.origin.x + f32::from(position.x) / scale,
            self.origin.y + f32::from(position.y) / scale,
        )
    }

    /// Move the view by screen pixels.
    pub fn pan(&mut self, delta: Point<Pixels>, rem: Pixels) {
        let scale = self.scale(rem);
        self.origin.x -= f32::from(delta.x) / scale;
        self.origin.y -= f32::from(delta.y) / scale;
    }

    /// Zoom, keeping the canvas position under `anchor` where it is.
    pub fn zoom_around(&mut self, zoom: f32, anchor: Point<Pixels>, rem: Pixels) {
        let before = self.to_canvas(anchor, rem);
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.to_canvas(anchor, rem);
        self.origin.x += before.x - after.x;
        self.origin.y += before.y - after.y;
    }

    /// Follow a scroll over the view. A mouse wheel zooms around `anchor`
    /// and a trackpad pans; see [`from_wheel`]. Shift makes the wheel pan,
    /// and Ctrl/Cmd makes the trackpad zoom.
    pub fn scroll(
        &mut self,
        delta: ScrollDelta,
        wheel: bool,
        modifiers: Modifiers,
        anchor: Point<Pixels>,
        line_height: Pixels,
        rem: Pixels,
    ) {
        let pixels = delta.pixel_delta(line_height);
        // A wheel that only scrolls sideways, such as a tilt wheel, pans.
        let wheel = wheel && !modifiers.shift && pixels.y != px(0.);
        if !wheel && !modifiers.secondary() && !modifiers.control {
            self.pan(pixels, rem);
            return;
        }

        let factor = match delta {
            ScrollDelta::Lines(lines) => (lines.y / NOTCH_LINES * WHEEL_ZOOM).exp(),
            ScrollDelta::Pixels(pixels) => (f32::from(pixels.y) * 0.004).exp(),
        };
        self.zoom_around(self.zoom * factor, anchor, rem);
    }

    /// The view that shows `content` whole in a view of `view` pixels, at
    /// no more than 100%.
    pub fn fit(content: Bounds<f32>, view: Size<Pixels>, rem: Pixels) -> Self {
        let margin = 48.;
        let rem = f32::from(rem) / BASE_REM;
        let width = (content.size.width + margin * 2.) * rem;
        let height = (content.size.height + margin * 2.) * rem;
        let zoom = (f32::from(view.width) / width)
            .min(f32::from(view.height) / height)
            .clamp(MIN_ZOOM, 1.);
        let scale = zoom * rem;

        Self {
            origin: point(
                content.origin.x + content.size.width / 2. - f32::from(view.width) / scale / 2.,
                content.origin.y + content.size.height / 2. - f32::from(view.height) / scale / 2.,
            ),
            zoom,
        }
    }

    /// The canvas area a view of `view` pixels shows.
    pub fn visible(&self, view: Size<Pixels>, rem: Pixels) -> Bounds<f32> {
        let scale = self.scale(rem);
        Bounds {
            origin: self.origin,
            size: size(
                f32::from(view.width) / scale,
                f32::from(view.height) / scale,
            ),
        }
    }
}

/// Whether a scroll comes from a mouse wheel rather than a trackpad. Wheels
/// scroll by lines and trackpads by pixels, except on X11, where trackpads
/// scroll by lines too. There, only whole notches are a wheel; fractions of
/// a notch, from a trackpad or a high-resolution wheel, are not.
pub(super) fn from_wheel(delta: ScrollDelta, x11: bool) -> bool {
    match delta {
        ScrollDelta::Pixels(_) => false,
        ScrollDelta::Lines(lines) if x11 => {
            let notches = lines.y / NOTCH_LINES;
            notches.round() != 0. && (notches - notches.round()).abs() < 0.01
        }
        ScrollDelta::Lines(_) => true,
    }
}

/// The control points of a connection's curve from an output to an input.
pub(super) fn wire(from: Point<f32>, to: Point<f32>) -> [Point<f32>; 4] {
    let reach = ((to.x - from.x).abs() / 2.).clamp(48., 240.);
    [
        from,
        point(from.x + reach, from.y),
        point(to.x - reach, to.y),
        to,
    ]
}

/// The points along a connection's curve, for drawing and hit testing.
pub(super) fn wire_points(curve: &[Point<f32>; 4], segments: usize) -> Vec<Point<f32>> {
    (0..=segments)
        .map(|step| {
            let t = step as f32 / segments as f32;
            let u = 1. - t;
            let [a, b, c, d] = curve;
            point(
                u * u * u * a.x + 3. * u * u * t * b.x + 3. * u * t * t * c.x + t * t * t * d.x,
                u * u * u * a.y + 3. * u * u * t * b.y + 3. * u * t * t * c.y + t * t * t * d.y,
            )
        })
        .collect()
}

/// How far a position is from a connection's curve.
pub(super) fn distance_to_wire(position: Point<f32>, curve: &[Point<f32>; 4]) -> f32 {
    wire_points(curve, 32)
        .windows(2)
        .map(|segment| distance_to_segment(position, segment[0], segment[1]))
        .fold(f32::INFINITY, f32::min)
}

fn distance_to_segment(position: Point<f32>, start: Point<f32>, end: Point<f32>) -> f32 {
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let length = dx * dx + dy * dy;
    let t = if length == 0. {
        0.
    } else {
        (((position.x - start.x) * dx + (position.y - start.y) * dy) / length).clamp(0., 1.)
    };
    let (x, y) = (start.x + t * dx, start.y + t * dy);

    ((position.x - x).powi(2) + (position.y - y).powi(2)).sqrt()
}

/// The rectangle between two corners, in either order.
pub(super) fn rectangle(a: Point<f32>, b: Point<f32>) -> Bounds<f32> {
    Bounds {
        origin: point(a.x.min(b.x), a.y.min(b.y)),
        size: size((a.x - b.x).abs(), (a.y - b.y).abs()),
    }
}

pub(super) fn intersects(a: &Bounds<f32>, b: &Bounds<f32>) -> bool {
    a.origin.x < b.origin.x + b.size.width
        && b.origin.x < a.origin.x + a.size.width
        && a.origin.y < b.origin.y + b.size.height
        && b.origin.y < a.origin.y + a.size.height
}

/// The smallest rectangle around all of `bounds`.
pub(super) fn union(bounds: impl IntoIterator<Item = Bounds<f32>>) -> Option<Bounds<f32>> {
    bounds.into_iter().reduce(|a, b| {
        let left = a.origin.x.min(b.origin.x);
        let top = a.origin.y.min(b.origin.y);
        let right = (a.origin.x + a.size.width).max(b.origin.x + b.size.width);
        let bottom = (a.origin.y + a.size.height).max(b.origin.y + b.size.height);
        Bounds {
            origin: point(left, top),
            size: size(right - left, bottom - top),
        }
    })
}
