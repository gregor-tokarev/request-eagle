use std::time::{Duration, Instant};

use gpui_kit::{
    AppContext as _, InputEvent as _, Modifiers, MouseButton, MouseMoveEvent, ScrollDelta,
    ScrollWheelEvent, TestAppContext, TouchPhase, component::Root, point, px, size,
};
use request::{Execution, HeaderMap, HttpMetrics, HttpResponse, Response, StatusCode, Version};

use crate::workspace::Layout;
use tab_ui::RequestDraft;
use tab_ui::test_support::ResponseContent;

// Run with --release, serially and without other CPU-heavy jobs. Includes Root,
// event dispatch, effects, drawing and cleanup; excludes GPU presentation.
#[gpui_kit::test]
#[ignore = "manual 120 fps response interaction budget"]
#[allow(clippy::assertions_on_constants)] // Reject accidental debug-mode measurements.
fn response_interaction_benchmark(cx: &mut TestAppContext) {
    assert!(!cfg!(debug_assertions), "run this benchmark with --release");
    let sample_count = std::env::var("REQUEST_EAGLE_BENCH_SAMPLES")
        .map(|value| value.parse::<usize>().unwrap())
        .unwrap_or(120);
    assert!(sample_count > 0);
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
        cx.set_reduce_motion(true);
    });
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    for index in 0..128 {
        headers.insert(
            format!("x-benchmark-{index}")
                .parse::<request::HeaderName>()
                .unwrap(),
            "A response header value with enough text to exercise selection"
                .parse()
                .unwrap(),
        );
    }
    for index in 0..32 {
        headers.append(
            "set-cookie",
            format!("session{index}=value{index}; Path=/; HttpOnly; SameSite=Lax")
                .parse()
                .unwrap(),
        );
    }
    let body = serde_json::to_vec(
        &(0..8_000)
            .map(|index| serde_json::json!({"id":index,"name":"Response benchmark","active":true}))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let body_bytes = body.len();
    let header_count = headers.len();
    let collections = crate::performance::collections(1_000);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let layout = cx.new(|cx| Layout::new(collections, updater::init("1.2.3", cx), window, cx));
        let tabs = layout.read(cx).main_view.clone();
        let draft = tabs.read(cx).tabs[0]
            .page
            .view()
            .downcast::<RequestDraft>()
            .ok()
            .unwrap();
        draft.update(cx, |draft, cx| draft.prepare(window, cx));
        let response = draft.read(cx).response_for_test();
        response.update(cx, |view, cx| {
            view.finish(
                Ok(ResponseContent::new(Execution {
                    scripts: Vec::new(),
                    elapsed: Duration::from_millis(250),
                    response: Response::Http(HttpResponse {
                        status: StatusCode::OK,
                        version: Version::HTTP_2,
                        headers,
                        body,
                        metrics: HttpMetrics {
                            waiting: Duration::from_millis(220),
                            download: Duration::from_millis(25),
                            ..HttpMetrics::default()
                        },
                    }),
                })),
                window,
                cx,
            );
        });
        Root::new(layout, window, cx)
    });
    let mut failures = Vec::new();
    for (width, height) in [(1024., 768.), (1440., 900.), (3440., 1410.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        for scenario in [
            "body scroll",
            "headers scroll",
            "cookies select",
            "timing select",
            "size select",
        ] {
            cx.simulate_mouse_move(point(px(0.), px(0.)), None, Modifiers::default());
            cx.executor().advance_clock(Duration::from_millis(400));
            cx.run_until_parked();
            let tab = cx
                .debug_bounds(match scenario {
                    "headers scroll" => "response-section-Headers",
                    "cookies select" => "response-section-Cookies",
                    _ => "response-section-Body",
                })
                .unwrap();
            cx.simulate_click(tab.center(), Modifiers::default());

            let selecting = scenario.ends_with("select");
            let (start, end) = if scenario == "timing select" || scenario == "size select" {
                let trigger = cx
                    .debug_bounds(if scenario == "timing select" {
                        "response-time"
                    } else {
                        "response-size"
                    })
                    .unwrap();
                cx.simulate_mouse_move(trigger.center(), None, Modifiers::default());
                cx.executor().advance_clock(Duration::from_millis(400));
                cx.run_until_parked();
                let first = cx
                    .debug_bounds(if scenario == "timing select" {
                        "detail-time-title"
                    } else {
                        "detail-response-total"
                    })
                    .unwrap();
                let last = cx
                    .debug_bounds(if scenario == "timing select" {
                        "timing-phase-2"
                    } else {
                        "detail-request-body"
                    })
                    .unwrap();
                (
                    point(first.left() + px(1.), first.center().y),
                    point(last.right() - px(1.), last.center().y),
                )
            } else if selecting {
                let first = cx.debug_bounds("response-header-name-0").unwrap();
                let last = cx.debug_bounds("response-header-value-3").unwrap();
                (
                    point(first.left() + px(8.), first.center().y),
                    point(last.left() + px(200.), last.center().y),
                )
            } else {
                let bounds = cx
                    .debug_bounds(if scenario == "body scroll" {
                        "response-body"
                    } else {
                        "response-header-table"
                    })
                    .unwrap();
                (bounds.center(), bounds.center())
            };
            if selecting {
                cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
            }

            let mut samples = Vec::with_capacity(sample_count);
            for index in 0..sample_count + 20 {
                let started = Instant::now();
                if selecting {
                    cx.update(|window, cx| {
                        window.dispatch_event(
                            MouseMoveEvent {
                                position: point(end.x - px((index % 2) as f32 * 10.), end.y),
                                pressed_button: Some(MouseButton::Left),
                                modifiers: Modifiers::default(),
                            }
                            .to_platform_input(),
                            cx,
                        )
                    });
                } else {
                    cx.update(|window, cx| {
                        window.dispatch_event(
                            ScrollWheelEvent {
                                position: start,
                                delta: ScrollDelta::Pixels(point(
                                    px(0.),
                                    px(if index % 40 < 20 { -24. } else { 24. }),
                                )),
                                modifiers: Modifiers::default(),
                                touch_phase: TouchPhase::Moved,
                            }
                            .to_platform_input(),
                            cx,
                        );
                    });
                }
                if index >= 20 {
                    samples.push(started.elapsed().as_secs_f64() * 1000.);
                }
            }
            if selecting {
                cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
                cx.update(|window, cx| {
                    assert!(
                        !gpui_kit::base::TextSelection::selected_text(window, cx).is_empty(),
                        "{scenario} must select text"
                    )
                });
            }
            samples.sort_by(f64::total_cmp);
            let mean = samples.iter().sum::<f64>() / sample_count as f64;
            let p95 = samples[(sample_count * 95).div_ceil(100) - 1];
            let p99 = samples[(sample_count * 99).div_ceil(100) - 1];
            let over = samples.iter().filter(|&&ms| ms > 1000. / 120.).count();
            eprintln!(
                "Workspace 1000 requests; response {body_bytes} B, {header_count} headers, {width}x{height} {scenario}: mean {mean:.2} ms, p95 {p95:.2}, p99 {p99:.2}, max {:.2}; over 8.33 ms: {over}/{sample_count}",
                samples[sample_count - 1]
            );
            if p99 > 1000. / 120. {
                failures.push(format!("{width}x{height} {scenario}: p99 {p99:.2} ms"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "120 fps CPU budget exceeded:\n{}",
        failures.join("\n")
    );
}
