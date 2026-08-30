#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the dialect capability registry, ``docs/dialect-support.toml``.

The identity registry (``docs/dialects.toml``, checked by
``scripts/check-dialects.py``) says which dialects exist. This one says what
cadmpeg does with each of them, and it changes per commit. It is a sibling
script rather than a section of the identity checker because its inputs are
different in kind: the identity checker reads one TOML file, while this one
reads three TOML files and the golden snapshot domains on disk.

The rules, all cross-referencing:

* every ``[[support]]`` row names a dialect the identity registry declares;
* every identity row has exactly one support row (totality, both ways);
* each reachable residual identity has the read disposition declared by its
  ``unknown_kind``;
* **fixture gating** -- a row may not claim a read score (``L0``..``L9``)
  with no golden snapshot domain. ``detected`` is the honest cell for an unwitnessed
  dialect, and the resulting unevenness is the output, not a defect;
* a row that is ``read = "refused"`` carries a ``reason`` that references a
  declared registry id or a named evidence gap;
* compiled write-catalog policy is checked in ``cadmpeg-registry`` tests,
  against the embedded identity and support registries.

Fixture self-verification (design section 5, "decode each fixture, read back
the emitted dialect id") is a Rust duty and lives with the per-codec golden
suites. This checker derives each dialect's snapshot domain from those
checked-in results and uses it directly as the read-score gate.

Run ``--self-test`` to execute the synthesized-violation suite in
``scripts/test_check_dialect_support.py``; every rule below fires there.
"""

from __future__ import annotations

import argparse
import datetime
import json
import re
import sys
import tomllib
import unittest
from collections import Counter
from pathlib import Path

from dialect_support_data import RegistryDataError, joined_rows, load_documents

ROOT = Path(__file__).resolve().parent.parent
IDENTITY_REL = Path("docs") / "dialects.toml"
SUPPORT_REL = Path("docs") / "dialect-support.toml"
EVALUATIONS_REL = Path("docs") / "evaluations.toml"
SELF_TEST_REL = Path("scripts") / "test_check_dialect_support.py"

ROW_KEYS = frozenset({"dialect", "read", "write", "reason"})
REQUIRED_ROW_KEYS = ("dialect", "read", "write")
FORMAT_KEYS = frozenset({"level", "scored"})
REQUIRED_FORMAT_KEYS = ("level", "scored")
EVALUATION_KEYS = frozenset({"dialect", "date", "level", "files", "result"})
REQUIRED_EVALUATION_KEYS = ("dialect", "date", "level", "files", "result")
REGISTRY_ONLY_FORMATS = frozenset({"acis", "parasolid"})
PATH_LIKE = re.compile(r"(?:[/\\]|(?:^|\s)\.\.?(?:[/\\]|$)|\b[A-Za-z]:[/\\])")

# `L0`..`L9` are the ladder; the other three are the non-score dispositions
# (design section 6.2). `detected` is the floor an unwitnessed row may claim.
READ_SCORES = frozenset(f"L{n}" for n in range(10))
READ_OTHER = frozenset({"detected", "refused", "unclassified-recovered"})
READ_VALUES = READ_SCORES | READ_OTHER

WRITE_VALUES = frozenset({"verified", "emitted", "preserved", "none"})
EVIDENCE_GAP = re.compile(r"evidence gap `([a-z0-9]+(?:-[a-z0-9]+)*)`")
REGISTRY_ID = re.compile(r"`([a-z0-9]+:[a-z0-9.-]+)`")

# Residual identities state whether classification recovers or refuses the
# unmatched input. Their capability row must state the same disposition.
RESIDUAL_READS = {
    "recovered-residual": "unclassified-recovered",
    "refused-residual": "refused",
}


def _is_table(value: object) -> bool:
    return isinstance(value, dict)


def _load(path: Path, label: str, failures: list[str]) -> dict | None:
    if not path.is_file():
        failures.append(f"{label}: not found")
        return None
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except tomllib.TOMLDecodeError as err:
        failures.append(f"{label}: parse error: {err}")
    except OSError as err:
        failures.append(f"{label}: {err}")
    return None


# --------------------------------------------------------------------------
# Row rules.
# --------------------------------------------------------------------------


def check_row(
    row: object,
    index: int,
    known: set[str],
    snapshot_domains: dict[str, list[str]],
    failures: list[str],
):
    """Validate one ``[[support]]`` row. Returns ``(dialect id, read, write)``."""
    if not _is_table(row):
        failures.append(f"support #{index}: not a table")
        return None, None, None
    raw = row.get("dialect")
    label = raw if isinstance(raw, str) and raw else f"support #{index}"

    for key in REQUIRED_ROW_KEYS:
        if key not in row:
            failures.append(f"{label}: missing {key}")
    for key in sorted(set(row) - ROW_KEYS):
        failures.append(f"{label}: unknown key {key}")

    dialect_id: str | None = None
    if "dialect" in row:
        if not isinstance(raw, str):
            failures.append(f"{label}: dialect must be a string")
        elif raw not in known:
            failures.append(f"{label}: no identity row for dialect {raw}")
        else:
            dialect_id = raw

    read = row.get("read")
    if "read" in row and read not in READ_VALUES:
        failures.append(f"{label}: read must be one of L0..L9, detected, refused, unclassified-recovered")
        read = None

    write = row.get("write")
    if "write" in row and write not in WRITE_VALUES:
        failures.append(f"{label}: write must be one of verified, emitted, preserved, none")
        write = None

    reason = row.get("reason")
    if "reason" in row and (not isinstance(reason, str) or not reason.strip()):
        failures.append(f"{label}: reason must be a non-empty string")
        reason = None

    # Snapshot-domain gating. A score is a claim about decoded files; without
    # a golden result that emits this dialect, `detected` is the honest cell.
    if read in READ_SCORES and dialect_id not in snapshot_domains:
        failures.append(
            f"{label}: read {read} with no golden snapshot domain; "
            "an unwitnessed row cannot claim above detected"
        )

    if read == "refused" and not (isinstance(reason, str) and reason.strip()):
        failures.append(f"{label}: read refused requires a reason")
    elif read == "refused" and isinstance(reason, str):
        referenced_ids = set(REGISTRY_ID.findall(reason))
        for unknown_id in sorted(referenced_ids - known):
            failures.append(f"{label}: refusal reason references unknown registry id {unknown_id}")
        if not referenced_ids and EVIDENCE_GAP.search(reason) is None:
            failures.append(
                f"{label}: refusal reason must reference a registry id or a named evidence gap"
            )
    return dialect_id, read, write


def check_totality(known: set[str], covered: Counter[str], failures: list[str]) -> None:
    """Both directions: no identity row uncovered, no dialect covered twice."""
    for dialect_id in sorted(known - set(covered)):
        failures.append(f"{dialect_id}: identity row has no support row")
    for dialect_id, count in sorted(covered.items()):
        if count > 1:
            failures.append(f"{dialect_id}: {count} support rows; expected one")


def check_residual_dispositions(
    identity: dict, reads: dict[str, object], failures: list[str]
) -> None:
    """Require each reachable residual identity to match its read disposition."""
    for row in identity.get("dialect", []):
        if not _is_table(row):
            continue
        dialect_id = row.get("id")
        expected = RESIDUAL_READS.get(row.get("unknown_kind"))
        if not isinstance(dialect_id, str) or expected is None or dialect_id not in reads:
            continue
        actual = reads[dialect_id]
        if actual != expected:
            failures.append(
                f"{dialect_id}: unknown_kind {row['unknown_kind']} requires "
                f"read {expected}, got {actual}"
            )


def check_formats(value: object, known: set[str], failures: list[str]) -> dict[str, tuple[int, list[str]]]:
    """Validate owner-declared levels and scored cuts."""
    if not _is_table(value):
        failures.append("format: must be a table")
        return {}
    codec_formats = {dialect_id.partition(":")[0] for dialect_id in known} - REGISTRY_ONLY_FORMATS
    for format_id in sorted(set(value) - codec_formats):
        failures.append(f"format.{format_id}: unknown codec format")
    for format_id in sorted(codec_formats - set(value)):
        failures.append(f"format.{format_id}: missing format block")
    checked: dict[str, tuple[int, list[str]]] = {}
    for format_id in sorted(codec_formats & set(value)):
        block = value[format_id]
        label = f"format.{format_id}"
        if not _is_table(block):
            failures.append(f"{label}: must be a table")
            continue
        for key in REQUIRED_FORMAT_KEYS:
            if key not in block:
                failures.append(f"{label}: missing {key}")
        for key in sorted(set(block) - FORMAT_KEYS):
            failures.append(f"{label}: unknown key {key}")
        level = block.get("level")
        if not isinstance(level, int) or isinstance(level, bool) or not 0 <= level <= 9:
            failures.append(f"{label}: level must be an integer from 0 through 9")
            level = None
        scored = block.get("scored")
        valid_scored: list[str] = []
        if not isinstance(scored, list) or not scored:
            failures.append(f"{label}: scored must be a non-empty list")
        else:
            counts = Counter(entry for entry in scored if isinstance(entry, str))
            for dialect_id, count in sorted(counts.items()):
                if count > 1:
                    failures.append(f"{label}: duplicate scored dialect {dialect_id}")
            for entry in scored:
                if not isinstance(entry, str) or not entry:
                    failures.append(f"{label}: scored dialect must be a non-empty string")
                elif entry not in known:
                    failures.append(f"{label}: scored dialect {entry} has no identity row")
                elif not entry.startswith(format_id + ":"):
                    failures.append(f"{label}: scored dialect {entry} belongs to another format")
                else:
                    valid_scored.append(entry)
        if level is not None:
            checked[format_id] = (level, valid_scored)
    return checked


def check_evaluations(value: object, known: set[str], failures: list[str]) -> dict[str, list[int]]:
    """Validate maintainer evaluation records and return levels by dialect."""
    rows = value.get("evaluation") if _is_table(value) else None
    if not isinstance(rows, list):
        failures.append("evaluations.toml: [[evaluation]] must be an array of tables")
        return {}
    levels: dict[str, list[int]] = {}
    for index, row in enumerate(rows):
        label = f"evaluation #{index}"
        if not _is_table(row):
            failures.append(f"{label}: not a table")
            continue
        dialect = row.get("dialect")
        if isinstance(dialect, str) and dialect:
            label = dialect
        for key in REQUIRED_EVALUATION_KEYS:
            if key not in row:
                failures.append(f"{label}: missing {key}")
        for key in sorted(set(row) - EVALUATION_KEYS):
            failures.append(f"{label}: unknown key {key}")
        if not isinstance(dialect, str) or not dialect:
            failures.append(f"{label}: dialect must be a non-empty string")
            dialect = None
        elif dialect not in known:
            failures.append(f"{label}: no identity row for dialect {dialect}")
        date = row.get("date")
        if not isinstance(date, datetime.date) or isinstance(date, datetime.datetime):
            failures.append(f"{label}: date must be a TOML local date")
        level = row.get("level")
        if not isinstance(level, int) or isinstance(level, bool) or not 0 <= level <= 9:
            failures.append(f"{label}: level must be an integer from 0 through 9")
            level = None
        files = row.get("files")
        if not isinstance(files, int) or isinstance(files, bool) or files < 1:
            failures.append(f"{label}: files must be a positive integer")
        result = row.get("result")
        if not isinstance(result, str) or not result.strip():
            failures.append(f"{label}: result must be a non-empty string")
        for key, entry in row.items():
            if isinstance(entry, str) and PATH_LIKE.search(entry):
                failures.append(f"{label}: {key} must not contain a path-like string")
        if dialect in known and level is not None:
            levels.setdefault(dialect, []).append(level)
    return levels


def check_declared_levels(
    formats: dict[str, tuple[int, list[str]]],
    reads: dict[str, object],
    evaluations: dict[str, list[int]],
    failures: list[str],
) -> None:
    """Reject evidence that contradicts an owner-declared format level."""
    for format_id, (level, scored) in sorted(formats.items()):
        for dialect_id in scored:
            read = reads.get(dialect_id)
            if read == "refused":
                failures.append(f"{format_id}: scored dialect {dialect_id} is refused")
            elif read == "unclassified-recovered" and dialect_id not in evaluations:
                failures.append(
                    f"{format_id}: scored unclassified-recovered dialect {dialect_id} requires an evaluation"
                )
            elif isinstance(read, str) and read in READ_SCORES and int(read[1:]) < level:
                failures.append(f"{format_id}: scored dialect {dialect_id} read {read} contradicts L{level}")
            for evaluated in evaluations.get(dialect_id, []):
                if evaluated < level:
                    failures.append(
                        f"{format_id}: evaluation L{evaluated} for scored dialect {dialect_id} contradicts L{level}"
                    )


# --------------------------------------------------------------------------
# Snapshot cross-check: the Python half of fixture self-verification.
# --------------------------------------------------------------------------


def golden_snapshots(root: Path) -> list[Path]:
    """Every checked-in golden snapshot, in both layouts the harness writes.

    Codecs with more than one branch write ``tests/golden/<branch>/<stem>.json``
    (``inspect``, ``decode``, ``encode``); a codec with one branch writes
    ``tests/golden/<stem>.json`` flat. Both are read: ``inspect`` pins the
    dialect for a fixture whose ``decode`` refuses.
    """
    snaps = sorted(root.glob("crates/*/tests/golden/*/*.json"))
    snaps += sorted(root.glob("crates/*/tests/golden/*.json"))
    return snaps


def snapshot_dialects(root: Path) -> dict[str, set[str]]:
    """Map each golden snapshot's fixture path to the ids it pins."""
    found: dict[str, set[str]] = {}
    for snap in golden_snapshots(root):
        try:
            data = json.loads(snap.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        ids = _walk_dialect_ids(data)
        if not ids:
            continue
        crate = snap.relative_to(root).parts[1]
        for candidate in _fixture_candidates(root, crate, snap.stem):
            found.setdefault(candidate.relative_to(root).as_posix(), set()).update(ids)
            break
    return found


def _fixture_candidates(root: Path, crate: str, stem: str) -> list[Path]:
    """Fixture files a golden snapshot basename can name, in harness order.

    The shared harness takes the crate's ``tests/golden/fixtures`` by default
    and accepts an override (``Harness::with_fixture_dir``). Two overrides are
    live: STEP reads ``tests/fixtures`` and FreeCAD reads the charter fixtures
    under ``corpus/freecad_fcstd/fixtures``.
    """
    roots = [
        root / "crates" / crate / "tests" / "golden" / "fixtures",
        root / "crates" / crate / "tests" / "fixtures",
        root / "corpus" / "freecad_fcstd" / "fixtures",
    ]
    found: list[Path] = []
    for base in roots:
        if base.is_dir():
            found.extend(sorted(base.rglob(stem + ".*")))
    return found


def _walk_dialect_ids(node: object) -> set[str]:
    ids: set[str] = set()
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "dialect" and isinstance(value, str) and ":" in value:
                ids.add(value)
            elif key == "dialects" and isinstance(value, list):
                for entry in value:
                    if isinstance(entry, dict) and isinstance(entry.get("dialect"), str):
                        ids.add(entry["dialect"])
            else:
                ids |= _walk_dialect_ids(value)
    elif isinstance(node, list):
        for entry in node:
            ids |= _walk_dialect_ids(entry)
    return ids


def snapshot_domains(root: Path) -> dict[str, list[str]]:
    """Return the golden fixture paths that emit each dialect id."""
    domains: dict[str, list[str]] = {}
    for path, dialects in sorted(snapshot_dialects(root).items()):
        for dialect_id in sorted(dialects):
            domains.setdefault(dialect_id, []).append(path)
    return domains


# --------------------------------------------------------------------------
# Driver.
# --------------------------------------------------------------------------


def check(root: Path) -> tuple[list[str], str]:
    """Check the capability registry under ``root``. Returns failures + summary."""
    failures: list[str] = []

    try:
        identity, support, evaluations_doc = load_documents(root)
    except RegistryDataError as error:
        failures.append(str(error))
        return failures, ""

    known = {
        row["id"]
        for row in identity.get("dialect", [])
        if _is_table(row) and isinstance(row.get("id"), str)
    }
    if not known:
        failures.append(f"{IDENTITY_REL.as_posix()}: no identity rows to support")
        return failures, ""
    try:
        joined_rows(identity, support)
    except RegistryDataError as error:
        failures.append(str(error))

    formats = check_formats(support.get("format"), known, failures)
    evaluations = check_evaluations(evaluations_doc, known, failures)
    domains = snapshot_domains(root)

    rows = support.get("support")
    if rows is None:
        failures.append("no [[support]] rows")
        return failures, ""
    if not isinstance(rows, list) or not rows:
        failures.append("[[support]] must be a non-empty array of tables")
        return failures, ""

    covered: Counter[str] = Counter()
    writes: dict[str, object] = {}
    reads: dict[str, object] = {}
    tally: Counter[str] = Counter()
    for index, row in enumerate(rows):
        dialect_id, read, write = check_row(row, index, known, domains, failures)
        if dialect_id is None:
            continue
        covered[dialect_id] += 1
        writes[dialect_id] = write
        reads[dialect_id] = read
        if isinstance(read, str):
            tally[read] += 1

    check_totality(known, covered, failures)
    check_residual_dispositions(identity, reads, failures)
    check_declared_levels(formats, reads, evaluations, failures)
    scored = sum(n for value, n in tally.items() if value in READ_SCORES)
    fixture_total = sum(len(paths) for paths in domains.values())
    summary = (
        f"dialect-support: ok ({len(rows)} rows covering {len(known)} identity rows; "
        f"{scored} scored, {tally['detected']} detected, {tally['refused']} refused, "
        f"{tally['unclassified-recovered']} unclassified-recovered; "
        f"{fixture_total} golden fixture-domain confirmations; "
        f"{sum(1 for value in writes.values() if value == 'preserved')} preserved)"
    )
    return failures, summary


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
