use std::collections::HashMap;

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
        uuid::Uuid::new_v4()
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

#[test]
fn repeated_noop_saves_preserve_multipart_field_comments_and_custom_keys() {
    let root = test_directory();
    let request_path = root.join("multipart.toml");

    fs::create_dir_all(&root).unwrap();
    fs::write(
        &request_path,
        r#"id = 'multipart-metadata'
name = 'Upload'
schema_version = 1

[request]
type = 'http'
method = 'POST'
path = 'https://example.com/upload'
headers = []

[request.form]
type = 'multipart'

[[request.form.fields]]
# token required by the upload service
type = 'text'
name = 'token'
value = 'abc' # fixture value
custom = 'text metadata'

[[request.form.fields]]
# attachment required by the upload service
type = 'file'
name = 'attachment'
path = 'upload.bin' # fixture file
custom = 'file metadata'
"#,
    )
    .unwrap();

    let mut collection =
        Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
    let Entry::File(entry) = &collection.entries[0] else {
        panic!("expected request file");
    };
    let Request::Http(expected) = entry.request.clone();

    for _ in 0..2 {
        collection.save_files().unwrap();

        let saved = fs::read_to_string(&request_path).unwrap();
        for content in [
            "# token required by the upload service",
            "value = 'abc' # fixture value",
            "custom = 'text metadata'",
            "# attachment required by the upload service",
            "path = 'upload.bin' # fixture file",
            "custom = 'file metadata'",
        ] {
            assert!(saved.contains(content), "missing {content:?} in {saved}");
        }

        collection =
            Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
        let Entry::File(entry) = &collection.entries[0] else {
            panic!("expected request file");
        };
        let Request::Http(request) = &entry.request;

        assert_eq!(request, &expected);
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn multipart_edits_and_reordering_keep_metadata_with_retained_fields() {
    let root = test_directory();
    let request_path = root.join("multipart.toml");

    fs::create_dir_all(&root).unwrap();
    fs::write(
        &request_path,
        r#"# user's multipart request
id = 'multipart-reordering'
name = 'Upload'
schema_version = 1
custom = 'request metadata'

[request]
type = 'http'
method = 'POST'
path = 'https://example.com/upload'
headers = []

[request.form]
type = 'multipart'

[[request.form.fields]]
# first tag annotation
type = 'text'
name = 'tag'
value = 'first' # first value annotation
custom = 'first metadata'

[request.form.fields.notes]
# first nested annotation
owner = 'first owner'

[[request.form.fields]]
# second tag annotation
type = 'text'
name = 'tag'
value = 'second' # second value annotation
custom = 'second metadata'

[[request.form.fields]]
# file tag annotation
type = 'file'
name = 'tag'
path = 'original.bin' # file path annotation
custom = 'file metadata'

[[request.form.fields]]
# removed field annotation
type = 'text'
name = 'removed'
value = 'gone'
custom = 'removed metadata'

[[request.form.fields]]
# replaced kind annotation
type = 'file'
name = 'replacement'
path = 'removed.bin'
custom = 'replaced kind metadata'
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
            name: "tag".into(),
            value: "second updated".into(),
        },
        MultipartField::File {
            name: "tag".into(),
            path: "updated.bin".into(),
        },
        // The unchanged duplicate appears after the edited duplicate. Its exact
        // match must be reserved before matching the edited field by name.
        MultipartField::Text {
            name: "tag".into(),
            value: "first".into(),
        },
        MultipartField::Text {
            name: "replacement".into(),
            value: "new text field".into(),
        },
        MultipartField::Text {
            name: "unrelated".into(),
            value: "new field".into(),
        },
    ]));
    let expected = request.clone();

    for _ in 0..2 {
        collection.save_files().unwrap();

        let saved = fs::read_to_string(&request_path).unwrap();
        let document: toml::Value = toml::from_str(&saved).unwrap();
        let fields = document["request"]["form"]["fields"].as_array().unwrap();
        let blocks: Vec<_> = saved.split("[[request.form.fields]]").skip(1).collect();

        assert_eq!(fields.len(), 5);
        assert_eq!(blocks.len(), 5);
        assert_eq!(fields[0]["custom"].as_str(), Some("second metadata"));
        assert_eq!(fields[1]["custom"].as_str(), Some("file metadata"));
        assert_eq!(fields[2]["custom"].as_str(), Some("first metadata"));
        assert_eq!(fields[2]["notes"]["owner"].as_str(), Some("first owner"));
        assert!(fields[0].get("notes").is_none());
        assert!(fields[1].get("notes").is_none());
        assert!(fields[3].get("custom").is_none());
        assert!(fields[4].get("custom").is_none());
        assert!(blocks[0].contains("# second tag annotation"));
        assert!(blocks[0].contains("# second value annotation"));
        assert!(blocks[1].contains("# file tag annotation"));
        assert!(blocks[1].contains("# file path annotation"));
        assert!(blocks[2].contains("# first tag annotation"));
        assert!(blocks[2].contains("# first value annotation"));
        assert!(blocks[2].contains("# first nested annotation"));
        assert!(!blocks[3].contains("annotation"));
        assert!(!blocks[4].contains("annotation"));
        assert!(!saved.contains("removed field annotation"));
        assert!(!saved.contains("removed metadata"));
        assert!(!saved.contains("replaced kind annotation"));
        assert!(!saved.contains("replaced kind metadata"));
        assert!(saved.contains("# user's multipart request"));
        assert!(saved.contains("custom = 'request metadata'"));

        collection =
            Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
        let Entry::File(entry) = &collection.entries[0] else {
            panic!("expected request file");
        };
        let Request::Http(request) = &entry.request;

        assert_eq!(request, &expected);
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inline_multipart_reordering_preserves_row_metadata_and_post_comma_comments() {
    let root = test_directory();
    let request_path = root.join("inline-fields.toml");

    fs::create_dir_all(&root).unwrap();
    fs::write(
        &request_path,
        r#"id = 'inline-fields'
name = 'Upload'
schema_version = 1

[request]
type = 'http'
method = 'POST'
path = 'https://example.com/upload'
headers = []

[request.form]
type = 'multipart'
fields = [
  { type = 'text', name = 'tag', value = 'first', custom = 'first metadata' }, # first row
  { type = 'text', name = 'tag', value = 'second', custom = 'second metadata' }, # second row
  { type = 'file', name = 'replacement', path = 'old.bin', custom = 'file metadata' }, # old file row
  { type = 'text', name = 'removed', value = 'gone', custom = 'removed metadata' }, # removed row
]
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
            name: "tag".into(),
            value: "second edited".into(),
        },
        MultipartField::Text {
            name: "tag".into(),
            value: "first".into(),
        },
        MultipartField::Text {
            name: "replacement".into(),
            value: "new text field".into(),
        },
    ]));
    let expected = request.clone();

    for _ in 0..2 {
        collection.save_files().unwrap();

        let saved = fs::read_to_string(&request_path).unwrap();
        let document: toml::Value = toml::from_str(&saved).unwrap();
        let fields = document["request"]["form"]["fields"].as_array().unwrap();

        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0]["custom"].as_str(), Some("second metadata"));
        assert_eq!(fields[1]["custom"].as_str(), Some("first metadata"));
        assert!(fields[2].get("custom").is_none());
        assert!(fields[2].get("path").is_none());
        for (metadata, comment) in [
            ("second metadata", "# second row"),
            ("first metadata", "# first row"),
        ] {
            let row = saved.lines().find(|line| line.contains(metadata)).unwrap();
            assert!(row.contains(comment), "{row}");
        }
        for removed in [
            "file metadata",
            "# old file row",
            "removed metadata",
            "# removed row",
        ] {
            assert!(!saved.contains(removed), "{saved}");
        }

        collection =
            Collection::from_path(&root, environment(&root.join("environment.toml"))).unwrap();
        let Entry::File(entry) = &collection.entries[0] else {
            panic!("expected request file");
        };
        let Request::Http(request) = &entry.request;

        assert_eq!(request, &expected);
    }

    fs::remove_dir_all(root).unwrap();
}
