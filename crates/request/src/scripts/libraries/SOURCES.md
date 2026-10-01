# Libraries scripts can require

| Library | Version | Source | File SHA-256 |
| --- | --- | --- | --- |
| `crypto-js` | 4.2.0 | [https://registry.npmjs.org/crypto-js/-/crypto-js-4.2.0.tgz](https://registry.npmjs.org/crypto-js/-/crypto-js-4.2.0.tgz) `crypto-js.js` | `ee02257ffbaf0a9b481c7039b0f3bb20c360c9674fe4be8b38ae709b2ea59bbe` |
| `lodash` | 4.17.21 | [https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz](https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz) `lodash.min.js` | `a9705dfc47c0763380d851ab1801be6f76019f6b67e40e9b873f8b4a0603f7a9` |
| `moment` | 2.30.1 | [https://registry.npmjs.org/moment/-/moment-2.30.1.tgz](https://registry.npmjs.org/moment/-/moment-2.30.1.tgz) `min/moment.min.js` | `845c524969edd5b3af9aa6d8718d29fe92e8dbe25b955214a8e064a05a9a5027` |

Each file is the package's unmodified browser build, under the MIT license
in the `<name>.LICENSE.txt` beside it. Scripts load a library the first time
they require it.

Regenerate with `python3 ../vendor_libraries.py` from this directory. Python
and network access are only needed for this development operation.
