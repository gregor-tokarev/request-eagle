use std::rc::Rc;

use gpui_kit::{prelude::*, *};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TabBadgeTone {
    Success,
    Warning,
    Info,
    Danger,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TabBadge {
    pub label: &'static str,
    pub tone: TabBadgeTone,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct TabState {
    pub badge: Option<TabBadge>,
    pub dirty: bool,
}

/// A workspace page, independent of its document type or request protocol.
pub trait TabPage: Render {
    const CACHE: bool = true;

    fn tab_state(&self) -> TabState {
        TabState::default()
    }

    fn prepare(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn send(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
}

/// Keeps a typed page's behavior after its renderable view has been erased.
#[derive(Clone)]
pub struct TabView(Rc<dyn PageHandle>);

impl TabView {
    pub fn new<T: TabPage>(page: Entity<T>) -> Self {
        Self(Rc::new(page))
    }

    pub fn view(&self) -> AnyView {
        self.0.view()
    }

    pub fn state(&self, cx: &App) -> TabState {
        self.0.state(cx)
    }

    pub fn prepare(&self, window: &mut Window, cx: &mut App) {
        self.0.prepare(window, cx);
    }

    pub fn send(&self, window: &mut Window, cx: &mut App) {
        self.0.send(window, cx);
    }

    pub fn render(&self) -> AnyElement {
        self.0.render()
    }
}

trait PageHandle {
    fn view(&self) -> AnyView;
    fn state(&self, cx: &App) -> TabState;
    fn prepare(&self, window: &mut Window, cx: &mut App);
    fn send(&self, window: &mut Window, cx: &mut App);
    fn render(&self) -> AnyElement;
}

impl<T: TabPage> PageHandle for Entity<T> {
    fn view(&self) -> AnyView {
        self.clone().into()
    }

    fn state(&self, cx: &App) -> TabState {
        self.read(cx).tab_state()
    }

    fn prepare(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |page, cx| page.prepare(window, cx));
    }

    fn send(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |page, cx| page.send(window, cx));
    }

    fn render(&self) -> AnyElement {
        if T::CACHE {
            self.clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element()
        } else {
            self.clone().into_any_element()
        }
    }
}
