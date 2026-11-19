#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_annex_a.py``. A stand-in replaces the generator, so these never run cargo."""

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
check = harness.load("preflight_annex_a")

DOCUMENT = "# Spec\n\n## Annex A. Record types stated by schema\n\nOpening.\n\n### `LineCurve`\n"
SOURCE = "crates/cadmpeg-ir/src/geometry/analytic.rs"


class AnnexCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({check.SPEC: DOCUMENT, SOURCE: "pub struct LineCurve { origin: f64 }\n",
                          "crates/cadmpeg-ir/src/other.rs": "pub struct Other;\n"})
        self.repo.branch()
        self.original = check.CHECK

    def tearDown(self) -> None:
        check.CHECK = self.original
        super().tearDown()

    def results(self, status: int = 0, **options):
        check.CHECK = ["python3", "-c", f"import sys; print('generator output'); sys.exit({status})"]
        return harness.run_check(check, self.repo.context(**options))

    def test_a_change_that_reaches_no_annex_type_passes_without_building(self) -> None:
        self.repo.commit({"crates/cadmpeg-ir/src/other.rs": "pub struct Other { a: u8 }\n"})
        results = self.results(status=1)
        self.assertEqual(harness.statuses(results), ["PASS"])
        self.assertEqual(results[0].summary, "no change reaches Annex A")

    def test_a_changed_annex_type_runs_the_generator(self) -> None:
        self.repo.commit({SOURCE: "pub struct LineCurve { origin: f64, direction: f64 }\n"})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])
        results = self.results(status=1)
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn(f"{SOURCE} declares `LineCurve`", results[0].details)
        self.assertIn("--write", results[0].details[-1])

    def test_a_specification_edit_runs_the_generator(self) -> None:
        self.repo.commit({check.SPEC: DOCUMENT + "\nmore\n"})
        self.assertEqual(harness.statuses(self.results(status=2)), ["FAIL"])

    def test_a_commit_under_rev_is_skipped(self) -> None:
        self.repo.commit({check.SPEC: DOCUMENT + "\nmore\n"})
        self.assertEqual(harness.statuses(self.results(rev="HEAD")), ["SKIP"])


if __name__ == "__main__":
    unittest.main()
