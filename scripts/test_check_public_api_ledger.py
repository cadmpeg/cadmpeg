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


if __name__ == "__main__":
    unittest.main()
