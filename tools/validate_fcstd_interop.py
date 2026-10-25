# SPDX-License-Identifier: Apache-2.0
"""Compare FCStd writes with independent native originals inside FreeCAD."""

import hashlib
import json
import math
import os
from pathlib import Path


GEOMETRY_RELATIVE_TOLERANCE = 1e-9
GEOMETRY_ABSOLUTE_TOLERANCE = 1e-7
APPEARANCE_PROPERTIES = (
    "ShapeColor", "DiffuseColor", "ShapeAppearance", "LineColor", "LineColorArray",
    "PointColor", "PointColorArray", "LineWidth", "PointSize", "Transparency",
    "Visibility", "DisplayMode",
)


def native_value(value):
    if value is None or isinstance(value, (str, bool, int)):
        return value
    if isinstance(value, float):
        if not math.isfinite(value):
            raise ValueError("nonfinite native property")
        return value
    if isinstance(value, (list, tuple)):
        return [native_value(item) for item in value]
    if isinstance(value, dict):
        return {str(key): native_value(item) for key, item in sorted(value.items())}
    if hasattr(value, "Type") and hasattr(value, "First"):
        return {name: native_value(getattr(value, name))
                for name in ("Type", "First", "FirstPos", "Second", "SecondPos",
                             "Third", "ThirdPos", "Value", "Name", "Driving",
                             "IsActive", "InVirtualSpace", "Label", "isDriving",
                             "isActive", "isInVirtualSpace") if hasattr(value, name)}
    if all(hasattr(value, name) for name in ("Name", "TypeId", "Document")):
        return {"object": value.Name, "type": value.TypeId}
    if all(hasattr(value, name) for name in ("UUID", "PhysicalProperties", "AppearanceProperties")):
        return {name: native_value(getattr(value, name))
                for name in ("UUID", "Name", "Properties", "PhysicalProperties", "AppearanceProperties")}
    if all(hasattr(value, name) for name in ("isNull", "isValid", "BoundBox")):
        return shape_snapshot(value)
    if hasattr(value, "Commands"):
        return [{"command": command.Name, "parameters": native_value(command.Parameters)}
                for command in value.Commands]
    if hasattr(value, "toMatrix"):
        return list(value.toMatrix().A)
    if all(hasattr(value, axis) for axis in ("x", "y", "z")):
        return [float(value.x), float(value.y), float(value.z)]
    if hasattr(value, "Q"):
        return list(value.Q)
    if hasattr(value, "Value"):
        return native_value(value.Value)
    if hasattr(value, "toShape"):
        return {"geometry_type": type(value).__name__, "shape": shape_snapshot(value.toShape())}
    if hasattr(value, "DiffuseColor"):
        return {
            name: native_value(getattr(value, name))
            for name in ("DiffuseColor", "AmbientColor", "SpecularColor", "EmissiveColor",
                         "Shininess", "Transparency")
            if hasattr(value, name)
        }
    raise TypeError(type(value).__name__)


def shape_snapshot(shape):
    if shape.isNull():
        return {"null": True}
    if not shape.isValid():
        raise ValueError("invalid native shape")
    bounds = shape.BoundBox
    return {
        "null": False,
        "solids": len(shape.Solids), "shells": len(shape.Shells),
        "faces": len(shape.Faces), "wires": len(shape.Wires),
        "edges": len(shape.Edges), "vertices": len(shape.Vertexes),
        "volume": shape.Volume, "area": shape.Area, "length": shape.Length,
        "bounds": [bounds.XMin, bounds.YMin, bounds.ZMin,
                   bounds.XMax, bounds.YMax, bounds.ZMax],
    }


def snapshot(document, require_appearance=False):
    objects = {}
    appearance_objects = 0
    for obj in document.Objects:
        states = list(obj.State)
        if any(state.lower() in ("invalid", "error") for state in states) or not obj.isValid():
            raise ValueError(f"object {obj.Name} has error state {states!r}")
        item = {"type": obj.TypeId,
                "dependencies": sorted(link.Name for link in obj.OutList),
                "properties": {}, "uncompared_properties": []}
        for name in sorted(obj.PropertiesList):
            if name in ("Shape", "Mesh", "Points"):
                continue
            if name == "Proxy":
                item["uncompared_properties"].append({"name": name,
                    "runtime_type": obj.getTypeIdOfProperty(name), "python_type": "inert Python proxy"})
                continue
            try:
                item["properties"][name] = native_value(getattr(obj, name))
            except TypeError as error:
                runtime_type = obj.getTypeIdOfProperty(name)
                omission = {"name": name, "runtime_type": runtime_type,
                            "python_type": str(error)}
                if obj.TypeId.startswith(("Part::", "PartDesign::", "Sketcher::")):
                    raise ValueError(f"uncompared required design property {obj.Name}.{name}: {omission!r}") from error
                item["uncompared_properties"].append(omission)
        if hasattr(obj, "getNonEmptyCells"):
            item["cells"] = {cell: obj.getContents(cell) for cell in obj.getNonEmptyCells()}
        if hasattr(obj, "Shape"):
            item["shape"] = shape_snapshot(obj.Shape)
        if hasattr(obj, "Mesh"):
            vertices, faces = obj.Mesh.Topology
            item["mesh"] = {"vertices": native_value(vertices), "faces": native_value(faces)}
        if hasattr(obj, "Points"):
            item["points"] = native_value(obj.Points.Points)
        view = getattr(obj, "ViewObject", None)
        if view is not None:
            item["appearance"] = {
                name: native_value(getattr(view, name))
                for name in APPEARANCE_PROPERTIES if hasattr(view, name)
            }
            appearance_objects += bool(item["appearance"])
        objects[obj.Name] = item
    if require_appearance and not appearance_objects:
        raise ValueError("native GUI appearance comparison requires view providers")
    return {"objects": objects}


def compare(expected, actual, path="document"):
    if isinstance(expected, bool) or expected is None or isinstance(expected, str):
        if type(actual) is not type(expected) or actual != expected:
            raise ValueError(f"{path}: {actual!r} != {expected!r}")
    elif isinstance(expected, (float, int)):
        if isinstance(actual, bool) or not isinstance(actual, (float, int)):
            raise ValueError(f"{path}: expected numeric value")
        if isinstance(expected, int) and isinstance(actual, int):
            equal = expected == actual
        else:
            equal = math.isclose(expected, actual, rel_tol=GEOMETRY_RELATIVE_TOLERANCE,
                                 abs_tol=GEOMETRY_ABSOLUTE_TOLERANCE)
        if not equal:
            raise ValueError(f"{path}: {actual!r} != {expected!r}")
    elif isinstance(expected, dict):
        if not isinstance(actual, dict) or expected.keys() != actual.keys():
            raise ValueError(f"{path}: key mismatch")
        for key in expected:
            compare(expected[key], actual[key], f"{path}.{key}")
    elif isinstance(expected, list):
        if not isinstance(actual, list) or len(expected) != len(actual):
            raise ValueError(f"{path}: sequence length mismatch")
        for index, (left, right) in enumerate(zip(expected, actual)):
            compare(left, right, f"{path}[{index}]")
    else:
        raise TypeError(type(expected).__name__)


def recompute(document):
    for obj in document.Objects:
        obj.touch()
    result = document.recompute(None, True)
    if not isinstance(result, int) or result < 0:
        raise ValueError(f"native recompute failed: {result!r}")
    return result


def reference_snapshot(app, path, edits, require_appearance):
    document = app.openDocument(str(path))
    if document is None:
        raise ValueError(f"native reference refused: {path}")
    try:
        for edit in edits:
            obj = document.getObject(edit["object"])
            if obj is None or edit["property"] not in obj.PropertiesList:
                raise ValueError(f"unknown oracle edit: {edit!r}")
            setattr(obj, edit["property"], edit["value"])
        recompute(document)
        return snapshot(document, require_appearance)
    finally:
        app.closeDocument(document.Name)


def verify(app, input_path, output_path, reference=None, edits=(),
           require_resave=False, require_appearance=False):
    if edits and not require_resave:
        raise ValueError("design-edit verification requires native recompute/resave")
    expected = None
    if reference is not None:
        expected = reference_snapshot(app, reference, edits, require_appearance)
    document = app.openDocument(str(input_path))
    if document is None:
        raise ValueError(f"native document refused: {input_path}")
    try:
        restored = snapshot(document, require_appearance)
        verified = restored
        recomputed_objects = None
        if expected is not None and not edits:
            compare(expected, restored)
        if require_resave:
            recomputed_objects = recompute(document)
            recomputed = snapshot(document, require_appearance)
            verified = recomputed
            compare(expected if expected is not None else restored, recomputed)
            document.saveAs(str(output_path))
    finally:
        app.closeDocument(document.Name)
    if require_resave:
        reopened = app.openDocument(str(output_path))
        if reopened is None:
            raise ValueError(f"native resave refused: {output_path}")
        try:
            compare(expected if expected is not None else restored,
                    snapshot(reopened, require_appearance))
        finally:
            app.closeDocument(reopened.Name)
    return {
        "filename": input_path.name,
        "objects": len(restored["objects"]),
        "document_opened": True,
        "semantic_equivalence_accepted": expected is not None,
        "native_resave_accepted": require_resave,
        "appearance_compared": require_appearance and expected is not None,
        "semantic_equivalence_scope": "geometry, supported design properties, dependencies, and requested GUI appearance",
        "uncompared_properties": {name: item["uncompared_properties"]
                                  for name, item in verified["objects"].items()
                                  if item["uncompared_properties"]},
        "recomputed_objects": recomputed_objects,
        "native_snapshot": verified,
    }


def main():
    import FreeCAD as app

    input_directory = Path(os.environ["CADMPEG_FCSTD_INPUT_DIR"])
    output_directory = Path(os.environ["CADMPEG_FCSTD_OUTPUT_DIR"])
    reference_directory = os.environ.get("CADMPEG_FCSTD_REFERENCE_DIR")
    require_semantics = os.environ.get("CADMPEG_FCSTD_REQUIRE_SEMANTIC_EQUIVALENCE") == "1"
    require_resave = os.environ.get("CADMPEG_FCSTD_REQUIRE_NATIVE_RESAVE") == "1"
    require_appearance = os.environ.get("CADMPEG_FCSTD_REQUIRE_APPEARANCE") == "1"
    if require_appearance:
        import FreeCADGui as gui
        gui.showMainWindow()
    if require_semantics and reference_directory is None:
        raise ValueError("semantic equivalence requires CADMPEG_FCSTD_REFERENCE_DIR")
    expectations_path = os.environ.get("CADMPEG_FCSTD_EXPECTATIONS")
    expectations = (json.loads(Path(expectations_path).read_text(encoding="utf-8"))
                    if expectations_path else {})
    output_directory.mkdir(parents=True, exist_ok=True)
    paths = sorted(input_directory.glob("*.FCStd"))
    if not paths:
        raise ValueError(f"no FCStd files found in {input_directory}")
    results = []
    failures = []
    for path in paths:
        try:
            expectation = expectations.get(path.name, {})
            reference_name = expectation.get("reference", path.name)
            reference = Path(reference_directory) / reference_name if reference_directory else None
            result = verify(app, path, output_directory / path.name, reference,
                            expectation.get("edits", ()),
                            require_resave, require_appearance)
            result["input_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
            result["reference_sha256"] = (hashlib.sha256(reference.read_bytes()).hexdigest()
                                          if reference is not None else None)
            results.append(result)
        except Exception as error:
            failures.append({"filename": path.name, "error": str(error)})
    report = {"files": results, "failures": failures}
    report_json = json.dumps(report, indent=2, sort_keys=True) + "\n"
    report_path = os.environ.get("CADMPEG_FCSTD_REPORT")
    if report_path:
        Path(report_path).write_text(report_json, encoding="utf-8")
    print(report_json, end="", flush=True)
    if failures:
        raise SystemExit(1)


if __name__ in ("__main__", "validate_fcstd_interop"):
    main()
