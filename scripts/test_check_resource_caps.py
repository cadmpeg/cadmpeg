#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-resource-caps.py``."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("check-resource-caps.py")
SPEC = importlib.util.spec_from_file_location("check_resource_caps", SCRIPT)
assert SPEC and SPEC.loader
caps = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = caps
SPEC.loader.exec_module(caps)


def census_doc(*rows: str, extra: str = "") -> str:
    table = "\n".join(rows)
    return f"""# Decode resource caps

## Production census

| Crate | File | Constant | Value | Kind | Enforcement |
| ----- | ---- | -------- | ----- | ---- | ----------- |
{table}
{extra}
## Routing
"""


FOO_ROW = "| `demo` | `src/lib.rs` | `MAX_FOO` | `1` | 2 | note |"


class ResourceCapsCensus(unittest.TestCase):
    def check(self, files: dict[str, str], parent: str = "") -> list[str]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / parent
            for name, source in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")
            with patch.object(caps, "ROOT", root):
                return caps.check_tree(root)

    def test_missing_row_fails(self) -> None:
        findings = self.check(
            {
                "crates/demo/src/lib.rs": (
                    "const MAX_FOO: usize = 1;\nconst MAX_BAR: usize = 2;\n"
                ),
                "docs/decode-resource-caps.md": census_doc(FOO_ROW),
            }
        )
        self.assertTrue(
            any("MAX_BAR" in line and "missing from table" in line for line in findings),
            findings,
        )

    def test_row_without_constant_fails(self) -> None:
        findings = self.check(
            {
                "crates/demo/src/lib.rs": "const MAX_FOO: usize = 1;\n",
                "docs/decode-resource-caps.md": census_doc(
                    FOO_ROW, "| `demo` | `src/lib.rs` | `MAX_GONE` | `1` | 2 | note |"
                ),
            }
        )
        self.assertTrue(
            any("MAX_GONE" in line and "no source constant" in line for line in findings),
            findings,
        )

    def test_stored_totals_fail(self) -> None:
        findings = self.check(
            {
                "crates/demo/src/lib.rs": "const MAX_FOO: usize = 1;\n",
                "docs/decode-resource-caps.md": census_doc(
                    FOO_ROW,
                    extra="\n## Totals\n\nProduction `MAX_*` constants: 1.\n",
                ),
            }
        )
        self.assertEqual(len(findings), 2, findings)
        self.assertTrue(all("stored total" in line for line in findings), findings)

    def test_session_ceiling_kind_fails(self) -> None:
        findings = self.check(
            {
                "crates/demo/src/lib.rs": "const MAX_FOO: usize = 1;\n",
                "docs/decode-resource-caps.md": census_doc(
                    "| `demo` | `src/lib.rs` | `MAX_FOO` | `1` | 3 | note |"
                ),
            }
        )
        self.assertTrue(any("is kind 3" in line for line in findings), findings)

    def test_counts_are_computed(self) -> None:
        constants = [
            caps.Constant("demo", "src/lib.rs", "MAX_FOO", 1, "crates/demo/src/lib.rs"),
            caps.Constant("demo", "src/lib.rs", "MAX_BAR", 2, "crates/demo/src/lib.rs"),
            caps.Constant("other", "src/lib.rs", "MAX_BAZ", 1, "crates/other/src/lib.rs"),
        ]
        rows = [
            caps.TableRow("demo", "src/lib.rs", "MAX_FOO", "2", 1),
            caps.TableRow("demo", "src/lib.rs", "MAX_BAR", "4", 2),
            caps.TableRow("other", "src/lib.rs", "MAX_BAZ", "4", 3),
        ]
        self.assertEqual(
            caps.counts(constants, rows),
            [
                "production MAX_* constants: 3",
                "by kind: 1 0, 2 1, 3 0, 4 2, N 0",
                "by crate: demo 2, other 1",
            ],
        )

    def test_checkout_under_src_counts_only_crate_sources(self) -> None:
        findings = self.check(
            {
                "crates/demo/src/lib.rs": "const MAX_FOO: usize = 1;\n",
                "crates/demo/fuzz_targets/target.rs": "const MAX_FUZZ: usize = 2;\n",
                "docs/decode-resource-caps.md": census_doc(FOO_ROW),
            },
            parent="src/cadmpeg",
        )
        self.assertEqual(findings, [])


if __name__ == "__main__":
    unittest.main()
