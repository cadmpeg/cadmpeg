#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Tests for resource-cap preflight dispatch."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "test_preflight", Path(__file__).with_name("test_preflight.py"))
assert SPEC and SPEC.loader
harness = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = harness
SPEC.loader.exec_module(harness)
check = harness.load("preflight_resource_caps")
CAPS_SPEC = importlib.util.spec_from_file_location(
    "resource_caps_test_checker", Path(__file__).with_name("check-resource-caps.py"))
assert CAPS_SPEC and CAPS_SPEC.loader
caps = importlib.util.module_from_spec(CAPS_SPEC)
sys.modules[CAPS_SPEC.name] = caps
CAPS_SPEC.loader.exec_module(caps)

SOURCE = "crates/demo/src/lib.rs"
VALID = "// resource-cap: MAX_FOO = 2\nconst MAX_FOO: usize = 1;\n"


class ResourceCapsCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({check.CHECKER: "", "docs/decode-resource-caps.md": "# Resource caps\n",
                          SOURCE: VALID})
        self.repo.branch()

    def results(self):
        with patch.object(check, "load_checker", return_value=caps):
            return harness.run_check(check, self.repo.context())

    def test_marker_only_removal_fails(self) -> None:
        self.repo.commit({SOURCE: "const MAX_FOO: usize = 1;\n"})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertTrue(any("missing resource-cap marker" in text for text in results[0].details))

    def test_valid_change_passes(self) -> None:
        self.repo.commit({SOURCE: VALID.replace("= 2", "= 4")})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_unrelated_change_skips_scan(self) -> None:
        self.repo.commit({"README.md": "change\n"})
        with patch.object(check, "load_checker", side_effect=AssertionError("unexpected scan")):
            self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])


if __name__ == "__main__":
    unittest.main()
