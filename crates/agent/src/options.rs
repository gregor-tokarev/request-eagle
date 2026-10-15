//! What an agent's session options say: the models it offers, the exact
//! model and reasoning level it runs, and the mode in which it runs tools
//! without asking.

use agent_client_protocol::schema::v1::{
    SessionConfigId, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOption, SessionConfigSelectOptions, SessionConfigValueId,
};
use preferences::ModelPreference;

/// The choices of a menu option, groups flattened.
pub fn choices(option: &SessionConfigOption) -> Vec<&SessionConfigSelectOption> {
    let SessionConfigKind::Select(select) = &option.kind else {
        return Vec::new();
    };

    match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .collect(),
        _ => Vec::new(),
    }
}

/// The models the agent offers, as its model menu lists them.
pub fn offered_models(options: &[SessionConfigOption]) -> Vec<ModelPreference> {
    options
        .iter()
        .filter(|option| option.category == Some(SessionConfigOptionCategory::Model))
        .flat_map(|option| {
            choices(option)
                .into_iter()
                .filter(|choice| !is_unspecified(option, &choice.value.0))
        })
        .map(|choice| ModelPreference {
            id: choice.value.0.to_string(),
            name: model_name(&choice.value.0, &choice.name),
            description: choice.description.clone().unwrap_or_default(),
            hidden: false,
        })
        .collect()
}

/// A model's name as its maker gives it. Codex's adapter drops GPT from
/// its models' names, such as 6.1 Sol for GPT-6.1-Sol, so it comes back.
pub fn model_name(id: &str, name: &str) -> String {
    let gpt = |text: &str| {
        text.get(..3)
            .is_some_and(|start| start.eq_ignore_ascii_case("gpt"))
    };
    if gpt(id) && !gpt(name) {
        format!("GPT-{name}")
    } else {
        name.to_owned()
    }
}

/// Whether a model or reasoning level choice leaves it to the agent, so it
/// does not say what runs, like Claude Code's Default (recommended) model
/// and Default effort.
pub fn is_unspecified(option: &SessionConfigOption, value: &str) -> bool {
    value == "default"
        && matches!(
            option.category,
            Some(SessionConfigOptionCategory::Model | SessionConfigOptionCategory::ThoughtLevel)
        )
}

/// The exact choice for a model or reasoning level the agent leaves to
/// itself: the model its default names, and medium reasoning, as Claude
/// Code's adapter recommends, or else the first level it offers.
pub fn exact_choice(option: &SessionConfigOption) -> Option<SessionConfigValueId> {
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    if !is_unspecified(option, &select.current_value.0) {
        return None;
    }

    let choices = choices(option);
    let exact: Vec<_> = choices
        .iter()
        .filter(|choice| !is_unspecified(option, &choice.value.0))
        .collect();
    let choice = if option.category == Some(SessionConfigOptionCategory::Model) {
        let named = choices
            .iter()
            .find(|choice| choice.value == select.current_value)?
            .description
            .as_deref()?;
        exact.into_iter().find(|choice| choice.name == named)?
    } else {
        exact
            .iter()
            .find(|choice| choice.value.0.as_ref() == "medium")
            .or(exact.first())?
    };

    Some(choice.value.clone())
}

/// The models `known` from before, with what the agent offers now: new
/// models join at the end, shown, and those it no longer offers leave. The
/// others keep their place and whether they are hidden.
pub fn merge_models(
    known: &[ModelPreference],
    offered: Vec<ModelPreference>,
) -> Vec<ModelPreference> {
    let mut merged: Vec<ModelPreference> = known
        .iter()
        .filter_map(|model| {
            let latest = offered.iter().find(|offered| offered.id == model.id)?;
            Some(ModelPreference {
                hidden: model.hidden,
                ..latest.clone()
            })
        })
        .collect();
    merged.extend(
        offered
            .into_iter()
            .filter(|model| known.iter().all(|known| known.id != model.id)),
    );

    merged
}

/// The agent's mode option and its value that runs tools without asking,
/// such as Claude Code's Bypass permissions or Codex's Full access.
pub fn bypass_mode(
    options: &[SessionConfigOption],
) -> Option<(SessionConfigId, SessionConfigValueId)> {
    options
        .iter()
        .filter(|option| option.category == Some(SessionConfigOptionCategory::Mode))
        .find_map(|option| {
            choices(option)
                .into_iter()
                .find(|choice| is_bypass(&choice.value.0, &choice.name))
                .map(|choice| (option.id.clone(), choice.value.clone()))
        })
}

/// The agent's mode option and a value of it that asks before tools run,
/// when the mode it is in does not ask.
pub fn prompting_mode(
    options: &[SessionConfigOption],
) -> Option<(SessionConfigId, SessionConfigValueId)> {
    options
        .iter()
        .filter(|option| option.category == Some(SessionConfigOptionCategory::Mode))
        .find_map(|option| {
            let SessionConfigKind::Select(select) = &option.kind else {
                return None;
            };
            let choices = choices(option);
            let current = choices
                .iter()
                .find(|choice| choice.value == select.current_value)?;
            if !is_bypass(&current.value.0, &current.name) {
                return None;
            }

            choices
                .iter()
                .find(|choice| !is_bypass(&choice.value.0, &choice.name))
                .map(|choice| (option.id.clone(), choice.value.clone()))
        })
}

/// Whether a mode runs tools without asking.
pub fn is_bypass(value: &str, name: &str) -> bool {
    let name = name.to_lowercase();
    matches!(value, "bypassPermissions" | "agent-full-access")
        || name.contains("bypass")
        || name == "full access"
}
