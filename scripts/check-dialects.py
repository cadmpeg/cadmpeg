#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the dialect identity registry, ``docs/dialects.toml``.

The registry pins format-identity names: an id chosen once is an id forever.
This checker is its oracle for registry-internal rules -- schema, id grammar,
witness form, supersession, and generated codec-id constants.

Rendered support tables are checked by ``scripts/render-format-support.py
--check``, which regenerates every published table from this registry and
``docs/dialect-support.toml`` and compares it byte for byte.

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
from typing import NamedTuple

ROOT = Path(__file__).resolve().parent.parent
REGISTRY_REL = Path("docs") / "dialects.toml"
ID_CONFORMANCE_REL = Path("docs") / "dialect-id-conformance.toml"
SELF_TEST_REL = Path("scripts") / "test_check_dialects.py"


class GeneratedIdOwner(NamedTuple):
    """One format's generated Rust module and constant visibility."""

    path: Path
    visibility: str
    format_visibility: str = "pub(crate)"


# This map owns implementation placement, not identity. Id values and constant
# names come only from docs/dialects.toml. Each format with dialect rows has
# exactly one output module; CADIR has no rows and therefore no output.
GENERATED_ID_OWNERS = {
    "acis": GeneratedIdOwner(
        Path("crates/cadmpeg-asm/src/dialect/registry_ids.rs"), "pub", "pub"
    ),
    "catia": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-catia/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
    "creo": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-creo/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
    "f3d": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-f3d/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
    "fcstd": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-freecad/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
    "iges": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-iges/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
    "inventor": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-inventor/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
    "nx": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-nx/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
    "parasolid": GeneratedIdOwner(
        Path("crates/cadmpeg-parasolid/src/registry_ids.rs"), "pub(crate)", "pub"
    ),
    "rhino": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-rhino/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
    "sat": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-sat/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
    "sldprt": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-sldprt/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
    "step": GeneratedIdOwner(
        Path("crates/cadmpeg-codec-step/src/dialect/registry_ids.rs"), "pub(crate)"
    ),
}

# The schema keys a `[[dialect]]` row may carry. Anything else is a typo or an
# unreviewed schema extension; both are failures.
ROW_KEYS = frozenset(
    {
        "id",
        "title",
        "discriminants",
        "witness",
        "seam",
        "supersedes",
        "unknown_kind",
    }
)
REQUIRED_ROW_KEYS = ("id", "title", "discriminants", "witness")

# `supersedes` is admitted only where the checker consumes it. IGES is the one
# consumer today.
SUPERSEDES_FORMATS = frozenset({"iges"})

WITNESS_PREFIXES = ("spec:", "corpus:", "code:")
UNKNOWN_KINDS = frozenset(
    {"detect-unreachable", "recovered-residual", "refused-residual"}
)

FORMAT_ID = re.compile(r"[a-z0-9]+")
FORMAT_KEYS = frozenset({"complete", "aliases"})
# Dots are legal in a dialect name: `iges:5.3-fixed-ascii`.
DIALECT_NAME = re.compile(r"[a-z0-9.-]+")


def valid_dialect_name(name: str) -> bool:
    """Return whether ``name`` has the canonical dialect-name grammar."""
    return bool(
        DIALECT_NAME.fullmatch(name)
        and not name.startswith("-")
        and not name.endswith("-")
    )


def valid_dialect_id(raw_id: str) -> bool:
    """Return whether ``raw_id`` has the canonical Rust/registry grammar."""
    if raw_id.count(":") != 1:
        return False
    head, name = raw_id.split(":")
    return bool(FORMAT_ID.fullmatch(head) and valid_dialect_name(name))


def check_id_conformance(root: Path, failures: list[str]) -> None:
    """Require the checker to satisfy the corpus shared with ``DialectId``."""
    path = root / ID_CONFORMANCE_REL
    if not path.is_file():
        failures.append(f"{ID_CONFORMANCE_REL.as_posix()}: not found")
        return
    try:
        with path.open("rb") as handle:
            cases = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as err:
        failures.append(f"{ID_CONFORMANCE_REL.as_posix()}: {err}")
        return
    valid = cases.get("valid")
    invalid = cases.get("invalid")
    if (
        not isinstance(valid, list)
        or not valid
        or not all(isinstance(case, str) for case in valid)
    ):
        failures.append(
            f"{ID_CONFORMANCE_REL.as_posix()}: valid must be a non-empty string list"
        )
        return
    if (
        not isinstance(invalid, list)
        or not invalid
        or not all(isinstance(case, str) for case in invalid)
    ):
        failures.append(
            f"{ID_CONFORMANCE_REL.as_posix()}: invalid must be a non-empty string list"
        )
        return
    for case in valid:
        if not valid_dialect_id(case):
            failures.append(f"{ID_CONFORMANCE_REL.as_posix()}: valid case rejected: {case!r}")
    for case in invalid:
        if valid_dialect_id(case):
            failures.append(f"{ID_CONFORMANCE_REL.as_posix()}: invalid case accepted: {case!r}")


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
    names: dict[str, str] = {}
    for name, body in formats.items():
        if not FORMAT_ID.fullmatch(name):
            failures.append(f"format {name}: id must match [a-z0-9]+")
            continue
        if not _is_table(body):
            failures.append(f"format {name}: not a table")
            continue
        for key in sorted(set(body) - FORMAT_KEYS):
            failures.append(f"format {name}: unknown key {key}")
        complete = body.get("complete")
        if complete is None:
            failures.append(f"format {name}: missing complete")
        elif not isinstance(complete, bool):
            failures.append(f"format {name}: complete must be a boolean")
        aliases = body.get("aliases", [])
        if not isinstance(aliases, list) or not all(isinstance(alias, str) for alias in aliases):
            failures.append(f"format {name}: aliases must be a list of strings")
            aliases = []
        previous = names.get(name)
        if previous is not None:
            failures.append(f"format {name}: canonical id duplicates alias owned by format {previous}")
        else:
            names[name] = name
        for alias in aliases:
            if not FORMAT_ID.fullmatch(alias):
                failures.append(f"format {name}: alias {alias!r} must match [a-z0-9]+")
                continue
            previous = names.get(alias)
            if previous is not None:
                failures.append(f"format {name}: alias {alias!r} duplicates a format name owned by {previous}")
            else:
                names[alias] = name
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


def check_supersedes_shape(label: str, fmt: str, row: dict, failures: list[str]) -> None:
    """Validate one supersession list, except cross-row id resolution."""
    if "supersedes" not in row:
        return
    if fmt not in SUPERSEDES_FORMATS:
        failures.append(
            f"{label}: supersedes is admitted only on {'/'.join(sorted(SUPERSEDES_FORMATS))} rows"
        )
        return
    entries = row["supersedes"]
    if not isinstance(entries, list) or not all(isinstance(entry, str) for entry in entries):
        failures.append(f"{label}: supersedes must be a list of strings")


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
            if not valid_dialect_name(name):
                failures.append(
                    f"{label}: name must be lowercase [a-z0-9.-]+ and must not start or end with a hyphen"
                )
            else:
                dialect_id = raw_id

    title = row.get("title")
    if "title" in row and (not isinstance(title, str) or not title.strip()):
        failures.append(f"{label}: title must be a non-empty string")

    if "seam" in row and not isinstance(row["seam"], str):
        failures.append(f"{label}: seam must be a string")

    is_unknown = isinstance(raw_id, str) and raw_id.endswith(":unknown")
    unknown_kind = row.get("unknown_kind")
    if is_unknown and unknown_kind is None:
        failures.append(f"{label}: unknown row must state unknown_kind")
    elif not is_unknown and unknown_kind is not None:
        failures.append(f"{label}: unknown_kind is allowed only on an :unknown row")
    elif unknown_kind is not None and unknown_kind not in UNKNOWN_KINDS:
        allowed = ", ".join(sorted(UNKNOWN_KINDS))
        failures.append(f"{label}: unknown_kind must be one of {allowed}")

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

    check_supersedes_shape(label, fmt or "", row, failures)
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


def check(root: Path, *, generated: bool = True) -> tuple[list[str], str]:
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

    check_id_conformance(root, failures)

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
    for fmt, body in formats.items():
        if body.get("complete") is False and f"{fmt}:unknown" not in seen:
            failures.append(f"format {fmt}: complete = false requires {fmt}:unknown")
    if generated and (root / "Cargo.toml").is_file() and not failures:
        failures.extend(check_generated_id_modules(root, rows))

    counts = ", ".join(f"{fmt} {n}" for fmt, n in sorted(per_format.items()))
    summary = (
        f"dialects: ok ({len(rows)} rows across {len(formats)} formats: {counts}; "
        f"{code_debt} rows on code: witnesses awaiting a spec/corpus upgrade)"
    )
    return failures, summary


def rust_constant_name(dialect_id: str) -> str:
    """Derive one stable Rust constant name from a registry id."""
    fmt, local = dialect_id.split(":", 1)
    return f"{fmt}_{re.sub(r'[^a-z0-9]', '_', local)}".upper()


def render_generated_id_module(
    fmt: str,
    rows: list[dict],
    visibility: str,
    format_visibility: str,
) -> tuple[str | None, list[str]]:
    """Render one format's constants and report generated-name collisions."""
    failures: list[str] = []
    constants: list[tuple[str, str, bool]] = []
    names: dict[str, str] = {}
    for row in rows:
        dialect_id = row.get("id")
        if not isinstance(dialect_id, str) or not dialect_id.startswith(f"{fmt}:"):
            continue
        name = rust_constant_name(dialect_id)
        if previous := names.get(name):
            failures.append(
                f"{dialect_id}: generated constant {name} collides with {previous}"
            )
        else:
            names[name] = dialect_id
            constants.append(
                (
                    name,
                    dialect_id,
                    row.get("unknown_kind") == "detect-unreachable",
                )
            )
    if failures:
        return None, failures
    lines = [
        "// SPDX-License-Identifier: Apache-2.0",
        "// Generated by scripts/check-dialects.py from docs/dialects.toml.",
        "// Do not edit this file directly.",
        "",
        "/// Registry-owned format namespace.",
        f'{format_visibility} const FORMAT: &str = "{fmt}";',
        "",
    ]
    for name, dialect_id, detect_unreachable in constants:
        lines.append(f"/// Registry-owned dialect id `{dialect_id}`.")
        if detect_unreachable:
            lines.extend(
                [
                    "// Container detection cannot produce this registry row.",
                    "#[allow(dead_code)]",
                ]
            )
        lines.append(
            f'{visibility} const {name}: DialectId = DialectId::pinned("{dialect_id}");'
        )
    return "\n".join(lines) + "\n", []


def generated_id_modules(
    rows: list[dict],
    owners: dict[str, GeneratedIdOwner] = GENERATED_ID_OWNERS,
) -> tuple[dict[Path, str], list[str]]:
    """Render every format module and enforce one implementation owner."""
    row_formats = {
        dialect_id.split(":", 1)[0]
        for row in rows
        if _is_table(row)
        and isinstance((dialect_id := row.get("id")), str)
        and dialect_id.count(":") == 1
    }
    failures = [
        f"format {fmt}: no generated dialect-id owner"
        for fmt in sorted(row_formats - owners.keys())
    ]
    failures.extend(
        f"format {fmt}: generated dialect-id owner has no registry rows"
        for fmt in sorted(owners.keys() - row_formats)
    )
    outputs: dict[Path, str] = {}
    for fmt in sorted(row_formats & owners.keys()):
        owner = owners[fmt]
        rendered, render_failures = render_generated_id_module(
            fmt, rows, owner.visibility, owner.format_visibility
        )
        failures.extend(render_failures)
        if rendered is not None:
            outputs[owner.path] = rendered
    return outputs, failures


def check_generated_id_modules(
    root: Path,
    rows: list[dict],
    owners: dict[str, GeneratedIdOwner] = GENERATED_ID_OWNERS,
) -> list[str]:
    """Require checked-in codec constants to equal registry-derived output."""
    outputs, failures = generated_id_modules(rows, owners)
    for rel, expected in outputs.items():
        path = root / rel
        try:
            actual = path.read_text(encoding="utf-8")
        except OSError:
            actual = None
        if actual != expected:
            failures.append(
                f"{rel.as_posix()}: generated dialect ids differ; run "
                "python3 scripts/check-dialects.py --write-generated"
            )
    return failures


def write_generated_id_modules(root: Path, rows: list[dict]) -> list[str]:
    """Write every registry-derived codec constant module."""
    outputs, failures = generated_id_modules(rows)
    if failures:
        return failures
    for rel, content in outputs.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    return []


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
    parser.add_argument(
        "--write-generated",
        action="store_true",
        help="regenerate per-format Rust dialect-id constants",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    root = args.root.resolve()
    failures, summary = check(root, generated=not args.write_generated)
    if not failures and args.write_generated:
        with (root / REGISTRY_REL).open("rb") as handle:
            rows = tomllib.load(handle).get("dialect", [])
        failures.extend(write_generated_id_modules(root, rows))
        if not failures:
            failures, summary = check(root)
    if failures:
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
        return 1
    print(summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
