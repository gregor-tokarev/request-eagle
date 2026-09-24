use crate::apply_preferences;

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeSet};
use gpui_kit::{App, Global};

include!(concat!(env!("OUT_DIR"), "/embedded_theme_sets.rs"));

struct ThemeCatalog(Vec<Rc<ThemeConfig>>);

impl Global for ThemeCatalog {}

pub fn init(cx: &mut App) {
    let mut themes = Vec::new();

    for theme_set in EMBEDDED_THEME_SETS {
        let theme_set: ThemeSet =
            serde_json::from_str(theme_set).expect("bundled Request Eagle themes should be valid");

        themes.extend(theme_set.themes.into_iter().map(Rc::new));
    }

    themes.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then(a.mode.cmp(&b.mode))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    cx.set_global(ThemeCatalog(themes));

    cx.observe_global::<preferences::Preferences>(|cx| {
        apply_preferences(cx.window_appearance(), cx);
    })
    .detach();
    apply_preferences(cx.window_appearance(), cx);
}

pub fn themes(cx: &App) -> &[Rc<ThemeConfig>] {
    &cx.global::<ThemeCatalog>().0
}

pub fn config(name: &str, cx: &App) -> Option<Rc<ThemeConfig>> {
    themes(cx).iter().find(|theme| theme.name == name).cloned()
}

pub fn apply(name: &str, cx: &mut App) -> bool {
    let Some(config) = config(name, cx) else {
        return false;
    };

    Theme::global_mut(cx).apply_config(&config);
    Theme::sync_base(cx);

    cx.refresh_windows();

    true
}
