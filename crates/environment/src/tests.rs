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

#[test]
fn saving_replaces_the_file_whole_and_leaves_nothing_else_behind() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("environment.toml");
    fs::write(&path, "token = 'previous'\nremoved = 'gone'\n").unwrap();

    Environment::from_toml(&path, "token = 'next'")
        .unwrap()
        .save_file()
        .unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "token = \"next\"\n");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn a_save_that_fails_keeps_the_previous_variables() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("environment.toml");
    fs::write(&path, "token = 'previous'\n").unwrap();
    let writable = fs::metadata(&path).unwrap().permissions();
    let mut read_only = writable.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&path, read_only).unwrap();

    let result = Environment::from_toml(&path, "token = 'next'")
        .unwrap()
        .save_file();

    fs::set_permissions(&path, writable).unwrap();

    result.unwrap_err();
    assert_eq!(fs::read_to_string(&path).unwrap(), "token = 'previous'\n");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn saving_keeps_the_permissions_of_the_file() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("environment.toml");
    fs::write(&path, "token = 'previous'\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    Environment::from_toml(&path, "token = 'next'")
        .unwrap()
        .save_file()
        .unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn saving_through_a_link_replaces_the_file_it_leads_to() {
    let directory = tempfile::tempdir().unwrap();
    let shared = directory.path().join("shared/variables.toml");
    fs::create_dir(shared.parent().unwrap()).unwrap();
    fs::write(&shared, "token = 'previous'\n").unwrap();
    let link = directory.path().join("environment.toml");
    std::os::unix::fs::symlink(&shared, &link).unwrap();

    Environment::from_toml(&link, "token = 'next'")
        .unwrap()
        .save_file()
        .unwrap();

    assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(&shared).unwrap(), "token = \"next\"\n");
    assert_eq!(fs::read_dir(shared.parent().unwrap()).unwrap().count(), 1);
}
