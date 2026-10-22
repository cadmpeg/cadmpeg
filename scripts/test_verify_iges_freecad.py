#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check IGES native-gate path discovery with stub imports, not native geometry."""

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


SCRIPT = Path(__file__).with_name("verify-iges-freecad.py")


class DamagedNativeGateDiscoveryTests(unittest.TestCase):
    def run_gate(self, root: Path, names: list[str], expected: list[str], invalid: bool = False) -> tuple[dict[str, object], list[str]]:
        inputs = root / "inputs"
        for name in names:
            target = inputs / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"synthetic path-discovery input")
        expectations = root / "expectations.json"
        expectations.write_text(json.dumps({"version": 1, "files": {name: {} for name in expected}}))
        report = root / "report.json"
        inserted: list[str] = []
        shape = SimpleNamespace(isNull=lambda: False, isValid=lambda: False)
        document = SimpleNamespace(Name="test", Objects=[SimpleNamespace(Name="Bad", Shape=shape)] if invalid else [])
        modules = {"FreeCAD": SimpleNamespace(newDocument=lambda _: document, closeDocument=lambda _: None), "Import": SimpleNamespace(insert=lambda path, _: inserted.append(Path(path).relative_to(inputs).as_posix()))}
        environment = {"CADMPEG_IGES_INPUT_DIR": str(inputs), "CADMPEG_IGES_EXPECTATIONS": str(expectations), "CADMPEG_IGES_REPORT": str(report), "CADMPEG_IGES_ALLOW_EMPTY": "1", "CADMPEG_IGES_REQUIRE_SHAPE": "0"}
        with patch.dict(sys.modules, modules), patch.dict(os.environ, environment), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            runpy.run_path(str(SCRIPT))
        return json.loads(report.read_text()), inserted

    def test_root_and_damaged_same_basename_keep_distinct_manifest_and_report_identities(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            names = ["point.igs", "damaged/group/point.igs"]
            report, inserted = self.run_gate(Path(directory), names, names)
            self.assertEqual(inserted, ["damaged/group/point.igs", "point.igs"])
            self.assertEqual([row["filename"] for row in report["files"]], inserted)
            self.assertEqual(report["failures"], [])
            self.assertTrue(all(row["expectation"]["status"] == "passed" for row in report["files"]))

    def test_missing_damaged_manifest_entry_still_refuses(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "missing expectations: damaged/point.igs"):
                self.run_gate(Path(directory), ["point.igs", "damaged/point.igs"], ["point.igs"])

    def test_damaged_input_does_not_weaken_invalid_native_shape_rejection(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(SystemExit) as caught:
                self.run_gate(root, ["damaged/invalid.igs"], ["damaged/invalid.igs"], invalid=True)
            self.assertEqual(caught.exception.code, 1)
            report = json.loads((root / "report.json").read_text())
            self.assertEqual(report["files"], [])
            self.assertEqual(report["failures"][0]["filename"], "damaged/invalid.igs")
            self.assertEqual(report["failures"][0]["error"], "invalid shapes: Bad")


if __name__ == "__main__":
    unittest.main()
