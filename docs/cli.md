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
- `call -` reads JSON from stdin, which keeps secrets out of process arguments.
- Saved scripts run only with `trust_scripts: true`. Read them first, including
  the collection's scripts from `collections.get`, which run before the request's.
- Response bodies and raw header values are Base64.
- Data lives in `~/.request-eagle`. `--data-dir`, `--collections-dir`, and
  `REQUEST_EAGLE_COLLECTIONS_DIR` select another location.
