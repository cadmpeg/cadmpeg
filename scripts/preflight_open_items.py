# SPDX-License-Identifier: Apache-2.0
"""A new open item takes an ID that upstream has not used for another item.

Open-items documents number their items per prefix (`### DR-90.`,
`### GC-41.`). Two branches that each add the next number merge without a
text conflict and leave two items with one ID. The check compares the items
a branch adds with the upstream tip and with the rest of the file, and names
the next free number for each collision.
"""

from __future__ import annotations

import re
from collections import Counter

NAME = "open-items"
ORDER = 30

OPEN_ITEMS = re.compile(r"^docs/formats/[^/]+-open-items\.md$")
ITEM = re.compile(r"^#{2,4} ([A-Z]{1,4})-(\d+)([A-Z]?)\.\s*(.*?)\s*$")


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
        problems = []
        counts = Counter(identifier for identifier, _, _ in new)
        for identifier, title, line in new:
            prefix = identifier.split("-")[0]
            free = next_free(prefix, new, upstream)
            if counts[identifier] > 1:
                problems.append(f"{path}:{line}: {identifier} appears {counts[identifier]} times; "
                                f"next free is {free}")
            elif (identifier not in base_ids and identifier in upstream_titles
                  and upstream_titles[identifier] != title):
                problems.append(f"{path}:{line}: {identifier} \"{title}\" collides with upstream's "
                                f"{identifier} \"{upstream_titles[identifier]}\"; next free is {free}")
        if problems:
            results.append(context.failed(
                f"open-item ID collision in {path}", *problems,
                "fix: renumber the new item and every reference to it (spec text, loss "
                "messages, code and test comments: git grep -n '<old ID>')",
            ))
    return results or context.passed(f"{len(paths)} open-items document(s): no ID collision")
