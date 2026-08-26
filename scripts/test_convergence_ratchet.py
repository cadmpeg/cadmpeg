#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``convergence-ratchet.py``."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("convergence-ratchet.py")
SPEC = importlib.util.spec_from_file_location("convergence_ratchet", SCRIPT)
assert SPEC and SPEC.loader
ratchet = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ratchet
SPEC.loader.exec_module(ratchet)


def _pad_lines(prefix: list[str], total_lines: int, *, trailing_newline: bool = True) -> str:
    assert total_lines >= len(prefix)
    lines = prefix + [f"// filler {i}" for i in range(total_lines - len(prefix))]
    text = "\n".join(lines)
    if trailing_newline:
        return text + "\n"
    return text


def _placement_counts(root: Path) -> tuple[dict[str, int], dict[str, list[dict[str, object]]]]:
    old_root = ratchet.ROOT
    try:
        ratchet.ROOT = root
        return ratchet.measure_all()
    finally:
        ratchet.ROOT = old_root


class TempRepoCase(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.old_root = ratchet.ROOT
        self.old_ledger = ratchet.LEDGER
        ratchet.ROOT = self.root
        ratchet.LEDGER = self.root / "docs" / "convergence-ledger.toml"

    def tearDown(self) -> None:
        ratchet.ROOT = self.old_root
        ratchet.LEDGER = self.old_ledger
        self._tmp.cleanup()

    def write(self, relative: str, text: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def measure(self) -> tuple[dict[str, int], dict[str, list[dict[str, object]]]]:
        return ratchet.measure_all()


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
        self.assertEqual(
            ratchet.FROM_ENDIAN.findall(stripped),
            ["from_le_bytes", "from_be_bytes"],
        )

    def test_keeps_non_test_cfg(self) -> None:
        text = "#[cfg(feature = \"x\")]\nfn f() { from_le_bytes(); }\n"
        stripped = ratchet.strip_cfg_test_items(text)
        self.assertEqual(ratchet.FROM_ENDIAN.findall(stripped), ["from_le_bytes"])

    def test_elides_cfg_test_items_without_blank_lines(self) -> None:
        text = (
            "fn prod() {}\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    fn t() {}\n"
            "}\n"
            "fn other() {}\n"
        )
        self.assertEqual(ratchet.elide_cfg_test_items(text), "fn prod() {}\nfn other() {}\n")


class PatternFilters(unittest.TestCase):
    def test_excludes_loss_note_return_and_struct(self) -> None:
        text = (
            "struct LossNote {\n"
            "    msg: String,\n"
            "}\n"
            "impl LossNote {\n"
            "    fn new() -> Self { todo!() }\n"
            "}\n"
            "fn make() -> LossNote {\n"
            "    LossNote { msg: String::new() }\n"
            "}\n"
        )
        hits = 0
        for line in text.splitlines():
            if not ratchet.LOSS_NOTE_LIT.search(line):
                continue
            if (
                ratchet.LOSS_NOTE_RETURN.search(line)
                or ratchet.LOSS_NOTE_STRUCT.search(line)
                or ratchet.LOSS_NOTE_IMPL.search(line)
            ):
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

    def test_bare_tolerance_includes_seven_eight_eleven(self) -> None:
        text = (
            "a 1e-6 b 1e-7 c 1e-8 d 1e-9 e 1e-10 f 1e-11 g 1e-12 "
            "h 1e-18 i 1.0e-9 j 1.00E-10\n"
        )
        self.assertEqual(
            ratchet.BARE_TOLERANCE.findall(text),
            [
                "1e-6",
                "1e-7",
                "1e-8",
                "1e-9",
                "1e-10",
                "1e-11",
                "1e-12",
                "1.0e-9",
                "1.00E-10",
            ],
        )

    def test_metric_source_masks_comments_and_literals(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "source.rs"
            path.write_text(
                '// from_le_bytes 1e-9\nconst NOTE: &str = "from_be_bytes 1e-8";\n'
                "fn actual() { from_le_bytes(); let _ = 1e-7; }\n",
                encoding="utf-8",
            )
            source = ratchet.metric_source_text(path)
        self.assertEqual(ratchet.FROM_ENDIAN.findall(source), ["from_le_bytes"])
        self.assertEqual(ratchet.count_bare_tolerance_literals(source), 1)

    def test_bare_tolerance_excludes_named_threshold_initializers(self) -> None:
        text = (
            "const EPS_DIRECT: f64 = 1e-9;\n"
            "pub(crate) static EPS_STATIC: f64 = 1.0e-10;\n"
            "let direct = 1e-9;\n"
            "let formatted = 1.0e-10;\n"
            "const DERIVED: f64 = f64::from_bits(1e-9 as u64);\n"
            "const MULTILINE: f64 =\n"
            "    1.0e-11;\n"
        )
        self.assertEqual(ratchet.count_bare_tolerance_literals(text), 3)

    def test_vec_repeat_scanner_ignores_nested_delimiters_and_strings(self) -> None:
        text = r'''
            let bytes = vec![0u8; len];
            let nested = vec![[0; 2]; outer_len];
            let message = vec![format!("a; b")];
            // vec![0; commented_len]
        '''
        self.assertEqual(
            list(ratchet.iter_vec_repeat_counts(text)), ["len", "outer_len"]
        )

    def test_from_endian_counts_non_codec_crates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for relative, source in (
                (
                    "crates/cadmpeg-codec-demo/src/lib.rs",
                    "fn f() { u32::from_le_bytes([0; 4]); }\n",
                ),
                (
                    "crates/cadmpeg-protein/src/lib.rs",
                    "fn f() { u32::from_le_bytes([0; 4]); u64::from_be_bytes([0; 8]); }\n",
                ),
            ):
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")
            old_root = ratchet.ROOT
            try:
                ratchet.ROOT = root
                self.assertEqual(ratchet.count_from_endian_bytes(), 3)
            finally:
                ratchet.ROOT = old_root


class PlacementMetrics(TempRepoCase):
    def test_root_router_counts_but_nested_tests_do_not(self) -> None:
        self.write(
            "crates/demo/src/lib.rs",
            "#[cfg(test)]\nmod tests;\nmod foo;\n",
        )
        self.write(
            "crates/demo/src/foo.rs",
            "#[cfg(test)]\nmod tests;\n",
        )
        self.write(
            "crates/demo/src/tests.rs",
            _pad_lines(["mod router_only;"], 2001),
        )
        self.write(
            "crates/demo/src/foo/tests.rs",
            _pad_lines(["fn helper() {}"], 2001),
        )
        counts, contributors = self.measure()
        self.assertEqual(counts["crate_root_tests_rs"], 1)
        debts = {item["path"]: item["debt"] for item in contributors["test_line_debt"]}
        self.assertEqual(debts["crates/demo/src/tests.rs"], 1)
        self.assertEqual(debts["crates/demo/src/foo/tests.rs"], 1)

    def test_path_includes_follow_test_only_ancestry(self) -> None:
        self.write("crates/demo/src/lib.rs", "")
        self.write(
            "crates/demo/src/tests.rs",
            "\n".join(
                [
                    '#[path = "integration_tests.rs"]',
                    "mod integration_tests;",
                    "mod nested {",
                    "    #[path =",
                    '        "bytes.rs"',
                    "    ]",
                    "    mod bytes;",
                    "}",
                    "",
                ]
            ),
        )
        self.write("crates/demo/src/integration_tests.rs", "fn smoke() {}\n")
        self.write(
            "crates/demo/src/bytes.rs",
            _pad_lines(["fn helper() {}"], 2001),
        )
        counts, contributors = self.measure()
        self.assertEqual(counts["path_test_includes"], 2)
        includes = {(item["path"], item["target"]) for item in contributors["path_test_includes"]}
        self.assertEqual(
            includes,
            {
                ("crates/demo/src/tests.rs", "integration_tests.rs"),
                ("crates/demo/src/tests.rs", "bytes.rs"),
            },
        )
        self.assertIn(
            {"path": "crates/demo/src/bytes.rs", "lines": 2001, "debt": 1},
            contributors["test_line_debt"],
        )

    def test_feature_gated_non_test_path_does_not_count(self) -> None:
        self.write(
            "crates/demo/src/lib.rs",
            "\n".join(
                [
                    "#[cfg(feature = \"fuzzing\")]",
                    '#[path = "fuzzing.rs"]',
                    "pub mod fuzz;",
                    "#[cfg(test)]",
                    '#[path = "unit.rs"]',
                    "mod tests;",
                    "",
                ]
            ),
        )
        self.write("crates/demo/src/fuzzing.rs", "pub fn fuzz() {}\n")
        self.write("crates/demo/src/unit.rs", "fn helper() {}\n")
        counts, contributors = self.measure()
        self.assertEqual(counts["path_test_includes"], 1)
        self.assertEqual(
            contributors["path_test_includes"],
            [{"path": "crates/demo/src/lib.rs", "target": "unit.rs", "debt": 1}],
        )

    def test_semantic_dirs_integration_support_and_golden_exclusion(self) -> None:
        self.write("crates/demo/src/lib.rs", "#[cfg(test)]\nmod golden_tests;\nmod foo;\n")
        self.write("crates/demo/src/foo.rs", "#[cfg(test)]\nmod tests;\n")
        self.write("crates/demo/src/foo/tests.rs", "mod parsing;\n")
        self.write(
            "crates/demo/src/foo/tests/parsing.rs",
            _pad_lines(["fn parsing() {}"], 2001),
        )
        self.write(
            "crates/demo/src/integration_tests.rs",
            _pad_lines(["fn integration() {}"], 2001),
        )
        self.write(
            "crates/demo/src/test_support.rs",
            _pad_lines(["fn support() {}"], 2001),
        )
        self.write(
            "crates/demo/src/golden_tests.rs",
            _pad_lines(["fn golden() {}"], 2501),
        )
        counts, contributors = self.measure()
        debts = {item["path"]: item["debt"] for item in contributors["test_line_debt"]}
        self.assertEqual(debts["crates/demo/src/foo/tests/parsing.rs"], 1)
        self.assertEqual(debts["crates/demo/src/integration_tests.rs"], 1)
        self.assertEqual(debts["crates/demo/src/test_support.rs"], 1)
        self.assertNotIn("crates/demo/src/golden_tests.rs", debts)
        self.assertEqual(counts["test_line_debt"], 3)

    def test_inline_test_module_debt_uses_module_span(self) -> None:
        inline_module = "\n".join(
            ["fn prod() {}", "#[cfg(test)]", "mod tests {"]
            + [f"    // filler {i}" for i in range(1998)]
            + ["}"]
        )
        self.write(
            "crates/demo/src/lib.rs",
            inline_module + "\n",
        )
        counts, contributors = self.measure()
        self.assertEqual(counts["test_line_debt"], 1)
        self.assertEqual(
            contributors["test_line_debt"],
            [
                {
                    "path": "crates/demo/src/lib.rs::tests",
                    "lines": 2001,
                    "debt": 1,
                }
            ],
        )

    def test_exact_boundaries_and_no_trailing_newline(self) -> None:
        self.write("crates/demo/src/lib.rs", "mod prod;\n")
        self.write(
            "crates/demo/src/prod.rs",
            _pad_lines(["fn prod() {}"], 10000),
        )
        self.write(
            "crates/demo/src/test_support.rs",
            _pad_lines(["fn helper() {}"], 2000, trailing_newline=False),
        )
        counts, contributors = self.measure()
        self.assertEqual(counts["test_line_debt"], 0)
        self.assertEqual(counts["production_line_debt"], 0)
        self.assertEqual(contributors["test_line_debt"], [])
        self.assertEqual(contributors["production_line_debt"], [])

        self.write(
            "crates/demo/src/prod.rs",
            _pad_lines(["fn prod() {}"], 10001, trailing_newline=False),
        )
        self.write(
            "crates/demo/src/test_support.rs",
            _pad_lines(["fn helper() {}"], 2001, trailing_newline=False),
        )
        counts, contributors = self.measure()
        self.assertEqual(counts["test_line_debt"], 1)
        self.assertEqual(counts["production_line_debt"], 1)
        self.assertEqual(
            contributors["test_line_debt"],
            [{"path": "crates/demo/src/test_support.rs", "lines": 2001, "debt": 1}],
        )
        self.assertEqual(
            contributors["production_line_debt"],
            [{"path": "crates/demo/src/prod.rs", "lines": 10001, "debt": 1}],
        )

    def test_split_reduces_debt(self) -> None:
        self.write("crates/demo/src/lib.rs", "#[cfg(test)]\nmod tests;\n")
        self.write(
            "crates/demo/src/tests.rs",
            _pad_lines(["fn all_in_one() {}"], 5000),
        )
        single_counts, _ = self.measure()

        self.write(
            "crates/demo/src/tests.rs",
            "\n".join(
                [
                    '#[path = "part_a.rs"]',
                    "mod part_a;",
                    '#[path = "part_b.rs"]',
                    "mod part_b;",
                    "",
                ]
            ),
        )
        self.write(
            "crates/demo/src/part_a.rs",
            _pad_lines(["fn a() {}"], 2500),
        )
        self.write(
            "crates/demo/src/part_b.rs",
            _pad_lines(["fn b() {}"], 2500),
        )
        split_counts, _ = self.measure()
        self.assertEqual(single_counts["test_line_debt"], 3000)
        self.assertEqual(split_counts["test_line_debt"], 1000)

    def test_cargo_tests_are_counted(self) -> None:
        self.write("crates/demo/src/lib.rs", "")
        self.write(
            "crates/demo/tests/smoke.rs",
            _pad_lines(["fn smoke() {}"], 2001),
        )
        counts, contributors = self.measure()
        self.assertEqual(counts["test_line_debt"], 1)
        self.assertEqual(
            contributors["test_line_debt"],
            [{"path": "crates/demo/tests/smoke.rs", "lines": 2001, "debt": 1}],
        )


def _zero_counts() -> dict[str, int]:
    return {key: 0 for key in ratchet.METRIC_KEYS}


def _complete_ceilings(**overrides: int) -> dict[str, int]:
    ceilings = _zero_counts()
    ceilings.update(overrides)
    return ceilings


def _complete_targets(**overrides: int) -> dict[str, int]:
    targets = {key: 0 for key in ratchet.TARGET_KEYS}
    targets.update(overrides)
    return targets


class LedgerRoundTrip(unittest.TestCase):
    def test_check_fails_above_ceiling(self) -> None:
        counts = _zero_counts()
        counts["from_endian_bytes"] = 2
        failures = ratchet.check(
            counts,
            _complete_ceilings(from_endian_bytes=1),
            _complete_targets(),
        )
        self.assertEqual(failures, ["from_endian_bytes: 2 > ledger 1"])

    def test_check_fails_missing_ceiling(self) -> None:
        counts = _zero_counts()
        ceilings = _complete_ceilings()
        del ceilings["nonliteral_vec_repeat"]
        failures = ratchet.check(counts, ceilings, _complete_targets())
        self.assertEqual(
            failures, ["ledger missing ceiling for nonliteral_vec_repeat"]
        )

    def test_check_fails_missing_target(self) -> None:
        counts = _zero_counts()
        targets = _complete_targets()
        del targets["path_test_includes"]
        failures = ratchet.check(counts, _complete_ceilings(), targets)
        self.assertEqual(
            failures, ["ledger missing target for path_test_includes"]
        )

    def test_parse_and_render_keep_targets(self) -> None:
        text = ratchet.render_ledger(
            "deadbeef",
            "filter with #[path] text",
            _complete_targets(),
            _complete_ceilings(crate_root_tests_rs=12, path_test_includes=23),
            {},
        )
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "ledger.toml"
            path.write_text(text, encoding="utf-8")
            parsed = ratchet.parse_ledger(path)
        self.assertEqual(parsed["targets"], _complete_targets())
        self.assertEqual(parsed["kinds"], dict(ratchet.KIND_BY_KEY))
        self.assertNotIn("from_endian_bytes", parsed["targets"])
        self.assertEqual(parsed["ceilings"]["crate_root_tests_rs"], 12)
        self.assertEqual(parsed["ceilings"]["path_test_includes"], 23)
        self.assertEqual(parsed["filter"], "filter with #[path] text")

    def test_check_rejects_invalid_measured_at(self) -> None:
        failures = ratchet.check(
            _zero_counts(),
            _complete_ceilings(),
            _complete_targets(),
            measured_at="not-a-sha",
        )
        self.assertEqual(failures, ["ledger measured_at is not a 40-char git SHA"])

    def test_check_accepts_valid_measured_at(self) -> None:
        failures = ratchet.check(
            _zero_counts(),
            _complete_ceilings(),
            _complete_targets(),
            measured_at="0123456789abcdef0123456789abcdef01234567",
        )
        self.assertEqual(failures, [])

    def test_measured_commit_must_exist_and_be_reachable(self) -> None:
        sha = "0123456789abcdef0123456789abcdef01234567"
        with patch.object(ratchet, "git_object_exists", return_value=False):
            self.assertEqual(
                ratchet.check_measured_commit(sha),
                ["ledger measured_at does not identify an existing commit"],
            )
        with (
            patch.object(ratchet, "git_object_exists", return_value=True),
            patch.object(ratchet, "git_is_ancestor", return_value=False),
        ):
            self.assertEqual(
                ratchet.check_measured_commit(sha),
                ["ledger measured_at is not an ancestor of HEAD"],
            )
        with (
            patch.object(ratchet, "git_object_exists", return_value=True),
            patch.object(ratchet, "git_is_ancestor", return_value=True),
        ):
            self.assertEqual(ratchet.check_measured_commit(sha), [])

    def test_raise_without_reason_fails(self) -> None:
        failures = ratchet.check(
            _zero_counts(),
            _complete_ceilings(from_endian_bytes=10),
            _complete_targets(),
            previous_ceilings=_complete_ceilings(from_endian_bytes=1),
            reasons={},
        )
        self.assertEqual(
            failures, ["from_endian_bytes: ceiling raised without [reasons].from_endian_bytes"]
        )

    def test_raise_with_reason_passes(self) -> None:
        failures = ratchet.check(
            _zero_counts(),
            _complete_ceilings(from_endian_bytes=10),
            _complete_targets(),
            previous_ceilings=_complete_ceilings(from_endian_bytes=1),
            reasons={"from_endian_bytes": "widened glob to all crates/**/src"},
        )
        self.assertEqual(failures, [])

    def test_kinds_must_match_script_classification(self) -> None:
        failures = ratchet.check(
            _zero_counts(),
            _complete_ceilings(),
            _complete_targets(),
            kinds={"from_endian_bytes": "convergence"},
        )
        self.assertIn("from_endian_bytes: kind 'convergence' != 'pressure'", failures)
        self.assertIn("ledger missing kind for codec_error_malformed_format", failures)

    def test_pressure_key_must_not_have_target(self) -> None:
        targets = _complete_targets()
        targets["from_endian_bytes"] = 0
        failures = ratchet.check(_zero_counts(), _complete_ceilings(), targets)
        self.assertEqual(
            failures, ["from_endian_bytes: pressure key must not have a [targets] entry"]
        )

    def test_update_refuses_increase(self) -> None:
        repo = TempRepoCase()
        try:
            repo.setUp()
            repo.write(
                "crates/cadmpeg-codec-demo/src/lib.rs",
                "fn f() { u32::from_le_bytes([0; 4]); }\n",
            )
            repo.write(
                "docs/convergence-ledger.toml",
                ratchet.render_ledger(
                    "0123456789abcdef0123456789abcdef01234567",
                    "filter",
                    _complete_targets(),
                    _complete_ceilings(),
                    {},
                ),
            )
            self.assertEqual(ratchet.main(["--update"]), 1)
        finally:
            repo.tearDown()


if __name__ == "__main__":
    unittest.main()
