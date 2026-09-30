use crate::{GENERATED_VARIABLES, VariableError, VariableResolver};
use std::collections::HashMap;

#[test]
fn escapes_literal_braces_without_resolving_colliding_names_or_other_sources() {
    let values = HashMap::from([("customer".into(), "resolved customer".into())]);
    let mut resolver = VariableResolver::new(&values);
    assert_eq!(
        resolver
            .resolve("Hello {{!customer}} / {{customer}}")
            .unwrap(),
        "Hello {{customer}} / resolved customer"
    );
    assert_eq!(
        resolver
            .resolve("{{!missing}} {{!$guid}} {{!another}}")
            .unwrap(),
        "{{missing}} {{$guid}} {{another}}"
    );
    assert_eq!(
        resolver
            .resolve("{{! customer }} {{!!name}} {{!{name}}}")
            .unwrap(),
        "{{ customer }} {{!name}} {{{name}}}"
    );
    assert_eq!(
        resolver.resolve("{{!#if customer}}yes{{!/if}}").unwrap(),
        "{{#if customer}}yes{{/if}}"
    );
}

#[test]
fn resolves_multiple_sources_without_recursively_expanding_values() {
    let values = HashMap::from([
        ("host".into(), "https://example.com".into()),
        ("literal".into(), "{{leave_me}}".into()),
    ]);
    let mut resolver = VariableResolver::new(&values);
    assert!(
        resolver
            .resolve("{{ host }}/{{literal}}?id={{$timestamp}}")
            .unwrap()
            .starts_with("https://example.com/{{leave_me}}?id=")
    );
    assert_eq!(
        resolver.resolve("{{missing}}"),
        Err(VariableError::Unknown("missing".into()))
    );
    assert_eq!(resolver.resolve("{{host"), Err(VariableError::Unclosed));
    assert_eq!(
        resolver.resolve("{{another}}"),
        Err(VariableError::Unknown("another".into()))
    );
}

#[test]
fn generated_values_are_valid_and_stable_for_one_send() {
    let values = HashMap::new();
    let mut resolver = VariableResolver::new(&values);
    for (name, _) in GENERATED_VARIABLES {
        let template = format!("{{{{{name}}}}}");
        let first = resolver.resolve(&template).unwrap();
        assert_eq!(first, resolver.resolve(&template).unwrap());
        assert!(!first.is_empty());
    }
    let guid = resolver.resolve("{{$guid}}").unwrap();
    assert_eq!(uuid::Uuid::parse_str(&guid).unwrap().get_version_num(), 4);
    assert_ne!(
        guid,
        VariableResolver::new(&values).resolve("{{$guid}}").unwrap()
    );
    chrono::DateTime::parse_from_rfc3339(&resolver.resolve("{{$isoTimestamp}}").unwrap()).unwrap();
    assert!(
        resolver
            .resolve("{{$randomInt}}")
            .unwrap()
            .parse::<u16>()
            .unwrap()
            <= 1000
    );
}

#[test]
fn generated_values_follow_postman_formats() {
    let values = HashMap::new();
    let mut resolver = VariableResolver::new(&values);
    let timestamp = resolver
        .resolve("{{$timestamp}}")
        .unwrap()
        .parse::<i64>()
        .unwrap();
    assert!((chrono::Utc::now().timestamp() - timestamp).abs() <= 1);
    assert!(
        resolver
            .resolve("{{$isoTimestamp}}")
            .unwrap()
            .ends_with('Z')
    );
    assert!(matches!(
        resolver.resolve("{{$randomBoolean}}").unwrap().as_str(),
        "true" | "false"
    ));
    let character = resolver.resolve("{{$randomAlphaNumeric}}").unwrap();
    assert_eq!(character.len(), 1);
    assert!(character.chars().all(|ch| ch.is_ascii_alphanumeric()));
    assert!(
        resolver
            .resolve("{{$randomEmail}}")
            .unwrap()
            .ends_with("@example.com")
    );
}
