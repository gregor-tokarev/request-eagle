---
name: request-eagle-cli
description: Manage Request Eagle's saved collections, requests, and settings, and execute requests through its standalone CLI. Use for Request Eagle data operations, not desktop UI automation.
---

# Request Eagle CLI

Start with `request-eagle-cli --help`. Discover usage through subcommand help
and JSON commands through `request-eagle-cli schema`. Use the installed CLI's
help and schema as the command reference.

If the binary is missing, see [installation instructions](../../docs/cli.md#install).

- Work with saved data. Close the desktop app before edits and reopen it to
  reload changes; unsaved drafts and open tabs are outside this interface.
- Use paths and IDs returned by the CLI. Read a request before updating it:
  updates replace the complete request, so preserve fields outside the task.
- Pass JSON through stdin with `call -`, especially when it contains secrets.
- Inspect saved scripts before opting into their execution.
- Check the JSON result, HTTP status, and script test results. Exit code 0
  means execution completed; HTTP errors and failed assertions can still occur.
