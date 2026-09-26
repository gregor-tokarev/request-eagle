# Request scripts

Open an individual HTTP request, select **Scripts**, and choose **Pre-request**
or **Post-response**. Each phase has its own JavaScript editor. **Snippets** inserts
an example into the selected phase. **Send** (or the configured send shortcut)
runs the scripts. Saving the request also saves both scripts; editing them marks
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

## Supported API

This is a small Postman-style API, not a complete Postman sandbox implementation.

- `pm.variables`: `get`, `set`, `has`, `unset`, `clear`, `toObject`, `replaceIn`.
- `pm.request`: writable `method` and string `url`; `headers.get/has/add/upsert/remove/toJSON`;
  `body.raw` and `body.update(string)`.
- `pm.response`: `code`, `status`, `responseTime` in milliseconds, `text()`, `json()`,
  `headers.get/has/toJSON`, `to.have.status(code)`, `to.have.header(name, value?)`.
- `pm.test(name, callback)` and `pm.expect(value, message?)`.
- Assertions: `equal/equals/eq`, `eql` or `deep.equal` for JSON values, `include`,
  `property`, `a/an`, `above`, `below`, `least`, `most`, `lengthOf`, `match`,
  `true`, `false`, `null`, `undefined`, `ok`, `empty`, and `not`.
- `console.log/info/warn/error/debug`.

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
