#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_ci_static.py``. These never run cargo."""

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
check = harness.load("preflight_ci_static")

WORKFLOW = """jobs:
  changes:
    steps:
      - run: python3 scripts/ci_scope.py --full
  static:
    steps:
      - name: Python tooling tests
        run: python3 -m unittest discover -q -s scripts -p 'test_*.py'
      - name: Good
        run: python3 scripts/good.py
      - name: Policy
        run: |
          set -euo pipefail
          python3 scripts/bad.py --check
  lint:
    steps:
      - run: cargo fmt --all --check
      - run: python3 scripts/ci_cargo.py clippy --all-targets
"""


class CommandSelection(unittest.TestCase):
    def test_static_python_and_fmt_only(self) -> None:
        self.assertEqual(check.commands(WORKFLOW, scripts_changed=True), [
            "python3 -m unittest discover -q -s scripts -p 'test_*.py'",
            "python3 scripts/good.py", "python3 scripts/bad.py --check",
            "cargo fmt --all --check"])

    def test_tooling_tests_need_a_scripts_change(self) -> None:
        self.assertNotIn("unittest", " ".join(check.commands(WORKFLOW, scripts_changed=False)))

    def test_repository_workflow_is_understood(self) -> None:
        workflow = (Path(__file__).resolve().parent.parent / check.WORKFLOW).read_text()
        selected = check.commands(workflow, scripts_changed=False)
        self.assertIn("python3 scripts/check-source-policy.py", selected)
        self.assertIn("cargo fmt --all --check", selected)


class StaticCheck(harness.FixtureCase):
    def test_failing_command_fails_with_output(self) -> None:
        self.repo.commit({check.WORKFLOW: WORKFLOW.replace("cargo fmt", "python3 scripts/good.py fmt"),
                          "scripts/good.py": "print('good: ok')\n",
                          "scripts/bad.py": "import sys\nprint('bad: 2 findings')\nsys.exit(1)\n"})
        self.repo.branch()
        self.repo.commit({"README.md": "x\n"})
        results = harness.run_check(check, self.repo.context())
        self.assertEqual(harness.statuses(results), ["FAIL", "PASS"])
        self.assertEqual(results[0].summary, "python3 scripts/bad.py --check")
        self.assertIn("bad: 2 findings", results[0].details)

    def test_rev_is_skipped(self) -> None:
        self.repo.commit({check.WORKFLOW: WORKFLOW})
        self.repo.branch()
        head = self.repo.commit({"README.md": "x\n"})
        results = harness.run_check(check, self.repo.context(rev=head))
        self.assertEqual(harness.statuses(results), ["SKIP"])


if __name__ == "__main__":
    unittest.main()
