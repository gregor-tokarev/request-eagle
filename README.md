# Request variable walkthrough

Native Request Eagle walkthrough recorded from source commit e61c577c215c47f07113debaef956f96b3275d53 for PR #40.

The 59-second video demonstrates environment and secret management, preserving an existing masked secret, URL/query/header/JSON-body autocomplete, unknown-variable errors, successful localhost HTTP execution, and saving the template. All values are synthetic and the Linux keyring is isolated. The recorded HTTP response was checked for the resolved Authorization header, custom header, and body values.

Automated verification: 238 workspace tests passed; 6 benchmarks ignored. Native keyring tests verify merges with external additions/updates, removal, and unavailable-provider behavior. Review regression coverage also includes invalidation after failed saves, environment comments and external edits, pending-save dismissal and dispatch, generated header previews, collection path reuse, deferred source failures, stale error recovery, and reserved completion namespaces.

Follow-up validation on source commit 76956d2cd5256b1a9bed47d80d9c28a80fbc851c: 240 workspace tests passed. The additional native-edge-cases.png screenshot shows a successful request after editing a shared symlinked environment; its unresolved URL fragment remains in the draft and is excluded from HTTP. The symlink and target were checked on disk.
