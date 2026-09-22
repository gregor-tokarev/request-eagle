use std::{collections::HashMap, time::SystemTime};

use super::Collection;
use crate::{Entry, Method, Request};
use environment::Environment;
use request::{FormBody, MultipartField};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn test_directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "request-eagle-collection-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn environment(path: &Path) -> Environment {
    Environment {
        path: path.to_path_buf(),
        entries: HashMap::new(),
    }
}

#[test]
fn loads_edits_and_saves_a_directory_without_losing_user_content() {
    let root = test_directory();
    let nested = root.join("users");
    let request_path = nested.join("list.toml");

    fs::create_dir_all(&nested).unwrap();
    fs::write(
        &request_path,
        r#"# user's request
id = "list-users"
name = "List users"
schema_version = 1
custom = "keep me"

[request]
type = "http"
method = "GET"
path = "/users"
headers = [["Accept", "application/json"]]
request_custom = "keep me too"
"#,
    )
    .unwrap();
    fs::write(root.join("environment.toml"), "base_url = \"local\"\n").unwrap();

    let mut collection =
        Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
    let Entry::Directory(users) = &mut collection.entries[0] else {
        panic!("expected users directory");
    };
    let Entry::File(request) = &mut users.entries[0] else {
        panic!("expected request file");
    };
    let Request::Http(request) = &mut request.request;

    assert!(matches!(request.method, Method::Get));

    request.path = "/v2/users".into();

    collection.save_files().unwrap();

    let saved = fs::read_to_string(&request_path).unwrap();
    assert!(saved.contains("# user's request"));
    assert!(saved.contains("custom = \"keep me\""));
    assert!(saved.contains("request_custom = \"keep me too\""));
    assert!(saved.contains("path = \"/v2/users\""));

    let reloaded =
        Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
    let Entry::Directory(users) = &reloaded.entries[0] else {
        panic!("expected users directory");
    };
    let Entry::File(request) = &users.entries[0] else {
        panic!("expected request file");
    };
    let Request::Http(request) = &request.request;

    assert_eq!(request.path, "/v2/users");

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn saves_and_reloads_form_bodies_with_new_http_methods() {
    let root = test_directory();
    let request_path = root.join("form.toml");
    let upload_path = root.join("binary upload.bin");

    fs::create_dir_all(&root).unwrap();
    fs::write(&upload_path, [0, 255, 42]).unwrap();
    fs::write(
        &request_path,
        r#"id = "form-request"
name = "Form request"
schema_version = 1

[request]
type = "http"
method = "POST"
path = "https://example.com/upload"
headers = [["Accept", "application/json"]]
body = [0, 255, 42]
"#,
    )
    .unwrap();

    let mut collection =
        Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();

    for method in [Method::Patch, Method::Head, Method::Options] {
        for form in [
            FormBody::UrlEncoded(vec![
                ("tag".into(), "first value".into()),
                ("tag".into(), "東京 & more".into()),
            ]),
            FormBody::Multipart(vec![
                MultipartField::Text {
                    name: "tag".into(),
                    value: "first value".into(),
                },
                MultipartField::Text {
                    name: "tag".into(),
                    value: "東京 & more".into(),
                },
                MultipartField::File {
                    name: "attachment".into(),
                    path: upload_path.clone(),
                },
            ]),
        ] {
            let Entry::File(entry) = &mut collection.entries[0] else {
                panic!("expected request file");
            };
            let Request::Http(request) = &mut entry.request;
            request.method = method;
            request.form = Some(form);
            let expected = request.clone();

            collection.save_files().unwrap();

            collection =
                Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
            let Entry::File(entry) = &collection.entries[0] else {
                panic!("expected request file");
            };
            let Request::Http(request) = &entry.request;

            assert_eq!(request, &expected);
            assert_eq!(fs::read(&upload_path).unwrap(), [0, 255, 42]);
        }
    }

    let Entry::File(entry) = &mut collection.entries[0] else {
        panic!("expected request file");
    };
    let Request::Http(request) = &mut entry.request;
    request.form = None;
    collection.save_files().unwrap();

    let reloaded =
        Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
    let Entry::File(entry) = &reloaded.entries[0] else {
        panic!("expected request file");
    };
    let Request::Http(request) = &entry.request;
    assert!(
        request.form.is_none(),
        "switching back to raw removes the saved form"
    );
    assert_eq!(request.body, Some(vec![0, 255, 42]));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeatedly_saves_multipart_fields_in_an_inline_request_without_losing_user_content() {
    let root = test_directory();
    let request_path = root.join("inline.toml");
    let upload_path = root.join("file upload.bin");

    fs::create_dir_all(&root).unwrap();
    fs::write(&upload_path, [0, 255, 42]).unwrap();
    fs::write(
        &request_path,
        r#"# user's inline request
id = 'inline-request'
name = 'Inline request'
schema_version = 1
custom = 'keep me' # root annotation

request = { type = 'http', method = 'POST', path = 'https://example.com', headers = [], request_custom = 'keep me too' } # request annotation

[metadata]
# unrelated table annotation
owner = 'request author'
"#,
    )
    .unwrap();

    let mut collection =
        Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
    let Entry::File(entry) = &mut collection.entries[0] else {
        panic!("expected request file");
    };
    let Request::Http(request) = &mut entry.request;
    request.form = Some(FormBody::Multipart(vec![
        MultipartField::Text {
            name: "description".into(),
            value: "first line\n東京 & more".into(),
        },
        MultipartField::File {
            name: "attachment".into(),
            path: upload_path.clone(),
        },
    ]));
    let expected = request.clone();

    collection.save_files().unwrap();
    collection.save_files().unwrap();

    let saved = fs::read_to_string(&request_path).unwrap();
    for content in [
        "# user's inline request",
        "custom = 'keep me' # root annotation",
        "request_custom = 'keep me too'",
        "# request annotation",
        "# unrelated table annotation",
        "owner = 'request author'",
    ] {
        assert!(saved.contains(content), "missing {content:?} in {saved}");
    }

    let reloaded =
        Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
    let Entry::File(entry) = &reloaded.entries[0] else {
        panic!("expected request file");
    };
    let Request::Http(request) = &entry.request;

    assert_eq!(request, &expected);
    assert_eq!(fs::read(&upload_path).unwrap(), [0, 255, 42]);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeatedly_saves_mode_changes_and_clearing_for_existing_inline_forms() {
    for request_source in [
        "request = { type = 'http', method = 'POST', path = 'https://example.com', headers = [], request_custom = 'keep me too', form = { type = 'url_encoded', fields = [['tag', 'initial']] } } # form annotation\n",
        "[request]\ntype = 'http'\nmethod = 'POST'\npath = 'https://example.com'\nheaders = []\nrequest_custom = 'keep me too'\nform = { type = 'url_encoded', fields = [['tag', 'initial']] } # form annotation\n",
    ] {
        let root = test_directory();
        let request_path = root.join("inline-form.toml");
        let upload_path = root.join("upload.bin");

        fs::create_dir_all(&root).unwrap();
        fs::write(&upload_path, [1, 2, 3]).unwrap();
        fs::write(
            &request_path,
            format!(
                "# user's inline form\nid = 'inline-form'\nname = 'Inline form'\nschema_version = 1\ncustom = 'keep me' # root annotation\n\n{request_source}\n[metadata]\n# unrelated table annotation\nowner = 'request author'\n"
            ),
        )
        .unwrap();

        let mut collection =
            Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();

        for form in [
            Some(FormBody::UrlEncoded(vec![("tag".into(), "initial".into())])),
            Some(FormBody::Multipart(vec![
                MultipartField::Text {
                    name: "tag".into(),
                    value: "first\nsecond".into(),
                },
                MultipartField::File {
                    name: "attachment".into(),
                    path: upload_path.clone(),
                },
            ])),
            Some(FormBody::UrlEncoded(vec![
                ("tag".into(), "updated & value".into()),
                ("tag".into(), "東京".into()),
            ])),
            None,
        ] {
            let Entry::File(entry) = &mut collection.entries[0] else {
                panic!("expected request file");
            };
            let Request::Http(request) = &mut entry.request;
            request.form = form;
            let expected = request.clone();

            collection.save_files().unwrap();
            collection.save_files().unwrap();

            let saved = fs::read_to_string(&request_path).unwrap();
            for content in [
                "# user's inline form",
                "custom = 'keep me' # root annotation",
                "request_custom = 'keep me too'",
                "# unrelated table annotation",
                "owner = 'request author'",
            ] {
                assert!(saved.contains(content), "missing {content:?} in {saved}");
            }

            if expected.form.is_some() {
                assert!(saved.contains("# form annotation"), "{saved}");
            }

            collection =
                Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
            let Entry::File(entry) = &collection.entries[0] else {
                panic!("expected request file");
            };
            let Request::Http(request) = &entry.request;

            assert_eq!(request, &expected);
            assert_eq!(fs::read(&upload_path).unwrap(), [1, 2, 3]);
        }

        fs::remove_dir_all(root).unwrap();
    }
}
