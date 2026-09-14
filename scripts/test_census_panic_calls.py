#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Regression tests for the production panic-call inventory."""

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


class CensusTests(unittest.TestCase):
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

    def output(self) -> str:
        result = io.StringIO()
        with redirect_stdout(result):
            census.census_panic_calls(listing=True)
        return result.getvalue()

    def test_file_reachable_in_both_modes_remains_production(self) -> None:
        self.write("lib.rs", 'mod shared;\n#[cfg(test)]\n#[path = "shared.rs"]\nmod tests;\n')
        shared = self.write("shared.rs", 'fn f() { value.expect("real"); }\n')
        kept, gated = census.production_files()
        self.assertIn(shared, kept)
        self.assertNotIn(shared, gated)
        self.assertIn('shared.rs:1\t.expect("real")', self.output())

    def test_explicit_module_path_is_followed(self) -> None:
        self.write("lib.rs", '#[path = "actual.rs"]\nmod alias;\n')
        actual = self.write("actual.rs", "fn f() {}\n")
        kept, _ = census.production_files()
        self.assertIn(actual, kept)

    def test_unreached_source_is_counted_instead_of_silently_excluded(self) -> None:
        self.write("lib.rs", "fn f() {}\n")
        self.write("unreached.rs", 'fn f() { value.expect("inspect this module"); }\n')
        result = self.output()
        self.assertIn('unreached.rs:1\t.expect("inspect this module")', result)
        self.assertIn("files in scope the walk never reaches: 1", result)

    def test_test_only_include_is_excluded_but_comment_include_is_not_followed(self) -> None:
        self.write("lib.rs", '''
// include!("unused.rs");
#[cfg(test)]
mod tests {
    include!("fixture.rs");
}
''')
        fixture = self.write("fixture.rs", 'fn f() { value.expect("fixture"); }\n')
        unused = self.write("unused.rs", "fn unused() {}\n")
        kept, gated = census.production_files()
        self.assertIn(fixture, gated)
        self.assertNotIn(fixture, kept)
        self.assertNotIn(unused, kept | gated)
        self.assertNotIn('fixture.rs:1', self.output())

    def test_inline_module_gates_its_children(self) -> None:
        self.write("lib.rs", "#[cfg(test)]\nmod tests {\n    mod helpers;\n}\n")
        child = self.write("tests/helpers.rs", 'fn f() { value.expect("fixture"); }\n')
        _, gated = census.production_files()
        self.assertIn(child, gated)
        self.assertNotIn('helpers.rs:1', self.output())

    def test_call_list_includes_multiline_messages_and_whitespace(self) -> None:
        self.write("lib.rs", '''fn f() {
    value.expect (
        "required member",
    );
    other.unwrap( );
}
''')
        result = self.output()
        self.assertIn('lib.rs:2\t.expect (         "required member",     )', result)
        self.assertIn('lib.rs:5\t.unwrap( )', result)
        self.assertIn('non-test lines calling .expect(: 1', result)
        self.assertIn('non-test bare .unwrap() calls: 1', result)

    def test_labels_do_not_hide_calls_and_literals_do_not_add_calls(self) -> None:
        self.write("lib.rs", '''fn f() {
    'outer: loop { value.expect("real"); break 'outer; }
    let text = r"a backslash\\";
    other.expect("second");
    /* nested /* inner */ fake.expect("not code"); */
}
''')
        result = self.output()
        self.assertIn('lib.rs:2\t.expect("real")', result)
        self.assertIn('lib.rs:4\t.expect("second")', result)
        self.assertIn('non-test lines calling .expect(: 2', result)


if __name__ == "__main__":
    unittest.main()
