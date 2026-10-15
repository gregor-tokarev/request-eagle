use agent_client_protocol::schema::v1::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
    SessionConfigSelectOptions,
};
use preferences::ModelPreference;

use crate::options::{
    bypass_mode, exact_choice, is_unspecified, merge_models, model_name, offered_models,
    prompting_mode,
};

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

fn currently(mut option: SessionConfigOption, value: &str) -> SessionConfigOption {
    if let SessionConfigKind::Select(select) = &mut option.kind {
        select.current_value = value.to_owned().into();
    }
    option
}

/// Claude Code's model menu, whose default names the model it stands for.
fn claude_models() -> SessionConfigOption {
    let mut option = menu(
        "model",
        SessionConfigOptionCategory::Model,
        &[
            ("default", "Default (recommended)"),
            ("opus", "Opus 5.5"),
            ("sonnet", "Sonnet 5.5"),
        ],
    );
    if let SessionConfigKind::Select(select) = &mut option.kind
        && let SessionConfigSelectOptions::Ungrouped(choices) = &mut select.options
    {
        choices[0].description = Some("Opus 5.5".into());
    }
    option
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

#[test]
fn finds_a_mode_that_asks_when_the_agent_starts_in_one_that_does_not() {
    let starting_in = |current: &str| {
        let mut option = menu(
            "mode",
            SessionConfigOptionCategory::Mode,
            &[
                ("default", "Manual"),
                ("bypassPermissions", "Bypass permissions"),
            ],
        );
        if let agent_client_protocol::schema::v1::SessionConfigKind::Select(select) =
            &mut option.kind
        {
            select.current_value = current.to_owned().into();
        }
        prompting_mode(&[option]).map(|(option, value)| (option.0.to_string(), value.0.to_string()))
    };

    assert_eq!(
        starting_in("bypassPermissions"),
        Some(("mode".into(), "default".into()))
    );
    assert_eq!(starting_in("default"), None);
}

#[test]
fn a_default_model_is_not_offered() {
    let models = offered_models(&[claude_models()]);

    assert_eq!(
        models
            .iter()
            .map(|model| model.name.as_str())
            .collect::<Vec<_>>(),
        ["Opus 5.5", "Sonnet 5.5"]
    );
}

#[test]
fn finds_the_model_a_default_stands_for() {
    let exact = |option| exact_choice(&option).map(|value| value.0.to_string());

    assert_eq!(exact(claude_models()), Some("opus".into()));
    assert_eq!(exact(currently(claude_models(), "sonnet")), None);
}

#[test]
fn finds_an_exact_reasoning_level_for_a_default_one() {
    let effort = |choices: &[(&str, &str)]| {
        let option = menu("effort", SessionConfigOptionCategory::ThoughtLevel, choices);
        exact_choice(&currently(option, "default")).map(|value| value.0.to_string())
    };

    assert_eq!(
        effort(&[
            ("default", "Default"),
            ("low", "Low"),
            ("medium", "Medium"),
            ("high", "High"),
        ]),
        Some("medium".into())
    );
    assert_eq!(
        effort(&[("max", "Max"), ("default", "Default")]),
        Some("max".into())
    );
}

#[test]
fn a_default_mode_is_a_mode_of_its_own() {
    let mode = menu(
        "mode",
        SessionConfigOptionCategory::Mode,
        &[("default", "Manual"), ("plan", "Plan")],
    );

    assert!(!is_unspecified(&mode, "default"));
    assert_eq!(exact_choice(&mode), None);
}

#[test]
fn gpt_models_keep_gpt_in_their_names() {
    assert_eq!(model_name("gpt-6.1-sol", "6.1 Sol"), "GPT-6.1 Sol");
    assert_eq!(
        model_name("opencode-go/gpt-6-luna", "OpenCode Go/GPT-6 Luna"),
        "OpenCode Go/GPT-6 Luna"
    );
    assert_eq!(model_name("opus", "Opus 5.5"), "Opus 5.5");
}
