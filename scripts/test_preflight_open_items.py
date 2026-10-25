#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_open_items.py``. These never run cargo."""

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
check = harness.load("preflight_open_items")

PATH = "docs/formats/f3d-open-items.md"


def document(*entries: str) -> str:
    text = "# Open items\n\n## 1. Records\n\n"
    for entry in entries:
        text += f"### {entry}\n\n**Question.** Why?\n\n"
    return text


class OpenItemsCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item")})
        self.repo.branch()

    def results(self):
        return harness.run_check(check, self.repo.context())

    def test_new_free_id_passes(self) -> None:
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                         "DR-90. Current revision")})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_id_taken_upstream_fails_with_next_free(self) -> None:
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                         "DR-90. Current revision")})
        self.repo.checkout("upstream")
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                         "DR-90. Sketch member component", "DR-91. Sense")})
        self.repo.checkout("topic")
        results = self.results()
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn("next free is DR-92", results[0].details[0])

    def test_duplicate_within_file_fails(self) -> None:
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                         "DR-89. Again")})
        self.assertEqual(harness.statuses(self.results()), ["FAIL"])

    def test_same_item_already_upstream_passes(self) -> None:
        entries = ("DR-88. Old item", "DR-89. Another item", "DR-90. Shared")
        self.repo.commit({PATH: document(*entries)})
        self.repo.checkout("upstream")
        self.repo.commit({PATH: document(*entries)})
        self.repo.checkout("topic")
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_next_free_keeps_zero_padding(self) -> None:
        self.assertEqual(check.next_free("GC", [("GC-01", "a", 1), ("GC-09A", "b", 2)]), "GC-10")


if __name__ == "__main__":
    unittest.main()
