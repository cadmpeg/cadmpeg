#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Tests for the shared Rust policy scan."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("check-rust-policy.py")
SPEC = importlib.util.spec_from_file_location("check_rust_policy", SCRIPT)
assert SPEC and SPEC.loader
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)


class SharedScan(unittest.TestCase):
    def test_lexical_spans_are_reused_in_one_scan(self):
        @check.policy.analysis_session
        def spans():
            source = 'const A: &str = "text"; // comment\n'
            return check.policy.rust_non_code_spans(source), check.policy.rust_non_code_spans(source)

        first, second = spans()
        self.assertIs(first, second)

    def test_all_three_findings_share_one_source_read(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "crates/demo/src/lib.rs"
            source.parent.mkdir(parents=True)
            source.write_text('''#[allow(dead_code)]
const MAX_BAD: usize = 1;
fn f() { panic!("failure"); }
''' + "// /" + "home/example/work/\n")
            doc = root / "docs/decode-resource-caps.md"
            doc.parent.mkdir(parents=True)
            doc.write_text("# Resource caps\n")
            with patch.object(check.policy, "ROOT", root), \
                    patch.object(check.caps, "ROOT", root), \
                    patch.object(check.panic, "ROOT", root):
                reads = []
                original = Path.read_text
                def record(path, *args, **kwargs):
                    if path.resolve() == source.resolve():
                        reads.append(path)
                    return original(path, *args, **kwargs)
                with patch.object(Path, "read_text", record), \
                        patch.object(check.policy, "ProcessPoolExecutor", side_effect=AssertionError("forked")):
                    source_findings, cap_findings, panic_findings = check.scan()
            self.assertTrue(any(f.rule == "lint_suppression" for f in source_findings))
            self.assertTrue(any(f.rule == "authoring_path" for f in source_findings))
            self.assertTrue(any("missing resource-cap marker" in f for f in cap_findings))
            self.assertTrue(any("runtime panic macro" in f for f in panic_findings))
            self.assertEqual(len(reads), 1)


if __name__ == "__main__":
    unittest.main()
