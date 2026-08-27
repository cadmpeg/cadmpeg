#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the dialect capability registry, ``docs/dialect-support.toml``.

The identity registry (``docs/dialects.toml``, checked by
``scripts/check-dialects.py``) says which dialects exist. This one says what
cadmpeg does with each of them, and it changes per commit. It is a sibling
script rather than a section of the identity checker because its inputs are
different in kind: the identity checker reads one TOML file, this one reads
two TOML files, the fixture tree on disk, and the Rust write-target catalogs.

The rules, all cross-referencing:

* every ``[[support]]`` row names a dialect the identity registry declares;
* every identity row has exactly one support row (totality, both ways);
* every path in ``fixtures`` names a file that exists;
* **fixture gating** -- a row may not claim a read score (``L0``..``L9``)
  with zero fixtures. ``detected`` is the honest cell for an unwitnessed
  dialect, and the resulting unevenness is the output, not a defect;
* a row that is ``read = "refused"``, or that has no fixtures, carries a
  ``reason``;
* the ids each encoder's ``targets()`` exports are a **subset** of that
  format's identity rows, and each exported id has a support row whose
  ``write`` is not ``none``. Subset, not equality: read-side rows
  (``step:ap242``) and residual rows (``*:unknown``) are legitimately not
  write targets;
* **the two write capabilities stay apart** (design section 8.2). A
  ``preserved`` row must NOT appear in any ``targets()`` catalog: it is
  reachable only from a source that already is that dialect, so listing it
  would advertise an output arbitrary input cannot reach. Conversely a
  ``verified``/``emitted`` row on a format that has a catalog must appear
  in it.

Write-target mechanism. The catalogs are ``const``/``static`` tables of
``TargetDescriptor`` literals in ``crates/cadmpeg-codec-*/src/**.rs``. This
script parses the ``id:`` field of each literal. Two spellings occur: a bare
string literal, and ``<Enum>::<Variant>.pinned()`` (IGES), resolved through
the ``pinned()`` match arms in the same file. Any third spelling is a
failure, never a silent skip -- an unparsed catalog would turn the subset
rule into a no-op.

Fixture self-verification (design section 5, "decode each fixture, read back
the emitted dialect id") is not done here. It is a Rust duty and lives with
the per-codec golden suites, whose checked-in decode snapshots pin the
emitted dialect id per fixture; ``check_snapshot_dialects`` below re-reads
those snapshots and compares them with this registry, which is the Python
half of the same guarantee.

Run ``--self-test`` to execute the synthesized-violation suite in
``scripts/test_check_dialect_support.py``; every rule below fires there.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
import unittest
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
IDENTITY_REL = Path("docs") / "dialects.toml"
SUPPORT_REL = Path("docs") / "dialect-support.toml"
SELF_TEST_REL = Path("scripts") / "test_check_dialect_support.py"

ROW_KEYS = frozenset({"dialect", "grammar", "read", "write", "fixtures", "reason"})
REQUIRED_ROW_KEYS = ("dialect", "read", "write", "fixtures")

# `L0`..`L9` are the ladder; the other three are the non-score dispositions
# (design section 6.2). `detected` is the floor a fixture-less row may claim.
READ_SCORES = frozenset(f"L{n}" for n in range(10))
READ_OTHER = frozenset({"detected", "refused", "unclassified-recovered"})
READ_VALUES = READ_SCORES | READ_OTHER

# `verified` and `emitted` are synthesis: the encoder builds the dialect from
# neutral IR for arbitrary input, and the id is in its `targets()` catalog.
# `preserved` is the other write capability (design section 8.2): the dialect
# is reachable only when the source already is one, through replay or patch of
# a retained baseline under `TargetRequest::Inherit`. It is input-conditioned,
# so it is never a catalog row -- a `targets()` that listed it would advertise
# an output arbitrary input cannot reach.
WRITE_VALUES = frozenset({"verified", "emitted", "preserved", "none"})
SYNTHESIS_WRITES = frozenset({"verified", "emitted"})

# Codec crates only. `cadmpeg-ir` declares the trait and returns an empty
# catalog for CADIR; the CLI only consumes catalogs.
CODEC_SRC_GLOB = "crates/cadmpeg-codec-*/src"

TARGET_LITERAL = re.compile(r"TargetDescriptor\s*\{(.*?)\}", re.DOTALL)
TARGET_ID_FIELD = re.compile(r"\bid\s*:\s*([^,\n]+?)\s*,")
STRING_LITERAL = re.compile(r'"([^"]*)"')
PINNED_CALL = re.compile(r"^[A-Za-z0-9_]+::([A-Za-z0-9_]+)\.pinned\(\)$")
PINNED_ARM = re.compile(r'Self::([A-Za-z0-9_]+)\s*=>\s*"([^"]+)"')


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
# Write-target catalogs, parsed out of the codec crates.
# --------------------------------------------------------------------------


def parse_target_catalogs(root: Path, failures: list[str]) -> dict[str, set[str]]:
    """Return ``{format: {target id}}`` from the ``TargetDescriptor`` tables.

    An ``id:`` expression this parser cannot resolve is a failure. A silent
    skip would make the subset rule vacuous for that catalog.
    """
    catalogs: dict[str, set[str]] = {}
    for src in sorted(root.glob(CODEC_SRC_GLOB)):
        for rs in sorted(src.rglob("*.rs")):
            text = rs.read_text(encoding="utf-8")
            if "TargetDescriptor {" not in text:
                continue
            arms = dict(PINNED_ARM.findall(text))
            rel = rs.relative_to(root).as_posix()
            for body in TARGET_LITERAL.findall(text):
                field = TARGET_ID_FIELD.search(body)
                if field is None:
                    failures.append(f"{rel}: TargetDescriptor literal has no id field")
                    continue
                expr = field.group(1).strip()
                resolved = _resolve_target_id(expr, arms)
                if resolved is None:
                    failures.append(f"{rel}: cannot resolve TargetDescriptor id expression {expr!r}")
                    continue
                if ":" not in resolved:
                    failures.append(f"{rel}: target id {resolved!r} is not <format>:<name>")
                    continue
                catalogs.setdefault(resolved.split(":", 1)[0], set()).add(resolved)
    return catalogs


def _resolve_target_id(expr: str, arms: dict[str, str]) -> str | None:
    literal = STRING_LITERAL.fullmatch(expr)
    if literal is not None:
        return literal.group(1)
    call = PINNED_CALL.match(expr)
    if call is not None:
        return arms.get(call.group(1))
    return None


# --------------------------------------------------------------------------
# Row rules.
# --------------------------------------------------------------------------


def check_row(row: object, index: int, known: set[str], root: Path, failures: list[str]):
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

    if "grammar" in row and (not isinstance(row["grammar"], str) or not row["grammar"].strip()):
        failures.append(f"{label}: grammar must be a non-empty string")

    reason = row.get("reason")
    if "reason" in row and (not isinstance(reason, str) or not reason.strip()):
        failures.append(f"{label}: reason must be a non-empty string")
        reason = None

    fixtures = check_fixtures(label, row.get("fixtures"), root, failures)

    # Fixture gating. A score is a claim about decoded files; with no file the
    # claim has no evidence, and `detected` is the honest cell.
    if read in READ_SCORES and not fixtures:
        failures.append(f"{label}: read {read} with no fixtures; a fixture-less row cannot claim above detected")

    if read == "refused" and not (isinstance(reason, str) and reason.strip()):
        failures.append(f"{label}: read refused requires a reason")
    if "fixtures" in row and not fixtures and not (isinstance(reason, str) and reason.strip()):
        failures.append(f"{label}: no fixtures requires a reason")

    return dialect_id, read, write


def check_fixtures(label: str, fixtures: object, root: Path, failures: list[str]) -> list[str]:
    """Validate the ``fixtures`` list and return the paths that parsed."""
    if fixtures is None:
        return []
    if not isinstance(fixtures, list):
        failures.append(f"{label}: fixtures must be a list of repo-relative paths")
        return []
    paths: list[str] = []
    for entry in fixtures:
        if not isinstance(entry, str) or not entry.strip():
            failures.append(f"{label}: fixture entry must be a non-empty string")
            continue
        path = Path(entry)
        if path.is_absolute() or ".." in path.parts:
            failures.append(f"{label}: fixture must be a repo-relative path: {entry}")
            continue
        if not (root / path).is_file():
            failures.append(f"{label}: fixture file not found: {entry}")
            continue
        paths.append(entry)
    return paths


def check_totality(known: set[str], covered: Counter[str], failures: list[str]) -> None:
    """Both directions: no identity row uncovered, no dialect covered twice."""
    for dialect_id in sorted(known - set(covered)):
        failures.append(f"{dialect_id}: identity row has no support row")
    for dialect_id, count in sorted(covered.items()):
        if count > 1:
            failures.append(f"{dialect_id}: {count} support rows; expected one")


def check_target_subset(
    catalogs: dict[str, set[str]],
    known: set[str],
    writes: dict[str, object],
    failures: list[str],
) -> None:
    """Write-target catalogs are a subset of their format's identity rows.

    Subset and not equality: read-side and residual identity rows are not
    write targets. Two further rules make it load-bearing, one in each
    direction. An exported target must be declared writable -- not
    ``write = "none"``. And a ``preserved`` row must NOT be exported: the
    two write capabilities are distinct (design section 8.2), and a
    preservation-only dialect in a synthesis catalog would advertise an
    output that arbitrary input cannot reach.
    """
    by_format: dict[str, set[str]] = {}
    for dialect_id in known:
        by_format.setdefault(dialect_id.split(":", 1)[0], set()).add(dialect_id)
    exported = {target for ids in catalogs.values() for target in ids}
    for fmt in sorted(catalogs):
        rows = by_format.get(fmt)
        if rows is None:
            failures.append(f"{fmt}: write-target catalog for a format with no identity rows")
            continue
        for target in sorted(catalogs[fmt] - rows):
            failures.append(f"{target}: write target is not an identity row of format {fmt}")
        for target in sorted(catalogs[fmt] & rows):
            if writes.get(target) == "none":
                failures.append(f"{target}: exported as a write target but the support row says write = \"none\"")
            elif writes.get(target) == "preserved":
                failures.append(
                    f"{target}: write = \"preserved\" but the encoder exports it as a synthesis "
                    "target; preservation is input-conditioned and is never a targets() row"
                )
    # The converse of the subset rule, scoped to formats that have a catalog at
    # all. A synthesis claim on a format whose encoder enumerates its outputs
    # must appear in that enumeration, or the two halves have drifted. Formats
    # with no catalog are exempt: an embedded-kernel layer is synthesized by its
    # host's writer and is never that host's target (design section 8.3).
    for dialect_id in sorted(writes):
        fmt = dialect_id.split(":", 1)[0]
        if fmt not in catalogs:
            continue
        if writes[dialect_id] in SYNTHESIS_WRITES and dialect_id not in exported:
            failures.append(
                f"{dialect_id}: write = \"{writes[dialect_id]}\" claims synthesis, but "
                f"the {fmt} targets() catalog does not export it"
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


def check_snapshot_dialects(
    root: Path,
    fixtures_by_dialect: dict[str, list[str]],
    reads: dict[str, object],
    failures: list[str],
) -> int:
    """A fixture listed under a dialect must decode to that dialect.

    The oracle is the codec's own checked-in golden snapshot, which pins the
    dialect id the decoder emitted. This is the Python half of design section
    5's "decode each fixture, read back the emitted dialect id": the Rust half
    is the golden harness itself, which re-decodes every frozen fixture and
    compares it with the snapshot on every test run.

    Three rules fire here. A listed fixture whose snapshot pins a different id
    is a failure. A row claiming a read score must have at least one fixture
    that is pinned at all -- otherwise an arbitrary file on disk would satisfy
    the fixture gate without any decoder ever confirming its dialect. And the
    listing is complete in the other direction too: a fixture a golden suite
    already pins to a dialect must appear under that dialect's row, so the
    evidence column cannot quietly fall behind the suites that produce it.
    """
    pinned = snapshot_dialects(root)
    checked = 0
    for dialect_id, paths in sorted(fixtures_by_dialect.items()):
        confirmed = 0
        for path in paths:
            emitted = pinned.get(path)
            if emitted is None:
                continue
            if dialect_id in emitted:
                confirmed += 1
                checked += 1
            else:
                failures.append(
                    f"{dialect_id}: fixture {path} decodes to {sorted(emitted)}, not {dialect_id}"
                )
        if reads.get(dialect_id) in READ_SCORES and confirmed == 0:
            failures.append(
                f"{dialect_id}: read {reads[dialect_id]} with no fixture confirmed by a golden "
                "snapshot; a score needs a decoder that reads this id back"
            )
    for path, emitted in sorted(pinned.items()):
        for dialect_id in sorted(emitted):
            if dialect_id in fixtures_by_dialect and path not in fixtures_by_dialect[dialect_id]:
                failures.append(
                    f"{dialect_id}: a golden snapshot pins {path} to this dialect, "
                    "but the support row does not list it"
                )
    return checked


# --------------------------------------------------------------------------
# Driver.
# --------------------------------------------------------------------------


def check(root: Path) -> tuple[list[str], str]:
    """Check the capability registry under ``root``. Returns failures + summary."""
    failures: list[str] = []

    identity = _load(root / IDENTITY_REL, IDENTITY_REL.as_posix(), failures)
    support = _load(root / SUPPORT_REL, SUPPORT_REL.as_posix(), failures)
    if identity is None or support is None:
        return failures, ""

    known = {
        row["id"]
        for row in identity.get("dialect", [])
        if _is_table(row) and isinstance(row.get("id"), str)
    }
    if not known:
        failures.append(f"{IDENTITY_REL.as_posix()}: no identity rows to support")
        return failures, ""

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
    fixtures_by_dialect: dict[str, list[str]] = {}
    tally: Counter[str] = Counter()
    fixture_total = 0
    for index, row in enumerate(rows):
        dialect_id, read, write = check_row(row, index, known, root, failures)
        if dialect_id is None:
            continue
        covered[dialect_id] += 1
        writes[dialect_id] = write
        reads[dialect_id] = read
        listed = row.get("fixtures")
        if isinstance(listed, list):
            paths = [e for e in listed if isinstance(e, str)]
            fixtures_by_dialect[dialect_id] = paths
            fixture_total += len(paths)
        if isinstance(read, str):
            tally[read] += 1

    check_totality(known, covered, failures)
    catalogs = parse_target_catalogs(root, failures)
    check_target_subset(catalogs, known, writes, failures)
    verified = check_snapshot_dialects(root, fixtures_by_dialect, reads, failures)

    scored = sum(n for value, n in tally.items() if value in READ_SCORES)
    targets = sum(len(ids) for ids in catalogs.values())
    summary = (
        f"dialect-support: ok ({len(rows)} rows covering {len(known)} identity rows; "
        f"{scored} scored, {tally['detected']} detected, {tally['refused']} refused, "
        f"{tally['unclassified-recovered']} unclassified-recovered; "
        f"{fixture_total} fixtures, {verified} confirmed against golden decode snapshots; "
        f"{targets} write targets across {len(catalogs)} catalogs, "
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
