# Request variable walkthrough

Native Request Eagle walkthrough recorded from source commit 3c68acaf916899f68e71484b8f268814ebf1c250 for PR #40.

The 58-second video demonstrates environment and secret management, URL/query/header/JSON-body autocomplete, unknown-variable errors, successful localhost HTTP execution, and saving the template. All values are synthetic and the Linux keyring is isolated. The received request was checked for its resolved Authorization header, custom header, and body values; saved requests retain placeholders.

Validation: 247 workspace tests passed; 6 benchmarks ignored. Native keyring checks cover concurrent processes, external updates, rename/collision behavior, removal, and unavailable providers. Environment tests cover six concurrent writer processes, TOML formatting, alias refresh after target removal/restoration, and selected-editor state after saves and removals. CI passes on this commit.

Supplemental screenshots captured during review revisions show symlinked environment edits and excluded URL fragments (native-edge-cases.png), literal brace escapes (native-secret-escape.png), variable renaming (native-rename.png), and the cleared editor after removal (native-remove.png). The final removal workflow also successfully created another variable afterward.

Video SHA-256: b5013f4da7cafb7781b24754a9b6f883437cec9f89ab2c5ed9da3611ed28ce1c
