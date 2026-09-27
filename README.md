# Request editor variable demo

Source: `09aed661d6cddfa365a7ed5749fce6143d6fb1fa` (PR #40).

[Watch the 41-second native application recording](editor-variables.mp4).

The recording shows `{{` completion in URL, query keys/values, header keys/values, and JSON body; filtered environment/generated suggestions; Enter/Tab insertion; a successful POST to a local echo server; an unknown variable blocking Send; and saving the original placeholders.

The collection environment was populated programmatically before launching the app. `environment.toml` and `request.toml` contain the demonstration fixtures. No variable-management UI or secret support is included.

Additional native checks verified fresh generated values on resend and reading external environment edits on the next Send. The saved request still contains all placeholders. The workspace test suite passed (237 tests, 6 ignored manual benchmarks), including light/dark themes and 12/16/24 px interface sizes.

The video is a continuous X11 recording of the labeled native application, with no compositing or simulated UI. The app runs against `echo.py` on localhost:18765 with an isolated collection directory.

SHA-256: `834019056e66abadca905f8b502f5d74ac4b0566eac75ae8dca37513b7244867`

## Repeat Linux smoke test

A second clean native app session passed on source `09aed66`, with a newly seeded collection environment and a local HTTP echo server.

[Watch the repeat smoke recording](linux-smoke.mp4). The recording covers URL/query/header/JSON completion, Enter/Tab acceptance, successful HTTP requests, unknown references blocking dispatch, and saving placeholders.

Additional native checks passed: undoing completion, generated UUID freshness, reading an external environment edit on resend, closing and reopening the saved request, unchanged saved templates, invalid environment blocking HTTP, and recovery after repairing the file. See `linux-smoke-results.json` for the echoed requests.
