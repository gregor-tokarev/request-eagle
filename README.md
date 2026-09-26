# Request variable walkthrough

Native Request Eagle walkthrough recorded from source commit d5ff4d8dce2418d2f7471005e85a19dc91236b02 for PR #40.

The 58-second video demonstrates environment and secret management, preserving an existing masked secret, URL/query/header/JSON-body autocomplete, unknown-variable errors, successful localhost HTTP execution, and saving the template. All values are synthetic and the Linux keyring is isolated. The recorded HTTP response was checked for the resolved Authorization header, custom header, and body values.

Automated verification: 243 workspace tests passed; 6 benchmarks ignored. Native keyring tests verify four concurrent process writes, merges with external additions/updates, removal, and unavailable-provider behavior.

Additional native screenshots: native-edge-cases.png shows symlinked environment edits and URL fragments excluded from execution (source 76956d2); native-secret-escape.png shows literal braces surviving JSON transmission. Native testing on the video source also verified that an unchanged editor preserves a token rotated by another process, and Reload secrets picks up an external update.

Video SHA-256: 4948237ddd6cb419b0ef1c897a409bb315ad1657222fb9451e0a361f1cc7d0aa

Follow-up validation on source bf2431d4aa22cdd08676e1943bfe28e1b061ff2a: 246 workspace tests passed. Six environment-writer processes preserved all edits; environment/keyring rename tests reject destination collisions and remove old names; shared-file cache tests refresh both scopes. Native UI checks renamed environment variables (native-rename.png) and sent a request using a renamed secret. The core walkthrough remains recorded from d5ff4d8.

Latest validation on 0a2c03a91913e227319ca4991f7b5ba93e2f991a: all 246 workspace tests and the native keyring harness pass. Rename-only saves now move the current stored value, preserving external updates; the rendered manager test also verifies the editor refresh and a subsequent unchanged Save.
