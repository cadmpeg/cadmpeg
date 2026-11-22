#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check docs/decode-resource-caps.md against production ``const MAX_*`` items.

Each production constant has exactly one table row, and each row names a
production constant. The document stores no counts: on success the checker
prints the overall, per-kind, and per-crate counts, so pull requests that
add constants to different crates edit different lines. Test source is
excluded the same way as ``check-source-policy.py``: path filters (``tests/`` trees,
``tests.rs``, and other test files) plus ``production_source``, which masks
``#[cfg(test)]`` modules and ``#[cfg(test)]`` / ``#[test]`` items.

Exit 0 prints ``resource-caps census: ok`` and the counts. Any mismatch prints
the findings and exits 1.
"""

from __future__ import annotations

import importlib.util
import re
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC_REL = Path("docs") / "decode-resource-caps.md"
SOURCE_SCRIPT = Path(__file__).with_name("check-source-policy.py")
SPEC = importlib.util.spec_from_file_location(
    "resource_caps_source_policy", SOURCE_SCRIPT
)
if SPEC is None or SPEC.loader is None:
    raise SystemExit(f"cannot load {SOURCE_SCRIPT}")
SOURCE_POLICY = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SOURCE_POLICY
SPEC.loader.exec_module(SOURCE_POLICY)

CONST_MAX = re.compile(
    r"(?<![\w:])(?:pub(?:\s*\([^)]*\))?\s+)?const\s+(MAX_[A-Z0-9_]+)\b"
)
STORED_TOTAL = re.compile(r"Production `MAX_\*` constants: \d+\.")
TOTALS_HEADING = re.compile(r"^## Totals\s*$", re.MULTILINE)
KINDS = ("1", "2", "3", "4", "N")
SESSION_CEILING = "3"


@dataclass(frozen=True)
class Constant:
    crate: str
    file: str
    name: str
    line: int
    relative: str


@dataclass(frozen=True)
class TableRow:
    crate: str
    file: str
    name: str
    kind: str
    line: int


def unwrap(cell: str) -> str:
    cell = cell.strip()
    if cell.startswith("`") and cell.endswith("`") and len(cell) >= 2:
        return cell[1:-1]
    return cell


def production_constants(root: Path) -> list[Constant]:
    """Every production ``const MAX_*`` under ``crates/**/*.rs``."""
    found: list[Constant] = []
    for path in sorted(root.glob("crates/**/*.rs")):
        relative_path = path.relative_to(root)
        if not SOURCE_POLICY.is_production_rs(relative_path):
            continue
        code, _ = SOURCE_POLICY.production_source(
            path.read_text(encoding="utf-8")
        )
        relative = relative_path.as_posix()
        parts = relative_path.parts
        crate = parts[1]
        file = Path(*parts[2:]).as_posix()
        for match in CONST_MAX.finditer(code):
            found.append(
                Constant(
                    crate=crate,
                    file=file,
                    name=match.group(1),
                    line=code.count("\n", 0, match.start()) + 1,
                    relative=relative,
                )
            )
    return found


def section(text: str, start_heading: str, *end_headings: str) -> tuple[str, int]:
    start = text.find(start_heading)
    if start < 0:
        return "", 0
    end = len(text)
    for heading in end_headings:
        at = text.find(heading, start + len(start_heading))
        if at >= 0:
            end = min(end, at)
    prefix_lines = text[:start].count("\n")
    return text[start:end], prefix_lines


def parse_census_rows(text: str) -> tuple[list[TableRow], list[str]]:
    body, offset = section(
        text, "## Production census", "### Work budgets", "## Routing"
    )
    if not body:
        return [], [f"{DOC_REL.as_posix()}: missing Production census section"]
    rows: list[TableRow] = []
    for index, line in enumerate(body.splitlines(), offset + 1):
        if not line.startswith("|"):
            continue
        cols = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cols) < 5:
            continue
        crate, file, name, _value, kind = (
            unwrap(cols[0]),
            unwrap(cols[1]),
            unwrap(cols[2]),
            cols[3],
            cols[4].strip(),
        )
        if not name.startswith("MAX_"):
            continue
        if kind not in KINDS:
            return rows, [
                f"{DOC_REL.as_posix()}:{index}: unknown kind {kind!r} for {name}"
            ]
        rows.append(TableRow(crate, file, name, kind, index))
    if not rows:
        return [], [f"{DOC_REL.as_posix()}: Production census table has no MAX_* rows"]
    return rows, []


def stored_totals(text: str) -> list[str]:
    """Hand-kept counts, which the checker computes and prints instead."""
    findings: list[str] = []
    for pattern in (TOTALS_HEADING, STORED_TOTAL):
        for match in pattern.finditer(text):
            line = text.count("\n", 0, match.start()) + 1
            findings.append(
                f"{DOC_REL.as_posix()}:{line}: stored total {match.group(0).strip()!r}; "
                f"delete it, python3 {Path('scripts') / Path(__file__).name} prints the counts"
            )
    return findings


def compare(constants: list[Constant], rows: list[TableRow]) -> list[str]:
    findings: list[str] = []
    src_by_key: dict[tuple[str, str, str], list[Constant]] = defaultdict(list)
    doc_by_key: dict[tuple[str, str, str], list[TableRow]] = defaultdict(list)
    for constant in constants:
        src_by_key[(constant.crate, constant.file, constant.name)].append(constant)
    for row in rows:
        doc_by_key[(row.crate, row.file, row.name)].append(row)

    for key in sorted(set(src_by_key) | set(doc_by_key)):
        source_hits = src_by_key.get(key, [])
        table_hits = doc_by_key.get(key, [])
        crate, file, name = key
        for constant in source_hits[len(table_hits) :]:
            findings.append(
                f"source constant missing from table: {constant.relative}:"
                f"{constant.line} {name}"
            )
        for row in table_hits[len(source_hits) :]:
            findings.append(
                f"table row with no source constant: {DOC_REL.as_posix()}:"
                f"{row.line} {crate} {file} {name}"
            )

    for row in rows:
        if row.kind == SESSION_CEILING:
            findings.append(
                f"{DOC_REL.as_posix()}:{row.line}: {row.name} is kind 3; only "
                "ResourceLimits fields are session ceilings"
            )
    return findings


def census(root: Path) -> tuple[list[Constant], list[TableRow], list[str]]:
    """The production constants, the table rows, and their mismatches."""
    constants = production_constants(root)
    doc = root / DOC_REL
    if not doc.is_file():
        return constants, [], [f"{DOC_REL.as_posix()}: missing"]
    text = doc.read_text(encoding="utf-8")
    rows, errors = parse_census_rows(text)
    return constants, rows, errors + stored_totals(text) + compare(constants, rows)


def check_tree(root: Path) -> list[str]:
    return census(root)[2]


def counts(constants: list[Constant], rows: list[TableRow]) -> list[str]:
    """The overall, per-kind, and per-crate counts of a census that matches."""
    kinds = Counter(row.kind for row in rows)
    crates = Counter(constant.crate for constant in constants)
    return [
        f"production MAX_* constants: {len(constants)}",
        "by kind: " + ", ".join(f"{kind} {kinds.get(kind, 0)}" for kind in KINDS),
        "by crate: " + ", ".join(f"{crate} {crates[crate]}" for crate in sorted(crates)),
    ]


@SOURCE_POLICY.analysis_session
def main() -> int:
    constants, rows, findings = census(ROOT)
    if findings:
        for line in findings:
            print(line, file=sys.stderr)
        print(
            f"resource-caps census: {len(findings)} mismatch(es)",
            file=sys.stderr,
        )
        return 1
    print("resource-caps census: ok")
    for line in counts(constants, rows):
        print(f"  {line}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
