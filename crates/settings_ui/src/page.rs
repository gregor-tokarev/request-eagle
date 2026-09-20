use super::appearance;
use gpui_kit::component::{
    button::*,
    resizable::{h_resizable, resizable_panel},
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::general::GeneralSettings;
use super::keybindings::KeybindingsPage;
use crate::actions::CloseSettings;
use updater::Updater;

pub enum SettingsEvent {
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    General,
    Appearance,
    Keybindings,
}

impl SettingsPage {
    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Keybindings => "Keybindings",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::General => Icon::new(IconName::Settings2),
            Self::Appearance => Icon::new(IconName::Palette),
            Self::Keybindings => Icon::default().path("icons/keyboard.svg"),
        }
    }
}

pub struct Settings {
    page: SettingsPage,
    general: Entity<GeneralSettings>,
    appearance: Entity<appearance::AppearanceSettings>,
    keybindings: Entity<KeybindingsPage>,
    focus_handle: FocusHandle,
}

impl EventEmitter<SettingsEvent> for Settings {}

impl Settings {
    pub fn new(updater: Entity<Updater>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            page: SettingsPage::General,
            general: cx.new(|cx| GeneralSettings::new(updater, window, cx)),
            appearance: cx.new(|cx| appearance::AppearanceSettings::new(window, cx)),
            keybindings: cx.new(|cx| KeybindingsPage::new(window, cx)),
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.page {
            SettingsPage::General | SettingsPage::Appearance => {
                window.focus(&self.focus_handle, cx)
            }
            SettingsPage::Keybindings => self
                .keybindings
                .update(cx, |page, cx| page.focus_search(window, cx)),
        }
    }

    pub fn select_page(&mut self, page: SettingsPage, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        self.focus(window, cx);

        cx.notify();
    }

    fn page_buttons(&self, cx: &mut Context<Self>) -> Vec<Button> {
        [
            SettingsPage::General,
            SettingsPage::Appearance,
            SettingsPage::Keybindings,
        ]
        .into_iter()
        .map(|page| {
            Button::new(page.title())
                .ghost()
                .h(px(32.))
                .px_2()
                .rounded_md()
                .text_color(cx.theme().muted_foreground)
                .selected(self.page == page)
                .when(self.page == page, |this| {
                    this.bg(cx.theme().muted).text_color(cx.theme().foreground)
                })
                .icon(page.icon().size_4())
                .accessibility_label(page.title())
                .child(
                    div()
                        .min_w_0()
                        .text_ellipsis()
                        .text_size(rems(0.8125))
                        .line_height(relative(1.))
                        .child(page.title()),
                )
                .child(div().flex_1())
                .on_click(
                    cx.listener(move |this, _, window, cx| this.select_page(page, window, cx)),
                )
        })
        .collect()
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .w_full()
            .h_full()
            .bg(cx.theme().background)
            .pt(px(48.))
            .px_2()
            .pb_2()
            .children(
                self.page_buttons(cx)
                    .into_iter()
                    .map(|button| button.w_full()),
            )
            .child(div().flex_1())
            .child(
                Button::new("settings-back")
                    .ghost()
                    .h(px(32.))
                    .w_full()
                    .px_2()
                    .rounded_md()
                    .text_color(cx.theme().muted_foreground)
                    .icon(Icon::new(IconName::ArrowLeft).size_4())
                    .accessibility_label("Back to workspace")
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(rems(0.8125))
                            .line_height(relative(1.))
                            .child("Back to workspace"),
                    )
                    .child(div().flex_1())
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close))),
            )
    }
}

impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = window.viewport_size().width < px(680.);

        let content = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .when(narrow, |this| {
                this.child(
                    h_flex()
                        .px_3()
                        .py_2()
                        .gap_2()
                        .flex_wrap()
                        .children(self.page_buttons(cx))
                        .child(div().flex_1())
                        .child(
                            Button::new("close-settings")
                                .ghost()
                                .small()
                                .icon(IconName::Close)
                                .tooltip_with_action(
                                    "Close settings",
                                    &CloseSettings,
                                    Some("Settings"),
                                )
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close))),
                        ),
                )
            })
            .child(if self.page == SettingsPage::Appearance {
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.appearance.clone())
                    .into_any_element()
            } else if self.page == SettingsPage::Keybindings {
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .justify_center()
                    .px(if narrow { px(20.) } else { px(48.) })
                    .py(if narrow { px(24.) } else { px(48.) })
                    .child(self.keybindings.clone())
                    .into_any_element()
            } else {
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .items_center()
                            .px(if narrow { px(20.) } else { px(48.) })
                            .py(if narrow { px(24.) } else { px(48.) })
                            .child(self.general.clone()),
                    )
                    .into_any_element()
            });

        h_flex()
            .id("settings")
            .debug_selector(|| "settings".into())
            .key_context("Settings")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &CloseSettings, _, cx| cx.emit(SettingsEvent::Close)))
            .relative()
            .size_full()
            .overflow_hidden()
            .text_sm()
            .text_color(cx.theme().foreground)
            .bg(cx.theme().background)
            .child(if narrow {
                content.into_any_element()
            } else {
                h_resizable("settings-panels")
                    .child(
                        resizable_panel()
                            .size(px(232.))
                            .size_range(
                                px(220.)..px(400.).min(window.viewport_size().width - px(448.)),
                            )
                            .flex_none()
                            .child(self.render_sidebar(cx)),
                    )
                    .child(
                        resizable_panel()
                            .size_range(px(448.)..Pixels::MAX)
                            .child(content),
                    )
                    .into_any_element()
            })
    }
}
