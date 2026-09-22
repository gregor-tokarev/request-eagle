use gpui_kit::component::Theme;
use gpui_kit::{Hsla, TestAppContext};

fn contrast(foreground: Hsla, background: Hsla) -> f32 {
    let luminance = |color: Hsla| {
        let color = color.to_rgb();

        [(color.r, 0.2126), (color.g, 0.7152), (color.b, 0.0722)]
            .into_iter()
            .map(|(channel, weight)| {
                let linear = if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                };

                linear * weight
            })
            .sum::<f32>()
    };
    let foreground = luminance(background.blend(foreground));
    let background = luminance(background);

    (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05)
}

fn assert_readable(name: &str, role: &str, foreground: Hsla, background: Hsla) {
    let ratio = contrast(foreground, background);

    assert!(
        ratio >= 4.5,
        "{name}: {role} has {ratio:.2}:1 contrast, expected at least 4.5:1 ({foreground:?} on {background:?})"
    );
}

#[gpui_kit::test]
fn text_and_request_methods_remain_readable_on_their_surfaces(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::init(cx);

        for &(light, dark) in crate::THEME_PAIRS {
            for name in [light, dark] {
                assert!(crate::apply(name, cx));

                let theme = Theme::global(cx);
                let background = theme.background;
                let muted = background.blend(theme.muted);
                let sidebar = background.blend(theme.sidebar);
                let selected_sidebar = sidebar.blend(theme.sidebar_accent);
                let tab_bar = background.blend(theme.tab_bar);
                let active_tab = tab_bar.blend(theme.tab_active);
                let popover = background.blend(theme.popover);
                let keycap = background
                    .blend(theme.primary.opacity(0.08))
                    .blend(theme.foreground.opacity(0.06));

                for surface in [
                    background,
                    muted,
                    sidebar,
                    selected_sidebar,
                    tab_bar,
                    active_tab,
                    popover,
                    keycap,
                ] {
                    assert_readable(name, "secondary text", theme.muted_foreground, surface);
                }

                for (role, color) in [
                    ("GET / success", theme.success),
                    ("POST / warning", theme.warning),
                    ("PUT / info", theme.info),
                    ("DELETE / error", theme.danger),
                ] {
                    for surface in [
                        background,
                        muted,
                        sidebar,
                        selected_sidebar,
                        tab_bar,
                        active_tab,
                        popover,
                        background.blend(color.opacity(0.15)),
                    ] {
                        assert_readable(name, role, color, surface);
                    }
                }

                for (role, color, surface) in [
                    ("body text", theme.foreground, background),
                    ("selected section", theme.foreground, muted),
                    ("sidebar", theme.sidebar_foreground, sidebar),
                    (
                        "selected sidebar",
                        theme.sidebar_accent_foreground,
                        selected_sidebar,
                    ),
                    ("inactive tab", theme.tab_foreground, tab_bar),
                    ("active tab", theme.tab_active_foreground, active_tab),
                    ("popover", theme.popover_foreground, popover),
                    ("link", theme.link, background),
                    ("theme card checkmark", theme.link, muted),
                    ("hovered link", theme.link_hover, background),
                    ("pressed link", theme.link_active, background),
                ] {
                    assert_readable(name, role, color, surface);
                }
            }
        }
    });
}

#[gpui_kit::test]
fn button_labels_remain_readable_when_hovered_and_pressed(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::init(cx);

        for &(light, dark) in crate::THEME_PAIRS {
            for name in [light, dark] {
                assert!(crate::apply(name, cx));

                let theme = Theme::global(cx);

                for background in [
                    theme.button_primary,
                    theme.button_primary_hover,
                    theme.button_primary_active,
                ] {
                    assert_readable(
                        name,
                        "primary button",
                        theme.button_primary_foreground,
                        theme.background.blend(background),
                    );
                }

                let delete_prompt = theme
                    .background
                    .blend(theme.sidebar)
                    .blend(theme.sidebar_accent);

                for background in [
                    theme.button_danger,
                    theme.button_danger_hover,
                    theme.button_danger_active,
                ] {
                    assert_readable(
                        name,
                        "delete confirmation",
                        theme.button_danger_foreground,
                        delete_prompt.blend(background),
                    );
                }
            }
        }
    });
}

#[gpui_kit::test]
fn editor_syntax_remains_readable_on_the_active_line(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::init(cx);

        for &(light, dark) in crate::THEME_PAIRS {
            for name in [light, dark] {
                assert!(crate::apply(name, cx));

                let theme = Theme::global(cx);
                let style = &theme.highlight_theme.style;
                let background = style.editor_background.unwrap_or(theme.background);
                let active_line = background.blend(style.editor_active_line.unwrap_or(background));
                let gutter = background.blend(style.editor_gutter_background.unwrap_or(background));
                let search_match = Hsla {
                    s: 0.1,
                    ..theme.selection
                };
                let selected_surfaces = [
                    background.blend(theme.selection),
                    active_line.blend(theme.selection),
                    background.blend(search_match).blend(theme.selection),
                    active_line.blend(search_match).blend(theme.selection),
                    // A focused selection over the active Find result paints both.
                    background
                        .blend(search_match)
                        .blend(theme.selection)
                        .blend(theme.selection),
                    active_line
                        .blend(search_match)
                        .blend(theme.selection)
                        .blend(theme.selection),
                ];

                // GPUI's editor uses the UI foregrounds for plain text and gutters.
                assert_readable(name, "plain editor text", theme.foreground, background);
                assert_readable(name, "editor gutter", theme.muted_foreground, gutter);
                assert_readable(name, "active gutter", theme.foreground, active_line);

                for surface in selected_surfaces {
                    assert_readable(name, "selected editor text", theme.foreground, surface);
                }

                for (role, color, surface) in [
                    ("editor text", style.editor_foreground, background),
                    ("active line text", style.editor_foreground, active_line),
                    ("line numbers", style.editor_line_number, gutter),
                    (
                        "active line number",
                        style.editor_active_line_number,
                        gutter,
                    ),
                ] {
                    if let Some(color) = color {
                        assert_readable(name, role, color, surface);
                    }
                }

                // These tokens cover JSON and the HTML/XML/text response editors.
                for token in [
                    "attribute",
                    "boolean",
                    "comment",
                    "comment.doc",
                    "constant",
                    "constructor",
                    "embedded",
                    "emphasis",
                    "emphasis.strong",
                    "enum",
                    "function",
                    "hint",
                    "keyword",
                    "label",
                    "link_text",
                    "link_uri",
                    "number",
                    "operator",
                    "predictive",
                    "preproc",
                    "primary",
                    "property",
                    "punctuation",
                    "punctuation.bracket",
                    "punctuation.delimiter",
                    "punctuation.list_marker",
                    "punctuation.special",
                    "string",
                    "string.escape",
                    "string.regex",
                    "string.special",
                    "string.special.symbol",
                    "tag",
                    "tag.doctype",
                    "text.code.span",
                    "text.literal",
                    "title",
                    "type",
                    "variable",
                    "variable.special",
                    "variant",
                ] {
                    if let Some(color) = style.syntax.style(token).and_then(|style| style.color) {
                        assert_readable(name, token, color, background);
                        assert_readable(name, token, color, active_line);

                        if [
                            "property",
                            "string",
                            "string.escape",
                            "number",
                            "boolean",
                            "constant",
                            "punctuation",
                            "punctuation.bracket",
                            "punctuation.delimiter",
                        ]
                        .contains(&token)
                        {
                            for surface in selected_surfaces {
                                assert_readable(name, token, color, surface);
                            }
                        }
                    }
                }
            }
        }
    });
}
