#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_spec_wording.py``. These never run cargo."""

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
check = harness.load("preflight_spec_wording")


class Phrases(unittest.TestCase):
    def test_behaviour_and_provenance_are_flagged(self) -> None:
        lines = [(1, "The decoder retains the raw z values natively."),
                 (2, "A reader selects the typed spelling from these positions."),
                 (3, "In the corpus every record stores 3."),
                 (4, "Offset 21 stores u32 `3`, the entry count."),
                 (5, "| 21 | 4 | `count` | the decoder |"),
                 (6, "The `decoder_flags` byte at offset 9 is zero."),
                 (7, "A scope whose operand paths do not read keeps its frames."),
                 (8, "The earliest definition record defines the component.")]
        self.assertEqual([number for number, _, _ in check.flagged(lines)], [1, 2, 3, 7, 8])


class SpecWordingCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({"docs/formats/f3d.md": "# F3D\n",
                          "docs/formats/f3d-open-items.md": "# Open\n"})
        self.repo.branch()

    def test_decoder_sentence_warns(self) -> None:
        self.repo.commit({"docs/formats/f3d.md": "# F3D\n\nThe last record is chosen.\n"})
        results = harness.run_check(check, self.repo.context())
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn("docs/formats/f3d.md:3: [is chosen]", results[0].details[0])

    def test_byte_semantics_pass(self) -> None:
        self.repo.commit({"docs/formats/f3d.md": "# F3D\n\nOffset 25 stores the frame sense.\n"})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])

    def test_open_items_are_not_specs(self) -> None:
        self.repo.commit({"docs/formats/f3d-open-items.md": "# Open\n\nThe decoder reads 0.\n"})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])


if __name__ == "__main__":
    unittest.main()
