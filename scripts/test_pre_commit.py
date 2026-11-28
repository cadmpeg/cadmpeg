#!/usr/bin/env python3
"""Check hook routing without running Cargo, Python checkers, or Git writes."""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest import mock

HOOK = Path(__file__).resolve().parent.parent / ".githooks/pre-commit"
STUBS = """
git() { printf '%s\n' "$STAGED"; }
python3() {
  if [ "$1" = scripts/staged-crates.py ]; then
    printf '%s\n' "$OWNERS"
    return
  fi
  printf 'python3 %s\n' "$*"
}
cargo() { printf 'cargo %s\n' "$*"; }
rustfmt() { printf 'rustfmt %s\n' "$*"; }
"""


def run_hook(staged, owners=""):
    result = subprocess.run(
        ["sh"], input=STUBS + HOOK.read_text(), text=True, capture_output=True,
        env={**os.environ, "STAGED": staged, "OWNERS": owners},
        check=True,
    )
    return result.stdout.splitlines()


class StagedCrates(unittest.TestCase):
    def owners(self, paths):
        selector_path = HOOK.parent.parent / "scripts/staged-crates.py"
        spec = importlib.util.spec_from_file_location("staged_crates", selector_path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        metadata = {"packages": [{"name": "cadmpeg-codec-step",
                    "manifest_path": "/repo/crates/cadmpeg-codec-step/Cargo.toml"}]}
        def command(args):
            if args[:2] == ["git", "rev-parse"]:
                return "/repo\n"
            if args[:2] == ["git", "diff"]:
                return "\n".join(paths)
            return json.dumps(metadata)
        output = io.StringIO()
        with mock.patch.object(module, "run", side_effect=command):
            with contextlib.redirect_stdout(output):
                self.assertEqual(module.main(), 0)
        return output.getvalue().splitlines()

    def test_fuzz_and_workspace_sources(self):
        self.assertEqual(self.owners(["crates/cadmpeg-fuzz/fuzz_targets/creo_datum.rs"]), [])
        self.assertEqual(self.owners(["crates/cadmpeg-fuzz/fuzz_targets/creo_datum.rs",
                                      "crates/cadmpeg-codec-step/src/reader.rs"]),
                         ["cadmpeg-codec-step"])
        self.assertEqual(self.owners(["tools/example.rs"]), ["WORKSPACE"])


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
    def cargo(self, staged, owners=""):
        return [line for line in run_hook(staged, owners) if line.startswith("cargo ")]

    def test_no_rust_input_runs_no_cargo(self):
        for staged in ["README.md", "crates/cadmpeg-registry/docs/dialect-support.toml"]:
            with self.subTest(staged=staged):
                self.assertEqual(self.cargo(staged), [])

    def test_crate_source_formats_and_lints_owners(self):
        self.assertEqual(
            self.cargo("crates/cadmpeg-codec-step/src/reader.rs",
                       owners="cadmpeg-codec-step"),
            ["cargo fmt -p cadmpeg-codec-step --check",
             "cargo clippy -p cadmpeg-codec-step --all-targets"],
        )

    def test_manifest_or_toolchain_selects_workspace(self):
        for staged in ["Cargo.lock", "crates/cadmpeg-ir/Cargo.toml", "rust-toolchain.toml",
                       ".cargo/config.toml"]:
            with self.subTest(staged=staged):
                self.assertEqual(
                    self.cargo(staged),
                    ["cargo fmt --all --check", "cargo clippy --workspace --all-targets"],
                )

    def test_fuzz_source_does_not_select_workspace_cargo(self):
        self.assertEqual(
            self.cargo("crates/cadmpeg-fuzz/fuzz_targets/creo_datum.rs"),
            [],
        )

    def test_only_staged_fuzz_rust_is_formatted(self):
        lines = run_hook("crates/cadmpeg-fuzz/fuzz_targets/creo_datum.rs\nREADME.md")
        self.assertIn("rustfmt --edition 2021 --check crates/cadmpeg-fuzz/fuzz_targets/creo_datum.rs", lines)

    def test_deleted_fuzz_file_is_not_formatted(self):
        lines = run_hook("crates/cadmpeg-fuzz/fuzz_targets/deleted.rs")
        self.assertFalse(any(line.startswith("rustfmt ") for line in lines))

    def test_fuzz_manifest_does_not_select_main_workspace(self):
        self.assertEqual(
            self.cargo("crates/cadmpeg-fuzz/Cargo.toml"),
            [],
        )

    def test_fuzz_and_workspace_sources_keep_workspace_gate_scoped(self):
        self.assertEqual(
            self.cargo("crates/cadmpeg-fuzz/fuzz_targets/creo_datum.rs\n"
                       "crates/cadmpeg-codec-step/src/reader.rs", "cadmpeg-codec-step"),
            ["cargo fmt -p cadmpeg-codec-step --check",
             "cargo clippy -p cadmpeg-codec-step --all-targets"],
        )

    def test_unowned_rust_file_selects_workspace(self):
        self.assertEqual(
            self.cargo("tools/example.rs", owners="WORKSPACE"),
            ["cargo fmt --all --check", "cargo clippy --workspace --all-targets"],
        )


if __name__ == "__main__":
    unittest.main()
