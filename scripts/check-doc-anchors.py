#!/usr/bin/env python3
"""Validate specification references in the open-items documents.

An open-items document cites the specification by section number and paragraph
start, for example ``f3d.md`` §7.3 ``off_spl_sur``. This checker resolves every
such reference and fails when one does not name exactly one paragraph. It also
fails on a bare ``file.md:123`` line reference, which cannot be validated and
goes stale whenever the specification gains or loses a line.

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


def normalize(line: str) -> str:
    """Strip indentation and a leading list marker so anchors match code blocks."""
    line = line.strip()
    return line[2:].strip() if line.startswith("- ") else line


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
        return sum(1 for line in body if line.startswith(phrase))
    return sum(
        1
        for line in body
        if line.startswith(f"**`{name}`") or line.startswith(f"{name} :=")
    )


def check(path: Path) -> list[str]:
    failures: list[str] = []
    specs: dict[str, dict[str, list[str]]] = {}

    for number, text in enumerate(path.read_text().split("\n"), 1):
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
        print(f"check-doc-anchors: {len(failures)} bad references")
        return 1
    print(f"check-doc-anchors: {len(paths)} document(s) checked; all references resolve")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
