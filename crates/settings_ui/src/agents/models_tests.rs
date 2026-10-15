use preferences::ModelPreference;

use super::models::{move_model, set_hidden};

fn models(spec: &[(&str, bool)]) -> Vec<ModelPreference> {
    spec.iter()
        .map(|(id, hidden)| ModelPreference {
            id: (*id).to_owned(),
            name: id.to_uppercase(),
            description: String::new(),
            hidden: *hidden,
        })
        .collect()
}

fn order(models: &[ModelPreference]) -> Vec<&str> {
    models.iter().map(|model| model.id.as_str()).collect()
}

#[test]
fn a_shown_model_moves_past_the_shown_model_beside_it() {
    let mut list = models(&[
        ("opus", false),
        ("old", true),
        ("sonnet", false),
        ("haiku", false),
    ]);

    move_model(&mut list, "sonnet", true);
    assert_eq!(order(&list), ["sonnet", "old", "opus", "haiku"]);

    move_model(&mut list, "sonnet", true);
    assert_eq!(order(&list), ["sonnet", "old", "opus", "haiku"]);

    move_model(&mut list, "opus", false);
    assert_eq!(order(&list), ["sonnet", "old", "haiku", "opus"]);
}

#[test]
fn hiding_a_model_keeps_its_place() {
    let mut list = models(&[("opus", false), ("sonnet", false)]);

    set_hidden(&mut list, "opus", true);

    assert_eq!(order(&list), ["opus", "sonnet"]);
    assert!(list[0].hidden);
    assert!(!list[1].hidden);
}
