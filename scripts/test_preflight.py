#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight.py``. These build small Git repositories and never run cargo.

``GitFixture`` is shared with the ``test_preflight_<name>.py`` check tests.
"""

from __future__ import annotations

import importlib.util
import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent


def load(name: str):
    """Import scripts/<name>.py as a module."""
    path = SCRIPTS / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


preflight = load("preflight")


class GitFixture:
    """A repository with an ``upstream`` branch and a ``topic`` branch cut from it."""

    def __init__(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.git("init", "-q", "-b", "upstream")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "commit.gpgsign", "false")

    def close(self) -> None:
        self.directory.cleanup()

    def git(self, *arguments: str) -> str:
        return subprocess.run(["git", *arguments], cwd=self.root, check=True,
                              capture_output=True, text=True).stdout

    def write(self, files: dict[str, str | None]) -> None:
        for path, text in files.items():
            file = self.root / path
            if text is None:
                file.unlink()
                continue
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(text)

    def commit(self, files: dict[str, str | None], message: str = "change") -> str:
        self.write(files)
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", message)
        return self.git("rev-parse", "HEAD").strip()

    def branch(self, name: str = "topic") -> None:
        self.git("checkout", "-q", "-b", name)

    def checkout(self, name: str) -> None:
        self.git("checkout", "-q", name)

    def context(self, rev: str | None = None, **options):
        base = self.git("merge-base", "upstream", rev or "HEAD").strip()
        return preflight.Context(self.root, "upstream", base, rev, preflight.Options(**options))


def run_check(module, context):
    context.check = module.NAME
    outcome = module.run(context)
    return [outcome] if isinstance(outcome, preflight.Result) else list(outcome)


def statuses(results) -> list[str]:
    return [result.status for result in results]


class FixtureCase(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = GitFixture()
        self.addCleanup(self.repo.close)


class ContextTests(FixtureCase):
    def test_changed_paths_cover_commits_edits_and_untracked_files(self) -> None:
        self.repo.commit({"a.txt": "a\n", "b.txt": "b\n"})
        self.repo.branch()
        self.repo.commit({"a.txt": "a\nmore\n"})
        self.repo.write({"b.txt": "B\n", "c.txt": "new\n"})
        context = self.repo.context()
        self.assertEqual(context.changed_paths(), ["a.txt", "b.txt", "c.txt"])
        self.assertEqual(context.added_lines("a.txt"), [(2, "more")])
        self.assertEqual(context.added_lines("c.txt"), [(1, "new")])

    def test_rev_reads_the_commit_not_the_working_tree(self) -> None:
        self.repo.commit({"a.txt": "a\n"})
        self.repo.branch()
        head = self.repo.commit({"a.txt": "b\n"})
        self.repo.write({"a.txt": "c\n"})
        context = self.repo.context(rev=head)
        self.assertEqual(context.read("a.txt"), "b\n")
        self.assertEqual(context.read_base("a.txt"), "a\n")

    def test_test_paths(self) -> None:
        self.assertTrue(preflight.is_test_path("crates/x/src/foo/tests.rs"))
        self.assertTrue(preflight.is_test_path("crates/x/src/foo/tests/parsing.rs"))
        self.assertTrue(preflight.is_test_path("crates/x/tests/layout.rs"))
        self.assertTrue(preflight.is_test_path("crates/x/src/golden_tests.rs"))
        self.assertFalse(preflight.is_test_path("crates/x/src/foo.rs"))


class ReportTests(unittest.TestCase):
    def test_any_fail_exits_one(self) -> None:
        out = io.StringIO()
        results = [preflight.Result("PASS", "a", "fine"),
                   preflight.Result("FAIL", "b", "broken", ["fix: mend it"])]
        self.assertEqual(preflight.report(results, out), 1)
        text = out.getvalue()
        self.assertIn("preflight: FAIL  b  broken\n  fix: mend it\n", text)
        self.assertIn("preflight: 1 FAIL, 0 WARN, 1 PASS, 0 SKIP", text)

    def test_warnings_alone_pass(self) -> None:
        results = [preflight.Result("WARN", "a", "look")]
        self.assertEqual(preflight.report(results, io.StringIO()), 0)

    def test_a_raising_check_fails_without_hiding_others(self) -> None:
        class Broken:
            NAME = "broken"

            @staticmethod
            def run(context):
                raise ValueError("bad input")

        class Fine:
            NAME = "fine"

            @staticmethod
            def run(context):
                return context.passed("ok")

        context = preflight.Context(Path("."), "upstream", "0" * 40)
        results = preflight.run_checks(context, [Broken, Fine])
        self.assertEqual([(r.check, r.status) for r in results],
                         [("broken", "FAIL"), ("fine", "PASS")])

    def test_upstream_remote_prefers_the_project_url(self) -> None:
        repo = GitFixture()
        self.addCleanup(repo.close)
        repo.git("remote", "add", "origin", "https://github.com/someone/cadmpeg.git")
        repo.git("remote", "add", "up", "git@github.com:cadmpeg/cadmpeg.git")
        self.assertEqual(preflight.upstream_remote(repo.root), "up")


class BranchCheckTests(FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.check = load("preflight_branch")
        self.repo.commit({"a.txt": "a\n", "b.txt": "b\n"})
        self.repo.branch()

    def test_up_to_date_branch_passes(self) -> None:
        self.repo.commit({"a.txt": "a2\n"})
        self.assertEqual(statuses(run_check(self.check, self.repo.context())), ["PASS"])

    def test_conflict_fails(self) -> None:
        self.repo.commit({"a.txt": "mine\n"})
        self.repo.checkout("upstream")
        self.repo.commit({"a.txt": "theirs\n"})
        self.repo.checkout("topic")
        results = run_check(self.check, self.repo.context())
        self.assertIn("FAIL", statuses(results))
        self.assertIn("conflict: a.txt", results[0].details)

    def test_related_merge_warns(self) -> None:
        self.repo.commit({"a.txt": "a\nmine\n"})
        self.repo.checkout("upstream")
        self.repo.commit({"a.txt": "theirs\na\n"})
        self.repo.checkout("topic")
        self.assertEqual(statuses(run_check(self.check, self.repo.context())), ["WARN"])

    def test_unrelated_merge_passes(self) -> None:
        self.repo.commit({"a.txt": "a2\n"})
        self.repo.checkout("upstream")
        self.repo.commit({"b.txt": "b2\n"})
        self.repo.checkout("topic")
        self.assertEqual(statuses(run_check(self.check, self.repo.context())), ["PASS"])


if __name__ == "__main__":
    unittest.main()
