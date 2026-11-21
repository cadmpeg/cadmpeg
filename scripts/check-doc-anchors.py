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

Every open-item ID a format document cites, such as ``DR-904``, names an item
heading of the open-items document it cites. A citation resolves against the
open-items document named right after the ID (``DR-904 in f3d-open-items.md``),
else the last one the line names before the ID, else the document's own
open-items document (``f3d.md`` and ``f3d-coverage.md`` cite
``f3d-open-items.md``), else the only open-items document the file names. Known
non-item prefixes (such as ``UTF`` and ``FTC``) and ``former SE-08`` are not
citations.

Usage: scripts/check-doc-anchors.py [doc.md ...]
With no arguments it checks every docs/formats/*-open-items.md file for
references and schema, and every docs/formats/*.md file for open-item citations.
Arguments name the documents to check; the open-items ones get every check.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

from preflight_open_items import items

DOCS = Path(__file__).resolve().parent.parent / "docs" / "formats"

REFERENCE = re.compile(r"`([\w.-]+\.md)` §([\d.]+) (?:`([^`]+)`|\"([^\"]+)\")")
LINE_REFERENCE = re.compile(r"`?([\w.-]+\.md):(\d+)`?")
HEADING = re.compile(r"^#{2,6}\s+([\d.]+)\s+\S")
ITEM_HEADING = re.compile(r"^### ([A-Z]{1,3}-\d+[A-Z]?)\.")
FIELD = re.compile(r"^\*\*([A-Za-z][A-Za-z0-9 ./-]*)\.\*\*")
CONFIDENCE_TAG = re.compile(r"\bConfidence:")
OPEN_ITEMS_NAME = re.compile(r"[\w.-]+-open-items\.md")
CITATION = re.compile(r"(?<!former )(?<![A-Za-z0-9-])[A-Z]{2,4}-\d+[A-Z]?(?![A-Za-z0-9-])(?!\.\d)")
NAMED_AFTER = re.compile(r"\s+in\s+\[?`?([\w.-]+-open-items\.md)")
FORMAT_BEFORE = re.compile(r"\b([A-Za-z0-9_]+)\s+$")
NON_ITEM_PREFIXES = frozenset({
    "CRC",
    "CTC",
    "FTC",
    "ID",
    "IEEE",
    "ISO",
    "SHA",
    "STC",
    "STD",
    "UTF",
    "XOR",
})

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
    lines = path.read_text(encoding="utf-8").split("\n")

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
                specs[spec] = section_bodies(target.read_text(encoding="utf-8").split("\n"))
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


def own_open_items(name: str, headings: dict[str, set[str]]) -> str | None:
    """The open-items document whose stem starts this file's name."""
    owners = [
        target
        for target in headings
        if name == target.removesuffix("-open-items.md") + ".md"
        or name.startswith(target.removesuffix("open-items.md"))
    ]
    return max(owners, key=len, default=None)


def check_citations(documents: list[Path]) -> list[str]:
    """Fail on an open-item ID that names no item heading of its document."""
    headings = {
        path.name: {identifier for identifier, _, _ in items(path.read_text(encoding="utf-8"))}
        for path in DOCS.glob("*-open-items.md")
    }
    failures: list[str] = []
    for path in documents:
        text = path.read_text(encoding="utf-8")
        default = own_open_items(path.name, headings)
        if default is None:
            named = set(OPEN_ITEMS_NAME.findall(text)) & headings.keys()
            default = named.pop() if len(named) == 1 else None
        for number, line in enumerate(text.split("\n"), 1):
            mentions = [(match.start(), match.group()) for match in OPEN_ITEMS_NAME.finditer(line)]
            for citation in CITATION.finditer(line):
                after = NAMED_AFTER.match(line, citation.end())
                before = [name for start, name in mentions if start < citation.start()]
                format_match = FORMAT_BEFORE.search(line[: citation.start()])
                format_target = (
                    f"{format_match.group(1).lower()}-open-items.md"
                    if format_match
                    else None
                )
                if after:
                    target = after.group(1)
                elif format_target in headings:
                    target = format_target
                else:
                    target = (before[-1:] or [default])[0]
                identifier = citation.group()
                if target not in headings or identifier.split("-")[0] in NON_ITEM_PREFIXES:
                    continue
                if identifier not in headings[target]:
                    failures.append(
                        f"{path.name}:{number}: {identifier} names no item heading "
                        f"in {target}"
                    )
    return failures


def main(argv: list[str]) -> int:
    documents = [Path(a) for a in argv[1:]] or sorted(DOCS.glob("*.md"))
    if not documents:
        print("check-doc-anchors: no format documents found")
        return 0
    paths = [path for path in documents if path.name.endswith("-open-items.md")]

    failures: list[str] = []
    for path in paths:
        failures.extend(check(path))
    failures.extend(check_citations(documents))

    for failure in failures:
        print(failure)
    if failures:
        print(f"check-doc-anchors: {len(failures)} error(s)")
        return 1
    print(
        f"check-doc-anchors: {len(paths)} document(s) checked; "
        "references resolve and field schema holds; "
        f"open-item citations in {len(documents)} document(s) name headings"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
