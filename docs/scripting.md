# Request scripts

Open a request's **Scripts** section to write JavaScript before sending or after
receiving a response. **Snippets** provides examples; typing `pm.` offers the
available APIs. Scripts run immediately when you send the request. Test results
and console output appear in the response panel.

The editor uses a bundled TypeScript language service for JavaScript completion.
It suggests object fields inside calls such as `pm.sendRequest({ ... })`, common
header names wherever a header is named, type names for `pm.expect(value).to.be.a()`,
and JSON Schema formats. It also infers local variables and callback response
types, and displays types when you hover. For a value declared before the call,
add a JSDoc type such as `/** @type {RequestEagle.RequestOptions} */` or
`/** @type {RequestEagle.RequestHeaders} */` to get the same suggestions.
Function parameter hints highlight the current argument as you type or move the
caret; Escape dismisses them. No Node.js installation is needed. Scripts still
execute as JavaScript; TypeScript type annotations are not executable script syntax.

Scripts run against a snapshot. Request edits affect that send without changing
the request draft or saved file. An uncaught pre-request error prevents the
request; a failed `pm.test` is reported without stopping it. Post-response errors
retain the received response and appear alongside the test results.

## Reuse a response value

Save a login token in the login request's post-response script:

```js
pm.response.to.have.status(200);
pm.environment.set("token", pm.response.json().token);
```

Other requests in the same collection can then use `Bearer {{token}}` in their
Authorization header. Session values are shared across that collection's tabs,
including nested folders and reopened requests, until the workspace closes.
Requests outside collections share a separate workspace session. Collections
have separate sessions, keyed by their environment file path.

| API | Behavior |
| --- | --- |
| `pm.environment.get(name)` / `.has(name)` | Read the environment file values plus session changes. |
| `pm.environment.set(name, value)` | Set a session value; values are converted to strings. Names cannot be empty or start with `$`. |
| `pm.environment.unset(name)` | Hide the value for this session, including its file value. |
| `pm.environment.clear()` | Hide all values visible to this script. |
| `pm.environment.toObject()` | Copy the visible environment into an object. |
| `pm.environment.replaceIn(text)` | Substitute environment and generated values in text. |
| `pm.variables.get(name)` / `.has(name)` | Look up a local override first, then the environment. |
| `pm.variables.set(name, value)` | Set a value only for the current execution, including its post-response phase. |
| `pm.variables.unset(name)` / `.clear()` | Remove local overrides, revealing environment values. |
| `pm.variables.toObject()` / `.replaceIn(text)` | Use the combined environment and local values. |

Session changes never write `environment.toml`. File edits appear on subsequent
sends unless a session change shadows that key. Restarting the workspace clears
session changes and reveals the file values again. Changes from a script phase
are applied together after it finishes without an uncaught error; caught test
failures do not discard changes. A successful pre-request phase's changes remain
even if the HTTP request subsequently fails. Concurrent requests merge changes
to different keys; the last completed update to the same key wins.

Generated values are shared within one execution. Substitution is single-pass;
it does not expand references inside variable values or escape JSON strings.
`replaceIn` leaves unknown references unchanged. Request sending rejects unknown
and unclosed references; use `{{!name}}` to send a literal `{{name}}`.

## Make HTTP calls from a script

Top-level `await` and promises work in both phases. For example, acquire a token
before the primary request:

```js
const login = await pm.sendRequest({
    url: "{{base_url}}/login",
    method: "POST",
    headers: {"Content-Type": "application/json"},
    body: JSON.stringify({username: pm.environment.get("username")}),
});

login.to.have.status(200);
pm.environment.set("token", login.json().token);
pm.request.headers.upsert({key: "Authorization", value: "Bearer {{token}}"});
```

`pm.sendRequest(url)` sends GET. An object accepts `url`, `method`, `headers`
(an object, `{key, value}` entries, or `[key, value]` pairs), and `body` (text or
`{mode: "raw", raw: text}`). `header` is an alias for `headers`. Set Content-Type
explicitly when needed. URL, header, and body variables resolve at the time of
the call, using the same placeholder syntax as the primary request. URL fragments
and GET/HEAD bodies are ignored, including any variable references inside them.

The returned promise resolves to a response with `code`, `status`,
`responseTime` in milliseconds, `headers.get/has/toJSON`, `text()`, `json()`, and
the same response assertions as `pm.response`. HTTP 4xx/5xx statuses are normal
responses; transport and configuration failures reject. An optional second
argument is an `(error, response)` callback. Its thrown errors reject the call.

Subrequests share the primary request's proxy, TLS verification, HTTP version,
redirect settings, and response limits. Each also uses the configured request
timeout. Subrequests do not run saved request scripts or recursively execute the
current scripts. Their responses are available in JavaScript; log what you need
to inspect in the Console.

All started HTTP calls and promise-returning tests finish before the phase is
reported, including calls not explicitly awaited. Use `await` when their values
affect subsequent code. Async tests use a promise-returning function:

```js
pm.test("Related resource exists", async () => {
    const related = await pm.sendRequest("{{base_url}}/related");
    related.to.have.status(200);
});
```

Callback-style `done` tests, timers, package imports, filesystem access, and
browser APIs such as `fetch` are not provided. An unhandled rejection fails the
phase. A promise that cannot settle produces an error rather than hanging.

## Sign and encode values

```js
const body = pm.variables.replaceIn(pm.request.body.raw || "");
const signature = pm.crypto.hmacSha256(pm.environment.get("secret"), body);
pm.request.headers.upsert({key: "X-Signature", value: signature});
```

| API | Result |
| --- | --- |
| `pm.crypto.sha256(text)` | SHA-256 of UTF-8 text, lowercase hexadecimal. |
| `pm.crypto.hmacSha256(secret, text)` | HMAC-SHA256 with a UTF-8 secret, lowercase hexadecimal. |
| `pm.crypto.randomBytes(count)` | Cryptographically secure random bytes encoded as hexadecimal; the string has `count * 2` characters. |
| `pm.encoding.base64Encode(text)` / `.base64Decode(text)` | Standard padded Base64 and UTF-8 text decoding. |
| `pm.encoding.base64UrlEncode(text)` / `.base64UrlDecode(text)` | URL-safe Base64; encoding omits padding, decoding accepts padded or unpadded input. |

Malformed Base64 or decoded non-UTF-8 bytes throw an error. Crypto functions use
the maintained Rust `ring` implementation rather than handwritten algorithms.

## Validate response structure

```js
const schema = {
    type: "object",
    required: ["id", "name"],
    properties: {
        id: {type: "integer"},
        name: {type: "string"},
    },
};

pm.test("Response matches the schema", () => {
    pm.response.to.have.jsonSchema(schema);
});
```

For other JSON values, `pm.schema.validate(data, schema)` returns
`{valid, errors, truncated}`. Each error has `instancePath`, `schemaPath` (JSON
pointers), and a readable `message`. Invalid schemas throw; valid schemas with
nonmatching data return `valid: false`. Response assertions include error paths
in the failed test message.

Validation uses the schema's `$schema` dialect when supplied (default: Draft
2020-12), including format checks. Local acyclic JSON-pointer references such
as `#/$defs/user` work. External references, nested identifiers, dynamic or
recursive references, and regular-expression lookaround/backreferences are not
supported. Validation never loads files or makes network requests.

## Skip the primary request

Use this only in a pre-request script:

```js
if (!pm.environment.get("token")) {
    pm.execution.skipRequest("No access token configured");
}
```

The response panel shows **Request skipped** with the reason. The primary
request and post-response script do not run. Pending subrequests are cancelled;
calls already sent before skipping cannot be undone. Changes made to the
environment before the skip are retained. Treat skipping as an immediate exit;
do not catch its control-flow exception.

## Execution limits

- Each phase has a 32 MiB JavaScript heap, 2 seconds of active execution, and a
  30-second elapsed deadline. Waiting for HTTP does not consume the active budget.
  Native utility calls have input limits and are checked at JavaScript interruption
  points. Cancel stops pending HTTP work and prevents committing unfinished changes.
- A phase can make 32 HTTP calls, with at most 4 in flight. Each request's
  serialized configuration is limited to 1 MiB, each response body to 8 MiB
  (or the lower configured limit), and total response bodies to 16 MiB per phase.
- Session environments retain at most 4096 changed names and 1 MiB of names and
  values, including deletion markers. Updates that exceed the limit fail together.
- Crypto inputs and decoded Base64 are limited to 1 MiB. Random generation accepts
  integer counts from 0 through 65536.
- Schema data is limited to 1 MiB, 20,000 JSON nodes and depth 64; schemas to
  64 KiB, 1000 nodes and depth 32, with bounded reference expansion. A conservative
  combined budget also limits estimated validation work to 100,000 node visits
  and 16 MiB of data/schema/path bytes, counting repeated references, nested
  branches and literal values. Exceeding a budget throws before native validation.
  Schemas using `pattern`, `patternProperties` or the `regex` format require all
  data strings and property names to be at most 4096 bytes. A separate cumulative
  regex-work budget of 4096 units conservatively charges all data string/key bytes
  plus 64 units per regex occurrence and schema ancestor, including repeated references.
  `unevaluatedProperties` and `unevaluatedItems` are
  unsupported. At most 20 validation errors are returned; `truncated` indicates
  further errors.
- A phase records up to 500 tests and 500 console entries; individual messages
  are limited to 4096 characters. Exceeding the test limit fails the phase.
