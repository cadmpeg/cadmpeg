#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Tests for version pins outside generated goldens."""

import importlib.util
import sys
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "test_preflight", Path(__file__).with_name("test_preflight.py"))
assert SPEC and SPEC.loader
harness = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = harness
SPEC.loader.exec_module(harness)


class VersionPins(harness.FixtureCase):
    def test_stale_report_input_fails_after_golden_is_updated(self):
        version = "crates/cadmpeg-ir/src/document.rs"
        golden = "crates/example/tests/golden/result.json"
        report = "scripts/report-input.json"
        self.repo.commit({version: 'pub const IR_VERSION: &str = "11";\n',
                          golden: '{"ir_version": "11"}\n',
                          report: '{"ir_version": "11"}\n'})
        self.repo.branch()
        head = self.repo.commit({version: 'pub const IR_VERSION: &str = "12";\n',
                                 golden: '{"ir_version": "12"}\n'})
        check = harness.load("preflight_version_pins")
        for rev in (None, head):
            result = harness.run_check(check, self.repo.context(rev=rev))[0]
            self.assertEqual(result.status, "FAIL")
            self.assertIn(report, result.details)
            self.assertNotIn(golden, result.details)
        self.repo.write({report: '{"ir_version": "12"}\n'})
        self.assertEqual(harness.run_check(check, self.repo.context())[0].status, "PASS")


if __name__ == "__main__":
    unittest.main()
