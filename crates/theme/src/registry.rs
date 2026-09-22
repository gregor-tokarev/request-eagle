use crate::{apply_preferences, theme_pair};

use gpui_kit::App;
use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use preferences::{AppearanceMode, AppearancePreferences};

include!(concat!(env!("OUT_DIR"), "/embedded_theme_sets.rs"));

pub fn init(cx: &mut App) {
    let registry = ThemeRegistry::global_mut(cx);

    for theme_set in EMBEDDED_THEME_SETS {
        registry
            .load_themes_from_str(theme_set)
            .expect("bundled gpui-component theme should be valid");
    }

    preferences::init(cx);
    link_saved_themes(cx);

    cx.observe_global::<preferences::Preferences>(|cx| {
        apply_preferences(cx.window_appearance(), cx);
    })
    .detach();
    apply_preferences(cx.window_appearance(), cx);
}

fn link_saved_themes(cx: &mut App) {
    let preferences = &cx.global::<preferences::Preferences>().appearance;
    let dark = match preferences.mode {
        AppearanceMode::System => ThemeMode::from(cx.window_appearance()).is_dark(),
        AppearanceMode::Light => false,
        AppearanceMode::Dark => true,
    };
    let (active, other) = if dark {
        (&preferences.dark_theme, &preferences.light_theme)
    } else {
        (&preferences.light_theme, &preferences.dark_theme)
    };

    // Keep the active family when upgrading, or the other saved variant if
    // the active theme was removed. The two retired Catppuccin variants
    // share Latte's remaining dark partner, Mocha.
    let saved_pair = |name: &str| {
        theme_pair(match name {
            "Catppuccin Frappe" | "Catppuccin Macchiato" => "Catppuccin Mocha",
            name => name,
        })
    };
    let (light, dark) = saved_pair(active)
        .or_else(|| saved_pair(other))
        .unwrap_or_else(|| {
            theme_pair(&AppearancePreferences::default().light_theme)
                .expect("default appearance themes should be paired")
        });

    if preferences.light_theme == light && preferences.dark_theme == dark {
        return;
    }

    if let Err(error) = preferences::update(cx, |preferences| {
        preferences.appearance.light_theme = light.into();
        preferences.appearance.dark_theme = dark.into();
    }) {
        eprintln!("Failed to save paired appearance themes: {error:#}");
    }
}

pub fn apply(name: &str, cx: &mut App) -> bool {
    let Some(config) = crate::config(name, cx) else {
        return false;
    };

    Theme::global_mut(cx).apply_config(&config);
    Theme::sync_base(cx);

    cx.refresh_windows();

    true
}
