# Request scripts

Open a request's **Scripts** section to write JavaScript that runs before
sending or after receiving a response. The **Snippets** menu and `pm.`
completion list the available APIs.

Scripts run against a snapshot: their request edits affect that send without
changing the request draft or saved file. An uncaught pre-request error
prevents the request; a failed `pm.test` does not.

## Collection scripts

Click a collection in the sidebar to open its tab. There you can rename it,
edit the variables in its `environment.toml`, and write scripts that run for
every request in the collection. Save with the **Save** button or the save
shortcut.

In each phase the collection's script runs first, then the request's own
script. Both scripts share `pm.variables` for that send. If the collection's
pre-request script fails, the request is not sent. If its post-response script
fails, the request's own tests still run. The tab stores the scripts in the
collection's `.request-eagle-collection.toml`.

## Reuse a response value

```js
pm.response.to.have.status(200);
pm.environment.set("token", pm.response.json().token);
```

Other requests in the same collection can then use `Bearer {{token}}`. Session
values last until the workspace closes and are never written to
`environment.toml`.

File values come from the collection's `environment.toml` and from the global
environment selected in the tab bar. The selected environment's values take
precedence. Global environments are stored in
`~/.request-eagle/environments/<name>.toml`.

Scripts read and change values in these scopes. A `{{name}}` reference uses
the first one that has the name:

| API | Scope |
| --- | --- |
| `pm.variables` | Overrides that last for the current execution only. |
| `pm.environment` | The selected environment, then the collection's variables. |
| `pm.collectionVariables` | The collection's `environment.toml`. |
| `pm.globals` | Values every collection shares. They start empty when the workspace opens. |

Each scope has the same methods:

| Method | Behavior |
| --- | --- |
| `get(name)` / `has(name)` | Read the file values plus session changes. |
| `set(name, value)` | Set a value for the scope's lifetime, converted to a string. |
| `unset(name)` / `clear()` | Hide values for this session, including file values and those of the scopes below. `pm.variables` only removes its overrides. |
| `toObject()` / `replaceIn(text)` | Copy the visible values, or substitute them in text. |

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
Like the primary request, they store and send cookies in the cookie jar, so a
login subrequest's session cookie goes with the request that follows.
Timers, filesystem access, and `fetch` are not provided.

## Read and change the body

`pm.request.body.mode` names the body's type, as in Postman: `raw`,
`urlencoded`, `formdata` or `file`.

| API | Body |
| --- | --- |
| `pm.request.body.raw` | Raw text. Setting it, or calling `update(text)`, makes any body raw text. After the response, it is what was sent. |
| `pm.request.body.urlencoded` | A URL-encoded form's fields: `get`, `has`, `add`, `remove`, `upsert`, `clear` and `toJSON`, like headers. |
| `pm.request.body.formdata` | A multipart form's parts, with the same methods. Text parts have a `value`; file parts have `type: "file"` and the file's path in `src`. |
| `pm.request.body.file.src` | The path of the file a binary body sends. |

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

## Use Postman's libraries

```js
const CryptoJS = require("crypto-js");
const moment = require("moment");

const timestamp = moment().utc().format();
const signature = CryptoJS.HmacSHA256(timestamp, pm.environment.get("secret"));
pm.request.headers.upsert({key: "X-Timestamp", value: timestamp});
pm.request.headers.upsert({key: "X-Signature", value: CryptoJS.enc.Base64.stringify(signature)});
```

`require` loads these libraries, as Postman does:

| Name | Library |
| --- | --- |
| `crypto-js` | crypto-js 4.2.0, also available as `CryptoJS`. |
| `lodash` | Lodash 4.17.21, also available as `_`. |
| `moment` | Moment.js 2.30.1. |
| `uuid` | `uuid()` and `uuid.v4()` return a random UUID. |
| `atob` / `btoa` | Base64 for text with one byte per character, also available as globals. |

Other names throw an error.

## Read and change cookies

```js
pm.environment.set("session", pm.cookies.get("session"));
```

`pm.cookies` lists the cookies of the current exchange: those the request
sends, from its `Cookie` header and the cookie jar, and after the response,
with the response's `Set-Cookie` headers applied. `pm.response.cookies` lists
only the cookies the response set, with their attributes. Both have
`get(name)`, `has(name)`, `one(name)`, `all()`, `count()` and `toObject()`.

`pm.cookies.jar()` reads and changes the cookie jar. As in Postman, its
methods report through a callback:

```js
const jar = pm.cookies.jar();
jar.set("https://api.example.com", "session", "abc", error => {
    if (error) throw error;
});
jar.get("https://api.example.com", "session", (error, value) => console.log(value));
```

| Method | Behavior |
| --- | --- |
| `get(url, name, callback)` | The value of the cookie named `name` that a request to `url` sends. |
| `getAll(url, callback)` | Every cookie a request to `url` sends, with its attributes. |
| `set(url, name, value, callback)` | Keep a cookie as if a response from `url` set it. Instead of `name` and `value`, an object can also give `path`, `domain`, `expires`, `maxAge`, `secure`, `httpOnly` and `sameSite`. |
| `unset(url, name, callback)` | Delete the cookies named `name` that a request to `url` sends. |
| `clear(url, callback)` | Delete every cookie a request to `url` sends. |

Changes made before sending apply to the request. When the cookie jar is
off in Settings, `pm.cookies` lists only the cookies of the exchange and each
jar method reports an error; without a callback, the error is logged.

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

Request Eagle sends one request at a time, so
`pm.execution.setNextRequest(name)` and `postman.setNextRequest(name)` have
no effect, as in Postman outside the Collection Runner. The legacy `postman`
object also has `getEnvironmentVariable`, `setEnvironmentVariable`,
`clearEnvironmentVariable`, `getGlobalVariable`, `setGlobalVariable` and
`clearGlobalVariable`.

## gRPC scripts

A gRPC request's **Scripts** section has three scripts:

- **Before invoke** runs before the method is invoked. It can change the
  server URL, metadata and message of that call, and
  `pm.execution.skipRequest(reason)` cancels it. It runs before server
  reflection loads the call's methods, so it can also set the metadata that
  reflection needs.
- **On message** runs for each message the server sends, in order.
- **After response** runs once the server ends the call, including with an
  error status. It does not run when the call fails without a status, such as
  when the server is unreachable, or when you cancel it.

The three scripts share `pm.variables` during the call. A `{{$guid}}` or other
generated value that Before invoke creates or sets keeps that value for the
whole call, including stream messages. Collection scripts run only for HTTP
requests.

```js
// Before invoke
const auth = await pm.sendRequest({url: "{{base_url}}/login", method: "POST"});
pm.request.metadata.upsert({key: "authorization", value: "Bearer " + auth.json().token});
```

```js
// On message
pm.test("Every update has an ID", () => {
    pm.expect(pm.message.data).to.have.property("id");
});
```

```js
// After response
pm.test("Status is OK", () => pm.response.to.have.status("OK"));
pm.test("An update reports success", () => {
    pm.response.messages.to.include({status: "success"});
});
pm.environment.set("orderId", pm.response.messages.idx(0).data.id);
```

Messages appear as JSON, with lowerCamelCase field names and 64-bit integers
as strings.

| API | Behavior |
| --- | --- |
| `pm.request.url` / `.message` | The server and the composed message as text. Before invoke can assign them; an object assigned to `message` becomes JSON. Unary and server streaming methods send the message when invoked. |
| `pm.request.methodPath` | The method as `package.Service/Method`. |
| `pm.request.metadata` | `get`, `has`, `add`, `remove`, `upsert`, `clear` and `toJSON`, like headers. Keys are case-insensitive. |
| `pm.message.data` / `.timestamp` | In On message, the received message and when it arrived. |
| `pm.response.code` / `.status` / `.statusMessage` | In After response, the status code (0 is OK), its name such as `NOT_FOUND`, and the server's message. |
| `pm.response.responseTime` | Milliseconds from invoking until the status arrived. |
| `pm.response.metadata` / `.trailers` | The metadata and trailers the server sent. |
| `pm.response.messages` / `pm.request.messages` | In After response, the received and sent messages, each with `data` and `timestamp`. |
| `pm.response.to.have.statusCode(code)` / `.status(codeOrName)` | Assert the status. `pm.response.to.be.ok` asserts OK, and `.error` any other status. |
| `pm.response.to.have.metadata(key, value?)` / `.trailer(key, value?)` | Assert that the server sent a key, and optionally its value. |
| `pm.response.to.have.message(object)` | Assert that a received message equals the object. |

Message lists are arrays with `idx(index)`, `count()`, `all()` and
`each(callback)`. Their `filter` also accepts the fields to match, such as
`filter({data: {type: "ping"}})`.

| Assertion | Passes when |
| --- | --- |
| `messages.to.include(fields)` | A message has these fields. Nested objects match the fields they name. |
| `messages.to.not.include(fields)` | No message has these fields. |
| `messages.to.have.property(path, value?)` | Every message has the property, such as `user.id`, optionally with this value. There must be a message. |
| `messages.to.have.jsonSchema(schema)` | Every message matches the schema. There must be a message. |

## Limits

Each phase is limited to:

- 2 seconds of active execution, 30 seconds elapsed, and a 32 MiB heap.
- 32 HTTP calls with 4 in flight, 8 MiB per response and 16 MiB in total.
- 500 tests and 500 console entries.
- 1 MiB for crypto, Base64, and schema data, and 64 KiB for a schema.

On message runs once for each received message, with these limits for each
run. A call shows up to 500 tests and 500 console entries from all of them.
After response sees the latest 8 MiB of messages in each direction.
