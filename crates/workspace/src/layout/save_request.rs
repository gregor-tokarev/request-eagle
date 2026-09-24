use collections_panel_ui::{CollectionPanel, SaveDestination};
use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::main_view::{MainView, NewRequestSaveRequested};

pub(crate) fn open(
    main: &Entity<MainView>,
    sidebar: &Entity<CollectionPanel>,
    event: &NewRequestSaveRequested,
    window: &mut Window,
    cx: &mut App,
) {
    let main = main.clone();
    let sidebar = sidebar.clone();
    let request = event.request.clone();
    let tab_id = event.tab_id;
    let dialog = cx.new(|cx| {
        let suggested = if request.path.trim().is_empty() {
            "New Request"
        } else {
            request.path.trim()
        };
        let name = cx.new(|cx| InputState::new(window, cx).default_value(suggested));
        let filter = cx
            .new(|cx| InputState::new(window, cx).placeholder("Search for a collection or folder"));
        let subscriptions = [&name, &filter]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |_, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                })
            })
            .collect();
        let destinations = sidebar.read(cx).save_destinations();
        let selected = None;
        SaveRequestDialog {
            main,
            sidebar,
            tab_id,
            request,
            name,
            filter,
            destinations,
            selected,
            error: None,
            _subscriptions: subscriptions,
        }
    });
    let name = dialog.read(cx).name.clone();
    window.open_dialog(cx, move |modal, _, _| {
        modal
            .title("Save request")
            .w(px(560.))
            .overlay_closable(false)
            .child(dialog.clone())
    });
    name.update(cx, |name, cx| name.focus(window, cx));
}

struct SaveRequestDialog {
    main: Entity<MainView>,
    sidebar: Entity<CollectionPanel>,
    tab_id: u64,
    request: collection::HttpRequest,
    name: Entity<InputState>,
    filter: Entity<InputState>,
    destinations: Vec<SaveDestination>,
    selected: Option<usize>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl SaveRequestDialog {
    fn navigate(
        &mut self,
        destination: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = destination;
        self.filter
            .update(cx, |filter, cx| filter.set_value("", window, cx));
        self.error = None;
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(destination) = self.selected.map(|index| self.destinations[index].clone()) else {
            return;
        };
        let name = self.name.read(cx).value().to_string();
        let result = self.sidebar.update(cx, |sidebar, cx| {
            sidebar.save_new_request(
                &destination.path,
                &name,
                self.request.clone().into(),
                window,
                cx,
            )
        });
        match result {
            Ok(file) => {
                self.main.update(cx, |main, cx| {
                    main.attach_saved_request(self.tab_id, &file, &destination, window, cx)
                });
                window.close_dialog(cx);
            }
            Err(error) => {
                self.error = Some(format!("Could not save request: {error}"));
                cx.notify();
            }
        }
    }
}

impl Render for SaveRequestDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.filter.read(cx).value().to_lowercase();
        let valid_name = !self.name.read(cx).value().trim().is_empty();
        let current = self.selected.map(|index| &self.destinations[index]);
        let visible: Vec<_> = self
            .destinations
            .iter()
            .enumerate()
            .filter(|(_, destination)| {
                let inside = match current {
                    Some(parent) => destination.path.parent() == Some(parent.path.as_path()),
                    None => destination.folders.is_empty(),
                };
                inside
                    && destination
                        .folders
                        .last()
                        .unwrap_or(&destination.collection)
                        .to_lowercase()
                        .contains(&query)
            })
            .collect();
        let mut breadcrumbs = h_flex().gap_1().child(
            Button::new("save-location-root")
                .debug_selector(|| "save-location-root".into())
                .ghost()
                .xsmall()
                .text_color(cx.theme().muted_foreground)
                .label("Collections")
                .on_click(cx.listener(|this, _, window, cx| this.navigate(None, window, cx))),
        );
        if let Some(current) = current {
            for (index, ancestor) in self
                .destinations
                .iter()
                .enumerate()
                .filter(|(_, destination)| current.path.starts_with(&destination.path))
            {
                let label = ancestor
                    .folders
                    .last()
                    .unwrap_or(&ancestor.collection)
                    .clone();
                breadcrumbs = breadcrumbs
                    .child(
                        Icon::new(IconName::ChevronRight)
                            .size_3()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        Button::new(("save-location", index))
                            .debug_selector(move || format!("save-location-{index}"))
                            .ghost()
                            .xsmall()
                            .when(self.selected != Some(index), |button| {
                                button.text_color(cx.theme().muted_foreground)
                            })
                            .label(label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.navigate(Some(index), window, cx)
                            })),
                    );
            }
        }
        v_flex()
            .key_context("Workspace")
            .on_action(
                cx.listener(|this, _: &crate::actions::SaveRequest, window, cx| {
                    this.save(window, cx)
                }),
            )
            .debug_selector(|| "save-request-dialog".into())
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Request name"),
                    )
                    .child(
                        div()
                            .debug_selector(|| "save-request-name".into())
                            .child(Input::new(&self.name).aria_label("Request name")),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Save to"),
                            )
                            .child(breadcrumbs),
                    )
                    .child(
                        v_flex()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_md()
                            .child(
                                div()
                                    .debug_selector(|| "save-location-filter".into())
                                    .child(
                                        Input::new(&self.filter)
                                            .aria_label("Search collections and folders"),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .id("save-destinations")
                                    .h(px(240.))
                                    .overflow_y_scroll()
                                    .when(visible.is_empty(), |list| {
                                        list.child(
                                            div()
                                                .p_3()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(if !query.is_empty() {
                                                    "No matching collections or folders."
                                                } else if self.selected.is_some() {
                                                    "Save here, or choose another location above."
                                                } else {
                                                    "Create a collection to save this request."
                                                }),
                                        )
                                    })
                                    .children(visible.into_iter().map(|(index, destination)| {
                                        let label = destination
                                            .folders
                                            .last()
                                            .unwrap_or(&destination.collection)
                                            .clone();
                                        Button::new(("save-destination", index))
                                            .debug_selector(move || {
                                                format!("save-destination-{index}")
                                            })
                                            .ghost()
                                            .w_full()
                                            .accessibility_label(label.clone())
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .child(Icon::new(IconName::Folder).size_4())
                                                    .child(label)
                                                    .child(div().flex_1())
                                                    .child(
                                                        Icon::new(IconName::ChevronRight)
                                                            .size_3()
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            ),
                                                    ),
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.navigate(Some(index), window, cx)
                                            }))
                                    })),
                            ),
                    ),
            )
            .when_some(self.error.clone(), |view, error| {
                view.child(
                    div()
                        .debug_selector(|| "save-request-error".into())
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("save-new-collection")
                            .debug_selector(|| "save-new-collection".into())
                            .ghost()
                            .small()
                            .text_color(cx.theme().muted_foreground)
                            .child(div().underline().child("New collection"))
                            .accessibility_label("New collection")
                            .on_click(cx.listener(|this, _, window, cx| {
                                match this
                                    .sidebar
                                    .update(cx, |sidebar, cx| sidebar.create_save_collection(cx))
                                {
                                    Ok(path) => {
                                        this.destinations =
                                            this.sidebar.read(cx).save_destinations();
                                        this.selected = this
                                            .destinations
                                            .iter()
                                            .position(|destination| destination.path == path);
                                        this.filter.update(cx, |filter, cx| {
                                            filter.set_value("", window, cx)
                                        });
                                        this.error = None;
                                    }
                                    Err(error) => {
                                        this.error =
                                            Some(format!("Could not create collection: {error}"))
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("confirm-save-request")
                            .debug_selector(|| "confirm-save-request".into())
                            .primary()
                            .small()
                            .label("Save")
                            .disabled(!valid_name || self.selected.is_none())
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    )
                    .child(
                        Button::new("cancel-save-request")
                            .debug_selector(|| "cancel-save-request".into())
                            .small()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    }
}
