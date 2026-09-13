use gpui_kit::component::{ActiveTheme as _, Theme, ThemeColor, ThemeRegistry};
use gpui_kit::{
    App, Bounds, ContentMask, IntoElement, SharedString, Styled as _, TextAlign, TextRun, canvas,
    fill, point, px, size,
};

/// Resolved once for the bundled catalog, independent of the active app theme.
pub(super) struct ThemePreview {
    pub name: SharedString,
    pub dark: bool,
    colors: ThemeColor,
}

impl ThemePreview {
    pub fn catalog(cx: &App) -> Vec<Self> {
        ThemeRegistry::global(cx)
            .sorted_themes()
            .into_iter()
            .map(|config| {
                let mut theme = Theme::default();
                theme.apply_config(config);

                Self {
                    name: config.name.clone(),
                    dark: config.mode.is_dark(),
                    colors: theme.colors,
                }
            })
            .collect()
    }

    pub fn render(&self, selected: bool, cx: &App) -> impl IntoElement + use<> {
        let colors = self.colors;
        let name = self.name.clone();
        let foreground = cx.theme().foreground;
        let primary = cx.theme().primary;
        let font_size = cx.theme().font_size * 0.75;
        let gap = cx.theme().font_size * 0.5;

        // The button owns focus and accessibility. Paint the fixed-size card
        // in one element so scrolling does not lay out its individual shapes.
        canvas(
            move |_, window, _| {
                let text_run = TextRun {
                    len: name.len(),
                    font: window.text_style().font(),
                    color: foreground,
                    ..Default::default()
                };
                let label = window.text_system().shape_line(
                    name,
                    font_size,
                    std::slice::from_ref(&text_run),
                    None,
                );

                let check = selected.then(|| {
                    window.text_system().shape_line(
                        "✓".into(),
                        font_size,
                        &[TextRun {
                            len: "✓".len(),
                            color: primary,
                            ..text_run
                        }],
                        None,
                    )
                });

                (label, check)
            },
            move |card_bounds, (label, check), window, cx| {
                let bounds = Bounds::new(card_bounds.origin, size(card_bounds.size.width, px(76.)));
                window.paint_quad(gpui_kit::quad(
                    bounds,
                    px(4.),
                    colors.background,
                    px(1.),
                    colors.border,
                    gpui_kit::BorderStyle::Solid,
                ));

                window.paint_quad(
                    fill(
                        Bounds::new(
                            bounds.origin
                                + point(bounds.size.width * 0.01, bounds.size.height * 0.02),
                            size(bounds.size.width * 0.22, bounds.size.height * 0.96),
                        ),
                        colors.sidebar,
                    )
                    .corner_radii(px(3.)),
                );

                // These marks do not overlap. One paint layer avoids inserting
                // each mark separately into GPUI's scene ordering bounds tree.
                window.paint_layer(bounds, |window| {
                    let mut rectangle = |x, y, width, height, radius, color| {
                        let rectangle = Bounds::new(
                            bounds.origin + point(bounds.size.width * x, bounds.size.height * y),
                            size(bounds.size.width * width, bounds.size.height * height),
                        );

                        window.paint_quad(fill(rectangle, color).corner_radii(px(radius)));
                    };

                    rectangle(0.05, 0.13, 0.13, 0.05, 1., colors.primary);
                    rectangle(0.05, 0.28, 0.13, 0.05, 1., colors.foreground.opacity(0.3));
                    rectangle(0.05, 0.43, 0.13, 0.05, 1., colors.foreground.opacity(0.2));

                    rectangle(0.28, 0.13, 0.66, 0.16, 2., colors.secondary);
                    rectangle(0.28, 0.40, 0.39, 0.05, 1., colors.primary);
                    rectangle(0.28, 0.55, 0.29, 0.05, 1., colors.foreground.opacity(0.4));

                    for (index, color) in [
                        colors.primary,
                        colors.green,
                        colors.yellow,
                        colors.red,
                        colors.blue,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let swatch = Bounds::new(
                            bounds.origin
                                + point(
                                    bounds.size.width * 0.28 + px(index as f32 * 11.),
                                    bounds.size.height * 0.73,
                                ),
                            size(px(7.), px(7.)),
                        );
                        window.paint_quad(fill(swatch, color).corner_radii(px(3.5)));
                    }
                });

                let label_origin = card_bounds.origin + point(px(0.), px(76.) + gap);
                let label_width =
                    card_bounds.size.width - if selected { font_size * 1.5 } else { px(0.) };

                window.with_content_mask(
                    Some(ContentMask {
                        bounds: Bounds::new(label_origin, size(label_width, font_size * 1.5)),
                    }),
                    |window| {
                        let _ =
                            label.paint(label_origin, font_size, TextAlign::Left, None, window, cx);
                    },
                );

                if let Some(check) = check {
                    let origin =
                        label_origin + point(card_bounds.size.width - check.width(), px(0.));
                    let _ = check.paint(origin, font_size, TextAlign::Left, None, window, cx);
                }
            },
        )
        .w_full()
        .h(px(76.) + gap + font_size)
    }
}
