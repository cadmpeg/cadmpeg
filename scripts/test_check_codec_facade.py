#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-codec-facade.py``."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-codec-facade.py")
SPEC = importlib.util.spec_from_file_location("check_codec_facade", SCRIPT)
assert SPEC and SPEC.loader
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)


def _row(
    crate_id: str = "demo",
    names: list[str] | None = None,
    hidden: list[str] | None = None,
    fuzz: str = "required",
    fuzz_reason: str | None = None,
) -> checker.CrateRow:
    return checker.CrateRow(
        crate_id=crate_id,
        names=tuple(names or ["DemoCodec"]),
        hidden=tuple(hidden or ()),
        fuzz=fuzz,
        fuzz_reason=fuzz_reason,
    )


def _ledger(rows: list[str]) -> str:
    header = (
        "# test ledger\n"
        "# scripts/check-codec-facade.py is the oracle.\n"
    )
    return header + "\n".join(rows) + "\n"


def _crate_toml(
    crate_id: str,
    names: list[str],
    fuzz: str = "required",
    hidden: list[str] | None = None,
    fuzz_reason: str | None = None,
) -> str:
    name_list = ", ".join(f'"{name}"' for name in names)
    lines = [
        "[[crate]]",
        f'id = "{crate_id}"',
        f"names = [{name_list}]",
    ]
    if hidden:
        hidden_list = ", ".join(f'"{name}"' for name in hidden)
        lines.append(f"hidden = [{hidden_list}]")
    lines.append(f'fuzz = "{fuzz}"')
    if fuzz_reason is not None:
        lines.append(f'fuzz_reason = "{fuzz_reason}"')
    return "\n".join(lines) + "\n"


def _tree(crates: dict[str, str], ledger: str) -> Path:
    root = Path(tempfile.mkdtemp())
    (root / "docs").mkdir()
    (root / "docs" / "codec-facade.toml").write_text(ledger, encoding="utf-8")
    for crate_id, lib in crates.items():
        path = root / "crates" / f"cadmpeg-codec-{crate_id}" / "src"
        path.mkdir(parents=True)
        (path / "lib.rs").write_text(lib, encoding="utf-8")
    return root


ALLOWED_LIB = """\
pub(crate) mod decode;

#[doc(hidden)]
pub mod fuzz;

#[derive(Debug)]
pub struct DemoCodec;

impl DemoCodec {
    pub fn decode(&self) {}
}
"""


class ParseLib(unittest.TestCase):
    def test_pub_use_brace_records_both_names(self) -> None:
        items = checker.collect_root_pubs(
            "pub use evaluation::{saved_body_census_evidence, BodyCensusEvidence};\n"
        )
        self.assertEqual(
            [item.name for item in items],
            ["saved_body_census_evidence", "BodyCensusEvidence"],
        )

    def test_pub_use_alias(self) -> None:
        items = checker.collect_root_pubs("pub use foo::Inner as Outer;\n")
        self.assertEqual([item.name for item in items], ["Outer"])

    def test_pub_crate_mod_is_ignored(self) -> None:
        items = checker.collect_root_pubs("pub(crate) mod decode;\npub struct DemoCodec;\n")
        self.assertEqual([item.name for item in items], ["DemoCodec"])

    def test_inherent_pub_fn_is_ignored(self) -> None:
        items = checker.collect_root_pubs(
            "pub struct DemoCodec;\n"
            "impl DemoCodec {\n"
            "    pub fn decode(&self) {}\n"
            "}\n"
        )
        self.assertEqual([item.name for item in items], ["DemoCodec"])

    def test_doc_hidden_attaches(self) -> None:
        items = checker.collect_root_pubs("#[doc(hidden)]\npub mod fuzz;\n")
        self.assertEqual(len(items), 1)
        self.assertEqual(items[0].name, "fuzz")
        self.assertTrue(items[0].hidden)
        self.assertEqual(items[0].kind, "mod")

    def test_cfg_test_pub_is_ignored(self) -> None:
        items = checker.collect_root_pubs("#[cfg(test)]\npub fn helper() {}\n")
        self.assertEqual(items, [])


class CheckCrate(unittest.TestCase):
    def test_allowed_root_passes(self) -> None:
        items = checker.collect_root_pubs(ALLOWED_LIB)
        self.assertEqual(checker.check_crate(_row(), items), [])

    def test_extra_pub_use_struct_mod_fail(self) -> None:
        items = checker.collect_root_pubs(
            "pub struct DemoCodec;\n"
            "pub struct ExtraType;\n"
            "pub use inner::Helper;\n"
            "pub mod extra;\n"
            "#[doc(hidden)]\n"
            "pub mod fuzz;\n"
        )
        failures = checker.check_crate(_row(), items)
        self.assertIn("demo: extra pub name ExtraType", failures)
        self.assertIn("demo: extra pub name Helper", failures)
        self.assertIn("demo: extra pub name extra", failures)

    def test_missing_required_fuzz_fails(self) -> None:
        items = checker.collect_root_pubs("pub struct DemoCodec;\n")
        self.assertIn(
            "demo: fuzz is required but missing",
            checker.check_crate(_row(), items),
        )

    def test_fuzz_not_hidden_fails(self) -> None:
        items = checker.collect_root_pubs("pub struct DemoCodec;\npub mod fuzz;\n")
        self.assertIn(
            "demo: fuzz is required but not #[doc(hidden)]",
            checker.check_crate(_row(), items),
        )

    def test_fuzz_absent_with_pub_mod_fails(self) -> None:
        items = checker.collect_root_pubs(
            "pub struct DemoCodec;\n#[doc(hidden)]\npub mod fuzz;\n"
        )
        failures = checker.check_crate(
            _row(fuzz="absent", fuzz_reason="targets call DemoCodec"),
            items,
        )
        self.assertIn("demo: fuzz is absent but pub mod fuzz exists", failures)

    def test_hidden_extra_without_doc_hidden_fails(self) -> None:
        items = checker.collect_root_pubs(
            "pub struct DemoCodec;\n"
            "pub fn saved_body_census_evidence() {}\n"
            "#[doc(hidden)]\n"
            "pub mod fuzz;\n"
        )
        failures = checker.check_crate(
            _row(hidden=["saved_body_census_evidence"]),
            items,
        )
        self.assertIn(
            "demo: hidden name saved_body_census_evidence is not #[doc(hidden)]",
            failures,
        )

    def test_stale_allowlist_name_fails(self) -> None:
        items = checker.collect_root_pubs(
            "pub struct DemoCodec;\n#[doc(hidden)]\npub mod fuzz;\n"
        )
        failures = checker.check_crate(_row(names=["DemoCodec", "Gone"]), items)
        self.assertIn("demo: missing pub name Gone", failures)


class CheckTree(unittest.TestCase):
    def test_allowed_tree_passes(self) -> None:
        root = _tree(
            {"demo": ALLOWED_LIB},
            _ledger([_crate_toml("demo", ["DemoCodec"])]),
        )
        self.assertEqual(checker.check(root), [])

    def test_crate_dir_without_ledger_row_fails(self) -> None:
        root = _tree(
            {
                "demo": ALLOWED_LIB,
                "other": "pub struct OtherCodec;\n#[doc(hidden)]\npub mod fuzz;\n",
            },
            _ledger([_crate_toml("demo", ["DemoCodec"])]),
        )
        failures = checker.check(root)
        self.assertIn("ledger missing crate other", failures)

    def test_fuzz_absent_without_reason_fails(self) -> None:
        root = _tree(
            {"demo": "pub struct DemoCodec;\n"},
            _ledger([_crate_toml("demo", ["DemoCodec"], fuzz="absent")]),
        )
        failures = checker.check(root)
        self.assertIn("demo: fuzz=absent requires fuzz_reason", failures)


if __name__ == "__main__":
    unittest.main()
