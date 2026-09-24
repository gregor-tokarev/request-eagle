use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, WindowAppearance, px};
use preferences::{AppearanceMode, AppearancePreferences};

/// Interpret appearance preferences without owning or writing their storage.
pub fn apply_preferences(appearance: WindowAppearance, cx: &mut App) {
    let preferences = cx
        .try_global::<preferences::Preferences>()
        .cloned()
        .unwrap_or_default()
        .appearance;

    let mode = match preferences.mode {
        AppearanceMode::System => ThemeMode::from(appearance),
        AppearanceMode::Light => ThemeMode::Light,
        AppearanceMode::Dark => ThemeMode::Dark,
    };

    let defaults = AppearancePreferences::default();
    let (name, fallback) = if mode.is_dark() {
        (&preferences.dark_theme, &defaults.dark_theme)
    } else {
        (&preferences.light_theme, &defaults.light_theme)
    };

    let config = crate::config(name, cx)
        .filter(|theme| theme.mode == mode)
        .or_else(|| crate::config(fallback, cx))
        .expect("default appearance themes should be bundled");

    let theme = Theme::global_mut(cx);
    theme.apply_config(&config);

    theme.font_size = px(preferences.interface_font_size);
    theme.mono_font_family = if preferences.editor_font.is_empty() {
        Theme::default().mono_font_family
    } else {
        preferences.editor_font.into()
    };

    Theme::sync_base(cx);
    cx.refresh_windows();
}

#[cfg(test)]
mod tests {
    use super::apply_preferences;
    use gpui_kit::component::Theme;
    use gpui_kit::{TestAppContext, WindowAppearance, base, px};
    use preferences::{AppearanceMode, AppearancePreferences};

    #[gpui_kit::test]
    fn preference_changes_apply_palettes_and_typography(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            preferences::init(cx);
            crate::init(cx);
        });
        cx.update(|cx| {
            preferences::update(cx, |preferences| {
                preferences.appearance = AppearancePreferences {
                    mode: AppearanceMode::System,
                    dark_theme: "Catppuccin Mocha".into(),
                    editor_font: "Menlo".into(),
                    interface_font_size: 20.,
                    ..Default::default()
                };
            })
            .unwrap();
        });

        // The shared store notification applies the new preferences itself.
        cx.read(|cx| {
            assert_eq!(Theme::global(cx).font_size, px(20.));
            assert_eq!(Theme::global(cx).mono_font_family, "Menlo");
        });

        cx.update(|cx| {
            for (appearance, dark, name) in [
                (WindowAppearance::Light, false, "Ayu Light"),
                (WindowAppearance::Dark, true, "Catppuccin Mocha"),
            ] {
                apply_preferences(appearance, cx);
                let theme = Theme::global(cx);
                assert_eq!(theme.mode.is_dark(), dark);
                assert_eq!(theme.highlight_theme.name, name);
                assert_eq!(base::Theme::global(cx).resizable.handle, Some(theme.border));
            }
            preferences::update(cx, |preferences| {
                preferences.appearance.mode = AppearanceMode::Light;
            })
            .unwrap();
        });
        cx.update(|cx| {
            apply_preferences(WindowAppearance::Dark, cx);
            assert!(!Theme::global(cx).is_dark());
        });
    }

    #[gpui_kit::test]
    fn missing_or_wrong_mode_themes_use_the_default_palette(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            preferences::init(cx);
            crate::init(cx);
            preferences::update(cx, |preferences| {
                preferences.appearance.light_theme = "Ayu Dark".into();
                preferences.appearance.dark_theme = "removed".into();
                preferences.appearance.mode = AppearanceMode::System;
            })
            .unwrap();
        });
        cx.update(|cx| {
            apply_preferences(WindowAppearance::Light, cx);
            assert_eq!(Theme::global(cx).highlight_theme.name, "Ayu Light");
            apply_preferences(WindowAppearance::Dark, cx);
            assert_eq!(Theme::global(cx).highlight_theme.name, "Ayu Dark");
        });
    }

    #[gpui_kit::test]
    fn initialization_uses_preferences_loaded_before_the_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            preferences::update(cx, |preferences| {
                preferences.appearance.mode = AppearanceMode::Light;
                preferences.appearance.light_theme = "Catppuccin Latte".into();
                preferences.appearance.interface_font_size = 21.;
            })
            .unwrap();
            crate::init(cx);

            let theme = Theme::global(cx);
            assert_eq!(theme.highlight_theme.name, "Catppuccin Latte");
            assert_eq!(theme.font_size, px(21.));
        });
    }
}
