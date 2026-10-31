# SPDX-License-Identifier: Apache-2.0
"""A new open item takes an ID that upstream has not used for another item.

Open-items documents number their items per prefix (`### DR-90.`,
`### GC-41.`). Two branches that each add the next number merge without a
text conflict and leave two items with one ID. The check compares the items
a branch adds with the upstream tip and with the rest of the file, and names
the next free number for each collision.

Run as a program, it checks every open-items document of the tree for a repeated
ID and reads no Git history. CI runs this form on the merge commit, so two
queued pull requests that take one ID fail the merge queue.
"""

from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path

NAME = "open-items"
ORDER = 30

ROOT = Path(__file__).resolve().parent.parent
OPEN_ITEMS_GLOB = "docs/formats/*-open-items.md"
OPEN_ITEMS = re.compile(r"^docs/formats/[^/]+-open-items\.md$")
ITEM = re.compile(r"^#{2,4} ([A-Z]{1,4})-(\d+)([A-Z]?)\.\s*(.*?)\s*$")
RENUMBER = ("fix: renumber the new item and every reference to it (spec text, loss "
            "messages, code and test comments: git grep -n '<old ID>')")


def items(text: str | None) -> list[tuple[str, str, int]]:
    """(ID, title, line) for each item heading."""
    found = []
    for number, line in enumerate((text or "").splitlines(), 1):
        match = ITEM.match(line)
        if match:
            prefix, digits, suffix, title = match.groups()
            found.append((f"{prefix}-{digits}{suffix}", title, number))
    return found


def next_free(prefix: str, *documents: list[tuple[str, str, int]]) -> str:
    numbers = [int(re.match(rf"{prefix}-(\d+)", identifier).group(1))
               for document in documents for identifier, _, _ in document
               if identifier.startswith(f"{prefix}-")]
    width = max((len(identifier.split("-")[1].rstrip("ABCDEFGHIJKLMNOPQRSTUVWXYZ"))
                 for document in documents for identifier, _, _ in document
                 if identifier.startswith(f"{prefix}-")), default=2)
    return f"{prefix}-{max(numbers, default=0) + 1:0{width}d}"


def duplicates(path: str, found: list[tuple[str, str, int]],
               *others: list[tuple[str, str, int]]) -> list[str]:
    """One finding for each heading whose ID repeats in the document."""
    counts = Counter(identifier for identifier, _, _ in found)
    return [f"{path}:{line}: {identifier} appears {counts[identifier]} times; "
            f"next free is {next_free(identifier.split('-')[0], found, *others)}"
            for identifier, _, line in found if counts[identifier] > 1]


def tree_findings(root: Path) -> list[str]:
    """Repeated IDs in every open-items document of a tree."""
    findings = []
    for file in sorted(root.glob(OPEN_ITEMS_GLOB)):
        findings += duplicates(file.relative_to(root).as_posix(),
                               items(file.read_text(encoding="utf-8")))
    return findings


def run(context):
    paths = [path for path in context.changed_paths() if OPEN_ITEMS.match(path)]
    if not paths:
        return context.passed("no open-items document changed")
    results = []
    for path in paths:
        new = items(context.read(path))
        base_ids = {identifier for identifier, _, _ in items(context.read_base(path))}
        upstream = items(context.read_upstream(path))
        upstream_titles = {identifier: title for identifier, title, _ in upstream}
        problems = duplicates(path, new, upstream)
        for identifier, title, line in new:
            if (identifier not in base_ids and identifier in upstream_titles
                    and upstream_titles[identifier] != title):
                free = next_free(identifier.split("-")[0], new, upstream)
                problems.append(f"{path}:{line}: {identifier} \"{title}\" collides with upstream's "
                                f"{identifier} \"{upstream_titles[identifier]}\"; next free is {free}")
        if problems:
            results.append(context.failed(f"open-item ID collision in {path}", *problems, RENUMBER))
    return results or context.passed(f"{len(paths)} open-items document(s): no ID collision")


def main() -> int:
    findings = tree_findings(ROOT)
    if findings:
        for line in (*findings, RENUMBER):
            print(line, file=sys.stderr)
        return 1
    print("open-item IDs: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
