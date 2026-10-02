use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use environment::EnvironmentSessions;

use super::{
    ExecutionInfo, LocalVariables, NextRequest, RequestScripts, ScriptReport,
    runtime::{ScriptState, post_response, pre_request},
};
use crate::{
    Execution, Field, HeaderMap, HttpMetrics, HttpRequest, HttpResponse, RequestExecutor,
    RequestPreferences, RequestVariables, Response, StatusCode, Version,
};

fn executor() -> RequestExecutor {
    RequestExecutor::new(&RequestPreferences::default()).unwrap()
}

fn send(
    request: HttpRequest,
    variables: RequestVariables,
) -> (HttpRequest, ScriptState, Vec<ScriptReport>) {
    smol::block_on(pre_request(
        request,
        variables,
        executor(),
        Arc::new(AtomicBool::new(false)),
    ))
    .unwrap()
}

fn pre(source: &str) -> ScriptReport {
    let request = HttpRequest {
        path: "https://example.com".into(),
        scripts: RequestScripts {
            pre_request: source.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let (_, _, mut reports) = send(request, RequestVariables::new(HashMap::new(), None));

    reports.remove(0)
}

fn passed(report: &ScriptReport) {
    assert!(report.error.is_none(), "{:?}", report.error);
    for test in &report.tests {
        assert!(test.error.is_none(), "{}: {:?}", test.name, test.error);
    }
}

#[test]
fn scopes_follow_postman_precedence_and_reach_the_session() {
    let workspace = EnvironmentSessions::default();
    let session = workspace.for_path(Some("/one/environment.toml".as_ref()));
    let collection = HashMap::from([
        ("base_url".to_owned(), "https://collection".to_owned()),
        ("page".to_owned(), "1".to_owned()),
    ]);
    let environment = HashMap::from([("base_url".to_owned(), "https://staging".to_owned())]);
    let request = HttpRequest {
        path: "{{base_url}}/{{region}}/{{page}}".into(),
        scripts: RequestScripts {
            pre_request: r#"
                pm.test("scopes", () => {
                    pm.expect(pm.collectionVariables.get("base_url")).to.equal("https://collection");
                    pm.expect(pm.environment.get("base_url")).to.equal("https://staging");
                    pm.expect(pm.environment.get("page")).to.equal("1");
                    pm.expect(pm.globals.has("page")).to.be.false;

                    pm.globals.set("region", "eu");
                    pm.collectionVariables.set("page", 2);
                    pm.expect(pm.variables.get("page")).to.equal("2");
                    pm.expect(pm.variables.toObject()).to.include({base_url: "https://staging", page: "2", region: "eu"});
                    pm.expect(pm.collectionVariables.toObject()).to.eql({base_url: "https://collection", page: "2"});
                    pm.expect(pm.globals.replaceIn("{{region}}/{{page}}")).to.equal("eu/{{page}}");
                });
                pm.test("names are checked", () => {
                    for (const set of [() => pm.globals.set("$guid", "x"), () => pm.collectionVariables.set("", "x")]) {
                        try { set(); throw new Error("accepted"); }
                        catch (error) { pm.expect(error.message).to.include("nonempty and cannot start with $"); }
                    }
                });
            "#
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };

    let variables = RequestVariables::with_environment_session(
        collection.clone(),
        environment.clone(),
        None,
        session.clone(),
    );
    let (sent, _, reports) = send(request, variables);
    passed(&reports[0]);
    assert_eq!(reports[0].tests.len(), 2);
    assert_eq!(sent.path, "https://staging/eu/2");

    let scopes = session.scopes(collection.clone(), environment.clone());
    assert_eq!(scopes.collection["page"].as_deref(), Some("2"));
    assert_eq!(scopes.globals["region"].as_deref(), Some("eu"));

    // Another collection shares the globals, but not the collection variables.
    let other = workspace.for_path(Some("/two/environment.toml".as_ref()));
    let values = other.values(HashMap::new(), HashMap::new());
    assert_eq!(values["region"], "eu");
    assert!(!values.contains_key("page"));
}

#[test]
fn unsetting_hides_a_name_in_its_scope_and_those_beneath() {
    let session = EnvironmentSessions::default().for_path(None);
    let collection = HashMap::from([("page".to_owned(), "1".to_owned())]);
    let request = HttpRequest {
        path: "https://example.com".into(),
        scripts: RequestScripts {
            pre_request: r#"
                pm.globals.set("region", "eu");
                pm.globals.set("page", "global");
                pm.environment.unset("page");
                pm.collectionVariables.unset("region");
                pm.test("hidden", () => {
                    pm.expect(pm.variables.has("page")).to.be.false;
                    pm.expect(pm.collectionVariables.get("page")).to.equal("1");
                    pm.expect(pm.variables.has("region")).to.be.false;
                    pm.expect(pm.globals.get("region")).to.equal("eu");
                });
                pm.globals.clear();
                pm.test("cleared", () => pm.expect(pm.globals.toObject()).to.eql({}));
            "#
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };

    let variables = RequestVariables::with_environment_session(
        collection,
        HashMap::new(),
        None,
        session.clone(),
    );
    let (_, _, reports) = send(request, variables);
    passed(&reports[0]);
    assert_eq!(reports[0].tests.len(), 2);
    assert!(
        session
            .scopes(HashMap::new(), HashMap::new())
            .globals
            .is_empty()
    );
}

#[test]
fn legacy_postman_api_and_set_next_request_are_accepted() {
    let report = pre(r#"
        postman.setNextRequest("Next");
        pm.execution.setNextRequest(null);
        postman.setGlobalVariable("token", 42);
        postman.setEnvironmentVariable("user", "eagle");
        pm.test("legacy", () => {
            pm.expect(postman.getGlobalVariable("token")).to.equal("42");
            pm.expect(pm.environment.get("user")).to.equal("eagle");
            postman.clearEnvironmentVariable("user");
            pm.expect(postman.getEnvironmentVariable("user")).to.be.undefined;
        });
    "#);

    passed(&report);
    assert_eq!(report.tests.len(), 1);
    // The last choice wins, as in Postman.
    assert_eq!(report.next_request, Some(NextRequest::Stop));
}

#[test]
fn set_next_request_reports_the_named_request() {
    assert_eq!(pre("const nothing = null;").next_request, None);
    assert_eq!(
        pre("pm.execution.setNextRequest(42)").next_request,
        Some(NextRequest::Request("42".into()))
    );
}

#[test]
fn iteration_data_resolves_over_the_scopes_and_under_overrides() {
    let request = HttpRequest {
        path: "https://{{host}}/{{name}}/{{user}}".into(),
        scripts: RequestScripts {
            pre_request: r#"
                pm.test("data", () => {
                    pm.expect(pm.iterationData.get("user")).to.equal("ada");
                    pm.expect(pm.iterationData.has("host")).to.be.false;
                    pm.expect(pm.iterationData.toObject()).to.eql({name: "data", user: "ada"});
                    pm.expect(pm.variables.get("name")).to.equal("data");
                    pm.expect(pm.variables.replaceIn("{{host}} {{name}}")).to.equal("env data");
                    pm.expect(pm.iterationData.replaceIn("{{user}} {{host}}")).to.equal("ada {{host}}");
                });
                pm.variables.set("user", "local");
            "#
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let variables = RequestVariables::new(
        HashMap::from([("host".into(), "env".into()), ("name".into(), "env".into())]),
        None,
    )
    .with_iteration_data(
        [("name", "data"), ("user", "ada")]
            .map(|(name, value)| (name.to_owned(), value.into()))
            .into(),
    );

    let (request, _, reports) = send(request, variables);

    passed(&reports[0]);
    assert_eq!(reports[0].tests.len(), 1);
    assert_eq!(request.path, "https://env/data/local");
}

#[test]
fn json_data_keeps_its_types_for_scripts_and_is_text_in_requests() {
    let request = HttpRequest {
        path: "https://example.com/{{count}}/{{flags}}/{{none}}".into(),
        scripts: RequestScripts {
            pre_request: r#"
                pm.test("typed", () => {
                    pm.expect(pm.iterationData.get("shouldRun")).to.equal(false);
                    pm.expect(pm.iterationData.get("count")).to.equal(2);
                    pm.expect(pm.variables.get("flags")).to.eql({a: true});
                    pm.expect(pm.variables.replaceIn("{{count}} {{shouldRun}}")).to.equal("2 false");
                    // As the request writes it, not as JavaScript would.
                    pm.expect(pm.variables.replaceIn("{{price}}")).to.equal("1e-6");
                });
            "#
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let data = serde_json::json!({
        "shouldRun": false,
        "count": 2,
        "flags": {"a": true},
        "none": null,
        "price": 0.000001,
    });
    let variables = RequestVariables::new(HashMap::new(), None).with_iteration_data(
        data.as_object()
            .unwrap()
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
    );

    let (request, _, reports) = send(request, variables);

    passed(&reports[0]);
    assert_eq!(reports[0].tests.len(), 1);
    assert_eq!(request.path, r#"https://example.com/2/{"a":true}/"#);
}

#[test]
fn local_variables_carry_over_to_the_next_request() {
    let locals = LocalVariables::default();
    let request = |source: &str| HttpRequest {
        path: "https://example.com/{{step}}".into(),
        scripts: RequestScripts {
            pre_request: source.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let variables = || {
        RequestVariables::new(HashMap::from([("step".into(), "env".into())]), None)
            .with_local_variables(locals.clone())
    };

    let (first, _, _) = send(request(r#"pm.variables.set("step", "one");"#), variables());
    let (second, _, reports) = send(
        request(r#"pm.test("kept", () => pm.expect(pm.variables.get("step")).to.equal("one"));"#),
        variables(),
    );

    assert_eq!(first.path, "https://example.com/one");
    assert_eq!(second.path, "https://example.com/one");
    passed(&reports[0]);
    // Skipping is not failing, so its values last too.
    let skipped = smol::block_on(pre_request(
        request(r#"pm.variables.set("step", "skipped"); pm.execution.skipRequest();"#),
        variables(),
        executor(),
        Arc::new(AtomicBool::new(false)),
    ));
    assert!(skipped.is_err());
    assert_eq!(locals.get()["step"], "skipped");
    locals.set([("step".to_owned(), "one".to_owned())].into());
    // A failed phase leaves the values as they were.
    let failed = smol::block_on(pre_request(
        request(r#"pm.variables.set("step", "two"); throw new Error("stop");"#),
        variables(),
        executor(),
        Arc::new(AtomicBool::new(false)),
    ));
    assert!(failed.is_err());
    assert_eq!(locals.get()["step"], "one");
}

#[test]
fn info_describes_the_request_and_its_iteration() {
    passed(&pre(r#"
        pm.test("single send", () => {
            pm.expect(pm.info).to.include({eventName: "prerequest", iteration: 0, iterationCount: 1});
        });
    "#));

    let request = HttpRequest {
        path: "https://example.com".into(),
        scripts: RequestScripts {
            pre_request: r#"
                pm.test("run", () => {
                    pm.expect(pm.info).to.eql({
                        eventName: "prerequest",
                        iteration: 2,
                        iterationCount: 3,
                        requestName: "Login",
                        requestId: "login-id",
                    });
                });
            "#
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let variables = RequestVariables::new(HashMap::new(), None).with_info(ExecutionInfo {
        request_name: "Login".into(),
        request_id: "login-id".into(),
        iteration: 2,
        iteration_count: 3,
    });

    let (_, _, reports) = send(request, variables);

    passed(&reports[0]);
    assert_eq!(reports[0].tests.len(), 1);
}

#[test]
fn scripts_require_postman_libraries() {
    let report = pre(r#"
        pm.test("crypto-js", () => {
            const CryptoJS = require("crypto-js");
            pm.expect(CryptoJS.HmacSHA256("what do ya want for nothing?", "Jefe").toString())
                .to.equal("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
            pm.expect(CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse("Hello"))).to.equal("SGVsbG8=");
            pm.expect(CryptoJS.MD5("abc").toString()).to.equal("900150983cd24fb0d6963f7d28e17f72");
            pm.expect(CryptoJS.lib.WordArray.random(16).sigBytes).to.equal(16);
            const encrypted = CryptoJS.AES.encrypt("secret", "passphrase").toString();
            pm.expect(CryptoJS.AES.decrypt(encrypted, "passphrase").toString(CryptoJS.enc.Utf8)).to.equal("secret");
        });
        pm.test("lodash", () => {
            const _ = require("lodash");
            pm.expect(_.get({a: [{b: 3}]}, "a[0].b")).to.equal(3);
            pm.expect(_.chunk([1, 2, 3], 2)).to.eql([[1, 2], [3]]);
            pm.expect(require("lodash")).to.equal(_);
        });
        pm.test("moment", () => {
            const moment = require("moment");
            pm.expect(moment.utc("2024-02-29T12:00:00Z").add(1, "year").format("YYYY-MM-DD")).to.equal("2025-02-28");
            pm.expect(moment().isValid()).to.be.true;
        });
        pm.test("uuid", () => {
            const uuid = require("uuid");
            pm.expect(uuid.v4()).to.match(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
            pm.expect(uuid()).to.have.lengthOf(36);
        });
        pm.test("base64", () => {
            pm.expect(require("btoa")("hiÿ")).to.equal("aGn/");
            pm.expect(atob("aGn/")).to.equal("hiÿ");
        });
        pm.test("unknown", () => {
            try { require("cheerio"); throw new Error("loaded"); }
            catch (error) { pm.expect(error.message).to.include("Cannot find module 'cheerio'"); }
        });
    "#);

    passed(&report);
    assert_eq!(report.tests.len(), 6);
}

#[test]
fn postman_library_globals_load_when_first_used() {
    let report = pre(r#"
        pm.test("globals", () => {
            pm.expect(CryptoJS.SHA256("abc").toString())
                .to.equal("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
            pm.expect(_.sum([1, 2, 3])).to.equal(6);
            pm.expect(_).to.equal(require("lodash"));
            pm.expect(btoa("hi")).to.equal("aGk=");
        });
    "#);

    passed(&report);
    assert_eq!(report.tests.len(), 1);
}

fn exchange(
    request_headers: Vec<(String, String)>,
    response_headers: &[(&'static str, &str)],
    source: &str,
) -> ScriptReport {
    let request = HttpRequest {
        headers: request_headers.into_iter().map(Field::from).collect(),
        scripts: RequestScripts {
            post_response: source.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut headers = HeaderMap::new();
    for &(name, value) in response_headers {
        headers.append(crate::HeaderName::from_static(name), value.parse().unwrap());
    }
    let execution = Execution {
        elapsed: Duration::from_millis(42),
        scripts: Vec::new(),
        sent: None,
        response: Response::Http(HttpResponse {
            status: StatusCode::OK,
            version: Version::HTTP_11,
            headers,
            body: Vec::new(),
            metrics: HttpMetrics::default(),
        }),
    };
    let state = ScriptState {
        variables: Default::default(),
        session: None,
        collection_post_response: String::new(),
        response_url: None,
        info: Default::default(),
        locals: None,
    };
    let result = smol::block_on(post_response(
        request,
        None,
        state,
        execution,
        executor(),
        Arc::new(AtomicBool::new(false)),
    ));

    result.scripts.into_iter().next().unwrap()
}

#[test]
fn cookies_list_what_the_request_sent_and_the_response_set() {
    let report = exchange(
        vec![("Cookie".into(), "a=1; b=2; theme=dark".into())],
        &[
            ("set-cookie", "b=3; Path=/; HttpOnly"),
            ("set-cookie", "a=; Max-Age=0"),
            (
                "set-cookie",
                "c=4; Domain=.example.com; Expires=Wed, 21 Oct 2099 07:28:00 GMT; Secure; SameSite=Lax",
            ),
        ],
        r#"
            pm.test("exchange", () => {
                pm.expect(pm.cookies.toObject()).to.eql({theme: "dark", b: "3", c: "4"});
                pm.expect(pm.cookies.has("a")).to.be.false;
                pm.expect(pm.cookies.has("b", "3")).to.be.true;
                pm.expect(pm.cookies.get("theme")).to.equal("dark");
                pm.expect(pm.cookies.count()).to.equal(3);
            });
            pm.test("response", () => {
                const cookies = pm.response.cookies;
                pm.expect(cookies.count()).to.equal(3);
                pm.expect(cookies.one("b")).to.include({value: "3", path: "/", httpOnly: true, secure: false});
                pm.expect(cookies.one("a").maxAge).to.equal(0);
                const c = cookies.one("c");
                pm.expect(c).to.include({domain: "example.com", secure: true, sameSite: "Lax"});
                pm.expect(c.expires.getUTCFullYear()).to.equal(2099);
            });
            pm.test("jar", () => {
                let reported;
                pm.cookies.jar().get("https://example.com", "b", error => { reported = error; });
                pm.expect(reported.message).to.include("cookie jar is off");
            });
            pm.execution.setNextRequest("Next");
        "#,
    );

    passed(&report);
    assert_eq!(report.tests.len(), 3);
    assert_eq!(
        report.next_request,
        Some(NextRequest::Request("Next".into()))
    );
}

#[test]
fn cookies_before_sending_resolve_variables_in_the_cookie_header() {
    let request = HttpRequest {
        path: "https://example.com".into(),
        headers: vec![Field::new("cookie", "session={{token}}")],
        scripts: RequestScripts {
            pre_request: r#"
                pm.test("cookie", () => pm.expect(pm.cookies.get("session")).to.equal("abc"));
                pm.cookies.jar().clear("https://example.com");
            "#
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let variables = RequestVariables::new(HashMap::from([("token".into(), "abc".into())]), None);
    let (_, _, reports) = send(request, variables);

    passed(&reports[0]);
    assert_eq!(reports[0].tests.len(), 1);
    assert_eq!(reports[0].logs[0].level, "warn");
    assert!(reports[0].logs[0].message.contains("cookie jar is off"));
}

#[test]
fn replacements_read_only_the_names_they_reference() {
    let mut values: HashMap<String, String> = (0..2000)
        .map(|index| (format!("unused{index}"), "x".to_owned()))
        .collect();
    values.insert("id".into(), "7".into());
    let request = HttpRequest {
        path: "https://example.com".into(),
        scripts: RequestScripts {
            pre_request: r#"
                let urls = 0;
                for (let row = 0; row < 2000; row++) {
                    urls += pm.environment.replaceIn("/items/{{id}}").length;
                    urls += pm.variables.replaceIn("/items/{{id}}").length;
                }
                pm.test("replaced", () => pm.expect(urls).to.equal(2000 * 2 * "/items/7".length));
            "#
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let (_, _, reports) = send(request, RequestVariables::new(values, None));

    passed(&reports[0]);
    assert_eq!(reports[0].tests.len(), 1);
}
