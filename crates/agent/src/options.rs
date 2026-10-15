//! What an agent's session options say: the models it offers and the mode
//! in which it runs tools without asking.

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
        .flat_map(choices)
        .map(|choice| ModelPreference {
            id: choice.value.0.to_string(),
            name: choice.name.clone(),
            description: choice.description.clone().unwrap_or_default(),
            hidden: false,
        })
        .collect()
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

/// Whether a mode runs tools without asking.
pub fn is_bypass(value: &str, name: &str) -> bool {
    let name = name.to_lowercase();
    matches!(value, "bypassPermissions" | "agent-full-access")
        || name.contains("bypass")
        || name == "full access"
}
