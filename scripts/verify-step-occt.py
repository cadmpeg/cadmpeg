#!/usr/bin/env python3
"""Verify generated STEP exchange structures with Open Cascade.

This is an optional independent-reader check. Install a Python distribution of
Open Cascade that exposes the ``OCP`` package, then pass one or more generated
Part 21 files. ``--require-shape`` additionally requires at least one root to
transfer into a TopoDS shape.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

try:
    from OCP.IFSelect import IFSelect_RetDone
    from OCP.Interface import Interface_CheckOK
    from OCP.STEPControl import STEPControl_Reader
except ImportError as error:
    raise SystemExit("Open Cascade Python package 'OCP' is required") from error


def verify(path: Path, require_shape: bool) -> tuple[int, int]:
    reader = STEPControl_Reader()
    status = reader.ReadFile(str(path))
    if status != IFSelect_RetDone:
        raise ValueError(f"reader returned {status}")
    check_status = reader.WS().ModelCheckList().Status()
    if check_status != Interface_CheckOK:
        raise ValueError(f"schema check returned {check_status}")
    roots = reader.NbRootsForTransfer()
    transferred = reader.TransferRoots() if roots else 0
    shapes = reader.NbShapes()
    if require_shape and (transferred == 0 or shapes == 0):
        raise ValueError("no root transferred to a TopoDS shape")
    return roots, shapes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--require-shape",
        action="store_true",
        help="require each exchange structure to transfer at least one shape",
    )
    parser.add_argument("files", nargs="+", type=Path)
    args = parser.parse_args()

    failed = False
    for path in args.files:
        try:
            roots, shapes = verify(path, args.require_shape)
        except Exception as error:  # OCCT bindings raise Standard_Failure as RuntimeError.
            failed = True
            print(f"FAIL {path}: {error}", file=sys.stderr)
        else:
            print(f"PASS {path}: roots={roots} shapes={shapes}")
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
