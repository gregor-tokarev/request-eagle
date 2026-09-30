# Embedded TypeScript 5.9.3

Source: [https://registry.npmjs.org/typescript/-/typescript-5.9.3.tgz](https://registry.npmjs.org/typescript/-/typescript-5.9.3.tgz), the Microsoft TypeScript npm distribution.
Upstream: https://github.com/microsoft/TypeScript/tree/v5.9.3
License: Apache-2.0; see LICENSE.txt and ThirdPartyNoticeText.txt.

The compiler is unmodified. The library bundle contains lib.es2023.d.ts and its
60 transitive library references, with their original license headers.
DOM and Node declarations are deliberately absent because scripts expose neither.

Archive SHA-256: `10e108c9cf7d5f2879053dff18515fb405abf2ccef63eaaf017d9c571687a1d3`
Uncompressed typescript.js SHA-256: `3ae902c92cc44dace175c0e69e13a4b0899f6983c6121d76b9ab8dd5795e7675`

Bundled file SHA-256 checksums:
- `typescript-5.9.3.js.gz`: `971137df07a2fca71e2aebf058b49f8b6a0f39b6fbe0df18a993a84098d8f709`
- `lib.es2023.json.gz`: `a30ca8fd1359c72c3051448bee108f22b2ace61198bfddfabba5277f21ef2c3c`
- `LICENSE.txt`: `a7d00bfd54525bc694b6e32f64c7ebcf5e6b7ae3657be5cc12767bce74654a47`
- `ThirdPartyNoticeText.txt`: `1af3c68039c57e539422da82a4faada506ce6d0ea6f90e0b699d02dbcdb7a90c`

Regenerate with `python3 ../vendor_typescript.py` from this directory. Python and
network access are only needed for this development operation. Application
startup uses embedded gzip data and requires no Node, installed TypeScript,
external language server, filesystem access, or network access.
