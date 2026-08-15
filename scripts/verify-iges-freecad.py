#!/usr/bin/env python3
"""Verify generated IGES files with FreeCAD's native importer.

Run this script with FreeCADCmd, for example::

    CADMPEG_IGES_INPUT_DIR=out \
        CADMPEG_IGES_REQUIRE_SHAPE=1 \
        CADMPEG_IGES_REPORT=out/freecad-report.json \
        FreeCADCmd scripts/verify-iges-freecad.py

The check is intentionally independent of cadmpeg. It imports each file in a
fresh document, rejects empty imports unless
``CADMPEG_IGES_ALLOW_EMPTY=1`` is specified, and rejects every imported object
whose Shape is null or invalid. FreeCAD's ``Import`` module may not expose
every presentation-only IGES record as an object; requiring a shape is
therefore a separate assertion for generated geometry profiles.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

sys.dont_write_bytecode = True

import FreeCAD
import Import


def verify(path: Path, require_shape: bool, allow_empty: bool) -> tuple[int, int, int, int, int]:
    document = FreeCAD.newDocument("cadmpeg_iges_verify")
    try:
        Import.insert(str(path), document.Name)
        objects = list(document.Objects)
        if not objects and not allow_empty:
            raise ValueError("import produced no objects")

        shape_objects = [obj for obj in objects if hasattr(obj, "Shape")]
        invalid = [
            obj
            for obj in shape_objects
            if obj.Shape.isNull() or not obj.Shape.isValid()
        ]
        if invalid:
            names = ", ".join(obj.Name for obj in invalid)
            raise ValueError(f"invalid shapes: {names}")
        if require_shape and not shape_objects:
            raise ValueError("import produced no shape-bearing objects")

        shapes = [obj.Shape for obj in shape_objects]
        return (
            len(objects),
            len(shape_objects),
            len(shape_objects) - len(invalid),
            sum(len(shape.Solids) for shape in shapes),
            sum(len(shape.Faces) for shape in shapes),
        )
    finally:
        FreeCAD.closeDocument(document.Name)


def main() -> None:
    input_directory = Path(os.environ["CADMPEG_IGES_INPUT_DIR"])
    require_shape = os.environ.get("CADMPEG_IGES_REQUIRE_SHAPE") == "1"
    allow_empty = os.environ.get("CADMPEG_IGES_ALLOW_EMPTY") == "1"
    files = sorted(
        (
            path
            for path in input_directory.iterdir()
            if path.is_file() and path.suffix.lower() in {".igs", ".iges"}
        ),
        key=lambda path: path.name,
    )
    if not files:
        print(f"no IGES files found in {input_directory}", file=sys.stderr)
        raise SystemExit(1)

    results = []
    failures = []
    for path in files:
        try:
            objects, shapes, valid, solids, faces = verify(
                path, require_shape, allow_empty
            )
        except Exception as error:  # FreeCAD modules raise Python and OCC exceptions.
            failures.append({"filename": path.name, "error": str(error)})
            print(f"FAIL {path}: {error}", file=sys.stderr)
        else:
            results.append(
                {
                    "filename": path.name,
                    "objects": objects,
                    "shapes": shapes,
                    "valid": valid,
                    "solids": solids,
                    "faces": faces,
                }
            )
    report = {"failures": failures, "files": results}
    report_json = json.dumps(report, indent=2, sort_keys=True) + "\n"
    report_path = os.environ.get("CADMPEG_IGES_REPORT")
    if report_path:
        Path(report_path).write_text(report_json, encoding="utf-8")
    print(report_json, end="", flush=True)
    if failures:
        raise SystemExit(1)


main()
