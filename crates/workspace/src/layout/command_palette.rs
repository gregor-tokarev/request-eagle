use std::path::PathBuf;

use collections_panel_ui::{CollectionPanel, RequestMatch};
use gpui_kit::component::{
    command::{Command, CommandGroup, CommandItem, CommandState},
    kbd::Kbd,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use super::main_view::MainView;
use crate::actions::ToggleCommandPalette;

/// Each group is capped, so opening the palette and every keystroke lay out
/// a short list even with thousands of collections or requests. Requests
/// appear only for a query.
const RESULT_LIMIT: usize = 50;

/// What confirming a row does.
enum Target {
    Command(Box<dyn Action>),
    Request(PathBuf),
    Collection(PathBuf),
    Environment(PathBuf),
}

struct PaletteCommand {
    label: &'static str,
    category: &'static str,
    action: Box<dyn Action>,
}

struct Group {
    heading: &'static str,
    items: Vec<CommandItem>,
    targets: Vec<Target>,
}

impl Group {
    fn new(heading: &'static str) -> Self {
        Self {
            heading,
            items: Vec::new(),
            targets: Vec::new(),
        }
    }

    fn set(&mut self, results: impl IntoIterator<Item = (CommandItem, Target)>) {
        (self.items, self.targets) = results.into_iter().unzip();
    }
}

/// Searches commands, requests, collections and environments.
pub(crate) struct CommandPalette {
    state: Entity<CommandState>,
    sidebar: Entity<CollectionPanel>,
    main_view: Entity<MainView>,
    sidebar_visible: Entity<bool>,

    /// Focus from before the palette opened. Closing the palette returns
    /// focus there, and commands run there.
    origin: Option<FocusHandle>,
    commands: Vec<PaletteCommand>,

    /// Results in display order. Empty groups are hidden, so palette
    /// sections index the non-empty groups.
    groups: [Group; 4],
    /// Replacing the search cancels one still running for an older query.
    request_search: Task<()>,
}

const COMMANDS: usize = 0;
const REQUESTS: usize = 1;
const COLLECTIONS: usize = 2;
const ENVIRONMENTS: usize = 3;

impl CommandPalette {
    /// Open the palette as a dialog. Call while focus is still in the
    /// workspace, so the commands available there can be listed.
    pub(crate) fn open(
        sidebar: Entity<CollectionPanel>,
        main_view: Entity<MainView>,
        sidebar_visible: Entity<bool>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let available = window.available_actions(cx);
        let mut commands: Vec<_> = keybindings_service::commands(cx)
            .into_iter()
            .filter(|command| command.id != ToggleCommandPalette::name_for_type())
            .filter_map(|command| {
                let action = available
                    .iter()
                    .find(|action| action.name() == command.id)?;

                Some(PaletteCommand {
                    label: command.label,
                    category: command.category,
                    action: action.boxed_clone(),
                })
            })
            .collect();
        commands.sort_by_key(|command| command.label);

        let palette = cx.new(|cx| {
            let mut palette = Self {
                state: cx.new(|cx| CommandState::new(window, cx)),
                sidebar,
                main_view,
                sidebar_visible,
                origin: window.focused(cx),
                commands,
                groups: [
                    Group::new("Commands"),
                    Group::new("Requests"),
                    Group::new("Collections"),
                    Group::new("Environments"),
                ],
                request_search: Task::ready(()),
            };
            palette.search("", cx);

            palette
        });

        let dialog_palette = palette.clone();
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .close_button(false)
                .p_0()
                .w(rems(36.).to_pixels(window.rem_size()))
                .child(dialog_palette.clone())
        });

        let state = palette.read(cx).state.clone();
        state.update(cx, |state, cx| state.focus(window, cx));

        palette
    }

    pub(super) fn search(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = query.trim().to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();

        let commands = self
            .commands
            .iter()
            .filter(|command| {
                let text = format!("{} {}", command.label, command.category).to_lowercase();
                words.iter().all(|word| text.contains(word))
            })
            .map(|command| {
                (
                    command_item(command, self.origin.clone()),
                    Target::Command(command.action.boxed_clone()),
                )
            })
            .collect::<Vec<_>>();
        self.groups[COMMANDS].set(commands);

        // Clear results for the previous query, so Enter cannot open a request
        // that no longer matches while the new search runs.
        self.groups[REQUESTS].set([]);
        let requests = self
            .sidebar
            .read(cx)
            .find_requests(&query, RESULT_LIMIT, cx);
        self.request_search = cx.spawn(async move |this, cx| {
            let requests = requests.await;

            let _ = this.update(cx, |this, cx| {
                this.groups[REQUESTS].set(requests.into_iter().map(request_result));
                cx.notify();
            });
        });

        let sidebar = self.sidebar.read(cx);

        let collections = sidebar
            .find_collections(&query, RESULT_LIMIT)
            .into_iter()
            .map(|collection| {
                let count = match collection.request_count {
                    1 => "1 request".to_owned(),
                    count => format!("{count} requests"),
                };
                let item = CommandItem::new()
                    .label(collection.name.clone())
                    .child(move |_, cx| {
                        row()
                            .child(
                                Icon::default()
                                    .path("icons/package.svg")
                                    .size_4()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(label(collection.name.clone()))
                            .child(detail(count.clone().into(), cx))
                    });

                (item, Target::Collection(collection.path))
            });
        let collections = collections.collect::<Vec<_>>();

        let environments = sidebar
            .find_environments(&query, RESULT_LIMIT)
            .into_iter()
            .map(|environment| {
                let count = match environment.variable_count {
                    1 => "1 variable".to_owned(),
                    count => format!("{count} variables"),
                };
                let item =
                    CommandItem::new()
                        .label(environment.name.clone())
                        .child(move |_, cx| {
                            row()
                                .child(
                                    Icon::new(IconName::Globe)
                                        .size_4()
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .child(label(environment.name.clone()))
                                .child(detail(count.clone().into(), cx))
                        });

                (item, Target::Environment(environment.path))
            })
            .collect::<Vec<_>>();

        self.groups[COLLECTIONS].set(collections);
        self.groups[ENVIRONMENTS].set(environments);

        cx.notify();
    }

    #[cfg(test)]
    pub(super) fn request_count(&self) -> usize {
        self.groups[REQUESTS].items.len()
    }

    fn visible_groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.iter().filter(|group| !group.items.is_empty())
    }

    fn confirm(&mut self, index: IndexPath, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self
            .visible_groups()
            .nth(index.section)
            .and_then(|group| group.targets.get(index.row))
        else {
            return;
        };

        // Closing restores the original focus, where commands are dispatched.
        window.close_dialog(cx);

        match target {
            Target::Command(action) => window.dispatch_action(action.boxed_clone(), cx),
            Target::Request(path) => {
                self.sidebar
                    .update(cx, |sidebar, cx| sidebar.open_request_at(path, cx));
                self.main_view.update(cx, |view, cx| view.focus(window, cx));
            }
            Target::Collection(path) => {
                self.sidebar_visible.update(cx, |visible, cx| {
                    *visible = true;
                    cx.notify();
                });
                self.sidebar
                    .update(cx, |sidebar, cx| sidebar.reveal(path, window, cx));
            }
            // Environments have no page yet; edit the file where it is kept.
            Target::Environment(path) => cx.open_with_system(path),
        }
    }
}

fn command_item(command: &PaletteCommand, origin: Option<FocusHandle>) -> CommandItem {
    let label_text = command.label;
    let category = command.category;
    let action = command.action.boxed_clone();

    CommandItem::new()
        .label(label_text)
        .child(move |window, cx| {
            // Show the shortcut that applies where the command will run.
            let binding = match &origin {
                Some(origin) => Kbd::binding_for_action_in(action.as_ref(), origin, window),
                None => Kbd::binding_for_action(action.as_ref(), None, window),
            };

            row()
                .child(label(label_text.into()))
                .child(detail(category.into(), cx))
                .when_some(binding, |row, binding| row.child(binding))
        })
}

fn request_result(request: RequestMatch) -> (CommandItem, Target) {
    let name = request.name.clone();
    let method = request.method;
    let location = request.location.clone();

    let item = CommandItem::new().label(request.name).child(move |_, cx| {
        let theme = cx.theme();
        let color = match method {
            "GET" => theme.success,
            "POST" => theme.warning,
            "PUT" | "PATCH" => theme.info,
            "HEAD" | "OPTIONS" => theme.muted_foreground,
            _ => theme.danger,
        };

        row()
            .child(
                div()
                    .flex_none()
                    .w(rems(3.5))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(method),
            )
            .child(label(name.clone()))
            .child(detail(location.clone(), cx))
    });

    (item, Target::Request(request.path))
}

impl Render for CommandPalette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query_palette = cx.weak_entity();
        let confirm_palette = cx.weak_entity();

        div().debug_selector(|| "command-palette".into()).child(
            Command::new(&self.state)
                .bordered(false)
                // Requests, collections and environments are matched by the
                // sidebar, which searches more than the row labels.
                .filterable(false)
                .placeholder("Type a command or search…")
                .max_h(rems(24.))
                .empty(|_, _, cx| {
                    div()
                        .py_6()
                        .w_full()
                        .text_center()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("No matching commands, requests, collections or environments.")
                })
                .on_query(move |query, _, cx| {
                    _ = query_palette.update(cx, |palette, cx| palette.search(query, cx));
                })
                .on_confirm(move |index, window, cx| {
                    _ = confirm_palette
                        .update(cx, |palette, cx| palette.confirm(index, window, cx));
                })
                .map(|command| {
                    self.visible_groups().fold(command, |command, group| {
                        command.group(
                            CommandGroup::new()
                                .label(group.heading)
                                .items(group.items.iter().cloned()),
                        )
                    })
                }),
        )
    }
}

fn row() -> Div {
    h_flex().flex_1().min_w_0().gap_2()
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
