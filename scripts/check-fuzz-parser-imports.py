#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Reject focused fuzz harnesses that import parser modules.

Focused targets under ``crates/cadmpeg-fuzz/fuzz_targets/`` named for a codec
prefix must reach internals through ``::fuzz``. Codec-level container, writer,
and public decode/inspect targets are excluded. Do not fold this into
``scripts/check-public-api-ledger.py``.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXCLUDED = frozenset(
    {
        "nx_container.rs",
        "creo_container.rs",
        "catia_container.rs",
        "sldprt_container.rs",
        "rhino_container.rs",
        "iges_container.rs",
        "iges_writer.rs",
        "inventor_codec.rs",
        "step_decode.rs",
        "step_reader.rs",
        "step_writer.rs",
        "step_writer_custom.rs",
        "step_geometry_degenerate.rs",
    }
)
PREFIXES = (
    "nx_",
    "creo_",
    "catia_",
    "sldprt_",
    "rhino_",
    "iges_",
    "step_",
    "inventor_",
)

NX_MODULES = frozenset(
    {"container", "geometry", "intersection", "nurbs", "parasolid", "topology"}
)
CREO_MODULES = frozenset(
    {
        "container",
        "curve",
        "feature",
        "legacy",
        "primdata",
        "psb",
        "reference",
        "scalar",
        "surface",
        "topology",
    }
)
CATIA_MODULES = frozenset(
    {
        "catalog",
        "container",
        "families",
        "object_graph",
        "value_block",
        "wire",
        "decode",
        "analytic",
        "assemble",
        "entity_table",
        "nurbs",
        "native",
    }
)
SLDPRT_MODULES = frozenset(
    {"brep", "container", "decode", "parasolid", "pmi", "records"}
)
RHINO_MODULES = frozenset(
    {
        "brep",
        "cage",
        "chunks",
        "container",
        "curves",
        "decode",
        "hatch",
        "mesh",
        "objects",
        "polyedge",
        "subd",
        "surfaces",
    }
)
IGES_MODULES = frozenset(
    {
        "card",
        "directory",
        "entities",
        "global",
        "graph",
        "native",
        "parameter",
        "reader",
        "writer",
    }
)
STEP_MODULES = frozenset(
    {
        "archive",
        "geometry",
        "ids",
        "lex",
        "parse",
        "reader",
        "signature",
        "strings",
        "writer",
    }
)
INVENTOR_MODULES = frozenset(
    {
        "assembly",
        "container",
        "database",
        "decode",
        "design",
        "kernel",
        "property_set",
        "protein",
        "records",
        "rse",
    }
)
MODULES = {
    "nx": NX_MODULES,
    "creo": CREO_MODULES,
    "catia": CATIA_MODULES,
    "sldprt": SLDPRT_MODULES,
    "rhino": RHINO_MODULES,
    "iges": IGES_MODULES,
    "step": STEP_MODULES,
    "inventor": INVENTOR_MODULES,
}

IMPORT = re.compile(
    r"cadmpeg_codec_(nx|creo|catia|sldprt|rhino|iges|step|inventor)::(?:\{([^}]*)\}|([A-Za-z_][A-Za-z0-9_]*))"
)
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def banned_names(codec: str, fragment: str) -> list[str]:
    allowed = MODULES[codec]
    return [name for name in IDENT.findall(fragment) if name in allowed]


def strip_line_comments(text: str) -> str:
    lines = []
    for line in text.splitlines():
        stripped = line.lstrip()
        if stripped.startswith("//"):
            continue
        if "//" in line:
            line = line[: line.index("//")]
        lines.append(line)
    return "\n".join(lines)


def scan_source(text: str) -> list[str]:
    hits: list[str] = []
    for match in IMPORT.finditer(strip_line_comments(text)):
        codec = match.group(1)
        fragment = match.group(2) if match.group(2) is not None else match.group(3)
        for name in banned_names(codec, fragment):
            item = f"cadmpeg_codec_{codec}::{name}"
            if item not in hits:
                hits.append(item)
    return hits


def focused_targets(root: Path) -> list[Path]:
    directory = root / "crates" / "cadmpeg-fuzz" / "fuzz_targets"
    if not directory.is_dir():
        raise FileNotFoundError(f"missing fuzz target directory: {directory}")
    paths = sorted(
        path
        for path in directory.iterdir()
        if path.is_file()
        and path.suffix == ".rs"
        and path.name.startswith(PREFIXES)
        and path.name not in EXCLUDED
    )
    return paths


def check(root: Path) -> list[str]:
    failures: list[str] = []
    for path in focused_targets(root):
        text = path.read_text(encoding="utf-8")
        relative = path.relative_to(root).as_posix()
        for item in scan_source(text):
            failures.append(f"{relative}: imports {item}")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root",
        nargs="?",
        type=Path,
        default=ROOT,
        help="repository root (default: parent of scripts/)",
    )
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        failures = check(root)
    except FileNotFoundError as error:
        print(error, file=sys.stderr)
        return 1
    if failures:
        print("focused harnesses must import ::fuzz, not parser modules:")
        for line in failures:
            print(f"  {line}")
        return 1
    print(f"fuzz parser imports: ok ({len(focused_targets(root))} focused targets)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
