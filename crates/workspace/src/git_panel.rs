use std::{collections::HashMap, path::PathBuf};

use git::{ChangeKind, Status};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::actions::{OpenCollectionFolder, ToggleGitPanel};
use crate::git_state::{GitAction, GitEvent, GitRepository, GitState};

/// How many changes a repository lists before it only counts the rest, so
/// that a bulk import does not make the panel draw thousands of rows.
const LISTED_CHANGES: usize = 100;

/// Takes the sidebar's place, as a code editor's source control view does.
/// It shows each repository that holds collections, its changes, and the
/// actions that commit, pull, push and sync them.
pub(crate) struct GitPanel {
    state: Entity<GitState>,
    /// Whether the sidebar shows it instead of the collections.
    pub(crate) open: bool,
    /// Each repository's commit message, by its folder. Enter commits.
    messages: HashMap<PathBuf, (Entity<InputState>, Subscription)>,
    /// The URL of the remote to publish a repository without one to. Enter
    /// publishes.
    remote_urls: HashMap<PathBuf, (Entity<InputState>, Subscription)>,
    pub(crate) focus: FocusHandle,
    _subscriptions: [Subscription; 2],
}

impl GitPanel {
    pub(crate) fn new(
        state: Entity<GitState>,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = [
            cx.observe_in(&state, window, |this, _, window, cx| {
                this.sync_inputs(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(&state, window, |this, _, event, window, cx| {
                let GitEvent::Finished { action, errors } = event;
                // A message is typed for one commit.
                if errors.is_empty() && matches!(action, GitAction::Commit | GitAction::Sync) {
                    for (message, _) in this.messages.values() {
                        message.update(cx, |message, cx| message.set_value("", window, cx));
                    }
                }
            }),
        ];

        Self {
            state,
            open,
            messages: HashMap::new(),
            remote_urls: HashMap::new(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Keep an input for each repository's message and remote.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let roots: Vec<PathBuf> = self
            .state
            .read(cx)
            .repositories
            .iter()
            .map(|known| known.repository.root().to_path_buf())
            .collect();

        for root in &roots {
            if !self.messages.contains_key(root) {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Commit message"));
                let committed = root.clone();
                let subscription = cx.subscribe(&input, move |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.act(GitAction::Commit, committed.clone(), cx);
                    }
                });
                self.messages.insert(root.clone(), (input, subscription));
            }
            if !self.remote_urls.contains_key(root) {
                let input = cx.new(|cx| {
                    InputState::new(window, cx).placeholder("git@github.com:you/collections.git")
                });
                let published = root.clone();
                let subscription =
                    cx.subscribe(&input, move |this, input, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::PressEnter { .. })
                            && !input.read(cx).value().trim().is_empty()
                        {
                            this.act(GitAction::Publish, published.clone(), cx);
                        }
                    });
                self.remote_urls.insert(root.clone(), (input, subscription));
            }
        }

        self.messages.retain(|root, _| roots.contains(root));
        self.remote_urls.retain(|root, _| roots.contains(root));
    }

    pub(crate) fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            cx.notify();
        }

        if open {
            self.state
                .update(cx, |state, cx| state.schedule_refresh(cx));
        }
    }

    /// Focus the commit message of the first repository with changes.
    pub(crate) fn begin_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.state.read(cx);
        let Some(input) = state
            .repositories
            .iter()
            .find(|known| {
                known
                    .status()
                    .is_some_and(|status| !status.changes.is_empty())
            })
            .or(state.repositories.first())
            .and_then(|known| self.messages.get(known.repository.root()))
            .map(|(input, _)| input.clone())
        else {
            return;
        };

        // The sidebar shows the panel from the next frame drawn. Next-frame
        // callbacks run before that frame is drawn, so focus one frame later.
        cx.on_next_frame(window, move |_, window, cx| {
            cx.on_next_frame(window, move |_, window, cx| {
                input.update(cx, |input, cx| input.focus(window, cx));
            });
        });
    }

    fn act(&mut self, action: GitAction, root: PathBuf, cx: &mut Context<Self>) {
        let input = match action {
            GitAction::Commit | GitAction::Sync => self.messages.get(&root).map(|(input, _)| input),
            GitAction::Publish => self.remote_urls.get(&root).map(|(input, _)| input),
            _ => None,
        }
        .map(|input| input.read(cx).value().to_string());

        self.state
            .update(cx, |state, cx| state.run(action, Some(&root), input, cx));
    }

    fn render_repository(&self, known: &GitRepository, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let root = known.repository.root().to_path_buf();
        let running = self.state.read(cx).running;
        let busy = running.is_some();
        let status = known.status();
        let changes = status.map_or(0, |status| status.changes.len());
        let upstream = status.is_some_and(|status| status.upstream.is_some());
        let remote = status.is_some_and(|status| !status.remotes.is_empty());
        let interrupted = known.is_interrupted();

        let action_button = |label: &'static str, action: GitAction, enabled: bool| {
            let root = root.clone();
            // Repositories can share a name, but not a folder.
            Button::new(SharedString::from(format!(
                "git-{label}-{}",
                root.display()
            )))
            .debug_selector(move || format!("git-{}", label.to_lowercase()))
            .loading(running == Some(action))
            .disabled(busy || interrupted || !enabled)
            .on_click(cx.listener(move |this, _, _, cx| this.act(action, root.clone(), cx)))
        };

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Icon::default()
                            .path("icons/git-branch.svg")
                            .size_4()
                            .flex_none()
                            .text_color(muted),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_ellipsis()
                                    .child(known.repository.name()),
                            )
                            .when_some(status.and_then(branch), |this, branch| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_ellipsis()
                                        .text_color(muted)
                                        .child(branch),
                                )
                            }),
                    )
                    // The commits to pull and push, which pull and push them.
                    .when_some(status.filter(|_| remote), |this, status| {
                        this.child(
                            h_flex()
                                .flex_none()
                                .child(
                                    action_button("Pull", GitAction::Pull, upstream)
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::ArrowDown)
                                        .when(status.behind > 0, |this| {
                                            this.label(status.behind.to_string())
                                        })
                                        .accessibility_label("Pull")
                                        .tooltip("Pull"),
                                )
                                .child(
                                    action_button("Push", GitAction::Push, true)
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::ArrowUp)
                                        .when(status.ahead > 0, |this| {
                                            this.label(status.ahead.to_string())
                                        })
                                        .accessibility_label("Push")
                                        .tooltip("Push"),
                                ),
                        )
                    }),
            )
            .map(|this| match &known.status {
                None => this.child(note("Reading the status…", muted)),
                Some(Err(error)) => this.child(warning(error.to_string(), cx)),
                Some(Ok(status)) => this
                    .when(interrupted, |this| {
                        this.child(warning(
                            "Resolve the conflicts with Git, then sync again.",
                            cx,
                        ))
                    })
                    .when(!remote, |this| {
                        let url = self.remote_urls.get(&root).map(|(input, _)| input.clone());
                        this.child(note(
                            "Add a remote to share these collections with your team.",
                            muted,
                        ))
                        .when_some(url, |this, url| {
                            let filled = !url.read(cx).value().trim().is_empty();
                            this.child(
                                h_flex()
                                    .gap_2()
                                    .child(div().flex_1().min_w_0().child(Input::new(&url)))
                                    .child(
                                        action_button("Publish", GitAction::Publish, filled)
                                            .label("Publish")
                                            .primary(),
                                    ),
                            )
                        })
                    })
                    .when(changes > 0 && !interrupted, |this| {
                        this.when_some(self.messages.get(&root), |this, (message, _)| {
                            this.child(Input::new(message))
                        })
                    })
                    .when(remote || changes > 0, |this| {
                        this.child(
                            h_flex()
                                .gap_2()
                                .child(
                                    action_button("Commit", GitAction::Commit, changes > 0)
                                        .label("Commit")
                                        .flex_1(),
                                )
                                .when(remote, |this| {
                                    this.child(
                                        action_button("Sync", GitAction::Sync, true)
                                            .label("Sync")
                                            .primary()
                                            .flex_1(),
                                    )
                                }),
                        )
                    })
                    .child(self.render_changes(known, status, cx)),
            })
            .into_any_element()
    }

    fn render_changes(&self, known: &GitRepository, status: &Status, cx: &App) -> AnyElement {
        let theme = cx.theme();
        if status.changes.is_empty() {
            return note("No changes to commit.", theme.muted_foreground).into_any_element();
        }

        let hidden = status.changes.len().saturating_sub(LISTED_CHANGES);
        v_flex()
            .gap_1()
            .children(status.changes.iter().take(LISTED_CHANGES).map(|change| {
                let (letter, color) = match change.kind {
                    ChangeKind::Added => ("A", theme.success),
                    ChangeKind::Modified => ("M", theme.warning),
                    ChangeKind::Deleted => ("D", theme.danger),
                    ChangeKind::Renamed => ("R", theme.info),
                    ChangeKind::Conflicted => ("!", theme.danger),
                };
                // Named from its collection, which the user knows it by.
                let path = known
                    .collection_of(&change.path)
                    .and_then(|collection| collection.real_path.parent())
                    .unwrap_or(known.repository.root());
                let path = change
                    .path
                    .strip_prefix(path)
                    .unwrap_or(&change.path)
                    .to_string_lossy()
                    .into_owned();

                h_flex()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .flex_none()
                            .w(rems(0.75))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(letter),
                    )
                    .child(div().flex_1().min_w_0().text_ellipsis().child(path))
            }))
            .when(hidden > 0, |this| {
                this.child(note(format!("and {hidden} more"), theme.muted_foreground))
            })
            .into_any_element()
    }

    fn render_empty(&self, cx: &Context<Self>) -> AnyElement {
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Your collections aren't in a Git repository"),
            )
            .child(note(
                "Track them with Git to keep their history and sync them with your team. \
                 Collections opened from a folder in a repository are tracked by it too.",
                cx.theme().muted_foreground,
            ))
            .child(
                v_flex()
                    .pt_1()
                    .gap_2()
                    .child(self.track_button(cx).w_full())
                    .child(
                        Button::new("git-open-folder")
                            .label("Open Folder…")
                            .w_full()
                            .on_click(|_, window, cx| {
                                window.dispatch_action(OpenCollectionFolder.boxed_clone(), cx)
                            }),
                    ),
            )
            .into_any_element()
    }

    /// Starts a repository in the collections directory.
    fn track_button(&self, cx: &Context<Self>) -> Button {
        let running = self.state.read(cx).running;

        Button::new("git-init")
            .debug_selector(|| "git-init".into())
            .primary()
            .label("Track with Git")
            .loading(running == Some(GitAction::Init))
            .disabled(running.is_some())
            .on_click(
                cx.listener(|this, _, _, cx| this.state.update(cx, |state, cx| state.init(cx))),
            )
    }
}

impl Focusable for GitPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for GitPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let border = cx.theme().sidebar_border;
        let empty = state.repositories.is_empty();
        let untracked_directory = state.untracked_directory;
        let sections: Vec<_> = (0..state.repositories.len()).collect();

        v_flex()
            .debug_selector(|| "git-panel".into())
            .key_context("GitPanel")
            .track_focus(&self.focus)
            .size_full()
            .pt_1()
            .child(
                h_flex()
                    .flex_none()
                    .h_8()
                    .pl_4()
                    .pr_3()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("GIT"),
                    )
                    .child(
                        Button::new("git-refresh")
                            .ghost()
                            .xsmall()
                            .icon(Icon::default().path("icons/refresh-cw.svg"))
                            .accessibility_label("Refresh")
                            .tooltip("Refresh")
                            .disabled(state.running.is_some())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.update(cx, |state, cx| state.refresh_all(cx))
                            })),
                    )
                    .child(
                        Button::new("git-close")
                            .debug_selector(|| "git-close".into())
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .accessibility_label("Show collections")
                            .tooltip_with_action("Show Collections", &ToggleGitPanel, None)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(ToggleGitPanel.boxed_clone(), cx)
                            }),
                    ),
            )
            .child(
                div()
                    .id("git-panel-repositories")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .px_4()
                            .pt_1()
                            .pb_4()
                            .gap_4()
                            .when(empty, |this| this.child(self.render_empty(cx)))
                            .children(sections.into_iter().map(|index| {
                                let known = &self.state.read(cx).repositories[index];
                                let section = self.render_repository(known, cx);

                                v_flex()
                                    .gap_4()
                                    .when(index > 0, |this| this.child(div().h(px(1.)).bg(border)))
                                    .child(section)
                            }))
                            // Other repositories can hold collections while
                            // those in the collections directory are in none.
                            .when(untracked_directory && !empty, |this| {
                                this.child(div().h(px(1.)).bg(border)).child(
                                    v_flex()
                                        .gap_2()
                                        .child(note(
                                            "The collections in your collections folder aren't \
                                             in a repository.",
                                            cx.theme().muted_foreground,
                                        ))
                                        .child(self.track_button(cx).w_full()),
                                )
                            }),
                    ),
            )
    }
}

/// The branch and the remote branch it tracks.
fn branch(status: &Status) -> Option<String> {
    let branch = status.branch.as_deref()?;

    Some(match &status.upstream {
        Some(upstream) => format!("{branch} → {upstream}"),
        None => branch.to_owned(),
    })
}

/// Commits to pull and to push, as arrows with counts.
pub(crate) fn sync_counts(behind: u32, ahead: u32, color: Hsla) -> Div {
    h_flex()
        .flex_none()
        .gap_1()
        .text_xs()
        .text_color(color)
        .child(Icon::new(IconName::ArrowDown).size_3())
        .child(behind.to_string())
        .child(Icon::new(IconName::ArrowUp).size_3())
        .child(ahead.to_string())
}

fn note(text: impl Into<SharedString>, color: Hsla) -> Div {
    div().text_xs().text_color(color).child(text.into())
}

/// A state that holds Git actions back until the user deals with it.
fn warning(text: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .items_start()
        .gap_2()
        .text_xs()
        .text_color(cx.theme().warning)
        .child(Icon::new(IconName::TriangleAlert).size_3p5().flex_none())
        .child(div().flex_1().min_w_0().child(text.into()))
}
