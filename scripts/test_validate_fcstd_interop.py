# SPDX-License-Identifier: Apache-2.0
"""Native acceptance criteria without a FreeCAD installation."""

import importlib.util
import os
import runpy
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "_fcstd_validator_under_test", Path(__file__).resolve().parents[1] / "tools/validate_fcstd_interop.py"
)
VALIDATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VALIDATOR)


class Shape:
    def __init__(self, null=False, volume=6000, faces=6):
        self.null = null
        self.Volume, self.Area, self.Length = volume, 2200, 240
        self.Solids, self.Shells = [1], [1]
        self.Faces, self.Wires = [1] * faces, [1] * faces
        self.Edges, self.Vertexes = [1] * 12, [1] * 8
        self.BoundBox = SimpleNamespace(XMin=0, YMin=0, ZMin=0, XMax=10, YMax=20, ZMax=30)

    def isNull(self):
        return self.null

    def isValid(self):
        return True


class Object:
    def __init__(self, name="Box", shape=None):
        self.Name, self.TypeId = name, "Part::Box"
        self.Shape = shape if shape is not None else Shape()
        self.State, self.OutList = [], []
        self.Length = 10.0
        self.PropertiesList = ["Length"]

    def isValid(self):
        return True

    def touch(self):
        self.touched = True

    def getTypeIdOfProperty(self, name):
        return "App::PropertyUnsupported"


class Document:
    def __init__(self, objects, recompute_ok=True):
        self.Name, self.Objects = "Test", objects
        self.recompute_ok = recompute_ok

    def recompute(self, *args):
        return self.recompute_ok

    def saveAs(self, path):
        self.saved = path

    def getObject(self, name):
        return next((obj for obj in self.Objects if obj.Name == name), None)


class Application:
    def __init__(self, *documents):
        self.documents = iter(documents)
        self.closed = []

    def openDocument(self, path):
        return next(self.documents)

    def closeDocument(self, name):
        self.closed.append(name)


class NativeAcceptanceTests(unittest.TestCase):
    def test_freecadcmd_module_name_executes_the_validation_entrypoint(self):
        with tempfile.TemporaryDirectory() as directory:
            env = {"CADMPEG_FCSTD_INPUT_DIR": directory, "CADMPEG_FCSTD_OUTPUT_DIR": directory}
            with mock.patch.dict(os.environ, env, clear=True), mock.patch.dict(sys.modules, {"FreeCAD": SimpleNamespace()}):
                with self.assertRaisesRegex(ValueError, "no FCStd files"):
                    runpy.run_path(str(SPEC.origin), run_name="validate_fcstd_interop")

    def test_zero_recomputed_objects_is_a_successful_no_op(self):
        VALIDATOR.recompute(Document([], recompute_ok=0))

    def test_missing_shape_is_not_a_semantic_success(self):
        reference = Document([Object("ZFirst"), Object("ASecond")])
        output = Document([Object("ZFirst", Shape(null=True)), Object("ASecond")])
        app = Application(reference, output)
        with self.assertRaisesRegex(ValueError, "shape.*key mismatch"):
            VALIDATOR.verify(app, Path("output.FCStd"), Path("resaved.FCStd"), Path("original.FCStd"))
        self.assertEqual(app.closed, ["Test", "Test"])

    def test_geometry_census_and_volume_are_independent_acceptance_criteria(self):
        original = VALIDATOR.snapshot(Document([Object()]))
        for shape in (Shape(volume=5000), Shape(faces=5)):
            with self.subTest(shape=shape), self.assertRaises(ValueError):
                VALIDATOR.compare(original, VALIDATOR.snapshot(Document([Object(shape=shape)])))

    def test_failed_recompute_and_object_error_refuse_acceptance(self):
        with self.assertRaisesRegex(ValueError, "recompute failed"):
            VALIDATOR.recompute(Document([], recompute_ok=-1))
        obj = Object()
        obj.State = ["Invalid"]
        with self.assertRaisesRegex(ValueError, "error state"):
            VALIDATOR.snapshot(Document([obj]))

    def test_native_resave_is_not_independent_equivalence(self):
        app = Application(Document([Object()]), Document([Object()]))
        result = VALIDATOR.verify(app, Path("output.FCStd"), Path("resaved.FCStd"), require_resave=True)
        self.assertTrue(result["document_opened"])
        self.assertTrue(result["native_resave_accepted"])
        self.assertFalse(result["semantic_equivalence_accepted"])

    def test_design_edit_is_applied_to_independent_reference(self):
        source = Document([Object()])
        output = Document([Object()])
        output.Objects[0].Length = 12
        app = Application(source, output, output)
        result = VALIDATOR.verify(app, Path("edited.FCStd"), Path("resaved.FCStd"),
                                  Path("original.FCStd"),
                                  [{"object": "Box", "property": "Length", "value": 12}],
                                  require_resave=True)
        self.assertTrue(result["semantic_equivalence_accepted"])
        self.assertEqual(source.Objects[0].Length, 12)

    def test_changed_design_operand_or_placement_refuses_equivalence(self):
        original = VALIDATOR.snapshot(Document([Object()]))
        changed = Document([Object()])
        changed.Objects[0].Length = 20
        with self.assertRaisesRegex(ValueError, "Length"):
            VALIDATOR.compare(original, VALIDATOR.snapshot(changed))
        changed = Document([Object()])
        changed.Objects[0].OutList = [Object("Other")]
        with self.assertRaisesRegex(ValueError, "dependencies"):
            VALIDATOR.compare(original, VALIDATOR.snapshot(changed))

    def test_native_shape_and_material_properties_are_compared(self):
        obj = Object()
        obj.PropertiesList.extend(["InternalShape", "ShapeMaterial"])
        obj.InternalShape = Shape(volume=6000)
        obj.ShapeMaterial = SimpleNamespace(UUID="material", Name="Owned", Properties={},
            PhysicalProperties={"Density": "1 kg/mm^3"}, AppearanceProperties={"DiffuseColor": "(1,0,0,1)"})
        original = VALIDATOR.snapshot(Document([obj]))
        obj.InternalShape = Shape(volume=5000)
        with self.assertRaisesRegex(ValueError, "InternalShape"):
            VALIDATOR.compare(original, VALIDATOR.snapshot(Document([obj])))
        obj.InternalShape = Shape(volume=6000)
        obj.ShapeMaterial.AppearanceProperties = {"DiffuseColor": "(0,1,0,1)"}
        with self.assertRaisesRegex(ValueError, "ShapeMaterial"):
            VALIDATOR.compare(original, VALIDATOR.snapshot(Document([obj])))

    def test_required_design_omission_fails_and_application_omission_is_named(self):
        obj = Object()
        obj.PropertiesList.append("Unsupported")
        obj.Unsupported = object()
        with self.assertRaisesRegex(ValueError, "uncompared required design property"):
            VALIDATOR.snapshot(Document([obj]))
        obj.TypeId = "App::FeaturePython"
        item = VALIDATOR.snapshot(Document([obj]))["objects"]["Box"]
        self.assertEqual(item["uncompared_properties"][0]["name"], "Unsupported")
        self.assertEqual(item["uncompared_properties"][0]["runtime_type"], "App::PropertyUnsupported")

    def test_constraint_kind_value_and_ordered_operands_are_compared(self):
        left = SimpleNamespace(Name="", TypeId="Sketcher::Constraint", Type="Coincident",
                               First=0, FirstPos=2, Second=1, SecondPos=1, Value=0.0,
                               Driving=True, IsActive=True, InVirtualSpace=False)
        right = SimpleNamespace(Name="", TypeId="Sketcher::Constraint", Type="Coincident",
                                First=2, FirstPos=1, Second=3, SecondPos=2, Value=0.0)
        self.assertEqual(VALIDATOR.native_value(left)["First"], 0)
        for name, value in (("First", 2), ("Type", "Horizontal"), ("Value", 5.0),
                            ("Driving", False), ("IsActive", False), ("InVirtualSpace", True),
                            ("Name", "Named constraint")):
            with self.subTest(field=name):
                different = SimpleNamespace(**vars(left))
                setattr(different, name, value)
                with self.assertRaises(ValueError):
                    VALIDATOR.compare(VALIDATOR.native_value(left), VALIDATOR.native_value(different))
        self.assertNotEqual(VALIDATOR.native_value(left), VALIDATOR.native_value(right))

    def test_placement_and_mesh_topology_drift_refuse_equivalence(self):
        obj = Object()
        obj.PropertiesList.append("Placement")
        obj.Placement = SimpleNamespace(toMatrix=lambda: SimpleNamespace(A=[1.0, 0.0, 0.0, 100.0]))
        obj.Mesh = SimpleNamespace(Topology=([(0.0, 0.0, 0.0), (2.0, 0.0, 0.0), (0.0, 3.0, 0.0)], [(0, 1, 2)]))
        original = VALIDATOR.snapshot(Document([obj]))
        obj.Placement = SimpleNamespace(toMatrix=lambda: SimpleNamespace(A=[1.0, 0.0, 0.0, 200.0]))
        with self.assertRaisesRegex(ValueError, "Placement"):
            VALIDATOR.compare(original, VALIDATOR.snapshot(Document([obj])))
        obj.Placement = SimpleNamespace(toMatrix=lambda: SimpleNamespace(A=[1.0, 0.0, 0.0, 100.0]))
        obj.Mesh.Topology = ([(0.0, 0.0, 0.0)], [])
        with self.assertRaisesRegex(ValueError, "mesh"):
            VALIDATOR.compare(original, VALIDATOR.snapshot(Document([obj])))

    def test_recompute_touches_the_complete_native_design(self):
        obj = Object()
        VALIDATOR.recompute(Document([obj]))
        self.assertTrue(obj.touched)

    def test_gui_appearance_comparison_requires_view_providers(self):
        with self.assertRaisesRegex(ValueError, "view providers"):
            VALIDATOR.snapshot(Document([Object()]), require_appearance=True)
        obj = Object()
        obj.ViewObject = SimpleNamespace(ShapeColor=(1.0, 0.0, 0.0), LineWidth=2.0)
        expected = VALIDATOR.snapshot(Document([obj]), require_appearance=True)
        obj.ViewObject.ShapeColor = (0.0, 1.0, 0.0)
        with self.assertRaisesRegex(ValueError, "ShapeColor"):
            VALIDATOR.compare(expected, VALIDATOR.snapshot(Document([obj]), require_appearance=True))

    def test_shape_counts_do_not_use_geometric_tolerance(self):
        with self.assertRaises(ValueError):
            VALIDATOR.compare({"vertices": 1000000000}, {"vertices": 1000000001})


if __name__ == "__main__":
    unittest.main()
