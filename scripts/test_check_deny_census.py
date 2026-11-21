#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Regression tests for the deny census's declaration resolution."""

import importlib.util
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "check_deny_census", Path(__file__).with_name("check-deny-census.py")
)
assert SPEC and SPEC.loader
census = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(census)


# The root of each wire package a reader path can name, as a fixture states it.
WIRE_PACKAGES = {
    "cadmpeg-core": "pub mod absent_key; pub mod distinct_keys; pub mod tag_only;",
    "cadmpeg-derive": "",
    "cadmpeg-ir": "pub mod bytes; pub mod features; pub mod ids; pub mod units;",
}
REGISTRY_PACKAGES = {"serde_json": "1", "serde_bytes": "0.11", "indexmap": "2"}


def write_workspace(directory: Path, crate: Path, package: str) -> None:
    """A workspace around one fixture crate: each wire package its reader
    paths name, and a manifest that depends on every one of them."""
    homes = {}
    for wire, root in WIRE_PACKAGES.items():
        homes[wire] = crate if package == wire else directory / "crates" / wire
        if package != wire:
            (homes[wire] / "src").mkdir(parents=True, exist_ok=True)
            (homes[wire] / "Cargo.toml").write_text(f'[package]\nname = "{wire}"\n')
            (homes[wire] / "src" / "lib.rs").write_text(root)
    workspace = "[workspace]\n[workspace.dependencies]\n" + "".join(
        f'{wire} = {{ path = "{home.relative_to(directory).as_posix()}" }}\n'
        for wire, home in homes.items()
    ) + "".join(f'{name} = "{version}"\n' for name, version in REGISTRY_PACKAGES.items())
    dependencies = "".join(
        f"{name}.workspace = true\n"
        for name in [*WIRE_PACKAGES, *REGISTRY_PACKAGES] if name != package
    )
    stated = f'[package]\nname = "{package}"\n[dependencies]\n{dependencies}'
    crate.mkdir(parents=True, exist_ok=True)
    if crate == directory:
        (crate / "Cargo.toml").write_text(f"{stated}{workspace}")
    else:
        (directory / "Cargo.toml").write_text(workspace)
        (crate / "Cargo.toml").write_text(stated)


def write_fixture(src: Path, files: dict[str, str]) -> list[Path]:
    """Write ``files`` under ``src``; a ``../`` name replaces a file of the
    crate itself, such as its manifest. Returns the Rust sources."""
    paths = []
    for name, source in files.items():
        path = src / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source, encoding="utf-8")
        if path.suffix == ".rs":
            paths.append(path)
    return paths


class DenyCensusTests(unittest.TestCase):
    def run_census(self, files: dict[str, str], crate: str = "") -> tuple[int, str]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_workspace(root, root / crate, crate or "fixture")
            paths = write_fixture(root / crate / "src", files)
            output = io.StringIO()
            # The absent-key rule has its own scope, the whole crates tree.
            # These fixtures state the unknown-key rule only, so that pass is
            # held out here and exercised by `run_absent_key_census` below,
            # and the set rule by `run_set_census`.
            with patch.object(census, "source_files", return_value=paths), \
                    patch.object(census, "EXCEPTIONS", {}), \
                    patch.object(census, "absent_key_failures", return_value=[]), \
                    patch.object(census, "set_spelling_failures", return_value=[]), \
                    patch.object(census, "trait_impl_source_files", return_value=[]), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                status = census.main()
            return status, output.getvalue()

    def run_set_census(self, files: dict[str, str]) -> list[str]:
        """The set rule's findings over one fixture crate."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_workspace(root, root, "fixture")
            paths = write_fixture(root / "src", files)
            with patch.object(census, "source_files", return_value=paths):
                return [message for _, _, message in census.set_spelling_failures()]

    def run_absent_key_census(self, files: dict[str, str]) -> list[str]:
        """The absent-key rule's findings over one fixture tree."""
        with tempfile.TemporaryDirectory() as directory:
            crate = Path(directory) / "crates" / "fixture"
            write_workspace(Path(directory), crate, "fixture")
            paths = write_fixture(crate / "src", files)
            with patch.object(census, "absent_key_source_files", return_value=paths), \
                    patch.object(census, "ABSENT_KEY_EXCEPTIONS", {}):
                return [message for _, _, message in census.absent_key_failures()]

    def run_absent_key_main(self, files: dict[str, str]) -> tuple[int, str]:
        """The whole census over one fixture tree, absent-key rule included."""
        with tempfile.TemporaryDirectory() as directory:
            crate = Path(directory) / "crates" / "fixture"
            write_workspace(Path(directory), crate, "fixture")
            paths = write_fixture(crate / "src", files)
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

    def test_invocations_reread_a_changed_file_at_the_same_path(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "src" / "wire.rs"
            path.parent.mkdir()
            output = io.StringIO()
            with patch.object(census, "source_files", return_value=[path]), \
                    patch.object(census, "EXCEPTIONS", {}), \
                    patch.object(census, "absent_key_failures", return_value=[]), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                path.write_text('#[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Wire { value: u32 }')
                self.assertEqual(census.main(), 0)
                path.write_text('#[derive(Deserialize)] struct Wire { value: u32 }')
                self.assertEqual(census.main(), 1)
            self.assertIn('Wire', output.getvalue())

    def test_function_index_retains_raw_identifiers_and_nested_structs(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'wire.rs'
            path.write_text('fn r#read<T>() { struct Inner { value: T } T::deserialize(d); }')
            entries = census.reader_function_index(path)['read']
            self.assertEqual(len(entries), 1)
            opening, end, read, parameters = entries[0]
            self.assertIn('struct Inner', census.read_source(path)[opening:end])
            self.assertEqual(read, {'T'})
            self.assertEqual(parameters, {'T'})

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

    def test_attribute_gap_accepts_whitespace_and_complete_line_comments(self) -> None:
        for gap in ("", " \t\r\n", "// plain\n", "/// docs\n//! inner\n",
                    "// comment without newline", " // repeated // markers\n \t"):
            with self.subTest(gap=gap):
                body = "prefix" + gap + "suffix"
                self.assertIsNotNone(census.ATTRIBUTE_GAP.match(body, 6, 6 + len(gap)))

    def test_attribute_gap_refuses_field_text_and_other_tokens(self) -> None:
        for gap in ("pub first: u32,", "/// docs\npub first: u32,\n",
                    "/", "/* block comment */", " \nX", "// line\n/ tail"):
            with self.subTest(gap=gap):
                body = "prefix" + gap + "suffix"
                self.assertIsNone(census.ATTRIBUTE_GAP.match(body, 6, 6 + len(gap)))

    def test_field_attribute_runs_bounds_repeated_comment_marker_work(self) -> None:
        body = ("#[serde(default)]\n" + "///" * 1000
                + "\nfirst: u32,\n#[serde(default)]\nsecond: u32,")
        # Isolate the match so a backtracking regression cannot hang the suite.
        result = subprocess.run(
            [sys.executable, "-c", """
import json, runpy, sys
census = runpy.run_path(sys.argv[1])
runs = census['field_attribute_runs'](sys.stdin.read())
print(json.dumps([(name, optional) for _, _, name, optional in runs]))
""", str(Path(census.__file__).resolve())],
            input=body, capture_output=True, text=True, timeout=5, check=True,
        )
        self.assertEqual(json.loads(result.stdout), [["second", False]])

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

    def test_a_tagged_reader_derive_is_checked_as_a_deserialize_derive(self) -> None:
        open_reader = '''
            #[derive(cadmpeg_derive::DeserializeTagged)]
            #[serde(tag = "kind")]
            enum Reader { Value { value: u8 } }
        '''
        status, output = self.run_census({"lib.rs": open_reader})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader", output)
        status, output = self.run_census({"lib.rs": open_reader.replace(
            '#[serde(tag = "kind")]', '#[serde(tag = "kind", deny_unknown_fields)]'
        )})
        self.assertEqual(status, 0, output)

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

    NAME_READ = '''
        IMPORT
        #[derive(Serialize, DERIVE)]
        #[serde(rename_all = "snake_case", deny_unknown_fields)]
        enum Reader { A, B = 2 }
        #[derive(Deserialize)] #[serde(deny_unknown_fields)]
        struct Holder { base: Reader }
    '''

    def name_read(self, derive="DeserializeName",
                  imported="use cadmpeg_derive::DeserializeName;") -> str:
        return self.NAME_READ.replace("IMPORT", imported).replace("DERIVE", derive)

    SET_PRELUDE = '''
        use std::collections::{BTreeMap, BTreeSet};
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Class { Model, Process }
    '''

    def test_a_set_or_map_read_by_serde_states_a_second_spelling(self) -> None:
        unsorted = "in any order and merges a repeated member"
        last = "keeping the last value of a repeated key"
        cases = {
            "a transparent newtype over a set": ('''
                #[derive(Serialize, Deserialize)] #[serde(transparent)]
                pub struct ArenaClasses(std::collections::BTreeSet<Class>);
            ''', f"ArenaClasses.0: serde reads the BTreeSet in std::collections::BTreeSet<Class> {unsorted}"),
            "an optional set field": ('''
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Machine { #[serde(default)] axes: Option<BTreeSet<Class>> }
            ''', f"Machine.axes: serde reads the BTreeSet in Option<BTreeSet<Class>> {unsorted}"),
            "a map field": ('''
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder { names: BTreeMap<String, u8> }
            ''', f"Holder.names: serde reads the BTreeMap in BTreeMap<String, u8> {last}"),
            "a JSON object field": ('''
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder { context: serde_json::Map<String, serde_json::Value> }
            ''', "Holder.context: serde reads the serde_json::Map"),
            "a newtype variant": ('''
                #[derive(Deserialize)] #[serde(tag = "kind", content = "value")]
                enum Holder { Classes(BTreeSet<Class>) }
            ''', f"Holder::Classes.0: serde reads the BTreeSet in BTreeSet<Class> {unsorted}"),
            "a struct variant": ('''
                #[derive(Deserialize)] #[serde(tag = "kind")]
                enum Holder { Classes { members: Vec<BTreeSet<Class>> } }
            ''', f"Holder::Classes.members: serde reads the BTreeSet in Vec<BTreeSet<Class>> {unsorted}"),
            "an alias": ('''
                type Classes = BTreeSet<Class>;
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder { classes: Classes }
            ''', f"Holder.classes: serde reads the BTreeSet in Classes {unsorted}"),
            "a set inside a distinct map": ('''
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder {
                    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
                    by_name: BTreeMap<String, BTreeSet<Class>>,
                }
            ''', f"Holder.by_name: serde reads the BTreeSet in BTreeSet<Class> {unsorted}"),
            "an integer key in a JSON object": ('''
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder {
                    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
                    by_index: BTreeMap<u32, u8>,
                }
            ''', "Holder.by_index: a JSON object key u32 is not a JSON string"),
            "a reader that reads serde's set": ('''
                fn read_classes<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeSet<Class>, D::Error> {
                    BTreeSet::deserialize(d)
                }
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder { #[serde(deserialize_with = "read_classes")] classes: BTreeSet<Class> }
            ''', "Holder.classes: the reader read_classes calls no cadmpeg_core::distinct_keys reader"),
            "a shim over a hand-written set reader": ('''
                struct Distinct<T>(BTreeSet<T>);
                impl<'de, T: Deserialize<'de> + Ord> Deserialize<'de> for Distinct<T> {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        Ok(Distinct(BTreeSet::from_iter(Vec::<T>::deserialize(d)?)))
                    }
                }
                impl<T> From<Distinct<T>> for BTreeSet<T> {
                    fn from(set: Distinct<T>) -> Self { set.0 }
                }
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Machine {
                    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "read_axes")]
                    axes: Option<BTreeSet<Class>>,
                }
                cadmpeg_core::named_optional_field!(read_axes, Distinct<Class> as BTreeSet<Class>, "axes");
            ''', "Distinct: a hand-written reader of a type holding a BTreeSet calls no "
                 "cadmpeg_core::distinct_keys reader"),
            "a set reader whose root binds another item": ('''
                mod cadmpeg_core { pub mod distinct_keys { pub mod set {} } }
                #[derive(Deserialize)] #[serde(transparent)]
                struct Classes(#[serde(with = "cadmpeg_core::distinct_keys::set")] BTreeSet<Class>);
            ''', "spells cadmpeg_core where it can bind another item"),
        }
        for case, (source, expected) in cases.items():
            with self.subTest(case=case):
                findings = self.run_set_census({"lib.rs": self.SET_PRELUDE + source})
                self.assertEqual(len(findings), 1, findings)
                self.assertIn(expected, findings[0])

    def test_a_set_or_map_read_by_a_distinct_keys_reader_has_one_spelling(self) -> None:
        cases = {
            "a transparent newtype over a set": '''
                #[derive(Serialize, Deserialize)] #[serde(transparent)]
                pub struct ArenaClasses(
                    #[serde(with = "cadmpeg_core::distinct_keys::set")] std::collections::BTreeSet<Class>,
                );
            ''',
            "maps by their key": '''
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder {
                    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
                    names: BTreeMap<String, u8>,
                    #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
                    by_index: BTreeMap<u32, u8>,
                    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::json_object")]
                    context: serde_json::Map<String, serde_json::Value>,
                    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::set::deserialize")]
                    classes: BTreeSet<Class>,
                }
            ''',
            "a shim over a set newtype": '''
                #[derive(Deserialize)] #[serde(transparent)]
                struct AxisSet(#[serde(with = "cadmpeg_core::distinct_keys::set")] BTreeSet<Class>);
                impl From<AxisSet> for BTreeSet<Class> {
                    fn from(AxisSet(axes): AxisSet) -> Self { axes }
                }
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Machine {
                    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "read_axes")]
                    axes: Option<BTreeSet<Class>>,
                }
                cadmpeg_core::named_optional_field!(read_axes, AxisSet as BTreeSet<Class>, "axes");
            ''',
            "a reader that forwards to a shim": '''
                #[derive(Deserialize)] #[serde(transparent)]
                struct Centers(#[serde(with = "cadmpeg_core::distinct_keys::set")] BTreeSet<Class>);
                fn read_centers<'de, D: Deserializer<'de>>(d: D) -> Result<Option<BTreeSet<Class>>, D::Error> {
                    read_center_set(d).map(|centers| centers.map(|Centers(set)| set))
                }
                cadmpeg_core::named_optional_field!(read_center_set, Centers, "centers");
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder {
                    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "read_centers")]
                    centers: Option<BTreeSet<Class>>,
                }
            ''',
            "a hand-written reader that reads a checked wire type": '''
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct StatsWire {
                    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
                    kinds: BTreeMap<String, usize>,
                }
                struct Stats { kinds: BTreeMap<String, usize> }
                impl<'de> Deserialize<'de> for Stats {
                    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                        let wire = StatsWire::deserialize(d)?;
                        Ok(Stats { kinds: wire.kinds })
                    }
                }
            ''',
            "an associated type and serde_json's value": '''
                trait Kind { type Value; }
                struct Classes;
                impl Kind for Classes { type Value = BTreeSet<Class>; }
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder<K: Kind> { value: K::Value, raw: serde_json::Value }
            ''',
            "a skipped field": '''
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder { #[serde(skip)] cache: BTreeSet<Class> }
            ''',
        }
        for case, source in cases.items():
            with self.subTest(case=case):
                self.assertEqual(self.run_set_census({"lib.rs": self.SET_PRELUDE + source}), [])

    def test_the_whole_census_fails_on_a_set_read_by_serde(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_workspace(root, root, "fixture")
            paths = write_fixture(root / "src", {"lib.rs": self.SET_PRELUDE + '''
                #[derive(Serialize, Deserialize)] #[serde(transparent)]
                pub struct ArenaClasses(BTreeSet<Class>);
            '''})
            output = io.StringIO()
            with patch.object(census, "source_files", return_value=paths), \
                    patch.object(census, "absent_key_failures", return_value=[]), \
                    patch.object(census, "trait_impl_source_files", return_value=[]), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                status = census.main()
        self.assertEqual(status, 1, output.getvalue())
        self.assertIn("ArenaClasses.0: serde reads the BTreeSet", output.getvalue())

    def test_a_data_free_enum_read_by_serde_states_a_second_spelling(self) -> None:
        for case, (source, name) in {
            "denying": (self.name_read("Deserialize", ""), "Reader"),
            "not denying": ('''
                #[derive(Deserialize)] #[serde(rename_all = "snake_case")]
                enum Reader { A, B }
            ''', "Reader"),
            "a macro template": ('''
                macro_rules! labels {
                    ($name:ident { $($variant:ident,)* }) => {
                        #[derive(Deserialize)]
                        enum $name { $($variant,)* }
                    };
                }
                labels! { Reader { A, B, } }
            ''', "$name"),
        }.items():
            with self.subTest(case=case):
                status, output = self.run_census({"lib.rs": source})
                self.assertEqual(status, 1, output)
                self.assertIn(
                    f"{name}: serde's derived Deserialize reads a data-free enum from "
                    '{"<variant>": null} beside its name; derive cadmpeg_derive::DeserializeName',
                    output,
                )

    def test_a_data_free_enum_read_by_its_name_alone(self) -> None:
        for case, source in {
            "imported": self.name_read(),
            "by its path": self.name_read("cadmpeg_derive::DeserializeName", ""),
            "by its absolute path": self.name_read("::cadmpeg_derive::DeserializeName", ""),
            "a macro template": '''
                use cadmpeg_derive::DeserializeName;
                macro_rules! labels {
                    ($name:ident { $($variant:ident,)* }) => {
                        #[derive(DeserializeName)]
                        enum $name { $($variant,)* }
                    };
                }
                labels! { Reader { A, B, } }
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                struct Holder { base: Reader }
            ''',
        }.items():
            with self.subTest(case=case):
                status, output = self.run_census({"lib.rs": source})
                self.assertEqual(status, 0, output)

    def test_a_name_reader_that_is_not_the_workspace_derive_proves_nothing(self) -> None:
        local = "mod cadmpeg_derive { pub use serde::Deserialize as DeserializeName; }"
        cases = {
            "no import": (self.name_read(imported=""), "imports no one"),
            "an import from another crate": (
                self.name_read(imported="use serde::Deserialize as DeserializeName;"),
                "serde::Deserialize is not cadmpeg_derive::DeserializeName",
            ),
            "a local module of the root's name": (
                local + self.name_read(),
                r"spells cadmpeg_derive where it can bind another item",
            ),
            "a conditional derive": (
                self.name_read().replace(
                    "#[derive(Serialize, DeserializeName)]",
                    '#[derive(Serialize)] #[cfg_attr(feature = "x", derive(DeserializeName))]',
                ),
                "the derive is conditional",
            ),
            "a variant that holds data": (
                self.name_read().replace("B = 2", "B(u8)"),
                "it reads an enum whose variants hold no data",
            ),
            "a tag": (
                self.name_read().replace('rename_all = "snake_case"', 'tag = "kind"'),
                "serde tag gives the enum another wire form",
            ),
        }
        for case, (source, reason) in cases.items():
            with self.subTest(case=case):
                status, output = self.run_census({"lib.rs": source})
                self.assertEqual(status, 1, output)
                self.assertIn("Reader: derives DeserializeName, but", output)
                self.assertRegex(output, reason)
        undeclared = '[package]\nname = "fixture"\n[dependencies]\nserde_json.workspace = true\n'
        status, output = self.run_census(
            {"lib.rs": self.name_read(), "../Cargo.toml": undeclared}, crate="fixture")
        self.assertEqual(status, 1, output)
        self.assertIn("names no dependency cadmpeg_derive", output)

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

    def test_internal_tagged_unit_arm_read_by_its_tag_alone(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader {
                #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
                A,
                B { value: u8 },
            }
        '''})
        self.assertEqual(status, 0, output)

    def test_internal_tagged_unit_arm_read_by_the_tag_only_reader_spellings(self) -> None:
        cases = [
            ("", "::cadmpeg_core::tag_only::read"),
            ("", "cadmpeg_core :: tag_only :: read"),
            ("cadmpeg-core", "crate::tag_only::read"),
        ]
        for crate, reader in cases:
            with self.subTest(crate=crate, reader=reader):
                status, output = self.run_census({"lib.rs": f'''
                    pub mod tag_only;
                    #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                    enum Reader {{
                        #[serde(deserialize_with = "{reader}")]
                        A,
                        B {{ value: u8 }},
                    }}
                '''}, crate=crate)
                self.assertEqual(status, 0, output)

    def test_internal_tagged_unit_arm_read_any_other_way_ignores_local_deny(self) -> None:
        arms = {
            "crate path outside the declaring crate":
                '#[serde(deserialize_with = "crate::tag_only::read")]',
            "suffix of another module":
                '#[serde(deserialize_with = "other::tag_only::read")]',
            "bare module path":
                '#[serde(deserialize_with = "tag_only::read")]',
            "conditional reader":
                '#[cfg_attr(feature = "x", serde(deserialize_with = '
                '"cadmpeg_core::tag_only::read"))]',
            "reader named in documentation":
                '/// #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]',
            "reader named in a doc attribute":
                '#[doc = "deserialize_with = \\"cadmpeg_core::tag_only::read\\""]',
            "two readers":
                '#[serde(deserialize_with = "cadmpeg_core::tag_only::read")] '
                '#[cfg_attr(feature = "x", serde(deserialize_with = "other::read"))]',
            "another reader":
                '#[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]',
        }
        for case, attributes in arms.items():
            with self.subTest(case=case):
                status, output = self.run_census({"lib.rs": f'''
                    #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
                    enum Reader {{
                        {attributes}
                        A,
                        B {{ value: u8 }},
                    }}
                '''}, crate="cadmpeg-ir")
                self.assertEqual(status, 1, output)
                self.assertIn("Reader::A: an internally tagged unit arm", output)

    TAG_ONLY_ARM = '''
        #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
        enum Reader {
            #[serde(deserialize_with = "READER")]
            A,
            B { value: u8 },
        }
    '''
    LOCAL_TAG_ONLY = '''pub mod tag_only {
            pub fn read<'de, D: serde::Deserializer<'de>>(d: D) -> Result<(), D::Error> {
                serde::Deserialize::deserialize(d).map(|_: ::serde_json::Value| ())
            }
        }'''

    def test_a_tag_only_path_whose_root_binds_another_item_is_no_reader(self) -> None:
        """The root names the workspace's cadmpeg-core only where nothing in
        the crate binds it otherwise."""
        arm = self.TAG_ONLY_ARM.replace("READER", "cadmpeg_core::tag_only::read")
        absolute = self.TAG_ONLY_ARM.replace("READER", "::cadmpeg_core::tag_only::read")
        local = self.LOCAL_TAG_ONLY
        renamed = (
            '[package]\nname = "cadmpeg-ir"\n[dependencies]\n'
            'cadmpeg_core = { package = "evil", path = "../evil" }\n'
            'serde_json.workspace = true\n'
        )
        moved = (
            '[package]\nname = "cadmpeg-ir"\n[dependencies]\n'
            'cadmpeg-core = { path = "../evil" }\nserde_json.workspace = true\n'
        )
        spelled = r"lib\.rs:\d+ spells cadmpeg_core where it can bind another item"
        cases = {
            "a local module": (
                {"lib.rs": f"mod cadmpeg_core {{ {local} }}" + arm}, spelled,
            ),
            "a module in another file": (
                {"lib.rs": "mod cadmpeg_core;" + arm, "cadmpeg_core.rs": local}, spelled,
            ),
            "a use rename": (
                {"lib.rs": f"mod other {{ {local} }} use other as cadmpeg_core;" + arm}, spelled,
            ),
            "a re-export another module's glob brings in": (
                {"lib.rs": "mod api; use api::*;" + arm,
                 "api.rs": f"pub mod other {{ {local} }} pub use other as cadmpeg_core;"},
                spelled.replace("lib", "api"),
            ),
            "a glob from outside the crate": (
                {"lib.rs": "use serde_json::*;" + arm}, "imports every name of serde_json",
            ),
            "an extern crate rename": (
                {"lib.rs": "extern crate serde_json as cadmpeg_core;" + arm}, spelled,
            ),
            "an extern crate rename of an absolute path": (
                {"lib.rs": "extern crate serde_json as cadmpeg_core;" + absolute}, spelled,
            ),
            "a macro that declares the module it is passed": (
                {"lib.rs": f"macro_rules! module {{ ($name:ident) => {{ mod $name {{ {local} }} }}; }}"
                           " module!(cadmpeg_core);" + arm},
                spelled,
            ),
            "a dependency renamed to the root": (
                {"lib.rs": arm, "../Cargo.toml": renamed},
                "is not the workspace's cadmpeg-core",
            ),
            "a dependency on another directory": (
                {"lib.rs": arm, "../Cargo.toml": moved},
                "is not the workspace's cadmpeg-core",
            ),
        }
        for case, (files, reason) in cases.items():
            with self.subTest(case=case):
                status, output = self.run_census(files, crate="cadmpeg-ir")
                self.assertEqual(status, 1, output)
                self.assertIn(
                    "Reader::A: an internally tagged unit arm names "
                    "cadmpeg_core::tag_only::read, but", output,
                )
                self.assertRegex(output, reason)
        for files in ({"lib.rs": arm}, {"lib.rs": "mod std {}" + absolute}):
            with self.subTest(control=files["lib.rs"][:12]):
                status, output = self.run_census(files, crate="cadmpeg-ir")
                self.assertEqual(status, 0, output)

    def test_a_crate_path_to_the_tag_only_reader_needs_the_real_module(self) -> None:
        for crate, files, reason in (
            ("cadmpeg-core", {"lib.rs": self.TAG_ONLY_ARM.replace(
                "READER", "$crate::tag_only::read")}, "names $crate"),
            ("cadmpeg-core", {"lib.rs": f"mod other {{ {self.LOCAL_TAG_ONLY} }}"
                              + self.TAG_ONLY_ARM.replace("READER", "crate::tag_only::read")},
             "declares no module tag_only"),
            ("cadmpeg-ir", {"lib.rs": "pub mod tag_only;" + self.TAG_ONLY_ARM.replace(
                "READER", "crate::tag_only::read")}, "is written outside cadmpeg-core"),
        ):
            with self.subTest(crate=crate, reason=reason):
                status, output = self.run_census(files, crate=crate)
                self.assertEqual(status, 1, output)
                self.assertIn("names cadmpeg_core::tag_only::read, but", output)
                self.assertIn(reason, output)

    def test_every_reader_accepted_by_path_needs_its_root_to_bind_the_real_crate(self) -> None:
        """The distinct-key, absent-key and free-form readers are each read
        by their path, so a local module of the root's name is not one."""
        payloads = {
            "a distinct-key map": (
                "cadmpeg_core",
                '#[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")] '
                'BTreeMap<String, u8>',
            ),
            "a nullable optional": (
                "cadmpeg_core",
                '#[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")] Option<u8>',
            ),
            "a free-form value": ("serde_json", "serde_json::Value"),
        }
        for case, (root, payload) in payloads.items():
            reader = f'''
                #[derive(Deserialize)]
                #[serde(tag = "kind", content = "value", deny_unknown_fields)]
                enum Reader {{ A({payload}) }}
            '''
            with self.subTest(case=case):
                status, output = self.run_census({"lib.rs": reader}, crate="cadmpeg-ir")
                self.assertEqual(status, 0, output)
                status, output = self.run_census(
                    {"lib.rs": f"mod {root} {{}}" + reader}, crate="cadmpeg-ir"
                )
                self.assertEqual(status, 1, output)
                self.assertIn("Reader::A: payload", output)

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

    NAMED_OPTIONAL_FIELD = '''
        macro_rules! named_optional_field {
            ($name:ident, $wire:ty as $value:ty, $field:literal) => {
                fn $name<'de, D: serde::Deserializer<'de>>(d: D)
                    -> Result<Option<$value>, D::Error> {
                    $crate::absent_key::named_present::<D, $wire>(d, $field)
                        .map(|value| value.map(Into::into))
                }
            };
        }
    '''

    def test_a_converting_shim_is_proved_through_the_wire_it_reads(self) -> None:
        status, output = self.run_census({"lib.rs": self.NAMED_OPTIONAL_FIELD + '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Closed { value: u8 }
            struct Value;
            impl From<Closed> for Value { fn from(_: Closed) -> Self { Self } }
            named_optional_field!(read_closed, Closed as Value, "closed");
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader {
                A(#[serde(deserialize_with = "read_closed")] Option<Value>),
            }
        '''}, crate="cadmpeg-core")
        self.assertEqual(status, 0, output)

    def test_a_converting_shim_over_an_open_wire_is_unproved(self) -> None:
        status, output = self.run_census({"lib.rs": self.NAMED_OPTIONAL_FIELD + '''
            struct Open;
            impl<'de> serde::Deserialize<'de> for Open {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    let _ = serde_json::Value::deserialize(d)?;
                    Ok(Self)
                }
            }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Closed { value: u8 }
            impl From<Open> for Closed { fn from(_: Open) -> Self { Self { value: 0 } } }
            named_optional_field!(read_open, Open as Closed, "open");
            #[derive(Deserialize)] #[serde(tag = "kind", deny_unknown_fields)]
            enum Reader {
                A(#[serde(deserialize_with = "read_open")] Option<Closed>),
            }
        '''}, crate="cadmpeg-core")
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)
        self.assertIn("payload reader Open lacks checked unknown-key refusal", output)

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

    CHECKED_QUANTITY = '''
        macro_rules! checked_quantity {
            ($(#[$attribute:meta])* $name:ident, $value:ident, $condition:expr, $error:literal) => {
                $(#[$attribute])*
                #[derive(Clone, Copy, Serialize, Deserialize)]
                #[serde(into = "f64", try_from = "{target}")]
                pub struct $name(f64);
            };
        }
        checked_quantity!(
            /// A positive finite area.
            PositiveArea, value, value > 0.0, "a PositiveArea is finite and positive"
        );
        #[derive(Deserialize)] #[serde(tag = "kind", content = "value", deny_unknown_fields)]
        enum Reader { A(PositiveArea) }
    '''

    def test_checked_quantity_macro_expands_its_conversion_reader(self) -> None:
        status, output = self.run_census(
            {"lib.rs": self.CHECKED_QUANTITY.replace("{target}", "f64")}
        )
        self.assertEqual(status, 0, output)

    def test_checked_quantity_macro_with_an_open_conversion_fails_closed(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)]
            struct Open { value: f64 }
        ''' + self.CHECKED_QUANTITY.replace("{target}", "Open")})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_checked_quantity_macro_without_a_conversion_route_fails_closed(self) -> None:
        source = self.CHECKED_QUANTITY.replace(
            '#[serde(into = "f64", try_from = "{target}")]', ""
        ).replace("pub struct $name(f64);", "pub struct $name { value: f64 }")
        status, output = self.run_census({"lib.rs": source})
        self.assertEqual(status, 1, output)
        self.assertIn("Reader::A", output)

    def test_an_imported_nonzero_integer_is_a_scalar_reader(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            use std::num::NonZeroU32;
            #[derive(Deserialize)] #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            enum Reader { A(NonZeroU32), B(core::num::NonZeroI8) }
        '''})
        self.assertEqual(status, 0, output)

    def test_a_local_nonzero_declaration_is_not_the_std_scalar(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)]
            struct NonZeroU32 { value: u32 }
            #[derive(Deserialize)] #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            enum Reader { A(NonZeroU32) }
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
        import_old = '''imported = import_missing if path in names else imported_path(path, owner)
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
        self, files: dict[str, str], old: str, new: str, crate: str = ""
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
            root = Path(directory)
            write_workspace(root, root / crate, crate or "fixture")
            paths = write_fixture(root / crate / "src", files)
            namespace["source_files"] = lambda: paths
            namespace["EXCEPTIONS"] = {}
            # The absent-key rule has its own scope, the whole crates tree,
            # and these fixtures state the unknown-key rule only. Without this
            # the mutated census walks the real repository once per mutation;
            # the walk for impls outside the wire crates is held out likewise.
            namespace["absent_key_failures"] = lambda: []
            namespace["trait_impl_source_files"] = lambda: []
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
            crate="cadmpeg-ir",
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

    def test_a_converting_shim_states_the_spelling(self) -> None:
        declared = self.OMITTED % ',\n        deserialize_with = "read_key"'
        shim = (
            'cadmpeg_core::named_optional_field!(\n'
            '    read_key,\n'
            '    wire::Distinct<u32> as BTreeSet<u32>,\n'
            '    "key"\n'
            ');\n'
        )
        self.assertEqual(
            self.run_absent_key_census({"wire.rs": declared + shim}), []
        )

    def test_a_declaration_macro_whose_root_binds_another_item_declares_nothing(self) -> None:
        declared = self.OMITTED % ',\n        deserialize_with = "read_key"'
        local = (
            "mod cadmpeg_core { macro_rules! named_optional_field "
            "{ ($($t:tt)*) => {}; } }\n"
        )
        for shim, reason in (
            ('cadmpeg_core::named_optional_field!(read_key, u32, "key");\n',
             "spells cadmpeg_core where it can bind another item"),
            ('other::cadmpeg_core::named_optional_field!(read_key, u32, "key");\n',
             "names no root of cadmpeg-core"),
            ('crate::named_optional_field!(read_key, u32, "key");\n',
             "is written outside cadmpeg-core"),
        ):
            with self.subTest(shim=shim):
                findings = self.run_absent_key_census({"wire.rs": local + declared + shim})
                self.assertTrue(
                    any("named_optional_field!: " in finding and reason in finding
                        for finding in findings),
                    findings,
                )
                self.assertTrue(any("Wire.key" in finding for finding in findings), findings)
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

    def test_generic_flattened_reader_follows_the_attributed_field_type(self) -> None:
        source = '''
            #[derive(serde::Deserialize)]
            struct Owner {
                #[serde(flatten)]
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[serde(deserialize_with = "read_inner")]
                inner: Option<Inner>,
            }
            fn read_inner<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
            where D: serde::Deserializer<'de>, T: serde::Deserialize<'de> {
                T::deserialize(d).map(Some)
            }
        '''
        for value in ("u32", "Option<u32>"):
            with self.subTest(value=value):
                inner = f"struct Inner {{ value: {value} }}"
                findings = self.run_absent_key_census({"wire.rs": source + inner})
                if value == "u32":
                    self.assertEqual(findings, [])
                else:
                    self.assertEqual(len(findings), 1, findings)
                    self.assertIn("Inner.value", findings[0])
                    self.assertIn("read_inner reads this key for Owner.inner", findings[0])

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

    FORWARDED_DIMENSION = CLOSED + '''
        #[derive(Deserialize)] #[serde(deny_unknown_fields)]
        struct AlsoClosed { other: u8 }
        #[derive(Deserialize)]
        #[serde(tag = "kind", content = "value", deny_unknown_fields,
                bound(deserialize = "T: Deserialize<'de>, P: Deserialize<'de>"))]
        enum Dimension<T, P = Closed> { Literal(T), Parameter(P) }
        #[derive(Deserialize)] #[serde(tag = "kind", content = "value", deny_unknown_fields)]
        enum Offset { Distance(Dimension<AlsoClosed>) }
        #[derive(Deserialize)] #[serde(tag = "kind", content = "value", deny_unknown_fields)]
        enum Law<Q> { Constant(Dimension<Q>) }
    '''

    def test_a_concrete_use_is_unproved_while_a_forwarder_has_no_instantiation(self) -> None:
        status, output = self.run_census({"lib.rs": self.FORWARDED_DIMENSION})
        self.assertEqual(status, 1, output)
        self.assertIn("Offset::Distance: payload Dimension<AlsoClosed>", output)
        self.assertIn("generic parameter Q of Law", output)

    def test_instantiating_the_forwarder_proves_every_use(self) -> None:
        status, output = self.run_census({"lib.rs": self.FORWARDED_DIMENSION + '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Twist { law: Law<Closed> }
        '''})
        self.assertEqual(status, 0, output)

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

    GENERIC_ALIAS = CLOSED + '''
        #[derive(Deserialize)]
        #[serde(tag = "kind", content = "value", deny_unknown_fields,
                bound(deserialize = "T: Deserialize<'de>, P: Deserialize<'de>"))]
        enum Dimension<T, P = Closed> { Literal(T), Parameter(P) }
        #[derive(Deserialize)] #[serde(deny_unknown_fields)]
        struct Ref { id: u8 }
        pub type RefDimension<T> = Dimension<T, Ref>;
    '''

    def test_a_generic_alias_is_proved_through_its_instantiations(self) -> None:
        status, output = self.run_census({"lib.rs": self.GENERIC_ALIAS + '''
            #[derive(Deserialize)] #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            enum Feed { PerTooth(RefDimension<Closed>) }
        '''})
        self.assertEqual(status, 0, output)

    def test_an_open_instantiation_of_a_generic_alias_is_unproved(self) -> None:
        status, output = self.run_census({"lib.rs": self.GENERIC_ALIAS + self.OPEN + '''
            #[derive(Deserialize)] #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            enum Feed { PerTooth(RefDimension<Open>) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Feed::PerTooth: payload RefDimension<Open>", output)
        self.assertIn("generic parameter T of RefDimension", output)
        self.assertIn("instantiated as Open", output)

    def test_a_generic_alias_with_a_where_clause_is_unproved(self) -> None:
        status, output = self.run_census({"lib.rs": self.GENERIC_ALIAS + '''
            pub type Bounded<T> where T: Copy = Dimension<T, Ref>;
            #[derive(Deserialize)] #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            enum Feed { PerTooth(Bounded<Closed>) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("payload reader Bounded lacks checked unknown-key refusal", output)

    SAME_NAMED_GENERICS = {
        "lib.rs": "mod first; mod second;",
        "first.rs": CLOSED + '''
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            pub enum Wrapper<T> { A(T) }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Use { inner: Wrapper<Closed> }
        ''',
        "second.rs": OPEN + '''
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            pub enum Wrapper<T> { A(T) }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            struct Use { inner: Wrapper<Open> }
        ''',
    }

    def test_each_same_named_generic_is_proved_by_its_own_instantiations(self) -> None:
        status, output = self.run_census(self.SAME_NAMED_GENERICS)
        self.assertEqual(status, 1, output)
        self.assertIn("second.rs:5 Wrapper::A: payload T", output)
        self.assertIn("instantiated as Open", output)
        self.assertNotIn("first.rs:5 Wrapper::A", output)

    def test_a_same_named_instantiation_does_not_prove_another_module(self) -> None:
        files = dict(self.SAME_NAMED_GENERICS)
        files["second.rs"] = files["second.rs"].replace(
            "struct Use { inner: Wrapper<Open> }", "struct Use { inner: u8 }"
        )
        status, output = self.run_census(files)
        self.assertEqual(status, 1, output)
        self.assertIn("second.rs:5 Wrapper::A: payload T", output)
        self.assertIn("no default and no workspace instantiation", output)
        self.assertNotIn("first.rs:5 Wrapper::A", output)

    def test_a_qualified_instantiation_reaches_its_module(self) -> None:
        files = dict(self.SAME_NAMED_GENERICS)
        files["second.rs"] = files["second.rs"].replace(
            "struct Use { inner: Wrapper<Open> }",
            "struct Use { inner: crate::first::Wrapper<Open> }",
        )
        status, output = self.run_census(files)
        self.assertEqual(status, 1, output)
        self.assertIn("first.rs:5 Wrapper::A: payload T", output)
        self.assertIn("instantiated as Open", output)

    def test_an_instantiation_that_resolves_nowhere_is_unread(self) -> None:
        files = dict(self.SAME_NAMED_GENERICS)
        files["second.rs"] = files["second.rs"].replace(
            "struct Use { inner: Wrapper<Open> }",
            "struct Use { inner: elsewhere::Wrapper<Open> }",
        )
        status, output = self.run_census(files)
        self.assertEqual(status, 1, output)
        self.assertIn("the use of elsewhere::Wrapper in Use resolves to no one declaration", output)

    WRAPPER = '''
        #[derive(Deserialize)]
        #[serde(tag = "kind", content = "value", deny_unknown_fields)]
        pub enum Wrapper<T = u8> { A(T) }
    '''

    def test_a_use_under_a_renamed_import_instantiates_the_parameter(self) -> None:
        for rename, payload in (
            ("pub use Wrapper as Closed;", "Closed<Open>"),
            ("mod api { pub use crate::Wrapper as Closed; }", "api::Closed<Open>"),
            ("pub use Wrapper as Middle; pub use Middle as Closed;", "Closed<Open>"),
        ):
            status, output = self.run_census({"lib.rs": self.WRAPPER + self.OPEN + rename + f'''
                #[derive(Deserialize)]
                #[serde(tag = "kind", content = "value", deny_unknown_fields)]
                pub enum Doc {{ Face({payload}) }}
            '''})
            self.assertEqual(status, 1, f"{rename}: {output}")
            self.assertIn(
                "Wrapper::A: payload T: generic parameter T instantiated as Open", output
            )

    def test_a_macro_that_can_declare_its_input_leaves_the_parameter_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.WRAPPER + self.OPEN + '''
            macro_rules! host {
                ($t:ty) => {
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    pub struct Host { pub value: $t }
                };
            }
            host!(Wrapper<Open>);
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn(
            "Wrapper::A: payload T: the macro call at", output
        )
        self.assertIn("names Wrapper, and the census does not expand it", output)

    def test_a_macro_writing_only_functions_states_no_instantiation(self) -> None:
        status, output = self.run_census({"lib.rs": self.WRAPPER + '''
            macro_rules! reader {
                ($name:ident, $t:ty) => {
                    fn $name() -> Option<$t> { None }
                };
            }
            reader!(read_wrapper, Wrapper<u16>);
        '''})
        self.assertEqual(status, 0, output)

    SAFE_USE = '''
        #[derive(Deserialize)] #[serde(deny_unknown_fields)]
        struct Host { value: Wrapper<u8> }
    '''

    def test_an_instantiation_outside_a_declaration_is_read_beside_a_safe_one(self) -> None:
        """A signature, a const, a static, a const block, an impl header and a
        macro body each instantiate what they spell."""
        for case, written in {
            "a function's return": "pub fn read() -> Wrapper<Open> { todo!() }",
            "a function's argument": "pub fn take(value: Wrapper<Open>) {}",
            "a function's bound": "pub fn read<F: Fn() -> Wrapper<Open>>(f: F) {}",
            "an inherent method": "pub struct Doc; impl Doc { pub fn read() -> Wrapper<Open> { todo!() } }",
            "a trait method": "pub trait Read { fn read() -> Wrapper<Open>; }",
            "a static": "pub static CELL: std::sync::OnceLock<Wrapper<Open>> = std::sync::OnceLock::new();",
            "a const": "pub const NEXT: fn() -> Wrapper<Open> = next;",
            "a const block": "const _: () = { let _ = Wrapper::<Open>::A; };",
            "an impl header": "pub struct Doc; impl From<Wrapper<Open>> for Doc { fn from(_: Wrapper<Open>) -> Self { Doc } }",
            "a macro body": '''macro_rules! reader {
                () => { pub fn read(t: &str) -> Wrapper<Open> { serde_json::from_str(t).unwrap() } };
            }
            reader!();''',
        }.items():
            with self.subTest(case=case):
                status, output = self.run_census(
                    {"lib.rs": self.WRAPPER + self.OPEN + self.SAFE_USE + written}
                )
                self.assertEqual(status, 1, output)
                self.assertIn(
                    "Wrapper::A: payload T: generic parameter T instantiated as Open", output
                )

    def test_a_forwarded_signature_parameter_states_nothing_and_a_wrapped_one_is_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.WRAPPER + self.SAFE_USE + '''
            pub fn read<U>() -> Wrapper<U> { todo!() }
            pub struct Doc<V>(V);
            impl<V> Doc<V> { pub fn get(&self) -> Wrapper<V> { todo!() } }
        '''})
        self.assertEqual(status, 0, output)
        status, output = self.run_census({"lib.rs": self.WRAPPER + self.SAFE_USE + '''
            pub fn read<U>() -> Wrapper<Option<U>> { todo!() }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn(
            "fills T with Option<U>, which names the parameter U there", output
        )

    def test_a_macro_body_spelling_a_fragment_leaves_the_parameter_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.WRAPPER + self.SAFE_USE + '''
            macro_rules! reader {
                ($t:ty) => { pub fn read() -> Wrapper<$t> { todo!() } };
            }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("instantiated as $t", output)
    def test_generic_parameters_reads_names_bounds_and_defaults(self) -> None:
        item = census.Item(
            Path("crates/ir/src/lib.rs"), 1, "enum", "Reader",
            "", "pub enum Reader<'a, T: Clone = Closed, const N: usize> { A(T) }",
        )
        self.assertEqual(census.generic_parameters(item), [("T", "Closed")])


class AssociatedTypeProofTests(unittest.TestCase):
    """A payload named by a trait's associated type is proved through its impls."""

    run_census = DenyCensusTests.run_census

    SPACE = '''
        pub trait Space {
            type Entity: Clone;
        }
        #[derive(Deserialize)] #[serde(deny_unknown_fields)]
        pub struct ClosedId { value: u8 }
        pub struct Planar;
        impl Space for Planar { type Entity = ClosedId; }
        pub struct Spatial;
        impl Space for Spatial { type Entity = u64; }
    '''
    OPERAND = '''
        #[derive(Deserialize)]
        #[serde(tag = "kind", content = "value", deny_unknown_fields, bound = "")]
        pub enum Operand<S: Space> { Entity(S::Entity) }
    '''
    OPEN_ID = '''
        #[derive(Deserialize)]
        pub struct OpenId { value: u8 }
    '''

    def test_every_closed_impl_proves_the_payload(self) -> None:
        status, output = self.run_census({"lib.rs": self.SPACE + self.OPERAND})
        self.assertEqual(status, 0, output)

    def test_one_open_impl_leaves_the_payload_unproved(self) -> None:
        source = self.SPACE.replace("type Entity = u64;", "type Entity = OpenId;")
        status, output = self.run_census({"lib.rs": source + self.OPEN_ID + self.OPERAND})
        self.assertEqual(status, 1, output)
        self.assertIn("Operand::Entity: payload S::Entity: impl Space for Spatial", output)
        self.assertIn("type Entity = OpenId: payload reader OpenId lacks", output)

    def test_a_forwarded_payload_reader_follows_the_impls(self) -> None:
        source = self.SPACE.replace("type Entity = u64;", "type Entity = OpenId;")
        status, output = self.run_census({"lib.rs": source + self.OPEN_ID + self.OPERAND + '''
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "value", deny_unknown_fields, bound = "")]
            pub enum Axis<S: Space> { Along(Operand<S>) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Axis::Along: payload Operand<S>", output)

    def test_an_impl_that_states_no_type_is_unread(self) -> None:
        source = self.SPACE.replace("{ type Entity = u64; }", "{}")
        status, output = self.run_census({"lib.rs": source + self.OPERAND})
        self.assertEqual(status, 1, output)
        self.assertIn("impl Space for Spatial at", output)
        self.assertIn("states no type Entity", output)

    def test_an_impl_type_the_parser_cannot_read_is_unread(self) -> None:
        source = self.SPACE.replace("type Entity = u64;", "type Entity<'a> = u64;")
        status, output = self.run_census({"lib.rs": source + self.OPERAND})
        self.assertEqual(status, 1, output)
        self.assertIn("states a type Entity the census cannot read", output)

    def test_a_trait_without_an_impl_is_unread(self) -> None:
        source = '''
            pub trait Space { type Entity; }
        '''
        status, output = self.run_census({"lib.rs": source + self.OPERAND})
        self.assertEqual(status, 1, output)
        self.assertIn("Space has no impl in the wire crates", output)

    def test_an_impl_outside_the_wire_crates_is_unread(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            codec = Path(directory) / "crates" / "codec" / "src" / "lib.rs"
            codec.parent.mkdir(parents=True)
            codec.write_text('''
                use wire::Space;
                pub struct Codec;
                impl Space for Codec { type Entity = CodecId; }
            ''', encoding="utf-8")
            status, output = self.run_foreign(
                {"lib.rs": self.SPACE + self.OPERAND}, [codec]
            )
        self.assertEqual(status, 1, output)
        self.assertIn("impl Space for Codec at", output)
        self.assertIn("is outside the wire crates, so its associated types are unread", output)

    def run_foreign(self, files, foreign):
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
                    patch.object(census, "absent_key_failures", return_value=[]), \
                    patch.object(census, "trait_impl_source_files", return_value=foreign), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                status = census.main()
            return status, output.getvalue()

    def test_a_where_clause_bound_is_read(self) -> None:
        operand = self.OPERAND.replace(
            "enum Operand<S: Space> {", "enum Operand<S> where S: Space {"
        )
        status, output = self.run_census({"lib.rs": self.SPACE + operand})
        self.assertEqual(status, 0, output)

    def test_a_qualified_associated_type_names_its_trait(self) -> None:
        operand = self.OPERAND.replace("S::Entity", "<S as Space>::Entity")
        status, output = self.run_census({"lib.rs": self.SPACE + operand})
        self.assertEqual(status, 0, output)
        source = self.SPACE.replace("type Entity = u64;", "type Entity = OpenId;")
        status, output = self.run_census({"lib.rs": source + self.OPEN_ID + operand})
        self.assertEqual(status, 1, output)
        self.assertIn("payload <S as Space>::Entity: impl Space for Spatial", output)

    def test_standard_bounds_beside_the_trait_are_ignored(self) -> None:
        operand = self.OPERAND.replace(
            "<S: Space>", "<S: Clone + Space + serde::de::DeserializeOwned>"
        )
        status, output = self.run_census({"lib.rs": self.SPACE + operand})
        self.assertEqual(status, 0, output)

    def test_a_parameter_without_a_bound_is_unread(self) -> None:
        operand = self.OPERAND.replace("<S: Space>", "<S>")
        status, output = self.run_census({"lib.rs": self.SPACE + operand})
        self.assertEqual(status, 1, output)
        self.assertIn("generic parameter S of Operand states no trait bound", output)

    def test_a_bound_outside_the_workspace_is_unread(self) -> None:
        operand = self.OPERAND.replace("<S: Space>", "<S: Space + external::Mystery>")
        status, output = self.run_census({"lib.rs": self.SPACE + operand})
        self.assertEqual(status, 1, output)
        self.assertIn("the bound external::Mystery of S is no workspace trait", output)

    def test_two_bounds_declaring_the_type_name_no_one_trait(self) -> None:
        operand = self.OPERAND.replace("<S: Space>", "<S: Space + Other>")
        status, output = self.run_census({"lib.rs": self.SPACE + operand + '''
            pub trait Other { type Entity; }
            impl Other for Planar { type Entity = u8; }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("2 bounds of S declare Entity", output)

    def test_a_default_associated_type_is_unread(self) -> None:
        source = self.SPACE.replace("type Entity: Clone;", "type Entity: Clone = u8;")
        status, output = self.run_census({"lib.rs": source + self.OPERAND})
        self.assertEqual(status, 1, output)
        self.assertIn("Space::Entity states a default", output)

    def test_an_impl_in_another_module_is_read_where_it_is_written(self) -> None:
        status, output = self.run_census({
            "lib.rs": "mod space; mod planar; use crate::space::Space;" + self.OPERAND,
            "space.rs": '''
                pub trait Space { type Entity; }
            ''',
            "planar.rs": '''
                use crate::space::Space;
                #[derive(Deserialize)]
                pub struct Id { value: u8 }
                pub struct Planar;
                impl Space for Planar { type Entity = Id; }
            ''',
        })
        self.assertEqual(status, 1, output)
        self.assertIn("impl Space for Planar at", output)
        self.assertIn("planar.rs", output)
        self.assertIn("payload reader Id lacks checked unknown-key refusal", output)

    def test_a_same_named_trait_in_another_module_is_another_trait(self) -> None:
        files = {
            "lib.rs": "mod first; mod second;",
            "first.rs": self.SPACE + self.OPERAND,
            "second.rs": '''
                pub trait Space { type Entity; }
                #[derive(Deserialize)]
                pub struct Open { value: u8 }
                pub struct Loose;
                impl Space for Loose { type Entity = Open; }
            ''',
        }
        status, output = self.run_census(files)
        self.assertEqual(status, 1, output)
        self.assertIn("second.rs:3 Open", output)
        self.assertNotIn("Operand::Entity", output)
        files["second.rs"] += self.OPERAND
        status, output = self.run_census(files)
        self.assertEqual(status, 1, output)
        self.assertIn("impl Space for Loose", output)
        self.assertNotIn("impl Space for Spatial", output)

    def test_an_impl_whose_trait_resolves_nowhere_is_unread(self) -> None:
        status, output = self.run_census({
            "lib.rs": "mod first; mod second; mod stray;",
            "first.rs": self.SPACE + self.OPERAND,
            "second.rs": "pub trait Space { type Entity; }",
            "stray.rs": '''
                pub struct Stray;
                impl Space for Stray { type Entity = u8; }
            ''',
        })
        self.assertEqual(status, 1, output)
        self.assertIn("the trait of impl Space for Stray", output)
        self.assertIn("resolves to no one declaration", output)

    def test_a_macro_call_naming_the_trait_leaves_its_impls_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.SPACE + self.OPERAND + '''
            macro_rules! implement {
                ($trait:ident, $name:ident) => { impl $trait for $name { type Entity = u8; } };
            }
            pub struct Extra;
            implement!(Space, Extra);
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("names Space and its macro spells impl", output)

    def test_a_macro_impl_of_the_trait_states_an_unread_type(self) -> None:
        status, output = self.run_census({"lib.rs": self.SPACE + self.OPERAND + '''
            macro_rules! space {
                ($name:ident, $entity:ty) => {
                    impl $crate::Space for $name { type Entity = $entity; }
                };
            }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("impl Space for $name at", output)
        self.assertIn("type Entity = $entity", output)

    IMPLEMENT = '''
        macro_rules! implement {
            ($t:ident, $n:ident) => { impl $t for $n { type Entity = OpenId; } };
        }
        pub struct Extra;
    '''

    def test_a_wrapper_macro_naming_the_trait_leaves_its_impls_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.SPACE + self.OPERAND + self.OPEN_ID + self.IMPLEMENT + '''
            macro_rules! expand {
                ($t:ident, $n:ident) => { implement!($t, $n); };
            }
            expand!(Space, Extra);
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Operand::Entity: payload S::Entity: the macro call at", output)
        self.assertIn("names Space and the census does not expand it", output)

    def test_a_wrapper_macro_body_naming_the_trait_leaves_its_impls_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.SPACE + self.OPERAND + self.OPEN_ID + self.IMPLEMENT + '''
            macro_rules! expand {
                ($n:ident) => { implement!(Space, $n); };
            }
            expand!(Extra);
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("the macro_rules! expand at", output)
        self.assertIn("names Space outside an impl header the census reads", output)

    def test_a_renamed_macro_naming_the_trait_leaves_its_impls_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.SPACE + self.OPERAND + self.OPEN_ID + self.IMPLEMENT + '''
            use implement as expand;
            expand!(Space, Extra);
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Operand::Entity: payload S::Entity: the macro call at", output)
        self.assertIn("names Space and the census does not expand it", output)

    def test_an_attribute_naming_the_trait_leaves_its_impls_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.SPACE + self.OPERAND + '''
            #[external::implement(Space)]
            pub struct Extra;
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Operand::Entity: payload S::Entity: the attribute at", output)
        self.assertIn("names Space and the census does not expand it", output)

    def test_an_impl_through_a_renamed_trait_is_read(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            mod geom {
                pub trait Space { type Entity; }
                pub struct Planar;
                impl Space for Planar { type Entity = u8; }
            }
            use geom::Space as Plane;
            pub struct Decoy;
            impl Plane for Decoy { type Entity = OpenId; }
        ''' + self.OPEN_ID + self.OPERAND.replace("<S: Space>", "<S: geom::Space>")})
        self.assertEqual(status, 1, output)
        self.assertIn("Operand::Entity: payload S::Entity: impl Space for Decoy", output)
        self.assertIn("type Entity = OpenId: payload reader OpenId lacks", output)

    def test_bounds_are_read_from_the_header_and_the_where_clause(self) -> None:
        item = census.Item(
            Path("crates/ir/src/lib.rs"), 1, "enum", "Reader", "",
            "pub enum Reader<'a, S: Space + Clone, T: ?Sized + a::Other<X = u8> = u8>"
            " where S: serde::Serialize, 'a: 'a { A(S) }",
        )
        self.assertEqual(census.generic_bounds(item), {
            "S": ["Space", "Clone", "serde::Serialize"],
            "T": ["a::Other"],
        })


class FailClosedResolutionTests(unittest.TestCase):
    """A path is read as what it names in Rust, never as a nearby declaration."""

    run_census = DenyCensusTests.run_census

    OPERAND = AssociatedTypeProofTests.OPERAND
    SPACE = '''
        pub trait Space { type Entity; }
        pub struct Planar;
        impl Space for Planar { type Entity = u8; }
    '''

    def run_tree(self, files: dict[str, str]) -> tuple[int, str]:
        """Run the census script itself from a fixture workspace root."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for crate in census.ROOTS:
                (root / crate).mkdir(parents=True)
                (root / crate / "lib.rs").write_text("", encoding="utf-8")
            for name, source in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")
            completed = subprocess.run(
                [sys.executable, str(Path(__file__).with_name("check-deny-census.py"))],
                cwd=root, text=True, capture_output=True, check=False,
            )
            return completed.returncode, completed.stdout + completed.stderr

    def test_an_impl_parameter_is_never_read_as_a_module(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            mod T {
                #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                pub struct Entity { pub value: u8 }
            }
            pub trait Space { type Entity; }
            pub struct Holder<T> { marker: std::marker::PhantomData<T> }
            impl<T: Space> Space for Holder<T> { type Entity = T::Entity; }
        ''' + self.OPERAND})
        self.assertEqual(status, 1, output)
        self.assertIn("Operand::Entity: payload S::Entity: impl Space for Holder<T>", output)
        self.assertIn("the type T::Entity depends on what fills the impl", output)

    def test_an_impl_parameter_is_never_read_as_a_declaration(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            pub struct T { pub value: u8 }
            pub trait Space { type Entity; }
            pub struct Holder<T> { marker: std::marker::PhantomData<T> }
            impl<T> Space for Holder<T> { type Entity = T; }
        ''' + self.OPERAND})
        self.assertEqual(status, 1, output)
        self.assertIn("the type T depends on what fills the generic parameter T", output)

    def test_a_type_parameter_shadows_a_same_named_declaration(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            pub struct T { pub value: u8 }
            pub struct Open { pub value: u8 }
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            pub enum Wrapper<T> { A(T) }
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            pub struct Host { pub value: Wrapper<Open> }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Wrapper::A: payload T: generic parameter T instantiated as Open", output)

    def test_a_path_through_a_type_parameter_is_unread(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            mod S {
                pub mod Entity {
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    pub struct Inner { pub value: u8 }
                }
            }
        ''' + self.SPACE + self.OPERAND.replace("S::Entity", "S::Entity::Inner")})
        self.assertEqual(status, 1, output)
        self.assertIn("the type S::Entity::Inner depends on what fills the generic parameter S", output)

    DESCENDANT_TRAIT = '''
        mod geom {
            pub trait Space { type Entity; }
            pub struct Planar;
            impl Space for Planar { type Entity = GEOM; }
        }
        mod other {
            REEXPORT
            pub mod inner {
                pub trait Space { type Entity; }
                pub struct Decoy;
                impl Space for Decoy { type Entity = u8; }
            }
        }
    '''

    def descendant_trait(self, geom: str, reexport: str) -> dict[str, str]:
        source = self.DESCENDANT_TRAIT.replace("GEOM", geom).replace("REEXPORT", reexport)
        return {"lib.rs": source + self.OPERAND.replace("<S: Space>", "<S: other::Space>")}

    def test_a_reexported_trait_is_the_trait_it_reexports(self) -> None:
        status, output = self.run_census(
            self.descendant_trait("Foreign", "pub use crate::geom::Space;")
        )
        self.assertEqual(status, 1, output)
        self.assertIn("impl Space for Planar", output)
        self.assertIn("type Entity = Foreign", output)
        self.assertNotIn("Decoy", output)

    def test_a_reexported_trait_with_an_open_impl_fails_its_payload(self) -> None:
        files = self.descendant_trait("OpenId", "pub use crate::geom::Space;")
        files["lib.rs"] += '''
            #[derive(Deserialize)]
            pub struct OpenId { value: u8 }
        '''
        status, output = self.run_census(files)
        self.assertEqual(status, 1, output)
        self.assertIn("Operand::Entity: payload S::Entity: impl Space for Planar", output)

    def test_a_descendant_trait_is_not_the_trait_a_shorter_path_names(self) -> None:
        status, output = self.run_census(self.descendant_trait("u8", ""))
        self.assertEqual(status, 1, output)
        self.assertIn("the bound other::Space of S is no workspace trait", output)

    def test_a_reexported_reader_is_the_reader_it_reexports(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            mod open_mod {
                #[derive(Deserialize)]
                pub struct Wrapper<T> { pub value: T }
            }
            mod closed {
                pub mod inner {
                    #[derive(Deserialize)] #[serde(deny_unknown_fields)]
                    pub struct Wrapper<T> { pub value: T }
                }
                pub use crate::open_mod::Wrapper;
            }
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            pub enum Doc { Face(closed::Wrapper<u8>) }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Doc::Face: payload closed::Wrapper<u8>: payload reader closed::Wrapper lacks", output)

    REEXPORTED_CLOSED = {
        "lib.rs": '''
            mod geom;
            mod api;
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            pub enum Doc { Face(api::Closed) }
        ''',
        "geom.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            pub struct Closed { pub value: u8 }
            pub struct Open { pub value: u8 }
        ''',
    }

    def test_reexports_are_followed_where_they_are_written(self) -> None:
        for api in (
            "pub use crate::geom::Closed;",
            "pub use super::geom::Closed;",
            "pub use crate::geom::*;",
            "pub use crate::geom::Open as Closed;",
        ):
            files = dict(self.REEXPORTED_CLOSED, **{"api.rs": api})
            status, output = self.run_census(files)
            if "Open" in api:
                self.assertEqual(status, 1, output)
                self.assertIn("payload reader api::Closed lacks", output)
            else:
                self.assertEqual(status, 0, f"{api}: {output}")

    def test_two_globs_bringing_in_a_name_resolve_it_nowhere(self) -> None:
        files = dict(self.REEXPORTED_CLOSED, **{
            "api.rs": "pub use crate::geom::*; pub use crate::other::*;",
            "other.rs": "pub struct Closed;",
        })
        files["lib.rs"] = "mod other;" + files["lib.rs"]
        status, output = self.run_census(files)
        self.assertEqual(status, 1, output)
        self.assertIn("Doc::Face: payload api::Closed: cannot resolve imported payload reader", output)

    def test_a_turbofish_argument_instantiates_the_parameter(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            pub enum Wrapper<T = u8> { A(T) }
            pub struct Foreign;
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            pub struct Host { pub value: Wrapper::<Foreign> }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Wrapper::A: payload T: generic parameter T instantiated as Foreign", output)

    def test_an_impl_stated_argument_instantiates_the_parameter(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            pub enum Wrapper<T = u8> { A(T) }
            pub struct Foreign;
            pub trait Space { type Entity; }
            pub struct Planar;
            impl Space for Planar { type Entity = Wrapper<Foreign>; }
        ''' + self.OPERAND})
        self.assertEqual(status, 1, output)
        self.assertIn("Wrapper::A: payload T: generic parameter T instantiated as Foreign", output)

    def test_an_impl_parameter_argument_is_unread(self) -> None:
        status, output = self.run_census({"lib.rs": '''
            #[derive(Deserialize)] #[serde(deny_unknown_fields)]
            pub struct T { pub value: u8 }
            #[derive(Deserialize)]
            #[serde(tag = "kind", content = "value", deny_unknown_fields)]
            pub enum Wrapper<T = u8> { A(T) }
            pub trait Space { type Entity; }
            pub struct Holder<T> { marker: std::marker::PhantomData<T> }
            impl<T> Space for Holder<T> { type Entity = Wrapper<T>; }
        ''' + self.OPERAND})
        self.assertEqual(status, 1, output)
        self.assertIn("generic parameter T instantiated as T: the type T depends on", output)

    WIRE = {"crates/cadmpeg-ir/src/lib.rs": SPACE + OPERAND}

    def test_the_census_proves_a_wire_impl_from_a_workspace_root(self) -> None:
        status, output = self.run_tree(self.WIRE)
        self.assertEqual(status, 0, output)

    def test_an_impl_under_examples_is_outside_the_wire_crates(self) -> None:
        for name in ("examples/demo/src/lib.rs", "examples/demo.rs", "build.rs"):
            files = dict(self.WIRE, **{name: '''
                pub struct Demo;
                impl Space for Demo { type Entity = DemoId; }
            '''})
            status, output = self.run_tree(files)
            self.assertEqual(status, 1, f"{name}: {output}")
            self.assertIn(f"impl Space for Demo at {name}:3 is outside the wire crates", output)

    def test_a_foreign_file_the_census_cannot_read_leaves_the_trait_unread(self) -> None:
        files = dict(self.WIRE, **{
            "examples/demo.rs": "impl Space for Demo { type Entity = DemoId;",
        })
        status, output = self.run_tree(files)
        self.assertEqual(status, 1, output)
        self.assertIn(
            "the impl at examples/demo.rs:1 names Space, and the census cannot parse its header",
            output,
        )
        self.assertNotIn("Traceback", output)

    def test_a_foreign_call_to_an_implementing_macro_leaves_the_trait_unread(self) -> None:
        files = dict(self.WIRE, **{
            "crates/codec/src/macros.rs": '''
                macro_rules! implement {
                    ($trait:path, $name:ident) => { impl $trait for $name { type Entity = Open; } };
                }
            ''',
            "crates/codec/src/lib.rs": "pub struct Codec;\nimplement!(Space, Codec);\n",
        })
        status, output = self.run_tree(files)
        self.assertEqual(status, 1, output)
        self.assertIn("the macro call at crates/codec/src/lib.rs:2 names Space", output)

    FOREIGN_IMPLEMENT = '''
        #[derive(Deserialize)]
        pub struct OpenId { pub value: u8 }
        macro_rules! implement {
            ($t:ident, $n:ident) => { impl $t for $n { type Entity = OpenId; } };
        }
        pub struct Extra;
    '''

    def test_a_foreign_wrapper_macro_naming_the_trait_leaves_it_unread(self) -> None:
        files = dict(self.WIRE, **{"examples/demo.rs": self.FOREIGN_IMPLEMENT + '''
            macro_rules! expand {
                ($t:ident, $n:ident) => { implement!($t, $n); };
            }
            expand!(Space, Extra);
        '''})
        status, output = self.run_tree(files)
        self.assertEqual(status, 1, output)
        self.assertIn(
            "Operand::Entity: payload S::Entity: the macro call at examples/demo.rs:"
            "12 names Space outside the wire crates",
            output,
        )

    def test_a_renamed_foreign_macro_naming_the_trait_leaves_it_unread(self) -> None:
        files = dict(self.WIRE, **{"examples/demo.rs": self.FOREIGN_IMPLEMENT + '''
            use implement as expand;
            expand!(Space, Extra);
        '''})
        status, output = self.run_tree(files)
        self.assertEqual(status, 1, output)
        self.assertIn("the macro call at examples/demo.rs:10 names Space outside the wire crates", output)

    def test_a_foreign_impl_through_a_renamed_trait_is_outside_the_wire_crates(self) -> None:
        files = dict(self.WIRE, **{"crates/codec/src/lib.rs": '''
            use cadmpeg_ir::Space as Plane;
            pub struct Codec;
            impl Plane for Codec { type Entity = CodecId; }
        '''})
        status, output = self.run_tree(files)
        self.assertEqual(status, 1, output)
        self.assertIn(
            "impl Plane for Codec at crates/codec/src/lib.rs:4 is outside the wire crates", output
        )

    def test_build_output_and_hidden_directories_hold_no_impl(self) -> None:
        impl = "pub struct Demo; impl Space for Demo { type Entity = DemoId; }"
        status, output = self.run_tree(dict(self.WIRE, **{
            "target/debug/build/out.rs": impl,
            ".git/hooks/demo.rs": impl,
            "crates/codec/tests/fixture.rs": impl,
        }))
        self.assertEqual(status, 0, output)

    GENERIC_SPACE = '''
        pub trait Space<F = ()> { type Entity; }
        pub struct Planar;
        impl Space for Planar { type Entity = u8; }
    '''

    def assert_open_impl_read(self, space: str, extra: str, described: str) -> None:
        """The census reads the impl in ``extra`` and fails its open type."""
        status, output = self.run_census({"lib.rs": space + self.OPERAND + extra})
        self.assertEqual(status, 1, output)
        self.assertIn(f"Operand::Entity: payload S::Entity: {described} at", output)
        self.assertIn("type Entity = Foreign", output)

    def test_an_fn_bound_in_the_impl_generics_is_read(self) -> None:
        self.assert_open_impl_read(self.SPACE, '''
            pub struct Holder<F>(F);
            impl<F: Fn() -> u8> Space for Holder<F> { type Entity = Foreign; }
        ''', "impl Space for Holder<F>")

    def test_a_binder_in_the_trait_arguments_is_read(self) -> None:
        self.assert_open_impl_read(self.GENERIC_SPACE, '''
            pub struct Extra;
            impl Space<for<'a> fn(&'a u8)> for Extra { type Entity = Foreign; }
        ''', "impl Space for Extra")

    def test_a_shift_in_an_array_length_of_the_trait_arguments_is_read(self) -> None:
        self.assert_open_impl_read(self.GENERIC_SPACE, '''
            pub struct Extra;
            impl Space<[u8; 1 << 2]> for Extra { type Entity = Foreign; }
        ''', "impl Space for Extra")

    def test_a_shift_in_a_const_block_of_the_self_type_is_read(self) -> None:
        self.assert_open_impl_read(self.SPACE, '''
            pub struct Arr<const N: usize>;
            impl Space for Arr<{ 1 << 2 }> { type Entity = Foreign; }
        ''', "impl Space for Arr<{ 1 << 2 }>")

    def test_a_const_impl_is_read(self) -> None:
        self.assert_open_impl_read(self.SPACE, '''
            pub struct Extra;
            impl const Space for Extra { type Entity = Foreign; }
        ''', "impl Space for Extra")

    def test_a_raw_identifier_trait_is_the_trait(self) -> None:
        self.assert_open_impl_read(self.SPACE, '''
            pub struct Extra;
            impl r#Space for Extra { type Entity = Foreign; }
        ''', "impl Space for Extra")

    def test_a_raw_identifier_in_the_trait_path_is_the_trait(self) -> None:
        self.assert_open_impl_read(self.SPACE, '''
            pub struct Extra;
            impl crate::r#Space for Extra { type Entity = Foreign; }
        ''', "impl Space for Extra")

    def test_a_wire_impl_header_the_parser_rejects_leaves_the_trait_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.GENERIC_SPACE + self.OPERAND + '''
            pub struct Extra;
            impl Space::<()> for Extra { type Entity = Foreign; }
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Operand::Entity: payload S::Entity: the impl at ", output)
        self.assertIn(
            "lib.rs:11 names Space, and the census cannot parse its header", output
        )

    def assert_foreign_impl_unread(self, codec: str, reason: str) -> None:
        status, output = self.run_tree(dict(self.WIRE, **{"crates/codec/src/lib.rs": codec}))
        self.assertEqual(status, 1, output)
        self.assertIn(f"Operand::Entity: payload S::Entity: {reason}", output)

    def test_a_foreign_impl_with_an_fn_bound_is_outside_the_wire_crates(self) -> None:
        self.assert_foreign_impl_unread(
            "pub struct Holder<F>(F);\n"
            "impl<F: Fn() -> u8> cadmpeg_ir::Space for Holder<F> { type Entity = CodecId; }\n",
            "impl cadmpeg_ir::Space for Holder<F> at crates/codec/src/lib.rs:2 is outside",
        )

    def test_a_foreign_impl_with_a_binder_is_outside_the_wire_crates(self) -> None:
        self.assert_foreign_impl_unread(
            "pub struct Codec;\n"
            "impl cadmpeg_ir::Space<for<'a> fn(&'a u8)> for Codec { type Entity = CodecId; }\n",
            "impl cadmpeg_ir::Space for Codec at crates/codec/src/lib.rs:2 is outside",
        )

    def test_a_foreign_raw_identifier_impl_is_outside_the_wire_crates(self) -> None:
        self.assert_foreign_impl_unread(
            "pub struct Codec;\n"
            "impl cadmpeg_ir::r#Space for Codec { type Entity = CodecId; }\n",
            "impl cadmpeg_ir::Space for Codec at crates/codec/src/lib.rs:2 is outside",
        )

    def test_a_foreign_impl_header_the_parser_rejects_leaves_the_trait_unread(self) -> None:
        self.assert_foreign_impl_unread(
            "pub struct Codec;\n"
            "impl cadmpeg_ir::Space::<()> for Codec { type Entity = CodecId; }\n",
            "the impl at crates/codec/src/lib.rs:2 names Space, and the census "
            "cannot parse its header",
        )

    WRAPPER = '''
        #[derive(Deserialize)]
        #[serde(tag = "kind", content = "value", deny_unknown_fields)]
        pub enum Wrapper<T = u8> { A(T) }
    '''

    def test_a_macro_naming_the_reader_beside_its_argument_leaves_it_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.WRAPPER + '''
            macro_rules! host {
                ($w:ident, $t:ty) => {
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    pub struct Host { pub value: $w<$t> }
                };
            }
            host!(Wrapper, Foreign);
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Wrapper::A: payload T: the macro call at ", output)
        self.assertIn("lib.rs:13 names Wrapper, and the census does not expand it", output)

    def test_a_foreign_macro_naming_the_reader_leaves_it_unread(self) -> None:
        status, output = self.run_census({"lib.rs": self.WRAPPER + '''
            other_crate::host!(Wrapper, Foreign);
        '''})
        self.assertEqual(status, 1, output)
        self.assertIn("Wrapper::A: payload T: the macro call at ", output)
        self.assertIn("lib.rs:6 names Wrapper, and the census does not expand it", output)


if __name__ == "__main__":
    unittest.main()
