use std::path::PathBuf;

use agent_client_protocol::schema::v1::{ContentBlock, EmbeddedResourceResource};
use gpui_kit::EntityId;
use gpui_kit::component::input::{InlineToken, InputContent};
use tab_ui::{Cite, CiteOrigin, CiteSource};

use crate::prompt::{Briefing, prompt, quote};

fn cite(source: CiteSource, text: &str) -> Cite {
    Cite {
        source,
        text: text.to_owned(),
        language: Some("json"),
        lines: Some((3, 4)),
        origin: CiteOrigin {
            page: EntityId::from(1u64),
            name: "Get user".into(),
            request: "GET {{base_url}}/users/42".into(),
            status: Some("500 Internal Server Error".into()),
            file: Some(PathBuf::from("/data/collections/Users API/Get user.toml")),
            unsaved_changes: true,
        },
    }
}

/// "why <cite> fails" with the cite as a token.
fn message() -> InputContent {
    let text = "why [Response body] fails";
    let start = text.find('[').unwrap();
    let end = text.find(']').unwrap() + 1;
    InputContent::new(text)
        .with_token(start..end, InlineToken::new("cite:1", "[Response body]"))
        .unwrap()
}

fn text_of(block: &ContentBlock) -> &str {
    match block {
        ContentBlock::Text(text) => &text.text,
        ContentBlock::Resource(resource) => match &resource.resource {
            EmbeddedResourceResource::TextResourceContents(contents) => &contents.text,
            _ => panic!("the cite is text"),
        },
        _ => panic!("unexpected block"),
    }
}

#[test]
fn cites_go_where_they_are_in_the_message() {
    let cites = vec![(
        "cite:1".to_owned(),
        cite(CiteSource::ResponseBody, "\"code\": \"INVALID_ID\""),
    )];

    let blocks = prompt(&message(), &cites, None, true, &mut Vec::new());

    assert_eq!(blocks.len(), 3);
    assert_eq!(text_of(&blocks[0]), "why ");
    assert!(matches!(&blocks[1], ContentBlock::Resource(_)));
    assert!(text_of(&blocks[1]).contains("\"code\": \"INVALID_ID\""));
    assert_eq!(text_of(&blocks[2]), " fails");
}

#[test]
fn agents_without_embedded_context_read_cites_in_the_text() {
    let cites = vec![("cite:1".to_owned(), cite(CiteSource::ResponseBody, "{}"))];

    let blocks = prompt(&message(), &cites, None, false, &mut Vec::new());

    assert!(matches!(&blocks[1], ContentBlock::Text(_)));
    assert!(text_of(&blocks[1]).starts_with("\n<cite ref=\"request-eagle:cite/1\">"));
}

#[test]
fn a_thread_starts_with_the_briefing() {
    let briefing = Briefing {
        data_directory: PathBuf::from("/home/me/.request-eagle"),
        cli: Some(PathBuf::from("/usr/local/bin/request-eagle-cli")),
    };

    let blocks = prompt(
        &message(),
        &[],
        Some(briefing.text()),
        true,
        &mut Vec::new(),
    );

    let first = text_of(&blocks[0]);
    assert!(
        first.contains("/home/me/.request-eagle/collections"),
        "{first}"
    );
    assert!(
        first.contains("/usr/local/bin/request-eagle-cli"),
        "{first}"
    );
    assert!(first.contains("{{token}}"), "{first}");
    // A token whose cite is gone is sent as its text.
    assert_eq!(text_of(&blocks[2]), "[Response body]");
}

#[test]
fn a_cite_says_what_it_quotes_and_from_where() {
    let quoted = quote(
        &cite(CiteSource::ResponseBody, "\"code\": \"INVALID_ID\""),
        &mut Vec::new(),
    );

    assert_eq!(
        quoted,
        "Response body of \"Get user\" (GET {{base_url}}/users/42), which responded 500 \
         Internal Server Error, lines 3–4.\n\
         Saved in /data/collections/Users API/Get user.toml, with changes in the tab that are \
         not saved yet.\n\n\
         ```json\n\"code\": \"INVALID_ID\"\n```"
    );
}

#[test]
fn a_fence_outlasts_the_backticks_in_a_cite() {
    let quoted = quote(
        &cite(CiteSource::Script, "const md = ```js```;"),
        &mut Vec::new(),
    );

    assert!(quoted.contains("\n````json\n"), "{quoted}");
}

#[test]
fn a_long_cite_is_cut_and_saved_whole_until_the_thread_ends() {
    let long = "x".repeat(50_000);
    let mut files = Vec::new();
    let quoted = quote(&cite(CiteSource::ResponseBody, &long), &mut files);

    let path = quoted
        .split("All of it is in ")
        .nth(1)
        .and_then(|rest| rest.split(".\n").next())
        .expect("the cite names its file")
        .to_owned();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), long);
    assert!(quoted.len() < long.len());
    assert_eq!(files.len(), 1);

    drop(files);
    assert!(!std::path::Path::new(&path).exists());
}
