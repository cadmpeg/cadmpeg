#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-spec-change.py``."""

from __future__ import annotations

import importlib.util
import io
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("check-spec-change.py")
SPEC = importlib.util.spec_from_file_location("check_spec_change", SCRIPT)
assert SPEC and SPEC.loader
rule = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = rule
SPEC.loader.exec_module(rule)

ANNEX = ("## Annex A. Record types stated by schema\n\n"
         "Each type below is given by the JSON Schema of its wire form.\n")
BODY = "\n### `Length`\n\n```json\n{\"type\": \"number\"}\n```\n"
DOCUMENT = "# CADIR v11\n\n## 5. The design layer\n\nA feature states its inputs.\n\n" + ANNEX + BODY


class Handwritten(unittest.TestCase):
    def test_annex_body_change_is_exempt(self) -> None:
        regenerated = DOCUMENT.replace('"number"', '"integer"')
        self.assertEqual(rule.crate_changes([rule.SPEC, "crates/cadmpeg-ir/src/units.rs"],
                                            DOCUMENT, regenerated), [])

    def test_section_edit_with_crates_reports_the_crates(self) -> None:
        edited = DOCUMENT.replace("states its inputs", "states its ordered inputs")
        paths = ["docs/cadir-v11.md", "crates/b/src/lib.rs", "scripts/x.py", "crates/a/Cargo.toml"]
        self.assertEqual(rule.crate_changes(paths, DOCUMENT, edited),
                         ["crates/a/Cargo.toml", "crates/b/src/lib.rs"])

    def test_section_edit_alone_passes(self) -> None:
        edited = DOCUMENT.replace("states its inputs", "states its ordered inputs")
        self.assertEqual(rule.crate_changes([rule.SPEC, "docs/cadir-v11-migration.md"], DOCUMENT, edited), [])

    def test_annex_opening_paragraph_is_handwritten(self) -> None:
        edited = DOCUMENT.replace("of its wire form.", "of its wire form, frozen with it.")
        self.assertEqual(rule.crate_changes([rule.SPEC, "crates/a/src/lib.rs"], DOCUMENT, edited),
                         ["crates/a/src/lib.rs"])

    def test_added_removed_or_unsplittable_specification_is_handwritten(self) -> None:
        paths = [rule.SPEC, "crates/a/src/lib.rs"]
        self.assertEqual(rule.crate_changes(paths, None, DOCUMENT), ["crates/a/src/lib.rs"])
        self.assertEqual(rule.crate_changes(paths, DOCUMENT, None), ["crates/a/src/lib.rs"])
        self.assertEqual(rule.crate_changes(paths, DOCUMENT, DOCUMENT.replace(ANNEX, "")),
                         ["crates/a/src/lib.rs"])


class Command(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.base = self.commit({rule.SPEC: DOCUMENT, "crates/a/src/lib.rs": "fn a() {}\n"})
        patcher = patch.object(rule, "ROOT", self.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def git(self, *arguments: str) -> str:
        return subprocess.run(["git", *arguments], cwd=self.root, check=True,
                              capture_output=True, text=True).stdout

    def commit(self, files: dict[str, str]) -> str:
        for path, text in files.items():
            (self.root / path).parent.mkdir(parents=True, exist_ok=True)
            (self.root / path).write_text(text)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "change")
        return self.git("rev-parse", "HEAD").strip()

    def run_check(self, *arguments: str) -> tuple[int, str]:
        output = io.StringIO()
        with redirect_stdout(output), redirect_stderr(output):
            status = rule.main(list(arguments))
        return status, output.getvalue()

    def test_specification_edit_with_a_crate_fails_and_names_the_process(self) -> None:
        self.commit({rule.SPEC: DOCUMENT.replace("its inputs", "its ordered inputs"),
                     "crates/a/src/lib.rs": "fn a() { }\n"})
        status, output = self.run_check("--base", self.base)
        self.assertEqual(status, 1)
        self.assertIn("docs/cadir-v11.md: spec_change_with_crates:", output)
        self.assertIn(rule.PROCESS, output)
        self.assertIn("  crates/a/src/lib.rs\n", output)

    def test_regenerated_annex_with_a_crate_passes(self) -> None:
        head = self.commit({rule.SPEC: DOCUMENT.replace('"number"', '"integer"'),
                            "crates/a/src/lib.rs": "fn a() { }\n"})
        self.assertEqual(self.run_check("--base", self.base, "--head", head),
                         (0, "check-spec-change: ok (only the generated Annex A body changes)\n"))

    def test_crate_change_alone_passes(self) -> None:
        self.commit({"crates/a/src/lib.rs": "fn a() { }\n"})
        self.assertEqual(self.run_check("--base", self.base)[0], 0)

    def test_unknown_base_is_an_error(self) -> None:
        status, output = self.run_check("--base", "0" * 40)
        self.assertEqual(status, 2)
        self.assertIn("check-spec-change:", output)


if __name__ == "__main__":
    unittest.main()
