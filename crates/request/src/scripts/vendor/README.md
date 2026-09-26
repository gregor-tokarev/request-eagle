# Chai assertion library

`chai.js` is the unmodified browser bundle from Chai 4.5.0 (MIT). It runs
inside the same bounded QuickJS context as request scripts. Only `expect` is
passed to our sandbox; no package loader or host I/O is exposed. There is no
Node.js or npm dependency at build time or runtime.

Source: https://raw.githubusercontent.com/chaijs/chai/v4.5.0/chai.js

SHA-256: `bdc229d660afad0313fc10d6afb5a339956a18c4c6e819d3eb5d8b94f314c202`

To update, download the browser bundle and LICENSE from a pinned upstream tag,
update this checksum, and run `cargo test -p request`. Dependency versions and
license notices in `LICENSE.dependencies` come from that tag's package-lock.json.

Chai provides the assertion semantics used by Postman's `pm.expect`, including
chained flags, nested properties, equality, array membership, keys, and numeric
comparisons. Postman-specific plugins (such as JSON Schema validation) are not
included. The editor suggests the common assertion methods documented in our
scripting guide.
