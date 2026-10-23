# SPDX-License-Identifier: Apache-2.0
"""Check native geometry against neutral mesh coordinates and STEP imports."""

import json
import math
import os
from pathlib import Path

import FreeCAD as App
import Mesh
import Part


COORDINATE_TOLERANCE = 1e-5
GEOMETRY_RELATIVE_TOLERANCE = 1e-7
WORK = Path(os.environ["CADMPEG_FCSTD_WORK_DIR"])
LOCAL_TRIANGLE = (App.Vector(0, 0, 0), App.Vector(2, 0, 0), App.Vector(0, 3, 0))


def mesh_world_vertices(document):
    expected = []
    for obj in document.Objects:
        if obj.TypeId == "Mesh::Feature":
            parent = obj.getGlobalPlacement().toMatrix().multiply(
                obj.Placement.toMatrix().inverse()
            )
            expected.append([list(parent.multVec(point.Vector)) for point in obj.Mesh.Points])
        elif obj.TypeId == "App::Link":
            paths = [f"{index}." for index in range(obj.ElementCount)] if obj.ElementCount else [""]
            for path in paths:
                transform = obj.getSubObject(path, 6)
                expected.append([list(transform.multVec(point)) for point in LOCAL_TRIANGLE])
    return expected


def vertices_equal(left, right):
    return len(left) == len(right) and all(
        math.isclose(a, b, abs_tol=COORDINATE_TOLERANCE, rel_tol=0)
        for first, second in zip(left, right)
        for a, b in zip(first, second)
    )


def check_mesh(name):
    document = App.openDocument(str(WORK / "reference" / f"{name}.FCStd"))
    try:
        expected = mesh_world_vertices(document)
    finally:
        App.closeDocument(document.Name)
    cadir = json.loads((WORK / "cadir" / f"{name}.json").read_text())
    actual = []
    for tessellation in cadir["model"]["tessellations"]:
        mesh = tessellation["mesh"]
        if mesh["kind"] != "list" or mesh["triangles"] != [[0, 1, 2]]:
            raise ValueError(f"{name}: triangle incidence changed")
        actual.append([[point[axis] for axis in ("x", "y", "z")] for point in mesh["vertices"]])
    count = len(expected)
    for vertices in actual:
        match = next((index for index, candidate in enumerate(expected)
                      if vertices_equal(candidate, vertices)), None)
        if match is None:
            raise ValueError(f"{name}: unexpected world vertices {vertices!r}")
        expected.pop(match)
    if expected:
        raise ValueError(f"{name}: missing {len(expected)} mesh occurrences")
    return {"filename": name, "world_meshes": count}


def check_step():
    document = App.openDocument(str(WORK / "reference" / "filleted_box.FCStd"))
    try:
        expected = document.getObject("FilletedBox").Shape.copy()
    finally:
        App.closeDocument(document.Name)
    results = []
    for path in sorted((WORK / "step").glob("*.step")):
        shape = Part.read(str(path))
        if shape.isNull() or not shape.isValid() or len(shape.Faces) != 26:
            raise ValueError(f"{path.name}: invalid filleted box or face count")
        for name in ("Volume", "Area"):
            if not math.isclose(getattr(expected, name), getattr(shape, name),
                                rel_tol=GEOMETRY_RELATIVE_TOLERANCE):
                raise ValueError(f"{path.name}: {name} changed")
        results.append({"filename": path.name, "faces": len(shape.Faces),
                        "solids": len(shape.Solids), "volume": shape.Volume})
    if len(results) != 3:
        raise ValueError("expected AP203, AP214 and AP242 native imports")
    return results


def check_source_less():
    document = App.openDocument(str(WORK / "source_less.FCStd"))
    try:
        if document.recompute(None, True) < 0:
            raise ValueError("source-less recompute failed")
        box = document.getObject("Box")
        if box is None or not box.isValid() or "Invalid" in box.State:
            raise ValueError("source-less box failed")
        if box.Shape.isNull() or not box.Shape.isValid() or len(box.Shape.Faces) != 6:
            raise ValueError("source-less box geometry failed")
        if not math.isclose(box.Shape.Volume, 12.5 * 7 * 3,
                            rel_tol=GEOMETRY_RELATIVE_TOLERANCE):
            raise ValueError("source-less box dimensions changed")
        return {"faces": 6, "volume": box.Shape.Volume}
    finally:
        App.closeDocument(document.Name)


def check_source_less_shapes():
    document = App.openDocument(str(WORK / "source_less_shapes.FCStd"))
    try:
        for name in ("ZFirst", "ASecond"):
            shape = document.getObject(name).Shape
            if shape.isNull() or not shape.isValid() or len(shape.Faces) != 6:
                raise ValueError(f"source-less {name} failed restoration")
            if not math.isclose(shape.Volume, 6000, rel_tol=GEOMETRY_RELATIVE_TOLERANCE):
                raise ValueError(f"source-less {name} volume changed")
        return {"objects": 2, "faces_per_object": 6, "volume_per_object": 6000}
    finally:
        App.closeDocument(document.Name)


report = {"accepted": False}
try:
    report["meshes"] = [check_mesh(name) for name in ("placed_mesh", "nested_mesh", "linked_mesh")]
    report["step"] = check_step()
    report["source_less"] = check_source_less()
    report["source_less_shapes"] = check_source_less_shapes()
    report["accepted"] = True
except Exception as error:
    report["error"] = str(error)
    raise
finally:
    (WORK / "projection-report.json").write_text(json.dumps(report, indent=2, sort_keys=True))
