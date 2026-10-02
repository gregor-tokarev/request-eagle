#!/usr/bin/env python3
"""Release notes from the pull requests merged between version tags.

    release-notes.py json TAG               Every release up to TAG, newest first,
                                            for the app to embed.
    release-notes.py markdown TAG [SINCE]   The changes after SINCE up to TAG, for
                                            the GitHub release. SINCE defaults to
                                            the release before TAG.

Pull requests are found on main's first-parent history, as merge commits
("Merge pull request #N from …", titled by their message) or squashed commits
("Title (#N)").
"""

import json
import os
import re
import subprocess
import sys

TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
MERGE = re.compile(r"^Merge pull request #(\d+) from \S+$")
SQUASH = re.compile(r"^(.*\S)\s+\(#(\d+)\)$")


def git(*arguments):
    return subprocess.run(
        ["git", *arguments], check=True, capture_output=True, text=True
    ).stdout


def tags(until):
    """Version tags in the history of `until`, newest first."""
    found = [tag for tag in git("tag", "--merged", until).split() if TAG.match(tag)]

    return sorted(found, key=lambda tag: tuple(map(int, TAG.match(tag).groups())), reverse=True)


def changes(since, until):
    """Pull requests merged after `since` up to `until`, in merge order."""
    span = f"{since}..{until}" if since else until
    log = git("log", "--first-parent", "--reverse", "--format=%s%x1f%b%x1e", span)
    found = []

    for entry in log.split("\x1e"):
        subject, _, body = entry.strip("\n").partition("\x1f")

        if merge := MERGE.match(subject):
            title = next((line.strip() for line in body.splitlines() if line.strip()), subject)
            found.append({"pull_request": int(merge[1]), "title": title})
        elif squash := SQUASH.match(subject):
            found.append({"pull_request": int(squash[2]), "title": squash[1]})

    return found


def releases(until):
    found = tags(until)

    return [
        {
            "version": tag[1:],
            "date": git("log", "-1", "--format=%cs", tag).strip(),
            "changes": changes(found[index + 1] if index + 1 < len(found) else None, tag),
        }
        for index, tag in enumerate(found)
    ]


def markdown(tag, since=None):
    if since is None:
        earlier = tags(tag)[1:]
        since = earlier[0] if earlier else None

    repository = os.environ.get("GITHUB_REPOSITORY", "gregor-tokarev/request-eagle")
    merged = changes(since, tag)
    lines = ["## What's new", ""]
    lines += [f"- {change['title']} (#{change['pull_request']})" for change in merged]

    if not merged:
        lines.append("No pull requests were merged in this release.")

    if since:
        lines += ["", f"**Full changelog**: https://github.com/{repository}/compare/{since}...{tag}"]

    return "\n".join(lines)


def main(arguments):
    if len(arguments) == 2 and arguments[0] == "json":
        print(json.dumps(releases(arguments[1]), indent=2))
    elif len(arguments) in (2, 3) and arguments[0] == "markdown":
        print(markdown(*arguments[1:]))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
