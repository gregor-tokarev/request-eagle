# Request variable walkthrough

Native Request Eagle walkthrough recorded from source commit c8d762f17a5bae9a9180ffc556b44871e5300cdd for PR #40.

The 74-second video demonstrates environment and secret management, preserving an existing masked secret, URL/query/header/JSON-body autocomplete, unknown-variable errors, successful localhost HTTP execution, and saving the template. All values are synthetic and the Linux keyring is isolated.

Automated verification: 236 workspace tests passed; 6 benchmarks ignored. Native manager saves also preserved comments and quoting in the environment file. Review regression coverage includes external environment edits, pending-save dismissal and dispatch, generated Host and Authorization previews, collection path reuse, deferred environment failures, and unavailable keyring references.
