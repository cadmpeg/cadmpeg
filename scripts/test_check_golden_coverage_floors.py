#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-golden-coverage-floors.py``."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-golden-coverage-floors.py")
SPEC = importlib.util.spec_from_file_location("check_golden_coverage_floors", SCRIPT)
assert SPEC and SPEC.loader
floors = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = floors
SPEC.loader.exec_module(floors)


class CountGolden(unittest.TestCase):
    def test_counts_regular_files_and_excludes_dotfiles(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            golden = root / "crates" / "cadmpeg-codec-demo" / "tests" / "golden"
            nested = golden / "sub"
            nested.mkdir(parents=True)
            (golden / "a.json").write_text("{}\n", encoding="utf-8")
            (nested / "b.json").write_text("{}\n", encoding="utf-8")
            (golden / ".hidden").write_text("x\n", encoding="utf-8")
            (nested / ".skip").write_text("x\n", encoding="utf-8")
            old_root = floors.ROOT
            try:
                floors.ROOT = root
                self.assertEqual(floors.count_golden("demo"), 2)
            finally:
                floors.ROOT = old_root

    def test_missing_golden_dir_is_zero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            crate = root / "crates" / "cadmpeg-codec-demo"
            crate.mkdir(parents=True)
            old_root = floors.ROOT
            try:
                floors.ROOT = root
                self.assertEqual(floors.count_golden("demo"), 0)
                self.assertEqual(floors.codec_ids(), ["demo"])
            finally:
                floors.ROOT = old_root


class CheckFloors(unittest.TestCase):
    def test_check_fails_below_floor(self) -> None:
        failures = floors.check({"catia": 56}, {"catia": 57})
        self.assertEqual(failures, ["catia: 56 < floor 57"])

    def test_check_ok_at_and_above_floor(self) -> None:
        self.assertEqual(floors.check({"catia": 57}, {"catia": 57}), [])
        self.assertEqual(floors.check({"catia": 58}, {"catia": 57}), [])

    def test_check_fails_missing_floor(self) -> None:
        failures = floors.check({"catia": 57, "creo": 84}, {"catia": 57})
        self.assertEqual(failures, ["ledger missing floor for creo"])

    def test_check_fails_missing_crate(self) -> None:
        failures = floors.check({"catia": 57}, {"catia": 57, "creo": 84})
        self.assertEqual(
            failures, ["floor for creo but no crates/cadmpeg-codec-creo"]
        )


class LedgerRoundTrip(unittest.TestCase):
    def test_parse_floors_and_notes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "floors.toml"
            path.write_text(
                floors.render_ledger(
                    "abc",
                    floors.FILTER_DESCRIPTION,
                    {"catia": 57, "sat": 6},
                    {"sat": "code-built inputs"},
                ),
                encoding="utf-8",
            )
            parsed = floors.parse_ledger(path)
            self.assertEqual(parsed["measured_at"], "abc")
            self.assertEqual(parsed["floors"], {"catia": 57, "sat": 6})
            self.assertEqual(parsed["notes"], {"sat": "code-built inputs"})


if __name__ == "__main__":
    unittest.main()
