#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Render every published capability table from the two dialect registries.

The registries are the only input: ``docs/dialects.toml`` (identity) and
``docs/dialect-support.toml`` (capability). This script owns marker-delimited
regions inside ``docs/format-support.md``, the root ``README.md``, each codec
crate's ``README.md``, and each codec crate's ``src/lib.rs`` doc header. Prose
outside a region is hand-written and is never touched.

A region begins with ``<!-- generated: <marker> -->`` and ends with
``<!-- /generated: <marker> -->``, each on its own line. In a ``lib.rs`` doc
header both marker lines carry the ``//! `` prefix.

``--check`` re-renders every target and compares it to the committed file
**byte for byte**. There is no whitespace normalization: a rendered table is
either the current render or it is stale. This retires the honour-system proof
criterion "This document matches the code and tests" for the score tables.

Run ``--self-test`` to execute ``scripts/test_render_format_support.py``.

Exit codes: 0 clean, 1 a committed file is stale (``--check``), 2 a structural
error in the registries, the target map, or a target file.
"""

from __future__ import annotations

import argparse
import difflib
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
IDENTITY_REL = Path("docs") / "dialects.toml"
SUPPORT_REL = Path("docs") / "dialect-support.toml"
LADDER_REL = Path("docs") / "format-support.md"
README_REL = Path("README.md")
SELF_TEST_REL = Path("scripts") / "test_render_format_support.py"

# Where the ladder document lives for a reader outside the repository. The
# crate READMEs and the rustdoc headers ship to crates.io and docs.rs, so their
# links cannot be repository-relative.
BLOB = "https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md"

READ_LEVEL = re.compile(r"^L([0-9])$")
WITNESSED_PREFIXES = ("spec:", "corpus:")
# The residual row of a `complete = false` format. It is not an enumerated
# dialect, so it is outside every breadth denominator.
RESIDUAL_NAME = "unknown"

MARKER_LADDER_TABLE = "ladder-table"
MARKER_README_LINES = "capability-lines"
MARKER_CAPABILITY = "capability"


@dataclass(frozen=True)
class Target:
    """A format's published surfaces. File paths are not registry data."""

    name: str
    crate: str | None = None

    @property
    def registry_only(self) -> bool:
        return self.crate is None


# Every format declared in `docs/dialects.toml` must appear here, and every key
# here must be a declared format. A new format therefore fails this renderer
# until its published surfaces are named. Order is the published order.
TARGETS: dict[str, Target] = {
    "fcstd": Target("FreeCAD `.FCStd`", "cadmpeg-codec-freecad"),
    "f3d": Target("Autodesk Fusion `.f3d`", "cadmpeg-codec-f3d"),
    "inventor": Target("Autodesk Inventor `.ipt`/`.iam`", "cadmpeg-codec-inventor"),
    "sldprt": Target("SolidWorks `.sldprt`", "cadmpeg-codec-sldprt"),
    "rhino": Target("Rhino `.3dm`", "cadmpeg-codec-rhino"),
    "nx": Target("Siemens NX `.prt`", "cadmpeg-codec-nx"),
    "catia": Target("CATIA V5 `.CATPart`", "cadmpeg-codec-catia"),
    "creo": Target("Creo Parametric `.prt`", "cadmpeg-codec-creo"),
    "step": Target("STEP Part 21", "cadmpeg-codec-step"),
    "iges": Target("IGES", "cadmpeg-codec-iges"),
    "sat": Target("ASM/ACIS bare streams", "cadmpeg-codec-sat"),
    "acis": Target("ACIS save formats"),
    "parasolid": Target("Parasolid schemas"),
}


class RenderError(Exception):
    """A structural fault that no re-render can fix. Exit code 2."""


# --------------------------------------------------------------------------
# Registries
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Row:
    """One identity row joined to its capability row."""

    dialect: str
    fmt: str
    name: str
    witness: str
    read: str
    write: str
    fixtures: int

    @property
    def level(self) -> int | None:
        match = READ_LEVEL.match(self.read)
        return int(match.group(1)) if match else None

    @property
    def witnessed(self) -> bool:
        """In a breadth denominator: spec- or corpus-witnessed, not residual."""
        if self.name == RESIDUAL_NAME:
            return False
        return self.witness.startswith(WITNESSED_PREFIXES)


@dataclass(frozen=True)
class Format:
    """One format's rows and the headline they produce."""

    fmt: str
    complete: bool
    rows: tuple[Row, ...]

    @property
    def depth(self) -> str:
        levels = [row.level for row in self.rows if row.level is not None]
        return f"L{max(levels)}" if levels else "none"

    @property
    def breadth(self) -> str:
        denominator = [row for row in self.rows if row.witnessed]
        if not denominator:
            return "n/a"
        numerator = sum(1 for row in denominator if (row.level or 0) >= 1)
        bound = "" if self.complete else ">="
        return f"{numerator} of {bound}{len(denominator)}"

    @property
    def headline(self) -> str:
        return f"depth {self.depth}, breadth {self.breadth}"

    @property
    def refusals(self) -> int:
        return sum(1 for row in self.rows if row.read == "refused")


def _load_toml(path: Path) -> dict:
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except FileNotFoundError as exc:
        raise RenderError(f"{path}: not found") from exc
    except tomllib.TOMLDecodeError as exc:
        raise RenderError(f"{path}: {exc}") from exc


def load_formats(root: Path) -> dict[str, Format]:
    """Join the two registries into per-format row sets."""
    identity = _load_toml(root / IDENTITY_REL)
    capability = _load_toml(root / SUPPORT_REL)

    declared = identity.get("format")
    if not isinstance(declared, dict) or not declared:
        raise RenderError(f"{IDENTITY_REL}: no [format.<id>] entries")
    dialects = identity.get("dialect")
    if not isinstance(dialects, list) or not dialects:
        raise RenderError(f"{IDENTITY_REL}: no [[dialect]] rows")
    supports = capability.get("support")
    if not isinstance(supports, list) or not supports:
        raise RenderError(f"{SUPPORT_REL}: no [[support]] rows")

    by_dialect: dict[str, dict] = {}
    for support in supports:
        dialect = support.get("dialect")
        if not isinstance(dialect, str):
            raise RenderError(f"{SUPPORT_REL}: a [[support]] row has no dialect id")
        if dialect in by_dialect:
            raise RenderError(f"{SUPPORT_REL}: duplicate support row for {dialect}")
        by_dialect[dialect] = support

    grouped: dict[str, list[Row]] = {fmt: [] for fmt in declared}
    for entry in dialects:
        dialect = entry.get("id")
        if not isinstance(dialect, str) or ":" not in dialect:
            raise RenderError(f"{IDENTITY_REL}: bad dialect id {dialect!r}")
        fmt, _, name = dialect.partition(":")
        if fmt not in declared:
            raise RenderError(f"{IDENTITY_REL}: {dialect} names undeclared format {fmt}")
        support = by_dialect.pop(dialect, None)
        if support is None:
            raise RenderError(f"{SUPPORT_REL}: no support row for {dialect}")
        grouped[fmt].append(
            Row(
                dialect=dialect,
                fmt=fmt,
                name=name,
                witness=entry.get("witness", ""),
                read=support.get("read", ""),
                write=support.get("write", ""),
                fixtures=len(support.get("fixtures", [])),
            )
        )
    if by_dialect:
        orphans = ", ".join(sorted(by_dialect))
        raise RenderError(f"{SUPPORT_REL}: support rows name no identity row: {orphans}")

    missing = sorted(set(declared) - set(TARGETS))
    if missing:
        raise RenderError(
            "formats absent from TARGETS in scripts/render-format-support.py: "
            + ", ".join(missing)
        )
    extra = sorted(set(TARGETS) - set(declared))
    if extra:
        raise RenderError(
            "TARGETS names formats absent from the registry: " + ", ".join(extra)
        )

    return {
        fmt: Format(fmt=fmt, complete=bool(declared[fmt].get("complete")), rows=tuple(rows))
        for fmt, rows in grouped.items()
    }


# --------------------------------------------------------------------------
# Region splicing
# --------------------------------------------------------------------------


def _markers(marker: str, prefix: str) -> tuple[str, str]:
    return f"{prefix}<!-- generated: {marker} -->", f"{prefix}<!-- /generated: {marker} -->"


def splice(text: str, marker: str, body: str, *, rel: Path, prefix: str = "") -> str:
    """Replace the region named by ``marker``. The marker lines survive."""
    begin, end = _markers(marker, prefix)
    lines = text.split("\n")
    starts = [i for i, line in enumerate(lines) if line == begin]
    ends = [i for i, line in enumerate(lines) if line == end]
    if len(starts) != 1:
        raise RenderError(f"{rel}: expected one {begin!r} line, found {len(starts)}")
    if len(ends) != 1:
        raise RenderError(f"{rel}: expected one {end!r} line, found {len(ends)}")
    if ends[0] < starts[0]:
        raise RenderError(f"{rel}: {end!r} precedes {begin!r}")
    replacement = body.split("\n") if body else []
    return "\n".join(lines[: starts[0] + 1] + replacement + lines[ends[0] :])


def _table(header: tuple[str, ...], rows: list[tuple[str, ...]]) -> str:
    widths = [len(cell) for cell in header]
    for row in rows:
        widths = [max(width, len(cell)) for width, cell in zip(widths, row)]

    def line(cells: tuple[str, ...]) -> str:
        return "| " + " | ".join(cell.ljust(width) for cell, width in zip(cells, widths)) + " |"

    out = [line(header), "| " + " | ".join("-" * width for width in widths) + " |"]
    out.extend(line(row) for row in rows)
    return "\n".join(out)


# --------------------------------------------------------------------------
# Bodies
# --------------------------------------------------------------------------

BREADTH_RULE = """\
Depth is the highest read level any declared dialect of the format reaches.
Breadth is the count of its witnessed identity rows at read `L1` or higher over
the count of witnessed rows. A witnessed row is one carrying a `spec:` or
`corpus:` witness; rows on `code:` witnesses are outside the denominator by the
witness rule in `docs/dialects.toml`, and each format's `<format>:unknown`
residual row is outside it because the row is a grammar residue, not an
enumerated dialect. A format whose `[format.<id>]` is `complete = false` prints
`>=` before the denominator: its rows are grammar classes, so the enumeration
is a floor. A format with no witnessed row prints `n/a`. Refusals are counted,
never excluded: refusing a dialect can only worsen a published number.

Both numbers are monotone under adding capability, and the denominator comes
from the identity registry, which changes when a vendor ships and not when
cadmpeg gains a decoder."""


def ladder_table(formats: dict[str, Format]) -> str:
    rows = [
        (
            TARGETS[fmt].name,
            formats[fmt].depth,
            formats[fmt].breadth,
            str(len(formats[fmt].rows)),
            str(formats[fmt].refusals),
        )
        for fmt in TARGETS
    ]
    header = ("Format", "Depth", "Breadth", "Identity rows", "Refused")
    return f"\n{BREADTH_RULE}\n\n{_table(header, rows)}\n"


def format_section(fmt: str, formats: dict[str, Format]) -> str:
    entry = formats[fmt]
    rows = [
        (f"`{row.dialect}`", row.read, row.write, str(row.fixtures))
        for row in entry.rows
    ]
    header = ("Dialect", "Read", "Write", "Fixtures")
    return f"\n**Ladder: {entry.headline}.**\n\n{_table(header, rows)}\n"


def readme_lines(formats: dict[str, Format], anchors: dict[str, str]) -> str:
    lines = [
        f"- **{TARGETS[fmt].name}**: {formats[fmt].headline} "
        f"([profile](docs/format-support.md#{anchors[fmt]}))"
        for fmt in TARGETS
        if not TARGETS[fmt].registry_only
    ]
    return "\n" + "\n".join(lines) + "\n"


def crate_line(fmt: str, formats: dict[str, Format], anchors: dict[str, str]) -> str:
    return f"Support: {formats[fmt].headline} ([ladder]({BLOB}#{anchors[fmt]}))."


# --------------------------------------------------------------------------
# Anchors
# --------------------------------------------------------------------------

ANCHOR_DROP = re.compile(r"[^a-z0-9 -]")


def _anchor(heading: str) -> str:
    return ANCHOR_DROP.sub("", heading.strip().lower()).replace(" ", "-")


def section_anchors(ladder: str) -> dict[str, str]:
    """Anchor of the ``##`` heading that encloses each per-format region.

    Deriving the anchor from the document its own links point into means a
    renamed heading cannot leave a dangling link behind.
    """
    anchors: dict[str, str] = {}
    heading = None
    begin = re.compile(r"^<!-- generated: dialects ([a-z0-9]+) -->$")
    for line in ladder.split("\n"):
        if line.startswith("## "):
            heading = line[3:]
        match = begin.match(line)
        if not match:
            continue
        if heading is None:
            raise RenderError(f"{LADDER_REL}: {line} precedes every '## ' heading")
        anchors[match.group(1)] = _anchor(heading)
    return anchors


# --------------------------------------------------------------------------
# Targets
# --------------------------------------------------------------------------


def render(root: Path) -> dict[Path, str]:
    """Return the full rendered text of every target file, keyed by rel path."""
    formats = load_formats(root)

    ladder_rel = LADDER_REL
    ladder = _read(root, ladder_rel)
    anchors = section_anchors(ladder)
    expected = {fmt for fmt, target in TARGETS.items() if not target.registry_only}
    if set(anchors) != expected:
        missing = ", ".join(sorted(expected - set(anchors))) or "none"
        extra = ", ".join(sorted(set(anchors) - expected)) or "none"
        raise RenderError(
            f"{ladder_rel}: per-format regions do not match the codec formats "
            f"(missing: {missing}; unexpected: {extra})"
        )

    ladder = splice(ladder, MARKER_LADDER_TABLE, ladder_table(formats), rel=ladder_rel)
    for fmt in anchors:
        ladder = splice(
            ladder, f"dialects {fmt}", format_section(fmt, formats), rel=ladder_rel
        )
    out = {ladder_rel: ladder}

    readme = _read(root, README_REL)
    out[README_REL] = splice(
        readme, MARKER_README_LINES, readme_lines(formats, anchors), rel=README_REL
    )

    for fmt, target in TARGETS.items():
        if target.crate is None:
            continue
        line = crate_line(fmt, formats, anchors)
        crate_readme = Path("crates") / target.crate / "README.md"
        out[crate_readme] = splice(
            _read(root, crate_readme), MARKER_CAPABILITY, f"\n{line}\n", rel=crate_readme
        )
        crate_lib = Path("crates") / target.crate / "src" / "lib.rs"
        out[crate_lib] = splice(
            _read(root, crate_lib),
            MARKER_CAPABILITY,
            f"//! {line}",
            rel=crate_lib,
            prefix="//! ",
        )
    return out


def _read(root: Path, rel: Path) -> str:
    path = root / rel
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise RenderError(f"{rel}: not found") from exc
    except UnicodeDecodeError as exc:
        raise RenderError(f"{rel}: not UTF-8: {exc}") from exc


# --------------------------------------------------------------------------
# Modes
# --------------------------------------------------------------------------


def check(root: Path) -> list[str]:
    """Return a unified diff per stale target. Empty means every file matches."""
    stale = []
    for rel, text in render(root).items():
        committed = _read(root, rel)
        if committed == text:
            continue
        diff = difflib.unified_diff(
            committed.splitlines(keepends=True),
            text.splitlines(keepends=True),
            fromfile=f"{rel} (committed)",
            tofile=f"{rel} (rendered)",
        )
        stale.append("".join(diff))
    return stale


def write(root: Path) -> list[Path]:
    written = []
    for rel, text in render(root).items():
        path = root / rel
        if path.read_text(encoding="utf-8") == text:
            continue
        path.write_text(text, encoding="utf-8")
        written.append(rel)
    return written


def self_test() -> int:
    import unittest

    suite = unittest.defaultTestLoader.discover(
        start_dir=str(ROOT / "scripts"),
        pattern=SELF_TEST_REL.name,
        top_level_dir=str(ROOT / "scripts"),
    )
    return 0 if unittest.TextTestRunner(verbosity=1).run(suite).wasSuccessful() else 1


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
        "--check",
        action="store_true",
        help="compare every committed target to a fresh render, byte for byte",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the renderer's own suite instead of rendering",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()

    root = args.root.resolve()
    try:
        if args.check:
            stale = check(root)
            for diff in stale:
                sys.stdout.write(diff)
            if stale:
                print(
                    f"error: {len(stale)} file(s) do not match a fresh render; "
                    "run scripts/render-format-support.py",
                    file=sys.stderr,
                )
                return 1
            print("format-support: ok (every rendered table matches the registries)")
            return 0
        written = write(root)
        for rel in written:
            print(f"rendered: {rel}")
        if not written:
            print("format-support: ok (already current)")
        return 0
    except RenderError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
