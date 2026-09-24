use gpui_kit::component::{ActiveTheme as _, Theme, ThemeColor, ThemeRegistry};
use gpui_kit::{
    App, Bounds, ContentMask, IntoElement, Pixels, SharedString, Styled as _, TextAlign, TextRun,
    canvas, fill, point, px, size,
};

/// Resolved once for the bundled catalog, independent of the active app theme.
pub(super) struct ThemePreview {
    pub name: SharedString,
    pub dark: bool,
    pub(super) colors: ThemeColor,
    radius: Pixels,
}

impl ThemePreview {
    pub fn catalog(cx: &App) -> Vec<Self> {
        ThemeRegistry::global(cx)
            .sorted_themes()
            .into_iter()
            .map(|config| {
                let config = request_eagle_theme::config(&config.name, cx)
                    .expect("catalog themes should be registered");
                let mut theme = Theme::default();
                theme.apply_config(&config);

                Self {
                    name: config.name.clone(),
                    dark: config.mode.is_dark(),
                    colors: theme.colors,
                    radius: theme.radius_tokens().sm,
                }
            })
            .collect()
    }

    pub fn render(&self, selected: bool, cx: &App) -> impl IntoElement + use<> {
        let colors = self.colors;
        let name = self.name.clone();
        let foreground = cx.theme().foreground;
        let link = cx.theme().link;
        let font_size = cx.theme().font_size * 0.75;
        let gap = cx.theme().font_size * 0.5;
        let preview_height = cx.theme().font_size * 5.;
        let inset = cx.theme().font_size * 0.25;
        // The card has an 8 px (0.5 rem) inset; its preview follows the inner corner.
        let frame_radius = (cx.theme().radius_tokens().lg - gap).max(Pixels::ZERO);
        let inner_radius = (frame_radius - inset).max(Pixels::ZERO);
        // These miniature controls visualize the previewed theme, not the active palette.
        let mark_radius = self.radius * 0.5;

        // The button owns focus and accessibility. Paint the miniature card
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
                            color: link,
                            ..text_run
                        }],
                        None,
                    )
                });

                (label, check)
            },
            move |card_bounds, (label, check), window, cx| {
                let bounds = Bounds::new(
                    card_bounds.origin,
                    size(card_bounds.size.width, preview_height),
                );
                window.paint_quad(gpui_kit::quad(
                    bounds,
                    frame_radius,
                    colors.background,
                    px(1.),
                    colors.border,
                    gpui_kit::BorderStyle::Solid,
                ));

                window.paint_quad(
                    fill(
                        Bounds::new(
                            bounds.origin + point(inset, inset),
                            size(
                                bounds.size.width * 0.22 - inset,
                                bounds.size.height - inset * 2.,
                            ),
                        ),
                        colors.sidebar,
                    )
                    .corner_radii(inner_radius),
                );

                // These marks do not overlap. One paint layer avoids inserting
                // each mark separately into GPUI's scene ordering bounds tree.
                window.paint_layer(bounds, |window| {
                    let mut rectangle = |x, y, width, height, radius, color| {
                        let rectangle = Bounds::new(
                            bounds.origin + point(bounds.size.width * x, bounds.size.height * y),
                            size(bounds.size.width * width, bounds.size.height * height),
                        );

                        window
                            .paint_quad(fill(rectangle, color).corner_radii(mark_radius * radius));
                    };

                    rectangle(0.05, 0.13, 0.13, 0.05, 1., colors.primary);
                    rectangle(0.05, 0.28, 0.13, 0.05, 1., colors.foreground.opacity(0.3));
                    rectangle(0.05, 0.43, 0.13, 0.05, 1., colors.foreground.opacity(0.2));

                    rectangle(0.28, 0.13, 0.66, 0.16, 2., colors.secondary);
                    rectangle(0.28, 0.40, 0.39, 0.05, 1., colors.primary);
                    rectangle(0.28, 0.55, 0.29, 0.05, 1., colors.foreground.opacity(0.4));

                    for (index, color) in [
                        colors.primary,
                        colors.success,
                        colors.warning,
                        colors.danger,
                        colors.info,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let swatch = Bounds::new(
                            bounds.origin
                                + point(
                                    bounds.size.width * 0.28 + gap * 1.5 * index as f32,
                                    bounds.size.height * 0.73,
                                ),
                            size(gap, gap),
                        );
                        window.paint_quad(fill(swatch, color).corner_radii(gap / 2.));
                    }
                });

                let label_origin = card_bounds.origin + point(px(0.), preview_height + gap);
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
        .h(preview_height + gap + font_size)
    }
}
