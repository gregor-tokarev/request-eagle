# Chai assertion library

`chai.min.js` is Chai 4.5.0's browser bundle (MIT), compacted with Terser 5.44.0.
Compression and name mangling are disabled; all assertions remain available.
Only `expect` is passed to our bounded QuickJS sandbox. Node/npm is needed only
to regenerate this checked-in artifact, never to build or run Request Eagle.

Source: https://raw.githubusercontent.com/chaijs/chai/v4.5.0/chai.js

Source SHA-256: `bdc229d660afad0313fc10d6afb5a339956a18c4c6e819d3eb5d8b94f314c202`

Artifact SHA-256: `1b7968ac8df51a69fae58cc585fcfded01c03798215f7b099ef56d8f24042dfd`

Reproduce from this directory ([Terser's CLI](https://github.com/terser/terser/blob/v5.44.0/README.md#command-line-usage) leaves compression/mangling off unless
`--compress`/`--mangle` is supplied):

```sh
set -e
chai_source=$(mktemp)
curl --fail --location https://raw.githubusercontent.com/chaijs/chai/v4.5.0/chai.js -o "$chai_source"
printf 'bdc229d660afad0313fc10d6afb5a339956a18c4c6e819d3eb5d8b94f314c202  %s\n' "$chai_source" | sha256sum --check
npm exec --yes --package=terser@5.44.0 -- terser "$chai_source" \
  --format 'comments=false,preamble="/*! Chai 4.5.0 (MIT); see LICENSE.chai, LICENSE.dependencies and README.md. */"' \
  --output chai.min.js
sha256sum chai.min.js
rm "$chai_source"
```

When updating, refresh both checksums, `LICENSE.chai`, and `LICENSE.dependencies`
(dependency versions come from the upstream tag's package-lock.json), then run
`cargo test -p request`. See [the scripting guide](../../../../../docs/scripting.md)
for the supported API and Postman compatibility limits.
