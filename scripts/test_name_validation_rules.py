#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Tests for validation rule references and admission candidates."""

import contextlib
import importlib.util
import io
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "name_validation_rules", Path(__file__).with_name("name-validation-rules.py")
)
assert SPEC and SPEC.loader
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)


class RuleChecks(unittest.TestCase):
    def run_check(self, source: str, mode: str) -> tuple[int, str, str]:
        path = "crates/cadmpeg-ir/src/validate.rs"
        scan = checker.scan_source(path, source)
        stdout, stderr = io.StringIO(), io.StringIO()
        with patch.object(checker, "rust_files", return_value=[path]), \
             patch.object(checker, "scan_file", return_value=scan), \
             patch.object(sys, "argv", ["checker", mode]), \
             contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            status = checker.main()
        return status, stdout.getvalue(), stderr.getvalue()

    def test_reference_requires_a_stated_rule(self) -> None:
        source = '''fn f() {
    record(Check::WireTopology, rule!("wire.present", "message"));
    let a = rule_name!("wire.present");
    let b = rule_name!("wire.missing");
}
'''
        status, output, _ = self.run_check(source, "--check")
        self.assertEqual(status, 1)
        self.assertIn(":4: `wire.missing` names no rule any finding states", output)
        self.assertNotIn("wire.present", output)

    def test_admission_lists_core_rules(self) -> None:
        source = 'fn f() { record(Check::WireTopology, rule!("wire.present", "message")); }'
        status, output, errors = self.run_check(source, "--admission")
        self.assertEqual(status, 0)
        self.assertEqual(output, '    crate::rule_name!("wire.present"),\n')
        self.assertEqual(errors, "")


if __name__ == "__main__":
    unittest.main()
