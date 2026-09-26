# Request scripts

Open an individual HTTP request, select **Scripts**, and choose **Pre-request**
or **Post-response**. Each phase has its own JavaScript editor. **Snippets** inserts
an example into the selected phase. **Send** (or the configured send shortcut)
runs the scripts.

The editor suggests supported methods and properties as you type, including
`pm.variables.`, `pm.request.headers.`, `pm.response.`, `pm.expect(...).to.be.`,
`console.` and `JSON.`. The menu shows method parameters. Use Up/Down to choose,
Enter or a click to insert the name, and Escape to dismiss. Type the arguments
after accepting a method. Response APIs are suggested only in Post-response;
comments and string literals do not open the menu. This completes the known
scripting API rather than inferring types for arbitrary JavaScript variables.
[Watch the close-up autocomplete demo](demos/script-autocomplete.mp4).

Saving the request also saves both scripts; editing them marks
the tab as an unsaved request draft. Collections and folders have no script editor.

Pre-request scripts run before URL validation and HTTP dispatch. They can edit
the outgoing request and set variables without changing the draft. Post-response
scripts run after the complete response body is available, including HTTP error
statuses. **Test Results** shows each assertion; **Console** shows logs from both
phases for the latest run. Failing assertions continue to the next test. An
uncaught pre-request error stops the request; a post-response error keeps the
response available and opens Test Results.

```javascript
// Pre-request
pm.variables.set("name", "Eagle");
pm.request.headers.upsert({key: "X-Request-Id", value: String(Date.now())});
console.log("Sending", pm.request.method, pm.request.url);
```

Use `{{name}}` in the URL, query parameter names/values, headers or a UTF-8 body.
Variables last for one execution and are shared with its post-response script.
Unresolved placeholders remain unchanged. Substitution is a single pass; query
parameter values are URL-encoded by the HTTP client after substitution.

Common dynamic variables also work in request URLs, query parameters, headers,
and UTF-8 bodies, including when no script is present: `{{$guid}}`,
`{{$randomUUID}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`, and `{{$randomInt}}`.
In scripts, use `pm.variables.replaceIn("{{$guid}}")`. UUIDs are version 4,
timestamps are Unix seconds or UTC ISO timestamps, and random integers are in
the inclusive range 0–1000. Each occurrence is generated separately. Store a
generated value with `pm.variables.set` to reuse the same value throughout a run.
A local variable with the same name takes precedence; unknown placeholders stay
unchanged. Dynamic values use the same 32 MiB expansion limit as script variables.

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

[Watch the native app walkthrough](demos/request-scripts.mp4). It shows editing both
phases, inserting a snippet, sending, inspecting tests and logs, correcting a
failing assertion, and reopening the saved request.
[Watch the common API example](demos/common-script-api.mp4) for a generated
request ID, allowed status codes, JSON keys, nested values, and response shortcuts.

## Supported API

This focuses on individual-request preparation and response testing. It supports
local variables, generated request data, request edits, JSON/text/header checks,
and Chai assertions. It is not a complete Postman sandbox implementation.

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
- `pm.expect` uses the bundled Chai 4.5.0 assertion library. Common completions
  include `oneOf`, `keys`, `members`, `nested.property`, `deep.include`,
  `equal/equals/eq`, `eql`, `a/an`, `within`, `closeTo`, `above/below`,
  `least/most`, `length/lengthOf`, `match`, `exist`, `empty`, and `not`.
  Chained modifiers such as `any/all`, `own`, `ordered`, `include`, and `deep`
  follow Chai semantics. The rest of Chai's built-in `expect` API is available;
  autocomplete highlights the common methods.
- `console.log/info/warn/error/debug`.

For example:

```javascript
pm.test("Expected status and JSON shape", () => {
    pm.expect(pm.response.code).to.be.oneOf([200, 201, 202]);
    const data = pm.response.json();
    pm.expect(data).to.include.all.keys("id", "name");
    pm.expect(data).to.have.nested.property("user.roles").that.includes("admin");
});
```

Persistent environment/global/collection variables need an environment workflow
and are deferred; `pm.variables` remains local to one execution. The request URL
is currently a string, rather than Postman's SDK `Url` object. Cookies, request
chaining, JSON Schema validation, `pm.test.skip`, and Postman-specific Chai
plugins are not included. The supported surface is based on
[Postman's common script examples](https://learning.postman.com/docs/tests-and-scripts/write-scripts/test-examples/)
and [Chai's assertion reference](https://www.chaijs.com/api/bdd/), without a claim
of full compatibility for imported scripts.

Scripts are synchronous. Async callbacks, promises, timers, `pm.sendRequest`,
external packages, persistent environment/global/collection variables, and host
file/network/process APIs are not supported. Each phase gets a fresh QuickJS
runtime through the Rust `rquickjs` binding. Limits are 2 seconds, 32 MiB of JS
memory, 32 MiB for the expanded request text, and 500 tests per phase. Console
output retains the first 500 messages;
individual log messages, test names and test errors are limited to 4,096 characters.
Cancellation interrupts either script phase. The request timeout includes the
pre-request script and HTTP transfer; post-response scripts use their own 2-second
limit so the completed response is retained. HTTP elapsed time excludes scripts.

## Visual reference

The editor follows Postman's request-level Scripts layout: phase navigation on
the left, a numbered JavaScript editor, and a Snippets control. Its colors, spacing,
controls and corner radii follow Request Eagle's GPUI Kit design guidelines.

[Downloaded Postman screenshot](references/postman-request-scripts.jpg), from
[Postman's pre-request script documentation](https://learning.postman.com/v11/docs/tests-and-scripts/write-scripts/pre-request-scripts/).
The screenshot is a third-party design reference, not an application asset.
[Original image](https://assets.postman.com/postman-docs/v11/pre-request-script-v11-12.jpg).
