# Public API fixtures

This folder contains 23 requests in Request Eagle's TOML format, nested folders,
three collection environments, and two alternative environments. No API keys are
needed for the default requests.

| Collection | Requests | Examples |
| --- | ---: | --- |
| [HTTPBingo](https://httpbingo.org/) | 13 | Echo, authentication, JSON and form bodies, HTML, redirects, delay, and error responses |
| [JSONPlaceholder](https://jsonplaceholder.typicode.com/guide/) | 8 | Posts, comments, users, todos, and simulated create, update, and delete operations |
| [Open-Meteo](https://open-meteo.com/en/docs) | 2 | Current weather in Berlin and a three day forecast for Tokyo |

JSONPlaceholder simulates writes without changing its stored data. HTTPBingo's
authentication examples use public demo values. Weather responses change over time.
Use these public services for manual checks, not load testing.

## Open the sidebar demo

Run from the project root:

```sh
make demo
```

This launches **Request Eagle (Collections demo)** with `test/collections` as its
collection directory. It loads the files directly without copying them into your
personal collections. The sidebar supports filtering by name, method, or URL,
collapsing folders, selecting requests, and keyboard navigation. Arrow keys move
through the tree; Left and Right collapse and expand folders; Home and End jump
to the first and last visible rows.

To use another directory:

```sh
REQUEST_EAGLE_COLLECTIONS_DIR=/absolute/path/to/collections make run
```

Without that variable, the app loads `~/.request-eagle/collections`.
The app's request editor and Send action are not implemented yet. Use the runner
below to execute the saved requests.

## Execute requests

The runner needs Python 3.11 or newer and uses only its standard library.

```sh
# List the resolved URLs without sending requests.
python3 test/run.py --list

# Send every request once and check its expected status.
python3 test/run.py

# Run one collection.
python3 test/run.py test/collections/JSONPlaceholder

# Run one request and print its response body.
python3 test/run.py test/collections/HTTPBingo/Echo/02-json.toml --verbose

# Run the echo requests against an alternative public server.
python3 test/run.py test/collections/HTTPBingo/Echo --environment test/environments/httpbin.toml
```

The runner reads each collection's `environment.toml`, joins `base_url` with
`request.path`, and encodes query pairs in order, including duplicate keys.
It sends the saved headers and body bytes and follows redirects. The optional
environment file overrides matching collection values. These are runner
conventions; the app currently loads environments but has no interpolation engine.

Each request has a `[test]` table with its expected HTTP status. The app's parser
ignores this extra metadata. The runner also checks that JSON responses parse.
Expected 404 and 500 responses count as passes. Network errors, malformed JSON,
and unexpected statuses produce a nonzero exit code. External service outages can
cause failures, so this is a manual smoke check rather than an offline CI test.

To run HTTPBin locally, start the server documented by
[HTTPBin](https://httpbin.org/) with port 8080 mapped to its port 80, then use
`--environment test/environments/httpbin-local.toml` with the HTTPBingo collection.

## File format

Each direct child of `collections/` is a collection. Request files can be nested
in folders. The loader excludes the root `environment.toml` from the request tree.
Standalone environment files belong outside `collections/` so the loader does not
mistake them for requests.

Environment values must be strings. Headers and query parameters are arrays of
string pairs. Bodies are UTF-8 byte arrays because the current Rust model uses
`Option<Vec<u8>>`; each body has a readable comment above it. The supported methods
are GET, POST, PUT, and DELETE.

## Rendering benchmark

The sidebar uses a fixed-height virtual list and draws only visible rows. Tree
indexing happens when the collection is loaded. Filtering and rebuilding the
visible row list run on a background worker after interactions, not during draws.

Run the existing full-layout CPU draw benchmark with 100,000 generated requests:

```sh
REQUEST_EAGLE_BENCH_PAGE='Workspace 100000 requests' \
  cargo test -p workspace --release --locked pages_render_benchmark \
  -- --ignored --nocapture --test-threads=1
```

It reports mean, p95, p99, maximum draw times, and the number of samples exceeding
8.33 ms, the CPU frame budget for 120 fps. It excludes collection loading, GPU
presentation, and the native Root wrapper. It does not guarantee a displayed frame
rate on every machine.

Measured on an Apple M5 Pro in the release build, with 120 samples for each draw
and scrolling case at 1024x768, 1440x900, and 3440x1410:

| Requests | Highest p99 CPU draw | Slowest CPU draw | Samples above 8.33 ms |
| ---: | ---: | ---: | ---: |
| 100 | 0.51 ms | 0.54 ms | 0 / 720 |
| 100,000 | 0.52 ms | 0.53 ms | 0 / 720 |
