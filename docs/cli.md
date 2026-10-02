# Request Eagle CLI

`request-eagle-cli` is an optional standalone binary for AI agents. It manages
saved collections and requests, runs them, and edits settings, without the
desktop app or a display.

## Install

Download the executable for your platform from the
[releases page](https://github.com/gregor-tokarev/request-eagle/releases/latest):

| Platform | Attachment |
| --- | --- |
| Apple Silicon macOS | `request-eagle-cli-aarch64-apple-darwin` |
| x86-64 Linux (glibc 2.35+) | `request-eagle-cli-x86_64-unknown-linux-gnu` |
| ARM64 Linux (glibc 2.39+) | `request-eagle-cli-aarch64-unknown-linux-gnu` |

```sh
install -m 755 request-eagle-cli-TARGET ~/.local/bin/request-eagle-cli
```

Or build it from a source checkout:

```sh
cargo install --path crates/request-eagle-cli --locked
```

## Usage

The CLI describes its own commands:

```sh
request-eagle-cli --help
request-eagle-cli schema
request-eagle-cli call '{"command":"requests.list","query":"health"}'
request-eagle-cli call '{"command":"requests.run","path":"/absolute/path/Health.toml"}'
```

Each command writes one JSON object to stdout:

```json
{"version":1,"ok":true,"result":{}}
{"version":1,"ok":false,"error":{"code":"operation_failed","message":"details"}}
```

Exit codes are 0 for success, 1 for a failure, and 2 for invalid input. HTTP
error statuses are completed responses and exit 0; check `status` and the
script results.

## Things to know

- Quit the desktop app before editing its data and reopen it to load the
  changes. There is no live synchronization.
- `requests.update` replaces the complete request. Read it first and preserve
  the fields you do not intend to change.
- WebSocket requests appear with a `websocket` object instead of `request`.
  The CLI can read, move, rename and delete them, but only the app connects
  to them or edits them.
- `call -` reads JSON from stdin, which keeps secrets out of process arguments.
- Saved scripts run only with `trust_scripts: true`. Read them first, including
  the collection's scripts from `collections.get`, which run before the request's.
- Response bodies and raw header values are Base64.
- An HTTP request's `body` has a `type`: `raw` with a `language` (`json`,
  `xml` or `text`) and its `text`, `url_encoded` with `fields`, `multipart`
  with `parts`, or `binary` with a `file`. A string alone is raw JSON. A
  multipart part with `"file": true` sends the file at its `value`. Relative
  file paths start at the collection's directory.
- HTTP requests can set their own `timeout_ms`, `follow_redirects` and
  `verify_certificates`. Fields left out follow `settings.request`, and the
  `timeout_ms` of `requests.run` replaces both for that run.
- Requests can set an `auth`, such as `{"type": "bearer", "token":
  "{{token}}"}`. Without one, a request inherits its collection's, which
  `collections.get` shows; `{"type": "none"}` sends none. Kinds are `api_key`,
  `bearer`, `basic`, `digest`, `oauth1`, `oauth2`, `jwt` and `aws_signature`;
  gRPC requests cannot use `digest`, `oauth1` or `aws_signature`. OAuth 2.0
  sends its saved `access_token`; get a new one in the app.
- `settings.request` takes `ca_certificates`, a PEM file of certificate
  authorities trusted in addition to the system's.
  `settings.client_certificates.add` checks a client certificate's files and
  saves it for mutual TLS with one host; its passphrase goes to the OS
  credential store, so pass it through stdin.
- Saved gRPC requests have `"protocol": "grpc"`, a `url`, a `method` such as
  `package.Service/Method` and a JSON `message`. Without `proto_file` their
  services come from server reflection. `requests.run` sends the message once,
  also on client streams, and returns every message with the final status;
  non-OK gRPC statuses exit 0 like HTTP errors. Their `before_invoke`,
  `on_message` and `after_response` scripts also need `trust_scripts: true`.
- `requests.run` keeps the cookies that responses set in the app's cookie
  jar, `cookies.json` in the data directory, and sends them with later runs to
  the same sites, as the app does. `cookies.list` shows them and
  `cookies.delete` removes a domain's cookies, or one by name. Turn the jar off
  with `settings.request` and `"cookie_jar": false`.
- Data lives in `~/.request-eagle`. `--data-dir`, `--collections-dir`, and
  `REQUEST_EAGLE_COLLECTIONS_DIR` select another location.
