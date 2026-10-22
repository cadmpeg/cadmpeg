#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_open_prs.py``. These never run cargo or gh."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "test_preflight", Path(__file__).with_name("test_preflight.py"))
assert SPEC and SPEC.loader
harness = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = harness
SPEC.loader.exec_module(harness)
check = harness.load("preflight_open_prs")

PULLS = [
    {"number": 7, "title": "step: helix carriers", "author": {"login": "someone"},
     "headRefName": "helix", "files": [{"path": "crates/x/src/export.rs"}]},
    {"number": 8, "title": "docs", "author": {"login": "other"}, "headRefName": "docs",
     "files": [{"path": "README.md"}]},
    {"number": 9, "title": "mine", "author": {"login": "me"}, "headRefName": "topic",
     "files": [{"path": "crates/x/src/export.rs"}]},
]


class OpenPulls(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({"crates/x/src/export.rs": "a\n"})
        self.repo.branch()
        self.repo.commit({"crates/x/src/export.rs": "b\n"})
        self.repo.git("remote", "add", "upstream", "https://github.com/cadmpeg/cadmpeg.git")

    def test_overlap_skips_the_own_branch(self) -> None:
        found = check.overlaps(PULLS, {"crates/x/src/export.rs"}, "topic")
        self.assertEqual(found, ["#7 step: helix carriers (someone): crates/x/src/export.rs"])

    def test_branch_of_rev_not_head(self) -> None:
        self.repo.git("branch", "other", "HEAD~1")
        self.assertEqual(check.branch_name(self.repo.context()), "topic")
        self.assertEqual(check.branch_name(self.repo.context(rev="other")), "other")
        sha = self.repo.git("rev-parse", "other").strip()
        self.assertEqual(check.branch_name(self.repo.context(rev=sha)), None)
        self.repo.git("branch", "-D", "other")
        self.assertEqual(check.branch_name(self.repo.context(rev=sha)), "upstream")

    def test_repository_from_the_remote(self) -> None:
        self.assertEqual(check.repository(self.repo.context()), "cadmpeg/cadmpeg")

    def test_overlap_warns(self) -> None:
        with patch.object(check, "open_pulls", return_value=PULLS), \
                patch.object(check.shutil, "which", return_value="/usr/bin/gh"):
            results = harness.run_check(check, self.repo.context())
        self.assertEqual(harness.statuses(results), ["WARN"])

    def test_rev_skips_its_own_pull_not_heads(self) -> None:
        self.repo.git("branch", "helix")
        with patch.object(check, "open_pulls", return_value=PULLS), \
                patch.object(check.shutil, "which", return_value="/usr/bin/gh"):
            results = harness.run_check(check, self.repo.context(rev="helix"))
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertEqual([d.split()[0] for d in results[0].details[:-1]], ["#9"])

    def test_offline_skips(self) -> None:
        results = harness.run_check(check, self.repo.context(offline=True))
        self.assertEqual(harness.statuses(results), ["SKIP"])


if __name__ == "__main__":
    unittest.main()
