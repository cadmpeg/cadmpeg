#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Regression tests for the validation rule namer."""

import importlib.util
from pathlib import Path
import sys
import unittest


SPEC = importlib.util.spec_from_file_location(
    "name_validation_rules", Path(__file__).with_name("name-validation-rules.py")
)
assert SPEC and SPEC.loader
namer = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = namer
SPEC.loader.exec_module(namer)

VALIDATOR_FILE = "crates/cadmpeg-ir/src/validate/topology.rs"


def rewrite(path: str, src: str) -> str:
    scan = namer.scan_source(path, src)
    names = namer.Names()
    for site in scan.sites:
        if not site.named:
            site.name = names.name(site.area, site.template, site.function)
    out = namer.rewritten(scan)
    return src if out is None else out


class NamerTests(unittest.TestCase):
    def test_slug_drops_placeholders_and_filler_words(self) -> None:
        self.assertEqual(
            namer.slug_words("edge {id} runs against its curve: the {} end"),
            ["edge", "runs", "against", "curve", "end"],
        )
        self.assertEqual(namer.slug_words("{message}"), [])

    def test_name_grammar(self) -> None:
        for good in ("topology.free-vertex", "f3d/links.native-ref", "design_x.a1"):
            self.assertTrue(namer.NAME_RE.match(good), good)
        for bad in ("topology", "Topology.x", "topology.-x", "a/b/c.d", "topology.x_y"):
            self.assertFalse(namer.NAME_RE.match(bad), bad)

    def test_a_finding_message_states_its_rule(self) -> None:
        src = (
            "fn f(findings: &mut Vec<Finding>) {\n"
            "    record_finding(findings, Check::WireTopology, Severity::Error, None,\n"
            '        format_args!("free vertex {id} is in {} shells", count));\n'
            "}\n"
        )
        out = rewrite(VALIDATOR_FILE, src)
        self.assertIn(
            'crate::rule!("topology.free-vertex-shells", "free vertex {id} is in {} shells"',
            out,
        )

    def test_a_named_site_is_left_as_it_is(self) -> None:
        src = (
            "fn f(findings: &mut Vec<Finding>) {\n"
            '    record_finding(findings, Check::WireTopology, Severity::Error, None,\n'
            '        crate::rule!("topology.kept", "free vertex"));\n'
            "}\n"
        )
        self.assertEqual(rewrite(VALIDATOR_FILE, src), src)

    def test_a_named_site_is_known_by_its_template(self) -> None:
        src = (
            "fn f(findings: &mut Vec<Finding>) {\n"
            '    record_finding(findings, Check::WireTopology, Severity::Error, None,\n'
            '        crate::rule!("topology.kept", "free vertex {id} in shells", id));\n'
            "}\n"
        )
        [site] = namer.scan_source(VALIDATOR_FILE, src).sites
        self.assertEqual(site.template, "free vertex {id} in shells")
        names = namer.Names()
        names.remember(site.area, site.template, site.name, site.function)
        self.assertEqual(
            names.name("topology", "free vertex {id} in shells", "g"), "topology.kept"
        )
        self.assertEqual(
            names.name("topology", "kept vertex", "g"), "topology.kept-vertex"
        )

    def test_a_message_parameter_takes_ruled(self) -> None:
        src = "fn error(findings: &mut Vec<Finding>, message: std::fmt::Arguments<'_>) {}\n"
        self.assertIn("message: Ruled<'_>", rewrite(VALIDATOR_FILE, src))

    def test_a_format_call_with_its_own_message_is_not_rewritten(self) -> None:
        src = 'fn f() -> CodecError { CodecError::malformed(format_args!("bad {x}")) }\n'
        self.assertEqual(rewrite(VALIDATOR_FILE, src), src)

    def test_a_template_of_placeholders_takes_its_literal_argument(self) -> None:
        toks = namer.tokenize('format_args!("{}", "spreadsheet cell does not resolve")')
        self.assertEqual(
            namer.format_template(toks, 2), "spreadsheet cell does not resolve"
        )

    def test_a_finding_literal_gains_a_rule(self) -> None:
        src = (
            "fn f() -> Finding {\n"
            "    Finding { check: Check::NativeLinks, severity: Severity::Error,\n"
            '        message: format!("native record {id} is unowned"), entity: None, related: Vec::new() }\n'
            "}\n"
        )
        out = rewrite("crates/cadmpeg-codec-nx/src/validate.rs", src)
        self.assertIn('rule: cadmpeg_ir::rule_name!("nx/validate.native-record-unowned")', out)

    def test_a_struct_update_keeps_its_rule(self) -> None:
        src = "fn f(x: Finding) -> Finding { Finding { check: Check::Design, ..x } }\n"
        self.assertEqual(rewrite(VALIDATOR_FILE, src), src)

    def test_one_slug_for_two_templates_takes_more_words(self) -> None:
        names = namer.Names()
        first = names.name("topology", "one two three four five six seven", "")
        second = names.name("topology", "one two three four five six eight", "")
        self.assertEqual(first, "topology.one-two-three-four-five-six")
        self.assertEqual(second, "topology.one-two-three-four-five-six-eight")
        self.assertEqual(
            names.name("topology", "one two three four five six seven", ""), first
        )

if __name__ == "__main__":
    unittest.main()
