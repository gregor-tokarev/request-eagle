use super::theme_preview::ThemePreview;

use gpui_kit::component::{
    button::*,
    select::{SearchableVec, Select, SelectEvent, SelectState},
    *,
};
use gpui_kit::{prelude::*, *};
use preferences::{AppearanceMode, AppearancePreferences};

type FontList = SearchableVec<SharedString>;

#[derive(Clone)]
pub(super) enum PageRow {
    Header,
    Heading(bool),
    Themes(Vec<usize>),
}

pub(crate) struct AppearanceSettings {
    font: Entity<SelectState<FontList>>,
    pub(super) previews: Vec<ThemePreview>,
    pub(super) theme_focus: Vec<FocusHandle>,
    pub(super) list: gpui_kit::ListState,
    pub(super) rows: Vec<PageRow>,
    pub(super) columns: usize,
    layout_font_size: Pixels,
    error: Option<String>,
    _font_subscription: Subscription,
}

impl AppearanceSettings {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let preferences = cx
            .try_global::<preferences::Preferences>()
            .cloned()
            .unwrap_or_default()
            .appearance;

        let mut names = cx.text_system().all_font_names();
        names.sort_by_key(|name| name.to_lowercase());
        names.dedup();

        if !preferences.editor_font.is_empty() && !names.contains(&preferences.editor_font) {
            names.insert(0, preferences.editor_font.clone());
        }

        let mut fonts = vec![SharedString::from("Default monospace")];
        fonts.extend(names.into_iter().map(SharedString::from));

        let selected = fonts
            .iter()
            .position(|font| font.as_ref() == preferences.editor_font)
            .unwrap_or(0);
        let font = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(fonts),
                Some(IndexPath::new(selected)),
                window,
                cx,
            )
            .searchable(true)
        });

        let subscription = cx.subscribe(&font, |this, _, event: &SelectEvent<FontList>, cx| {
            if let SelectEvent::Confirm(Some(font)) = event {
                let mut preferences = cx
                    .try_global::<preferences::Preferences>()
                    .cloned()
                    .unwrap_or_default()
                    .appearance;
                preferences.editor_font = if font == "Default monospace" {
                    String::new()
                } else {
                    font.to_string()
                };

                this.save(preferences, cx);
            }
        });

        let previews = ThemePreview::catalog(cx);
        let theme_focus = previews.iter().map(|_| cx.focus_handle()).collect();

        Self {
            font,
            previews,
            theme_focus,
            // Measure row heights once so fast scrolling and the scrollbar know
            // the full catalog extent. Subsequent frames render only visible rows.
            list: gpui_kit::ListState::new(0, ListAlignment::Top, px(0.)).measure_all(),
            rows: Vec::new(),
            columns: 0,
            layout_font_size: px(0.),
            error: None,
            _font_subscription: subscription,
        }
    }

    fn save(&mut self, preferences: AppearancePreferences, cx: &mut Context<Self>) {
        let had_error = self.error.is_some();
        self.error = preferences::update(cx, |settings| settings.appearance = preferences)
            .err()
            .map(|error| format!("Could not save appearance settings: {error}"));

        if had_error != self.error.is_some() && !self.rows.is_empty() {
            self.list.remeasure_items(0..1);
        }

        cx.notify();
    }

    fn mode_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let preferences = cx
            .try_global::<preferences::Preferences>()
            .cloned()
            .unwrap_or_default()
            .appearance;

        h_flex().gap_2().children(
            [
                (AppearanceMode::Light, "Light", IconName::Sun),
                (AppearanceMode::Dark, "Dark", IconName::Moon),
                (AppearanceMode::System, "System", IconName::Settings2),
            ]
            .into_iter()
            .map(|(mode, label, icon)| {
                Button::new(label)
                    .debug_selector(move || format!("appearance-{label}"))
                    .outline()
                    .flex_1()
                    .h_12()
                    .icon(icon)
                    .label(label)
                    .when(preferences.mode == mode, |button| button.primary())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut preferences = cx
                            .try_global::<preferences::Preferences>()
                            .cloned()
                            .unwrap_or_default()
                            .appearance;
                        preferences.mode = mode;

                        this.save(preferences, cx);
                    }))
            }),
        )
    }

    fn theme_cards(&self, indices: &[usize], cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let preferences = cx
            .try_global::<preferences::Preferences>()
            .cloned()
            .unwrap_or_default()
            .appearance;

        h_flex().gap_3().children(indices.iter().map(|&index| {
            let preview = &self.previews[index];
            let dark = preview.dark;
            let name = preview.name.clone();

            let selected = if dark {
                &preferences.dark_theme
            } else {
                &preferences.light_theme
            };
            let active = selected == name.as_ref();

            gpui_kit::base::Button::new(name.clone())
                .track_focus(&self.theme_focus[index])
                .on_key_down(cx.listener(move |this, event, window, cx| {
                    this.navigate_themes(index, event, window, cx);
                }))
                .accessibility_label(format!(
                    "{}{}",
                    name,
                    if active { ", selected" } else { "" }
                ))
                .debug_selector({
                    let name = name.clone();
                    move || format!("theme-{name}")
                })
                .border_1()
                .cursor_pointer()
                .bg(cx.theme().background)
                .hover(|style| style.bg(cx.theme().muted))
                .focus_visible(|style| style.border_color(cx.theme().ring))
                .w(px(190.))
                .h_auto()
                .p_2()
                .rounded_lg()
                .border_color(if active {
                    cx.theme().primary
                } else {
                    cx.theme().border
                })
                .child(preview.render(active, cx))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let mut preferences = cx
                        .try_global::<preferences::Preferences>()
                        .cloned()
                        .unwrap_or_default()
                        .appearance;

                    let Some((light, dark)) = request_eagle_theme::theme_pair(&name) else {
                        return;
                    };

                    preferences.light_theme = light.into();
                    preferences.dark_theme = dark.into();

                    this.save(preferences, cx);
                }))
        }))
    }

    fn navigate_themes(
        &mut self,
        index: usize,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = match event.keystroke.key.as_str() {
            "left" => -1,
            "right" => 1,
            "up" => -(self.columns as isize),
            "down" => self.columns as isize,
            "tab" if event.keystroke.modifiers.shift => -1,
            "tab" => 1,
            _ => return,
        };

        let order = self
            .rows
            .iter()
            .enumerate()
            .flat_map(|(row, item)| match item {
                PageRow::Themes(indices) => indices
                    .iter()
                    .map(move |&index| (row, index))
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect::<Vec<_>>();

        let Some(position) = order.iter().position(|&(_, candidate)| candidate == index) else {
            return;
        };

        let Some(target) = position
            .checked_add_signed(delta)
            .and_then(|position| order.get(position))
        else {
            return;
        };

        let viewport = self.list.viewport_bounds();
        if self.list.bounds_for_item(target.0).is_none_or(|bounds| {
            bounds.top() < viewport.top() || bounds.bottom() > viewport.bottom()
        }) {
            self.list.scroll_to(gpui_kit::ListOffset {
                item_ix: target.0,
                offset_in_item: px(0.),
            });
        }

        window.focus(&self.theme_focus[target.1], cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let preferences = cx
            .try_global::<preferences::Preferences>()
            .cloned()
            .unwrap_or_default()
            .appearance;
        let font_size = preferences.interface_font_size;

        v_flex()
            .w_full()
            .max_w(px(880.))
            .gap_6()
            .child(
                div()
                    .text_size(rems(1.625))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Appearance"),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(div().font_weight(FontWeight::MEDIUM).child("Color mode"))
                    .child(self.mode_buttons(cx))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("System follows your device's light or dark appearance. Selecting a theme also selects its matching light or dark variant."),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(div().font_weight(FontWeight::MEDIUM).child("Typography"))
                    .child(
                        h_flex()
                            .justify_between()
                            .flex_wrap()
                            .gap_3()
                            .child(
                                v_flex().gap_1().child("Editor font").child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Choose a font installed on this device."),
                                ),
                            )
                            .child(
                                div().w(px(260.)).child(
                                    Select::new(&self.font)
                                        .accessibility_label("Editor font")
                                        .menu_width(px(300.)),
                                ),
                            ),
                    )
                    .child(
                        v_flex()
                            .p_4()
                            .gap_1()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().muted)
                            .overflow_hidden()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .child(
                                div()
                                    .text_color(cx.theme().primary)
                                    .child("GET /api/requests HTTP/1.1"),
                            )
                            .child("Content-Type: application/json")
                            .child("{ \"message\": \"Hello, Eagle\", \"status\": 200 }"),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .flex_wrap()
                            .gap_3()
                            .child("Interface font size")
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("font-size-decrease")
                                            .outline()
                                            .icon(IconName::Minus)
                                            .disabled(font_size <= 12.)
                                            .accessibility_label("Decrease interface font size")
                                            .tooltip("Decrease interface font size")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let mut preferences = cx
                                                    .try_global::<preferences::Preferences>()
                                                    .cloned()
                                                    .unwrap_or_default()
                                                    .appearance;
                                                preferences.interface_font_size -= 1.;

                                                this.save(preferences, cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .min_w(px(48.))
                                            .text_center()
                                            .child(format!("{font_size:.0} px")),
                                    )
                                    .child(
                                        Button::new("font-size-increase")
                                            .outline()
                                            .icon(IconName::Plus)
                                            .disabled(font_size >= 24.)
                                            .accessibility_label("Increase interface font size")
                                            .tooltip("Increase interface font size")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let mut preferences = cx
                                                    .try_global::<preferences::Preferences>()
                                                    .cloned()
                                                    .unwrap_or_default()
                                                    .appearance;
                                                preferences.interface_font_size += 1.;

                                                this.save(preferences, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("font-size-reset")
                                            .ghost()
                                            .label("Reset")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let mut preferences = cx
                                                    .try_global::<preferences::Preferences>()
                                                    .cloned()
                                                    .unwrap_or_default()
                                                    .appearance;
                                                preferences.interface_font_size = 16.;

                                                this.save(preferences, cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
    }

    fn update_rows(&mut self, width: Pixels, font_size: Pixels) {
        let gap = font_size * 0.75;
        let columns = ((width + gap) / (px(190.) + gap)).floor().max(1.) as usize;

        if self.columns == columns && self.layout_font_size == font_size {
            return;
        }

        self.columns = columns;
        self.layout_font_size = font_size;
        self.rows = vec![PageRow::Header];

        for dark in [false, true] {
            self.rows.push(PageRow::Heading(dark));

            let indices = self
                .previews
                .iter()
                .enumerate()
                .filter_map(|(index, preview)| (preview.dark == dark).then_some(index))
                .collect::<Vec<_>>();

            for row in indices.chunks(columns) {
                self.rows.push(PageRow::Themes(row.to_vec()));
            }
        }

        self.list.reset(self.rows.len());
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(index) else {
            return div().into_any_element();
        };

        let preferences = cx
            .try_global::<preferences::Preferences>()
            .cloned()
            .unwrap_or_default()
            .appearance;

        let content = match row {
            PageRow::Header => self.header(cx).into_any_element(),
            PageRow::Heading(dark) => v_flex()
                .gap_3()
                .pt_3()
                .child(div().font_weight(FontWeight::MEDIUM).child(if *dark {
                    "Dark themes"
                } else {
                    "Light themes"
                }))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if *dark {
                            format!("Used in dark mode: {}", preferences.dark_theme)
                        } else {
                            format!("Used in light mode: {}", preferences.light_theme)
                        }),
                )
                .into_any_element(),
            PageRow::Themes(indices) => self.theme_cards(indices, cx).into_any_element(),
        };

        v_flex()
            .w_full()
            .items_center()
            .child(div().w_full().max_w(px(880.)).pb_3().child(content))
            .into_any_element()
    }
}

impl Render for AppearanceSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = window.viewport_size().width < px(680.);
        let padding = if narrow { px(20.) } else { px(48.) };
        let font_size = cx.theme().font_size;
        let weak = cx.entity().downgrade();
        let row_view = cx.entity().downgrade();

        div()
            .size_full()
            .relative()
            .on_prepaint(move |bounds, _, cx| {
                let _ = weak.update(cx, |this, _| {
                    this.update_rows((bounds.size.width - padding * 2.).min(px(880.)), font_size);
                });
            })
            .child(
                gpui_kit::list(self.list.clone(), move |index, _, cx| {
                    row_view
                        .update(cx, |this, cx| this.row(index, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .size_full()
                .px(padding)
                .py(if narrow { px(24.) } else { px(48.) }),
            )
            .child(scroll::Scrollbar::vertical(&self.list))
    }
}
