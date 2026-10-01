use std::{
    rc::Rc,
    time::{Duration, SystemTime},
};

use gpui_kit::base::SelectableText;
use gpui_kit::component::{
    button::*,
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use preferences::Preferences;
use request::Cookie;

use crate::cookies::Cookies;

/// A row of the cookie table: a domain's heading, or one of its cookies.
enum Row {
    Domain { domain: SharedString, count: usize },
    Cookie(Cookie),
}

/// Lists the cookies in the jar that requests share, by domain, and deletes
/// them.
pub struct CookiePage {
    cookies: Vec<Cookie>,
    /// The cookies that match the filter, under their domains. Only the rows
    /// in view are drawn.
    rows: Rc<[Row]>,
    list: ListState,
    search: Option<Entity<InputState>>,
    query: String,
    /// Reloads the cookies when the next one expires.
    expiry: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl CookiePage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            // Also redraws when saving the cookies fails.
            cx.observe_global::<Cookies>(|this, cx| {
                this.reload(cx);
                cx.notify();
            }),
            // The notice that the jar is off follows the setting.
            cx.observe_global::<Preferences>(|_, cx| cx.notify()),
        ];

        let mut page = Self {
            cookies: Vec::new(),
            rows: Rc::new([]),
            // Rows below the view are measured ahead, so scrolling can reach
            // them; without it, scrolling stops at the first screen.
            list: ListState::new(0, ListAlignment::Top, px(400.)),
            search: None,
            query: String::new(),
            expiry: None,
            _subscriptions: subscriptions,
        };
        page.reload(cx);

        page
    }

    /// Opening the tab again shows the cookies that have not expired since.
    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reload(cx);

        if self.search.is_some() {
            return;
        }

        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter by domain or name"));
        let subscription = cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.query = input.read(cx).value().trim().to_lowercase();
                this.filter();
                cx.notify();
            }
        });

        self.search = Some(search);
        self._subscriptions.push(subscription);
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let cookies = Cookies::jar(cx).cookies();
        self.watch_expiry(&cookies, cx);

        // Keep the scroll position unless the cookies changed.
        if cookies != self.cookies {
            self.cookies = cookies;
            self.filter();
            cx.notify();
        }
    }

    /// Expired cookies leave the jar without changing it, so check again
    /// once the next one has expired.
    fn watch_expiry(&mut self, cookies: &[Cookie], cx: &mut Context<Self>) {
        let now = SystemTime::now();
        let next = cookies
            .iter()
            .filter_map(|cookie| cookie.expires?.duration_since(now).ok())
            .min();

        self.expiry = next.map(|wait| {
            cx.spawn(async move |this, cx| {
                // Listed expiry times are whole seconds; wait past the
                // fraction. Look again daily rather than sleeping for years.
                let wait = (wait + Duration::from_secs(1)).min(Duration::from_secs(24 * 60 * 60));
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |this, cx| this.reload(cx));
            })
        });
    }

    fn filter(&mut self) {
        let matching = self
            .cookies
            .iter()
            .filter(|cookie| {
                self.query.is_empty()
                    || cookie.domain.to_lowercase().contains(&self.query)
                    || cookie.name.to_lowercase().contains(&self.query)
            })
            .collect::<Vec<_>>();

        let mut rows = Vec::new();
        for group in matching.chunk_by(|a, b| a.domain == b.domain) {
            rows.push(Row::Domain {
                domain: group[0].domain.clone().into(),
                count: group.len(),
            });
            rows.extend(group.iter().map(|cookie| Row::Cookie((*cookie).clone())));
        }

        self.list.reset(rows.len());
        self.rows = rows.into();
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

    fn table(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let rows = self.rows.clone();
        let page = cx.entity().downgrade();

        v_flex()
            .id("cookie-table")
            .debug_selector(|| "cookie-table".into())
            .flex_1()
            .min_h_0()
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
                    .flex_none()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(name_column().child("Name"))
                    .child(value_column().child("Value"))
                    .child(path_column().child("Path"))
                    .child(expires_column().child("Expires"))
                    .child(attributes_column().child("Attributes"))
                    .child(div().w_8().flex_none()),
            )
            .child(
                list(self.list.clone(), move |index, _, cx| match &rows[index] {
                    Row::Domain { domain, count } => {
                        domain_row(index, domain, *count, &page, cx).into_any_element()
                    }
                    Row::Cookie(cookie) => cookie_row(index, cookie, &page, cx).into_any_element(),
                })
                .flex_1()
                .min_h_0(),
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

fn domain_row(
    index: usize,
    domain: &SharedString,
    count: usize,
    page: &WeakEntity<CookiePage>,
    cx: &App,
) -> impl IntoElement + use<> {
    let removed = domain.clone();
    let page = page.clone();

    h_flex()
        .debug_selector(move || format!("cookie-domain-{index}"))
        .w_full()
        .h_8()
        .px_2()
        .gap_2()
        .when(index > 0, |row| row.border_t_1())
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
                .child(match count {
                    1 => "1 cookie".to_owned(),
                    count => format!("{count} cookies"),
                }),
        )
        .child(div().flex_1())
        .child(
            Button::new(("delete-domain", index))
                .ghost()
                .xsmall()
                .icon(Icon::default().path("icons/trash.svg"))
                .accessibility_label(format!("Delete cookies for {domain}"))
                .tooltip(format!("Delete cookies for {domain}"))
                .on_click(move |_, _, cx| {
                    let _ = page.update(cx, |page, cx| {
                        page.remove(|cookie| cookie.domain == removed.as_ref(), cx)
                    });
                }),
        )
}

fn cookie_row(
    index: usize,
    cookie: &Cookie,
    page: &WeakEntity<CookiePage>,
    cx: &App,
) -> impl IntoElement + use<> {
    let same_site = cookie
        .same_site
        .as_ref()
        .map(|same_site| format!("SameSite={same_site}"));
    let attributes = [
        (!cookie.host_only).then(|| "Subdomains".to_owned()),
        cookie.secure.then(|| "Secure".to_owned()),
        cookie.http_only.then(|| "HttpOnly".to_owned()),
        same_site,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ");
    let selectable = |column: &'static str, text: SharedString, order: usize| {
        SelectableText::new((column, index), text).document_order((index * 2 + order) as u64)
    };
    let removed = cookie.clone();
    let page = page.clone();

    h_flex()
        .id(("cookie", index))
        .debug_selector(move || format!("cookie-row-{index}"))
        .w_full()
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
                    .on_click(move |_, _, cx| {
                        let _ = page
                            .update(cx, |page, cx| page.remove(|cookie| *cookie == removed, cx));
                    }),
            ),
        )
}

/// Columns share the table's width in proportion, so each keeps room at
/// every interface size.
fn column(share: f32) -> Div {
    div()
        .flex_basis(relative(0.))
        .flex_grow(share)
        .min_w_0()
        .px_2()
        .py_2()
}

fn name_column() -> Div {
    column(3.)
}

fn value_column() -> Div {
    column(5.)
}

fn path_column() -> Div {
    column(2.)
}

fn expires_column() -> Div {
    column(2.5)
}

fn attributes_column() -> Div {
    column(2.5)
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

        v_flex()
            .id("cookie-page")
            .debug_selector(|| "cookie-page".into())
            .size_full()
            .min_w_0()
            .px_4()
            .pb_4()
            .gap_2()
            .text_sm()
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
            .child(if self.rows.is_empty() {
                self.empty_state().into_any_element()
            } else {
                self.table(cx).into_any_element()
            })
    }
}
