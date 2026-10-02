use gpui_kit::component::{
    button::*,
    resizable::{h_resizable, resizable_panel},
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use updater::Updater;

use crate::{
    actions::CloseSettings, appearance::AppearanceSettings, certificates::CertificateSettings,
    general::GeneralSettings, keybindings::KeybindingsPage, layout, proxy::ProxySettings,
};

const SIDEBAR_MIN: Rems = rems(14.);
const CONTENT_MIN: Rems = rems(28.);

pub enum SettingsEvent {
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    General,
    Proxy,
    Certificates,
    Appearance,
    Keybindings,
}

impl SettingsPage {
    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Proxy => "Proxy",
            Self::Certificates => "Certificates",
            Self::Appearance => "Appearance",
            Self::Keybindings => "Keybindings",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::General => Icon::new(IconName::Settings2),
            Self::Proxy => Icon::default().path("icons/network.svg"),
            Self::Certificates => Icon::default().path("icons/shield-check.svg"),
            Self::Appearance => Icon::new(IconName::Palette),
            Self::Keybindings => Icon::default().path("icons/keyboard.svg"),
        }
    }
}

pub struct Settings {
    page: SettingsPage,
    general: Entity<GeneralSettings>,
    proxy: Entity<ProxySettings>,
    certificates: Entity<CertificateSettings>,
    appearance: Entity<AppearanceSettings>,
    keybindings: Entity<KeybindingsPage>,
    focus_handle: FocusHandle,
}

impl EventEmitter<SettingsEvent> for Settings {}

impl Settings {
    pub fn new(updater: Entity<Updater>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            page: SettingsPage::General,
            general: cx.new(|cx| GeneralSettings::new(updater, window, cx)),
            proxy: cx.new(|cx| ProxySettings::new(window, cx)),
            certificates: cx.new(|cx| CertificateSettings::new(window, cx)),
            appearance: cx.new(|cx| AppearanceSettings::new(window, cx)),
            keybindings: cx.new(|cx| KeybindingsPage::new(window, cx)),
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.page {
            SettingsPage::General
            | SettingsPage::Proxy
            | SettingsPage::Certificates
            | SettingsPage::Appearance => window.focus(&self.focus_handle, cx),
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
            SettingsPage::Proxy,
            SettingsPage::Certificates,
            SettingsPage::Appearance,
            SettingsPage::Keybindings,
        ]
        .into_iter()
        .map(|page| {
            Button::new(page.title())
                .ghost()
                .px_2()
                .rounded(cx.theme().radius_tokens().md)
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
                        .text_sm()
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
            .pt_8()
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
                    .w_full()
                    .px_2()
                    .rounded(cx.theme().radius_tokens().md)
                    .text_color(cx.theme().muted_foreground)
                    .icon(Icon::new(IconName::ArrowLeft).size_4())
                    .accessibility_label("Back to workspace")
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_sm()
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
        let narrow = layout::is_narrow(window);

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
            .child(match self.page {
                SettingsPage::Appearance => div()
                    .flex_1()
                    .min_h_0()
                    .child(self.appearance.clone())
                    .into_any_element(),
                SettingsPage::Keybindings => h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .justify_center()
                    .px(layout::page_inset(window))
                    .py(layout::page_inset(window))
                    .child(self.keybindings.clone())
                    .into_any_element(),
                SettingsPage::General | SettingsPage::Proxy | SettingsPage::Certificates => div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .items_center()
                            .px(layout::page_inset(window))
                            .py(layout::page_inset(window))
                            .child(match self.page {
                                SettingsPage::Proxy => self.proxy.clone().into_any_element(),
                                SettingsPage::Certificates => {
                                    self.certificates.clone().into_any_element()
                                }
                                _ => self.general.clone().into_any_element(),
                            }),
                    )
                    .into_any_element(),
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
                            .size(rems(15.).to_pixels(window.rem_size()))
                            .size_range(
                                SIDEBAR_MIN.to_pixels(window.rem_size())
                                    ..rems(25.).to_pixels(window.rem_size()).min(
                                        window.viewport_size().width
                                            - CONTENT_MIN.to_pixels(window.rem_size()),
                                    ),
                            )
                            .flex_none()
                            .child(self.render_sidebar(cx)),
                    )
                    .child(
                        resizable_panel()
                            .size_range(CONTENT_MIN.to_pixels(window.rem_size())..Pixels::MAX)
                            .child(content),
                    )
                    .into_any_element()
            })
    }
}
