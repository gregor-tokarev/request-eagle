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
preferences change. HTTP/HTTPS execution supports the existing methods, repeated
headers and query pairs, binary bodies, and HTTP version selection. The complete
operation, including the response body, is bounded by the timeout. Dropping its
future cancels the operation. Response limits count body bytes, including for
chunked responses; zero disables either limit. The stored size setting uses MiB.

HTTP statuses, including redirects, 4xx, and 5xx, are returned with their headers
and body. Redirects are not followed. Headers retain repeated and non-UTF-8 values;
bodies are not decoded or decompressed. Malformed input, transport failures,
truncated bodies, timeouts, and oversized responses return typed errors.

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
