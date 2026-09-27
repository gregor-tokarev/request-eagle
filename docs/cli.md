# Request Eagle CLI

The optional `request-eagle-cli` binary gives agents JSON commands for the running
Request Eagle app. It operates on the same collections, tabs, request drafts,
responses, settings and credential store as the UI. There is no separate database.
The app must be running; the CLI does not launch a headless copy or require a display
server itself. Supported releases: Apple Silicon macOS, x86-64 Linux (glibc 2.35+),
and ARM64 Linux (glibc 2.39+).

## Install

Open **Settings → General → Request Eagle CLI → Install CLI**. This downloads the
separate binary for the installed app version, verifies its SHA-256 checksum, and
installs it at `~/.request-eagle/bin/request-eagle-cli`. It never requests root or
modifies your shell configuration. Add it to PATH:

```sh
export PATH="$HOME/.request-eagle/bin:$PATH"
```

The app bundle and normal `cargo build` do not include the CLI. To build it yourself:

```sh
cargo install --path crates/request-eagle-cli --locked
```

The General section shows the installed version and offers an update when the app
version changes. Development builds whose version has no CLI release can use the
source installation command. Remove the installed binary and its adjacent `.json`
receipt to uninstall; remove the PATH entry if you added it.

## Agent workflow

```sh
request-eagle-cli schema
request-eagle-cli instances
request-eagle-cli call '{"command":"collections.list"}'
request-eagle-cli call '{"command":"tabs.new"}'
request-eagle-cli call '{"command":"drafts.get","tab":2}'
request-eagle-cli call - <<'JSON'
{"command":"drafts.set","tab":2,"request":{"method":"POST","url":"https://example.com/api","headers":[["Accept","application/json"]],"body":"{\"hello\":\"world\"}"}}
JSON
request-eagle-cli call '{"command":"requests.send","tab":2}'
request-eagle-cli call '{"command":"responses.get","tab":2}'
```

Use returned tab IDs instead of assuming `2`. `schema` works without the app and
returns JSON Schema for every command and input. Unknown fields are errors to
catch misspelled arguments. `call -` reads one JSON command from stdin and avoids
putting secrets in command-line arguments. The CLI never prompts.

Every call prints one JSON object with `version`, `ok`, and either `result` or
`error: {code, message}`. Exit status is 0 for success, 1 for application/connection
failure, and 2 for malformed CLI input. `--help` and `--version` print plain text.
Arguments and socket paths must be valid UTF-8; invalid encoding produces a
structured error. Transport failures are ambiguous for mutations: inspect current state before
retrying. A request's HTTP error status is a completed response, not a CLI error;
inspect `status`, `failed`, and script test errors in `responses.get`.

`requests.send` starts execution and returns immediately. Poll `responses.get`
until `loading` is false. `requests.cancel` cancels that tab's execution. Script
execution needs `trust_scripts: true` after reading the scripts in `drafts.get`;
trust applies only to the current scripts in that tab, just like the UI prompt.
Scripts have the same sandbox, variables, limits, tests and console as UI sends.

Response bodies are lossless base64, with byte offsets and `next_offset`; repeat
`responses.get` until `next_offset` is null. The default chunk is 64 KiB, maximum
256 KiB. Headers preserve repeated entries and include lossless `value_base64`.
Responses also expose cookies, timings, sizes, script tests and console messages.
Agents can decode, format, search, select and save the returned data with their
usual tools. No truncation is silently accepted: oversized messages fail with
`result_too_large`; use a narrower collection query or smaller response chunk.

## Capabilities

| Area | Commands |
| --- | --- |
| Collections and search | `collections.list`, `collections.create` |
| Folders and request creation | `folders.create`, `requests.create` |
| Saved entries | `requests.get`, `requests.open`, `entries.rename`, `entries.move`, `entries.delete` |
| Tabs | `tabs.list`, `tabs.new`, `tabs.select`, `tabs.close` |
| Unsaved request drafts | `drafts.get`, `drafts.set`, `drafts.save` |
| Execution and responses | `requests.send`, `requests.cancel`, `responses.get` |
| Preferences | `settings.get`, `settings.request`, `settings.appearance`, `settings.proxy` |
| Appearance catalogs | `themes.list`, `fonts.list` |
| Shortcuts | `keybindings.list`, `keybindings.set`, `keybindings.reset` |
| Workspace navigation | `ui.show`, `ui.sidebar`, `app.status` |
| Application updates | `updates.check`, `updates.status`, `updates.download`, `updates.install` |

Paths returned by collection commands identify saved entries. Tab IDs identify
open pages. `drafts.set` replaces a complete editable request without saving;
`drafts.save` commits it. New drafts require `parent` and `name`; providing these
for an existing draft performs Save As. Opening a saved request that is already
open preserves its unsaved draft. Rename and move update the open tab's path and
variable scope without replacing the draft. Deletion requires `confirm: true`,
and closing a dirty tab requires `discard: true` or saving first. Deleting a saved
file leaves an open draft available to save elsewhere.

Proxy credential arguments are optional patches: omission preserves a credential
only while the endpoint stays the same; an empty string clears it. Changing the
host, port, or protocol clears stored credentials and disables authentication.
Authenticating a new endpoint requires explicit `username`, `password`, and
`authentication: true`. Unavailable keyring credentials can be retried or replaced at the same endpoint;
changing that endpoint requires explicit, nonempty replacement credentials. Reads never return proxy
passwords or usernames.
Writes use the same encrypted OS credential store as Settings → Proxy.
Collections, requests, responses and script output may contain secrets; avoid
logging their contents when running agents.

## Instances and local protocol

Each app instance creates `~/.request-eagle/automation/<pid>.sock` in a private
0700 directory, with a 0600 Unix socket. Only processes running as the same user
can access it. There is no network listener. The endpoint exists while the app is
running regardless of whether the optional client is installed.

`instances` lists live applications and their window titles. If multiple apps are
running, `call` requires `--socket PATH` before `call`; it never silently chooses
one. Slow or incompatible listeners remain listed with an error and still count
toward ambiguity. `--timeout-ms N` controls client I/O deadlines (default 30 seconds).
`REQUEST_EAGLE_AUTOMATION_DIR` overrides discovery for isolated development runs;
set it for both app and CLI. Label development windows with
`REQUEST_EAGLE_WINDOW_TITLE='Request Eagle (CLI development)'`.

The wire format is one newline-delimited JSON call per connection:
`{"version":1,"command":{"command":"tabs.list"}}`. The response uses the CLI
output envelope. Frames are limited to 8 MiB and server connections time out
in 30 seconds. Version mismatches fail explicitly. Requests execute on the app's
UI thread; request network work and installer downloads remain asynchronous.

## Development checks

`cargo test --workspace` covers the command contract, CLI transport, shared app
state, and installer validation. To exercise the compiled CLI against an isolated
native app and loopback HTTP server on Linux:

```sh
cargo build -p request-eagle -p request-eagle-cli
xvfb-run -a ./scripts/check-cli.sh
```

This covers unsaved drafts, saving and moving requests, script trust and execution,
collection variables, lossless paginated binary responses, cookies, preference
synchronization, and light/dark appearance at 12, 16 and 24 px. The launcher gives
the test app its own home directory, collections, socket, and window title.
