#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_resource_caps.py``. These never run cargo."""

from __future__ import annotations

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
check = harness.load("preflight_resource_caps")

# A stand-in census checker: the census states one number, the tree has constants.
CHECKER = '''
import re
from pathlib import Path

def production_constants(root):
    return [line for path in sorted((root / "crates").rglob("*.rs"))
            for line in path.read_text().splitlines() if "const MAX_" in line]

def check_tree(root):
    stated = int(re.search(r"constants: (\\d+)", (root / "docs/decode-resource-caps.md").read_text()).group(1))
    actual = len(production_constants(root))
    return [] if stated == actual else [f"census total {stated}, tree {actual}"]
'''
SOURCE = "crates/cadmpeg-codec-step/src/export.rs"


class ResourceCapsCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({check.CHECKER: CHECKER,
                          check.CENSUS: "Production `MAX_*` constants: 1.\n",
                          SOURCE: "const MAX_SAMPLES: usize = 64;\n"})
        self.repo.branch()

    def results(self):
        return harness.run_check(check, self.repo.context())

    def test_no_new_constant_passes(self) -> None:
        self.repo.commit({SOURCE: "const MAX_SAMPLES: usize = 128;\n"})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_constant_without_census_fails(self) -> None:
        self.repo.commit({SOURCE: "const MAX_SAMPLES: usize = 64;\n"
                                  "pub(crate) const MAX_WORK: usize = 9;\n"})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn(f"{SOURCE}:2: MAX_WORK", results[0].details)
        self.assertIn("overall 2", results[0].details[-1])

    def test_constant_with_census_passes(self) -> None:
        self.repo.commit({SOURCE: "const MAX_SAMPLES: usize = 64;\nconst MAX_WORK: usize = 9;\n",
                          check.CENSUS: "Production `MAX_*` constants: 2.\n"})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_removed_constant_without_census_fails(self) -> None:
        self.repo.commit({SOURCE: "const LIMIT: usize = 64;\n"})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn(f"{SOURCE}: MAX_SAMPLES removed", results[0].details)
        self.assertIn("overall 0", results[0].details[-1])

    def test_removed_constant_with_census_passes(self) -> None:
        self.repo.commit({SOURCE: "const LIMIT: usize = 64;\n",
                          check.CENSUS: "Production `MAX_*` constants: 0.\n"})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_test_constants_are_ignored(self) -> None:
        self.repo.commit({"crates/cadmpeg-codec-step/src/export/tests.rs":
                          "const MAX_FIXTURE: usize = 1;\n"})
        self.assertEqual(check.changed_constants(self.repo.context()), ([], []))


if __name__ == "__main__":
    unittest.main()
