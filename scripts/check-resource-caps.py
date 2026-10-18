#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check docs/decode-resource-caps.md against production ``const MAX_*`` items.

The table, the overall count, the per-kind counts, and the per-crate bullets
must match each other and the production constants. Test source is excluded
the same way as ``check-source-policy.py``: path filters (``tests/`` trees,
``tests.rs``, and other test files) plus ``production_source``, which masks
``#[cfg(test)]`` modules and ``#[cfg(test)]`` / ``#[test]`` items.

Exit 0 prints ``resource-caps census: ok``. Any mismatch prints a diff and
exits 1.
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
OVERALL = re.compile(r"Production `MAX_\*` constants: (\d+)\.")
CRATE_COUNT = re.compile(r"`([^`]+)`\s+(\d+)")
KINDS = ("1", "2", "3", "4", "N")


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


@dataclass(frozen=True)
class Totals:
    overall: int | None
    overall_line: int | None
    kinds: dict[str, int]
    kind_lines: dict[str, int]
    kind_total: int | None
    kind_total_line: int | None
    crates: dict[str, int]
    crate_line: int | None


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
        text, "## Production census", "### Work budgets", "## Totals"
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


def parse_totals(text: str) -> tuple[Totals, list[str]]:
    body, offset = section(text, "## Totals", "## Routing")
    errors: list[str] = []
    if not body:
        errors.append(f"{DOC_REL.as_posix()}: missing Totals section")
        return Totals(None, None, {}, {}, None, None, {}, None), errors

    overall = None
    overall_line = None
    for index, line in enumerate(body.splitlines(), offset + 1):
        match = OVERALL.search(line)
        if match:
            overall = int(match.group(1))
            overall_line = index
            break
    if overall is None:
        errors.append(
            f"{DOC_REL.as_posix()}: missing 'Production MAX_* constants: N' total"
        )

    kinds: dict[str, int] = {}
    kind_lines: dict[str, int] = {}
    kind_total = None
    kind_total_line = None
    in_kind_table = False
    for index, line in enumerate(body.splitlines(), offset + 1):
        if line.startswith("| Kind"):
            in_kind_table = True
            continue
        if in_kind_table and not line.startswith("|"):
            break
        if not in_kind_table or not line.startswith("|"):
            continue
        cols = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cols) < 3 or set(cols[0]) <= {"-", ":"}:
            continue
        kind, _meaning, count_cell = cols[0], cols[1], cols[2]
        try:
            count = int(count_cell)
        except ValueError:
            errors.append(
                f"{DOC_REL.as_posix()}:{index}: kind count is not an integer"
            )
            continue
        if kind in KINDS:
            kinds[kind] = count
            kind_lines[kind] = index
        elif not kind:
            kind_total = count
            kind_total_line = index

    crates: dict[str, int] = {}
    crate_line = None
    crate_at = body.find("By crate:")
    if crate_at < 0:
        errors.append(f"{DOC_REL.as_posix()}: missing By crate totals")
    else:
        crate_line = offset + body[:crate_at].count("\n") + 1
        rest = body[crate_at + len("By crate:") :]
        blank = re.search(r"\n\s*\n", rest)
        para = rest if blank is None else rest[: blank.start()]
        for name, count in CRATE_COUNT.findall(para):
            crates[name] = int(count)
        if not crates:
            errors.append(f"{DOC_REL.as_posix()}:{crate_line}: By crate list is empty")

    return (
        Totals(
            overall,
            overall_line,
            kinds,
            kind_lines,
            kind_total,
            kind_total_line,
            crates,
            crate_line,
        ),
        errors,
    )


def parse_doc(text: str) -> tuple[list[TableRow], Totals, list[str]]:
    rows, row_errors = parse_census_rows(text)
    totals, total_errors = parse_totals(text)
    return rows, totals, row_errors + total_errors


def compare(
    constants: list[Constant], rows: list[TableRow], totals: Totals
) -> list[str]:
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
        extra_source = source_hits[len(table_hits) :]
        extra_table = table_hits[len(source_hits) :]
        for constant in extra_source:
            findings.append(
                f"source constant missing from table: {constant.relative}:"
                f"{constant.line} {name}"
            )
        for row in extra_table:
            findings.append(
                f"table row with no source constant: {DOC_REL.as_posix()}:"
                f"{row.line} {crate} {file} {name}"
            )

    n_source = len(constants)
    n_rows = len(rows)
    loc = f"{DOC_REL.as_posix()}:{totals.overall_line}" if totals.overall_line else DOC_REL.as_posix()
    if totals.overall is not None and totals.overall != n_rows:
        findings.append(
            f"stated overall {totals.overall} at {loc}, table has {n_rows} rows"
        )
    if totals.overall is not None and totals.overall != n_source:
        findings.append(
            f"stated overall {totals.overall} at {loc}, source has {n_source} constants"
        )
    if totals.overall is None and n_rows != n_source:
        findings.append(
            f"table has {n_rows} rows, source has {n_source} constants"
        )

    row_kinds = Counter(row.kind for row in rows)
    for kind in KINDS:
        stated = totals.kinds.get(kind)
        actual = row_kinds.get(kind, 0)
        if stated is None and actual == 0:
            continue
        if stated is None:
            findings.append(f"kind {kind}: table has {actual}, missing from kind totals")
            continue
        kind_loc = (
            f"{DOC_REL.as_posix()}:{totals.kind_lines[kind]}"
            if kind in totals.kind_lines
            else DOC_REL.as_posix()
        )
        if stated != actual:
            findings.append(
                f"kind {kind}: stated {stated} at {kind_loc}, table has {actual}"
            )

    kind_sum = sum(totals.kinds.values())
    if totals.kind_total is not None:
        total_loc = (
            f"{DOC_REL.as_posix()}:{totals.kind_total_line}"
            if totals.kind_total_line
            else DOC_REL.as_posix()
        )
        if totals.kind_total != kind_sum:
            findings.append(
                f"kind table total {totals.kind_total} at {total_loc} "
                f"does not equal the sum of kind counts {kind_sum}"
            )
        if totals.overall is not None and totals.kind_total != totals.overall:
            findings.append(
                f"kind table total {totals.kind_total} at {total_loc} "
                f"does not equal overall {totals.overall}"
            )

    src_crates = Counter(constant.crate for constant in constants)
    row_crates = Counter(row.crate for row in rows)
    crate_loc = (
        f"{DOC_REL.as_posix()}:{totals.crate_line}"
        if totals.crate_line
        else DOC_REL.as_posix()
    )
    for crate in sorted(set(totals.crates) | set(src_crates) | set(row_crates)):
        stated = totals.crates.get(crate)
        actual_rows = row_crates.get(crate, 0)
        actual_src = src_crates.get(crate, 0)
        if stated is None:
            findings.append(
                f"crate {crate}: missing from the By crate list at {crate_loc} "
                f"(table {actual_rows}, source {actual_src})"
            )
            continue
        if stated != actual_rows:
            findings.append(
                f"crate {crate}: stated {stated} at {crate_loc}, table has {actual_rows}"
            )
        if stated != actual_src:
            findings.append(
                f"crate {crate}: stated {stated} at {crate_loc}, source has {actual_src}"
            )

    crate_sum = sum(totals.crates.values())
    if totals.crates and totals.overall is not None and crate_sum != totals.overall:
        findings.append(
            f"By crate sum {crate_sum} at {crate_loc} does not equal overall {totals.overall}"
        )
    return findings


def check_tree(root: Path) -> list[str]:
    doc = root / DOC_REL
    if not doc.is_file():
        return [f"{DOC_REL.as_posix()}: missing"]
    rows, totals, errors = parse_doc(doc.read_text(encoding="utf-8"))
    if errors:
        return errors + compare(production_constants(root), rows, totals)
    return compare(production_constants(root), rows, totals)


@SOURCE_POLICY.analysis_session
def main() -> int:
    findings = check_tree(ROOT)
    if findings:
        for line in findings:
            print(line, file=sys.stderr)
        print(
            f"resource-caps census: {len(findings)} mismatch(es)",
            file=sys.stderr,
        )
        return 1
    print("resource-caps census: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
