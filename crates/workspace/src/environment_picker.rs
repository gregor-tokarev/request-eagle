use gpui_kit::component::{
    button::*,
    searchable_list::{SearchableListItem, SearchableVec},
    select::{Select, SelectEvent, SelectState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use tab_ui::Environments;

pub(crate) enum EnvironmentPickerEvent {
    Open(SharedString),
    Create,
}

#[derive(Clone, PartialEq)]
enum Choice {
    /// A global environment, or none.
    Environment(Option<SharedString>),
    Create,
}

#[derive(Clone)]
struct EnvironmentChoice {
    title: SharedString,
    choice: Choice,
}

impl SearchableListItem for EnvironmentChoice {
    type Value = Choice;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex()
            .gap_2()
            .when(self.choice == Choice::Create, |this| {
                this.child(Icon::new(IconName::Plus).size_3p5())
            })
            .child(self.title.clone())
    }

    fn value(&self) -> &Self::Value {
        &self.choice
    }

    /// Creating stays available whatever the search.
    fn matches(&self, query: &str) -> bool {
        self.choice == Choice::Create || self.title.to_lowercase().contains(&query.to_lowercase())
    }
}

type ChoiceState = SelectState<SearchableVec<EnvironmentChoice>>;

/// Chooses the global environment requests use, next to the tab strip.
/// Creating an environment is the menu's last item: choosing an item is what
/// closes the menu, and a footer button would leave it open.
pub(crate) struct EnvironmentPicker {
    environments: Entity<Environments>,
    state: Entity<ChoiceState>,
    /// The names in the menu, so unrelated changes leave an open menu alone.
    names: Option<Vec<SharedString>>,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<EnvironmentPickerEvent> for EnvironmentPicker {}

impl EnvironmentPicker {
    pub(crate) fn new(
        environments: Entity<Environments>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(true)
        });

        let environments_subscription =
            cx.observe_in(&environments, window, |this, _, window, cx| {
                this.refresh(window, cx);
            });
        let selection_subscription = cx.subscribe_in(
            &state,
            window,
            |this, state, event: &SelectEvent<SearchableVec<EnvironmentChoice>>, window, cx| {
                match event {
                    SelectEvent::Confirm(Some(Choice::Create)) => {
                        // Keep showing the active environment, not the command.
                        let active =
                            Choice::Environment(this.environments.read(cx).active().cloned());
                        state.update(cx, |state, cx| {
                            state.set_selected_value(&active, window, cx)
                        });
                        cx.emit(EnvironmentPickerEvent::Create);
                    }
                    SelectEvent::Confirm(Some(Choice::Environment(name))) => {
                        let name = name.clone();
                        this.environments
                            .update(cx, |environments, cx| environments.set_active(name, cx));
                    }
                    SelectEvent::Confirm(None) => {}
                }
            },
        );

        let mut picker = Self {
            environments,
            state,
            names: None,
            _subscriptions: [environments_subscription, selection_subscription],
        };
        picker.refresh(window, cx);

        picker
    }

    /// Sync the menu with the stored environments. Replacing the items or the
    /// selection while the menu handles its own choice leaves it stuck open,
    /// so only change what differs.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let environments = self.environments.read(cx);
        let active = Choice::Environment(environments.active().cloned());
        let names = environments.names().to_vec();

        if self.names.as_ref() != Some(&names) {
            let choices = std::iter::once(EnvironmentChoice {
                title: "No Environment".into(),
                choice: Choice::Environment(None),
            })
            .chain(names.iter().map(|name| EnvironmentChoice {
                title: name.clone(),
                choice: Choice::Environment(Some(name.clone())),
            }))
            .chain(std::iter::once(EnvironmentChoice {
                title: "Create new environment".into(),
                choice: Choice::Create,
            }))
            .collect::<Vec<_>>();

            self.state.update(cx, |state, cx| {
                state.set_items(SearchableVec::new(choices), window, cx);
                state.set_selected_value(&active, window, cx);
            });
            self.names = Some(names);
        } else if self.state.read(cx).selected_value() != Some(&active) {
            self.state.update(cx, |state, cx| {
                state.set_selected_value(&active, window, cx)
            });
        }

        cx.notify();
    }
}

impl Render for EnvironmentPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.environments.read(cx).active().cloned();

        h_flex()
            .flex_none()
            .gap_1()
            .child(
                div()
                    .debug_selector(|| "environment-picker".into())
                    .w(rems(11.))
                    .child(
                        Select::new(&self.state)
                            .small()
                            .accessibility_label("Active environment")
                            .menu_width(rems(18.))
                            .search_placeholder("Search"),
                    ),
            )
            .child(
                Button::new("open-active-environment")
                    .debug_selector(|| "open-active-environment".into())
                    .ghost()
                    .small()
                    .icon(IconName::Eye)
                    .disabled(active.is_none())
                    .accessibility_label("Open active environment")
                    .tooltip("Open active environment")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if let Some(name) = active.clone() {
                            cx.emit(EnvironmentPickerEvent::Open(name));
                        }
                    })),
            )
    }
}
