# Request Eagle CLI

`request-eagle-cli` is an optional standalone binary for AI agents. It manages
saved collections and requests, executes HTTP requests, and edits application
settings. It calls the same `collection`, `request`, and `preferences` backends as
the desktop app. It works without launching the app or connecting to a display.

## Install

**Settings → General → Request Eagle CLI** links to the release downloads and
these instructions. The CLI is a separate release attachment and is never
included in the desktop bundle or the default workspace build.

Download the executable for your platform from the
[releases page](https://github.com/gregor-tokarev/request-eagle/releases/latest):

| Platform | Attachment |
| --- | --- |
| Apple Silicon macOS | `request-eagle-cli-aarch64-apple-darwin` |
| x86-64 Linux (glibc 2.35+) | `request-eagle-cli-x86_64-unknown-linux-gnu` |
| ARM64 Linux (glibc 2.39+) | `request-eagle-cli-aarch64-unknown-linux-gnu` |

The macOS binary is signed and notarized. The release's `SHA256SUMS` includes the
CLI attachments. After downloading, install the matching file into a directory
on your PATH, for example:

```sh
mkdir -p ~/.local/bin
# Replace TARGET with your platform from the table above.
install -m 755 request-eagle-cli-TARGET ~/.local/bin/request-eagle-cli
export PATH="$HOME/.local/bin:$PATH"
request-eagle-cli --version
```

From a source checkout:

```sh
cargo install --path crates/request-eagle-cli --locked
```

No access token, socket, daemon, or running desktop application is required.
Remove the installed executable to uninstall.

## Agent workflow

Discover the complete command schema before constructing commands:

```sh
request-eagle-cli schema
request-eagle-cli call '{"command":"collections.list"}'
request-eagle-cli call '{"command":"requests.list","query":"health"}'
```

Use `--help` for CLI options and `call --help` for JSON input usage.

Use the absolute paths and IDs returned by the list/get commands. To enumerate
folders and requests in a collection, use `collections.get` with its `path`.

```sh
request-eagle-cli call '{"command":"requests.get","path":"/absolute/path/Health.toml"}'
request-eagle-cli call - <<'JSON'
{
  "command": "requests.update",
  "path": "/absolute/path/Health.toml",
  "expected_id": "ID returned by requests.get",
  "request": {
    "method": "GET",
    "url": "{{base_url}}/health",
    "headers": [["Accept", "application/json"]],
    "query": [],
    "body": null,
    "pre_request": "",
    "post_response": ""
  }
}
JSON
request-eagle-cli call '{"command":"requests.run","path":"/absolute/path/Health.toml","timeout_ms":10000}'
```

`requests.update` replaces the complete request, so read it first and preserve
fields you do not intend to change. It retains the file's identity, comments,
and unrelated TOML metadata. `expected_id` rejects replacement of a different
request at that path; it is not a content revision or a merge mechanism.

Create requests with `requests.create` (`parent`, `name`, `request`). Create
collections and folders with `collections.create` and `folders.create`; these
use the application's default names, which `entries.rename` can change. Delete
any saved entry with `entries.delete` and `confirm: true`. Move entries with
`entries.move`, a `target` path, and `placement: before`, `after`, or `inside`.

## Data and settings

By default the CLI uses `~/.request-eagle/collections` and
`~/.request-eagle/preferences.json`. `--data-dir PATH` selects another data root.
`--collections-dir PATH` overrides only the collection location; otherwise
`REQUEST_EAGLE_COLLECTIONS_DIR` is honored, as in the desktop app. Explicit flags
win over the environment. Directory flags work before or after the subcommand.

```sh
request-eagle-cli --data-dir /tmp/eagle-example call '{"command":"collections.create"}'
request-eagle-cli call '{"command":"settings.get"}'
request-eagle-cli call '{"command":"settings.request","timeout_ms":10000,"follow_all_redirects":false}'
request-eagle-cli call '{"command":"settings.appearance","mode":"dark","interface_font_size":16}'
request-eagle-cli call '{"command":"settings.proxy","mode":"disabled"}'
```

Settings commands patch only supplied fields. Request settings cover HTTP
version, timeout, response size limit, certificate verification, and redirects.
Appearance settings cover mode, theme names, editor font, and interface font
size (12–24 px). Theme/font names use the desktop app's normal fallback behavior
when unavailable. Proxy settings use the same validation and OS credential store
as the desktop app. `settings.get` never returns proxy usernames, passwords, or
credential references. Supply `username` and `password` together through stdin
to replace credentials; both empty strings remove them. Changing the proxy host,
port, or protocol without replacement credentials disables authentication and
clears the previous reference. Keyring access may require unlocking the OS store.

**Quit the desktop app before editing its collections or settings with the CLI,
and reopen it to load the changes.** The current desktop app caches that data;
there is no live synchronization. CLI commands use file locks to serialize their
own edits. Reads and request execution do not write or lock collection files, so
they also work on read-only collections. Avoid reading during edits if you need
a consistent snapshot. External editors and the desktop app do not participate
in the edit locks. Unsaved drafts, open tabs, and previous desktop responses are
outside the CLI's scope. Keybinding and app-update automation are also outside
this interface.

## Execution and output

`requests.run` waits for one completed execution and returns status, repeated
headers, elapsed time, body bytes as `body_base64`, and script logs/test results.
There is no response polling or stored response state. Stop the CLI process to
cancel a run. HTTP error statuses are completed responses, so they still exit 0;
inspect `status` and script results for application-level success.

Execution uses saved request preferences, including proxy credentials and TLS
verification. `timeout_ms` on a run overrides the stored request deadline for
that invocation; zero disables it. The stored response size limit also applies.
Scripts have their own runtime limits. Read any saved scripts before opting in
with `trust_scripts: true`. Run-local `variables` override values from the
collection's `environment.toml`. Script environment changes last through the
pre-request and post-response phases of that invocation; they are not written
back to the environment file or shared with the desktop session.

Each command writes one JSON object to stdout:

```json
{"version":1,"ok":true,"result":{}}
{"version":1,"ok":false,"error":{"code":"operation_failed","message":"details"}}
```

Exit codes: 0 for success, 1 for an operation or execution failure, and 2 for
invalid CLI input (`invalid_input`). Help and version output are plain text.
Unknown command fields are rejected. Command input is limited to 8 MiB; paths
must be valid UTF-8. `call -` reads JSON from stdin and avoids placing secrets in
process arguments. Response bodies and raw header values use base64 to preserve
arbitrary bytes; nothing is silently truncated.

## Development checks

```sh
cargo build --locked -p request-eagle-cli
cargo test --locked -p request-eagle-cli
```

Integration tests use temporary data directories and a loopback HTTP server,
with display variables removed. They exercise collection editing, identity and
metadata preservation, execution, scripts, binary responses, settings, and
invalid-input handling. `scripts/check-linux-keyring.sh` additionally tests
headless/desktop credential interoperability against an isolated KeePassXC store.
