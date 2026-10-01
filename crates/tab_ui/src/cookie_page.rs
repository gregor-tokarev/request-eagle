use std::time::SystemTime;

use gpui_kit::base::SelectableText;
use gpui_kit::component::{
    button::*,
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use preferences::Preferences;
use request::Cookie;

use crate::cookies::Cookies;

/// Lists the cookies in the jar that requests share, by domain, and deletes
/// them.
pub struct CookiePage {
    cookies: Vec<Cookie>,
    search: Option<Entity<InputState>>,
    query: String,
    _subscriptions: Vec<Subscription>,
}

impl CookiePage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe_global::<Cookies>(|this, cx| this.reload(cx)),
            // The notice that the jar is off follows the setting.
            cx.observe_global::<Preferences>(|_, cx| cx.notify()),
        ];

        Self {
            cookies: Cookies::jar(cx).cookies(),
            search: None,
            query: String::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_some() {
            return;
        }

        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter by domain or name"));
        let subscription = cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.query = input.read(cx).value().trim().to_lowercase();
                cx.notify();
            }
        });

        self.search = Some(search);
        self._subscriptions.push(subscription);
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.cookies = Cookies::jar(cx).cookies();
        cx.notify();
    }

    fn remove(&mut self, matches: impl Fn(&Cookie) -> bool, cx: &mut Context<Self>) {
        let jar = Cookies::jar(cx);

        for cookie in self.cookies.iter().filter(|cookie| matches(cookie)) {
            jar.remove(cookie);
        }

        Cookies::changed(cx);
        self.reload(cx);
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        Cookies::jar(cx).clear();
        Cookies::changed(cx);
        self.reload(cx);
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .flex_none()
            .h_10()
            .gap_2()
            .child(
                Icon::default()
                    .path("icons/cookie.svg")
                    .size_4()
                    .flex_none()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(div().font_weight(FontWeight::MEDIUM).child("Cookies"))
            .child(div().flex_1())
            .child(
                div()
                    .debug_selector(|| "cookie-search".into())
                    .w(rems(20.))
                    .min_w_0()
                    .when_some(self.search.as_ref(), |this, search| {
                        this.child(
                            Input::new(search)
                                .small()
                                .prefix(IconName::Search)
                                .cleanable(true)
                                .aria_label("Filter cookies"),
                        )
                    }),
            )
            .child(
                Button::new("delete-all-cookies")
                    .debug_selector(|| "delete-all-cookies".into())
                    .small()
                    .ghost()
                    .icon(Icon::default().path("icons/trash.svg"))
                    .label("Delete all")
                    .disabled(self.cookies.is_empty())
                    .tooltip("Delete every cookie in the jar")
                    .on_click(cx.listener(|this, _, _, cx| this.clear(cx))),
            )
    }

    fn table(&self, cookies: &[&Cookie], cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut table = v_flex()
            .id("cookie-table")
            .debug_selector(|| "cookie-table".into())
            .flex_none()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_tokens().md)
            .overflow_hidden()
            // Selectable text updates without redrawing this cached page.
            // Paint the changing highlight while dragging.
            .on_mouse_move(cx.listener(|_, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    cx.notify();
                }
            }))
            .child(
                h_flex()
                    .h_8()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(name_column().child("Name"))
                    .child(value_column().child("Value"))
                    .child(path_column().child("Path"))
                    .child(expires_column().child("Expires"))
                    .child(attributes_column().child("Attributes"))
                    .child(div().w_8().flex_none()),
            );

        let mut index = 0;
        for group in cookies.chunk_by(|a, b| a.domain == b.domain) {
            let domain = group[0].domain.clone();

            table = table.child(
                h_flex()
                    .debug_selector({
                        let domain = domain.clone();
                        move || format!("cookie-domain-{domain}")
                    })
                    .h_8()
                    .px_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().muted)
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .font_weight(FontWeight::MEDIUM)
                            .child(domain.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(match group.len() {
                                1 => "1 cookie".to_owned(),
                                count => format!("{count} cookies"),
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new(SharedString::from(format!("delete-domain-{domain}")))
                            .ghost()
                            .xsmall()
                            .icon(Icon::default().path("icons/trash.svg"))
                            .accessibility_label(format!("Delete cookies for {domain}"))
                            .tooltip(format!("Delete cookies for {domain}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove(|cookie| cookie.domain == domain, cx)
                            })),
                    ),
            );

            for cookie in group {
                table = table.child(self.row(index, cookie, cx));
                index += 1;
            }
        }

        table
    }

    fn row(
        &self,
        index: usize,
        cookie: &Cookie,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let attributes = [
            (!cookie.host_only).then_some("Subdomains"),
            cookie.secure.then_some("Secure"),
            cookie.http_only.then_some("HttpOnly"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
        let selectable = |column: &'static str, text: SharedString, order: usize| {
            SelectableText::new((column, index), text).document_order((index * 2 + order) as u64)
        };
        let removed = cookie.clone();

        h_flex()
            .id(("cookie", index))
            .debug_selector(move || format!("cookie-row-{index}"))
            .items_start()
            .min_h_8()
            .border_t_1()
            .border_color(cx.theme().border)
            .hover(|row| row.bg(cx.theme().table_hover))
            .child(
                name_column()
                    .font_family(cx.theme().mono_font_family.clone())
                    .cursor_text()
                    .child(selectable("cookie-name", cookie.name.clone().into(), 0)),
            )
            .child(
                value_column()
                    .font_family(cx.theme().mono_font_family.clone())
                    .cursor_text()
                    .child(selectable("cookie-value", cookie.value.clone().into(), 1)),
            )
            .child(path_column().child(cookie.path.clone()))
            .child(expires_column().child(expires(cookie.expires)))
            .child(
                attributes_column()
                    .text_color(cx.theme().muted_foreground)
                    .child(attributes),
            )
            .child(
                h_flex().w_8().flex_none().h_8().justify_center().child(
                    Button::new(("delete-cookie", index))
                        .debug_selector(move || format!("delete-cookie-{index}"))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .accessibility_label(format!("Delete cookie {}", cookie.name))
                        .tooltip("Delete cookie")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove(|cookie| *cookie == removed, cx)
                        })),
                ),
            )
    }

    fn empty_state(&self) -> impl IntoElement + use<> {
        let (title, description) = if self.cookies.is_empty() {
            (
                "No cookies yet".to_owned(),
                "When a response sets a cookie, it appears here.",
            )
        } else {
            (
                format!("No cookies match \u{201c}{}\u{201d}", self.query),
                "Search by a cookie's domain or name.",
            )
        };

        div().flex().flex_1().min_h(rems(16.)).child(
            Empty::new().header(
                EmptyHeader::new()
                    .media(
                        EmptyMedia::new()
                            .with_variant(EmptyMediaVariant::Icon)
                            .child(Icon::default().path("icons/cookie.svg")),
                    )
                    .title(EmptyTitle::new().child(title))
                    .description(EmptyDescription::new().child(description)),
            ),
        )
    }
}

fn cell() -> Div {
    div().min_w_0().px_2().py_2()
}

fn name_column() -> Div {
    cell().w(rems(12.)).flex_none()
}

fn value_column() -> Div {
    cell().flex_1()
}

fn path_column() -> Div {
    cell().w(rems(8.)).flex_none()
}

fn expires_column() -> Div {
    cell().w(rems(9.)).flex_none()
}

fn attributes_column() -> Div {
    cell().w(rems(11.)).flex_none()
}

/// When a cookie expires, in local time. Session cookies have no expiry.
fn expires(expires: Option<SystemTime>) -> String {
    match expires {
        Some(time) => chrono::DateTime::<chrono::Local>::from(time)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => "Session".to_owned(),
    }
}

impl Render for CookiePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = cx
            .try_global::<Preferences>()
            .is_none_or(|preferences| preferences.request.cookie_jar);
        let cookies = self
            .cookies
            .iter()
            .filter(|cookie| {
                self.query.is_empty()
                    || cookie.domain.to_lowercase().contains(&self.query)
                    || cookie.name.to_lowercase().contains(&self.query)
            })
            .collect::<Vec<_>>();

        v_flex()
            .id("cookie-page")
            .debug_selector(|| "cookie-page".into())
            .size_full()
            .min_w_0()
            .px_4()
            .pb_4()
            .gap_2()
            .text_sm()
            .overflow_y_scrollbar()
            .child(self.header(cx))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Cookies that responses set are kept here and sent with later requests \
                         to the same sites. They stay until they expire or you delete them.",
                    ),
            )
            .when(!enabled, |this| {
                this.child(
                    h_flex()
                        .debug_selector(|| "cookie-jar-off".into())
                        .gap_2()
                        .child(div().text_color(cx.theme().warning).child(
                            "The cookie jar is off, so requests do not store or send cookies.",
                        ))
                        .child(
                            Button::new("turn-on-cookie-jar")
                                .debug_selector(|| "turn-on-cookie-jar".into())
                                .small()
                                .outline()
                                .label("Turn on")
                                .on_click(cx.listener(|_, _, _, cx| {
                                    if let Err(error) = preferences::update(cx, |preferences| {
                                        preferences.request.cookie_jar = true
                                    }) {
                                        eprintln!("Could not turn on the cookie jar: {error:#}");
                                    }
                                })),
                        ),
                )
            })
            .when_some(Cookies::error(cx).map(str::to_owned), |this, error| {
                this.child(
                    div()
                        .debug_selector(|| "cookie-error".into())
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(if cookies.is_empty() {
                self.empty_state().into_any_element()
            } else {
                self.table(&cookies, cx).into_any_element()
            })
    }
}
