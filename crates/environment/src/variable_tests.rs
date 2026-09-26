use crate::{GENERATED_VARIABLES, VariableError, VariableResolver, VariableValues};

#[test]
fn escapes_literal_braces_without_resolving_colliding_names_or_other_sources() {
    let values = VariableValues {
        environment: [("customer".into(), "resolved customer".into())].into(),
        ..Default::default()
    };
    let mut resolver = VariableResolver::new(&values);
    assert_eq!(
        resolver
            .resolve("Hello {{!customer}} / {{customer}}")
            .unwrap(),
        "Hello {{customer}} / resolved customer"
    );
    assert_eq!(
        resolver
            .resolve("{{!missing}} {{!$guid}} {{!vault:missing}}")
            .unwrap(),
        "{{missing}} {{$guid}} {{vault:missing}}"
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
    let values = VariableValues {
        environment: [
            ("host".into(), "https://example.com".into()),
            ("literal".into(), "{{leave_me}}".into()),
        ]
        .into(),
        secrets: [("token".into(), "test-secret".into())].into(),
    };
    let mut resolver = VariableResolver::new(&values);
    assert_eq!(
        resolver
            .resolve("{{ host }}/{{literal}}?auth={{vault:token}}")
            .unwrap(),
        "https://example.com/{{leave_me}}?auth=test-secret"
    );
    assert_eq!(
        resolver.resolve("{{missing}}"),
        Err(VariableError::Unknown("missing".into()))
    );
    assert_eq!(resolver.resolve("{{host"), Err(VariableError::Unclosed));
    assert_eq!(
        resolver.resolve("{{vault:missing}}"),
        Err(VariableError::Unknown("vault:missing".into()))
    );
}

#[test]
fn generated_values_are_valid_and_stable_for_one_send() {
    let values = VariableValues::default();
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
