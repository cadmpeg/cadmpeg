#!/usr/bin/env python3
"""Verify generated .step/.stp files with Gmsh's native STEP importer."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

try:
    import gmsh
except ImportError as error:
    raise SystemExit("Gmsh's Python package is required") from error


def verify(path: Path) -> tuple[int, int]:
    if path.suffix.lower() not in {".step", ".stp"}:
        raise ValueError("Gmsh selects STEP by the .step or .stp suffix")
    gmsh.initialize()
    gmsh.option.setNumber("General.Terminal", 0)
    try:
        imported = gmsh.model.occ.importShapes(str(path))
        gmsh.model.occ.synchronize()
        surfaces = gmsh.model.getEntities(2)
        if not imported or not surfaces:
            raise ValueError("no shape with surfaces was imported")
        return len(imported), len(surfaces)
    finally:
        gmsh.finalize()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", nargs="+", type=Path)
    args = parser.parse_args()

    failed = False
    for path in args.files:
        try:
            shapes, surfaces = verify(path)
        except Exception as error:
            failed = True
            print(f"FAIL {path}: {error}", file=sys.stderr)
        else:
            print(f"PASS {path}: shapes={shapes} surfaces={surfaces}")
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
