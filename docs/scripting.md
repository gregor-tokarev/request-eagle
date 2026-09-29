# Request scripts

Open a request's **Scripts** section to write JavaScript that runs before
sending or after receiving a response. The **Snippets** menu and `pm.`
completion list the available APIs.

Scripts run against a snapshot: their request edits affect that send without
changing the request draft or saved file. An uncaught pre-request error
prevents the request; a failed `pm.test` does not.

## Reuse a response value

```js
pm.response.to.have.status(200);
pm.environment.set("token", pm.response.json().token);
```

Other requests in the same collection can then use `Bearer {{token}}`. Session
values last until the workspace closes and are never written to
`environment.toml`.

| API | Behavior |
| --- | --- |
| `pm.environment.get(name)` / `.has(name)` | Read the environment file values plus session changes. |
| `pm.environment.set(name, value)` | Set a session value, converted to a string. |
| `pm.environment.unset(name)` / `.clear()` | Hide values for this session, including file values. |
| `pm.environment.toObject()` / `.replaceIn(text)` | Copy the visible values, or substitute them in text. |
| `pm.variables.*` | The same methods, with overrides that last for the current execution only. |

Use `{{!name}}` to send a literal `{{name}}`.

## Make HTTP calls from a script

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

`pm.sendRequest(url)` sends GET. The response has `code`, `status`,
`responseTime`, `headers`, `text()`, `json()`, and the same assertions as
`pm.response`. HTTP 4xx/5xx statuses are normal responses; transport failures
reject.

Subrequests use the primary request's settings and do not run saved scripts.
Timers, package imports, filesystem access, and `fetch` are not provided.

## Sign and encode values

```js
const body = pm.variables.replaceIn(pm.request.body.raw || "");
const signature = pm.crypto.hmacSha256(pm.environment.get("secret"), body);
pm.request.headers.upsert({key: "X-Signature", value: signature});
```

| API | Result |
| --- | --- |
| `pm.crypto.sha256(text)` | SHA-256, lowercase hexadecimal. |
| `pm.crypto.hmacSha256(secret, text)` | HMAC-SHA256, lowercase hexadecimal. |
| `pm.crypto.randomBytes(count)` | Secure random bytes, hexadecimal. |
| `pm.encoding.base64Encode(text)` / `.base64Decode(text)` | Standard padded Base64. |
| `pm.encoding.base64UrlEncode(text)` / `.base64UrlDecode(text)` | URL-safe Base64 without padding. |

## Validate response structure

```js
pm.test("Response matches the schema", () => {
    pm.response.to.have.jsonSchema({
        type: "object",
        required: ["id", "name"],
        properties: {
            id: {type: "integer"},
            name: {type: "string"},
        },
    });
});
```

For other values, `pm.schema.validate(data, schema)` returns
`{valid, errors, truncated}`. The default dialect is Draft 2020-12. Local
references such as `#/$defs/user` work; external references are not loaded.

## Skip the primary request

```js
if (!pm.environment.get("token")) {
    pm.execution.skipRequest("No access token configured");
}
```

The primary request and post-response script do not run.

## Limits

Each phase is limited to:

- 2 seconds of active execution, 30 seconds elapsed, and a 32 MiB heap.
- 32 HTTP calls with 4 in flight, 8 MiB per response and 16 MiB in total.
- 500 tests and 500 console entries.
- 1 MiB for crypto, Base64, and schema data, and 64 KiB for a schema.
