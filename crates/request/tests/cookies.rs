use std::collections::HashMap;

use request::{
    CookieJar, HttpRequest, HttpVersion, RequestExecutor, RequestPreferences, RequestScripts,
    RequestVariables,
};
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

async fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();

    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }

    String::from_utf8(head).unwrap()
}

/// Answer one connection with each response in turn, and return the request
/// heads that arrived.
async fn serve(responses: Vec<&'static str>) -> (String, smol::Task<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = smol::spawn(async move {
        let mut heads = Vec::new();

        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            heads.push(read_head(&mut stream).await);
            stream
                .write_all(
                    format!("{response}Content-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
                )
                .await
                .unwrap();
        }

        heads
    });

    (url, server)
}

fn executor(cookie_jar: bool, jar: &CookieJar) -> RequestExecutor {
    RequestExecutor::new(&RequestPreferences {
        timeout_ms: 5_000,
        http_version: HttpVersion::Http1_1,
        cookie_jar,
        ..RequestPreferences::default()
    })
    .unwrap()
    .with_cookie_jar(jar.clone())
}

async fn get(executor: &RequestExecutor, url: String, headers: Vec<(String, String)>) {
    executor
        .execute(
            HttpRequest {
                path: url,
                headers,
                ..HttpRequest::default()
            },
            RequestVariables::new(HashMap::new(), None),
        )
        .await
        .unwrap();
}

fn cookie_header(head: &str) -> Option<&str> {
    head.lines().find_map(|line| line.strip_prefix("cookie: "))
}

#[test]
fn sends_stored_cookies_to_matching_paths_with_longer_paths_first() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nSet-Cookie: theme=dark; Path=/\r\nSet-Cookie: token=abc; Path=/admin; HttpOnly\r\n",
            "HTTP/1.1 200 OK\r\n",
            "HTTP/1.1 200 OK\r\n",
        ])
        .await;
        let jar = CookieJar::new();
        let executor = executor(true, &jar);

        get(&executor, format!("{url}/login"), Vec::new()).await;
        get(&executor, format!("{url}/api"), Vec::new()).await;
        get(&executor, format!("{url}/admin/users"), Vec::new()).await;
        let heads = server.await;

        assert_eq!(cookie_header(&heads[0]), None);
        assert_eq!(cookie_header(&heads[1]), Some("theme=dark"));
        assert_eq!(cookie_header(&heads[2]), Some("token=abc; theme=dark"));

        let cookies = jar.cookies();
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "theme");
        assert_eq!(cookies[0].domain, "127.0.0.1");
        assert!(cookies[0].host_only);
        assert_eq!(cookies[0].expires, None);
        assert_eq!(cookies[1].name, "token");
        assert_eq!(cookies[1].path, "/admin");
        assert!(cookies[1].http_only);
    });
}

#[test]
fn redirects_receive_the_cookies_that_earlier_responses_set() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 302 Found\r\nSet-Cookie: session=first\r\nLocation: /step\r\n",
            "HTTP/1.1 302 Found\r\nSet-Cookie: session=second\r\nLocation: /done\r\n",
            "HTTP/1.1 200 OK\r\n",
        ])
        .await;
        let jar = CookieJar::new();

        get(&executor(true, &jar), format!("{url}/login"), Vec::new()).await;
        let heads = server.await;

        assert_eq!(cookie_header(&heads[0]), None);
        assert_eq!(cookie_header(&heads[1]), Some("session=first"));
        assert_eq!(cookie_header(&heads[2]), Some("session=second"));
        assert_eq!(jar.cookies()[0].value, "second");
    });
}

#[test]
fn a_cookie_header_on_the_request_overrides_jar_cookies_of_the_same_name() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nSet-Cookie: session=stored\r\nSet-Cookie: theme=dark\r\n",
            "HTTP/1.1 200 OK\r\n",
        ])
        .await;
        let jar = CookieJar::new();
        let executor = executor(true, &jar);

        get(&executor, url.clone(), Vec::new()).await;
        let typed = vec![("cookie".to_owned(), "session=typed; extra=1".to_owned())];
        assert_eq!(
            jar.cookie_header(&url, &typed).as_deref(),
            Some("theme=dark")
        );
        assert_eq!(
            jar.cookie_header(&url, &[]).as_deref(),
            Some("session=stored; theme=dark")
        );
        assert_eq!(jar.cookie_header("{{base_url}}/users", &[]), None);

        get(&executor, url, typed).await;
        let heads = server.await;

        assert_eq!(
            cookie_header(&heads[1]),
            Some("session=typed; extra=1; theme=dark")
        );
    });
}

#[test]
fn several_cookie_headers_are_sent_with_the_jar_cookies_as_one() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nSet-Cookie: theme=dark\r\n",
            "HTTP/1.1 200 OK\r\n",
        ])
        .await;
        let jar = CookieJar::new();
        let executor = executor(true, &jar);

        get(&executor, url.clone(), Vec::new()).await;
        get(
            &executor,
            url,
            ["a=1", "b=2", "c=3"]
                .map(|cookie| ("Cookie".to_owned(), cookie.to_owned()))
                .to_vec(),
        )
        .await;
        let heads = server.await;

        assert_eq!(heads[1].matches("\r\ncookie: ").count(), 1);
        assert_eq!(cookie_header(&heads[1]), Some("a=1; b=2; c=3; theme=dark"));
    });
}

#[test]
fn expired_cookies_are_removed_and_no_longer_sent() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc\r\nSet-Cookie: theme=dark; Max-Age=3600\r\n",
            "HTTP/1.1 200 OK\r\nSet-Cookie: session=; Max-Age=0\r\n",
            "HTTP/1.1 200 OK\r\n",
        ])
        .await;
        let jar = CookieJar::new();
        let executor = executor(true, &jar);

        get(&executor, url.clone(), Vec::new()).await;
        let revision = jar.revision();
        get(&executor, url.clone(), Vec::new()).await;
        get(&executor, url, Vec::new()).await;
        let heads = server.await;

        assert!(jar.revision() > revision);
        assert_eq!(cookie_header(&heads[2]), Some("theme=dark"));
        let cookies = jar.cookies();
        assert_eq!(cookies.len(), 1);
        assert!(cookies[0].expires.is_some());
    });
}

#[test]
fn turning_the_cookie_jar_off_neither_stores_nor_sends_cookies() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc\r\n",
            "HTTP/1.1 200 OK\r\nSet-Cookie: other=1\r\n",
        ])
        .await;
        let jar = CookieJar::new();

        get(&executor(true, &jar), url.clone(), Vec::new()).await;
        get(&executor(false, &jar), url, Vec::new()).await;
        let heads = server.await;

        assert_eq!(cookie_header(&heads[1]), None);
        assert_eq!(jar.cookies().len(), 1);
    });
}

#[test]
fn script_requests_share_the_jar_with_the_request() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nSet-Cookie: session=from-script\r\n",
            "HTTP/1.1 200 OK\r\n",
        ])
        .await;
        let jar = CookieJar::new();

        executor(true, &jar)
            .execute(
                HttpRequest {
                    path: format!("{url}/data"),
                    scripts: RequestScripts {
                        pre_request: format!("await pm.sendRequest('{url}/login');"),
                        post_response: String::new(),
                    },
                    ..HttpRequest::default()
                },
                RequestVariables::new(HashMap::new(), None),
            )
            .await
            .unwrap();
        let heads = server.await;

        assert!(heads[0].starts_with("GET /login "));
        assert_eq!(cookie_header(&heads[1]), Some("session=from-script"));
    });
}

#[test]
fn scripts_read_and_change_the_jar() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc\r\n",
            "HTTP/1.1 200 OK\r\nSet-Cookie: late=2\r\n",
        ])
        .await;
        let jar = CookieJar::new();
        let executor = executor(true, &jar);
        get(&executor, format!("{url}/login"), Vec::new()).await;

        let execution = executor
            .execute(
                HttpRequest {
                    path: format!("{url}/data"),
                    scripts: RequestScripts {
                        pre_request: format!(
                            r#"
                            const url = "{url}/data";
                            pm.test("pm.cookies lists the jar", () => pm.expect(pm.cookies.get("session")).to.equal("abc"));
                            const jar = pm.cookies.jar();
                            jar.set(url, "added", "1", error => {{ if (error) throw error; }});
                            jar.set(url, {{name: "scoped", value: "2", path: "/other"}}, error => {{ if (error) throw error; }});
                            jar.unset(url, "session", error => {{ if (error) throw error; }});
                            let all;
                            jar.getAll(url, (error, cookies) => {{ all = cookies; }});
                            pm.test("getAll", () => pm.expect(all.map(cookie => cookie.name)).to.eql(["added"]));
                            jar.get(url, "added", (error, value) => pm.test("get", () => pm.expect(value).to.equal("1")));
                            jar.set("not a url", "x", "1", error => pm.test("invalid URL", () => pm.expect(error).to.be.an("error")));
                            "#
                        ),
                        post_response: r#"
                            pm.test("pm.cookies after the response", () => {
                                pm.expect(pm.cookies.toObject()).to.eql({added: "1", late: "2"});
                                pm.expect(pm.response.cookies.count()).to.equal(1);
                            });
                        "#
                        .into(),
                    },
                    ..HttpRequest::default()
                },
                RequestVariables::new(HashMap::new(), None),
            )
            .await
            .unwrap();
        let heads = server.await;

        for report in &execution.scripts {
            assert_eq!(report.error, None);
            for test in &report.tests {
                assert_eq!(test.error, None, "{}", test.name);
            }
        }
        assert_eq!(execution.scripts[0].tests.len(), 4);
        assert_eq!(cookie_header(&heads[1]), Some("added=1"));

        let names = jar
            .cookies()
            .into_iter()
            .map(|cookie| cookie.name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["added", "late", "scoped"]);
    });
}

#[test]
fn after_a_response_pm_cookies_keeps_the_jar_order_and_set_returns_the_stored_cookie() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nSet-Cookie: sid=public; Path=/\r\nSet-Cookie: sid=private; Path=/admin\r\n",
            "HTTP/1.1 200 OK\r\nSet-Cookie: sid=renewed; Path=/admin\r\nSet-Cookie: elsewhere=1; Path=/api\r\nSet-Cookie: gone=1; Path=/api\r\nSet-Cookie: gone=; Path=/api; Max-Age=0\r\nSet-Cookie: twice=first; Path=/api\r\nSet-Cookie: twice=second; Path=/api\r\n",
        ])
        .await;
        let jar = CookieJar::new();
        let executor = executor(true, &jar);
        get(&executor, format!("{url}/admin/login"), Vec::new()).await;

        let execution = executor
            .execute(
                HttpRequest {
                    path: format!("{url}/admin/users"),
                    scripts: RequestScripts {
                        pre_request: String::new(),
                        post_response: format!(
                            r#"
                            pm.test("the more specific cookie", () => pm.expect(pm.cookies.get("sid")).to.equal("renewed"));
                            pm.test("cookies for other paths", () => pm.expect(pm.cookies.get("elsewhere")).to.equal("1"));
                            pm.test("as the jar holds them", () => {{
                                pm.expect(pm.cookies.has("gone")).to.equal(false);
                                pm.expect(pm.cookies.get("twice")).to.equal("second");
                            }});
                            pm.cookies.jar().set("{url}/admin/users", {{name: "sid", value: "base", path: "/", sameSite: "Strict"}}, (error, cookie) => {{
                                pm.test("set returns the stored cookie", () => {{
                                    pm.expect(error).to.equal(null);
                                    pm.expect(cookie.value).to.equal("base");
                                    pm.expect(cookie.path).to.equal("/");
                                    pm.expect(cookie.sameSite).to.equal("Strict");
                                }});
                            }});
                            "#
                        ),
                    },
                    ..HttpRequest::default()
                },
                RequestVariables::new(HashMap::new(), None),
            )
            .await
            .unwrap();
        server.await;

        let tests = &execution.scripts[0].tests;
        assert_eq!(tests.len(), 4);
        for test in tests {
            assert_eq!(test.error, None, "{}", test.name);
        }
    });
}

#[test]
fn saved_cookies_reopen_including_session_cookies() {
    smol::block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data/cookies.json");
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nSet-Cookie: session=abc; Secure\r\nSet-Cookie: theme=dark; Max-Age=3600\r\n",
        ])
        .await;
        let jar = CookieJar::open(&path).unwrap();
        // Nothing changed yet, so there is nothing to write.
        jar.save().unwrap();
        assert!(!path.exists());

        get(&executor(true, &jar), url, Vec::new()).await;
        server.await;
        jar.save().unwrap();

        let reopened = CookieJar::open(&path).unwrap();
        let cookies = reopened.cookies();
        assert_eq!(cookies, jar.cookies());
        assert_eq!(cookies.len(), 2);
        assert!(cookies[0].secure && cookies[0].expires.is_none());
        assert!(cookies[1].expires.is_some());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "only the user can read saved cookies");
        }

        reopened.remove(&cookies[0]);
        reopened.save().unwrap();
        assert_eq!(CookieJar::open(&path).unwrap().cookies(), &cookies[1..]);

        std::fs::write(&path, "not json").unwrap();
        assert!(CookieJar::open(&path).is_err());
    });
}

#[test]
fn clearing_the_jar_removes_every_cookie() {
    smol::block_on(async {
        let (url, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\n",
        ])
        .await;
        let jar = CookieJar::new();

        get(&executor(true, &jar), url, Vec::new()).await;
        server.await;
        assert_eq!(jar.cookies().len(), 2);

        let revision = jar.revision();
        jar.clear();
        assert!(jar.cookies().is_empty());
        assert!(jar.revision() > revision);
    });
}
