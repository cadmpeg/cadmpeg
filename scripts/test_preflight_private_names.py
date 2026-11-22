#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_private_names.py``. These use a stand-in scanner."""

from __future__ import annotations

import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "test_preflight", Path(__file__).with_name("test_preflight.py"))
assert SPEC and SPEC.loader
harness = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = harness
SPEC.loader.exec_module(harness)
check = harness.load("preflight_private_names")

# Reports one hit for every tracked line holding the marker word (REVIEW with --review),
# and its arguments.
SCANNER = '''
import subprocess, sys
args = sys.argv[1:]
repo = args[args.index("--repo") + 1]
if "--fail" in open(repo + "/mode.txt").read():
    print("scan_names: cannot read the corpus manifest", file=sys.stderr)
    sys.exit(2)
marker = "REVIEW" if "--review" in args else "MARKER"
hits = subprocess.run(["git", "-C", repo, "grep", "-n", marker], capture_output=True,
                      text=True).stdout.splitlines()
for hit in hits:
    path, line, _ = hit.split(":", 2)
    print(f"{path}:{line}: stem 0123abcd")
print(f"scan_names: {len(hits)} hits; args {' '.join(args)}", file=sys.stderr)
sys.exit(1 if hits else 0)
'''


class PrivateNamesCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        private = tempfile.TemporaryDirectory()
        self.addCleanup(private.cleanup)
        self.private = Path(private.name)
        environment = mock.patch.dict(os.environ, {"CADMPEG_PRIVATE": private.name})
        environment.start()
        self.addCleanup(environment.stop)
        self.repo.commit({"mode.txt": "ok\n", "README.md": "A part.\n"})
        self.repo.branch()

    def install(self) -> None:
        (self.private / "corpus").mkdir()
        (self.private / "corpus" / "scan_names.py").write_text(SCANNER)

    def results(self, rev: str | None = None):
        return harness.run_check(check, self.repo.context(rev))

    def test_without_a_private_checkout_skips(self) -> None:
        self.assertEqual(harness.statuses(self.results()), ["SKIP"])

    def test_clean_tree_passes_with_the_scanner_summary(self) -> None:
        self.install()
        results = self.results()
        self.assertEqual(harness.statuses(results), ["PASS", "PASS"])
        self.assertIn("0 hits", results[0].summary)
        self.assertNotIn("--rev", results[0].summary)
        self.assertIn("--review", results[1].summary)

    def test_a_named_file_fails_with_its_line_and_stem_id(self) -> None:
        self.install()
        self.repo.commit({"docs/notes.md": "first\nthe MARKER part\n"})
        results = self.results(rev="HEAD")
        self.assertEqual(harness.statuses(results), ["FAIL", "PASS"])
        self.assertIn("--rev HEAD", results[0].summary)
        self.assertIn("docs/notes.md:2: stem 0123abcd", results[0].details)
        self.assertTrue(results[0].details[-1].startswith("fix: "))

    def test_a_review_hit_warns_with_its_line(self) -> None:
        self.install()
        self.repo.commit({"docs/notes.md": "the REVIEW part\n"})
        results = self.results(rev="HEAD")
        self.assertEqual(harness.statuses(results), ["PASS", "WARN"])
        self.assertIn("--rev HEAD --review", results[1].summary)
        self.assertIn("docs/notes.md:1: stem 0123abcd", results[1].details)
        self.assertTrue(results[1].details[-1].startswith("read each line: "))

    def test_a_broken_scanner_fails(self) -> None:
        self.install()
        self.repo.commit({"mode.txt": "--fail\n"})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["FAIL", "FAIL"])
        self.assertIn("exit status 2", results[0].summary)
        self.assertIn("scan_names: cannot read the corpus manifest", results[0].details)


if __name__ == "__main__":
    unittest.main()
