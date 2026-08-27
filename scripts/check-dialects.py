#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the dialect identity registry, ``docs/dialects.toml``.

The registry pins format-identity names: an id chosen once is an id forever.
This checker is its oracle. Version 1 is registry-internal only -- schema,
id grammar, witness form, and the IGES admission lattice. The cross-checks
against codec behaviour (emitted ids, rendered support tables) arrive with
later phases and are stubbed at the bottom of this file so their wiring
point is fixed now.

Capability -- what cadmpeg does with each dialect, including fixture gating
-- lives in ``docs/dialect-support.toml`` and is checked by the sibling
``scripts/check-dialect-support.py``.

Run ``--self-test`` to execute the synthesized-violation suite in
``scripts/test_check_dialects.py``; every rule below fires there.
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
import unittest
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REGISTRY_REL = Path("docs") / "dialects.toml"
SELF_TEST_REL = Path("scripts") / "test_check_dialects.py"

# The schema keys a `[[dialect]]` row may carry. Anything else is a typo or an
# unreviewed schema extension; both are failures.
ROW_KEYS = frozenset(
    {"id", "title", "discriminants", "witness", "seam", "supersedes", "adds", "subtracts"}
)
REQUIRED_ROW_KEYS = ("id", "title", "discriminants", "witness")

# `supersedes`/`adds`/`subtracts` are admitted only where the checker consumes
# them (consumer law, design section 5). IGES is the one consumer today.
LATTICE_KEYS = ("supersedes", "adds", "subtracts")
LATTICE_FORMATS = frozenset({"iges"})

WITNESS_PREFIXES = ("spec:", "corpus:", "code:")

FORMAT_ID = re.compile(r"[a-z0-9]+")
# Dots are legal in a dialect name: `iges:5.3-fixed-ascii`.
DIALECT_NAME = re.compile(r"[a-z0-9.-]+")
# `subtracts` is a list of single (type, form) admissions.
ADMISSION = re.compile(r"(\d+):(\d+)")
# `adds` also accepts a closed form range, because the additive half of the
# IGES 5.0 table includes the implementor-defined band 5001..=9999
# (crates/cadmpeg-codec-iges/src/profile.rs:101 via profile.rs:124).
ADMISSION_RANGE = re.compile(r"(\d+):(\d+)(?:-(\d+))?")


def _is_table(value: object) -> bool:
    return isinstance(value, dict)


def check_formats(formats: object, failures: list[str]) -> dict[str, dict]:
    """Validate the ``[format.<id>]`` table and return it, or ``{}``."""
    if formats is None:
        failures.append("no [format.<id>] entries")
        return {}
    if not _is_table(formats):
        failures.append("[format] is not a table")
        return {}
    checked: dict[str, dict] = {}
    for name, body in formats.items():
        if not FORMAT_ID.fullmatch(name):
            failures.append(f"format {name}: id must match [a-z0-9]+")
            continue
        if not _is_table(body):
            failures.append(f"format {name}: not a table")
            continue
        complete = body.get("complete")
        if complete is None:
            failures.append(f"format {name}: missing complete")
        elif not isinstance(complete, bool):
            failures.append(f"format {name}: complete must be a boolean")
        checked[name] = body
    return checked


def _check_witness_path(label: str, kind: str, rel: str, root: Path, failures: list[str]) -> None:
    """Require a witness path to be repo-relative and to name a file that exists."""
    if not rel:
        failures.append(f"{label}: {kind} witness names no path")
        return
    path = Path(rel)
    if path.is_absolute() or ".." in path.parts:
        failures.append(f"{label}: {kind} witness must be a repo-relative path: {rel}")
        return
    if not (root / path).is_file():
        failures.append(f"{label}: {kind} witness file not found: {rel}")


def _code_witness_file(rest: str) -> str:
    """Split ``<file>[:<line>]`` and return the file part.

    The line number is evidence of where the discriminant is read, not part of
    the file's identity. Line drift is expected as the codec changes, so the
    line is never checked -- only the file it points into.
    """
    head, sep, tail = rest.rpartition(":")
    if sep and tail.isdigit():
        return head.strip()
    return rest


def check_witness(label: str, witness: object, root: Path, failures: list[str]) -> bool:
    """Validate one ``witness`` value. Returns True when it is a ``code:`` debt."""
    if not isinstance(witness, str):
        failures.append(f"{label}: witness must be a string")
        return False
    if not witness.startswith(WITNESS_PREFIXES):
        failures.append(f"{label}: witness must start with spec:, corpus:, or code:")
        return False
    if witness.startswith("code:"):
        rest = witness[len("code:") :].strip()
        _check_witness_path(label, "code", _code_witness_file(rest), root, failures)
        return True
    if witness.startswith("corpus:"):
        _check_witness_path(label, "corpus", witness[len("corpus:") :].strip(), root, failures)
    return False


def check_lattice_shape(label: str, fmt: str, row: dict, failures: list[str]) -> None:
    """Validate the lattice keys on one row, except cross-row id resolution."""
    for key in LATTICE_KEYS:
        if key not in row:
            continue
        if fmt not in LATTICE_FORMATS:
            failures.append(f"{label}: {key} is admitted only on {'/'.join(sorted(LATTICE_FORMATS))} rows")
            continue
        entries = row[key]
        if not isinstance(entries, list) or not all(isinstance(e, str) for e in entries):
            failures.append(f"{label}: {key} must be a list of strings")
            continue
        if key == "supersedes":
            continue
        pattern = ADMISSION_RANGE if key == "adds" else ADMISSION
        shape = "type:form or type:low-high" if key == "adds" else "type:form"
        for entry in entries:
            match = pattern.fullmatch(entry)
            if match is None:
                failures.append(f"{label}: {key} entry {entry!r} is not {shape}")
                continue
            high = match.group(3) if key == "adds" else None
            if high is not None and int(high) < int(match.group(2)):
                failures.append(f"{label}: {key} entry {entry!r} has an inverted form range")


def check_row(row: object, index: int, formats: dict, root: Path, failures: list[str]):
    """Validate one ``[[dialect]]`` row. Returns ``(id, format, code_debt)``."""
    if not _is_table(row):
        failures.append(f"dialect #{index}: not a table")
        return None, None, False
    raw_id = row.get("id")
    label = raw_id if isinstance(raw_id, str) and raw_id else f"dialect #{index}"

    for key in REQUIRED_ROW_KEYS:
        if key not in row:
            failures.append(f"{label}: missing {key}")
    for key in sorted(set(row) - ROW_KEYS):
        failures.append(f"{label}: unknown key {key}")

    fmt: str | None = None
    dialect_id: str | None = None
    if "id" in row:
        if not isinstance(raw_id, str):
            failures.append(f"{label}: id must be a string")
        elif raw_id.count(":") != 1:
            failures.append(f"{label}: id must be <format>:<name>")
        else:
            head, name = raw_id.split(":")
            if not FORMAT_ID.fullmatch(head):
                failures.append(f"{label}: format prefix must match [a-z0-9]+")
            else:
                # Keep the prefix even when it is unregistered, so the lattice
                # rule below reports the real reason rather than a cascade.
                fmt = head
                if head not in formats:
                    failures.append(f"{label}: no [format.{head}] entry")
            if not DIALECT_NAME.fullmatch(name):
                failures.append(f"{label}: name must be lowercase [a-z0-9.-]+")
            else:
                dialect_id = raw_id

    title = row.get("title")
    if "title" in row and (not isinstance(title, str) or not title.strip()):
        failures.append(f"{label}: title must be a non-empty string")

    if "seam" in row and not isinstance(row["seam"], str):
        failures.append(f"{label}: seam must be a string")

    discriminants = row.get("discriminants")
    if "discriminants" in row:
        if not _is_table(discriminants):
            failures.append(f"{label}: discriminants must be a table")
        elif not discriminants:
            failures.append(f"{label}: discriminants must not be empty")
        else:
            for key, value in discriminants.items():
                if not isinstance(value, str):
                    failures.append(f"{label}: discriminant {key} must be a string")

    code_debt = check_witness(label, row["witness"], root, failures) if "witness" in row else False

    check_lattice_shape(label, fmt or "", row, failures)
    return dialect_id, fmt, code_debt


def check_supersedes_graph(rows: list[dict], known: set[str], failures: list[str]) -> None:
    """Resolve ``supersedes`` references and prove the expansion terminates."""
    graph: dict[str, list[str]] = {}
    for row in rows:
        row_id = row.get("id")
        if not isinstance(row_id, str):
            continue
        parents = row.get("supersedes")
        if not isinstance(parents, list):
            continue
        edges: list[str] = []
        for parent in parents:
            if not isinstance(parent, str):
                continue
            if parent not in known:
                failures.append(f"{row_id}: supersedes unknown id {parent}")
                continue
            edges.append(parent)
        graph[row_id] = edges

    # Iterative DFS with a three-colour marking; a grey target is a back edge.
    WHITE, GREY, BLACK = 0, 1, 2
    colour: dict[str, int] = {}
    for start in graph:
        if colour.get(start, WHITE) != WHITE:
            continue
        stack: list[tuple[str, list[str]]] = [(start, list(graph.get(start, ())))]
        colour[start] = GREY
        path = [start]
        while stack:
            node, pending = stack[-1]
            if not pending:
                colour[node] = BLACK
                stack.pop()
                path.pop()
                continue
            nxt = pending.pop()
            state = colour.get(nxt, WHITE)
            if state == GREY:
                cycle = path[path.index(nxt) :] + [nxt] if nxt in path else [node, nxt]
                failures.append(f"{start}: supersedes cycle {' -> '.join(cycle)}")
                continue
            if state == BLACK:
                continue
            colour[nxt] = GREY
            path.append(nxt)
            stack.append((nxt, list(graph.get(nxt, ()))))


def check(root: Path) -> tuple[list[str], str]:
    """Check the registry under ``root``. Returns ``(failures, summary)``."""
    failures: list[str] = []
    path = root / REGISTRY_REL
    if not path.is_file():
        return [f"{REGISTRY_REL.as_posix()}: not found"], ""
    try:
        with path.open("rb") as handle:
            data = tomllib.load(handle)
    except tomllib.TOMLDecodeError as err:
        return [f"{REGISTRY_REL.as_posix()}: parse error: {err}"], ""
    except OSError as err:
        return [f"{REGISTRY_REL.as_posix()}: {err}"], ""

    formats = check_formats(data.get("format"), failures)

    rows = data.get("dialect")
    if rows is None:
        failures.append("no [[dialect]] rows")
        return failures, ""
    if not isinstance(rows, list) or not rows:
        failures.append("[[dialect]] must be a non-empty array of tables")
        return failures, ""

    seen: set[str] = set()
    per_format: Counter[str] = Counter()
    code_debt = 0
    for index, row in enumerate(rows):
        dialect_id, fmt, debt = check_row(row, index, formats, root, failures)
        if dialect_id is not None:
            if dialect_id in seen:
                failures.append(f"{dialect_id}: duplicate id")
            seen.add(dialect_id)
        if fmt is not None:
            per_format[fmt] += 1
        code_debt += int(debt)

    check_supersedes_graph([r for r in rows if _is_table(r)], seen, failures)

    counts = ", ".join(f"{fmt} {n}" for fmt, n in sorted(per_format.items()))
    summary = (
        f"dialects: ok ({len(rows)} rows across {len(formats)} formats: {counts}; "
        f"{code_debt} rows on code: witnesses awaiting a spec/corpus upgrade)"
    )
    return failures, summary


# --------------------------------------------------------------------------
# Extension points. Later phases replace the body; the contract is fixed now.
# --------------------------------------------------------------------------


def check_codec_emitted_ids(root: Path) -> list[str]:
    """The ids a codec can emit must equal the registry's set for that format.

    Contract: for each ``[format.<id>]``, collect the dialect ids the codec
    can actually emit at runtime and compare the two sets. A registry id no
    codec emits is a dead name; an emitted id absent from the registry is an
    unpinned name. Both are failures once a codec emits ids at all.
    """
    return ["not yet enforced"]


def check_support_tables(root: Path) -> list[str]:
    """Rendered support tables must match the two registries.

    Contract: every table generated into ``docs/format-support.md`` (and the
    crate READMEs) is regenerated from ``docs/dialects.toml`` plus
    ``docs/dialect-support.toml`` and compared byte for byte with what is
    committed.
    """
    return ["not yet enforced"]


def self_test() -> int:
    """Run the synthesized-violation suite; every rule above fires there."""
    suite = unittest.defaultTestLoader.discover(
        start_dir=str(ROOT / "scripts"),
        pattern=SELF_TEST_REL.name,
        top_level_dir=str(ROOT / "scripts"),
    )
    result = unittest.TextTestRunner(verbosity=1).run(suite)
    return 0 if result.wasSuccessful() else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root",
        nargs="?",
        type=Path,
        default=ROOT,
        help="repository root (default: parent of scripts/)",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the synthesized-violation suite instead of checking the registry",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    failures, summary = check(args.root.resolve())
    if failures:
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
        return 1
    print(summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
