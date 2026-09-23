# Request execution

This crate owns the request data shared by saved collections and drafts, request
preferences, and the executor. It does not depend on GPUI application or UI types.
`collection` and `preferences` re-export their original types, so their imports and
serialized files remain compatible.

```rust,no_run
use request::{HttpRequest, RequestExecutor, RequestPreferences, Response};

let executor = RequestExecutor::new(&RequestPreferences::default())?;
let draft = HttpRequest {
    path: "https://example.com/api".into(),
    ..HttpRequest::default()
};

// Also accepts a saved Request, by value or reference. The returned future owns
// a snapshot and can be spawned on a background executor without borrowing a tab.
let run = executor.execute(&draft);
let execution = smol::block_on(run)?;
let Response::Http(response) = execution.response;
println!("{}: {} bytes in {:?}", response.status, response.body.len(), execution.elapsed);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The executor reuses its HTTP connection pool. Construct a new executor when
preferences change. HTTP/HTTPS execution supports GET, POST, PUT, PATCH, DELETE,
HEAD, and OPTIONS, repeated headers and query pairs, binary bodies, and HTTP
version selection. The complete
operation, including the response body, is bounded by the timeout. Dropping its
future cancels the operation. Response limits apply to both downloaded and decompressed body bytes, including
chunked responses; zero disables either limit. The stored size setting uses MiB.

Proxy preferences default to the system/environment proxy. Custom mode supports
HTTP or HTTPS proxy servers, separate HTTP/HTTPS request selection, Basic proxy
authentication, and comma-separated bypass hosts, domains, and IP ranges. A domain
also matches its subdomains; `*.example.com` and `.example.com` are accepted, and
`*` bypasses all destinations. Bypassed or unselected request types connect
directly. Disabled mode ignores all proxies. Proxy settings are saved with the
other local preferences and apply to the next request. Credentials are held in
memory by this crate; the preferences crate persists them in the OS keyring,
with only a credential reference in the preferences file.
In Settings > Proxy, pasting a full proxy URL into the host field fills the
protocol, hostname, port, and authentication fields. Valid edits save automatically.

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

URLs must be absolute and already resolved. Collection environment substitution,
authentication editors, and UI Send actions are outside this module.

`Request` and `Response` are protocol enums. HTTP details live in `http.rs`, while
`executor.rs` dispatches requests. Add WebSocket, GraphQL, gRPC, or another protocol
with its own request/response variants and execution module. A streaming protocol
can return a session handle in its response variant instead of an HTTP-style
buffered body. Only HTTP/HTTPS is implemented today.

Run local-server tests, with request/response and error output:

```sh
cargo test -p request -- --nocapture --test-threads=1
```

TLS tests generate fresh self-signed certificates and private keys in memory for
their loopback servers. No certificate or key fixtures are stored on disk.
