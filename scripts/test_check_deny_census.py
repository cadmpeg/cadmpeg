#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Regression tests for the deny census's declaration resolution."""

import importlib.util
import contextlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "check_deny_census", Path(__file__).with_name("check-deny-census.py")
)
assert SPEC and SPEC.loader
census = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(census)


class DenyCensusTests(unittest.TestCase):
    def run_census(self, files: dict[str, str]) -> tuple[int, str]:
        with tempfile.TemporaryDirectory() as directory:
            paths = []
            for name, source in files.items():
                path = Path(directory) / "src" / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")
                paths.append(path)
            output = io.StringIO()
            with patch.object(census, "source_files", return_value=paths), \
                    patch.object(census, "EXCEPTIONS", {}), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                status = census.main()
            return status, output.getvalue()

    def item(self, path: str, name: str) -> census.Item:
        return census.Item(Path(path), 1, "struct", name, "", "")

    def test_same_file_declaration_wins_over_a_duplicate_elsewhere(self) -> None:
        local = self.item("crates/ir/src/local.rs", "Wire")
        foreign = self.item("crates/core/src/foreign.rs", "Wire")
        owner = self.item("crates/ir/src/local.rs", "Reader")

        ambiguities = []
        resolved = census.resolve_item(
            {"Wire": [foreign, local]}, "Wire", owner, ambiguities
        )

        self.assertIs(resolved, local)
        self.assertEqual(ambiguities, [])

    def test_cross_file_duplicate_is_reported_instead_of_using_first_visit(self) -> None:
        first = self.item("crates/ir/src/first.rs", "Wire")
        second = self.item("crates/core/src/second.rs", "Wire")
        owner = self.item("crates/asm/src/reader.rs", "Reader")

        ambiguities = []
        resolved = census.resolve_item(
            {"Wire": [first, second]}, "Wire", owner, ambiguities
        )

        self.assertIsNone(resolved)
        self.assertEqual(
            ambiguities,
            [
                (
                    owner.path,
                    owner.line,
                    owner.name,
                    "Wire",
                    (first.path, second.path),
                )
            ],
        )

    def test_native_projection_is_not_a_static_census_exception(self) -> None:
        self.assertNotIn(
            "crates/cadmpeg-ir/src/unknown.rs:NativeUnknownRecord",
            census.EXCEPTIONS,
        )

    def test_qualified_wire_cannot_use_a_denying_local_namesake(self) -> None:
        status, output = self.run_census({
            "lib.rs": '''
                use serde::Deserialize;
                mod other;
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Wire { value: String }
                #[derive(Deserialize)] #[serde(from = "other::Wire")]
                struct Reader;
                impl From<other::Wire> for Reader {
                    fn from(_: other::Wire) -> Self { Self }
                }
            ''',
            "other.rs": '''
                use serde::{Deserialize, Deserializer};
                #[derive(Debug)] pub struct Wire;
                impl<'de> Deserialize<'de> for Wire {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }
                }
            ''',
        })
        self.assertEqual(status, 1, output)
        self.assertIn("Reader", output)

    def test_qualified_wire_accepts_its_actual_denying_owner(self) -> None:
        status, output = self.run_census({
            "lib.rs": '''
                use serde::Deserialize;
                mod other;
                #[derive(Debug)] struct Wire;
                #[derive(Deserialize)] #[serde(from = "crate::other::Wire")]
                struct Reader;
                impl From<other::Wire> for Reader {
                    fn from(_: other::Wire) -> Self { Self }
                }
            ''',
            "other.rs": '''
                use serde::Deserialize;
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                pub struct Wire { value: String }
            ''',
        })
        self.assertEqual(status, 0, output)

    def test_nested_module_deny_does_not_prove_an_outer_bare_wire(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            mod hidden {
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Wire { value: String }
            }
            #[derive(Deserialize)] #[serde(from = "Wire")]
            struct Reader;
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("cannot resolve type Wire", output)

    def test_qualified_nested_wire_is_checked_in_its_inline_module(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Wire { value: String }
            mod nested {
                #[derive(Debug)] pub struct Wire;
            }
            #[derive(Deserialize)] #[serde(from = "self::nested::Wire")]
            struct Reader;
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader", output)

    def test_unresolved_qualified_wire_fails_closed(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Wire { value: String }
            #[derive(Deserialize)] #[serde(from = "missing::Wire")]
            struct Reader;
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("missing::Wire", output)

    def test_same_line_declarations_and_nested_cfg_are_censused(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[cfg(test)] mod tests { #[derive(Deserialize)] struct Ignored { value: String } }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)] struct First { value: String }
            #[derive(Deserialize)] struct Second { value: String }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Second", output)
        self.assertNotIn("Ignored", output)

    def test_comments_and_raw_strings_do_not_create_declarations(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            const TEXT: &str = r#"
            #[derive(Deserialize)] struct Fake { value: String }
            "#;
            /* #[derive(Deserialize)] struct AlsoFake { value: String } */
            #[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Real { value: String }
        '''})
        self.assertEqual(status, 0, output)

    def test_comments_between_attributes_do_not_hide_a_deserialize_derive(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)]
            // A production annotation between derive and another attribute.
            #[allow(dead_code)]
            struct Reader { value: String }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader", output)

    def test_missing_source_root_cannot_be_an_empty_success(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            missing = Path(directory) / "missing"
            with patch.object(census, "ROOTS", (missing,)):
                with self.assertRaises(FileNotFoundError):
                    list(census.source_files())

    def test_source_enumeration_propagates_directory_errors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            def inaccessible(base, onerror):
                onerror(PermissionError(f"cannot enumerate {base}"))
                return iter(())

            with patch.object(census, "ROOTS", (Path(directory),)), \
                    patch.object(census.os, "walk", side_effect=inaccessible):
                with self.assertRaises(PermissionError):
                    list(census.source_files())

    def test_an_unattributed_hand_reader_is_not_an_unknown_scalar(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            struct Open;
            impl<'de> serde::Deserialize<'de> for Open {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    let _ = serde_json::Value::deserialize(d)?;
                    Ok(Self)
                }
            }
            #[derive(Deserialize)] #[serde(from = "Open")]
            struct Reader;
            impl From<Open> for Reader { fn from(_: Open) -> Self { Self } }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader", output)

    def test_an_alias_cannot_hide_an_open_reader(self) -> None:
        for target in ["Open", "open"]:
            with self.subTest(target=target):
                status, output = self.run_census({"lib.rs": f'''
                    struct {target};
                    type Wire = {target};
                    type Alias = Wire;
                    #[derive(Deserialize)] #[serde(from = "Alias")]
                    struct Reader;
                '''})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader", output)

    def test_an_alias_to_a_denying_reader_is_checked(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Closed { value: u8 }
            type Wire = Closed;
            type Alias = Wire;
            #[derive(Deserialize)] #[serde(from = "Alias")]
            struct Reader;
        '''})
        self.assertEqual(status, 0, output)

    def test_an_alias_to_a_scalar_does_not_invent_an_object_key_set(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            type Scalar = u8;
            type Wire = [Scalar; 2];
            #[derive(Deserialize)] #[serde(from = "Wire")]
            struct Reader;
        '''})
        self.assertEqual(status, 0, output)

    def test_literal_and_comment_text_cannot_supply_serde_proof(self) -> None:
        attributes = [
            '#[doc = "serde(deny_unknown_fields)"]',
            '#[doc = r#"serde(deny_unknown_fields)"#]',
            '#[allow(dead_code)] /* serde(deny_unknown_fields) */',
            '#[serde(rename = "deny_unknown_fields")]',
            '#[serde(rename = "transparent")]',
            '#[serde(rename = "untagged")]',
        ]
        for attrs in attributes:
            with self.subTest(attrs=attrs):
                status, output = self.run_census({"lib.rs": f'''
                    #[derive(Deserialize)] {attrs}
                    struct Reader {{ value: u8 }}
                '''})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader", output)

    def test_inactive_conditional_deny_is_not_unconditional_proof(self) -> None:
        for attrs in [
            '#[cfg_attr(feature = "strict", serde(deny_unknown_fields))]',
            '#[cfg_attr(all(), cfg_attr(feature = "strict", serde(deny_unknown_fields)))]',
        ]:
            with self.subTest(attrs=attrs):
                status, output = self.run_census({"lib.rs": f'''
                    #[derive(Deserialize)] {attrs}
                    struct Reader {{ value: u8 }}
                '''})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader", output)

    def test_from_reader_takes_precedence_over_local_deny(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            struct Open;
            #[derive(Deserialize)] #[serde(deny_unknown_fields, from = "Open")]
            struct Reader;
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader", output)

    def test_from_reader_can_supply_its_own_deny(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Closed { value: u8 }
            #[derive(Deserialize)] #[serde(deny_unknown_fields, from = "Closed")]
            struct Reader;
        '''})
        self.assertEqual(status, 0, output)

    def test_conditional_reader_replacement_cannot_inherit_local_deny(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            struct Open;
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            #[cfg_attr(feature = "open", serde(from = "Open"))]
            struct Reader { value: u8 }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader", output)

    def test_conditional_serialization_metadata_does_not_hide_actual_deny(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            #[cfg_attr(feature = "named", serde(rename = "a,b)"))]
            struct Reader { value: u8 }
        '''})
        self.assertEqual(status, 0, output)

    def test_documentation_cannot_create_a_deserialize_derive(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[doc = "derive(Deserialize)"]
            struct Reader { value: u8 }
        '''})
        self.assertEqual(status, 0, output)

    def test_conditional_deserialize_derives_are_inventoried(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[cfg_attr(all(), cfg_attr(feature = "wire", derive(serde::Deserialize)))]
            struct Reader { value: u8 }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader", output)

    def test_spaces_and_comments_in_derive_paths_do_not_hide_readers(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(serde /* qualification */ :: Deserialize)]
            struct Reader { value: u8 }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader", output)

    def test_raw_type_literals_follow_the_actual_reader(self) -> None:
        for target, status in [("Closed", 0), ("Open", 1)]:
            with self.subTest(target=target):
                result, output = self.run_census({"lib.rs": f'''
                    struct Open;
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    struct Closed {{ value: u8 }}
                    #[derive(Deserialize)] #[serde(from = r##"{target}"##)]
                    struct Reader;
                '''})
                self.assertEqual(result, status, output)

    def test_comments_around_the_reader_literal_preserve_its_route(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Closed { value: u8 }
            #[derive(Deserialize)]
            #[serde(from /* before */ = /* after */ "Closed" /* trailing */)]
            struct Reader;
        '''})
        self.assertEqual(status, 0, output)

    def test_path_spacing_cannot_turn_a_foreign_reader_into_a_local_one(self) -> None:
        for path in ['other :: Wire', 'other /* qualification */ :: Wire']:
            with self.subTest(path=path):
                status, output = self.run_census({"lib.rs": f'''
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    struct Wire {{ value: u8 }}
                    mod other {{ pub struct Wire; }}
                    #[derive(Deserialize)] #[serde(from = "{path}")]
                    struct Reader;
                '''})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader", output)

    def test_tagged_unit_enum_has_an_object_key_set(self) -> None:
        for metadata in ['tag = "kind"', 'tag = "kind", content = "value"']:
            with self.subTest(metadata=metadata):
                status, output = self.run_census({"lib.rs": f'''
                    #[derive(Deserialize)] #[serde({metadata})]
                    enum Reader {{ A, B }}
                '''})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader", output)

    def test_adjacent_tagged_unit_enum_can_deny_unknown_keys(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            enum Reader { A, B }
        '''})
        self.assertEqual(status, 0, output)

    def test_internal_tagged_unit_arm_ignores_local_deny(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { A, B { value: u8 } }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_internal_tagged_empty_struct_and_skipped_unit_arms(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader {
                /// Empty, but still checks the object keys.
                A {},
                #[doc = "Comment commas, and delimiters [ ( { stay text"]
                B { value: u8 },
                #[serde(skip_deserializing)]
                C,
            }
        '''})
        self.assertEqual(status, 0, output)

    def test_documentation_cannot_make_a_variant_untagged(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            enum Reader {
                #[doc = "serde(untagged)"]
                A { value: u8 },
            }
        '''})
        self.assertEqual(status, 0, output)

    def test_conditional_untagged_variant_inherits_container_refusal(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            enum Reader {
                A,
                #[cfg_attr(feature = "open", serde(untagged))]
                B { value: u8 },
            }
        '''})
        self.assertEqual(status, 0, output)

    def test_untagged_inline_variant_requires_container_refusal(self) -> None:
        for container in ('tag = "kind"', 'untagged'):
            for deny in (False, True):
                with self.subTest(container=container, deny=deny):
                    metadata = container + (", deny_unknown_fields" if deny else "")
                    arm_attribute = '' if container == 'untagged' else '#[serde(untagged)]'
                    status, output = self.run_census({"lib.rs": f'''
                        #[derive(Deserialize)] #[serde({metadata})]
                        enum Reader {{ {arm_attribute} A {{ value: u8 }} }}
                    '''})
                    self.assertEqual(status, 0 if deny else 1, output)

    def test_internal_newtype_with_skipped_field_ignores_container_refusal(self) -> None:
        for field_attribute in (
            '#[serde(skip)]',
            '#[serde(skip_deserializing)]',
            '#[cfg_attr(feature = "omit", serde(skip_deserializing))]',
        ):
            with self.subTest(field_attribute=field_attribute):
                status, output = self.run_census({"lib.rs": f'''
                    #[derive(Default, Deserialize)] #[serde(deny_unknown_fields)]
                    struct Closed {{}}
                    #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                    enum Reader {{ A({field_attribute} Closed) }}
                '''})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader::A", output)

    def test_internal_untagged_unit_arm_reads_null_without_object_keys(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { A {}, #[serde(untagged)] B }
        '''})
        self.assertEqual(status, 0, output)

    def test_conditional_untagged_unit_arm_must_deny_its_tagged_route(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader {
                A {},
                #[cfg_attr(feature = "untagged", serde(untagged))]
                B,
            }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::B", output)

    def test_untagged_tuple_checks_each_payload_reader(self) -> None:
        for fields, admitted in (("Closed, String", True), ("String, Open", False)):
            with self.subTest(fields=fields):
                status, output = self.run_census({"lib.rs": f'''
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    struct Closed {{ value: String }}
                    struct Open;
                    #[derive(Deserialize)] #[serde(untagged)]
                    enum Reader {{ A({fields}) }}
                '''})
                self.assertEqual(status, 0 if admitted else 1, output)

    def test_deny_follows_external_internal_adjacent_and_untagged_newtypes(self) -> None:
        open_reader = '''
            struct Open;
            impl<'de> serde::Deserialize<'de> for Open {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    let _ = serde_json::Value::deserialize(d)?;
                    Ok(Self)
                }
            }
        '''
        routes = {
            "external": "deny_unknown_fields",
            "internal": 'tag = "kind", deny_unknown_fields',
            "adjacent": 'tag = "kind", content = "value", deny_unknown_fields',
            "untagged": "untagged, deny_unknown_fields",
        }
        for route, metadata in routes.items():
            with self.subTest(route=route):
                status, output = self.run_census({"lib.rs": f'''
                    {open_reader}
                    #[derive(Deserialize)] #[serde({metadata})]
                    enum Reader {{ A(Open) }}
                '''})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader::A", output)

    def test_closed_scalar_sequence_and_free_form_payloads_remain_admitted(self) -> None:
        routes = {
            "external": "deny_unknown_fields",
            "internal": 'tag = "kind", deny_unknown_fields',
            "adjacent": 'tag = "kind", content = "value", deny_unknown_fields',
            "untagged": "untagged, deny_unknown_fields",
        }
        payloads = {
            "closed": "Closed",
            "scalar": "u8",
            "sequence": "Vec<u8>",
            "free_form": "serde_json::Value",
        }
        for route, metadata in routes.items():
            for name, payload in payloads.items():
                with self.subTest(route=route, payload=name):
                    status, output = self.run_census({"lib.rs": f'''
                        #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                        struct Closed {{ value: u8 }}
                        #[derive(Deserialize)] #[serde({metadata})]
                        enum Reader {{ A({payload}) }}
                    '''})
                    self.assertEqual(status, 0, output)

    def test_free_form_map_custom_reader_remains_admitted(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            use std::collections::BTreeMap;
            fn map_reader<'de, D>(deserializer: D) -> Result<BTreeMap<String, u8>, D::Error>
            where D: serde::Deserializer<'de> {
                BTreeMap::deserialize(deserializer)
            }
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader {
                A(#[serde(deserialize_with = "map_reader")] BTreeMap<String, u8>),
            }
        '''})
        self.assertEqual(status, 0, output)

    def test_optional_and_transparent_open_payloads_are_followed(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            struct Open;
            impl<'de> serde::Deserialize<'de> for Open {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    let _ = serde_json::Value::deserialize(d)?;
                    Ok(Self)
                }
            }
            #[derive(Deserialize)] #[serde(transparent)]
            struct Wrapper(Open);
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { Optional(Option<Open>), Wrapped(Wrapper) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::Optional", output)
        self.assertIn("Reader::Wrapped", output)

    def test_handwritten_denied_wire_proves_a_newtype_payload(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            struct Closed;
            impl<'de> serde::Deserialize<'de> for Closed {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Wire { value: u8 }
                    let _ = Wire::deserialize(d)?;
                    Ok(Self)
                }
            }
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { A(Closed) }
        '''})
        self.assertEqual(status, 0, output)

    def test_handwritten_open_visitor_reader_fails_closed(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            struct Open;
            impl<'de> serde::Deserialize<'de> for Open {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    d.deserialize_map(OpenVisitor)
                }
            }
            struct OpenVisitor;
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { A(Open) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_forwarding_deserializer_macro_still_follows_the_payload(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            macro_rules! selection_field_deserializer {
                ($name:ident, $field:literal) => {
                    fn $name<'de, D, T>(deserializer: D) -> Result<T, D::Error>
                    where D: serde::Deserializer<'de>, T: serde::Deserialize<'de> {
                        T::deserialize(deserializer)
                    }
                };
            }
            selection_field_deserializer!(deserialize_open, "open");
            struct Open;
            impl<'de> serde::Deserialize<'de> for Open {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    let _ = serde_json::Value::deserialize(d)?;
                    Ok(Self)
                }
            }
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader {
                A(#[serde(deserialize_with = "deserialize_open")] Open),
            }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_unproved_deserializer_macro_body_cannot_certify_a_payload(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            macro_rules! selection_field_deserializer {
                ($name:ident, $field:literal) => {
                    fn $name<'de, D, T>(deserializer: D) -> Result<T, D::Error>
                    where D: serde::Deserializer<'de>, T: serde::Deserialize<'de> {
                        serde_json::Value::deserialize(deserializer)
                            .map(|_| panic!("open"))
                    }
                };
            }
            selection_field_deserializer!(deserialize_closed, "closed");
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Closed { value: u8 }
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader {
                A(#[serde(deserialize_with = "deserialize_closed")] Closed),
            }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_checked_geometry_macro_expands_its_actual_inner_reader(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            macro_rules! checked_feature_geometry {
                ($name:ident, $raw:ident) => {
                    #[derive(Serialize)] #[serde(transparent)]
                    struct $name($raw);
                    impl<'de> serde::Deserialize<'de> for $name {
                        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                            Ok(Self($raw::deserialize(d)?))
                        }
                    }
                };
            }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Closed { value: u8 }
            checked_feature_geometry!(ClosedGeometry, Closed);
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { A(ClosedGeometry) }
        '''})
        self.assertEqual(status, 0, output)

    def test_checked_geometry_macro_without_raw_reader_proof_fails_closed(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            macro_rules! checked_feature_geometry {
                ($name:ident, $raw:ident) => {
                    #[derive(Serialize)] #[serde(transparent)]
                    struct $name($raw);
                    impl<'de> serde::Deserialize<'de> for $name {
                        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                            serde_json::Value::deserialize(d).map(|_| panic!("open"))
                        }
                    }
                };
            }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Closed { value: u8 }
            checked_feature_geometry!(ClosedGeometry, Closed);
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { A(ClosedGeometry) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_conflicting_deserializer_macro_contracts_do_not_admit_a_helper(self) -> None:
        status, output = self.run_census({
            "first.rs": '''
                macro_rules! selection_field_deserializer {
                    ($name:ident, $field:literal) => {
                        fn $name<'de, D, T>(deserializer: D) -> Result<T, D::Error>
                        where D: serde::Deserializer<'de>, T: serde::Deserialize<'de> {
                            T::deserialize(deserializer)
                        }
                    };
                }
            ''',
            "second.rs": '''
                macro_rules! selection_field_deserializer {
                    ($name:ident, $field:literal) => {
                        fn $name<'de, D, T>(deserializer: D) -> Result<T, D::Error>
                        where D: serde::Deserializer<'de>, T: serde::Deserialize<'de> {
                            serde_json::Value::deserialize(deserializer)
                                .map(|_| panic!("open"))
                        }
                    };
                }
                selection_field_deserializer!(deserialize_closed, "closed");
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Closed { value: u8 }
                #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                enum Reader {
                    A(#[serde(deserialize_with = "deserialize_closed")] Closed),
                }
            ''',
        })
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_declared_id_macro_expansion_proves_qualified_payload(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            mod ids {
                macro_rules! id_type {
                    ($name:ident) => {
                        #[derive(Deserialize)]
                        #[serde(transparent)]
                        struct $name(String);
                    };
                }
                id_type!(ClosedId);
            }
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { A(crate::ids::ClosedId) }
        '''})
        self.assertEqual(status, 0, output)

    def test_recursive_conversion_routes_fail_closed_without_rejecting_denied_structs(self) -> None:
        cycle_status, cycle_output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(try_from = "Second")]
            struct First;
            #[derive(Deserialize)] #[serde(try_from = "First")]
            struct Second;
        '''})
        self.assertEqual(cycle_status, 1, cycle_output)
        self.assertIn("First", cycle_output)
        self.assertIn("Second", cycle_output)

        terminating_status, terminating_output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Node { next: Option<Box<Node>> }
        '''})
        self.assertEqual(terminating_status, 0, terminating_output)

        recursive_enum_status, recursive_enum_output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            enum Node { Next(Box<Node>), Leaf }
        '''})
        self.assertEqual(recursive_enum_status, 0, recursive_enum_output)

    def test_unresolved_generic_payload_fails_closed(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            enum Reader<T> { A(T) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_local_type_names_cannot_impersonate_builtin_readers(self) -> None:
        for name, declaration in (
            ("Map", '''
                struct Map;
                impl<'de> serde::Deserialize<'de> for Map {
                    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }
                }
            '''),
            ("Vec", '''
                struct Vec<T>(T);
                impl<'de, T> serde::Deserialize<'de> for Vec<T>
                where T: serde::Deserialize<'de> {
                    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        Ok(Self(T::deserialize(d)?))
                    }
                }
            '''),
        ):
            with self.subTest(name=name):
                status, output = self.run_census({"lib.rs": f'''
                    {declaration}
                    #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                    enum Reader {{ A({name}{"<u8>" if name == "Vec" else ""}) }}
                '''})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader::A", output)

    def test_compiled_route_counterexamples_fail_closed(self) -> None:
        preamble = '''
            use serde::{Deserialize, Deserializer};
            fn discard_json<'de, D: Deserializer<'de>>(d: D) -> Result<(), D::Error> {
                let _ = serde_json::Value::deserialize(d)?;
                Ok(())
            }
        '''
        cases = {
            "qualified scalar": {
                "lib.rs": preamble + '''
                    mod hostile;
                    #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                    enum Reader { A(hostile::String) }
                ''',
                "hostile.rs": '''
                    use serde::{Deserialize, Deserializer};
                    pub struct String;
                    impl<'de> Deserialize<'de> for String {
                        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                            let _ = serde_json::Value::deserialize(d)?;
                            Ok(Self)
                        }
                    }
                ''',
            },
            "nonconsuming scalar": {
                "lib.rs": preamble + '''
                    struct Open;
                    impl<'de> Deserialize<'de> for Open {
                        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                            let _unused = String::deserialize::<
                                serde::de::value::StrDeserializer<serde::de::value::Error>
                            >;
                            discard_json(d)?;
                            Ok(Self)
                        }
                    }
                    #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                    enum Reader { A(Open) }
                ''',
            },
            "macro decoy": {
                "lib.rs": preamble + '''
                    macro_rules! checked_scalar {
                        ($name:ident) => {
                            #[derive(serde::Serialize)] #[serde(transparent)]
                            struct $name(f64);
                            impl<'de> Deserialize<'de> for $name {
                                fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                                    let _unused = f64::deserialize::<
                                        serde::de::value::F64Deserializer<serde::de::value::Error>
                                    >;
                                    Ok(Self(0.0))
                                }
                            }
                        }
                    }
                    checked_scalar!(Open);
                    #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                    enum Reader { A(Open) }
                ''',
            },
            "macro helper decoy": {
                "lib.rs": '''
                    macro_rules! checked_scalar {
                        ($name:ident) => {
                            #[derive(serde::Serialize)] #[serde(transparent)]
                            struct $name(f64);
                            impl<'de> serde::Deserialize<'de> for $name {
                                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                                    Ok(Self(0.0))
                                }
                                fn helper<D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
                                    f64::deserialize(d)
                                }
                            }
                        }
                    }
                    checked_scalar!(Open);
                    #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                    enum Reader { A(Open) }
                ''',
            },
            "qualified helper": {
                "lib.rs": preamble + '''
                    mod hostile;
                    fn read<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
                        String::deserialize(d)
                    }
                    #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                    enum Reader {
                        A(#[serde(deserialize_with = "hostile::read")] String),
                    }
                ''',
                "hostile.rs": '''
                    use serde::{Deserialize, Deserializer};
                    pub fn read<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(String::new())
                    }
                ''',
            },
        }
        for name, files in cases.items():
            with self.subTest(name=name):
                status, output = self.run_census(files)
                self.assertEqual(status, 1, output)
                self.assertIn("Reader", output)

    def test_rebound_input_cannot_certify_a_manual_macro_or_helper_reader(self) -> None:
        preamble = '''
            use serde::{Deserialize, Deserializer};
            fn discard<'de, D: Deserializer<'de>>(input: D) -> Result<(), D::Error> {
                let _ = serde_json::Value::deserialize(input)?;
                Ok(())
            }
        '''
        for body in [
            '''let actual = d;
               let d = serde::de::value::F64Deserializer::<D::Error>::new(1.0);
               let _ = f64::deserialize(d)?;
               discard(actual)?;''',
            '''let actual = d;
               let (d,) = (serde::de::value::F64Deserializer::<D::Error>::new(1.0),);
               let _ = f64::deserialize(d)?;
               discard(actual)?;''',
            '''let _ = (|d: serde::de::value::F64Deserializer<D::Error>| {
                   f64::deserialize(d)
               })(serde::de::value::F64Deserializer::<D::Error>::new(1.0))?;
               discard(d)?;''',
        ]:
            readers = {
                "manual": f'''
                    struct Open;
                    impl<'de> Deserialize<'de> for Open {{
                        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{
                            {body} Ok(Self)
                        }}
                    }}
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    enum Reader {{ A(Open) }}
                ''',
                "macro": f'''
                    macro_rules! checked_scalar {{
                        ($name:ident) => {{
                            #[derive(serde::Serialize)] #[serde(transparent)] struct $name(f64);
                            impl<'de> Deserialize<'de> for $name {{
                                fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{
                                    {body} Ok(Self(0.0))
                                }}
                            }}
                        }};
                    }}
                    checked_scalar!(Open);
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    enum Reader {{ A(Open) }}
                ''',
                "helper": f'''
                    fn read<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {{
                        {body} Ok(0.0)
                    }}
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    enum Reader {{ A(#[serde(deserialize_with = "read")] f64) }}
                ''',
            }
            for route, reader in readers.items():
                with self.subTest(route=route, body=body):
                    files = {"lib.rs": preamble + reader}
                    status, output = self.run_census(files)
                    self.assertEqual(status, 1, output)
                    mutated, output = self.run_mutated_census(
                        files,
                        "direct_input_argument(arguments, single_use)",
                        "direct_input_argument(arguments, bindings)",
                    )
                    self.assertEqual(mutated, 0, output)

    def test_route_counterexample_mutations_are_load_bearing(self) -> None:
        qualified_scalar = {
            "lib.rs": '''
                mod hostile;
                #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                enum Reader { A(hostile::String) }
            ''',
            "hostile.rs": '''
                use serde::{Deserialize, Deserializer};
                pub struct String;
                impl<'de> Deserialize<'de> for String {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }
                }
            ''',
        }
        scalar_old = '''if is_keyless_path(path, index) or is_builtin_path(path, MAP_TYPES, index):
            return True, "scalar, sequence, or map outer reader"'''
        scalar_mutated, scalar_output = self.run_mutated_census(
            qualified_scalar,
            scalar_old,
            '''if is_keyless_path(path, index) or base in SCALAR_TYPES or base in SEQUENCE_TYPES or base in MAP_TYPES:
            return True, "scalar, sequence, or map outer reader"''',
        )
        self.assertEqual(scalar_mutated, 0, scalar_output)

        nonconsuming = {"lib.rs": '''
            struct Open;
            impl<'de> serde::Deserialize<'de> for Open {
                fn deserialize<D: serde::Deserializer<'de>>(_d: D) -> Result<Self, D::Error> {
                    let _unused = String::deserialize::<
                        serde::de::value::StrDeserializer<serde::de::value::Error>
                    >;
                    Ok(Self)
                }
            }
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { A(Open) }
        '''}
        binding_old = '''calls = [
            call for call in DESERIALIZE_CALL_RE.finditer(code)
            if call_uses_input(code, call, reader.input_bindings)
        ]'''
        binding_mutated, binding_output = self.run_mutated_census(
            nonconsuming,
            binding_old,
            '''calls = list(DESERIALIZE_CALL_RE.finditer(code))''',
        )
        self.assertEqual(binding_mutated, 0, binding_output)

        macro_decoy = {"lib.rs": '''
            macro_rules! checked_scalar {
                ($name:ident) => {
                    #[derive(serde::Serialize)] #[serde(transparent)]
                    struct $name(f64);
                    impl<'de> serde::Deserialize<'de> for $name {
                        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                            let _unused = f64::deserialize::<
                                serde::de::value::F64Deserializer<serde::de::value::Error>
                            >;
                            Ok(Self(0.0))
                        }
                    }
                }
            }
            checked_scalar!(Open);
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { A(Open) }
        '''}
        macro_old = 'and macro_reader_contract(macro_body, "f64")'
        macro_mutated, macro_output = self.run_mutated_census(
            macro_decoy,
            macro_old,
            'and bool(re.search(r"\\bf64\\s*::\\s*deserialize\\b", macro_body))',
        )
        self.assertEqual(macro_mutated, 0, macro_output)

        qualified_helper = {
            "lib.rs": '''
                mod hostile;
                fn read<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
                    String::deserialize(d)
                }
                #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                enum Reader {
                    A(#[serde(deserialize_with = "hostile::read")] String),
                }
            ''',
            "hostile.rs": '''
                use serde::{Deserialize, Deserializer};
                pub fn read<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
                    let _ = serde_json::Value::deserialize(d)?;
                    Ok(String::new())
                }
            ''',
        }
        helper_old = '''path = canonical_path(name).lstrip(":")
        parts = path.split("::")'''
        helper_mutated, helper_output = self.run_mutated_census(
            qualified_helper,
            helper_old,
            '''path = canonical_path(name).lstrip(":")
        parts = path.split("::")
        if len(parts) > 1:
            return [
                function for function in helper_functions.get(parts[-1], ())
                if function.path == owner.path
            ]''',
        )
        self.assertEqual(helper_mutated, 0, helper_output)

    def run_mutated_census(
        self, files: dict[str, str], old: str, new: str
    ) -> tuple[int, str]:
        source = Path(__file__).with_name("check-deny-census.py").read_text(
            encoding="utf-8"
        )
        self.assertEqual(source.count(old), 1)
        namespace = {
            "__name__": "mutated_deny_census",
            "__file__": str(Path(__file__).with_name("check-deny-census.py")),
        }
        exec(compile(source.replace(old, new), "<mutated-census>", "exec"), namespace)
        with tempfile.TemporaryDirectory() as directory:
            paths = []
            for name, content in files.items():
                path = Path(directory) / "src" / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding="utf-8")
                paths.append(path)
            namespace["source_files"] = lambda: paths
            namespace["EXCEPTIONS"] = {}
            output = io.StringIO()
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                status = namespace["main"]()
            return status, output.getvalue()

    def test_mutations_prove_each_new_admission_check_is_load_bearing(self) -> None:
        open_reader = '''
            struct Open;
            impl<'de> serde::Deserialize<'de> for Open {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    let _ = serde_json::Value::deserialize(d)?;
                    Ok(Self)
                }
            }
        '''
        route_fixture = {"lib.rs": f'''
            {open_reader}
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader {{ A(Open) }}
        '''}
        fixed_status, fixed_output = self.run_census(route_fixture)
        self.assertEqual(fixed_status, 1, fixed_output)
        payload_old = '''if has_serde_flag(item.attrs, "deny_unknown_fields"):
                if item.kind == "enum":
                    return check_tuple_payloads(item, untagged_arms(item))
                return True'''
        payload_mutated, payload_output = self.run_mutated_census(
            route_fixture, payload_old, '''if has_serde_flag(item.attrs, "deny_unknown_fields"):
                return True'''
        )
        self.assertEqual(payload_mutated, 0, payload_output)

        manual_old = '''if manual:
                return all(manual_reader_passes(item, reader) for reader in manual)'''
        manual_mutated, manual_output = self.run_mutated_census(
            route_fixture, manual_old, '''if manual:
                return True'''
        )
        self.assertEqual(manual_mutated, 0, manual_output)

        cycle_fixture = {"lib.rs": '''
            #[derive(Deserialize)] #[serde(try_from = "Second")]
            struct First;
            #[derive(Deserialize)] #[serde(try_from = "First")]
            struct Second;
        '''}
        cycle_old = '''if identity in checking:
            # A conversion/transparent cycle is not a finite key-refusal
            # proof. A directly denied container is already the boundary for
            # this recursive route; other conversion/transparent cycles have
            # no finite reader proof and remain rejected.
            if has_serde_flag(item.attrs, "deny_unknown_fields"):
                return True
            return False'''
        cycle_mutated, cycle_output = self.run_mutated_census(
            cycle_fixture, cycle_old, '''if identity in checking:
            if has_serde_flag(item.attrs, "deny_unknown_fields"):
                return True
            return True'''
        )
        self.assertEqual(cycle_mutated, 0, cycle_output)

        macro_fixture = {"lib.rs": '''
            mod ids {
                macro_rules! id_type {
                    ($name:ident) => {
                        #[derive(Deserialize)]
                        #[serde(transparent)]
                        struct $name(String);
                    };
                }
                id_type!(ClosedId);
            }
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader { A(crate::ids::ClosedId) }
        '''}
        macro_old = "generated = macro_generated_items(path, raw_items[path], templates)"
        macro_mutated, macro_output = self.run_mutated_census(
            macro_fixture, macro_old, "generated = []"
        )
        self.assertEqual(macro_mutated, 1, macro_output)

        generic_fixture = {"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            enum Reader<T> { A(T) }
        '''}
        generic_old = 'return False, "cannot resolve bare generic/import statically"'
        generic_mutated, generic_output = self.run_mutated_census(
            generic_fixture,
            generic_old,
            'return True, "unresolved bare generic/import (mutated)"',
        )
        self.assertEqual(generic_mutated, 0, generic_output)

        bad_macro_fixture = {"lib.rs": '''
            macro_rules! selection_field_deserializer {
                ($name:ident, $field:literal) => {
                    fn $name<'de, D, T>(deserializer: D) -> Result<T, D::Error>
                    where D: serde::Deserializer<'de>, T: serde::Deserialize<'de> {
                        serde_json::Value::deserialize(deserializer)
                            .map(|_| panic!("open"))
                    }
                };
            }
            selection_field_deserializer!(deserialize_closed, "closed");
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Closed { value: u8 }
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader {
                A(#[serde(deserialize_with = "deserialize_closed")] Closed),
            }
        '''}
        macro_contract_old = '''if contracts.get(call.group("macro")) == "forward":
                forwarders.add(name)'''
        macro_contract_mutated, macro_contract_output = self.run_mutated_census(
            bad_macro_fixture,
            macro_contract_old,
            '''if True:
                forwarders.add(name)''',
        )
        self.assertEqual(macro_contract_mutated, 0, macro_contract_output)


if __name__ == "__main__":
    unittest.main()
