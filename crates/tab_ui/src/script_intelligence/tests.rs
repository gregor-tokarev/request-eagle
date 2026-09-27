use lsp_types::{CompletionItem, CompletionTextEdit, HoverContents, ParameterLabel};
use request::ScriptPhase;

fn marked(source: &str) -> (String, usize) {
    let offset = source.find('|').expect("cursor marker");
    let mut source = source.to_owned();
    source.remove(offset);

    (source, offset)
}

fn complete(source: &str, phase: ScriptPhase) -> Vec<CompletionItem> {
    let (source, offset) = marked(source);

    smol::block_on(super::completions(source, offset, phase)).unwrap()
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_str()).collect()
}

#[test]
fn typescript_completes_contextual_options_nested_headers_and_raw_body() {
    let phase = ScriptPhase::PreRequest;
    let items = complete("pm.sendRequest({u|})", phase);
    assert_eq!(labels(&items), ["url"]);
    assert_eq!(items[0].filter_text.as_deref(), Some("u"));
    assert!(items[0].detail.as_deref().unwrap().contains("string"));
    assert!(items[0].documentation.is_some());

    assert_eq!(
        labels(&complete("pm.sendRequest({url: '', m|})", phase)),
        ["method"]
    );
    assert_eq!(
        labels(&complete("pm.sendRequest({url: '', body: {r|}})", phase)),
        ["raw"]
    );
    assert_eq!(
        labels(&complete(
            "pm.sendRequest({url: '', headers: [{k|}]})",
            phase
        )),
        ["key"]
    );
    assert_eq!(
        labels(&complete("pm.request.headers.upsert({v|})", phase)),
        ["value"]
    );

    let methods = complete("pm.sendRequest({url: '', method: 'P|'})", phase);
    let mut methods = labels(&methods);
    methods.sort_unstable();
    assert_eq!(methods, ["PATCH", "POST", "PUT"]);

    // TypeScript supplies recovery completions even while a comma is missing.
    assert!(!complete("pm.sendRequest({body: {}}|)", phase).is_empty());
}

#[test]
fn typescript_infers_local_values_awaited_responses_and_callback_parameters() {
    let phase = ScriptPhase::PreRequest;
    assert_eq!(
        labels(&complete(
            "const result = await pm.sendRequest(''); result.co|",
            phase
        )),
        ["code"]
    );
    assert_eq!(
        labels(&complete(
            "pm.sendRequest('', (error, response) => { response.he| });",
            phase
        )),
        ["headers"]
    );
    assert_eq!(
        labels(&complete(
            "const options = {url: 'https://example.com', retries: 3}; options.re|",
            phase
        )),
        ["retries"]
    );
    assert!(labels(&complete("const names = ['Eagle']; names.ma|", phase)).contains(&"map"));
}

#[test]
fn phase_declarations_switch_without_leaking_response_or_execution_members() {
    assert_eq!(
        labels(&complete("pm.res|", ScriptPhase::PostResponse)),
        ["response"]
    );
    assert!(complete("pm.res|", ScriptPhase::PreRequest).is_empty());
    assert_eq!(
        labels(&complete("pm.execution.sk|", ScriptPhase::PreRequest)),
        ["skipRequest"]
    );
    assert!(complete("pm.execution.sk|", ScriptPhase::PostResponse).is_empty());
    assert_eq!(
        labels(&complete("pm.res|", ScriptPhase::PostResponse)),
        ["response"]
    );
}

#[test]
fn signature_help_reports_parameter_names_and_active_argument() {
    let (source, offset) = marked("pm.crypto.hmacSha256('secret', |)");
    let help = smol::block_on(super::signature_help(
        source,
        offset,
        ScriptPhase::PreRequest,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(help.active_parameter, Some(1));
    let signature = &help.signatures[help.active_signature.unwrap_or(0) as usize];
    assert!(signature.label.contains("hmacSha256"));
    assert!(signature.label.contains("secret: string"));
    assert!(signature.label.contains("text: string"));

    let parameters = signature.parameters.as_ref().unwrap();
    let ParameterLabel::LabelOffsets([start, end]) = parameters[1].label else {
        panic!("signature parameter must have offsets");
    };
    assert_eq!(
        &signature.label[start as usize..end as usize],
        "text: string"
    );

    let (source, offset) = marked("pm.sendRequest({url: ''}, |)");
    let help = smol::block_on(super::signature_help(
        source,
        offset,
        ScriptPhase::PreRequest,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(help.active_parameter, Some(1));
    assert!(
        help.signatures
            .iter()
            .any(|signature| signature.label.contains("callback"))
    );
}

#[test]
fn hover_uses_inferred_types_and_api_documentation() {
    let (source, offset) = marked("pm.sendReq|uest({url: ''})");
    let hover = smol::block_on(super::hover(source, offset, ScriptPhase::PreRequest))
        .unwrap()
        .unwrap();
    let HoverContents::Markup(contents) = hover.contents else {
        panic!("markdown hover");
    };
    assert!(contents.value.contains("sendRequest"));
    assert!(contents.value.contains("Promise"));
    assert!(hover.range.is_some());
}

#[test]
fn completions_replace_suffixes_and_map_utf8_offsets_to_utf16_ranges() {
    let items = complete(
        "const emoji = '🚀';\r\npm.sendRequest({u|rl})",
        ScriptPhase::PreRequest,
    );
    assert_eq!(labels(&items), ["url"]);
    let CompletionTextEdit::Edit(edit) = items[0].text_edit.as_ref().unwrap() else {
        panic!("simple completion edit");
    };
    assert_eq!(edit.range.start.line, 1);
    assert_eq!(edit.range.start.character, 16);
    assert_eq!(edit.range.end.character, 19);
    assert_eq!(edit.new_text, "url");

    let items = complete(
        "const 𐐀 = '🚀'; pm.request.he|aders",
        ScriptPhase::PreRequest,
    );
    assert_eq!(labels(&items), ["headers"]);
    let CompletionTextEdit::Edit(edit) = items[0].text_edit.as_ref().unwrap() else {
        panic!("simple completion edit");
    };
    assert_eq!(
        edit.range.start.character,
        "const 𐐀 = '🚀'; pm.request.".encode_utf16().count() as u32
    );
    assert_eq!(edit.range.end.character - edit.range.start.character, 7);
}

#[test]
fn completion_and_hover_ranges_use_lf_lines_with_unicode_separators_and_bare_cr() {
    let position = |source: &str, offset: usize| {
        let prefix = &source[..offset];
        lsp_types::Position::new(
            prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
            prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
        )
    };

    for prefix in [
        "const text = 'a\u{2028}b'; ",
        "const text = 'a\u{2029}🦅b'; ",
        "const text = '🦅';\rpm.variables.set('x', 'y'); ",
        "const text = 'a\u{2028}b';\r\nconst other = 'c\u{2029}d';\r",
    ] {
        let (source, offset) = marked(&format!("{prefix}pm.sendRequest({{u|rl: ''}})"));
        let items = smol::block_on(super::completions(
            source.clone(),
            offset,
            ScriptPhase::PreRequest,
        ))
        .unwrap();
        assert_eq!(labels(&items), ["url"], "{source:?}");
        let CompletionTextEdit::Edit(edit) = items[0].text_edit.as_ref().unwrap() else {
            panic!("simple completion edit");
        };
        assert_eq!(
            edit.range.start,
            position(&source, offset - 1),
            "{source:?}"
        );
        assert_eq!(edit.range.end, position(&source, offset + 2), "{source:?}");

        let (source, offset) = marked(&format!("{prefix}pm.sendReq|uest({{url: ''}})"));
        let hover = smol::block_on(super::hover(
            source.clone(),
            offset,
            ScriptPhase::PreRequest,
        ))
        .unwrap()
        .unwrap();
        let range = hover.range.unwrap();
        assert_eq!(range.start, position(&source, offset - 7), "{source:?}");
        assert_eq!(range.end, position(&source, offset + 4), "{source:?}");
    }
}

#[test]
fn user_source_is_never_executed_and_comments_do_not_offer_code_members() {
    let phase = ScriptPhase::PreRequest;
    assert_eq!(
        labels(&complete(
            "while (true) {} throw new Error('executed'); pm.sendRequest({u|})",
            phase
        )),
        ["url"]
    );

    for source in [
        "// pm.request.he|",
        "/* pm.request.he| */",
        "const text = 'pm.request.he|';",
        "p| => 42",
        "const p| = 42",
    ] {
        assert!(complete(source, phase).is_empty(), "{source}");
    }
}

#[test]
fn invalid_offsets_and_large_sources_fail_without_entering_the_worker() {
    assert!(smol::block_on(super::completions("🚀".into(), 1, ScriptPhase::PreRequest)).is_err());
    assert!(smol::block_on(super::completions("".into(), 1, ScriptPhase::PreRequest)).is_err());
    assert!(
        smol::block_on(super::completions(
            "x".repeat(256 * 1024 + 1),
            0,
            ScriptPhase::PreRequest
        ))
        .is_err()
    );
}

#[test]
fn active_cancellation_preserves_the_compiler_for_same_and_changed_source_queries() {
    // Match the worker's native stack size. Cancel at actual language-service
    // checkpoints so this regression does not depend on scheduler/timer speed.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            use std::{cell::Cell, rc::Rc};

            let mut compiler = super::compiler::Compiler::new().unwrap();

            for phase in [ScriptPhase::PreRequest, ScriptPhase::PostResponse] {
                for (source, offset, kind, after_checks) in [
                    ("pm.", 3, "completions", 8),
                    ("pm.sendRequest({url: ''}, ", 25, "signature", 1),
                    ("pm.sendRequest", 5, "hover", 1),
                ] {
                    let checks = Rc::new(Cell::new(0));
                    let current_checks = checks.clone();
                    let result = compiler
                        .query(source, offset, phase, kind, move || {
                            current_checks.set(current_checks.get() + 1);
                            current_checks.get() > after_checks
                        })
                        .unwrap();

                    assert!(result.is_none(), "active {kind} query must cancel");
                    assert!(checks.get() > after_checks);

                    // Reuse the same instance after cancellation, including
                    // its partially visited source, then a new typed source.
                    let result = compiler
                        .query(source, offset, phase, kind, || false)
                        .unwrap()
                        .unwrap();
                    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
                    match kind {
                        "completions" => assert!(result.as_array().unwrap().iter().any(|item| {
                            item["label"] == "sendRequest"
                                && item["detail"].as_str().unwrap().contains("Promise")
                        })),
                        "signature" => assert_eq!(result["activeParameter"], 1),
                        "hover" => assert!(
                            result["contents"]["value"]
                                .as_str()
                                .unwrap()
                                .contains("Promise")
                        ),
                        _ => unreachable!(),
                    }

                    let result = compiler
                        .query("pm.sendRequest({u})", 17, phase, "completions", || false)
                        .unwrap()
                        .unwrap();
                    let result: Vec<CompletionItem> = serde_json::from_str(&result).unwrap();
                    assert_eq!(labels(&result), ["url"]);
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
