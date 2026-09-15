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


if __name__ == "__main__":
    unittest.main()
