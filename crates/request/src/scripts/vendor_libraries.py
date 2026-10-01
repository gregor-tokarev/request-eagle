"""Regenerate the pinned libraries scripts can require. Development only; requires Python 3."""

from pathlib import Path
import hashlib
import io
import tarfile
import urllib.request


# Package name, version, npm archive SHA-256, and the browser build to embed.
PACKAGES = [
    ("crypto-js", "4.2.0", "2d288a658b3eae000d7fadfdfdf5fe2bec3952cb19212360db8c7686c2b6ce09", "crypto-js.js"),
    ("lodash", "4.17.21", "6a087ac9e5702a0c9d60fbcd48696012646ec8df1491dea472b150e79fcaf804", "lodash.min.js"),
    ("moment", "2.30.1", "52219a9fee5e1faade4c72536c173c54cedd5e2619272dd0c251a30aeafcde8c", "min/moment.min.js"),
]

libraries = Path(__file__).parent / "libraries"
libraries.mkdir(exist_ok=True)
rows = []

for name, version, archive_sha256, build in PACKAGES:
    url = f"https://registry.npmjs.org/{name}/-/{name}-{version}.tgz"
    archive_bytes = urllib.request.urlopen(url).read()
    assert hashlib.sha256(archive_bytes).hexdigest() == archive_sha256, name
    archive = tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:gz")

    source = archive.extractfile(f"package/{build}").read()
    license = archive.extractfile("package/LICENSE").read()
    (libraries / f"{name}.js").write_bytes(source)
    (libraries / f"{name}.LICENSE.txt").write_bytes(license)
    rows.append(
        f"| `{name}` | {version} | [{url}]({url}) `{build}` | `{hashlib.sha256(source).hexdigest()}` |"
    )

table = "\n".join(rows)
(libraries / "SOURCES.md").write_text(f"""# Libraries scripts can require

| Library | Version | Source | File SHA-256 |
| --- | --- | --- | --- |
{table}

Each file is the package's unmodified browser build, under the MIT license
in the `<name>.LICENSE.txt` beside it. Scripts load a library the first time
they require it.

Regenerate with `python3 ../vendor_libraries.py` from this directory. Python
and network access are only needed for this development operation.
""")
