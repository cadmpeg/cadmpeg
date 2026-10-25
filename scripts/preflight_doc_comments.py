# SPDX-License-Identifier: Apache-2.0
"""A new item does not take over the doc comment of the item below it.

Inserting a `const` or `fn` between an existing `///` comment and the item
it documents moves that comment onto the new item and leaves the old item
undocumented. Rust accepts it and `missing_docs` stays quiet, because both
items now have a comment. The check warns when an added item line sits
directly under a doc block that, at the merge base, documented an item
with another name.
"""

from __future__ import annotations

import re

from preflight import is_test_path

NAME = "doc-comments"
ORDER = 86

ITEM = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+fn|async\s+fn|unsafe\s+fn|fn|const|"
                  r"static|struct|enum|type|trait|mod)\s+([A-Za-z_]\w*)")


def documented_name(lines: list[str], doc_index: int) -> str | None:
    """Name of the item that the doc block containing `doc_index` documents."""
    index = doc_index
    while index < len(lines) and (lines[index].strip().startswith(("///", "#["))
                                  or not lines[index].strip()):
        index += 1
    match = ITEM.match(lines[index]) if index < len(lines) else None
    return match.group(1) if match else None


def displaced(new: str, base: str, added: set[int]) -> list[tuple[int, str, str]]:
    found = []
    lines = new.splitlines()
    base_lines = base.splitlines()
    for number in sorted(added):
        match = ITEM.match(lines[number - 1]) if number <= len(lines) else None
        if not match:
            continue
        index = number - 2
        while index >= 0 and lines[index].strip().startswith("#["):
            index -= 1
        block = []
        while index >= 0 and lines[index].strip().startswith("///"):
            block.append(index + 1)
            index -= 1
        old = [line for line in block if line not in added]
        if not old:
            continue
        text = lines[min(old) - 1]
        owners = {documented_name(base_lines, position)
                  for position, line in enumerate(base_lines) if line == text}
        owners.discard(None)
        if owners and match.group(1) not in owners:
            found.append((number, match.group(1), ", ".join(sorted(owners))))
    return found


def run(context):
    details = []
    for path in context.changed_paths():
        if not path.endswith(".rs") or is_test_path(path):
            continue
        base = context.read_base(path)
        new = context.read(path)
        if base is None or new is None:
            continue
        added = {number for number, _ in context.added_lines(path)}
        for number, name, owner in displaced(new, base, added):
            details.append(f"{path}:{number}: `{name}` now carries the doc comment of `{owner}`")
    if not details:
        return context.passed("no added item took over an existing doc comment")
    return context.warned(f"{len(details)} doc comment(s) moved onto a new item", *details,
                          "fix: insert the new item above the doc comment, or after the old item")
