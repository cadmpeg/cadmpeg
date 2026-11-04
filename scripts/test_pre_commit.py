#!/usr/bin/env python3
"""Check hook routing without running Cargo, Python checkers, or Git writes."""

import os
from pathlib import Path
import subprocess
import unittest

HOOK = Path(__file__).resolve().parent.parent / ".githooks/pre-commit"
STUBS = """
git() { printf '%s\n' "$STAGED"; }
python3() {
  if [ "$1" = scripts/staged-crates.py ]; then
    if [ "$2" = --owners ]; then printf '%s\n' "$OWNERS"; else printf '%s\n' "$CRATES"; fi
    return
  fi
  printf 'python3 %s\n' "$*"
}
cargo() { printf 'cargo %s\n' "$*"; }
"""


def run_hook(staged, owners="", crates=""):
    result = subprocess.run(
        ["sh"], input=STUBS + HOOK.read_text(), text=True, capture_output=True,
        env={**os.environ, "STAGED": staged, "OWNERS": owners, "CRATES": crates},
        check=True,
    )
    return result.stdout.splitlines()


class CheckerRouting(unittest.TestCase):
    def test_inputs_run_checks_but_only_checker_changes_run_checker_tests(self):
        cases = [
            ("crates/cadmpeg-codec-step/src/reader.rs", set()),
            ("crates/cadmpeg-registry/docs/dialects.toml", set()),
            ("README.md", set()),
            ("scripts/check-dialects.py", set()),
            ("scripts/dialect_support_data.py",
             {"test_check_dialect_support.py", "test_render_format_support.py"}),
            ("scripts/check-dialect-support.py", {"test_check_dialect_support.py"}),
            ("scripts/render-format-support.py", {"test_render_format_support.py"}),
        ]
        for staged, expected in cases:
            with self.subTest(staged=staged):
                lines = run_hook(staged)
                tests = {
                    line.rsplit(" ", 1)[1] for line in lines
                    if line.startswith("python3 -m unittest discover ")
                }
                self.assertEqual(tests, expected)
                if staged in {"crates/cadmpeg-registry/docs/dialects.toml",
                              "scripts/dialect_support_data.py"}:
                    self.assertIn("python3 scripts/check-dialect-support.py", lines)
                    self.assertIn("python3 scripts/render-format-support.py --check", lines)

    def test_codec_source_runs_cheap_checks_and_no_whole_tree_scan(self):
        lines = run_hook("crates/cadmpeg-codec-step/src/reader.rs")
        python = {line for line in lines if line.startswith("python3 ")}
        self.assertEqual(python, {"python3 scripts/check-dialect-support.py"})


class RustGate(unittest.TestCase):
    def cargo(self, staged, owners="", crates=""):
        return [line for line in run_hook(staged, owners, crates) if line.startswith("cargo ")]

    def test_no_rust_input_runs_no_cargo(self):
        for staged in ["README.md", "crates/cadmpeg-registry/docs/dialect-support.toml"]:
            with self.subTest(staged=staged):
                self.assertEqual(self.cargo(staged), [])

    def test_crate_source_formats_owners_and_lints_dependents(self):
        self.assertEqual(
            self.cargo("crates/cadmpeg-codec-step/src/reader.rs",
                       owners="cadmpeg-codec-step",
                       crates="cadmpeg\ncadmpeg-codec-step"),
            ["cargo fmt -p cadmpeg-codec-step --check",
             "cargo clippy -p cadmpeg -p cadmpeg-codec-step --all-targets"],
        )

    def test_manifest_or_toolchain_selects_workspace(self):
        for staged in ["Cargo.lock", "crates/cadmpeg-ir/Cargo.toml", "rust-toolchain.toml",
                       ".cargo/config.toml"]:
            with self.subTest(staged=staged):
                self.assertEqual(
                    self.cargo(staged),
                    ["cargo fmt --all --check", "cargo clippy --workspace --all-targets"],
                )

    def test_unowned_rust_file_selects_workspace(self):
        self.assertEqual(
            self.cargo("crates/cadmpeg-fuzz/fuzz_targets/step.rs",
                       owners="WORKSPACE", crates="WORKSPACE"),
            ["cargo fmt --all --check", "cargo clippy --workspace --all-targets"],
        )


if __name__ == "__main__":
    unittest.main()
