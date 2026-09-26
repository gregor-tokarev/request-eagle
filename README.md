# Request variable walkthrough

Native Request Eagle walkthrough recorded from source commit d7e0945791988366e47d0f8dcbc4a49774f0517f for PR #40.

The 72-second video demonstrates environment and secret management, preserving an existing masked secret, URL/query/header/JSON-body autocomplete, unknown-variable errors, successful localhost HTTP execution, and saving the template. All values are synthetic and the Linux keyring is isolated.

Automated verification: 232 workspace tests passed; 6 benchmarks ignored. Review regression coverage includes external environment edits, pending-save dismissal and dispatch, generated Host previews, collection path reuse, and unavailable keyring references.
