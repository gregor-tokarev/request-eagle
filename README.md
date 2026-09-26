# Request variable walkthrough

Native Request Eagle walkthrough recorded from source commit 843b50777665e70a5854b82c4e5bbf4b934ae12c for PR #40.

The 58-second video demonstrates environment and secret management, URL/query/header/JSON-body autocomplete, unknown-variable errors, successful localhost HTTP execution, and saving the template. All values are synthetic and the Linux keyring is isolated. The received request was checked for resolved Authorization, custom header, and body values. The saved request was checked for unchanged placeholders.

Validation: 247 workspace tests passed; 6 benchmarks ignored. Native keyring checks cover concurrent writers, external updates, rename/collision behavior, removal, and unavailable providers. Environment tests cover concurrent writers, TOML formatting, shared-file removal/restoration, and editor lifecycle. CI and final code/security reviews completed successfully, with no unresolved review threads.

Supplemental screenshots captured during review revisions:
- native-edge-cases.png: shared symlink edits and excluded URL fragments.
- native-secret-escape.png: literal brace escapes sent in JSON.
- native-rename.png: a renamed environment entry.
- native-remove.png: cleared editor after removing the selected entry.
- native-pending-edit.png: both entries survived typing the next variable while a save was blocked by a file lock.
- native-conflict-error.png and native-conflict-response.png: secret-name conflicts keep editing available and leave the original secret usable for authenticated requests without a keyring retry.

Video SHA-256: 20d5132035c62e850786dfed76eca05e01050e04bebfd96dbed64107574c5c4a
