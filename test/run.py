#!/usr/bin/env python3
"""Send the saved TOML requests with Python 3.11+ and no dependencies."""

import argparse
import json
from pathlib import Path
import sys
import tomllib
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode, urljoin
from urllib.request import Request, urlopen


def load(path):
    with path.open("rb") as source:
        return tomllib.load(source)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", nargs="?", type=Path, default=Path(__file__).parent / "collections")
    parser.add_argument("--environment", type=Path, help="Override the collection's base_url")
    parser.add_argument("--list", action="store_true", help="List requests without sending them")
    parser.add_argument("--verbose", action="store_true", help="Print response bodies")
    args = parser.parse_args()

    files = [args.path] if args.path.is_file() else sorted(args.path.rglob("*.toml"))
    files = [path for path in files if path.name != "environment.toml"]
    if not files:
        parser.error(f"No request files found in {args.path}")

    override = load(args.environment) if args.environment else {}
    failures = 0

    for path in files:
        try:
            fixture = load(path)
            request = fixture["request"]
            environment_path = next(
                folder / "environment.toml"
                for folder in path.resolve().parents
                if (folder / "environment.toml").is_file()
            )
            environment = load(environment_path) | override
            url = urljoin(environment["base_url"].rstrip("/") + "/", request["path"])
            if request.get("query"):
                url += ("&" if "?" in url else "?") + urlencode([tuple(pair) for pair in request["query"]])

            label = f"{request['method']:6} {url}"
            if args.list:
                print(label)
                continue

            headers = {"User-Agent": "RequestEagle-Fixtures/1.0", **dict(request.get("headers", []))}
            outgoing = Request(
                url,
                data=bytes(request["body"]) if "body" in request else None,
                headers=headers,
                method=request["method"],
            )
            try:
                response = urlopen(outgoing, timeout=20)
            except HTTPError as error:
                response = error

            with response:
                body = response.read()
                expected = fixture["test"]["status"]
                passed = response.status == expected
                if passed and body and "application/json" in response.headers.get("Content-Type", ""):
                    json.loads(body)

                failures += not passed
                print(f"{'PASS' if passed else 'FAIL'} {response.status} {label} (expected {expected})", flush=True)
                if args.verbose and body:
                    print(body.decode("utf-8", errors="replace"))

        except (OSError, URLError, ValueError, KeyError, StopIteration) as error:
            failures += 1
            print(f"FAIL {path}: {error}", file=sys.stderr, flush=True)

    if not args.list:
        print(f"\n{len(files) - failures}/{len(files)} requests passed.")

    return int(failures > 0)


if __name__ == "__main__":
    sys.exit(main())
