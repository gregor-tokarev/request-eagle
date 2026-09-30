use crate::tests::{collections, init, no_environments, workspace};
use crate::workspace::Workspace;
use collection::CollectionRegistry;
use gpui_kit::{
    InputEvent as _, Modifiers, MouseButton, MouseMoveEvent, ScrollDelta, ScrollWheelEvent,
    TestAppContext, TouchPhase, point, px, size,
};
use request::{Execution, HeaderMap, HttpMetrics, HttpResponse, Response, StatusCode, Version};
use settings_ui::SettingsPage;
use std::time::{Duration, Instant};
use tab_ui::test_support::ResponseContent;

/// The frame time of a 120 Hz display.
const FRAME_BUDGET_MS: f64 = 1000. / 120.;

/// Samples per measurement, set by REQUEST_EAGLE_BENCH_SAMPLES.
fn sample_count() -> usize {
    let count = std::env::var("REQUEST_EAGLE_BENCH_SAMPLES")
        .map(|value| value.parse::<usize>().expect("positive sample count"))
        .unwrap_or(120);
    assert!(count > 0);

    count
}

/// Reject debug builds when a budgeted benchmark runs, not when compiling
/// ordinary tests.
fn require_release_build() {
    #[expect(
        clippy::assertions_on_constants,
        reason = "The assertion only fails in debug builds."
    )]
    {
        assert!(!cfg!(debug_assertions), "run this benchmark with --release");
    }
}

/// Print frame times in milliseconds, and return their 99th percentile.
fn report(label: &str, mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    let count = samples.len();
    let percentile = |percent: usize| samples[(count * percent).div_ceil(100) - 1];
    let mean = samples.iter().sum::<f64>() / count as f64;
    let over_budget = samples.iter().filter(|&&ms| ms > FRAME_BUDGET_MS).count();

    eprintln!(
        "{label}: mean {mean:.2} ms, p95 {:.2}, p99 {:.2}, max {:.2}; over 8.33 ms: {over_budget}/{count}",
        percentile(95),
        percentile(99),
        samples[count - 1],
    );

    percentile(99)
}

// Run serially, without other benchmarks competing for CPU:
// cargo test -p workspace pages_render_benchmark -- --ignored --nocapture --test-threads=1
// Set REQUEST_EAGLE_BENCH_TRANSITIONS=1 to switch away and back before each draw.
// Like appearance_render_benchmark, this measures forced CPU draws, including
// element cleanup, but excludes GPU presentation, Root and the application's FPS monitor.
#[gpui_kit::test]
#[ignore = "manual full-layout page render benchmark"]
fn pages_render_benchmark(cx: &mut TestAppContext) {
    let page_filter = std::env::var("REQUEST_EAGLE_BENCH_PAGE").ok();
    let switch_pages = std::env::var_os("REQUEST_EAGLE_BENCH_TRANSITIONS").is_some();
    let sample_count = sample_count();

    init(cx);
    cx.update(|cx| {
        eprintln!(
            "Keybindings: {} registered commands",
            keybindings_service::commands(cx).len()
        );
    });

    for (label, request_count, page) in [
        ("Workspace empty", 0, None),
        ("Workspace 100 requests", 100, None),
        ("Workspace 1000 requests", 1000, None),
        ("Workspace 10000 requests", 10000, None),
        ("Workspace 100000 requests", 100000, None),
        ("General", 0, Some(SettingsPage::General)),
        ("Appearance", 0, Some(SettingsPage::Appearance)),
        ("Keybindings", 0, Some(SettingsPage::Keybindings)),
    ] {
        if page_filter.as_ref().is_some_and(|filter| filter != label) {
            continue;
        }

        let collections = collections(request_count);
        let (layout, cx) = cx.add_window_view(|window, cx| {
            Workspace::new(
                collections,
                no_environments(),
                updater::init("1.2.3", cx),
                window,
                cx,
            )
        });

        if let Some(page) = page {
            cx.update(|window, cx| {
                layout.update(cx, |layout, cx| {
                    layout.open_settings(window, cx);
                    layout.settings.update(cx, |settings, cx| {
                        settings.select_page(page, window, cx);
                    });
                });
            });
        }

        for (width, height) in [(1024., 768.), (1440., 900.), (3440., 1410.)] {
            cx.simulate_resize(size(px(width), px(height)));
            cx.run_until_parked();
            assert!(
                cx.debug_bounds(if page.is_some() {
                    "settings"
                } else {
                    "main-view"
                })
                .is_some()
            );

            for scrolling in [false, true] {
                // Only populated collections and the Appearance catalog overflow.
                if scrolling && request_count == 0 && page != Some(SettingsPage::Appearance) {
                    continue;
                }

                let mut samples = Vec::with_capacity(sample_count);

                for index in 0..sample_count + 20 {
                    let duration = cx.update(|window, cx| {
                        if switch_pages && let Some(page) = page {
                            layout.update(cx, |layout, cx| {
                                layout.settings.update(cx, |settings, cx| {
                                    settings.select_page(SettingsPage::General, window, cx);
                                });
                            });
                            window.refresh();
                            window.draw(cx).clear(cx);
                            layout.update(cx, |layout, cx| {
                                layout.settings.update(cx, |settings, cx| {
                                    settings.select_page(page, window, cx);
                                });
                            });
                        }

                        if scrolling {
                            window.dispatch_event(
                                ScrollWheelEvent {
                                    position: point(
                                        px(if page.is_some() { width - 100. } else { 100. }),
                                        px(height / 2.),
                                    ),
                                    delta: ScrollDelta::Pixels(point(
                                        px(0.),
                                        px(if index % 80 < 40 { -24. } else { 24. }),
                                    )),
                                    modifiers: Modifiers::default(),
                                    touch_phase: TouchPhase::Moved,
                                }
                                .to_platform_input(),
                                cx,
                            );
                        }

                        window.refresh();
                        let started = Instant::now();
                        window.draw(cx).clear(cx);
                        started.elapsed()
                    });

                    if index >= 20 {
                        samples.push(duration.as_secs_f64() * 1000.);
                    }
                }

                let interaction = if scrolling {
                    "scroll"
                } else if switch_pages && page.is_some() {
                    "page switch"
                } else {
                    "draw"
                };
                report(&format!("{label} {width}x{height} {interaction}"), samples);
            }
        }
    }
}

// Includes event dispatch, effects, Root, drawing and element cleanup. GPU
// presentation and the application's FPS monitor still require checking the native app.
// Run without CPU contention.
#[gpui_kit::test]
#[ignore = "manual 120 fps tab interaction budget"]
fn tabs_interaction_benchmark(cx: &mut TestAppContext) {
    require_release_build();
    let sample_count = sample_count();
    let mut failures = Vec::new();

    init(cx);

    for tab_count in [100, 1_000, 10_000] {
        let (layout, cx) = workspace(CollectionRegistry::new(), no_environments(), cx);

        cx.update(|_, cx| {
            layout.read(cx).main_view.clone().update(cx, |view, cx| {
                for _ in 1..tab_count {
                    view.new_tab(cx);
                }
            });
        });

        for (width, height) in [(1024., 768.), (1440., 900.), (3440., 1410.)] {
            cx.simulate_resize(size(px(width), px(height)));
            cx.run_until_parked();

            for interaction in ["switch", "scroll", "create"] {
                // Exercise unseen pages, scrolling, and keyboard wraparound.
                cx.simulate_keystrokes("secondary-9");
                let bar = cx.debug_bounds("main-tab-bar").unwrap();
                let shortcut = gpui_kit::Keystroke::parse(if interaction == "create" {
                    "secondary-t"
                } else {
                    "secondary-}"
                })
                .unwrap();
                let mut samples = Vec::with_capacity(sample_count);

                for index in 0..sample_count + 20 {
                    let started = Instant::now();
                    cx.update(|window, cx| {
                        if interaction == "scroll" {
                            window.dispatch_event(
                                ScrollWheelEvent {
                                    position: bar.center(),
                                    delta: ScrollDelta::Pixels(point(
                                        px(if index % 40 < 20 { 80. } else { -80. }),
                                        px(0.),
                                    )),
                                    modifiers: Modifiers::default(),
                                    touch_phase: TouchPhase::Moved,
                                }
                                .to_platform_input(),
                                cx,
                            );
                        } else {
                            assert!(window.dispatch_keystroke(shortcut.clone(), cx));
                        }
                    });
                    // TestAppContext::update flushes effects and draws dirty
                    // windows, including element cleanup. Do not force another
                    // draw here or time the same interaction twice.

                    if index >= 20 {
                        samples.push(started.elapsed().as_secs_f64() * 1000.);
                    }
                }

                let label = format!("{tab_count} tabs {width}x{height} {interaction}");
                let p99 = report(&label, samples);
                if p99 > FRAME_BUDGET_MS {
                    failures.push(format!("{label}: p99 {p99:.2} ms"));
                }

                if interaction == "create" {
                    cx.update(|_, cx| {
                        layout.read(cx).main_view.clone().update(cx, |view, cx| {
                            for _ in 0..sample_count + 20 {
                                view.close_active_tab(cx);
                            }
                        });
                    });
                }
            }
        }

        cx.update(|window, _| window.remove_window());
    }

    assert!(
        failures.is_empty(),
        "120 fps CPU budget exceeded:\n{}",
        failures.join("\n")
    );
}

// Opening, typing in and navigating the command palette, with collections
// of every size. Measured like tabs_interaction_benchmark; run with --release.
// Each sample is one frame: "open" draws the dialog and its search field,
// "reveal" draws the rows on the next frame, and "results" applies background
// request results. The test executor runs that search on this thread, so
// "results" includes it.
#[gpui_kit::test]
#[ignore = "manual 120 fps command palette budget"]
fn palette_interaction_benchmark(cx: &mut TestAppContext) {
    require_release_build();
    let sample_count = sample_count();
    let mut failures = Vec::new();

    init(cx);

    // "g" matches every request; the rest narrows to a single one.
    let query = [
        "g", "e", "t", "space", "r", "e", "s", "o", "u", "r", "c", "e", "space", "9",
    ];

    for request_count in [1_000, 10_000, 100_000] {
        let (layout, cx) = workspace(collections(request_count), no_environments(), cx);
        cx.update(|window, cx| {
            window.activate_window();
            crate::workspace::on_toggle_command_palette(&layout, window.window_handle(), cx);
        });

        for (width, height) in [(1440., 900.), (3440., 1410.)] {
            cx.simulate_resize(size(px(width), px(height)));
            cx.run_until_parked();

            let mut samples: Vec<(&str, Vec<f64>)> =
                ["open", "reveal", "close", "type", "results", "navigate"]
                    .into_iter()
                    .map(|interaction| (interaction, Vec::new()))
                    .collect();
            let mut record = |interaction: &str, index: usize, started: Instant| {
                if index >= 5 {
                    let ms = started.elapsed().as_secs_f64() * 1000.;
                    samples
                        .iter_mut()
                        .find(|(name, _)| *name == interaction)
                        .unwrap()
                        .1
                        .push(ms);
                }
            };
            let keystroke = |keys: &str| gpui_kit::Keystroke::parse(keys).unwrap();

            // Each round opens, types the query, navigates and closes; the first
            // five warm up.
            let rounds = sample_count + 5;
            for index in 0..rounds {
                // The toggle is a global action, deferred to the next effect
                // cycle; run_until_parked includes that and the palette's draw.
                let started = Instant::now();
                cx.update(|window, cx| {
                    assert!(window.dispatch_keystroke(keystroke("secondary-k"), cx));
                });
                record("open", index, started);
                assert!(cx.read(|cx| {
                    let palette = layout.read(cx).command_palette.as_ref();
                    palette.and_then(|palette| palette.upgrade()).is_some()
                }));

                cx.run_until_parked();
                let started = Instant::now();
                cx.update(|window, cx| {
                    assert!(window.simulate_next_frame(cx) >= 1);
                });
                record("reveal", index, started);
                cx.run_until_parked();

                for key in query {
                    let started = Instant::now();
                    cx.update(|window, cx| {
                        window.dispatch_keystroke(keystroke(key), cx);
                    });
                    record("type", index, started);

                    let started = Instant::now();
                    cx.run_until_parked();
                    record("results", index, started);
                }

                for key in ["down", "down", "up"] {
                    let started = Instant::now();
                    cx.update(|window, cx| {
                        window.dispatch_keystroke(keystroke(key), cx);
                    });
                    record("navigate", index, started);
                }

                let started = Instant::now();
                cx.update(|window, cx| {
                    assert!(window.dispatch_keystroke(keystroke("secondary-k"), cx));
                });
                cx.run_until_parked();
                record("close", index, started);
                assert!(cx.read(|cx| {
                    layout
                        .read(cx)
                        .command_palette
                        .as_ref()
                        .unwrap()
                        .upgrade()
                        .is_none()
                }));
            }

            for (interaction, samples) in samples {
                let label = format!("{request_count} requests {width}x{height} {interaction}");
                let p99 = report(&label, samples);
                if p99 > FRAME_BUDGET_MS {
                    failures.push(format!("{label}: p99 {p99:.2} ms"));
                }
            }
        }

        cx.update(|window, _| window.remove_window());
    }

    assert!(
        failures.is_empty(),
        "120 fps CPU budget exceeded:\n{}",
        failures.join("\n")
    );
}

// Run with --release, serially and without other CPU-heavy jobs. Includes Root,
// event dispatch, effects, drawing and cleanup; excludes GPU presentation.
#[gpui_kit::test]
#[ignore = "manual 120 fps response interaction budget"]
fn response_interaction_benchmark(cx: &mut TestAppContext) {
    require_release_build();
    let sample_count = sample_count();
    init(cx);
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
    let (layout, cx) = workspace(collections(1_000), no_environments(), cx);
    cx.update(|window, cx| {
        let draft = layout.read(cx).main_view.read(cx).tabs[0].draft();
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
            let p99 = report(
                &format!(
                    "Workspace 1000 requests; response {body_bytes} B, {header_count} headers, {width}x{height} {scenario}"
                ),
                samples,
            );
            if p99 > FRAME_BUDGET_MS {
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
