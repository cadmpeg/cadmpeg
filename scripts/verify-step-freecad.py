#!/usr/bin/env python3
"""Verify generated STEP files with FreeCAD's native importer.

Run this script with FreeCADCmd, for example::

    CADMPEG_STEP_INPUT_DIR=out \
        CADMPEG_STEP_REQUIRE_SHAPE=1 \
        CADMPEG_STEP_REPORT=out/freecad-report.json \
        FreeCADCmd scripts/verify-step-freecad.py

The check is independent of cadmpeg. It imports each file in a fresh
document, rejects empty imports unless ``CADMPEG_STEP_ALLOW_EMPTY=1`` is set,
and rejects every imported object whose Shape is null or invalid. The
importer nests leaf geometry under product containers holding the same
shapes again; solid and face counts cover each connected product once, at
its ultimate root, so containers cannot multiply the geometry.

When ``CADMPEG_STEP_MANIFEST`` names the contract suite's ``manifest.json``,
the file set must agree exactly, and the FreeCAD solid and face counts must
equal the cadmpeg re-decode counts in the matching entry. An entry carrying
``error`` needs only a successful import; its counts do not exist. Files
under ``damaged/`` keep that prefix as their manifest key.
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
    document = FreeCAD.newDocument("cadmpeg_step_verify")
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

        # The importer nests leaf geometry under product containers that hold
        # the same shapes again. Count each connected product once, at its
        # ultimate root, so containers cannot multiply the geometry.
        roots = [obj for obj in objects if not obj.InList]
        root_shapes = [obj.Shape for obj in roots if hasattr(obj, "Shape")]
        return (
            len(objects),
            len(shape_objects),
            len(shape_objects) - len(invalid),
            sum(len(shape.Solids) for shape in root_shapes),
            sum(len(shape.Faces) for shape in root_shapes),
        )
    finally:
        FreeCAD.closeDocument(document.Name)


def load_manifest(path: Path) -> dict:
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or manifest.get("version") != 1:
        raise ValueError(f"{path}: expected JSON object with version 1")
    files = manifest.get("files")
    if not isinstance(files, dict) or not all(
        isinstance(name, str) and isinstance(entry, dict)
        for name, entry in files.items()
    ):
        raise ValueError(f"{path}: files must map names to objects")
    return files


def check_manifest_counts(observed: dict, entry: dict, filename: str) -> list:
    if "error" in entry:
        return []
    errors = []
    for field in ("solids", "faces"):
        expected = entry.get(field)
        if isinstance(expected, bool) or not isinstance(expected, int):
            errors.append(f"manifest {field} for {filename} is not an integer")
        elif observed[field] != expected:
            errors.append(f"{field}: expected {expected}, observed {observed[field]}")
    unknown = set(entry) - {"solids", "faces", "error"}
    errors.extend(f"unknown manifest field: {field}" for field in sorted(unknown))
    return errors


def main() -> None:
    input_directory = Path(os.environ["CADMPEG_STEP_INPUT_DIR"])
    require_shape = os.environ.get("CADMPEG_STEP_REQUIRE_SHAPE") == "1"
    allow_empty = os.environ.get("CADMPEG_STEP_ALLOW_EMPTY") == "1"
    files = sorted(
        (
            path
            for path in input_directory.rglob("*")
            if path.is_file() and path.suffix.lower() in {".step", ".stp"}
        ),
        key=lambda path: path.relative_to(input_directory).as_posix(),
    )
    if not files:
        print(f"no STEP files found in {input_directory}", file=sys.stderr)
        raise SystemExit(1)
    manifest_path = os.environ.get("CADMPEG_STEP_MANIFEST")
    manifest = load_manifest(Path(manifest_path)) if manifest_path else None
    if manifest is not None:
        actual_names = {path.relative_to(input_directory).as_posix() for path in files}
        expected_names = set(manifest)
        missing = sorted(actual_names - expected_names)
        extra = sorted(expected_names - actual_names)
        if missing or extra:
            details = []
            if missing:
                details.append(f"files without a manifest entry: {', '.join(missing)}")
            if extra:
                details.append(f"manifest entries without a file: {', '.join(extra)}")
            raise ValueError("; ".join(details))

    results = []
    failures = []
    for path in files:
        filename = path.relative_to(input_directory).as_posix()
        manifest_entry = manifest.get(filename) if manifest is not None else None
        try:
            objects, shapes, valid, solids, faces = verify(
                path, require_shape, allow_empty
            )
            observed = {
                "filename": filename,
                "objects": objects,
                "shapes": shapes,
                "valid": valid,
                "solids": solids,
                "faces": faces,
            }
            manifest_errors = (
                check_manifest_counts(observed, manifest_entry, filename)
                if manifest_entry is not None
                else []
            )
            if manifest_errors:
                raise ValueError("; ".join(manifest_errors))
        except Exception as error:  # FreeCAD modules raise Python and OCC exceptions.
            failure = {"filename": filename, "error": str(error)}
            if manifest_entry is not None:
                failure["manifest"] = manifest_entry
            failures.append(failure)
            print(f"FAIL {path}: {error}", file=sys.stderr)
        else:
            if manifest_entry is not None:
                observed["manifest"] = {"status": "passed"}
            print(f"PASS {filename}", file=sys.stderr, flush=True)
            results.append(observed)
    report = {"manifest": manifest_path, "failures": failures, "files": results}
    report_json = json.dumps(report, indent=2, sort_keys=True) + "\n"
    report_path = os.environ.get("CADMPEG_STEP_REPORT")
    if report_path:
        Path(report_path).write_text(report_json, encoding="utf-8")
    print(report_json, end="", flush=True)
    if failures:
        raise SystemExit(1)


try:
    main()
except SystemExit:
    raise
except Exception as error:
    # FreeCADCmd propagates SystemExit but swallows any other uncaught
    # exception with a zero exit, which would let a failed gate pass.
    print(f"FAILED: {error}", file=sys.stderr, flush=True)
    raise SystemExit(1)
