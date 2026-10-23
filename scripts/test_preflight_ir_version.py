#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``preflight_ir_version.py``. These never run cargo."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "test_preflight", Path(__file__).with_name("test_preflight.py"))
assert SPEC and SPEC.loader
harness = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = harness
SPEC.loader.exec_module(harness)
check = harness.load("preflight_ir_version")

DOCUMENT = "crates/cadmpeg-ir/src/document.rs"
FEATURES = "crates/cadmpeg-ir/src/features.rs"
POLICY = "crates/cadmpeg-core/src/decode/policy.rs"
DIALECT = "crates/cadmpeg-core/src/dialect.rs"
STRUCT = """#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureInputTopology {
    pub faces: Vec<FaceId>,
}
"""
WIDER = STRUCT.replace("}\n", """    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_geometry: Vec<HistoricalEdgeGeometry>,
}
""")
LIMITS = """#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    pub max_input_bytes: u64,
}
"""
LAYERS = """#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialectLayers {
    pub container: String,
}
"""
GOLDEN = "crates/cadmpeg-codec-step/tests/golden/decode/box.json"


def document(version: int) -> str:
    return f'''pub const IR_VERSION: &str = "{version}";

#[derive(Serialize)]
struct CadIrWriteWire<'a> {{
    ir_version: IrVersion,
    features: &'a [FeatureInputTopology],
    layers: Option<cadmpeg_core::dialect::DialectLayers>,
}}
'''


def golden(version: int) -> str:
    return f'{{\n  "ir_version": "{version}",\n  "model": {{}}\n}}\n'


class SerializedItems(unittest.TestCase):
    def test_fields_and_serde_attributes_are_compared(self) -> None:
        before = check.serialized_items(STRUCT)
        self.assertEqual(list(before), ["struct FeatureInputTopology"])
        self.assertNotEqual(before, check.serialized_items(WIDER))
        renamed = STRUCT.replace("deny_unknown_fields", "deny_unknown_fields, rename_all = \"x\"")
        self.assertNotEqual(before, check.serialized_items(renamed))

    def test_other_attributes_and_visibility_are_ignored(self) -> None:
        before = check.serialized_items(STRUCT)
        after = STRUCT.replace("Debug,", "Debug, Clone,").replace("pub faces", "pub(super) faces")
        after = after.replace("pub struct", "#[must_use]\n#[non_exhaustive]\npub(crate) struct")
        after = after.replace("pub(super) faces", "#[schemars(length(min = 1))]\n    pub(super) faces")
        self.assertEqual(before, check.serialized_items(after))

    def test_comments_and_unserialized_items_are_ignored(self) -> None:
        commented = STRUCT.replace("pub faces", "/// The faces.\n    pub faces")
        self.assertEqual(check.serialized_items(STRUCT), check.serialized_items(commented))
        self.assertEqual(check.serialized_items("pub struct Plain {\n    a: u8,\n}\n"), {})

    def test_multiline_derive(self) -> None:
        text = "#[derive(\n    Debug,\n    Serialize,\n)]\npub enum Kind {\n    A,\n}\n"
        self.assertEqual(list(check.serialized_items(text)), ["enum Kind"])


class IrVersionCheck(harness.FixtureCase):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit({DOCUMENT: document(8), FEATURES: STRUCT, GOLDEN: golden(8),
                          POLICY: LIMITS, DIALECT: LAYERS})
        self.repo.branch()

    def results(self):
        return harness.run_check(check, self.repo.context())

    def assertPasses(self, files: dict[str, str | None]) -> None:
        self.repo.commit(files)
        results = self.results()
        self.assertEqual(harness.statuses(results), ["PASS"], results)

    def test_wire_change_keeps_release_version(self) -> None:
        # A new optional field on a CADIR type, as in #327.
        self.repo.commit({FEATURES: WIDER})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn("release IR_VERSION 8", results[0].summary)
        self.assertIn(f"{FEATURES}: struct FeatureInputTopology changed", results[0].details)
        self.assertFalse(any('set IR_VERSION' in detail for detail in results[0].details))

    def test_serde_attribute_change_warns(self) -> None:
        self.repo.commit({FEATURES: STRUCT.replace("pub faces", "#[serde(default)]\n    pub faces")})
        self.assertEqual(harness.statuses(self.results()), ["WARN"])
        self.repo.commit({FEATURES: STRUCT.replace(
            "deny_unknown_fields", "deny_unknown_fields, rename_all = \"camelCase\"")})
        self.assertEqual(harness.statuses(self.results()), ["WARN"])

    def test_core_type_the_document_names_warns(self) -> None:
        self.repo.commit({DIALECT: LAYERS.replace("}\n", "    pub codec: String,\n}\n")})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn(f"{DIALECT}: struct DialectLayers changed", results[0].details)

    def test_type_reached_through_a_macro_warns(self) -> None:
        registry = """macro_rules! declare_model {
    ($($field:ident: $ty:ty;)*) => {
        #[derive(Serialize)]
        struct ModelWire { $($field: Vec<$ty>,)* }
    };
}
declare_model! { sketches: Sketch; }
"""
        sketch = "#[derive(Serialize)]\npub struct Sketch {\n    pub id: u32,\n}\n"
        root = document(8).replace("features: &'a", "model: ModelWire,\n    features: &'a")
        self.repo.commit({DOCUMENT: root + registry, "crates/cadmpeg-ir/src/sketches.rs": sketch})
        self.repo.checkout("upstream")
        self.repo.git("merge", "-q", "topic")
        self.repo.checkout("topic")
        self.repo.commit({"crates/cadmpeg-ir/src/sketches.rs":
                          sketch.replace("}\n", "    pub plane: u32,\n}\n")})
        self.assertEqual(harness.statuses(self.results()), ["WARN"])

    def test_non_serde_derive_and_visibility_pass(self) -> None:
        self.assertPasses({FEATURES: STRUCT.replace("Debug,", "Debug, Clone, PartialEq,")
                           .replace("pub faces", "pub(crate) faces")})

    def test_lint_and_schema_attributes_pass(self) -> None:
        self.assertPasses({FEATURES: STRUCT.replace(
            "#[serde(deny_unknown_fields)]\n",
            "#[cfg_attr(feature = \"schema\", derive(JsonSchema))]\n#[must_use]\n"
            "#[non_exhaustive]\n#[serde(deny_unknown_fields)]\n")})

    def test_comments_and_formatting_pass(self) -> None:
        self.assertPasses({FEATURES: STRUCT.replace(
            "pub faces: Vec<FaceId>,", "/// The faces.\n    pub faces : Vec< FaceId >, // ids")})

    def test_type_moved_to_another_file_passes(self) -> None:
        self.assertPasses({FEATURES: "mod input;\npub use input::FeatureInputTopology;\n",
                           "crates/cadmpeg-ir/src/features/input.rs": STRUCT})

    def test_inline_test_module_is_ignored(self) -> None:
        tests = """
#[cfg(test)]
mod tests {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct DialectIdConformance {
        valid: Vec<String>,
    }
}
"""
        self.repo.commit({DIALECT: LAYERS + tests})
        self.repo.checkout("upstream")
        self.repo.git("merge", "-q", "topic")
        self.repo.checkout("topic")
        self.assertPasses({DIALECT: LAYERS + tests.replace(
            "valid: Vec<String>,", "valid: Vec<String>,\n        invalid: Vec<String>,")})

    def test_cfg_test_module_file_is_ignored(self) -> None:
        self.repo.commit({FEATURES: STRUCT + "#[cfg(test)]\nmod checks;\n",
                          "crates/cadmpeg-ir/src/features/checks.rs": STRUCT})
        self.repo.checkout("upstream")
        self.repo.git("merge", "-q", "topic")
        self.repo.checkout("topic")
        self.assertPasses({"crates/cadmpeg-ir/src/features/checks.rs": WIDER})

    def test_core_type_off_the_wire_passes(self) -> None:
        self.assertPasses({POLICY: LIMITS.replace("}\n", "    pub max_output_bytes: u64,\n}\n")})

    def test_ir_type_no_document_reaches_warns(self) -> None:
        self.repo.commit({"crates/cadmpeg-ir/src/codec.rs": LIMITS.replace("ResourceLimits",
                                                                         "DecodeOptions")})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn("no IR_VERSION document reaches it", results[0].details[0])

    def test_reached_name_defined_twice_warns(self) -> None:
        self.repo.commit({DIALECT: LAYERS.replace("}\n", "    pub codec: String,\n}\n"),
                          "crates/cadmpeg-core/src/other.rs": LAYERS})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["WARN"])
        self.assertIn("name defined more than once", results[0].details[0])

    def test_bump_with_regenerated_fixtures_passes(self) -> None:
        self.repo.commit({FEATURES: WIDER, DOCUMENT: document(9), GOLDEN: golden(9)})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])

    def test_release_reset_with_updated_fixtures_passes(self) -> None:
        self.repo.commit({DOCUMENT: document(11), GOLDEN: golden(11)})
        self.repo.checkout("upstream")
        self.repo.git("merge", "-q", "topic")
        self.repo.checkout("topic")
        self.assertPasses({DOCUMENT: document(8), GOLDEN: golden(8)})

    def test_bump_with_stale_fixture_fails(self) -> None:
        self.repo.commit({FEATURES: WIDER, DOCUMENT: document(9)})
        results = self.results()
        self.assertEqual(harness.statuses(results), ["FAIL"])
        self.assertIn(GOLDEN, results[0].details)

    def test_upstream_release_version_needs_no_extra_increment(self) -> None:
        self.repo.commit({FEATURES: WIDER, DOCUMENT: document(9), GOLDEN: golden(9)})
        self.repo.checkout("upstream")
        self.repo.commit({DOCUMENT: document(9), GOLDEN: golden(9)})
        self.repo.checkout("topic")
        results = self.results()
        self.assertEqual(harness.statuses(results), ["PASS"])

    def test_codec_serialized_type_warns(self) -> None:
        self.repo.commit({"crates/cadmpeg-codec-f3d/src/native.rs": STRUCT})
        self.assertEqual(harness.statuses(self.results()), ["WARN"])

    def test_test_path_is_ignored(self) -> None:
        self.repo.commit({"crates/cadmpeg-ir/src/features/tests.rs": WIDER})
        self.assertEqual(harness.statuses(self.results()), ["PASS"])


if __name__ == "__main__":
    unittest.main()
