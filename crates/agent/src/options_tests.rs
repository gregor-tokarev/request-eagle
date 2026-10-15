use agent_client_protocol::schema::v1::{
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
};
use preferences::ModelPreference;

use crate::options::{bypass_mode, merge_models, offered_models};

fn menu(
    id: &str,
    category: SessionConfigOptionCategory,
    choices: &[(&str, &str)],
) -> SessionConfigOption {
    SessionConfigOption::select(
        id.to_owned(),
        id.to_owned(),
        choices[0].0.to_owned(),
        choices
            .iter()
            .map(|(value, name)| {
                SessionConfigSelectOption::new(value.to_string(), name.to_string())
            })
            .collect::<Vec<_>>(),
    )
    .category(category)
}

fn model(id: &str, hidden: bool) -> ModelPreference {
    ModelPreference {
        id: id.into(),
        name: id.to_uppercase(),
        description: String::new(),
        hidden,
    }
}

#[test]
fn finds_the_models_an_agent_offers() {
    let options = vec![
        menu(
            "mode",
            SessionConfigOptionCategory::Mode,
            &[("default", "Manual")],
        ),
        menu(
            "model",
            SessionConfigOptionCategory::Model,
            &[("opus", "Opus"), ("haiku", "Haiku")],
        ),
    ];

    let models = offered_models(&options);

    assert_eq!(
        models
            .iter()
            .map(|model| (model.id.as_str(), model.name.as_str()))
            .collect::<Vec<_>>(),
        [("opus", "Opus"), ("haiku", "Haiku")]
    );
}

#[test]
fn known_models_keep_their_place_and_new_ones_join_at_the_end() {
    let known = vec![
        model("haiku", true),
        model("gone", false),
        model("opus", false),
    ];
    let offered = vec![
        model("opus", false),
        model("haiku", false),
        model("fable", false),
    ];

    let merged = merge_models(&known, offered);

    assert_eq!(
        merged
            .iter()
            .map(|model| (model.id.as_str(), model.hidden))
            .collect::<Vec<_>>(),
        [("haiku", true), ("opus", false), ("fable", false)]
    );
}

#[test]
fn finds_the_mode_that_does_not_ask() {
    let claude = vec![menu(
        "mode",
        SessionConfigOptionCategory::Mode,
        &[
            ("default", "Manual"),
            ("acceptEdits", "Accept edits"),
            ("bypassPermissions", "Bypass permissions"),
        ],
    )];
    let codex = vec![menu(
        "mode",
        SessionConfigOptionCategory::Mode,
        &[
            ("read-only", "Read-only"),
            ("agent-full-access", "Full access"),
        ],
    )];
    let opencode = vec![menu(
        "mode",
        SessionConfigOptionCategory::Mode,
        &[("build", "build"), ("plan", "plan")],
    )];

    let value = |options: &[SessionConfigOption]| {
        bypass_mode(options).map(|(option, value)| (option.0.to_string(), value.0.to_string()))
    };
    assert_eq!(
        value(&claude),
        Some(("mode".into(), "bypassPermissions".into()))
    );
    assert_eq!(
        value(&codex),
        Some(("mode".into(), "agent-full-access".into()))
    );
    assert_eq!(value(&opencode), None);
}
