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
                    self.assertIn("python3 scripts/check-resource-caps.py\n", result.stdout)


class RustTargetRouting(unittest.TestCase):
    def test_scoped_tests_exclude_benches_and_admit_binary_only_packages(self):
        hook = Path(__file__).resolve().parent.parent / ".githooks/pre-commit"
        commands = """
git() { printf '%s\n' "$STAGED"; }
python3() {
  if [ "$1" = scripts/staged-crates.py ]; then
    printf '%s\n' "$TASK_SCOPE"
  fi
}
cargo() {
  if [ "$1" = nextest ] && [ "$2" = --version ]; then
    return "$TASK_NEXTEST_EXIT"
  fi
  printf 'cargo %s\n' "$*"
}
"""
        for scope in ["cadmpeg", "cadmpeg-codec-step", "cadmpeg cadmpeg-codec-step"]:
            for installed in [True, False]:
                with self.subTest(scope=scope, nextest=installed):
                    result = subprocess.run(
                        ["sh"], input=commands + hook.read_text(), text=True,
                        capture_output=True, check=True,
                        env={**os.environ, "STAGED": "crates/cadmpeg/src/main.rs",
                             "TASK_SCOPE": scope,
                             "TASK_NEXTEST_EXIT": "0" if installed else "1"},
                    )
                    cargo = [line for line in result.stdout.splitlines()
                             if line.startswith("cargo ")]
                    flags = "".join(f" -p {package}" for package in scope.split())
                    self.assertIn(f"cargo clippy{flags} --all-targets", cargo)
                    if installed:
                        self.assertIn(
                            "cargo nextest run --cargo-quiet --status-level fail "
                            f"--no-tests pass{flags} --bins --tests", cargo,
                        )
                    else:
                        self.assertIn(f"cargo test{flags} --bins --tests", cargo)


if __name__ == "__main__":
    unittest.main()
