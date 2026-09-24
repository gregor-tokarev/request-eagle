use super::{apply, init};
use gpui_kit::component::Theme;
use gpui_kit::{TestAppContext, base};

#[gpui_kit::test]
fn bundled_themes_have_unique_names_and_can_be_applied(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        init(cx);

        let themes = crate::themes(cx).to_vec();
        let mut names = std::collections::HashSet::new();
        assert!(!themes.is_empty());

        for config in themes {
            let name = config.name.as_ref();
            assert!(names.insert(config.name.clone()), "duplicate theme: {name}");
            assert!(apply(name, cx));
            assert_eq!(Theme::global(cx).is_dark(), config.mode.is_dark());
            assert_eq!(Theme::global(cx).highlight_theme.name, name);
        }
    });
}

#[gpui_kit::test]
fn startup_preserves_independent_theme_choices(cx: &mut TestAppContext) {
    use preferences::AppearanceMode::{Dark, Light};

    for (mode, light, dark, expected_active) in [
        (Light, "Catppuccin Latte", "Ayu Dark", "Catppuccin Latte"),
        (Dark, "Catppuccin Latte", "Ayu Dark", "Ayu Dark"),
        (Light, "removed", "Solarized Dark", "Ayu Light"),
        (Dark, "Gruvbox Light", "removed", "Ayu Dark"),
    ] {
        cx.update(|cx| {
            gpui_kit::init(cx);
            preferences::update(cx, |preferences| {
                preferences.appearance.mode = mode;
                preferences.appearance.light_theme = light.into();
                preferences.appearance.dark_theme = dark.into();
            })
            .unwrap();

            init(cx);

            let preferences = &cx.global::<preferences::Preferences>().appearance;
            assert_eq!(preferences.light_theme, light);
            assert_eq!(preferences.dark_theme, dark);
            assert_eq!(Theme::global(cx).highlight_theme.name, expected_active);
        });
    }
}

#[gpui_kit::test]
fn resize_handles_follow_the_applied_theme(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        init(cx);

        for name in ["Ayu Light", "Ayu Dark"] {
            assert!(apply(name, cx));

            let theme = Theme::global(cx);
            let base_theme = base::Theme::global(cx);

            assert_eq!(base_theme.resizable.handle, Some(theme.border), "{name}");
            assert_eq!(
                base_theme.resizable.active_handle,
                Some(theme.drag_border),
                "{name}"
            );
        }
    });
}
