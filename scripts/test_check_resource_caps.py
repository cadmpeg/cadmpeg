#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-resource-caps.py``."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-resource-caps.py")
SPEC = importlib.util.spec_from_file_location("check_resource_caps", SCRIPT)
assert SPEC and SPEC.loader
caps = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = caps
SPEC.loader.exec_module(caps)


class MarkerCensus(unittest.TestCase):
    def scan(self, source: str):
        return caps.scan_markers("crates/demo/src/lib.rs", source)

    def test_markers_bind_each_occurrence(self) -> None:
        source = """// resource-cap: MAX_FOO = 2
const MAX_FOO: usize = 1;
fn f() {
    // resource-cap: MAX_FOO = 4
    const MAX_FOO: usize = 2;
}
"""
        constants, findings = self.scan(source)
        self.assertEqual(findings, [])
        self.assertEqual([c.kind for c in constants], ["2", "4"])

    def test_missing_stale_duplicate_and_invalid_markers_fail(self) -> None:
        for source, detail in [
            ("const MAX_FOO: usize = 1;\n", "missing"),
            ("// resource-cap: MAX_FOO = 2\nfn f() {}\n", "stale"),
            ("// resource-cap: MAX_FOO = 2\n// resource-cap: MAX_FOO = 4\nconst MAX_FOO: usize = 1;\n", "duplicate"),
            ("// resource-cap: MAX_BAR = 2\nconst MAX_FOO: usize = 1;\n", "invalid"),
            ("// resource-cap: MAX_FOO = 9\nconst MAX_FOO: usize = 1;\n", "invalid"),
            ("// resource-cap: MAX_FOO = 3\nconst MAX_FOO: usize = 1;\n", "kind 3"),
            ("// resource-cap: MAX_FOO = 2\nconst MAX_FOO: usize = 1; const MAX_FOO: usize = 2;\n", "reused"),
        ]:
            with self.subTest(source=source):
                _, findings = self.scan(source)
                self.assertTrue(any(detail in finding for finding in findings), findings)

    def test_test_items_and_literal_decoys_are_ignored(self) -> None:
        source = '''const S: &str = "// resource-cap: MAX_FAKE = 2";
#[cfg(test)]
mod tests {
    // resource-cap: MAX_TEST = 2
    const MAX_TEST: usize = 1;
}
#[cfg(test)]
// resource-cap: MAX_LOCAL = 4
const MAX_LOCAL: usize = 1;
'''
        constants, findings = self.scan(source)
        self.assertEqual(constants, [])
        self.assertEqual(findings, [])


class ResourceCapsCensus(unittest.TestCase):
    def test_stored_totals_fail(self) -> None:
        findings = caps.stored_totals("## Totals\nProduction `MAX_*` constants: 1.\n")
        self.assertEqual(len(findings), 2)

    def test_counts_use_classifications(self) -> None:
        constants = [
            caps.Constant("crates/demo/src/lib.rs", "MAX_A", 1, "2"),
            caps.Constant("crates/demo/src/lib.rs", "MAX_B", 3, "4"),
        ]
        self.assertEqual(caps.counts(constants), [
            "production MAX_* constants: 2",
            "by kind: 1 0, 2 1, 3 0, 4 1, N 0",
            "by crate: demo 2",
        ])


if __name__ == "__main__":
    unittest.main()
