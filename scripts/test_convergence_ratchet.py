#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``convergence-ratchet.py``."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("convergence-ratchet.py")
SPEC = importlib.util.spec_from_file_location("convergence_ratchet", SCRIPT)
assert SPEC and SPEC.loader
ratchet = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ratchet
SPEC.loader.exec_module(ratchet)


class StripCfgTest(unittest.TestCase):
    def test_strips_cfg_test_mod_body(self) -> None:
        text = (
            "fn prod() { from_le_bytes(); }\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    fn t() { from_le_bytes(); }\n"
            "}\n"
            "fn other() { from_be_bytes(); }\n"
        )
        stripped = ratchet.strip_cfg_test_items(text)
        self.assertEqual(ratchet.FROM_ENDIAN.findall(stripped), ["from_le_bytes", "from_be_bytes"])

    def test_keeps_non_test_cfg(self) -> None:
        text = "#[cfg(feature = \"x\")]\nfn f() { from_le_bytes(); }\n"
        stripped = ratchet.strip_cfg_test_items(text)
        self.assertEqual(ratchet.FROM_ENDIAN.findall(stripped), ["from_le_bytes"])


class PatternFilters(unittest.TestCase):
    def test_skips_use_import_for_le_at(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "crates" / "cadmpeg-codec-demo" / "src"
            src.mkdir(parents=True)
            (src / "lib.rs").write_text(
                "use cadmpeg_core::le::u32_at;\nfn f(b: &[u8]) { let _ = le::u32_at(b, 0); }\n",
                encoding="utf-8",
            )
            old_root = ratchet.ROOT
            try:
                ratchet.ROOT = root
                self.assertEqual(ratchet.count_le_be_at_outside_core(), 1)
            finally:
                ratchet.ROOT = old_root

    def test_excludes_loss_note_return_and_struct(self) -> None:
        text = (
            "struct LossNote {\n"
            "    msg: String,\n"
            "}\n"
            "fn make() -> LossNote {\n"
            "    LossNote { msg: String::new() }\n"
            "}\n"
        )
        # Inline the line filter used by the counter.
        hits = 0
        for line in text.splitlines():
            if not ratchet.LOSS_NOTE_LIT.search(line):
                continue
            if ratchet.LOSS_NOTE_RETURN.search(line) or ratchet.LOSS_NOTE_STRUCT.search(line):
                continue
            hits += 1
        self.assertEqual(hits, 1)

    def test_malformed_format_multiline(self) -> None:
        text = 'CodecError::Malformed(format!(\n    "bad {}", x\n))\n'
        self.assertEqual(len(ratchet.MALFORMED_FORMAT.findall(text)), 1)

    def test_production_filter_rejects_test_paths(self) -> None:
        self.assertFalse(ratchet.is_production_rs(Path("crates/c/src/foo_test.rs")))
        self.assertFalse(ratchet.is_production_rs(Path("crates/c/tests/foo.rs")))
        self.assertFalse(ratchet.is_production_rs(Path("crates/c/src/tests.rs")))
        self.assertTrue(ratchet.is_production_rs(Path("crates/c/src/decode.rs")))


def _zero_counts() -> dict[str, int]:
    return {key: 0 for key in ratchet.METRIC_KEYS}


def _complete_ceilings(**overrides: int) -> dict[str, int]:
    ceilings = _zero_counts()
    ceilings.update(overrides)
    return ceilings


class LedgerRoundTrip(unittest.TestCase):
    def test_check_fails_above_ceiling(self) -> None:
        counts = _zero_counts()
        counts["from_endian_bytes"] = 2
        failures = ratchet.check(counts, _complete_ceilings(from_endian_bytes=1))
        self.assertEqual(failures, ["from_endian_bytes: 2 > ledger 1"])

    def test_check_fails_missing_ceiling(self) -> None:
        counts = _zero_counts()
        ceilings = _complete_ceilings()
        del ceilings["nonliteral_vec_repeat"]
        failures = ratchet.check(counts, ceilings)
        self.assertEqual(
            failures, ["ledger missing ceiling for nonliteral_vec_repeat"]
        )


if __name__ == "__main__":
    unittest.main()
