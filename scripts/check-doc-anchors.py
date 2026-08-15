#!/usr/bin/env python3
"""Validate specification references and field schema in the open-items documents.

An open-items document cites the specification by section number and paragraph
start, for example ``f3d.md`` §7.3 ``off_spl_sur``. This checker resolves every
such reference and fails when one does not name exactly one paragraph. It also
fails on a bare ``file.md:123`` line reference, which cannot be validated and
goes stale whenever the specification gains or loses a line.

Each ``### ID.`` item may use only the declared fields Question, Known, Need,
Conflict, and Note. Question, Known, and Need are required. A resolved item is
deleted; a Resolved field is a schema error.

Usage: scripts/check-doc-anchors.py [open-items.md ...]
With no arguments it checks every docs/formats/*-open-items.md file.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent / "docs" / "formats"

REFERENCE = re.compile(r"`([\w.-]+\.md)` §([\d.]+) (?:`([^`]+)`|\"([^\"]+)\")")
LINE_REFERENCE = re.compile(r"`?([\w.-]+\.md):(\d+)`?")
HEADING = re.compile(r"^#{2,6}\s+([\d.]+)\s+\S")
ITEM_HEADING = re.compile(r"^### ([A-Z]{1,3}-\d+[A-Z]?)\.")
FIELD = re.compile(r"^\*\*([A-Za-z][A-Za-z0-9 ./-]*)\.\*\*")
CONFIDENCE_TAG = re.compile(r"\bConfidence:")

ALLOWED_FIELDS = frozenset({"Question", "Known", "Need", "Conflict", "Note"})
REQUIRED_FIELDS = ("Question", "Known", "Need")


def normalize(line: str) -> str:
    """Strip indentation and a leading list marker so anchors match code blocks."""
    line = line.strip()
    return line[2:].strip() if line.startswith("- ") else line


def collapse_ws(text: str) -> str:
    """Collapse table-column padding so a citation need not copy the cell width."""
    return " ".join(text.split())


def phrase_key(text: str) -> str:
    """Keep code-block spacing; collapse only markdown table rows."""
    stripped = text.strip()
    if stripped.startswith("|"):
        return collapse_ws(stripped)
    return stripped


def section_bodies(lines: list[str]) -> dict[str, list[str]]:
    """Map each numbered section to the normalized lines it owns."""
    starts: list[tuple[int, str]] = []
    for index, line in enumerate(lines):
        match = HEADING.match(line)
        if match:
            starts.append((index, match.group(1).rstrip(".")))
    bodies: dict[str, list[str]] = {}
    for position, (index, number) in enumerate(starts):
        end = starts[position + 1][0] if position + 1 < len(starts) else len(lines)
        bodies[number] = [normalize(line) for line in lines[index:end]]
    return bodies


def count_matches(body: list[str], name: str | None, phrase: str | None) -> int:
    """Count paragraphs a reference selects: a named record or a phrase start."""
    if phrase is not None:
        needle = phrase_key(phrase)
        return sum(1 for line in body if phrase_key(line).startswith(needle))
    return sum(
        1
        for line in body
        if line.startswith(f"**`{name}`") or line.startswith(f"{name} :=")
    )


def check_schema(path: Path, lines: list[str]) -> list[str]:
    """Reject undeclared fields, missing required fields, and Confidence tags."""
    failures: list[str] = []
    item_id: str | None = None
    item_line = 0
    seen: set[str] = set()

    def close_item() -> None:
        if item_id is None:
            return
        missing = [name for name in REQUIRED_FIELDS if name not in seen]
        if missing:
            failures.append(
                f"{path.name}:{item_line}: {item_id} missing "
                + ", ".join(f"**{name}.**" for name in missing)
            )

    for number, text in enumerate(lines, 1):
        heading = ITEM_HEADING.match(text)
        if heading:
            close_item()
            item_id = heading.group(1)
            item_line = number
            seen = set()
            continue
        if text.startswith("## "):
            close_item()
            item_id = None
            continue
        field = FIELD.match(text)
        if field and item_id is not None:
            name = field.group(1)
            if name not in ALLOWED_FIELDS:
                failures.append(
                    f"{path.name}:{number}: {item_id} undeclared field **{name}.**; "
                    f"allowed: {', '.join(sorted(ALLOWED_FIELDS))}"
                )
            elif name in seen:
                failures.append(
                    f"{path.name}:{number}: {item_id} repeats **{name}.**"
                )
            seen.add(name)
        if CONFIDENCE_TAG.search(text):
            failures.append(
                f"{path.name}:{number}: undocumented Confidence tag; "
                "fold the substance into **Note.**"
            )
    close_item()
    return failures


def check(path: Path) -> list[str]:
    failures: list[str] = []
    specs: dict[str, dict[str, list[str]]] = {}
    lines = path.read_text().split("\n")

    for number, text in enumerate(lines, 1):
        for spec, line_number in LINE_REFERENCE.findall(text):
            failures.append(
                f"{path.name}:{number}: line reference `{spec}:{line_number}`; "
                f"cite the section number and the paragraph start instead"
            )

        for spec, section, name, phrase in REFERENCE.findall(text):
            target = DOCS / spec
            if not target.exists():
                failures.append(f"{path.name}:{number}: no file {spec}")
                continue
            if spec not in specs:
                specs[spec] = section_bodies(target.read_text().split("\n"))
            body = specs[spec].get(section)
            if body is None:
                failures.append(f"{path.name}:{number}: {spec} has no section §{section}")
                continue
            anchor = name or phrase
            found = count_matches(body, name or None, phrase or None)
            if found != 1:
                failures.append(
                    f"{path.name}:{number}: {spec} §{section} {anchor!r} "
                    f"selects {found} paragraphs; expected 1"
                )

    failures.extend(check_schema(path, lines))
    return failures


def main(argv: list[str]) -> int:
    paths = [Path(a) for a in argv[1:]] or sorted(DOCS.glob("*-open-items.md"))
    if not paths:
        print("check-doc-anchors: no open-items documents found")
        return 0

    failures: list[str] = []
    for path in paths:
        failures.extend(check(path))

    for failure in failures:
        print(failure)
    if failures:
        print(f"check-doc-anchors: {len(failures)} error(s)")
        return 1
    print(
        f"check-doc-anchors: {len(paths)} document(s) checked; "
        "references resolve and field schema holds"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
