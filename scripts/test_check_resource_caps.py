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


def census_doc(*, rows: list[str], overall: int, kinds: dict[str, int], crates: str) -> str:
    kind_rows = "\n".join(
        f"| {kind:<4} | meaning | {kinds.get(kind, 0)} |" for kind in caps.KINDS
    )
    table = "\n".join(rows)
    return f"""# Decode resource caps

## Production census

| Crate | File | Constant | Value | Kind | Enforcement |
| ----- | ---- | -------- | ----- | ---- | ----------- |
{table}

## Totals

Production `MAX_*` constants: {overall}.

| Kind | Meaning | Count |
| ---- | ------- | ----- |
{kind_rows}
|      | Total   | {overall} |

By crate: {crates}.
"""


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
                "docs/decode-resource-caps.md": census_doc(
                    rows=[
                        "| `demo` | `src/lib.rs` | `MAX_FOO` | `1` | 2 | note |",
                    ],
                    overall=1,
                    kinds={"2": 1},
                    crates="`demo` 1",
                ),
            }
        )
        self.assertTrue(
            any("MAX_BAR" in line and "missing from table" in line for line in findings),
            findings,
        )

    def test_wrong_total_fails(self) -> None:
        findings = self.check(
            {
                "crates/demo/src/lib.rs": "const MAX_FOO: usize = 1;\n",
                "docs/decode-resource-caps.md": census_doc(
                    rows=[
                        "| `demo` | `src/lib.rs` | `MAX_FOO` | `1` | 2 | note |",
                    ],
                    overall=99,
                    kinds={"2": 1},
                    crates="`demo` 1",
                ),
            }
        )
        self.assertTrue(
            any("stated overall 99" in line for line in findings),
            findings,
        )

    def test_checkout_under_src_counts_only_crate_sources(self) -> None:
        findings = self.check(
            {
                "crates/demo/src/lib.rs": "const MAX_FOO: usize = 1;\n",
                "crates/demo/fuzz_targets/target.rs": "const MAX_FUZZ: usize = 2;\n",
                "docs/decode-resource-caps.md": census_doc(
                    rows=[
                        "| `demo` | `src/lib.rs` | `MAX_FOO` | `1` | 2 | note |",
                    ],
                    overall=1,
                    kinds={"2": 1},
                    crates="`demo` 1",
                ),
            },
            parent="src/cadmpeg",
        )
        self.assertEqual(findings, [])


if __name__ == "__main__":
    unittest.main()
