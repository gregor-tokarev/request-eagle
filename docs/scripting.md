# Request scripts

Open an individual HTTP request's **Scripts** tab and choose **Pre-request** or
**Post-response**. **Snippets** inserts examples; **Send** (or its shortcut) runs
both phases. Collections and folders have no scripts.

Autocomplete suggests the supported API and parameters. Use Up/Down to choose,
Enter or a click to insert a name, and Escape to dismiss; then type the arguments.
Response suggestions appear only in Post-response; comments and strings suppress
the menu. Arbitrary JavaScript variable types are not inferred.

Editing scripts marks the request draft unsaved. Saving writes both phases under
`[request.scripts]` in the request's TOML file, by default at
`~/.request-eagle/collections/<collection>/<folders>/<request>.toml`.
`REQUEST_EAGLE_COLLECTIONS_DIR` overrides the collections directory.

Pre-request runs before URL validation and dispatch, modifying only the outgoing
snapshot. Post-response runs after the complete response, including HTTP errors.
**Test Results** and **Console** show the latest run's assertions and logs.
Failed tests continue; an uncaught pre-request error stops dispatch, while a
post-response error retains the response and opens Test Results.

```javascript
// Pre-request
pm.variables.set("name", "Eagle");
pm.request.headers.upsert({key: "X-Request-Id", value: pm.variables.replaceIn("{{$guid}}")});
console.log("Sending", pm.request.method, pm.request.url);
```

```javascript
// Post-response
pm.test("Status code is 200", function () {
    pm.response.to.have.status(200);
});
pm.test("Response reports success", function () {
    pm.expect(pm.response.json()).to.have.property("success", true);
});
console.log(pm.response.json());
```

Use `{{name}}` in URLs, query names/values, headers, or UTF-8 bodies. Variables
last for one execution and are shared across phases. Expansion is single-pass;
unknown placeholders remain unchanged, and the HTTP client URL-encodes query
values afterward.

Dynamic variables also work without scripts: `{{$guid}}` and `{{$randomUUID}}`
generate UUID v4 values, `{{$timestamp}}` Unix seconds, `{{$isoTimestamp}}` UTC ISO
timestamps, and `{{$randomInt}}` integers from 0–1000 inclusive. Each occurrence
is fresh; store a value with `pm.variables.set` to reuse it. Local variables take
precedence over dynamic names. Both share the request expansion limit below.

Videos: [walkthrough](demos/request-scripts.mp4),
[autocomplete close-up](demos/script-autocomplete.mp4),
[common API example](demos/common-script-api.mp4).

## Supported API

This is a subset of Postman's sandbox for request preparation and response tests.

- `pm.variables`: `get`, `set`, `has`, `unset`, `clear`, `toObject`, `replaceIn`.
- `pm.request`: writable `method` and string `url`; `headers.get/has/add/upsert/remove/toJSON`;
  `body.raw` and `body.update(string)`.
- `pm.response`: `code`, `status`, `responseTime` in milliseconds, `text()`, `json()`,
  `headers.get/has/toJSON`.
- Response shortcuts: `to.have.status(codeOrReason)` accepts a numeric code or
  status text such as `"Created"`; `to.have.header(name, value?)` checks a header.
  `to.have.body()` checks for a nonempty body, or accepts exact text, a regular
  expression, or a JSON object. `to.have.jsonBody(path?, value?)` checks JSON
  validity, a nested property, or its expected value.
  `to.be.ok/success/error/clientError/serverError/json` checks 200, 2xx, 4xx/5xx,
  4xx, 5xx, or valid JSON, respectively. These shortcuts are a fixed subset;
  use `pm.expect` for chained and negated assertions.
- `pm.test(name, callback)` and `pm.expect(value, message?)`.
- `pm.expect` provides Chai 4.5.0's built-in assertion API. Autocomplete covers
  common chains, including `oneOf`, `keys`, `members`, `nested.property`,
  `deep.include`, equality, types, numeric comparisons, lengths, and negation.
- `console.log/info/warn/error/debug`.

References: [Postman's common examples](https://learning.postman.com/docs/tests-and-scripts/write-scripts/test-examples/)
and [Chai assertions](https://www.chaijs.com/api/bdd/).

## Limits

Scripts are synchronous and request-only. Persistent environment/global/collection
variables, cookies, request chaining (`pm.sendRequest`), promises, async callbacks,
timers, external packages, JSON Schema validation, `pm.test.skip`, Postman-specific
Chai plugins, and host file/network/process APIs are not supported. The request
URL is a string, not Postman's SDK `Url` object; imported scripts may need changes.

Each phase gets a fresh QuickJS runtime via `rquickjs`, with a 2-second deadline,
32 MiB of JS memory, and 500 tests. Request text expansion is capped at 32 MiB.
Console output keeps the first 500 messages per phase; messages, test names, and
test errors are capped at 4,096 characters each. Cancellation interrupts either
phase. The request timeout includes pre-request and HTTP transfer; post-response
uses its own deadline to retain the response. HTTP elapsed time excludes scripts.

## Visual reference

The [downloaded Postman screenshot](references/postman-request-scripts.jpg)
([source](https://assets.postman.com/postman-docs/v11/pre-request-script-v11-12.jpg))
guides the phase navigation, numbered editor, and Snippets control. Styling follows
[Request Eagle's design guidelines](design-guidelines.md); the reference is not an app asset.
