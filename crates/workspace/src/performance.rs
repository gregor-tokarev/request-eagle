use crate::workspace::Layout;
use collection::CollectionRegistry;
use gpui_kit::{
    AppContext, InputEvent as _, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase, component::Root, point, px, size,
};
use settings_ui::SettingsPage;
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

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
    let sample_count = std::env::var("REQUEST_EAGLE_BENCH_SAMPLES")
        .map(|value| value.parse::<usize>().expect("positive sample count"))
        .unwrap_or(120);
    assert!(sample_count > 0);

    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
        cx.set_reduce_motion(true);
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
            Layout::new(collections, updater::init("1.2.3", cx), window, cx)
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

                samples.sort_by(f64::total_cmp);
                let mean = samples.iter().sum::<f64>() / samples.len() as f64;
                let over_budget = samples.iter().filter(|&&ms| ms > 1000. / 120.).count();
                eprintln!(
                    "{label} {width}x{height} {}: mean {mean:.2} ms, p95 {:.2}, p99 {:.2}, max {:.2}; over 8.33 ms: {over_budget}/{sample_count}",
                    if scrolling {
                        "scroll"
                    } else if switch_pages && page.is_some() {
                        "page switch"
                    } else {
                        "draw"
                    },
                    samples[(sample_count * 95).div_ceil(100) - 1],
                    samples[(sample_count * 99).div_ceil(100) - 1],
                    samples[sample_count - 1],
                );
            }
        }
    }
}

pub(crate) fn collections(request_count: usize) -> CollectionRegistry {
    if request_count == 0 {
        return CollectionRegistry::new();
    }

    // Load synthetic requests through the real parser, outside the timed region.
    // Never load or modify the user's collections or preferences. Tests run in
    // parallel, so each call gets its own directory.
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "request-eagle-page-benchmark-{}-{request_count}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    for index in 0..request_count {
        let folder = directory.join(format!(
            "collection-{:02}/folder-{:02}",
            index / 100,
            index % 100 / 20
        ));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("request-{index:04}.toml")), format!(
            "id = \"request-{index}\"\nname = \"Get resource {index}\"\nschema_version = 1\n[request]\ntype = \"http\"\nmethod = \"GET\"\npath = \"/resources/{index}\"\nheaders = []\n"
        )).unwrap();
    }

    let collections = CollectionRegistry::from_path(&directory).unwrap();
    fs::remove_dir_all(directory).unwrap();
    collections
}

// Includes event dispatch, effects, Root, drawing and element cleanup. GPU
// presentation and the application's FPS monitor still require checking the native app.
// Run without CPU contention.
#[gpui_kit::test]
#[ignore = "manual 120 fps tab interaction budget"]
fn tabs_interaction_benchmark(cx: &mut TestAppContext) {
    let sample_count = std::env::var("REQUEST_EAGLE_BENCH_SAMPLES")
        .map(|value| value.parse::<usize>().expect("positive sample count"))
        .unwrap_or(120);
    assert!(sample_count > 0);
    #[expect(
        clippy::assertions_on_constants,
        reason = "Reject debug builds when the ignored benchmark runs, not when compiling ordinary tests."
    )]
    {
        assert!(!cfg!(debug_assertions), "run this benchmark with --release");
    }

    let mut failures = Vec::new();

    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
        cx.set_reduce_motion(true);
    });

    for tab_count in [100, 1_000, 10_000] {
        let mut layout = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                Layout::new(
                    CollectionRegistry::new(),
                    updater::init("1.2.3", cx),
                    window,
                    cx,
                )
            });
            layout = Some(view.clone());

            Root::new(view, window, cx)
        });
        let layout = layout.unwrap();

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

                samples.sort_by(f64::total_cmp);
                let mean = samples.iter().sum::<f64>() / sample_count as f64;
                let p99 = samples[(sample_count * 99).div_ceil(100) - 1];
                let over_budget = samples.iter().filter(|&&ms| ms > 1000. / 120.).count();
                let label = format!("{tab_count} tabs {width}x{height} {interaction}");
                eprintln!(
                    "{label}: mean {mean:.2} ms, p95 {:.2}, p99 {p99:.2}, max {:.2}; over 8.33 ms: {over_budget}/{sample_count}",
                    samples[(sample_count * 95).div_ceil(100) - 1],
                    samples[sample_count - 1],
                );
                if p99 > 1000. / 120. {
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
    let sample_count = std::env::var("REQUEST_EAGLE_BENCH_SAMPLES")
        .map(|value| value.parse::<usize>().expect("positive sample count"))
        .unwrap_or(120);
    assert!(sample_count > 0);
    #[expect(
        clippy::assertions_on_constants,
        reason = "Reject debug builds when the ignored benchmark runs, not when compiling ordinary tests."
    )]
    {
        assert!(!cfg!(debug_assertions), "run this benchmark with --release");
    }

    let mut failures = Vec::new();

    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        crate::actions::init(cx);
        cx.set_reduce_motion(true);
    });

    // "g" matches every request; the rest narrows to a single one.
    let query = [
        "g", "e", "t", "space", "r", "e", "s", "o", "u", "r", "c", "e", "space", "9",
    ];

    for request_count in [1_000, 10_000, 100_000] {
        let mut layout = None;
        let collections = collections(request_count);
        let (_root, cx) = cx.add_window_view(|window, cx| {
            let view =
                cx.new(|cx| Layout::new(collections, updater::init("1.2.3", cx), window, cx));
            layout = Some(view.clone());

            Root::new(view, window, cx)
        });
        let layout = layout.unwrap();
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

            for (interaction, mut samples) in samples {
                let count = samples.len();
                samples.sort_by(f64::total_cmp);
                let mean = samples.iter().sum::<f64>() / count as f64;
                let p99 = samples[(count * 99).div_ceil(100) - 1];
                let over_budget = samples.iter().filter(|&&ms| ms > 1000. / 120.).count();
                let label = format!("{request_count} requests {width}x{height} {interaction}");
                eprintln!(
                    "{label}: mean {mean:.2} ms, p95 {:.2}, p99 {p99:.2}, max {:.2}; over 8.33 ms: {over_budget}/{count}",
                    samples[(count * 95).div_ceil(100) - 1],
                    samples[count - 1],
                );
                if p99 > 1000. / 120. {
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
