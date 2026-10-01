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

    let config = crate::registry::config(name, cx)
        .filter(|theme| theme.mode == mode)
        .or_else(|| crate::registry::config(fallback, cx))
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
