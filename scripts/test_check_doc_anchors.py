#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for the open-item citation check of ``check-doc-anchors.py``."""

from __future__ import annotations

import importlib.util
import io
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("check-doc-anchors.py")
SPEC = importlib.util.spec_from_file_location("check_doc_anchors", SCRIPT)
assert SPEC and SPEC.loader
anchors = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = anchors
SPEC.loader.exec_module(anchors)


def open_items(*headings: str) -> str:
    text = "# Open items\n\n## 1. Records\n\n"
    for heading in headings:
        text += f"### {heading}\n\n**Question.** Why?\n\n**Known.** A.\n\n**Need.** B.\n\n"
    return text


class CitationCheck(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.docs = Path(directory.name)
        patcher = patch.object(anchors, "DOCS", self.docs)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.write("f3d-open-items.md", open_items("DR-88. Old item", "AS-01. Shared prefix"))
        self.write("sldprt-open-items.md", open_items("AS-02. Assembly"))

    def write(self, name: str, text: str) -> None:
        (self.docs / name).write_text(text, encoding="utf-8")

    def findings(self) -> list[str]:
        return anchors.check_citations(sorted(self.docs.glob("*.md")))

    def test_cited_heading_passes(self) -> None:
        self.write("f3d.md", "# F3D\n\nOpen item DR-88 holds the role. UTF-8 names.\n")
        self.assertEqual(self.findings(), [])

    def test_dangling_citation_fails_with_file_and_line(self) -> None:
        self.write("f3d.md", "# F3D\n\nOpen item DR-89 holds the role.\n")
        self.assertEqual(
            self.findings(), ["f3d.md:3: DR-89 names no item heading in f3d-open-items.md"]
        )

    def test_companion_document_cites_its_format(self) -> None:
        self.write("f3d-coverage.md", "See DR-90.\n")
        self.assertEqual(
            self.findings(),
            ["f3d-coverage.md:1: DR-90 names no item heading in f3d-open-items.md"],
        )

    def test_item_cites_a_sibling_item(self) -> None:
        self.write(
            "f3d-open-items.md",
            open_items("DR-88. Old item", "AS-01. Shared prefix") + "**Note.** See DR-91.\n",
        )
        self.assertEqual(len(self.findings()), 1)
        self.assertIn("DR-91 names no item heading", self.findings()[0])

    def test_named_document_on_the_line_is_the_target(self) -> None:
        self.write("f3d.md", "See `sldprt-open-items.md` AS-02 and AS-01 in `f3d-open-items.md`.\n")
        self.assertEqual(self.findings(), [])
        self.write("f3d.md", "See `sldprt-open-items.md` AS-01.\n")
        self.assertEqual(
            self.findings(), ["f3d.md:1: AS-01 names no item heading in sldprt-open-items.md"]
        )

    def test_document_without_own_items_cites_the_one_it_names(self) -> None:
        self.write("protein.md", "Read [`f3d-open-items.md`](f3d-open-items.md).\n\nDR-92 holds it.\n")
        self.assertEqual(
            self.findings(), ["protein.md:3: DR-92 names no item heading in f3d-open-items.md"]
        )

    def test_former_id_names_a_deleted_item(self) -> None:
        self.write("f3d.md", "The former DR-12 question is tracked in DR-88.\n")
        self.assertEqual(self.findings(), [])

    def test_main_fails_on_a_dangling_citation(self) -> None:
        self.write("f3d.md", "Open item DR-89 holds the role.\n")
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(anchors.main(["check-doc-anchors.py"]), 1)
        self.assertIn("f3d.md:1: DR-89 names no item heading", output.getvalue())


if __name__ == "__main__":
    unittest.main()
