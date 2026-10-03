use flow::BlockType;
use gpui_kit::{Bounds, Modifiers, ScrollDelta, point, px, size};

use super::geometry::*;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn converts_between_canvas_and_view_at_every_zoom_and_font_size() {
    for rem in [px(12.), px(16.), px(24.)] {
        for zoom in [MIN_ZOOM, 0.5, 1., 1.75] {
            let viewport = Viewport {
                origin: point(-120., 40.),
                zoom,
            };
            let position = point(310.5, -72.25);

            let back = viewport.to_canvas(viewport.to_view(position, rem), rem);
            assert!(close(back.x, position.x) && close(back.y, position.y));
            assert!(close(viewport.scale(rem), zoom * f32::from(rem) / BASE_REM));
        }
    }
}

#[test]
fn zooming_keeps_the_point_under_the_pointer() {
    let rem = px(16.);
    let mut viewport = Viewport::default();
    let anchor = point(px(300.), px(200.));
    let under = viewport.to_canvas(anchor, rem);

    viewport.zoom_around(1.6, anchor, rem);
    let after = viewport.to_canvas(anchor, rem);
    assert!(close(under.x, after.x) && close(under.y, after.y));

    viewport.zoom_around(100., anchor, rem);
    assert_eq!(viewport.zoom, MAX_ZOOM);
    viewport.zoom_around(0., anchor, rem);
    assert_eq!(viewport.zoom, MIN_ZOOM);
}

#[test]
fn panning_moves_by_screen_pixels() {
    let rem = px(24.);
    let mut viewport = Viewport {
        origin: point(0., 0.),
        zoom: 2.,
    };

    viewport.pan(point(px(30.), px(-60.)), rem);

    // 2× zoom at a 24 px font is 3 screen pixels per canvas pixel.
    assert!(close(viewport.origin.x, -10.) && close(viewport.origin.y, 20.));
}

#[test]
fn a_mouse_wheel_zooms_around_the_pointer() {
    let rem = px(16.);
    let line = px(20.);
    let anchor = point(px(300.), px(200.));
    let notch = |viewport: &mut Viewport, notches: f32| {
        viewport.scroll(
            ScrollDelta::Lines(point(0., notches * NOTCH_LINES)),
            true,
            Modifiers::none(),
            anchor,
            line,
            rem,
        )
    };
    let mut viewport = Viewport::default();
    let under = viewport.to_canvas(anchor, rem);

    notch(&mut viewport, 1.);
    let step = viewport.zoom;
    assert!(step > 1.1 && step < 1.25);
    let after = viewport.to_canvas(anchor, rem);
    assert!(close(under.x, after.x) && close(under.y, after.y));

    // The same rotation zooms as far however the system splits it.
    let mut halves = Viewport::default();
    notch(&mut halves, 0.5);
    notch(&mut halves, 0.5);
    assert!(close(halves.zoom, step));
    let mut double = Viewport::default();
    notch(&mut double, 2.);
    assert!(close(double.zoom, step * step));

    notch(&mut viewport, -1.);
    assert!(close(viewport.zoom, 1.));
}

#[test]
fn a_mouse_wheel_pans_with_shift_or_sideways() {
    let rem = px(16.);
    let line = px(20.);
    let anchor = point(px(300.), px(200.));

    for (delta, modifiers) in [
        (point(1., 0.), Modifiers::shift()),
        (point(0., 1.), Modifiers::shift()),
        (point(1., 0.), Modifiers::none()),
    ] {
        let mut viewport = Viewport::default();
        viewport.scroll(
            ScrollDelta::Lines(delta),
            true,
            modifiers,
            anchor,
            line,
            rem,
        );

        assert_eq!(viewport.zoom, 1.);
        assert_ne!(viewport.origin, Viewport::default().origin);
    }
}

#[test]
fn a_trackpad_pans_and_zooms_with_ctrl_or_cmd() {
    let rem = px(16.);
    let line = px(20.);
    let anchor = point(px(300.), px(200.));
    let swipe = ScrollDelta::Pixels(point(px(30.), px(-60.)));

    let mut viewport = Viewport::default();
    viewport.scroll(swipe, false, Modifiers::none(), anchor, line, rem);
    assert_eq!(viewport.zoom, 1.);
    let origin = Viewport::default().origin;
    assert!(close(viewport.origin.x, origin.x - 30.) && close(viewport.origin.y, origin.y + 60.));

    // X11 trackpads scroll by lines.
    let lines = ScrollDelta::Lines(point(0., -0.4));
    let mut viewport = Viewport::default();
    viewport.scroll(lines, false, Modifiers::none(), anchor, line, rem);
    assert_eq!(viewport.zoom, 1.);
    assert!(close(viewport.origin.y, origin.y + 0.4 * 20.));

    for delta in [swipe, lines] {
        for modifiers in [Modifiers::control(), Modifiers::secondary_key()] {
            let mut viewport = Viewport::default();
            viewport.scroll(delta, false, modifiers, anchor, line, rem);
            assert!(viewport.zoom < 1.);
        }
    }
}

#[test]
fn wheels_scroll_by_lines_and_trackpads_by_pixels_or_fractions_on_x11() {
    let lines = |y: f32| ScrollDelta::Lines(point(0., y));
    let pixels = ScrollDelta::Pixels(point(px(0.), px(12.)));

    assert!(!from_wheel(pixels, false));
    assert!(!from_wheel(pixels, true));
    assert!(from_wheel(lines(0.4), false));
    assert!(from_wheel(lines(NOTCH_LINES), true));
    assert!(from_wheel(lines(-2. * NOTCH_LINES), true));
    assert!(!from_wheel(lines(0.4 * NOTCH_LINES), true));
    assert!(!from_wheel(lines(0.), true));
}

#[test]
fn fitting_centers_the_content_without_magnifying_it() {
    let rem = px(16.);
    let content = Bounds {
        origin: point(100., 100.),
        size: size(400., 200.),
    };
    let view = size(px(1000.), px(800.));

    let viewport = Viewport::fit(content, view, rem);
    assert_eq!(viewport.zoom, 1.);
    let center = viewport.to_canvas(point(px(500.), px(400.)), rem);
    assert!(close(center.x, 300.) && close(center.y, 200.));

    let large = Bounds {
        origin: point(0., 0.),
        size: size(4000., 1000.),
    };
    let viewport = Viewport::fit(large, view, rem);
    assert!(viewport.zoom < 0.3);
    let visible = viewport.visible(view, rem);
    assert!(visible.origin.x <= 0. && visible.origin.x + visible.size.width >= 4000.);
}

#[test]
fn blocks_grow_with_their_ports_and_place_ports_on_rows() {
    let kind = BlockType::Evaluate.block_kind();
    let one = block_size(&kind, 1);
    let three = block_size(&kind, 3);

    assert_eq!(three.height - one.height, ROW * 2.);
    assert_eq!(one.width, three.width);
    assert_eq!(port_offset(0), HEADER + ROW / 2.);
    assert_eq!(port_offset(2) - port_offset(1), ROW);
}

#[test]
fn connections_are_hit_near_their_curve() {
    let curve = wire(point(0., 0.), point(200., 100.));

    assert!(distance_to_wire(point(0., 0.), &curve) < 0.01);
    assert!(distance_to_wire(point(200., 100.), &curve) < 0.01);
    assert!(distance_to_wire(point(100., 50.), &curve) < 2.);
    assert!(distance_to_wire(point(100., -40.), &curve) > 30.);
    // A connection back to an earlier block still curves out of its output.
    let back = wire(point(300., 0.), point(0., 0.));
    assert!(back[1].x > 300. && back[2].x < 0.);
}

#[test]
fn rectangles_intersect_and_combine() {
    let a = rectangle(point(10., 10.), point(0., 0.));
    assert_eq!(a.size, size(10., 10.));
    let b = Bounds {
        origin: point(5., 5.),
        size: size(10., 10.),
    };
    let c = Bounds {
        origin: point(20., 20.),
        size: size(1., 1.),
    };

    assert!(intersects(&a, &b));
    assert!(!intersects(&a, &c));
    let all = union([a, b, c]).unwrap();
    assert_eq!(all.origin, point(0., 0.));
    assert_eq!(all.size, size(21., 21.));
    assert!(union([]).is_none());
}
