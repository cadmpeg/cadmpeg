#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_dependent_goldens.py``. These never run cargo."""

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
check = harness.load("preflight_dependent_goldens")

# A stand-in for ci_scope.select: an asm change reaches sat and step.
SCOPE = """
def select(root, paths, base_lock=None):
    reached = set()
    for path in paths:
        if path.startswith("crates/cadmpeg-asm/"):
            reached |= {"cadmpeg-asm", "cadmpeg-codec-sat", "cadmpeg-codec-step"}
        elif path.startswith("crates/cadmpeg-codec-sat/"):
            reached.add("cadmpeg-codec-sat")
    return {"rust": bool(reached), "packages": sorted(reached)}
"""


def manifest(name: str) -> str:
    return f'[package]\nname = "{name}"\n'


class DependentGoldens(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({
            check.SCOPE: SCOPE,
            "crates/cadmpeg-asm/Cargo.toml": manifest("cadmpeg-asm"),
            "crates/cadmpeg-asm/src/lib.rs": "\n",
            "crates/cadmpeg-codec-sat/Cargo.toml": manifest("cadmpeg-codec-sat"),
            "crates/cadmpeg-codec-sat/src/golden_tests.rs": "\n",
            "crates/cadmpeg-codec-step/Cargo.toml": manifest("cadmpeg-codec-step"),
            "crates/cadmpeg-codec-step/src/lib.rs": "\n",
        })
        self.repo.branch()

    def test_shared_crate_lists_dependent_goldens(self) -> None:
        self.repo.commit({"crates/cadmpeg-asm/src/lib.rs": "// body kind\n"})
        results = harness.run_check(check, self.repo.context())
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn("cadmpeg-codec-sat", results[0].details)
        self.assertNotIn("cadmpeg-codec-step", results[0].details)
        self.assertIn("run: cargo test -q --no-fail-fast -p cadmpeg-codec-sat golden",
                      results[0].details)

    def test_leaf_codec_passes(self) -> None:
        self.repo.commit({"crates/cadmpeg-codec-sat/src/lib.rs": "\n"})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])

    def test_tooling_only_change_passes(self) -> None:
        self.repo.commit({"scripts/tool.py": "\n"})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])

    def test_full_runs_and_reports_drift(self) -> None:
        self.repo.commit({"crates/cadmpeg-asm/src/lib.rs": "// body kind\n"})
        context = self.repo.context(full=True)
        context.run = lambda command, env=None: (101, "golden mismatch: body kind sheet -> solid")
        results = harness.run_check(check, context)
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn("golden mismatch: body kind sheet -> solid", results[0].details)


if __name__ == "__main__":
    unittest.main()
