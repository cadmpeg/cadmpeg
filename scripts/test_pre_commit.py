#!/usr/bin/env python3
"""Check hook routing without running Cargo, Python checkers, or Git writes."""

import os
from pathlib import Path
import subprocess
import unittest


class CheckerRouting(unittest.TestCase):
    def test_inputs_run_checks_but_only_checker_changes_run_checker_tests(self):
        hook = Path(__file__).resolve().parent.parent / ".githooks/pre-commit"
        commands = """
git() { printf '%s\n' "$STAGED"; }
python3() { printf 'python3 %s\n' "$*"; }
cargo() { :; }
"""
        cases = [
            ("crates/cadmpeg-codec-step/src/reader.rs", set()),
            ("crates/cadmpeg-registry/docs/dialects.toml", set()),
            ("README.md", set()),
            ("scripts/check-dialects.py", {"test_check_dialects.py"}),
            ("scripts/test_check_dialects.py", {"test_check_dialects.py"}),
            ("scripts/dialect_support_data.py",
             {"test_check_dialect_support.py", "test_render_format_support.py"}),
            ("scripts/check-dialect-support.py", {"test_check_dialect_support.py"}),
            ("scripts/render-format-support.py", {"test_render_format_support.py"}),
        ]
        for staged, expected in cases:
            with self.subTest(staged=staged):
                result = subprocess.run(
                    ["sh"], input=commands + hook.read_text(), text=True,
                    capture_output=True, env={**os.environ, "STAGED": staged},
                    check=True,
                )
                tests = {
                    line.rsplit(" ", 1)[1] for line in result.stdout.splitlines()
                    if line.startswith("python3 -m unittest discover ")
                }
                self.assertEqual(tests, expected)
                if staged in {"crates/cadmpeg-registry/docs/dialects.toml", "scripts/dialect_support_data.py"}:
                    self.assertIn("python3 scripts/check-dialect-support.py\n", result.stdout)
                    self.assertIn("python3 scripts/render-format-support.py --check\n",
                                  result.stdout)
                if staged == "crates/cadmpeg-codec-step/src/reader.rs":
                    self.assertIn("python3 scripts/check-dialects.py\n", result.stdout)
                    self.assertIn("python3 scripts/check-source-policy.py\n", result.stdout)


if __name__ == "__main__":
    unittest.main()
