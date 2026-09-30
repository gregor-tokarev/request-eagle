use std::path::PathBuf;

use collections_panel_ui::{CollectionPanel, RequestMatch};
use gpui_kit::component::{
    kbd::Kbd,
    list::{List, ListDelegate, ListItem, ListState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::main_view::MainView;
use crate::actions::ToggleCommandPalette;
use tab_ui::Environments;

/// Each group is capped, so a query never builds thousands of rows. The list
/// only lays out the rows in view. Requests appear only for a query.
const RESULT_LIMIT: usize = 50;

/// Every row and heading shares one height, so the list measures a single
/// row instead of each one.
const ROW_HEIGHT: Rems = rems(2.);
const HEADING_HEIGHT: Rems = rems(1.75);

const COMMANDS: usize = 0;
const REQUESTS: usize = 1;
const COLLECTIONS: usize = 2;
const ENVIRONMENTS: usize = 3;
const HEADINGS: [&str; 4] = ["Commands", "Requests", "Collections", "Environments"];

struct PaletteCommand {
    label: &'static str,
    category: &'static str,
    action: Box<dyn Action>,
    /// The shortcut that applies where the command will run, resolved once
    /// when the palette opens rather than on every render.
    binding: Option<Kbd>,
}

enum Row {
    Command(usize),
    Request(RequestMatch),
    Collection {
        path: PathBuf,
        name: SharedString,
        detail: SharedString,
    },
    Environment {
        name: SharedString,
        active: bool,
    },
}

/// Searches commands, requests, collections and environments. Sections are
/// the four groups in order; the list hides empty ones.
pub(crate) struct CommandPalette {
    sidebar: Entity<CollectionPanel>,
    main_view: Entity<MainView>,
    environments: Entity<Environments>,

    /// Commands available where focus was when the palette opened. Closing
    /// the palette returns focus there, and commands run there.
    commands: Vec<PaletteCommand>,

    groups: [Vec<Row>; 4],
    selected: Option<IndexPath>,
    /// Hides the empty message until requests have been searched too.
    searching_requests: bool,
    /// Rows appear one frame after the dialog. Opening moves focus, which
    /// redraws the whole window, so drawing the rows separately keeps both
    /// frames within the 120 fps budget.
    revealed: bool,
}

impl CommandPalette {
    /// Open the palette as a dialog. Call while focus is still in the
    /// workspace, so the commands available there can be listed.
    pub(crate) fn open(
        sidebar: Entity<CollectionPanel>,
        main_view: Entity<MainView>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<ListState<Self>> {
        let origin = window.focused(cx);
        let available = window.available_actions(cx);
        let mut commands: Vec<_> = keybindings_service::commands(cx)
            .into_iter()
            .filter(|command| command.id != ToggleCommandPalette::name_for_type())
            .filter_map(|command| {
                let action = available
                    .iter()
                    .find(|action| action.name() == command.id)?;

                let binding = match &origin {
                    Some(origin) => Kbd::binding_for_action_in(action.as_ref(), origin, window),
                    None => Kbd::binding_for_action(action.as_ref(), None, window),
                };

                Some(PaletteCommand {
                    label: command.label,
                    category: command.category,
                    action: action.boxed_clone(),
                    binding,
                })
            })
            .collect();
        commands.sort_by_key(|command| command.label);

        let environments = main_view.read(cx).environments.clone();
        let mut palette = Self {
            sidebar,
            main_view,
            environments,
            commands,
            groups: Default::default(),
            selected: None,
            searching_requests: false,
            revealed: false,
        };
        palette.search_now("", cx);

        let state = cx.new(|cx| {
            let mut state = ListState::new(palette, window, cx).searchable(true);
            let first = state.delegate().first_row();
            state.set_selected_index(first, window, cx);

            state
        });

        let dialog_state = state.clone();
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .close_button(false)
                .p_0()
                .w(rems(36.).to_pixels(window.rem_size()))
                .child(
                    div().debug_selector(|| "command-palette".into()).child(
                        List::new(&dialog_state)
                            .search_placeholder("Type a command or search…")
                            .max_h(rems(24.)),
                    ),
                )
        });

        state.update(cx, |state, cx| state.focus(window, cx));

        let revealed_state = state.downgrade();
        window.on_next_frame(move |_, cx| {
            let _ = revealed_state.update(cx, |state, cx| {
                state.delegate_mut().revealed = true;
                cx.notify();
            });
        });

        state
    }

    /// Update every group except requests, which are searched in the background.
    fn search_now(&mut self, query: &str, cx: &App) {
        let query = query.trim().to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();

        self.groups[COMMANDS] = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, command)| {
                let text = format!("{} {}", command.label, command.category).to_lowercase();
                words.iter().all(|word| text.contains(word))
            })
            .map(|(index, _)| Row::Command(index))
            .collect();

        // Clear results for the previous query, so Enter cannot open a request
        // that no longer matches while the new search runs.
        self.groups[REQUESTS].clear();

        let sidebar = self.sidebar.read(cx);

        self.groups[COLLECTIONS] = sidebar
            .find_collections(&query, RESULT_LIMIT)
            .into_iter()
            .map(|collection| Row::Collection {
                path: collection.path,
                name: collection.name,
                detail: match collection.request_count {
                    1 => "1 request".into(),
                    count => format!("{count} requests").into(),
                },
            })
            .collect();

        let environments = self.environments.read(cx);
        self.groups[ENVIRONMENTS] = environments
            .names()
            .iter()
            .filter(|name| name.to_lowercase().contains(&query))
            .take(RESULT_LIMIT)
            .map(|name| Row::Environment {
                name: name.clone(),
                active: environments.active() == Some(name),
            })
            .collect();
    }

    fn first_row(&self) -> Option<IndexPath> {
        self.groups
            .iter()
            .position(|rows| !rows.is_empty())
            .map(|section| IndexPath::new(0).section(section))
    }

    #[cfg(test)]
    pub(crate) fn request_count(&self) -> usize {
        self.groups[REQUESTS].len()
    }

    fn render_row(&self, row: &Row, cx: &App) -> Div {
        let content = h_flex().w_full().min_w_0().gap_2();
        let muted = cx.theme().muted_foreground;

        match row {
            Row::Command(index) => {
                let command = &self.commands[*index];

                content
                    .child(label(command.label.into()))
                    .child(detail(command.category.into(), cx))
                    .when_some(command.binding.clone(), |row, binding| row.child(binding))
            }
            Row::Request(request) => {
                let theme = cx.theme();
                let color = match request.method {
                    "GET" => theme.success,
                    "POST" => theme.warning,
                    "PUT" | "PATCH" => theme.info,
                    "HEAD" | "OPTIONS" => theme.muted_foreground,
                    _ => theme.danger,
                };

                content
                    .child(
                        div()
                            .flex_none()
                            .w(rems(3.5))
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(request.method),
                    )
                    .child(label(request.name.clone()))
                    .child(detail(request.location.clone(), cx))
            }
            Row::Collection {
                name, detail: text, ..
            } => content
                .child(
                    Icon::default()
                        .path("icons/package.svg")
                        .size_4()
                        .text_color(muted),
                )
                .child(label(name.clone()))
                .child(detail(text.clone(), cx)),
            Row::Environment { name, active } => content
                .child(Icon::new(IconName::Globe).size_4().text_color(muted))
                .child(label(name.clone()))
                .when(*active, |row| row.child(detail("Active".into(), cx))),
        }
    }
}

impl ListDelegate for CommandPalette {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.search_now(query, cx);

        let requests = (!query.trim().is_empty()).then(|| {
            self.sidebar
                .read(cx)
                .find_requests(query.trim(), RESULT_LIMIT, cx)
        });
        self.searching_requests = requests.is_some();

        cx.spawn_in(window, async move |state, cx| {
            // The list resets its selection once this search starts; select
            // the first result instead.
            let _ = state.update_in(cx, |state, window, cx| {
                let first = state.delegate().first_row();
                state.set_selected_index(first, window, cx);
            });

            let Some(requests) = requests else {
                return;
            };
            let requests = requests.await;

            let _ = state.update_in(cx, |state, window, cx| {
                let untouched = state.selected_index() == state.delegate().first_row();

                let palette = state.delegate_mut();
                palette.groups[REQUESTS] = requests.into_iter().map(Row::Request).collect();
                palette.searching_requests = false;

                // Requests come before collections, so move a selection the
                // user has not changed to the new first result.
                if untouched {
                    let first = state.delegate().first_row();
                    state.set_selected_index(first, window, cx);
                }
                cx.notify();
            });
        })
    }

    fn sections_count(&self, _: &App) -> usize {
        self.groups.len()
    }

    fn items_count(&self, section: usize, _: &App) -> usize {
        if self.revealed {
            self.groups[section].len()
        } else {
            0
        }
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        Some(
            h_flex()
                .h(HEADING_HEIGHT)
                .px_3()
                .text_xs()
                .font_medium()
                .text_color(cx.theme().muted_foreground)
                .child(HEADINGS[section]),
        )
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let row = self.groups[index.section].get(index.row)?;

        Some(
            ListItem::new((
                "command-palette-row",
                index.section * RESULT_LIMIT * 2 + index.row,
            ))
            .h(ROW_HEIGHT)
            .text_sm()
            .child(self.render_row(row, cx)),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .py_6()
            .w_full()
            .text_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .when(self.revealed && !self.searching_requests, |this| {
                this.child("No matching commands, requests, collections or environments.")
            })
    }

    fn set_selected_index(
        &mut self,
        index: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = index;
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(row) = self
            .selected
            .and_then(|index| self.groups[index.section].get(index.row))
        else {
            return;
        };

        // Closing restores the original focus, where commands are dispatched.
        window.close_dialog(cx);

        match row {
            Row::Command(index) => {
                window.dispatch_action(self.commands[*index].action.boxed_clone(), cx)
            }
            Row::Request(RequestMatch { path, .. }) | Row::Collection { path, .. } => {
                self.sidebar
                    .update(cx, |sidebar, cx| sidebar.open_at(path, cx));
                self.main_view.update(cx, |view, cx| view.focus(window, cx));
            }
            Row::Environment { name, .. } => {
                self.main_view.update(cx, |view, cx| {
                    view.open_environment(name.clone(), window, cx)
                });
            }
        }
    }
}

fn label(text: SharedString) -> Div {
    div().flex_1().min_w_0().text_ellipsis().child(text)
}

fn detail(text: SharedString, cx: &App) -> Div {
    div()
        .flex_none()
        .max_w(rems(14.))
        .text_ellipsis()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}
