#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-public-api-ledger.py``."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-public-api-ledger.py")
SPEC = importlib.util.spec_from_file_location("check_public_api_ledger", SCRIPT)
assert SPEC and SPEC.loader
ledger = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ledger
SPEC.loader.exec_module(ledger)


def _row(**overrides: str) -> dict[str, str]:
    row = {
        "commit": "0123456789abcdef0123456789abcdef01234567",
        "crate": "cadmpeg-ir",
        "kind": "signature",
        "item": "cadmpeg_ir::Foo",
        "reason": "test",
    }
    row.update(overrides)
    return row


class ShapeChecks(unittest.TestCase):
    def test_missing_top_level(self) -> None:
        failures = ledger.check_shape({"change": []})
        self.assertIn("ledger missing baseline_commit", failures)
        self.assertIn("ledger missing api_baseline_dir", failures)
        self.assertIn("ledger missing measured_at", failures)

    def test_unknown_kind(self) -> None:
        data = {
            "baseline_commit": "0123456789abcdef0123456789abcdef01234567",
            "api_baseline_dir": "docs/api-baseline",
            "measured_at": "0123456789abcdef0123456789abcdef01234567",
            "change": [_row(kind="rename")],
        }
        failures = ledger.check_shape(data)
        self.assertEqual(failures, ["change[0] unknown kind 'rename'"])

    def test_variant_kind_is_valid(self) -> None:
        data = {
            "baseline_commit": "0123456789abcdef0123456789abcdef01234567",
            "api_baseline_dir": "docs/api-baseline",
            "measured_at": "0123456789abcdef0123456789abcdef01234567",
            "change": [_row(kind="variant")],
        }
        self.assertEqual(ledger.check_shape(data), [])

    def test_short_sha(self) -> None:
        data = {
            "baseline_commit": "abc",
            "api_baseline_dir": "docs/api-baseline",
            "measured_at": "0123456789abcdef0123456789abcdef01234567",
            "change": [],
        }
        failures = ledger.check_shape(data)
        self.assertTrue(any("not a 40-character" in item for item in failures))


class SnapshotChecks(unittest.TestCase):
    def test_requires_generated_at_header(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            baseline = root / "docs" / "api-baseline"
            baseline.mkdir(parents=True)
            (baseline / "cadmpeg-core.txt").write_text(
                "pub mod cadmpeg_core\n", encoding="utf-8"
            )
            failures = ledger.check_snapshots(
                {"api_baseline_dir": "docs/api-baseline"}, root
            )
            self.assertEqual(len(failures), 1)
            self.assertIn("first line must be", failures[0])

    def test_accepts_header(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            baseline = root / "docs" / "api-baseline"
            baseline.mkdir(parents=True)
            (baseline / "cadmpeg-core.txt").write_text(
                "# generated at abcdef0\npub mod cadmpeg_core\n",
                encoding="utf-8",
            )
            self.assertEqual(
                ledger.check_snapshots(
                    {"api_baseline_dir": "docs/api-baseline"}, root
                ),
                [],
            )

    def test_header_stripping_comparison(self) -> None:
        generated = b"pub mod cadmpeg_core\npub struct Widget\n"
        snapshot = b"# generated at abcdef0\n" + generated
        self.assertTrue(ledger.snapshot_matches(snapshot, generated))
        self.assertFalse(
            ledger.snapshot_matches(snapshot, generated + b"pub enum New {}\n")
        )

    def test_diff_selection_uses_staged_source_crates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp)
            (baseline / "cadmpeg-core.txt").touch()
            (baseline / "cadmpeg-ir.txt").touch()
            self.assertEqual(
                ledger.crates_for_diff(
                    baseline,
                    {
                        "crates/cadmpeg-core/src/lib.rs",
                        "docs/api-baseline/cadmpeg-ir.txt",
                    },
                ),
                ["cadmpeg-core"],
            )
            self.assertEqual(
                ledger.crates_for_diff(baseline, set()),
                ["cadmpeg-core", "cadmpeg-ir"],
            )


REALISTIC_LEDGER_DIFF = """\
diff --git a/docs/public-api-ledger.toml b/docs/public-api-ledger.toml
index 1111111..2222222 100644
--- a/docs/public-api-ledger.toml
+++ b/docs/public-api-ledger.toml
@@ -20,0 +21,7 @@
+[[change]]
+commit = "0123456789abcdef0123456789abcdef01234567"
+crate = "cadmpeg-core"
+kind = "addition"
+item = "cadmpeg_core::Widget"
+reason = "test fixture"
"""


class StagedCouplingChecks(unittest.TestCase):
    def test_staged_row_with_snapshot_passes(self) -> None:
        staged = {
            "docs/public-api-ledger.toml",
            "docs/api-baseline/cadmpeg-core.txt",
        }
        self.assertEqual(ledger.check_staged_coupling(REALISTIC_LEDGER_DIFF, staged), [])

    def test_staged_row_without_snapshot_fails_and_names_crate(self) -> None:
        failures = ledger.check_staged_coupling(
            REALISTIC_LEDGER_DIFF, {"docs/public-api-ledger.toml"}
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("cadmpeg-core", failures[0])
        self.assertIn("cargo +nightly public-api -p cadmpeg-core", failures[0])

    def test_nothing_staged_passes(self) -> None:
        self.assertEqual(ledger.check_staged_coupling("", set()), [])

    def test_crate_outside_added_change_block_is_ignored(self) -> None:
        diff = REALISTIC_LEDGER_DIFF + "\n+[[metadata]]\n+crate = \"not-a-change\"\n"
        self.assertEqual(ledger.added_change_crates(diff), {"cadmpeg-core"})


if __name__ == "__main__":
    unittest.main()
