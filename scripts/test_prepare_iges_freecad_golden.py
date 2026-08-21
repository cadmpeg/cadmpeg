#!/usr/bin/env python3
"""Tests for the IGES FreeCAD golden materializer."""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("prepare-iges-freecad-golden.py")
SPEC = importlib.util.spec_from_file_location("prepare_iges_freecad_golden", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class GoldenMaterializerTests(unittest.TestCase):
    def setUp(self) -> None:
        scratch = Path.home() / "side2" / "tmp" / "iges-l9"
        scratch.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=scratch)
        self.root = Path(self.directory.name)
        self.golden = self.root / "golden"
        self.golden.mkdir()

    def tearDown(self) -> None:
        self.directory.cleanup()

    def write_golden(self, name: str, payload: object) -> None:
        (self.golden / f"{name}.json").write_text(
            json.dumps(payload), encoding="utf-8"
        )

    def test_all_writable_skips_refused_goldens(self) -> None:
        self.write_golden("point", {"output": "point"})
        self.write_golden("refused", {"encode_error": "unsupported"})

        outputs = MODULE.all_writable_outputs(self.golden)

        self.assertEqual(outputs, [(Path("point.igs"), "point")])

    def test_materialize_refuses_conflicting_existing_output(self) -> None:
        output_dir = self.root / "out"
        output_dir.mkdir()
        target = output_dir / "point.igs"
        target.write_text("old", encoding="utf-8")

        with self.assertRaises(SystemExit):
            MODULE.materialize(output_dir, [(Path("point.igs"), "new")])

    def test_manifest_selects_named_files(self) -> None:
        self.write_golden("point", {"output": "point"})
        expectations = self.root / "expectations.json"
        expectations.write_text(
            json.dumps({"version": 1, "files": {"point.igs": {}}}),
            encoding="utf-8",
        )

        outputs = MODULE.manifest_outputs(expectations, self.golden)

        self.assertEqual(outputs, [(Path("point.igs"), "point")])


if __name__ == "__main__":
    unittest.main()
