use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use super::{
    RequestScripts, ScriptPhase, ScriptReport,
    runtime::{post_response, pre_request},
};
use crate::{
    Execution, HeaderMap, HttpMetrics, HttpRequest, HttpResponse, Response, StatusCode, Version,
};

fn pre(source: &str) -> (HttpRequest, super::variables::Variables, Vec<ScriptReport>) {
    smol::block_on(pre_request(
        HttpRequest {
            path: "https://example.com".into(),
            scripts: RequestScripts {
                pre_request: source.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        Arc::new(AtomicBool::new(false)),
    ))
    .unwrap()
}

#[test]
fn common_chai_assertions_support_real_chaining_and_nested_values() {
    let (_, _, reports) = pre(r#"
        pm.test('allowed status', () => pm.expect(201).to.be.oneOf([200, 201, 202]));
        pm.test('keys', () => pm.expect({id: 1, name: 'Eagle'}).to.have.all.keys('id', 'name'));
        pm.test('subset keys', () => pm.expect({id: 1, name: 'Eagle'}).to.include.keys('id'));
        pm.test('any keys', () => pm.expect({id: 1}).to.have.any.keys('id', 'name'));
        pm.test('members', () => pm.expect(['email', 'sms']).to.have.members(['sms', 'email']));
        pm.test('deep subset', () => pm.expect([{id: 1}, {id: 2}]).to.deep.include.members([{id: 2}]));
        pm.test('nested value', () => pm.expect({users: [{name: 'Eagle'}]}).to.have.nested.property('users[0].name', 'Eagle'));
        pm.test('nested deep value', () => pm.expect({data: {roles: ['admin']}}).to.have.deep.nested.property('data.roles', ['admin']));
        pm.test('deep object subset', () => pm.expect({ok: true, errors: []}).to.deep.include({errors: []}));
        pm.test('ranges', () => pm.expect(42).to.be.within(0, 100).and.at.least(40).and.at.most(50));
        pm.test('length chain', () => pm.expect(['a', 'b']).to.have.lengthOf.above(1));
        pm.test('existence', () => pm.expect(0).to.exist);
        pm.test('nullish', () => pm.expect(undefined).not.to.exist);
        pm.test('numeric type', () => pm.expect(NaN).to.be.NaN);
        pm.test('aliases', () => pm.expect('request eagle').to.contain('eagle').and.match(/request/));
        pm.test('custom message', () => pm.expect(42).to.be.a('number', 'the answer'));
        pm.test('no host globals', () => {
            for (const name of ['chai', 'module', 'exports', 'require', 'process', 'fetch']) {
                pm.expect(typeof globalThis[name]).to.equal('undefined');
            }
        });
    "#);
    assert_eq!(reports[0].tests.len(), 17);
    for test in &reports[0].tests {
        assert!(test.error.is_none(), "{}: {:?}", test.name, test.error);
    }
}

#[test]
fn invalid_assertions_fail_instead_of_producing_false_positives() {
    let (_, _, reports) = pre(r#"
        pm.test('wrong status', () => pm.expect(500).to.be.oneOf([200, 201]));
        pm.test('missing key', () => pm.expect({id: 1}).to.have.all.keys('id', 'name'));
        pm.test('wrong members', () => pm.expect(['email']).to.have.members(['sms']));
        pm.test('missing nested value', () => pm.expect({users: []}).to.have.nested.property('users[0].name'));
        pm.test('shallow include', () => pm.expect({data: {id: 1}}).to.include({data: {id: 1}}));
        pm.test('shallow property', () => pm.expect({data: {id: 1}}).to.have.property('data', {id: 1}));
        pm.test('different dates', () => pm.expect(new Date(0)).to.deep.equal(new Date(1)));
        pm.test('invalid numeric type', () => pm.expect('42').to.be.above(1));
        pm.test('negation across chain', () => pm.expect(2).not.to.equal(1).and.equal(2));
        pm.test('typo', () => pm.expect(true).to.be.tru);
        pm.test('continues', () => pm.expect(true).to.be.true);
    "#);
    assert_eq!(reports[0].tests.len(), 11);
    for test in &reports[0].tests[..10] {
        assert!(test.error.is_some(), "{} should fail", test.name);
    }
    assert!(reports[0].tests[10].error.is_none());
}

fn response_tests(status: StatusCode, body: &[u8], source: &str) -> ScriptReport {
    let request = HttpRequest {
        scripts: RequestScripts {
            post_response: source.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    let execution = Execution {
        elapsed: Duration::from_millis(42),
        scripts: Vec::new(),
        response: Response::Http(HttpResponse {
            status,
            version: Version::HTTP_11,
            headers,
            body: body.to_vec(),
            metrics: HttpMetrics::default(),
        }),
    };
    let result = smol::block_on(post_response(
        request,
        Default::default(),
        execution,
        Arc::new(AtomicBool::new(false)),
    ));
    let report = result.scripts.into_iter().next().unwrap();
    assert!(report.error.is_none(), "{:?}", report.error);
    assert_eq!(report.phase, ScriptPhase::PostResponse);
    report
}

#[test]
fn response_shortcuts_check_status_names_body_headers_and_json_paths() {
    let report = response_tests(
        StatusCode::CREATED,
        br#"{"users":[{"id":1}]}"#,
        r#"
        pm.test('status number', () => pm.response.to.have.status(201));
        pm.test('status text', () => pm.response.to.have.status('Created'));
        pm.test('header', () => pm.response.to.have.header('CONTENT-TYPE', 'application/json'));
        pm.test('body present', () => pm.response.to.have.body());
        pm.test('body text', () => pm.response.to.have.body('{"users":[{"id":1}]}'));
        pm.test('body object', () => pm.response.to.have.body({users: [{id: 1}]}));
        pm.test('body pattern', () => pm.response.to.have.body(/users/));
        pm.test('json valid', () => pm.response.to.have.jsonBody());
        pm.test('json path', () => pm.response.to.have.jsonBody('users[0].id', 1));
        pm.test('json object', () => pm.response.to.have.jsonBody('users[0]', {id: 1}));
        pm.test('json property', () => pm.response.to.have.jsonBody('users'));
        pm.test('success', () => pm.response.to.be.success);
        pm.test('json', () => pm.response.to.be.json);
        pm.test('wrong status', () => pm.response.to.have.status('OK'));
        pm.test('wrong body', () => pm.response.to.have.body({users: []}));
        pm.test('wrong path', () => pm.response.to.have.jsonBody('users[1].id'));
        pm.test('wrong value', () => pm.response.to.have.jsonBody('users[0].id', 2));
        pm.test('not 200', () => pm.response.to.be.ok);
        pm.test('typo', () => pm.response.to.be.sucess);
    "#,
    );
    assert_eq!(report.tests.len(), 19);
    for (index, test) in report.tests.iter().enumerate() {
        assert_eq!(
            test.error.is_none(),
            index < 13,
            "{}: {:?}",
            test.name,
            test.error
        );
    }
    let report = response_tests(
        StatusCode::INTERNAL_SERVER_ERROR,
        b"",
        r#"
        pm.test('error', () => pm.response.to.be.error);
        pm.test('server error', () => pm.response.to.be.serverError);
        pm.test('not client error', () => pm.response.to.be.clientError);
        pm.test('not success', () => pm.response.to.be.success);
        pm.test('empty body', () => pm.response.to.have.body());
        pm.test('invalid JSON', () => pm.response.to.be.json);
    "#,
    );
    assert_eq!(report.tests.len(), 6);
    for (index, test) in report.tests.iter().enumerate() {
        assert_eq!(
            test.error.is_none(),
            index < 2,
            "{}: {:?}",
            test.name,
            test.error
        );
    }
}

#[test]
fn dynamic_variables_work_in_scripts_and_preserve_local_overrides() {
    let (_, variables, reports) = pre(r#"
        const id = pm.variables.replaceIn('{{$guid}}');
        pm.variables.set('id', id);
        pm.test('fresh UUID', () => pm.expect(pm.variables.replaceIn('{{$randomUUID}}')).not.to.equal(id));
        pm.test('timestamp', () => pm.expect(Number(pm.variables.replaceIn('{{$timestamp}}'))).to.be.closeTo(Date.now() / 1000, 2));
        pm.test('ISO timestamp', () => pm.expect(pm.variables.replaceIn('{{$isoTimestamp}}')).to.match(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/));
        pm.test('integer', () => pm.expect(Number(pm.variables.replaceIn('{{$randomInt}}'))).to.be.within(0, 1000));
        pm.variables.set('$guid', 'override');
        pm.test('local wins', () => pm.expect(pm.variables.replaceIn('{{$guid}}')).to.equal('override'));
        pm.test('unknown stays', () => pm.expect(pm.variables.replaceIn('{{missing}}/{{$missing}}')).to.equal('{{missing}}/{{$missing}}'));
        pm.variables.set('nested', '{{$guid}}');
        pm.test('single pass', () => pm.expect(pm.variables.replaceIn('{{nested}}')).to.equal('{{$guid}}'));
    "#);
    assert_eq!(reports[0].tests.len(), 7);
    for test in &reports[0].tests {
        assert!(test.error.is_none(), "{}: {:?}", test.name, test.error);
    }
    assert_eq!(
        uuid::Uuid::parse_str(&variables["id"])
            .unwrap()
            .get_version_num(),
        4
    );
}

#[test]
fn dynamic_request_templates_work_without_scripts_and_do_not_change_the_draft() {
    let original = HttpRequest {
        path: "https://example.com/{{$guid}}".into(),
        headers: vec![
            ("X-Time".into(), "{{$timestamp}}".into()),
            ("{{$guid}}".into(), "{{$randomUUID}}".into()),
        ],
        query: Some(vec![("id".into(), "{{$randomUUID}}".into())]),
        body: Some(
            br#"{"time":"{{$isoTimestamp}}","n":{{$randomInt}},"keep":"{{$unknown}}"}"#.to_vec(),
        ),
        ..Default::default()
    };
    let (sent, variables, reports) = smol::block_on(pre_request(
        original.clone(),
        Arc::new(AtomicBool::new(false)),
    ))
    .unwrap();
    assert!(original.path.contains("{{$guid}}"));
    assert!(variables.is_empty());
    assert!(reports.is_empty());
    uuid::Uuid::parse_str(sent.path.strip_prefix("https://example.com/").unwrap()).unwrap();
    assert!(sent.headers[0].1.parse::<i64>().unwrap() > 0);
    uuid::Uuid::parse_str(&sent.headers[1].0).unwrap();
    uuid::Uuid::parse_str(&sent.headers[1].1).unwrap();
    uuid::Uuid::parse_str(&sent.query.unwrap()[0].1).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&sent.body.unwrap()).unwrap();
    chrono::DateTime::parse_from_rfc3339(body["time"].as_str().unwrap()).unwrap();
    assert!(body["n"].as_u64().unwrap() <= 1000);
    assert_eq!(body["keep"], "{{$unknown}}");
}
