use std::rc::Rc;

use gpui_kit::component::{ThemeConfig, ThemeRegistry, highlighter::SyntaxColors};
use gpui_kit::{App, rgb};

/// Resolve the same application palette for previews and active windows.
pub fn config(name: &str, cx: &App) -> Option<Rc<ThemeConfig>> {
    let config = ThemeRegistry::global(cx).themes().get(name)?.clone();

    if name != "Default Light" && name != "Default Dark" {
        return Some(config);
    }

    // The component registry owns this palette and does not accept replacements.
    // Its status colors are button fills, but request methods use them as text.
    let mut config = (*config).clone();

    if name == "Default Dark" {
        config.colors.button_danger_foreground = Some("#fbb6b6".into());

        if let Some(highlight) = &mut config.highlight {
            let syntax: SyntaxColors = serde_json::from_str(
                r##"{
                    "boolean": { "color": "#dfac00" },
                    "constant": { "color": "#dfac00" },
                    "number": { "color": "#dfac00" }
                }"##,
            )
            .expect("default syntax corrections should be valid");

            highlight.syntax.boolean = syntax.boolean;
            highlight.syntax.constant = syntax.constant;
            highlight.syntax.number = syntax.number;
        }

        return Some(Rc::new(config));
    }

    config.colors.muted_foreground = Some("#626262".into());
    config.colors.success = Some("#147638".into());
    config.colors.warning = Some("#806204".into());
    config.colors.danger = Some("#c81111".into());
    config.colors.info = Some("#047083".into());
    config.colors.button_danger_foreground = Some("#770a0a".into());

    if let Some(highlight) = &mut config.highlight {
        highlight.editor_line_number = Some(rgb(0x767676).into());

        // Syntax style fields are private in GPUI Kit, so use its theme format.
        let syntax: SyntaxColors = serde_json::from_str(
            r##"{
                "attribute": { "color": "#866d2c" },
                "boolean": { "color": "#9a0509" },
                "comment": { "color": "#006ddb" },
                "constant": { "color": "#9a0509" },
                "link_uri": { "color": "#676f8f", "font_style": "italic" },
                "number": { "color": "#002ae0" },
                "string": { "color": "#025806" },
                "string.escape": { "color": "#025806" }
            }"##,
        )
        .expect("default syntax corrections should be valid");

        highlight.syntax.attribute = syntax.attribute;
        highlight.syntax.boolean = syntax.boolean;
        highlight.syntax.comment = syntax.comment;
        highlight.syntax.comment_doc = syntax.comment;
        highlight.syntax.constant = syntax.constant;
        highlight.syntax.link_uri = syntax.link_uri;
        highlight.syntax.number = syntax.number;
        highlight.syntax.string = syntax.string;
        highlight.syntax.string_escape = syntax.string_escape;
    }

    Some(Rc::new(config))
}
