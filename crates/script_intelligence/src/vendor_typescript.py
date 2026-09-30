"""Regenerate the pinned embedded compiler assets. Development only; requires Python 3."""

from pathlib import Path
import gzip
import hashlib
import io
import json
import tarfile
import urllib.request


VERSION = "5.9.3"
URL = f"https://registry.npmjs.org/typescript/-/typescript-{VERSION}.tgz"
ARCHIVE_SHA256 = "10e108c9cf7d5f2879053dff18515fb405abf2ccef63eaaf017d9c571687a1d3"

# Declarations for the APIs request scripts use. The QuickJS script runtime has
# no Intl or decorators, so their libraries (and the Intl-only Date and Number
# overloads) are left out. Shared memory, WeakRef, Proxy, Reflect and
# well-known symbol declarations are also left out: scripts do not need them,
# and every bundled declaration stays in memory while scripts are edited.
LIBRARIES = [
    "es5",
    "es2015.collection",
    "es2015.core",
    "es2015.generator",
    "es2015.iterable",
    "es2015.promise",
    "es2015.symbol",
    "es2016.array.include",
    "es2017.arraybuffer",
    "es2017.date",
    "es2017.object",
    "es2017.string",
    "es2017.typedarrays",
    "es2018.asyncgenerator",
    "es2018.asynciterable",
    "es2018.promise",
    "es2018.regexp",
    "es2019.array",
    "es2019.object",
    "es2019.string",
    "es2019.symbol",
    "es2020.bigint",
    "es2020.promise",
    "es2020.string",
    "es2020.symbol.wellknown",
    "es2021.promise",
    "es2021.string",
    "es2022.array",
    "es2022.error",
    "es2022.object",
    "es2022.regexp",
    "es2022.string",
    "es2023.array",
    "es2023.collection",
]

assets = Path(__file__).parent / "assets"
archive_bytes = urllib.request.urlopen(URL).read()
assert hashlib.sha256(archive_bytes).hexdigest() == ARCHIVE_SHA256
archive = tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:gz")
assets.mkdir(exist_ok=True)


def read(name):
    return archive.extractfile("package/" + name).read()


libraries = {f"lib.{name}.d.ts": read(f"lib/lib.{name}.d.ts").decode() for name in LIBRARIES}
compiler = read("lib/typescript.js")
files = {
    f"typescript-{VERSION}.js.gz": gzip.compress(compiler, mtime=0),
    "libraries.json.gz": gzip.compress(json.dumps(libraries, sort_keys=True, separators=(",", ":")).encode(), mtime=0),
    "LICENSE.txt": read("LICENSE.txt"),
    "ThirdPartyNoticeText.txt": read("ThirdPartyNoticeText.txt"),
}

for name, content in files.items():
    (assets / name).write_bytes(content)

hashes = "\n".join(f"- `{name}`: `{hashlib.sha256(content).hexdigest()}`" for name, content in files.items())
(assets / "README.md").write_text(f"""# Embedded TypeScript {VERSION}

Source: [{URL}]({URL}), the Microsoft TypeScript npm distribution.
Upstream: https://github.com/microsoft/TypeScript/tree/v{VERSION}
License: Apache-2.0; see LICENSE.txt and ThirdPartyNoticeText.txt.

The compiler is unmodified. The library bundle contains lib.es5.d.ts and
{len(libraries) - 1} ES2015–ES2023 libraries, with their original license headers.
Libraries they reference that are not bundled are skipped by the compiler.
DOM, Node, Intl and decorator declarations are deliberately absent because
scripts expose none of them. See `LIBRARIES` in the vendoring script for the
other omissions.

Archive SHA-256: `{ARCHIVE_SHA256}`
Uncompressed typescript.js SHA-256: `{hashlib.sha256(compiler).hexdigest()}`

Bundled file SHA-256 checksums:
{hashes}

Regenerate with `python3 ../vendor_typescript.py` from this directory. Python and
network access are only needed for this development operation. The application
is built from these assets and requires no Node, installed TypeScript, external
language server, filesystem access, or network access.
""")
