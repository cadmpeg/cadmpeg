#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Write the GitHub release notes for a release tag as Markdown on stdout.

The notes have these sections, each only when it has content:

* Compatibility: an IR_VERSION change, curated ``### Breaking`` lines from
  changelog fragments added by the release, and every line of the
  ``## Breaking change`` section of a pull-request description (collapsed
  below the curated lines when fragments supply ``### Breaking`` notes).
* Support levels: changed headline levels in ``dialect-support.toml``.
* Changes: one collapsed list per area, from the commit subject scope.
* Contributors: the GitHub logins of the change authors, without bots.

The changes are the first-parent commits after the previous release tag.
The fragments are the ``changelog.d/*.md`` files that the release adds
(``changelog.d/README.md`` states their form).
Pull-request data comes from the GitHub GraphQL API through ``gh``, so ``gh``
must be authenticated. A commit whose merged pull request GitHub does not
associate with it (a rewritten history) takes the pull request its subject
names.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import PurePosixPath

IR_FILE = "crates/cadmpeg-ir/src/document.rs"
IR_VERSION = re.compile(r'pub const IR_VERSION: &str = "(\d+)";')
SUPPORT_FILE = "crates/cadmpeg-registry/docs/dialect-support.toml"
DEPENDENCY_BOT = "dependabot[bot]"
SCOPE = re.compile(r"^([A-Za-z0-9][\w+-]*(?:, ?[A-Za-z0-9][\w+-]*)*): +(.+)$")
PR_SUFFIX = re.compile(r"\s*\(#(\d+)\)$")
BREAKING_HEADING = re.compile(r"^#{2,3}\s*Breaking changes?\s*$", re.IGNORECASE)
HEADING = re.compile(r"^#{1,6}\s")
COMMENT = re.compile(r"<!--.*?-->", re.DOTALL)
BREAKING = re.compile(r"^Breaking:\s*", re.IGNORECASE)
NO_NOTE = {"", "-", "none", "n/a", "na"}
FIRST_SENTENCE = re.compile(r"[.:;](?:\s|$)")
FRAGMENT_DIR = "changelog.d"
FRAGMENT_README = "README.md"
FRAGMENT_HEADING = re.compile(r"^#{1,6}\s+(.+?)\s*$")
FRAGMENT_BREAKING = re.compile(r"^breaking(?: changes?)?$", re.IGNORECASE)
GRAPHQL_BATCH = 40
SHORT_SHA = 10

CORE = "Core and CLI"
GENERAL = "General"
PROJECT = "Project"
TRAILING_AREAS = (CORE, GENERAL, PROJECT)
AREAS = {
    "f3d": "Fusion",
    "f3z": "Fusion",
    "fusion": "Fusion",
    "inventor": "Inventor",
    "sldprt": "SolidWorks",
    "freecad": "FreeCAD",
    "fcstd": "FreeCAD",
    "step": "STEP",
    "iges": "IGES",
    "creo": "Creo",
    "pvz": "Creo View",
    "edrawings": "eDrawings",
    "nx": "NX",
    "rhino": "Rhino",
    "catia": "CATIA",
    "solidedge": "Solid Edge",
    "ironcad": "IronCAD",
    "sat": "ACIS",
    "asm": "Shape Manager",
    "parasolid": "Parasolid",
    "ir": CORE,
    "core": CORE,
    "cli": CORE,
    "measure": CORE,
    "registry": CORE,
    "cfb": CORE,
    "container": CORE,
    "ci": PROJECT,
    "docs": PROJECT,
    "scripts": PROJECT,
    "preflight": PROJECT,
    "deps": PROJECT,
}

CHANGE_FRAGMENT = """
fragment change on Commit {
  author { user { __typename login } }
  associatedPullRequests(first: 5) {
    nodes { number body mergeCommit { oid } author { __typename login } }
  }
}
"""
NUMBERED_FRAGMENT = """
fragment numbered on IssueOrPullRequest {
  __typename
  ... on PullRequest { number body mergedAt author { __typename login } }
}
"""


@dataclass
class Change:
    sha: str
    subject: str
    number: int | None = None
    login: str | None = None
    body: str = ""

    def reference(self) -> str:
        return f"(#{self.number})" if self.number is not None else f"({self.sha[:SHORT_SHA]})"


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, stdout=subprocess.PIPE, text=True).stdout


def git_file(rev: str, path: str) -> str | None:
    result = subprocess.run(
        ["git", "show", f"{rev}:{path}"],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    return result.stdout if result.returncode == 0 else None


def previous_tag(tag: str) -> str:
    return git("describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", f"{tag}^").strip()


def parse_commits(log: str) -> tuple[list[Change], int]:
    """Changes and the dependency-update count from `%H%x1f%an%x1f%s%x1e` records."""
    changes = []
    dependency_updates = 0
    for record in log.split("\x1e"):
        record = record.strip("\n")
        if not record:
            continue
        sha, author, subject = record.split("\x1f")
        if author == DEPENDENCY_BOT:
            dependency_updates += 1
        elif not author.endswith("[bot]"):
            suffix = PR_SUFFIX.search(subject)
            changes.append(Change(sha, subject, number=int(suffix.group(1)) if suffix else None))
    return changes, dependency_updates


def login_of(actor: dict | None) -> str | None:
    if not actor or actor.get("__typename") == "Bot":
        return None
    login = actor.get("login")
    return None if not login or login.endswith("[bot]") else login


def apply_github(change: Change, commit: dict | None, numbered: dict | None = None) -> None:
    """Take the pull request merged as `change`, else the merged pull request
    its subject names, else the commit author."""
    pulls = [
        pull
        for pull in ((commit or {}).get("associatedPullRequests") or {}).get("nodes", [])
        if (pull.get("mergeCommit") or {}).get("oid") == change.sha
    ]
    if not pulls and numbered and numbered.get("__typename") == "PullRequest" and numbered.get("mergedAt"):
        pulls = [numbered]
    if pulls:
        change.number = pulls[0]["number"]
        change.body = pulls[0].get("body") or ""
        change.login = login_of(pulls[0].get("author"))
    elif commit is not None:
        change.login = login_of((commit.get("author") or {}).get("user"))


def graphql_data(output: str, stderr: str, optional: str) -> dict:
    """The response data, refusing every error but a missing `optional` alias."""
    try:
        response = json.loads(output)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"GitHub GraphQL failed: {stderr.strip() or error}") from error
    errors = [
        error
        for error in response.get("errors") or []
        if not (
            error.get("type") == "NOT_FOUND"
            and len(error.get("path") or []) == 2
            and str(error["path"][1]).startswith(optional)
        )
    ]
    if errors or not response.get("data"):
        raise RuntimeError(f"GitHub GraphQL errors: {errors or response}")
    return response["data"]


def fetch_github(repo: str, changes: list[Change]) -> None:
    owner, name = repo.split("/", 1)
    for start in range(0, len(changes), GRAPHQL_BATCH):
        batch = changes[start : start + GRAPHQL_BATCH]
        fields = "\n".join(
            f'c{index}: object(oid: "{change.sha}") {{ ...change }}'
            + (
                f"\np{index}: issueOrPullRequest(number: {change.number}) {{ ...numbered }}"
                if change.number is not None
                else ""
            )
            for index, change in enumerate(batch)
        )
        query = (
            "query($owner: String!, $name: String!) {\n"
            f"  repository(owner: $owner, name: $name) {{\n{fields}\n  }}\n}}\n{CHANGE_FRAGMENT}"
            + (NUMBERED_FRAGMENT if any(change.number is not None for change in batch) else "")
        )
        # gh exits non-zero on any GraphQL error, also on a subject number
        # that names no issue or pull request; graphql_data decides.
        result = subprocess.run(
            ["gh", "api", "graphql", "-f", f"query={query}", "-f", f"owner={owner}", "-f", f"name={name}"],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        repository = graphql_data(result.stdout, result.stderr, "p")["repository"]
        for index, change in enumerate(batch):
            apply_github(change, repository.get(f"c{index}"), repository.get(f"p{index}"))


def breaking_notes(body: str) -> list[str]:
    """Lines of the first `## Breaking change` section, without comments and
    placeholders. A line whose first sentence is a placeholder ("None. ...")
    states no break."""
    notes = []
    inside = False
    for line in COMMENT.sub("", body).splitlines():
        if not inside:
            inside = bool(BREAKING_HEADING.match(line.strip()))
            continue
        if HEADING.match(line):
            break
        text = line.strip()
        for bullet in ("- ", "* "):
            text = text.removeprefix(bullet)
        text = text.strip()
        if FIRST_SENTENCE.split(text, maxsplit=1)[0].strip().lower() not in NO_NOTE:
            notes.append(text)
    return notes


def fragment_breaking_notes(texts: list[str]) -> list[str]:
    """Bullet lines under ``### Breaking`` headings in changelog fragments, in order.

    An indented line after a bullet continues it. Bullets outside a
    ``### Breaking`` (or ``### Breaking change(s)``) section are ignored.
    """
    notes: list[str] = []
    for text in texts:
        inside = False
        continuing = False
        for line in COMMENT.sub("", text).splitlines():
            stripped = line.strip()
            match = FRAGMENT_HEADING.match(stripped)
            if match:
                inside = bool(FRAGMENT_BREAKING.match(match.group(1)))
                continuing = False
            elif inside and stripped.startswith(("- ", "* ")):
                bullet_text = stripped[2:].strip()
                if bullet_text:
                    notes.append(bullet_text)
                    continuing = True
                else:
                    continuing = False
            elif inside and stripped and continuing and line[:1].isspace():
                notes[-1] = f"{notes[-1]} {stripped}"
            else:
                continuing = False
    return notes


def added_fragments(previous: str, tag: str) -> list[str]:
    """The fragments present at `tag` and absent at `previous`, by file name."""
    names = git(
        "diff",
        "--name-only",
        "--no-renames",
        "--diff-filter=A",
        previous,
        tag,
        "--",
        f"{FRAGMENT_DIR}/*.md",
    ).split()
    return [
        git_file(tag, name) or ""
        for name in sorted(names)
        if PurePosixPath(name).name != FRAGMENT_README
    ]


def areas_of(subject: str) -> tuple[list[str], str]:
    """The areas a subject names by scope, and the subject without its scope."""
    match = SCOPE.match(subject)
    if match:
        labels = [AREAS.get(scope.strip().lower()) for scope in match.group(1).split(",")]
        if all(labels):
            return list(dict.fromkeys(labels)), match.group(2)
    return [GENERAL], subject


def change_line(change: Change, text: str) -> str:
    text = PR_SUFFIX.sub("", text)
    return f"{text[:1].upper()}{text[1:]} {change.reference()}"


def ir_change(old: str | None, new: str | None) -> str | None:
    before = IR_VERSION.search(old or "")
    after = IR_VERSION.search(new or "")
    if not before or not after or before.group(1) == after.group(1):
        return None
    return (
        f"CADIR documents, decode sidecars, and CLI reports use IR {after.group(1)}. "
        f"This release does not read IR {before.group(1)} files: decode the source files again."
    )


def support_levels(text: str | None) -> dict[str, int] | None:
    if text is None:
        return None
    formats = tomllib.loads(text).get("format", {})
    return {
        format_id: spec["level"]
        for format_id, spec in formats.items()
        if isinstance(spec, dict) and "level" in spec
    }


def support_changes(old: str | None, new: str | None) -> list[str]:
    before = support_levels(old)
    after = support_levels(new)
    if before is None or after is None:
        return []
    lines = []
    for format_id in sorted(before.keys() | after.keys()):
        was = before.get(format_id)
        now = after.get(format_id)
        if was == now:
            continue
        if was is None:
            lines.append(f"`{format_id}`: L{now} (new)")
        elif now is None:
            lines.append(f"`{format_id}`: no longer scored (was L{was})")
        else:
            lines.append(f"`{format_id}`: L{was} → L{now}")
    return lines


def group_changes(changes: list[Change], dependency_updates: int) -> list[tuple[str, list[str]]]:
    groups: dict[str, list[str]] = {}
    for change in changes:
        labels, text = areas_of(change.subject)
        for label in labels:
            groups.setdefault(label, []).append(change_line(change, text))
    if dependency_updates:
        noun = "update" if dependency_updates == 1 else "updates"
        groups.setdefault(PROJECT, []).append(f"{dependency_updates} dependency {noun}")
    formats = sorted(
        (label for label in groups if label not in TRAILING_AREAS),
        key=lambda label: (-len(groups[label]), label),
    )
    return [(label, groups[label]) for label in (*formats, *TRAILING_AREAS) if label in groups]


def bullets(lines: list[str]) -> str:
    return "\n".join(f"- {line}" for line in lines)


def render(
    tag: str,
    previous: str,
    repo: str,
    changes: list[Change],
    dependency_updates: int,
    ir_line: str | None,
    support: list[str],
    curated_breaking: list[str] | None = None,
) -> str:
    curated = list(curated_breaking or ())
    compatibility = ([ir_line] if ir_line else []) + curated
    by_pull_request = [
        f"{BREAKING.sub('', note)} {change.reference()}"
        for change in changes
        for note in breaking_notes(change.body)
    ]

    parts = []
    if curated and by_pull_request:
        parts.append(
            f"## Compatibility\n\n{bullets(compatibility)}\n\n"
            f"<details><summary><b>Breaking changes by pull request</b> ({len(by_pull_request)})</summary>"
            f"\n\n{bullets(by_pull_request)}\n\n</details>"
        )
    elif compatibility or by_pull_request:
        parts.append(f"## Compatibility\n\n{bullets(compatibility + by_pull_request)}")
    if support:
        parts.append(f"## Support levels\n\n{bullets(support)}")
    groups = group_changes(changes, dependency_updates)
    if groups:
        parts.append(
            "## Changes\n\n"
            + "\n\n".join(
                f"<details><summary><b>{label}</b> ({len(lines)})</summary>\n\n{bullets(lines)}\n\n</details>"
                for label, lines in groups
            )
        )
    contributors = sorted({change.login for change in changes if change.login}, key=str.lower)
    if contributors:
        parts.append("## Contributors\n\n" + " ".join(f"@{login}" for login in contributors))
    parts.append(
        f"[Install instructions](https://github.com/{repo}#install) · "
        f"[All changes since {previous}](https://github.com/{repo}/compare/{previous}...{tag})"
    )
    return "\n\n".join(parts) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("tag", help="release tag, e.g. v0.6.7")
    parser.add_argument("--previous", help="previous release tag (default: the nearest earlier v* tag)")
    parser.add_argument("--repo", required=True, help="GitHub repository as OWNER/NAME")
    args = parser.parse_args(argv)

    previous = args.previous or previous_tag(args.tag)
    log = git("log", "--first-parent", "--format=%H%x1f%an%x1f%s%x1e", f"{previous}..{args.tag}")
    changes, dependency_updates = parse_commits(log)
    fetch_github(args.repo, changes)
    sys.stdout.write(
        render(
            args.tag,
            previous,
            args.repo,
            changes,
            dependency_updates,
            ir_change(git_file(previous, IR_FILE), git_file(args.tag, IR_FILE)),
            support_changes(git_file(previous, SUPPORT_FILE), git_file(args.tag, SUPPORT_FILE)),
            fragment_breaking_notes(added_fragments(previous, args.tag)),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
