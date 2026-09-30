"""Regenerate the pinned embedded compiler assets. Development only; requires Python 3."""

from pathlib import Path
import gzip
import hashlib
import io
import json
import re
import tarfile
import urllib.request


VERSION = "5.9.3"
URL = f"https://registry.npmjs.org/typescript/-/typescript-{VERSION}.tgz"
ARCHIVE_SHA256 = "10e108c9cf7d5f2879053dff18515fb405abf2ccef63eaaf017d9c571687a1d3"
assets = Path(__file__).parent / "assets"
archive_bytes = urllib.request.urlopen(URL).read()
assert hashlib.sha256(archive_bytes).hexdigest() == ARCHIVE_SHA256
archive = tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:gz")
assets.mkdir(exist_ok=True)


def read(name):
    return archive.extractfile("package/" + name).read()


libraries = {}


def include(name):
    if name in libraries:
        return
    text = read("lib/" + name).decode()
    libraries[name] = text
    for dependency in re.findall(r'/// <reference lib="([^"]+)"', text):
        include("lib." + dependency + ".d.ts")


include("lib.es2023.d.ts")
compiler = read("lib/typescript.js")
files = {
    f"typescript-{VERSION}.js.gz": gzip.compress(compiler, mtime=0),
    "lib.es2023.json.gz": gzip.compress(json.dumps(libraries, sort_keys=True, separators=(",", ":")).encode(), mtime=0),
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

The compiler is unmodified. The library bundle contains lib.es2023.d.ts and its
{len(libraries) - 1} transitive library references, with their original license headers.
DOM and Node declarations are deliberately absent because scripts expose neither.

Archive SHA-256: `{ARCHIVE_SHA256}`
Uncompressed typescript.js SHA-256: `{hashlib.sha256(compiler).hexdigest()}`

Bundled file SHA-256 checksums:
{hashes}

Regenerate with `python3 ../vendor_typescript.py` from this directory. Python and
network access are only needed for this development operation. Application
startup uses embedded gzip data and requires no Node, installed TypeScript,
external language server, filesystem access, or network access.
""")
