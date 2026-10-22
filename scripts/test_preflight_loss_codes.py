#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_loss_codes.py``. These never run cargo."""

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
check = harness.load("preflight_loss_codes")

LOSS = "crates/cadmpeg-codec-f3d/src/loss.rs"
BEFORE = "pub enum F3dLossCode {\n    JointUnprojected,\n}\n"
AFTER = "pub enum F3dLossCode {\n    JointUnprojected,\n    ComponentDefinitionRevised,\n}\n"


class LossCodes(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({LOSS: BEFORE})
        self.repo.branch()

    def test_untested_code_warns(self) -> None:
        self.repo.commit({LOSS: AFTER, "crates/cadmpeg-codec-f3d/src/loss/tests.rs":
                          "ComponentDefinitionRevised\n"})
        results = harness.run_check(check, self.repo.context())
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn(f"{LOSS}: ComponentDefinitionRevised", results[0].details)

    def test_emission_test_passes(self) -> None:
        self.repo.commit({LOSS: AFTER, "crates/cadmpeg-codec-f3d/src/design/tests.rs":
                          "assert!(codes.contains(&F3dLossCode::ComponentDefinitionRevised));\n"})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])

    def test_untracked_test_counts(self) -> None:
        self.repo.commit({LOSS: AFTER})
        self.repo.write({"crates/cadmpeg-codec-f3d/src/meta/tests.rs":
                         "F3dLossCode::ComponentDefinitionRevised\n"})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])

    def test_inline_test_module_counts(self) -> None:
        source = ("pub fn bind() -> Vec<F3dLossCode> { Vec::new() }\n\n"
                  "#[cfg(test)]\nmod tests {\n    use super::*;\n\n"
                  "    #[test]\n    fn revised() {\n        let close = '}';\n"
                  "        assert!(bind().contains(&F3dLossCode::ComponentDefinitionRevised));\n"
                  "    }\n}\n")
        self.repo.commit({LOSS: AFTER,
                          "crates/cadmpeg-codec-f3d/src/design/components.rs": source})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["PASS"])

    def test_mention_outside_test_code_warns(self) -> None:
        source = ("// F3dLossCode::ComponentDefinitionRevised is emitted below.\n"
                  "pub fn bind() -> F3dLossCode { F3dLossCode::ComponentDefinitionRevised }\n\n"
                  "#[cfg(test)]\nmod tests;\n\n#[cfg(test)]\nmod more {\n"
                  "    // ComponentDefinitionRevised\n    const NAME: &str = \"}\";\n}\n")
        self.repo.commit({LOSS: AFTER,
                          "crates/cadmpeg-codec-f3d/src/design/components.rs": source})
        self.assertEqual(harness.statuses(harness.run_check(check, self.repo.context())), ["WARN"])


if __name__ == "__main__":
    unittest.main()
