#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the STEP and Rhino native gates' manifest agreement with stubs."""

from __future__ import annotations

import contextlib
import io
import json
import os
from pathlib import Path
import runpy
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


STEP_SCRIPT = Path(__file__).with_name("verify-step-freecad.py")
RHINO_SCRIPT = Path(__file__).with_name("verify-rhino-rhino3dm.py")


def load(name: str, filename: str):
    import importlib.util

    specification = importlib.util.spec_from_file_location(
        name, Path(__file__).with_name(filename))
    assert specification is not None and specification.loader is not None
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


class StepManifestTests(unittest.TestCase):
    def run_gate(self, root: Path, names: list[str], manifest: dict) -> dict:
        inputs = root / "inputs"
        for name in names:
            target = inputs / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"synthetic manifest input")
        manifest_path = root / "manifest.json"
        manifest_path.write_text(json.dumps({"version": 1, "writer": "step", "files": manifest}))
        report = root / "report.json"
        shape = SimpleNamespace(isNull=lambda: False, isValid=lambda: True,
                                Solids=[object()], Faces=[object(), object()])
        leaf = SimpleNamespace(Name="Leaf", Shape=shape, InList=[])
        document = SimpleNamespace(Name="test", Objects=[leaf])
        modules = {"FreeCAD": SimpleNamespace(newDocument=lambda _: document, closeDocument=lambda _: None),
                   "Import": SimpleNamespace(insert=lambda path, _: None)}
        environment = {"CADMPEG_STEP_INPUT_DIR": str(inputs), "CADMPEG_STEP_MANIFEST": str(manifest_path),
                       "CADMPEG_STEP_REPORT": str(report), "CADMPEG_STEP_ALLOW_EMPTY": "1"}
        with patch.dict(sys.modules, modules), patch.dict(os.environ, environment), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            runpy.run_path(str(STEP_SCRIPT))
        return json.loads(report.read_text())

    def test_solid_and_face_counts_must_match_the_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            report = self.run_gate(Path(directory), ["solid.step", "damaged/odd.stp"],
                                   {"solid.step": {"solids": 1, "faces": 2},
                                    "damaged/odd.stp": {"error": "output does not decode"}})
            self.assertEqual(report["failures"], [])
            self.assertEqual([row["filename"] for row in report["files"]],
                             ["damaged/odd.stp", "solid.step"])

    def test_count_mismatch_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(SystemExit):
                self.run_gate(root, ["solid.step"], {"solid.step": {"solids": 1, "faces": 9}})
            report = json.loads((root / "report.json").read_text())
            self.assertIn("faces: expected 9, observed 2", report["failures"][0]["error"])

    def test_product_containers_do_not_multiply_the_counts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = root / "inputs"
            inputs.mkdir(parents=True)
            (inputs / "assembly.step").write_bytes(b"synthetic manifest input")
            manifest_path = root / "manifest.json"
            manifest_path.write_text(json.dumps({"version": 1, "writer": "step",
                                                 "files": {"assembly.step": {"solids": 1, "faces": 2}}}))
            report = root / "report.json"
            shape = SimpleNamespace(isNull=lambda: False, isValid=lambda: True,
                                    Solids=[object()], Faces=[object(), object()])
            container = SimpleNamespace(Name="Model", Shape=shape, InList=[])
            leaf = SimpleNamespace(Name="Leaf", Shape=shape, InList=[container])
            document = SimpleNamespace(Name="test", Objects=[container, leaf])
            modules = {"FreeCAD": SimpleNamespace(newDocument=lambda _: document, closeDocument=lambda _: None),
                       "Import": SimpleNamespace(insert=lambda path, _: None)}
            environment = {"CADMPEG_STEP_INPUT_DIR": str(inputs), "CADMPEG_STEP_MANIFEST": str(manifest_path),
                           "CADMPEG_STEP_REPORT": str(report), "CADMPEG_STEP_ALLOW_EMPTY": "1"}
            with patch.dict(sys.modules, modules), patch.dict(os.environ, environment), \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                runpy.run_path(str(STEP_SCRIPT))
            self.assertEqual(json.loads(report.read_text())["failures"], [])

    def test_file_without_manifest_entry_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = root / "inputs"
            target = inputs / "solid.step"
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"synthetic manifest input")
            manifest_path = root / "manifest.json"
            manifest_path.write_text(json.dumps({"version": 1, "writer": "step",
                                                 "files": {"other.step": {"solids": 0, "faces": 0}}}))
            report = root / "report.json"
            shape = SimpleNamespace(isNull=lambda: False, isValid=lambda: True,
                                    Solids=[object()], Faces=[object(), object()])
            document = SimpleNamespace(Name="test", Objects=[SimpleNamespace(Name="Solid", Shape=shape)])
            modules = {"FreeCAD": SimpleNamespace(newDocument=lambda _: document, closeDocument=lambda _: None),
                       "Import": SimpleNamespace(insert=lambda path, _: None)}
            environment = {"CADMPEG_STEP_INPUT_DIR": str(inputs), "CADMPEG_STEP_MANIFEST": str(manifest_path),
                           "CADMPEG_STEP_REPORT": str(report), "CADMPEG_STEP_ALLOW_EMPTY": "1"}
            errors = io.StringIO()
            with patch.dict(sys.modules, modules), patch.dict(os.environ, environment), \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(errors):
                with self.assertRaises(SystemExit):
                    runpy.run_path(str(STEP_SCRIPT))
            self.assertIn("without a manifest entry", errors.getvalue())


class RhinoManifestTests(unittest.TestCase):
    def run_gate(self, root: Path, names: list[str], manifest: dict, readable: bool = True) -> dict:
        inputs = root / "inputs"
        for name in names:
            target = inputs / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"synthetic manifest input")
        manifest_path = root / "manifest.json"
        manifest_path.write_text(json.dumps({"version": 1, "writer": "rhino", "files": manifest}))
        report = root / "report.json"
        model = SimpleNamespace(Objects=[object(), object()], Layers=[object()]) if readable else None
        rhino3dm = SimpleNamespace(File3dm=SimpleNamespace(Read=lambda _: model))
        environment = {"CADMPEG_RHINO_INPUT_DIR": str(inputs), "CADMPEG_RHINO_MANIFEST": str(manifest_path),
                       "CADMPEG_RHINO_REPORT": str(report)}
        with patch.dict(sys.modules, {"rhino3dm": rhino3dm}), patch.dict(os.environ, environment), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            runpy.run_path(str(RHINO_SCRIPT))
        return json.loads(report.read_text())

    def test_object_and_layer_counts_must_match_the_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            report = self.run_gate(Path(directory), ["box.3dm", "damaged/odd.3dm"],
                                   {"box.3dm": {"objects": 2, "layers": 1},
                                    "damaged/odd.3dm": {"error": "output does not decode"}})
            self.assertEqual(report["failures"], [])
            self.assertEqual(report["files"][0], {"filename": "box.3dm", "objects": 2, "layers": 1})

    def test_count_mismatch_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(SystemExit):
                self.run_gate(root, ["box.3dm"], {"box.3dm": {"objects": 7, "layers": 1}})
            report = json.loads((root / "report.json").read_text())
            self.assertIn("objects: expected 7, observed 2", report["failures"][0]["error"])

    def test_unreadable_model_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(SystemExit):
                self.run_gate(root, ["box.3dm"], {"box.3dm": {"objects": 2, "layers": 1}}, readable=False)
            report = json.loads((root / "report.json").read_text())
            self.assertIn("refused to open", report["failures"][0]["error"])

    def test_manifest_without_file_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "without a file"):
                self.run_gate(Path(directory), ["box.3dm"],
                              {"box.3dm": {"objects": 2, "layers": 1},
                               "ghost.3dm": {"objects": 0, "layers": 0}})


if __name__ == "__main__":
    unittest.main()
