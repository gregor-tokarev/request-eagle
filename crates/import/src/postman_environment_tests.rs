use crate::{Import, ImportedEnvironment, parse};

fn environment(source: &str) -> ImportedEnvironment {
    match parse(source).unwrap() {
        Import::Environment(environment) => environment,
        Import::Collection(import) => panic!("{} is a collection", import.collection.name),
    }
}

#[test]
fn exported_environments_keep_their_enabled_variables() {
    let environment = environment(
        r#"{
            "id": "76fce2d0-8158-4e00-b794-ee952d0bf253",
            "name": "tm:prod",
            "values": [
                {"key": "tm_url", "value": "tm.prod.test:443", "type": "default", "enabled": true},
                {"key": "token", "value": "secret", "type": "secret", "enabled": true},
                {"key": "port", "value": 8080},
                {"key": "old_url", "value": "tm.old.test", "enabled": false},
                {"key": "", "value": "nameless"}
            ],
            "_postman_variable_scope": "environment"
        }"#,
    );

    assert_eq!(environment.name, "tm:prod");
    assert_eq!(environment.variables.len(), 3);
    assert_eq!(environment.variables["tm_url"], "tm.prod.test:443");
    assert_eq!(environment.variables["token"], "secret");
    assert_eq!(environment.variables["port"], "8080");
}

#[test]
fn environment_files_from_git_workspaces_are_read() {
    let environment = environment(
        "name: Local\nvalues:\n  - key: api_url\n    value: http://localhost:8080\n  - key: \
         debug\n    value: 'true'\n    disabled: true\n",
    );

    assert_eq!(environment.name, "Local");
    assert_eq!(environment.variables.len(), 1);
    assert_eq!(environment.variables["api_url"], "http://localhost:8080");
}
