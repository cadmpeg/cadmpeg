#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_doc_comments.py``. These never run cargo."""

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
check = harness.load("preflight_doc_comments")

PATH = "crates/cadmpeg-codec-freecad/src/topology_transfer.rs"
BASE = "/// Transfer a shape.\n#[must_use]\nfn transfer() {}\n"


class DocComments(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({PATH: BASE})
        self.repo.branch()

    def results(self):
        return harness.run_check(check, self.repo.context())

    def test_inserted_const_warns(self) -> None:
        self.repo.commit({PATH: "/// Transfer a shape.\nconst SHAPE_CLOSED_FLAG: u8 = 4;\n"
                                "#[must_use]\nfn transfer() {}\n"})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn("`SHAPE_CLOSED_FLAG` now carries the doc comment of `transfer`",
                      results[0].details[0])

    def test_changed_signature_passes(self) -> None:
        self.repo.commit({PATH: "/// Transfer a shape.\n#[must_use]\nfn transfer(depth: u8) {}\n"})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_item_above_the_comment_passes(self) -> None:
        self.repo.commit({PATH: "/// The closed flag.\nconst SHAPE_CLOSED_FLAG: u8 = 4;\n\n" + BASE})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])


if __name__ == "__main__":
    unittest.main()
