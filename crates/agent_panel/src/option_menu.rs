use gpui_kit::base::actions::Confirm;
use gpui_kit::component::{
    searchable_list::SearchableListItem,
    select::{SearchableVec, SelectEvent, SelectState},
};
use gpui_kit::*;

use crate::panel::AgentPanel;

/// A choice in one of the agent's option menus.
#[derive(Clone, PartialEq)]
pub(crate) struct OptionChoice {
    pub(crate) value: String,
    /// The name in the menu.
    pub(crate) name: SharedString,
    /// A shorter name for the menu's button, such as a model's without its
    /// provider.
    pub(crate) short_name: Option<SharedString>,
}

impl SearchableListItem for OptionChoice {
    type Value = String;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn display_title(&self) -> Option<AnyElement> {
        self.short_name.clone().map(IntoElement::into_any_element)
    }

    fn value(&self) -> &String {
        &self.value
    }
}

pub(crate) type OptionChoices = SearchableVec<OptionChoice>;

/// One of the agent's options as a menu in the message box. Typing filters
/// its choices, the arrow keys move through them, and a long list scrolls.
pub(crate) struct OptionMenu {
    pub(crate) state: Entity<SelectState<OptionChoices>>,
    /// The choices the menu has, so they are only replaced when they change:
    /// replacing them while the menu handles a choice leaves it stuck open.
    choices: Vec<OptionChoice>,
    _subscription: Subscription,
}

impl OptionMenu {
    /// A menu that sets the option `id` to the choice made in it.
    pub(crate) fn new(id: String, window: &mut Window, cx: &mut Context<AgentPanel>) -> Self {
        let state = cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(true)
        });
        let subscription = cx.subscribe_in(
            &state,
            window,
            move |panel, _, event: &SelectEvent<OptionChoices>, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    panel.set_option(id.clone(), value.clone(), cx);
                    // Back to the message, as after any other menu.
                    panel.focus_composer(window, cx);
                }
            },
        );

        Self {
            state,
            choices: Vec::new(),
            _subscription: subscription,
        }
    }

    /// Open the menu the way Enter opens it on the focused select.
    pub(crate) fn open(&self, window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, cx| state.focus(window, cx));
        window.dispatch_action(Confirm { secondary: false }.boxed_clone(), cx);
    }

    /// Show `choices`, with `current` chosen when it is among them, changing
    /// only what differs.
    pub(crate) fn show(
        &mut self,
        choices: Vec<OptionChoice>,
        current: &String,
        window: &mut Window,
        cx: &mut App,
    ) {
        let selected = choices
            .iter()
            .any(|choice| choice.value == *current)
            .then_some(current);
        let select = |state: &mut SelectState<OptionChoices>,
                      window: &mut Window,
                      cx: &mut Context<SelectState<OptionChoices>>| {
            match selected {
                Some(value) => state.set_selected_value(value, window, cx),
                None => state.set_selected_index(None, window, cx),
            }
        };

        if self.choices != choices {
            self.state.update(cx, |state, cx| {
                state.set_items(SearchableVec::new(choices.clone()), window, cx);
                select(state, window, cx);
            });
            self.choices = choices;
        } else if self.state.read(cx).selected_value() != selected {
            self.state.update(cx, |state, cx| select(state, window, cx));
        }
    }
}
