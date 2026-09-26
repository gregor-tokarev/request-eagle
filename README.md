# Request variable walkthrough

Native Request Eagle walkthrough recorded from source commit 74a3d9eb130efe0a8d64c3aa95ee9ab924531094 for PR #40.

The 75-second video demonstrates environment and secret management, preserving an existing masked secret, URL/query/header/JSON-body autocomplete, unknown-variable errors, successful localhost HTTP execution, and saving the template. All values are synthetic and the Linux keyring is isolated.

Automated verification: 238 workspace tests passed; 6 benchmarks ignored. Native manager saves also preserved comments and quoting in the environment file. Review regression coverage includes external environment edits, pending-save dismissal and dispatch, generated header previews, collection path reuse, deferred source failures, stale error recovery, and reserved completion namespaces.
