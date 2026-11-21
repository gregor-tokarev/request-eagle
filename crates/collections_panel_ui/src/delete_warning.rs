use std::path::{Path, PathBuf};

use collection::Collections;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    dialog::{Confirm, DialogFooter},
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::Request;

use super::{
    panel::{CollectionPanel, CollectionPanelEvent},
    tree::{CollectionTree, ItemKind},
};

/// How deleting or removing a target affects a flow's HTTP Request blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowImpact {
    /// At least one referenced block will have no saved HTTP request left to send.
    MissingRequest,
    /// Affected blocks will resolve to another saved HTTP request with the same ID.
    DifferentRequest,
    /// Some blocks will lose their request and others will resolve to another copy.
    Mixed,
}

/// A flow that sends a saved HTTP request being deleted or removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AffectedFlow {
    pub path: PathBuf,
    pub id: SharedString,
    pub name: SharedString,
    pub impact: FlowImpact,
}

/// Snapshot of a sidebar item and the saved requests under it when deletion is
/// requested, so confirmation can detect changes before deleting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TargetSnapshot {
    pub path: PathBuf,
    pub label: SharedString,
    pub kind: ItemKind,
    pub linked: bool,
    /// Saved requests at or under `path`: `(path, id, is_http)`.
    pub requests: Vec<(PathBuf, String, bool)>,
}

impl TargetSnapshot {
    pub(super) fn capture(
        collections: &Collections,
        tree: &CollectionTree,
        path: &Path,
    ) -> Option<Self> {
        let index = tree.index_of(path)?;
        let item = &tree.items[index];
        if item.kind == ItemKind::Empty {
            return None;
        }

        let linked = item.kind == ItemKind::Collection && collections.registry().is_linked(path);
        let requests = match item.kind {
            ItemKind::Request(_) => {
                let (_, file) = collections.request(path)?;
                vec![(
                    file.path.clone(),
                    file.id.clone(),
                    matches!(file.request, Request::Http(_)),
                )]
            }
            ItemKind::Collection => {
                collections.collection(path)?;
                collections
                    .requests_in(path)?
                    .into_iter()
                    .map(|(_, file)| {
                        (
                            file.path.clone(),
                            file.id.clone(),
                            matches!(file.request, Request::Http(_)),
                        )
                    })
                    .collect()
            }
            ItemKind::Folder => collections
                .requests_in(path)?
                .into_iter()
                .map(|(_, file)| {
                    (
                        file.path.clone(),
                        file.id.clone(),
                        matches!(file.request, Request::Http(_)),
                    )
                })
                .collect(),
            ItemKind::Empty => return None,
        };

        Some(Self {
            path: item.path.clone(),
            label: item.label.clone(),
            kind: item.kind,
            linked,
            requests,
        })
    }
}

/// Result of rechecking a pending delete right before executing it.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ConfirmationCheck {
    Proceed,
    Missing,
    Changed {
        target: TargetSnapshot,
        affected: Vec<AffectedFlow>,
        notice: String,
    },
}

pub(super) fn recheck_confirmation(
    previous_target: &TargetSnapshot,
    previous_affected: &[AffectedFlow],
    current_target: Option<TargetSnapshot>,
    current_affected: Vec<AffectedFlow>,
) -> ConfirmationCheck {
    let Some(target) = current_target else {
        return ConfirmationCheck::Missing;
    };

    let target_changed = target != *previous_target;
    let affected_changed = current_affected != previous_affected;

    if !target_changed && !affected_changed {
        return ConfirmationCheck::Proceed;
    }

    let notice = match (target_changed, affected_changed) {
        (true, true) => {
            "This item and its affected flows changed while this dialog was open. Review the \
             updated details and confirm again."
        }
        (true, false) => {
            "This item changed while this dialog was open. Review the updated details and confirm \
             again."
        }
        (false, true) => {
            "Affected flows changed while this dialog was open. Review the updated details and \
             confirm again."
        }
        (false, false) => unreachable!(),
    };

    ConfirmationCheck::Changed {
        target,
        affected: current_affected,
        notice: notice.to_owned(),
    }
}

impl CollectionPanel {
    /// Warns about flows that send saved requests under `target`, with links to
    /// open and inspect each flow before deleting or removing it.
    pub(super) fn open_delete_warning(
        &mut self,
        target: TargetSnapshot,
        affected: Vec<AffectedFlow>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }

        let panel = cx.entity().downgrade();
        let dialog = cx.new(|_| DeleteWarningDialog {
            panel,
            target,
            affected,
            checking: None,
            notice: None,
            error: None,
            missing: false,
        });

        window.open_alert_dialog(cx, move |alert, window, cx| {
            let theme = cx.theme();
            let state = dialog.read(cx);
            let (title, description, _) = delete_warning_copy(
                state.target.kind,
                &state.target.label,
                state.target.linked,
                &state.affected,
            );
            let confirm_label = if state.target.linked {
                "Remove"
            } else {
                "Delete"
            };
            let missing = state.missing;
            let checking = state.checking.is_some();
            let cancel_dialog = dialog.clone();
            let cancel_dialog_click = dialog.clone();
            let cancel_dialog_key = dialog.clone();
            let confirm_dialog = dialog.clone();
            let confirm_dialog_key = dialog.clone();

            alert
                .width(rems(30.).to_pixels(window.rem_size()))
                .icon(
                    Icon::new(IconName::TriangleAlert)
                        .size_4()
                        .mt(rems(0.125))
                        .flex_none()
                        .text_color(theme.warning),
                )
                .title(title)
                .description(description)
                // Require explicit confirmation via the Delete/Remove button so
                // a reflex Enter press cannot delete a request used by flows.
                .on_ok(|_, _, _| false)
                .on_cancel(move |_, _, cx| {
                    cancel_dialog.update(cx, |dialog, _| dialog.checking = None);
                    true
                })
                .child(dialog.clone())
                .footer(
                    DialogFooter::new()
                        .child(
                            div()
                                .on_action(move |_: &Confirm, window, cx| {
                                    cancel_dialog_key
                                        .update(cx, |dialog, _| dialog.checking = None);
                                    window.close_dialog(cx);
                                })
                                .child(
                                    Button::new("cancel-delete-affected-flows")
                                        .debug_selector(|| "cancel-delete-affected-flows".into())
                                        .label("Cancel")
                                        .on_click(move |_, window, cx| {
                                            cancel_dialog_click
                                                .update(cx, |dialog, _| dialog.checking = None);
                                            window.close_dialog(cx);
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .on_action(move |_: &Confirm, window, cx| {
                                    confirm_dialog_key
                                        .update(cx, |dialog, cx| dialog.confirm(window, cx));
                                })
                                .child(
                                    Button::new("confirm-delete-affected-flows")
                                        .debug_selector(|| "confirm-delete-affected-flows".into())
                                        .danger()
                                        .disabled(missing)
                                        .loading(checking)
                                        .label(confirm_label)
                                        .on_click(move |_, window, cx| {
                                            confirm_dialog.update(cx, |dialog, cx| {
                                                dialog.confirm(window, cx)
                                            });
                                        }),
                                ),
                        ),
                )
        });
    }
}

/// Title, description, and section heading for the affected-flows warning dialog.
pub(super) fn delete_warning_copy(
    kind: ItemKind,
    label: &str,
    linked: bool,
    affected: &[AffectedFlow],
) -> (SharedString, SharedString, SharedString) {
    let title = match kind {
        ItemKind::Collection if linked => "Remove collection?",
        ItemKind::Collection => "Delete collection?",
        ItemKind::Folder => "Delete folder?",
        ItemKind::Request(_) | ItemKind::Empty => "Delete request?",
    };

    let count = affected.len();
    let heading = match count {
        1 => "Affected flow (1)".to_owned(),
        count => format!("Affected flows ({count})"),
    };

    if count == 0 {
        let description = match (kind, linked) {
            (ItemKind::Collection, true) => format!(
                "No flows currently use requests in “{label}”. Its folder stays on disk. Confirm \
                 again to remove this collection."
            ),
            (ItemKind::Collection, false) => format!(
                "No flows currently use requests in “{label}”. Confirm again to delete this \
                 collection."
            ),
            (ItemKind::Folder, _) => format!(
                "No flows currently use requests in “{label}”. Confirm again to delete this folder."
            ),
            (ItemKind::Request(_) | ItemKind::Empty, _) => {
                format!("No flows currently use “{label}”. Confirm again to delete it.")
            }
        };

        return (title.into(), description.into(), heading.into());
    }

    let flows = match count {
        1 => "1 flow".to_owned(),
        count => format!("{count} flows"),
    };
    let impact = combined_impact(affected);
    let block_outcome = match impact {
        Some(FlowImpact::MissingRequest) => "fail if a run reaches it",
        Some(FlowImpact::DifferentRequest) => {
            "send another saved request with the same ID if a run reaches it"
        }
        Some(FlowImpact::Mixed) | None => {
            "fail or send another saved request with the same ID if a run reaches it"
        }
    };

    let description = match (kind, linked) {
        (ItemKind::Collection, true) => {
            let conjunction = if impact == Some(FlowImpact::MissingRequest) {
                "but"
            } else {
                "and"
            };
            format!(
                "Removing “{label}” affects {flows}. Its folder stays on disk, {conjunction} any \
                 block that sends a request from it will {block_outcome}."
            )
        }
        (ItemKind::Collection, false) => format!(
            "Deleting “{label}” affects {flows}. Any block that sends a request from this \
             collection will {block_outcome}."
        ),
        (ItemKind::Folder, _) => format!(
            "Deleting “{label}” affects {flows}. Any block that sends a request from this folder \
             will {block_outcome}."
        ),
        (ItemKind::Request(_) | ItemKind::Empty, _) => format!(
            "“{label}” is used by {flows}. Deleting it will cause any block that uses it to \
             {block_outcome}."
        ),
    };

    (title.into(), description.into(), heading.into())
}

fn combined_impact(affected: &[AffectedFlow]) -> Option<FlowImpact> {
    let mut combined: Option<FlowImpact> = None;

    for flow in affected {
        combined = match (combined, flow.impact) {
            (None, impact) => Some(impact),
            (Some(current), impact) if current == impact => Some(current),
            (Some(_), _) => return Some(FlowImpact::Mixed),
        };
    }

    combined
}

struct DeleteWarningDialog {
    panel: WeakEntity<CollectionPanel>,
    target: TargetSnapshot,
    affected: Vec<AffectedFlow>,
    /// Waiting for an in-progress collections or flows refresh before rechecking.
    checking: Option<Task<()>>,
    notice: Option<String>,
    error: Option<String>,
    missing: bool,
}

impl DeleteWarningDialog {
    fn inspect_flow(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(flow) = self.affected.get(index).cloned() else {
            return;
        };

        self.checking = None;
        // Close synchronously before opening the flow so dialog focus restore
        // does not pull focus back away from the opened flow tab.
        window.close_dialog(cx);
        let _ = self.panel.update(cx, |_, cx| {
            cx.emit(CollectionPanelEvent::OpenFlow {
                path: flow.path,
                id: flow.id,
            });
        });
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.missing || self.checking.is_some() {
            return;
        }

        let wait = self
            .panel
            .read_with(cx, |panel, _| panel.refresh_waiter())
            .ok()
            .and_then(|waiter| waiter(cx));
        if let Some(wait) = wait {
            self.checking = Some(cx.spawn_in(window, async move |this, cx| {
                wait.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.checking = None;
                    this.confirm(window, cx);
                });
            }));
            cx.notify();
            return;
        }

        let check = self.panel.read_with(cx, |panel, cx| {
            panel.recheck_delete(&self.target, &self.affected, cx)
        });

        match check {
            Ok(ConfirmationCheck::Proceed) => {}
            Ok(ConfirmationCheck::Missing) => {
                self.missing = true;
                self.notice = None;
                self.error = Some("This item is no longer saved.".to_owned());
                cx.notify();
                return;
            }
            Ok(ConfirmationCheck::Changed {
                target,
                affected,
                notice,
            }) => {
                self.target = target;
                self.affected = affected;
                self.notice = Some(notice);
                self.error = None;
                cx.notify();
                return;
            }
            Err(_) => {
                window.close_dialog(cx);
                return;
            }
        }

        let path = self.target.path.clone();
        let linked = self.target.linked;
        let result = self
            .panel
            .update(cx, |panel, cx| panel.delete_path(&path, window, cx));

        match result {
            Ok(Ok(())) => {
                window.close_dialog(cx);
                let _ = self.panel.update(cx, |panel, cx| {
                    window.focus(&panel.focus, cx);
                });
            }
            Ok(Err(error)) => {
                let verb = if linked { "remove" } else { "delete" };
                self.notice = None;
                self.error = Some(format!("Could not {verb}: {error}"));
                cx.notify();
            }
            Err(_) => {
                window.close_dialog(cx);
            }
        }
    }
}

impl Render for DeleteWarningDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (_, _, heading) = delete_warning_copy(
            self.target.kind,
            &self.target.label,
            self.target.linked,
            &self.affected,
        );

        v_flex()
            .debug_selector(|| "delete-affected-flows-dialog".into())
            .gap_3()
            .when_some(self.notice.clone(), |view, notice| {
                view.child(
                    h_flex()
                        .debug_selector(|| "delete-affected-flows-notice".into())
                        .gap_2()
                        .items_start()
                        .p_2()
                        .rounded(theme.radius_tokens().md)
                        .border_1()
                        .border_color(theme.warning.opacity(0.4))
                        .bg(theme.warning.opacity(0.08))
                        .text_xs()
                        .text_color(theme.warning)
                        .child(Icon::new(IconName::TriangleAlert).size_3p5().flex_none())
                        .child(div().flex_1().min_w_0().child(notice)),
                )
            })
            .when(!self.affected.is_empty(), |view| {
                view.child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .debug_selector(|| "delete-affected-flows-heading".into())
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .child(heading),
                        )
                        .child(
                            div()
                                .border_1()
                                .border_color(theme.border)
                                .rounded(theme.radius_tokens().md)
                                .child(
                                    v_flex()
                                        .id("affected-flows-list")
                                        .max_h(rems(14.))
                                        .overflow_y_scroll()
                                        .p_1()
                                        .gap_px()
                                        .children(self.affected.iter().enumerate().map(
                                            |(index, flow)| {
                                                let name = flow.name.clone();
                                                div()
                                                    .w_full()
                                                    .on_action(cx.listener(
                                                        move |this, _: &Confirm, window, cx| {
                                                            this.inspect_flow(index, window, cx);
                                                        },
                                                    ))
                                                    .child(
                                                        Button::new(("affected-flow", index))
                                                            .debug_selector(move || {
                                                                format!("affected-flow-{index}")
                                                            })
                                                            .ghost()
                                                            .w_full()
                                                            .accessibility_label(format!(
                                                                "Inspect flow {name}"
                                                            ))
                                                            .tooltip(format!(
                                                                "Open {name} to inspect"
                                                            ))
                                                            .child(
                                                                h_flex()
                                                                    .w_full()
                                                                    .min_w_0()
                                                                    .gap_2()
                                                                    .items_center()
                                                                    .child(
                                                                        Icon::default()
                                                                            .path(
                                                                                "icons/workflow.svg",
                                                                            )
                                                                            .size(rems(0.875))
                                                                            .flex_none()
                                                                            .text_color(
                                                                                theme.chart_4,
                                                                            ),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .flex_1()
                                                                            .min_w_0()
                                                                            .text_left()
                                                                            .text_ellipsis()
                                                                            .child(name),
                                                                    )
                                                                    .child(
                                                                        h_flex()
                                                                            .flex_none()
                                                                            .gap_1()
                                                                            .items_center()
                                                                            .text_xs()
                                                                            .text_color(
                                                                                theme
                                                                                    .muted_foreground,
                                                                            )
                                                                            .child("Inspect")
                                                                            .child(
                                                                                Icon::new(
                                                                                    IconName::ChevronRight,
                                                                                )
                                                                                .size_3(),
                                                                            ),
                                                                    ),
                                                            )
                                                            .on_click(cx.listener(
                                                                move |this, _, window, cx| {
                                                                    this.inspect_flow(
                                                                        index, window, cx,
                                                                    );
                                                                },
                                                            )),
                                                    )
                                            },
                                        )),
                                ),
                        ),
                )
            })
            .when_some(self.error.clone(), |view, error| {
                view.child(
                    div()
                        .debug_selector(|| "delete-affected-flows-error".into())
                        .text_sm()
                        .text_color(theme.danger)
                        .child(error),
                )
            })
    }
}
