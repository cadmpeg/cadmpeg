#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_spec_change.py``. These never run cargo."""

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
check = harness.load("preflight_spec_change")

DOCUMENT = ("# CADIR v11\n\nA feature states its inputs.\n\n"
            "## Annex A. Record types stated by schema\n\nEach type below is generated.\n\n"
            "### `Length`\n\nnumber\n")
CRATE = "crates/cadmpeg-ir/src/units.rs"


class SpecChangeCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({check.rule.SPEC: DOCUMENT, CRATE: "pub struct Length;\n"})
        self.repo.branch()

    def results(self, rev=None):
        return harness.run_check(check, self.repo.context(rev))

    def test_specification_edit_with_a_crate_fails(self) -> None:
        self.repo.commit({check.rule.SPEC: DOCUMENT.replace("its inputs", "its ordered inputs"),
                          CRATE: "pub struct Length(f64);\n"})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn(CRATE, results[0].details)
        self.assertIn(check.rule.PROCESS, results[0].details[-1])

    def test_uncommitted_specification_edit_is_seen(self) -> None:
        self.repo.commit({CRATE: "pub struct Length(f64);\n"})
        self.repo.write({check.rule.SPEC: DOCUMENT.replace("its inputs", "its ordered inputs")})
        self.assertEqual(harness.statuses(self.results()), ["FAIL"])

    def test_rev_checks_the_commit(self) -> None:
        head = self.repo.commit({check.rule.SPEC: DOCUMENT.replace("its inputs", "its ordered inputs"),
                                 CRATE: "pub struct Length(f64);\n"})
        self.assertEqual(harness.statuses(self.results(head)), ["FAIL"])

    def test_regenerated_annex_with_a_crate_passes(self) -> None:
        self.repo.commit({check.rule.SPEC: DOCUMENT.replace("number", "integer"),
                          CRATE: "pub struct Length(i64);\n"})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_specification_only_branch_passes(self) -> None:
        self.repo.commit({check.rule.SPEC: DOCUMENT.replace("its inputs", "its ordered inputs")})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])


if __name__ == "__main__":
    unittest.main()
