#!/usr/bin/env python3
"""Verify generated IGES files with FreeCAD's native importer.

Run this script with FreeCADCmd, for example::

    CADMPEG_IGES_INPUT_DIR=out \
        CADMPEG_IGES_REQUIRE_SHAPE=1 \
        CADMPEG_IGES_EXPECTATIONS=expectations.json \
        CADMPEG_IGES_REPORT=out/freecad-report.json \
        FreeCADCmd scripts/verify-iges-freecad.py

The check is intentionally independent of cadmpeg. It imports each file in a
fresh document, rejects empty imports unless
``CADMPEG_IGES_ALLOW_EMPTY=1`` is specified, and rejects every imported object
whose Shape is null or invalid. FreeCAD's ``Import`` module may not expose
every presentation-only IGES record as an object; requiring a shape is
therefore a separate assertion for generated geometry profiles.

When ``CADMPEG_IGES_EXPECTATIONS`` is set, the JSON manifest must contain one
entry for every input filename and no other entries. Each entry may specify
exact topology counts, a bounding box, and scalar measures with explicit
absolute or relative tolerances. The report records the measured values and
the expectation status.
"""

from __future__ import annotations

import json
import math
import os
import sys
from pathlib import Path
from typing import Any

sys.dont_write_bytecode = True

import FreeCAD
import Import


COUNT_FIELDS = {
    "objects",
    "shapes",
    "valid",
    "solids",
    "shells",
    "faces",
    "wires",
    "edges",
    "vertices",
}
MEASURE_FIELDS = {"volume", "area", "length"}


def verify(path: Path, require_shape: bool, allow_empty: bool) -> dict[str, Any]:
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
        bounds = [shape.BoundBox for shape in shapes]
        bounding_box = None
        if bounds:
            bounding_box = [
                min(bound.XMin for bound in bounds),
                min(bound.YMin for bound in bounds),
                min(bound.ZMin for bound in bounds),
                max(bound.XMax for bound in bounds),
                max(bound.YMax for bound in bounds),
                max(bound.ZMax for bound in bounds),
            ]
        return {
            "objects": len(objects),
            "shapes": len(shape_objects),
            "valid": len(shape_objects) - len(invalid),
            "solids": sum(len(shape.Solids) for shape in shapes),
            "shells": sum(len(shape.Shells) for shape in shapes),
            "faces": sum(len(shape.Faces) for shape in shapes),
            "wires": sum(len(shape.Wires) for shape in shapes),
            "edges": sum(len(shape.Edges) for shape in shapes),
            "vertices": sum(len(shape.Vertexes) for shape in shapes),
            "bbox": bounding_box,
            "volume": sum(float(shape.Volume) for shape in shapes),
            "area": sum(float(shape.Area) for shape in shapes),
            "length": sum(float(shape.Length) for shape in shapes),
        }
    finally:
        FreeCAD.closeDocument(document.Name)


def load_expectations(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict) or data.get("version") != 1:
        raise ValueError(f"{path}: expected JSON object with version 1")
    files = data.get("files")
    if not isinstance(files, dict) or not all(
        isinstance(name, str) and isinstance(expectation, dict)
        for name, expectation in files.items()
    ):
        raise ValueError(f"{path}: files must map names to expectation objects")
    return files


def _number(value: Any, field: str, errors: list[str]) -> float | None:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        errors.append(f"{field} must be a number")
        return None
    number = float(value)
    if not math.isfinite(number):
        errors.append(f"{field} must be finite")
        return None
    return number


def _tolerances(spec: dict[str, Any], field: str, errors: list[str]) -> tuple[float, float]:
    absolute = _number(spec.get("abs_tolerance", 0.0), f"{field}.abs_tolerance", errors)
    relative = _number(spec.get("rel_tolerance", 0.0), f"{field}.rel_tolerance", errors)
    if absolute is not None and absolute < 0:
        errors.append(f"{field}.abs_tolerance must not be negative")
        absolute = None
    if relative is not None and relative < 0:
        errors.append(f"{field}.rel_tolerance must not be negative")
        relative = None
    return absolute or 0.0, relative or 0.0


def _compare_number(
    observed: Any,
    expected: Any,
    field: str,
    absolute: float,
    relative: float,
    errors: list[str],
) -> None:
    observed_number = _number(observed, f"observed {field}", errors)
    expected_number = _number(expected, f"{field}.value", errors)
    if observed_number is None or expected_number is None:
        return
    tolerance = absolute + relative * max(abs(observed_number), abs(expected_number))
    if abs(observed_number - expected_number) > tolerance:
        errors.append(
            f"{field}: expected {expected_number} ± {tolerance}, observed {observed_number}"
        )


def check_expectation(observed: dict[str, Any], expectation: Any) -> list[str]:
    errors: list[str] = []
    if not isinstance(expectation, dict):
        return ["expectation must be an object"]
    unknown = set(expectation) - {"require_shape", "counts", "bbox", "measures"}
    errors.extend(f"unknown expectation field: {field}" for field in sorted(unknown))

    require_shape = expectation.get("require_shape")
    if require_shape is not None and not isinstance(require_shape, bool):
        errors.append("require_shape must be boolean")

    counts = expectation.get("counts", {})
    if not isinstance(counts, dict):
        errors.append("counts must be an object")
    else:
        for field, expected in counts.items():
            if field not in COUNT_FIELDS:
                errors.append(f"unknown count field: {field}")
                continue
            if isinstance(expected, bool) or not isinstance(expected, int):
                errors.append(f"counts.{field} must be an integer")
                continue
            if observed[field] != expected:
                errors.append(
                    f"counts.{field}: expected {expected}, observed {observed[field]}"
                )

    bbox = expectation.get("bbox")
    if bbox is not None:
        if not isinstance(bbox, dict):
            errors.append("bbox must be an object")
        else:
            unknown_bbox = set(bbox) - {"min", "max", "abs_tolerance", "rel_tolerance"}
            errors.extend(
                f"unknown bbox field: {field}" for field in sorted(unknown_bbox)
            )
            absolute, relative = _tolerances(bbox, "bbox", errors)
            observed_bbox = observed["bbox"]
            if not isinstance(observed_bbox, list) or len(observed_bbox) != 6:
                errors.append("bbox: observed shape has no six-value bounding box")
            else:
                for label, offset in (("min", 0), ("max", 3)):
                    expected_values = bbox.get(label)
                    if not isinstance(expected_values, list) or len(expected_values) != 3:
                        errors.append(f"bbox.{label} must contain three numbers")
                        continue
                    for index, expected in enumerate(expected_values):
                        _compare_number(
                            observed_bbox[offset + index],
                            expected,
                            f"bbox.{label}[{index}]",
                            absolute,
                            relative,
                            errors,
                        )

    measures = expectation.get("measures", {})
    if not isinstance(measures, dict):
        errors.append("measures must be an object")
    else:
        for field, spec in measures.items():
            if field not in MEASURE_FIELDS:
                errors.append(f"unknown measure field: {field}")
                continue
            if not isinstance(spec, dict):
                errors.append(f"measures.{field} must be an object")
                continue
            unknown_measure = set(spec) - {"value", "abs_tolerance", "rel_tolerance"}
            errors.extend(
                f"unknown {field} field: {name}" for name in sorted(unknown_measure)
            )
            absolute, relative = _tolerances(spec, f"measures.{field}", errors)
            if "value" not in spec:
                errors.append(f"measures.{field}.value is required")
                continue
            _compare_number(
                observed[field],
                spec["value"],
                f"measures.{field}",
                absolute,
                relative,
                errors,
            )
    return errors


def main() -> None:
    input_directory = Path(os.environ["CADMPEG_IGES_INPUT_DIR"])
    require_shape = os.environ.get("CADMPEG_IGES_REQUIRE_SHAPE") == "1"
    allow_empty = os.environ.get("CADMPEG_IGES_ALLOW_EMPTY") == "1"
    expectations_path = os.environ.get("CADMPEG_IGES_EXPECTATIONS")
    expectations = (
        load_expectations(Path(expectations_path)) if expectations_path else None
    )
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
    if expectations is not None:
        expected_names = set(expectations)
        actual_names = {path.name for path in files}
        missing = sorted(actual_names - expected_names)
        extra = sorted(expected_names - actual_names)
        if missing or extra:
            details = []
            if missing:
                details.append(f"missing expectations: {', '.join(missing)}")
            if extra:
                details.append(f"expectations without files: {', '.join(extra)}")
            raise ValueError("; ".join(details))

    results = []
    failures = []
    for path in files:
        expectation = expectations.get(path.name) if expectations is not None else None
        metrics = None
        try:
            entry_require_shape = require_shape or bool(
                expectation.get("require_shape", False)
                if expectation is not None
                else False
            )
            metrics = verify(path, entry_require_shape, allow_empty)
            expectation_errors = (
                check_expectation(metrics, expectation)
                if expectation is not None
                else []
            )
            if expectation_errors:
                raise ValueError("expectation mismatch: " + "; ".join(expectation_errors))
        except Exception as error:  # FreeCAD modules raise Python and OCC exceptions.
            failure = {"filename": path.name, "error": str(error)}
            if metrics is not None:
                failure["observed"] = metrics
            if expectation is not None:
                failure["expectation"] = expectation
            failures.append(failure)
            print(f"FAIL {path}: {error}", file=sys.stderr)
        else:
            result = {"filename": path.name, **metrics}
            if expectation is not None:
                result["expectation"] = {"status": "passed", "spec": expectation}
            results.append(result)
    report = {
        "expectations": expectations_path,
        "failures": failures,
        "files": results,
    }
    report_json = json.dumps(report, indent=2, sort_keys=True) + "\n"
    report_path = os.environ.get("CADMPEG_IGES_REPORT")
    if report_path:
        Path(report_path).write_text(report_json, encoding="utf-8")
    print(report_json, end="", flush=True)
    if failures:
        raise SystemExit(1)


main()
