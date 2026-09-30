# Request execution

This crate owns the request data shared by saved collections and drafts, request
preferences, and the executor. It does not depend on GPUI application or UI types.
`collection` and `preferences` re-export their original types, so their imports and
serialized files remain compatible.

TLS certificate verification is enabled by default. New profiles and preference
files that omit `ssl_certificate_verification` verify server certificates. An
explicit stored value is preserved, including `false` saved by older versions.

```rust,no_run
use environment::VariableValues;
use request::{HttpRequest, RequestExecutor, RequestPreferences, RequestVariables, Response};

let executor = RequestExecutor::new(&RequestPreferences::default())?;
let request = HttpRequest {
    path: "https://example.com/{{route}}".into(),
    ..HttpRequest::default()
};
let values = VariableValues {
    environment: [("route".into(), "api".into())].into(),
};

// The returned future owns its inputs and can be spawned on a background
// executor without borrowing a tab.
let run = executor.execute(request, RequestVariables::new(values, None));
let execution = smol::block_on(run)?;
let Response::Http(response) = execution.response;
println!("{}: {} bytes in {:?}", response.status, response.body.len(), execution.elapsed);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`execute` runs the pre-request scripts, resolves `{{variables}}` in the URL,
headers, query and body, applies the editor's send defaults
(`HttpRequest::prepare_for_send`: an `https://` scheme for URLs without one, no
body for GET and HEAD, and a JSON `Content-Type` for bodies without one), sends
the request, then runs the post-response scripts.

The executor reuses its HTTP connection pool. Construct a new executor when
preferences change. HTTP/HTTPS execution supports GET, POST, PUT, PATCH, DELETE,
HEAD, and OPTIONS, repeated headers and query pairs, binary bodies, and HTTP
version selection. The complete operation, including the response body, is
bounded by the timeout. Dropping its future cancels the operation. Response
limits apply to both downloaded and decompressed body bytes, including chunked
responses; zero disables either limit. The stored size setting uses MiB.

Proxy preferences default to the system/environment proxy. Custom mode supports
HTTP or HTTPS proxy servers, separate HTTP/HTTPS request selection, Basic proxy
authentication, and comma-separated bypass hosts, domains, and IP ranges. A domain
also matches its subdomains; `*.example.com` and `.example.com` are accepted, and
`*` bypasses all destinations. Bypassed or unselected request types connect
directly. Disabled mode ignores all proxies. Proxy settings are saved with the
other local preferences and apply to the next request. Credentials are held in
memory by this crate; the preferences crate persists them in the OS keyring,
with only a credential reference in the preferences file.

HTTP redirects (301, 302, 303, 307, and 308) are followed by default, with a
limit of 100 redirects to stop loops. Explicit `Host` overrides are preserved on
the same authority and removed when a redirect changes the host or port.
Set `follow_all_redirects` to `false` to inspect redirect responses directly.
Final HTTP statuses, including 4xx and 5xx, are returned with their headers and
body. Headers retain repeated and non-UTF-8 values. Requests advertise gzip unless
an explicit Accept-Encoding overrides it or the request contains Range. Complete
gzip responses are decompressed while their original headers and downloaded byte
counts are retained. Partial byte-range responses and unsupported content encodings
remain unchanged. Malformed input, transport failures, corrupt
gzip streams, truncated bodies, timeouts, and oversized responses return typed errors.

`Request` and `Response` are protocol enums. HTTP details live in `http.rs`, while
`executor.rs` runs the scripts and sends the request. Only HTTP/HTTPS is implemented.

Run local-server tests, with request/response and error output:

```sh
cargo test -p request -- --nocapture --test-threads=1
```

Request scripts expose the [scripting API](../../docs/scripting.md), including
awaitable HTTP subrequests, signing, schema validation and request skipping.
`ExecutionError::Skipped` is a deliberate pre-request outcome, carrying the
reason and script report without an HTTP response.

To retain `pm.environment` changes between executions, pass
`RequestVariables::with_environment_session(file_values, file_error, session)`
to `execute`. Reuse the same `environment::EnvironmentSession` handle for
requests sharing an environment and create a fresh snapshot for each execution.
`RequestVariables::new` has no persistent session; its script values last only
for that execution. Session changes are in-memory and do not write environment
files.

TLS tests generate fresh self-signed certificates and private keys in memory for
their loopback servers. No certificate or key fixtures are stored on disk.
