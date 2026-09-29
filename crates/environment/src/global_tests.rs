use std::{collections::HashMap, fs};

use crate::{Environment, GlobalEnvironmentError, GlobalEnvironments};

#[test]
fn a_missing_directory_has_no_environments() {
    let directory = tempfile::tempdir().unwrap();
    let environments = GlobalEnvironments::new(directory.path().join("environments"));

    assert!(environments.names().unwrap().is_empty());
}

#[test]
fn creates_unique_names_and_lists_them_in_order() {
    let directory = tempfile::tempdir().unwrap();
    let environments = GlobalEnvironments::new(directory.path().join("environments"));

    assert_eq!(
        environments.create("New Environment").unwrap(),
        "New Environment"
    );
    assert_eq!(
        environments.create("New Environment").unwrap(),
        "New Environment 2"
    );
    assert_eq!(environments.create("alpha").unwrap(), "alpha");
    fs::write(directory.path().join("environments/notes.txt"), "").unwrap();
    fs::create_dir(directory.path().join("environments/folder.toml")).unwrap();

    assert_eq!(
        environments.names().unwrap(),
        ["alpha", "New Environment", "New Environment 2"]
    );
    assert!(matches!(
        environments.create("../escape"),
        Err(GlobalEnvironmentError::InvalidName)
    ));
}

#[test]
fn renames_without_replacing_another_environment() {
    let directory = tempfile::tempdir().unwrap();
    let environments = GlobalEnvironments::new(directory.path());
    environments.create("Staging").unwrap();
    environments.create("Production").unwrap();
    Environment {
        path: environments.path("Staging"),
        entries: HashMap::from([("host".into(), "staging.example.com".into())]),
    }
    .save_file()
    .unwrap();

    assert!(matches!(
        environments.rename("Staging", "Production"),
        Err(GlobalEnvironmentError::AlreadyExists)
    ));
    assert!(matches!(
        environments.rename("Staging", " "),
        Err(GlobalEnvironmentError::InvalidName)
    ));

    assert_eq!(environments.rename("Staging", " QA ").unwrap(), "QA");
    assert_eq!(environments.names().unwrap(), ["Production", "QA"]);
    assert_eq!(
        Environment::from_file(environments.path("QA"))
            .unwrap()
            .resolve("host"),
        Some("staging.example.com")
    );

    environments.delete("QA").unwrap();
    environments.delete("QA").unwrap();
    assert_eq!(environments.names().unwrap(), ["Production"]);
}
