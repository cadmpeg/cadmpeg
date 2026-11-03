#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_open_items.py``. These never run cargo."""

from __future__ import annotations

import importlib.util
import io
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

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

    def test_clean_merge_of_two_branches_taking_one_id_fails_the_tree(self) -> None:
        self.repo.commit({PATH: document("DR-90. Current revision", "DR-88. Old item",
                                         "DR-89. Another item")})
        self.repo.checkout("upstream")
        self.repo.commit({PATH: document("DR-88. Old item", "DR-89. Another item",
                                         "DR-90. Sense")})
        self.assertEqual(check.tree_findings(self.repo.root), [])
        self.repo.git("merge", "-q", "--no-edit", "topic")
        findings = check.tree_findings(self.repo.root)
        self.assertEqual(len(findings), 2)
        self.assertIn("DR-90 appears 2 times; next free is DR-91", findings[0])


class TreeCheck(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        (self.root / "docs/formats").mkdir(parents=True)

    def write(self, path: str, *entries: str) -> None:
        (self.root / path).write_text(document(*entries), encoding="utf-8")

    def test_repeat_is_reported_at_each_heading(self) -> None:
        self.write(PATH, "DR-88. Old item", "DR-89. Another item", "DR-89. Again")
        findings = check.tree_findings(self.root)
        self.assertEqual(len(findings), 2)
        self.assertTrue(findings[0].startswith(f"{PATH}:"))
        self.assertIn("DR-89 appears 2 times; next free is DR-90", findings[0])

    def test_one_id_in_two_documents_passes(self) -> None:
        self.write(PATH, "DR-88. Old item")
        self.write("docs/formats/asm-open-items.md", "DR-88. Old item")
        self.assertEqual(check.tree_findings(self.root), [])

    def test_suffixed_ids_are_distinct(self) -> None:
        self.write(PATH, "DR-88. Old item", "DR-88A. Part")
        self.assertEqual(check.tree_findings(self.root), [])

    def test_other_documents_are_not_read(self) -> None:
        self.write("docs/formats/f3d.md", "DR-88. Old item", "DR-88. Again")
        self.assertEqual(check.tree_findings(self.root), [])

    def test_main_fails_on_a_repeat_with_the_fix(self) -> None:
        self.write(PATH, "DR-88. Old item", "DR-88. Again")
        errors = io.StringIO()
        with patch.object(check, "ROOT", self.root), redirect_stderr(errors):
            self.assertEqual(check.main(), 1)
        self.assertIn("appears 2 times", errors.getvalue())
        self.assertIn("fix: renumber", errors.getvalue())

    def test_main_passes_on_unique_ids(self) -> None:
        self.write(PATH, "DR-88. Old item", "DR-89. Another item")
        output = io.StringIO()
        with patch.object(check, "ROOT", self.root), redirect_stdout(output):
            self.assertEqual(check.main(), 0)
        self.assertEqual(output.getvalue(), "open-item IDs: ok\n")


if __name__ == "__main__":
    unittest.main()
