use super::Environment;
use std::fs;

#[test]
fn saves_and_loads_entries() {
    let path = std::env::temp_dir().join(format!(
        "request-eagle-environment-{}-save.toml",
        std::process::id()
    ));
    let environment = Environment::from_toml(
        &path,
        r#"
            second = "two"
            first = "one"
        "#,
    )
    .unwrap();

    environment.save_file().unwrap();
    let loaded = Environment::from_file(&path).unwrap();

    assert_eq!(loaded.resolve("first"), Some("one"));
    assert_eq!(loaded.resolve("second"), Some("two"));

    fs::remove_file(path).unwrap();
}
