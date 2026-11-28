#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Tests for the production panic-macro check."""

import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "census_panic_calls", Path(__file__).with_name("census-panic-calls.py")
)
assert SPEC and SPEC.loader
census = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(census)


class PanicMacroTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.root_patch = patch.object(census, "ROOT", self.root)
        self.root_patch.start()

    def tearDown(self) -> None:
        self.root_patch.stop()
        self.directory.cleanup()

    def write(self, path: str, source: str) -> Path:
        target = self.root / "crates/demo/src" / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(source, encoding="utf-8")
        return target.resolve()

    def check(self) -> tuple[int, str]:
        output = io.StringIO()
        with redirect_stdout(output):
            result = census.check_panic_macros()
        return result, output.getvalue()

    def test_runtime_panic_and_unreachable_fail(self) -> None:
        self.write("lib.rs", 'fn f() { panic!("bad"); unreachable!(); }\n')
        result, output = self.check()
        self.assertEqual(result, 1)
        self.assertEqual(output.count("runtime panic macro"), 2)

    def test_const_and_test_scopes_are_exempt(self) -> None:
        self.write("lib.rs", '''const A: u8 = match u8::checked_add(1, 1) {
    Some(value) => value,
    None => panic!("one plus one fits"),
};
fn g() -> u8 { const { match u8::checked_add(2, 2) {
    Some(value) => value, None => panic!("two plus two fits"),
} } }
pub const fn h(values: &[u8]) -> u8 {
    let [first, ..] = values else { panic!("one row"); }; *first
}
#[test] fn fixture() { unreachable!(); }
#[cfg(test)] mod tests { fn fixture() { panic!("test"); } }
''')
        self.assertEqual(self.check()[0], 0)

    def test_unreached_source_is_checked(self) -> None:
        self.write("lib.rs", "fn f() {}\n")
        self.write("unreached.rs", 'fn f() { panic!("bad"); }\n')
        result, output = self.check()
        self.assertEqual(result, 1)
        self.assertIn("unreached.rs:1", output)

    def test_test_only_include_is_excluded(self) -> None:
        self.write("lib.rs", '#[cfg(test)] mod tests { include!("fixture.rs"); }\n')
        fixture = self.write("fixture.rs", 'fn f() { panic!("test"); }\n')
        _, gated = census.production_files()
        self.assertIn(fixture, gated)
        self.assertEqual(self.check()[0], 0)

    def test_test_only_crate_is_excluded(self) -> None:
        self.write("lib.rs", "fn f() {}\n")
        support = self.root / "crates/cadmpeg-test-support/src/lib.rs"
        support.parent.mkdir(parents=True)
        support.write_text('pub fn assert_it() { panic!("test failure"); }\n')
        self.assertEqual(self.check()[0], 0)

    def test_production_after_test_item_is_checked(self) -> None:
        self.write("lib.rs", '#[test] fn fixture() { panic!("test"); } fn prod() { panic!("bad"); }\n')
        result, output = self.check()
        self.assertEqual(result, 1)
        self.assertEqual(output.count("runtime panic macro"), 1)


if __name__ == "__main__":
    unittest.main()
