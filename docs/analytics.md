# Usage data

Release builds send anonymous usage data to PostHog, to learn which features
people use. People turn it off in Settings > General > Privacy > Share usage
data. Nothing is sent while it is off, or while `preferences.json` cannot be
read, since it may turn it off.

Events never contain requests, responses, URLs, headers, collection or
environment contents, or names. Each installation sends a random ID, kept in
`~/.request-eagle/analytics-id`, which nothing links to the person using it.
Events are sent without a person profile.

| Event | When | Properties |
| --- | --- | --- |
| `app_opened` | The window opens | `update_track` |
| `request_sent` | An HTTP request is sent, a gRPC method invoked or a WebSocket connected | `protocol` (`http`, `grpc` or `websocket`), `saved` |
| `collection_run_started` | The Collection Runner starts | `requests` |
| `flow_run_started` | A flow runs | `blocks` |
| `import_finished` | Collections or environments were imported, or failed to | `collections`, `environments`, `skipped_requests`, `failed` |
| `curl_imported` | A pasted cURL command opens as a request | |

Every event also carries `$app_version`, `$os` and `arch`, and `$lib` set to
`request-eagle`.

## Builds

The app sends events only when it was built with the PostHog project token in
`REQUEST_EAGLE_POSTHOG_TOKEN`. The Release workflow sets it, so builds from
source and development builds send nothing. Events go to PostHog Cloud EU.
