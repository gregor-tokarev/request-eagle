use gpui_kit::component::{button::*, switch::Switch, *};
use gpui_kit::{prelude::*, *};
use preferences::ModelPreference;

use super::page::AgentSettings;
use crate::layout::section;

/// Move a model above the shown model before it, or below the one after
/// it. Hidden models keep their places.
pub(super) fn move_model(models: &mut [ModelPreference], id: &str, up: bool) {
    let Some(index) = models.iter().position(|model| model.id == id) else {
        return;
    };
    let hidden = models[index].hidden;
    let neighbour = if up {
        models[..index]
            .iter()
            .rposition(|model| model.hidden == hidden)
    } else {
        models[index + 1..]
            .iter()
            .position(|model| model.hidden == hidden)
            .map(|offset| index + 1 + offset)
    };

    if let Some(neighbour) = neighbour {
        models.swap(index, neighbour);
    }
}

pub(super) fn set_hidden(models: &mut [ModelPreference], id: &str, hidden: bool) {
    if let Some(model) = models.iter_mut().find(|model| model.id == id) {
        model.hidden = hidden;
    }
}

impl AgentSettings {
    pub(super) fn models(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let agent = self.selected;
        let models = self.provider(cx).models;
        let hidden = models.iter().filter(|model| model.hidden).count();
        let shown: Vec<&ModelPreference> = models.iter().filter(|model| !model.hidden).collect();
        let last = shown.len().saturating_sub(1);
        let shown_rows: Vec<AnyElement> = shown
            .iter()
            .enumerate()
            .map(|(position, model)| self.model_row(model, position, last, cx))
            .collect();
        let hidden_rows: Vec<AnyElement> = models
            .iter()
            .filter(|model| model.hidden)
            .map(|model| self.model_row(model, 0, 0, cx))
            .collect();
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;

        let body = if models.is_empty() {
            div()
                .text_color(muted)
                .child(format!(
                    "{}'s models show here once it has started, here or in the agent panel.",
                    agent.name
                ))
                .into_any_element()
        } else {
            v_flex()
                .w_full()
                .gap_1()
                .child(
                    h_flex()
                        .gap_3()
                        .pb_1()
                        .child(
                            div()
                                .flex_1()
                                .text_color(muted)
                                .child(format!("{} models · {hidden} hidden", models.len())),
                        )
                        .child(
                            Button::new("agent-models-toggle-all")
                                .debug_selector(|| "agent-models-toggle-all".into())
                                .ghost()
                                .small()
                                .label(if hidden > 0 { "Show All" } else { "Hide All" })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.update_provider(cx, |provider| {
                                        for model in &mut provider.models {
                                            model.hidden = hidden == 0;
                                        }
                                    });
                                })),
                        ),
                )
                .children(shown_rows)
                .when(hidden > 0, |this| {
                    this.child(
                        div()
                            .pt_3()
                            .pb_1()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(muted)
                            .child("Hidden from the model menu"),
                    )
                    .children(hidden_rows)
                })
                .into_any_element()
        };

        section("Models").child(
            v_flex()
                .w_full()
                .py_4()
                .gap_3()
                .border_t_1()
                .border_color(border)
                .child(div().text_color(muted).child(format!(
                    "The models {} offers, in the order the panel's model menu lists them. \
                     Hidden ones stay out of the menu.",
                    agent.name
                )))
                .child(body),
        )
    }

    /// A model with its place in the menu and whether the menu shows it.
    /// `position` and `last` place a shown model among the shown ones.
    fn model_row(
        &self,
        model: &ModelPreference,
        position: usize,
        last: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let id = model.id.clone();
        let shown = !model.hidden;
        let reorder = |up: bool| {
            let id = id.clone();
            Button::new(SharedString::from(format!(
                "agent-model-{}-{}",
                if up { "up" } else { "down" },
                model.id
            )))
            .ghost()
            .xsmall()
            .icon(if up {
                IconName::ArrowUp
            } else {
                IconName::ArrowDown
            })
            .accessibility_label(if up { "Move up" } else { "Move down" })
            .disabled(if up { position == 0 } else { position == last })
            .on_click(cx.listener(move |this, _, _, cx| {
                let id = id.clone();
                this.update_provider(cx, |provider| move_model(&mut provider.models, &id, up));
            }))
        };

        h_flex()
            .id(SharedString::from(format!("agent-model-{}", model.id)))
            .w_full()
            .min_h_8()
            .gap_3()
            .when(!shown, |this| this.text_color(theme.muted_foreground))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(div().flex_none().child(model.name.clone()))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .font_family(theme.mono_font_family.clone())
                            .text_color(theme.muted_foreground)
                            .child(model.id.clone()),
                    ),
            )
            .when(!model.description.is_empty(), |this| {
                this.child(
                    div()
                        .w(rems(14.))
                        .flex_shrink(1.)
                        .min_w_0()
                        .text_ellipsis()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(model.description.clone()),
                )
            })
            // A hidden model keeps its place, so its row lines up without
            // the buttons that move it.
            .child(
                h_flex()
                    .flex_none()
                    .when(!shown, |this| this.invisible())
                    .child(reorder(true))
                    .child(reorder(false)),
            )
            .child(
                Switch::new(SharedString::from(format!(
                    "agent-model-shown-{}",
                    model.id
                )))
                .accessibility_label(format!("Show {} in the model menu", model.name))
                .checked(shown)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                    let checked = *checked;
                    let id = id.clone();
                    this.update_provider(cx, |provider| {
                        set_hidden(&mut provider.models, &id, !checked)
                    });
                })),
            )
            .into_any_element()
    }
}
