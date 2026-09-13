use super::AppearanceSettings;
use gpui_kit::TestAppContext;
use std::time::Instant;

#[gpui_kit::test]
#[ignore = "manual appearance render benchmark"]
fn appearance_render_benchmark(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });

    let (page, cx) = cx.add_window_view(AppearanceSettings::new);
    for (width, height) in [(1024., 768.), (1440., 900.), (3440., 1410.)] {
        cx.simulate_resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(height)));
        cx.run_until_parked();

        let mut draws = Vec::new();
        let mut scrolls = Vec::new();

        for index in 0..140 {
            let duration = cx.update(|window, cx| {
                window.refresh();
                let started = Instant::now();
                window.draw(cx).clear(cx);
                started.elapsed()
            });

            if index >= 20 {
                draws.push(duration);
            }
        }

        for index in 0..140 {
            let duration = cx.update(|window, cx| {
                page.read(cx)
                    .list
                    .scroll_by(gpui_kit::px(if index % 80 < 40 { 24. } else { -24. }));
                window.refresh();
                let started = Instant::now();
                window.draw(cx).clear(cx);
                started.elapsed()
            });

            if index >= 20 {
                scrolls.push(duration);
            }
        }

        for (label, mut samples) in [("draw", draws), ("scroll", scrolls)] {
            samples.sort();
            let mean = samples
                .iter()
                .map(|duration| duration.as_secs_f64())
                .sum::<f64>()
                * 1000.
                / samples.len() as f64;
            eprintln!(
                "{width}x{height} {label}: mean {mean:.2} ms, p95 {:?}, p99 {:?}, max {:?}",
                samples[113], samples[118], samples[119]
            );
        }
    }
}

#[gpui_kit::test]
fn preview_cards_apply_the_selected_palette(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });

    let (_, cx) = cx.add_window_view(AppearanceSettings::new);

    for (selector, expected_light, expected_dark) in [
        (
            "theme-Catppuccin Latte",
            "Catppuccin Latte",
            "Catppuccin Mocha",
        ),
        ("appearance-Light", "Catppuccin Latte", "Catppuccin Mocha"),
    ] {
        let bounds = cx
            .debug_bounds(selector)
            .expect("visible appearance control");
        cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        cx.run_until_parked();

        cx.read(|cx| {
            let preferences = cx
                .try_global::<preferences::Preferences>()
                .cloned()
                .unwrap_or_default()
                .appearance;
            assert_eq!(preferences.light_theme, expected_light);
            assert_eq!(preferences.dark_theme, expected_dark);
        });
    }

    cx.read(|cx| {
        let theme = gpui_kit::component::Theme::global(cx);
        assert!(!theme.is_dark());
        assert_eq!(theme.light_theme.name, "Catppuccin Latte");
    });
}

#[gpui_kit::test]
fn catalog_virtualizes_rows_and_reflows_on_resize(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });

    let (page, cx) = cx.add_window_view(AppearanceSettings::new);
    assert!(cx.debug_bounds("theme-Solarized Dark").is_none());

    cx.update(|_, cx| {
        page.update(cx, |page, cx| {
            page.list.scroll_to(gpui_kit::ListOffset {
                item_ix: page.rows.len() - 1,
                offset_in_item: gpui_kit::px(0.),
            });
            cx.notify();
        });
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("theme-Solarized Dark").is_some());

    for (width, expected_columns) in [(360., 1), (1024., 4)] {
        cx.simulate_resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(768.)));
        cx.run_until_parked();
        cx.read(|cx| assert_eq!(page.read(cx).columns, expected_columns));
        assert!(cx.debug_bounds("appearance-Light").is_some());
    }
}

#[gpui_kit::test]
fn every_theme_card_links_both_variants_and_switches_modes(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });

    let (page, cx) = cx.add_window_view(AppearanceSettings::new);

    for &(light, dark) in request_eagle_theme::THEME_PAIRS {
        for name in [light, dark] {
            cx.update(|_, cx| {
                page.update(cx, |page, cx| {
                    let row = page
                        .rows
                        .iter()
                        .position(|row| match row {
                            super::page::PageRow::Themes(indices) => indices
                                .iter()
                                .any(|&index| page.previews[index].name == name),
                            _ => false,
                        })
                        .unwrap();

                    page.list.scroll_to(gpui_kit::ListOffset {
                        item_ix: row,
                        offset_in_item: gpui_kit::px(0.),
                    });
                    cx.notify();
                });
            });
            cx.run_until_parked();

            let bounds = cx
                .debug_bounds(format!("theme-{name}").leak())
                .expect("theme card is visible");
            cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
            cx.run_until_parked();

            cx.read(|cx| {
                let preferences = &cx.global::<preferences::Preferences>().appearance;
                assert_eq!(preferences.light_theme, light, "{name}");
                assert_eq!(preferences.dark_theme, dark, "{name}");
            });

            cx.update(|_, cx| {
                page.update(cx, |page, cx| {
                    page.list.scroll_to(gpui_kit::ListOffset::default());
                    cx.notify();
                });
            });
            cx.run_until_parked();

            for (selector, expected) in [("appearance-Light", light), ("appearance-Dark", dark)] {
                let bounds = cx.debug_bounds(selector).unwrap();
                cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
                cx.run_until_parked();
                cx.read(|cx| {
                    assert_eq!(
                        gpui_kit::component::Theme::global(cx).highlight_theme.name,
                        expected
                    )
                });
            }

            let bounds = cx.debug_bounds("appearance-System").unwrap();
            cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
            cx.run_until_parked();

            cx.update(|_, cx| {
                assert_eq!(
                    cx.global::<preferences::Preferences>().appearance.mode,
                    preferences::AppearanceMode::System
                );

                for (appearance, expected) in [
                    (gpui_kit::WindowAppearance::Light, light),
                    (gpui_kit::WindowAppearance::Dark, dark),
                ] {
                    request_eagle_theme::apply_preferences(appearance, cx);
                    assert_eq!(
                        gpui_kit::component::Theme::global(cx).highlight_theme.name,
                        expected
                    );
                }
            });
        }
    }
}

#[gpui_kit::test]
fn keyboard_can_reach_offscreen_themes(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
    });
    let (page, cx) = cx.add_window_view(AppearanceSettings::new);

    cx.update(|window, cx| {
        let page = page.read(cx);
        let index = page
            .previews
            .iter()
            .position(|preview| preview.name == "Default Light")
            .unwrap();
        let focus = page.theme_focus[index].clone();
        window.focus(&focus, cx);
    });

    for _ in 0..21 {
        cx.simulate_keystrokes("tab");
    }
    let keystroke = gpui_kit::Keystroke::parse("enter").unwrap();
    cx.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    cx.read(|cx| {
        assert_eq!(
            cx.try_global::<preferences::Preferences>()
                .cloned()
                .unwrap_or_default()
                .appearance
                .dark_theme,
            "Solarized Dark"
        )
    });
    assert!(cx.debug_bounds("theme-Solarized Dark").is_some());
}
