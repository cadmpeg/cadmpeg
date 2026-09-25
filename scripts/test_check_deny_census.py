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
            # The absent-key rule has its own scope, the whole crates tree.
            # These fixtures state the unknown-key rule only, so that pass is
            # held out here and exercised by `run_absent_key_census` below.
            with patch.object(census, "source_files", return_value=paths), \
                    patch.object(census, "EXCEPTIONS", {}), \
                    patch.object(census, "absent_key_failures", return_value=[]), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                status = census.main()
            return status, output.getvalue()

    def run_absent_key_census(self, files: dict[str, str]) -> list[str]:
        """The absent-key rule's findings over one fixture tree."""
        with tempfile.TemporaryDirectory() as directory:
            paths = []
            for name, source in files.items():
                path = Path(directory) / "crates" / "fixture" / "src" / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")
                paths.append(path)
            with patch.object(census, "absent_key_source_files", return_value=paths), \
                    patch.object(census, "ABSENT_KEY_EXCEPTIONS", {}):
                return [message for _, _, message in census.absent_key_failures()]

    def run_absent_key_main(self, files: dict[str, str]) -> tuple[int, str]:
        """The whole census over one fixture tree, absent-key rule included."""
        with tempfile.TemporaryDirectory() as directory:
            paths = []
            for name, source in files.items():
                path = Path(directory) / "crates" / "fixture" / "src" / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")
                paths.append(path)
            output = io.StringIO()
            with patch.object(census, "source_files", return_value=[]), \
                    patch.object(census, "absent_key_source_files",
                                 return_value=paths), \
                    patch.object(census, "ABSENT_KEY_EXCEPTIONS", {}), \
                    patch.object(census, "ABSENT_KEY_PROJECTIONS", {}), \
                    contextlib.redirect_stdout(output), \
                    contextlib.redirect_stderr(output):
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

    def test_imported_aliases_resolve_before_builtin_reader_shortcuts(self) -> None:
        status, output = self.run_census({
            "lib.rs": '''
                use serde::Deserialize;
                mod hostile;
                use hostile::{Open as String};
                #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                enum Reader { A(String) }
            ''',
            "hostile.rs": '''
                use serde::{Deserialize, Deserializer};
                pub struct Open;
                impl<'de> Deserialize<'de> for Open {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }
                }
            ''',
        })
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

        status, output = self.run_census({
            "lib.rs": '''
                use serde::Deserialize;
                use std::string::String as Scalar;
                #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                enum Reader { A(Scalar) }
            ''',
        })
        self.assertEqual(status, 0, output)

        status, output = self.run_census({
            "lib.rs": '''
                use serde::Deserialize;
                struct String;
                use std::string::String as Scalar;
                #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                enum Reader { A(Scalar) }
            '''
        })
        self.assertEqual(status, 0, output)

    def test_root_group_import_resolves_an_open_reader_alias(self) -> None:
        status, output = self.run_census({
            "lib.rs": '''
                use serde::Deserialize;
                mod hostile;
                use {hostile::Open as String};
                #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                enum Reader { A(String) }
            ''',
            "hostile.rs": '''
                use serde::{Deserialize, Deserializer};
                pub struct Open;
                impl<'de> Deserialize<'de> for Open {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }
                }
            ''',
        })
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_deepest_block_import_shadows_an_outer_alias(self) -> None:
        status, output = self.run_census({
            "lib.rs": '''
                use serde::Deserialize;
                mod hostile;
                use hostile::Open as Scalar;
                fn local() {
                    use std::string::String as Scalar;
                    #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                    enum Reader { A(Scalar) }
                }
            ''',
            "hostile.rs": '''
                use serde::{Deserialize, Deserializer};
                pub struct Open;
                impl<'de> Deserialize<'de> for Open {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }
                }
            ''',
        })
        self.assertEqual(status, 0, output)

    def test_local_alias_chain_follows_an_imported_reader(self) -> None:
        status, output = self.run_census({
            "lib.rs": '''
                use serde::Deserialize;
                mod hostile;
                use hostile::Open as Scalar;
                type Wire = Scalar;
                #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                enum Reader { A(Wire) }
            ''',
            "hostile.rs": '''
                use serde::{Deserialize, Deserializer};
                pub struct Open;
                impl<'de> Deserialize<'de> for Open {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }
                }
            ''',
        })
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_function_local_import_cannot_impersonate_a_scalar_reader(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            use serde::{Deserialize, Deserializer};
            mod hostile {
                pub struct Open;
                impl<'de> Deserialize<'de> for Open {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }
                }
            }
            struct ReaderPayload;
            impl<'de> Deserialize<'de> for ReaderPayload {
                fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    use hostile::Open as String;
                    let _ = String::deserialize(d)?;
                    Ok(Self)
                }
            }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            enum Reader { A(ReaderPayload) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_imported_custom_reader_alias_is_followed(self) -> None:
        status, output = self.run_census({
            "lib.rs": '''
                use serde::{Deserialize, Deserializer};
                mod hostile;
                use hostile::read as read_open;
                #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                enum Reader {
                    A(#[serde(deserialize_with = "read_open")] u8),
                }
            ''',
            "hostile.rs": '''
                use serde::{Deserialize, Deserializer};
                pub fn read<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
                    let _ = serde_json::Value::deserialize(d)?;
                    Ok(0)
                }
            ''',
        })
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

        status, output = self.run_census({
            "lib.rs": '''
                use serde::{Deserialize, Deserializer};
                mod hostile;
                #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                enum Reader {
                    A(#[serde(deserialize_with = "hostile::read")] u8),
                }
            ''',
            "hostile.rs": '''
                use serde::{Deserialize, Deserializer};
                pub struct Open;
                impl<'de> Deserialize<'de> for Open {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }
                }
                pub fn read<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
                    use self::Open as String;
                    let _ = String::deserialize(d)?;
                    Ok(0)
                }
            ''',
        })
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_conditional_and_never_invoked_reader_calls_fail_closed(self) -> None:
        bodies = {
            "false branch": "if false { let _ = u8::deserialize(d)?; } Ok(Self)",
            "assigned if": "let _ = if false { Some(u8::deserialize(d)?) } else { None }; Ok(Self)",
            "assigned match": "let _ = match false { true => Some(u8::deserialize(d)?), false => None }; Ok(Self)",
            "early return": "return Ok(Self); let _ = u8::deserialize(d)?; Ok(Self)",
            "unused macro": "macro_rules! unused { () => { u8::deserialize(d)? }; } Ok(Self)",
            "macro argument parentheses": "macro_rules! discard { ($($t:tt)*) => { () } } discard!(u8::deserialize(d)?); Ok(Self)",
            "macro argument braces": "macro_rules! discard { ($($t:tt)*) => { () } } discard!{u8::deserialize(d)?}; Ok(Self)",
            "macro argument brackets": "macro_rules! discard { ($($t:tt)*) => { () } } discard![u8::deserialize(d)?]; Ok(Self)",
            "closure": "let _reader = || u8::deserialize(d); Ok(Self)",
            "closure block": "let _reader = || { u8::deserialize(d) }; Ok(Self)",
            "async block": "let _reader = async { u8::deserialize(d) }; Ok(Self)",
            "short circuit": "let _ = true || u8::deserialize(d).is_ok(); Ok(Self)",
        }
        for name, body in bodies.items():
            with self.subTest(name=name):
                status, output = self.run_census({"lib.rs": f'''
                    use serde::{{Deserialize, Deserializer}};
                    struct Open;
                    impl<'de> Deserialize<'de> for Open {{
                        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{
                            {body}
                        }}
                    }}
                    #[derive(Deserialize)] #[serde(untagged, deny_unknown_fields)]
                    enum Reader {{ A(Open) }}
                '''})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader::A", output)

    def test_wildcards_and_module_bindings_cannot_impersonate_external_readers(self) -> None:
        open_reader = '''
            use serde::{Deserialize, Deserializer};
            pub struct Open;
            impl<'de> Deserialize<'de> for Open {
                fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    let _ = serde_json::Value::deserialize(d)?;
                    Ok(Self)
                }
            }
        '''
        cases = [
            ('mod hostile { pub use super::Open as String; } use hostile::*;', 'String'),
            ('mod hostile { pub use super::Open as String; } use hostile::{*};', 'String'),
            ('''mod hostile { pub mod string { pub use crate::Open as String; } }
                use hostile as std;''', 'std::string::String'),
            ('''mod hostile { pub mod string { pub use crate::Open as String; } }
                use hostile as std; use std::string::String as Scalar;''', 'Scalar'),
            ('''mod std { pub mod string { pub use crate::Open as String; } }''',
             'std::string::String'),
            ('''pub mod hostile { pub mod string { pub use crate::Open as String; } }
                mod imported { pub use crate::hostile as std; } use imported::*;''',
             'std::string::String'),
        ]
        for bindings, payload in cases:
            with self.subTest(bindings=bindings):
                status, output = self.run_census({"lib.rs": open_reader + f'''
                    {bindings}
                    #[derive(Deserialize)] #[serde(untagged, deny_unknown_fields)]
                    enum Reader {{ A({payload}) }}
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    struct Document {{ reader: Reader }}
                '''})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader::A", output)

    def test_explicit_external_bindings_and_consumed_input_remain_proved(self) -> None:
        for bindings, payload in [
            ('use std::string::String as Scalar;', 'Scalar'),
            ('use std as standard;', 'standard::string::String'),
            ('use std as standard; use standard::string::String as Scalar;', 'Scalar'),
            ('mod std {}', '::std::string::String'),
        ]:
            with self.subTest(bindings=bindings):
                status, output = self.run_census({"lib.rs": f'''
                    use serde::{{Deserialize, Deserializer}};
                    {bindings}
                    struct Payload;
                    impl<'de> Deserialize<'de> for Payload {{
                        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{
                            let _ = {payload}::deserialize(d)?;
                            Ok(Self)
                        }}
                    }}
                    #[derive(Deserialize)] #[serde(untagged, deny_unknown_fields)]
                    enum Reader {{ A(Payload) }}
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    struct Document {{ reader: Reader }}
                '''})
                self.assertEqual(status, 0, output)

    def test_discarded_reader_errors_do_not_establish_refusal(self) -> None:
        for result_use in [
            "let _ = f64::deserialize(d); Ok(Self(0.0))",
            "let _ = f64::deserialize(d).unwrap_or_default(); Ok(Self(0.0))",
            "let _ = f64::deserialize(d).ok(); Ok(Self(0.0))",
            "f64::deserialize(d).or_else(|_| Ok(0.0)).map(Self)",
        ]:
            method = f'''
                fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{
                    {result_use}
                }}
            '''
            readers = {
                "manual": f'''
                    struct Payload(f64);
                    impl<'de> Deserialize<'de> for Payload {{ {method} }}
                ''',
                "macro": f'''
                    macro_rules! checked_scalar {{
                        ($name:ident) => {{
                            #[derive(serde::Serialize)] #[serde(transparent)]
                            struct $name(f64);
                            impl<'de> Deserialize<'de> for $name {{ {method} }}
                        }};
                    }}
                    checked_scalar!(Payload);
                ''',
                "helper": f'''
                    fn read<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {{
                        {result_use.replace('Self(0.0)', '0.0').replace('.map(Self)', '')}
                    }}
                    type Payload = f64;
                ''',
            }
            for route, reader in readers.items():
                with self.subTest(route=route, result_use=result_use):
                    field = '#[serde(deserialize_with = "read")] ' if route == "helper" else ""
                    files = {"lib.rs": f'''
                        use serde::{{Deserialize, Deserializer}};
                        {reader}
                        #[derive(Deserialize)] #[serde(untagged, deny_unknown_fields)]
                        enum Reader {{ A({field}Payload) }}
                        #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                        struct Document {{ reader: Reader }}
                    '''}
                    status, output = self.run_census(files)
                    self.assertEqual(status, 1, output)
                    self.assertIn("Reader::A", output)
                    status, output = self.run_mutated_census(
                        files,
                        "        or not call_propagates_error(code, call)\n",
                        "",
                    )
                    self.assertEqual(status, 0, output)

    def test_propagated_reader_errors_remain_proved(self) -> None:
        for body in [
            "Ok(Self(f64::deserialize(d)?))",
            "macro_rules! discard { ($($t:tt)*) => { () } } discard!(anything); Ok(Self(f64::deserialize(d)?))",
            "f64::deserialize(d).map(Self)",
            "f64::deserialize(d).and_then(|value| Ok(Self(value)))",
            "let value = f64::deserialize(d).map_err(D::Error::custom)?; Ok(Self(value))",
        ]:
            with self.subTest(body=body):
                status, output = self.run_census({"lib.rs": f'''
                    use serde::{{Deserialize, Deserializer}};
                    use serde::de::Error;
                    struct Payload(f64);
                    impl<'de> Deserialize<'de> for Payload {{
                        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{
                            {body}
                        }}
                    }}
                    #[derive(Deserialize)] #[serde(untagged, deny_unknown_fields)]
                    enum Reader {{ A(Payload) }}
                '''})
                self.assertEqual(status, 0, output)

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
                    expected = 1 if "|d:" in body else 0
                    self.assertEqual(mutated, expected, output)

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
        scalar_old = '''if is_keyless_path(resolved_path, index) or is_builtin_path(
            resolved_path, MAP_TYPES, index
        ):
            return True, "scalar, sequence, or map outer reader"'''
        scalar_mutated, scalar_output = self.run_mutated_census(
            qualified_scalar,
            scalar_old,
            '''if is_keyless_path(resolved_path, index) or base in SCALAR_TYPES or base in SEQUENCE_TYPES or base in MAP_TYPES:
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
            if call_has_direct_input(code, call, reader.input_bindings)
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
        helper_old = '''parts = path.split("::")'''
        helper_mutated, helper_output = self.run_mutated_census(
            qualified_helper,
            helper_old,
            "parts = path.rsplit(\"::\", 1)[-1:]",
        )
        self.assertEqual(helper_mutated, 0, helper_output)

    def test_import_and_control_flow_checks_are_load_bearing(self) -> None:
        imported_alias = {
            "lib.rs": '''
                use serde::Deserialize;
                mod hostile;
                use hostile::Open as String;
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                enum Reader { A(String) }
            ''',
            "hostile.rs": '''
                use serde::{Deserialize, Deserializer};
                pub struct Open;
                impl<'de> Deserialize<'de> for Open {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }
                }
            ''',
        }
        fixed_status, fixed_output = self.run_census(imported_alias)
        self.assertEqual(fixed_status, 1, fixed_output)
        import_old = '''imported = imported_path(path, owner)
        if imported is import_ambiguous:
            return False, f"ambiguous imported payload reader {path}"
        resolved_path = path if imported is import_missing else imported'''
        import_mutated, import_output = self.run_mutated_census(
            imported_alias,
            import_old,
            "imported = import_missing\n        resolved_path = path",
        )
        self.assertEqual(import_mutated, 0, import_output)

        dead_route = {"lib.rs": '''
            use serde::{Deserialize, Deserializer};
            struct Open;
            impl<'de> Deserialize<'de> for Open {
                fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    if false { let _ = u8::deserialize(d)?; }
                    Ok(Self)
                }
            }
            #[derive(Deserialize)] #[serde(untagged, deny_unknown_fields)]
            enum Reader { A(Open) }
        '''}
        fixed_status, fixed_output = self.run_census(dead_route)
        self.assertEqual(fixed_status, 1, fixed_output)
        flow_old = '''return any(
        not call_is_unconditional(code, call)
        or not call_propagates_error(code, call)
        for call in direct_input_calls(code, bindings)
    )'''
        flow_mutated, flow_output = self.run_mutated_census(
            dead_route,
            flow_old,
            "return False",
        )
        self.assertEqual(flow_mutated, 0, flow_output)

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
            # The absent-key rule has its own scope, the whole crates tree,
            # and these fixtures state the unknown-key rule only. Without this
            # the mutated census walks the real repository once per mutation.
            namespace["absent_key_failures"] = lambda: []
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

        # A bare name that is not the reader's own generic parameter and
        # resolves to no declaration keeps the lexical fail-closed route.
        import_fixture = {"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            enum Reader { A(Foreign) }
        '''}
        import_old = 'return False, "cannot resolve bare generic/import statically"'
        import_mutated, import_output = self.run_mutated_census(
            import_fixture,
            import_old,
            'return True, "unresolved bare generic/import (mutated)"',
        )
        self.assertEqual(import_mutated, 0, import_output)

        # A generic parameter with no default and no workspace instantiation
        # names no reader, and the empty-candidate refusal is what says so.
        generic_fixture = {"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            enum Reader<T> { A(T) }
        '''}
        generic_old = '''                return False, (
                    f"{route}generic parameter {name} has no default and no "
                    "workspace instantiation"
                )'''
        generic_mutated, generic_output = self.run_mutated_census(
            generic_fixture,
            generic_old,
            '''                return True, "empty candidate set (mutated)"''',
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



class AbsentKeyCensusTests(unittest.TestCase):
    """The absence spelling of an optional key whose writer omits it."""

    run_absent_key_census = DenyCensusTests.run_absent_key_census

    OMITTED = (
        '#[derive(serde::Deserialize)]\n'
        '#[serde(deny_unknown_fields)]\n'
        'struct Wire {\n'
        '    #[serde(default, skip_serializing_if = "Option::is_none"%s)]\n'
        '    key: Option<u32>,\n'
        '}\n'
    )

    def test_an_omitted_optional_key_without_a_declaration_is_named(self) -> None:
        findings = self.run_absent_key_census({"wire.rs": self.OMITTED % ""})
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("Wire.key", findings[0])
        self.assertIn("named_optional_field!", findings[0])

    def test_the_shared_helper_states_the_spelling(self) -> None:
        declared = self.OMITTED % ',\n        deserialize_with = "read_key"'
        shim = 'cadmpeg_core::named_optional_field!(read_key, u32, "key");\n'
        self.assertEqual(
            self.run_absent_key_census({"wire.rs": declared + shim}), []
        )

    def test_the_unnamed_refusal_is_not_a_declaration(self) -> None:
        declared = self.OMITTED % (
            ',\n        deserialize_with = "cadmpeg_core::absent_key::present"'
        )
        findings = self.run_absent_key_census({"wire.rs": declared})
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("Wire.key", findings[0])
        self.assertIn("named_optional_field!", findings[0])

    def test_a_hand_written_call_to_the_helper_is_no_declaration(self) -> None:
        declared = self.OMITTED % ',\n        deserialize_with = "read_key"'
        forwarder = (
            "fn read_key<'de, D: serde::Deserializer<'de>>(d: D)"
            " -> Result<Option<u32>, D::Error> {\n"
            '    cadmpeg_core::absent_key::named_present(d, "key")\n'
            "}\n"
        )
        findings = self.run_absent_key_census({"wire.rs": declared + forwarder})
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("Wire.key", findings[0])
        self.assertIn("named_optional_field!", findings[0])

    def test_an_omitted_optional_key_without_a_default_is_named(self) -> None:
        findings = self.run_absent_key_census({
            "wire.rs": (
                '#[derive(serde::Deserialize)]\n'
                'struct Wire {\n'
                '    #[serde(skip_serializing_if = "Option::is_none")]\n'
                '    key: Option<u32>,\n'
                '}\n'
            )
        })
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("serde(default)", findings[0])

    def test_a_flattened_field_states_its_own_keys(self) -> None:
        flattened = (
            '#[derive(serde::Deserialize)]\n'
            'struct Wire {\n'
            '    #[serde(default, flatten, skip_serializing_if = "Option::is_none")]\n'
            '    key: Option<u32>,\n'
            '}\n'
        )
        self.assertEqual(self.run_absent_key_census({"wire.rs": flattened}), [])

    def test_a_routed_container_is_not_read_field_by_field(self) -> None:
        routed = (
            '#[derive(serde::Deserialize)]\n'
            '#[serde(try_from = "Other")]\n'
            'struct Wire {\n'
            '    #[serde(default, skip_serializing_if = "Option::is_none")]\n'
            '    key: Option<u32>,\n'
            '}\n'
        )
        self.assertEqual(self.run_absent_key_census({"wire.rs": routed}), [])

    run_absent_key_main = DenyCensusTests.run_absent_key_main

    def test_a_reader_declared_in_another_file_is_not_this_field_s(self) -> None:
        field = self.OMITTED % ',\n        deserialize_with = "read_key"'
        elsewhere = 'cadmpeg_core::named_optional_field!(read_key, u32, "key");\n'
        status, output = self.run_absent_key_main(
            {"wire.rs": field, "other.rs": elsewhere}
        )
        self.assertEqual(status, 1, output)
        self.assertIn("wire.rs", output)
        self.assertIn("Wire.key", output)
        self.assertIn("no declaration this module reaches", output)

    def test_a_qualified_reader_resolves_only_through_the_module_it_names(self) -> None:
        field = self.OMITTED % ',\n        deserialize_with = "foo::read_key"'
        unrelated = 'cadmpeg_core::named_optional_field!(read_key, u32, "key");\n'
        status, output = self.run_absent_key_main(
            {"records/wire.rs": field, "alpha/foo.rs": unrelated}
        )
        self.assertEqual(status, 1, output)
        self.assertIn("records/wire.rs", output)
        self.assertIn("Wire.key", output)
        self.assertIn("`foo::read_key`", output)
        self.assertIn("no declaration this module reaches", output)

    def test_an_imported_reader_resolves_to_its_declaration(self) -> None:
        field = (
            "use super::read_key;\n"
            + self.OMITTED % ',\n        deserialize_with = "read_key"'
        )
        parent = 'cadmpeg_core::named_optional_field!(read_key, u32, "key");\n'
        status, output = self.run_absent_key_main(
            {"records/child.rs": field, "records.rs": parent}
        )
        self.assertEqual(status, 0, output)

    def test_a_local_macro_of_the_declaring_name_is_named(self) -> None:
        local = (
            "macro_rules! named_optional_field {\n"
            "    ($name:ident, $value:ty, $field:literal) => {};\n"
            "}\n"
            'named_optional_field!(read_key, u32, "key");\n'
            + self.OMITTED % ',\n        deserialize_with = "read_key"'
        )
        status, output = self.run_absent_key_main({"wire.rs": local})
        self.assertEqual(status, 1, output)
        self.assertIn("wire.rs", output)
        self.assertIn("this file defines a macro of that name", output)
        self.assertIn("Wire.key", output)

    def test_a_declaration_of_another_key_is_not_this_field_s(self) -> None:
        field = self.OMITTED % ',\n        deserialize_with = "read_key"'
        shim = 'cadmpeg_core::named_optional_field!(read_key, u32, "other");\n'
        status, output = self.run_absent_key_main({"wire.rs": field + shim})
        self.assertEqual(status, 1, output)
        self.assertIn("wire.rs", output)
        self.assertIn("Wire.key", output)
        self.assertIn("declares the key `other`", output)

    def test_a_renamed_key_is_the_key_the_declaration_states(self) -> None:
        field = self.OMITTED % (
            ',\n        rename = "other",\n        deserialize_with = "read_key"'
        )
        shim = 'cadmpeg_core::named_optional_field!(read_key, u32, "other");\n'
        status, output = self.run_absent_key_main({"wire.rs": field + shim})
        self.assertEqual(status, 0, output)

    FLATTENED_MODULE = (
        '#[derive(serde::Deserialize)]\n'
        'struct Outer {\n'
        '    #[serde(flatten, with = "inner_wire")]\n'
        '    value: Option<u32>,\n'
        '}\n'
        'mod inner_wire {\n'
        '    #[derive(serde::Deserialize)]\n'
        '    pub(super) struct Wire {\n'
        '        #[serde(default%s)]\n'
        '        inner_key: Option<u32>,\n'
        '    }\n'
        '}\n'
    )

    def test_a_flattened_module_with_an_undeclared_option_is_named(self) -> None:
        status, output = self.run_absent_key_main(
            {"wire.rs": self.FLATTENED_MODULE % ""}
        )
        self.assertEqual(status, 1, output)
        self.assertIn("Wire.inner_key", output)
        self.assertIn("inner_wire reads this key for Outer.value", output)
        self.assertIn("named_optional_field!", output)

    def test_a_flattened_module_that_declares_its_keys_passes(self) -> None:
        declared = self.FLATTENED_MODULE % (
            ', deserialize_with = "read_inner_key"'
        )
        declared = declared.replace(
            "    }\n}\n",
            "    }\n"
            '    cadmpeg_core::named_optional_field!(read_inner_key, u32, "inner_key");\n'
            "}\n",
        )
        status, output = self.run_absent_key_main({"wire.rs": declared})
        self.assertEqual(status, 0, output)

    def test_a_flattened_reader_key_may_state_the_nullable_spelling(self) -> None:
        nullable = (
            '#[derive(serde::Deserialize)]\n'
            'struct Outer {\n'
            '    #[serde(flatten, deserialize_with = "read_inner")]\n'
            '    value: Option<u32>,\n'
            '}\n'
            '#[derive(serde::Deserialize)]\n'
            'struct InnerWire {\n'
            '    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]\n'
            '    inner_key: Option<u32>,\n'
            '}\n'
            "fn read_inner<'de, D: serde::Deserializer<'de>>(d: D)"
            ' -> Result<Option<u32>, D::Error> {\n'
            '    let wire = InnerWire::deserialize(d)?;\n'
            '    Ok(wire.inner_key)\n'
            '}\n'
        )
        status, output = self.run_absent_key_main({"wire.rs": nullable})
        self.assertEqual(status, 0, output)
        stated = nullable.replace(
            '    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]\n',
            "",
        )
        status, output = self.run_absent_key_main({"wire.rs": stated})
        self.assertEqual(status, 1, output)
        self.assertIn("InnerWire.inner_key", output)
        self.assertIn("read_inner reads this key for Outer.value", output)

    def test_an_optional_key_under_default_is_read_without_a_writer_hint(
        self,
    ) -> None:
        findings = self.run_absent_key_census({
            "wire.rs": (
                '#[derive(serde::Deserialize)]\n'
                'struct Wire {\n'
                '    #[serde(default)]\n'
                '    key: Option<u32>,\n'
                '}\n'
            )
        })
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("Wire.key", findings[0])
        self.assertIn("named_optional_field!", findings[0])

    def test_a_projection_admission_is_refused_for_a_writing_item(self) -> None:
        source = (
            '#[derive(serde::Serialize, serde::Deserialize)]\n'
            'struct Probe {\n'
            '    #[serde(default)]\n'
            '    key: Option<u32>,\n'
            '}\n'
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "crates" / "fixture" / "src" / "probe.rs"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(source, encoding="utf-8")
            with patch.object(census, "absent_key_source_files",
                              return_value=[path]), \
                    patch.object(census, "ABSENT_KEY_EXCEPTIONS", {}), \
                    patch.object(
                        census, "ABSENT_KEY_PROJECTIONS",
                        {f"{path.as_posix()}:Probe": "a stated reason"}):
                findings = [
                    message for _, _, message in census.absent_key_failures()
                ]
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("derives Serialize", findings[0])

    def test_a_serialize_only_item_states_no_reader(self) -> None:
        writer = (
            '#[derive(serde::Serialize)]\n'
            'struct Wire {\n'
            '    #[serde(default, skip_serializing_if = "Option::is_none")]\n'
            '    key: Option<u32>,\n'
            '}\n'
        )
        self.assertEqual(self.run_absent_key_census({"wire.rs": writer}), [])

    FLATTENED = (
        '#[derive(serde::Deserialize)]\n'
        'struct Owner {\n'
        '    #[serde(flatten, deserialize_with = "%s")]\n'
        '    inner: Inner,\n'
        '}\n'
    )
    UNDECLARED = (
        'struct Wire {\n'
        '    #[serde(default, skip_serializing_if = "Option::is_none")]\n'
        '    key: Option<u32>,\n'
        '}\n'
    )

    def test_a_flattened_reader_that_resolves_to_no_struct_is_named(self) -> None:
        findings = self.run_absent_key_census({
            "owner.rs": self.FLATTENED % "missing_reader",
        })
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("Owner.inner", findings[0])
        self.assertIn("missing_reader", findings[0])
        self.assertIn("resolves to no struct", findings[0])

    def test_a_flattened_reader_that_resolves_to_two_modules_is_named(self) -> None:
        findings = self.run_absent_key_census({
            "owner.rs": self.FLATTENED % "inner",
            "alpha/inner.rs": "struct Wire {\n    value: u32,\n}\n",
            "beta/inner.rs": self.UNDECLARED,
        })
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("Owner.inner", findings[0])
        self.assertIn("resolves to 2 modules", findings[0])
        self.assertIn("alpha/inner.rs", findings[0])
        self.assertIn("beta/inner.rs", findings[0])

    def test_a_flattened_reader_path_names_the_module_it_reads(self) -> None:
        self.assertEqual(
            self.run_absent_key_census({
                "owner.rs": self.FLATTENED % "crate::alpha::inner",
                "alpha/inner.rs": "struct Wire {\n    value: u32,\n}\n",
                "beta/inner.rs": self.UNDECLARED,
            }),
            [],
        )

    def test_a_routed_reader_struct_states_no_key_of_its_own(self) -> None:
        self.assertEqual(
            self.run_absent_key_census({
                "owner.rs": self.FLATTENED % "crate::alpha::inner",
                "alpha/inner.rs": (
                    '#[serde(try_from = "WireRepr")]\n'
                    + self.UNDECLARED
                ),
            }),
            [],
        )



class GenericParameterProofTests(unittest.TestCase):
    """A generic payload parameter is proved over every type that reaches it."""

    run_census = DenyCensusTests.run_census

    CLOSED = '''
        #[derive(Deserialize)] #[serde(deny_unknown_fields)]
        struct Closed { value: u8 }
    '''
    OPEN = '''
        #[derive(Deserialize)]
        struct Open { value: u8 }
    '''

    def test_default_and_every_instantiation_are_proved(self) -> None:
        status, output = self.run_census({"lib.rs": self.CLOSED + '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct AlsoClosed { other: u8 }
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader<T = Closed> { A(T) }
            pub type Other = Reader<AlsoClosed>;
        '''})
        self.assertEqual(status, 0, output)

    def test_an_open_default_leaves_the_parameter_unproved(self) -> None:
        status, output = self.run_census({"lib.rs": self.OPEN + '''
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader<T = Open> { A(T) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("generic parameter T instantiated as Open", output)

    def test_an_open_instantiation_leaves_the_parameter_unproved(self) -> None:
        status, output = self.run_census({"lib.rs": self.CLOSED + self.OPEN + '''
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader<T = Closed> { A(T) }
            pub type Leak = Reader<Open>;
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("generic parameter T instantiated as Open", output)

    def test_a_parameter_with_no_default_and_no_use_is_unproved(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader<T> { A(T) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("has no default and no workspace instantiation", output)

    def test_an_instantiation_by_another_generic_parameter_is_unproved(self) -> None:
        status, output = self.run_census({"lib.rs": self.CLOSED + '''
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader<T = Closed> { A(T) }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Forwarder<U> { inner: Reader<U> }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn(
            "instantiated with the generic parameter U of Forwarder", output
        )

    def test_two_generic_owners_forward_to_checked_readers(self) -> None:
        status, output = self.run_census({"lib.rs": self.CLOSED + '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct AlsoClosed { other: u8 }
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader<T = Closed> { A(T) }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Middle<U = Closed> { inner: Reader<U> }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Outer<V = Closed> { inner: Middle<V> }
            type Concrete = Outer<AlsoClosed>;
        '''})
        self.assertEqual(status, 0, output)

    def test_two_generic_owners_forward_to_an_open_reader(self) -> None:
        status, output = self.run_census({"lib.rs": self.CLOSED + self.OPEN + '''
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader<T = Closed> { A(T) }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Middle<U = Closed> { inner: Reader<U> }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Outer<V = Closed> { inner: Middle<V> }
            type Concrete = Outer<Open>;
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A: payload T:", output)
        self.assertIn("generic parameter V instantiated as Open", output)
        self.assertIn("payload reader Open lacks checked unknown-key refusal", output)

    def test_generic_forwarding_cycle_without_a_concrete_reader_fails(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            enum Reader<T> { A(T), B(Forwarder<T>) }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Forwarder<U> { inner: Reader<U> }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A: payload T:", output)
        self.assertIn("no concrete workspace instantiation", output)

    def test_an_uninhabited_instantiation_refuses_every_input(self) -> None:
        status, output = self.run_census({"lib.rs": self.CLOSED + '''
            #[derive(Deserialize)]
            pub enum Never {}
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader<T = Closed> { A(T) }
            pub type Sheet = Reader<Never>;
        '''})
        self.assertEqual(status, 0, output)

    def test_generic_parameters_reads_names_bounds_and_defaults(self) -> None:
        item = census.Item(
            Path("crates/ir/src/lib.rs"), 1, "enum", "Reader",
            "", "pub enum Reader<'a, T: Clone = Closed, const N: usize> { A(T) }",
        )
        self.assertEqual(census.generic_parameters(item), [("T", "Closed")])


if __name__ == "__main__":
    unittest.main()
