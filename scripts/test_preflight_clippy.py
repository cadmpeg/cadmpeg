#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_clippy.py``. These never run cargo."""

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
check = harness.load("preflight_clippy")

WORKFLOW = """jobs:
  lint:
    steps:
      - name: Clippy
        run: python3 scripts/ci_cargo.py clippy --all-targets
  features:
    steps:
      - name: Schema feature
        run: |
          set -euo pipefail
          cargo test -q -p cadmpeg-ir --features cadmpeg-ir/schema --lib
          cargo check -q -p cadmpeg-core --all-targets \\
            --features cadmpeg-core/schema
      - name: Next
        run: cargo check -q
"""
SCOPE = """
def select(root, paths, base_lock=None):
    rust = any(path.startswith("crates/") for path in paths)
    schema = any(path.startswith("crates/cadmpeg-ir/") for path in paths)
    return {"rust": rust, "full": False, "schema": schema, "codec_scope": ["sat"] if not schema else [],
            "packages": ["cadmpeg-codec-sat", "cadmpeg-ir"] if schema else ["cadmpeg-codec-sat"]}
"""
CARGO = """import os, sys
print("lint over", os.environ["CI_PACKAGES"])
print("features", os.environ["CI_CODEC_FEATURES"])
sys.exit(1 if os.path.exists("lint-fails") or "CI_CODEC_FEATURES" not in os.environ else 0)
"""


class SchemaCommands(unittest.TestCase):
    def test_continuations_are_joined(self) -> None:
        self.assertEqual(check.schema_commands(WORKFLOW), [
            "cargo test -q -p cadmpeg-ir --features cadmpeg-ir/schema --lib",
            "cargo check -q -p cadmpeg-core --all-targets --features cadmpeg-core/schema"])

    def test_repository_workflow_is_understood(self) -> None:
        workflow = (Path(__file__).resolve().parent.parent / check.WORKFLOW).read_text()
        self.assertIsNotNone(check.CLIPPY.search(workflow))
        self.assertTrue(check.schema_commands(workflow))


class ClippyCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({check.WORKFLOW: WORKFLOW, check.SCOPE: SCOPE,
                          "scripts/ci_cargo.py": CARGO})
        self.repo.branch()

    def test_without_full_prints_the_command(self) -> None:
        self.repo.commit({"crates/cadmpeg-codec-sat/src/lib.rs": "\n"})
        results = harness.run_check(check, self.repo.context())
        self.assertEqual(harness.statuses(results), ["SKIP"])
        self.assertIn("CI_PACKAGES='[\"cadmpeg-codec-sat\"]'", results[0].details[0])
        self.assertIn("CI_CODEC_FEATURES='[\"sat\"]'", results[0].details[0])

    def test_full_runs_the_gate(self) -> None:
        self.repo.commit({"crates/cadmpeg-codec-sat/src/lib.rs": "\n"})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context(full=True))),
                         ["PASS"])

    def test_full_reports_a_lint_failure(self) -> None:
        self.repo.commit({"crates/cadmpeg-codec-sat/src/lib.rs": "\n", "lint-fails": ""})
        results = harness.run_check(check, self.repo.context(full=True))
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn('lint over ["cadmpeg-codec-sat"]', results[0].details)

    def test_schema_gate_is_listed_for_ir_changes(self) -> None:
        self.repo.commit({"crates/cadmpeg-ir/src/lib.rs": "\n"})
        results = harness.run_check(check, self.repo.context())
        self.assertEqual(len(results[0].details), 3)

    def test_docs_only_passes(self) -> None:
        self.repo.commit({"docs/notes.md": "x\n"})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context(full=True))),
                         ["PASS"])


if __name__ == "__main__":
    unittest.main()
