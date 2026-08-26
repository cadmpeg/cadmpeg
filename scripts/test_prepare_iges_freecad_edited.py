#!/usr/bin/env python3
"""Tests for the deterministic edited IGES input builder."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("prepare-iges-freecad-edited.py")
SPEC = importlib.util.spec_from_file_location("prepare_iges_freecad_edited", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class EditedFixtureTests(unittest.TestCase):
    def test_edits_the_single_point(self) -> None:
        document = {
            "model": {
                "points": [
                    {"id": "point#1", "position": {"x": 1.0, "y": 2.0, "z": 3.0}}
                ]
            }
        }

        MODULE.edit_point(document)

        self.assertEqual(document["model"]["points"][0]["position"], MODULE.EDITED_POSITION)

    def test_rejects_a_document_with_multiple_points(self) -> None:
        document = {"model": {"points": [{}, {}]}}

        with self.assertRaises(SystemExit):
            MODULE.edit_point(document)


if __name__ == "__main__":
    unittest.main()
