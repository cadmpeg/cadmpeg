#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Regression tests for the absent-key census."""

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


class AbsentKeyFixtures(unittest.TestCase):
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
            with patch.object(census, "absent_key_source_files",
                                 return_value=paths), \
                    patch.object(census, "ABSENT_KEY_EXCEPTIONS", {}), \
                    patch.object(census, "ABSENT_KEY_PROJECTIONS", {}), \
                    contextlib.redirect_stdout(output), \
                    contextlib.redirect_stderr(output):
                status = census.main()
            return status, output.getvalue()


class AbsentKeyCensusTests(unittest.TestCase):
    """The absence spelling of an optional key whose writer omits it."""

    run_absent_key_census = AbsentKeyFixtures.run_absent_key_census

    def test_an_unreadable_declaration_is_a_failure(self) -> None:
        output = io.StringIO()
        with patch.object(census, "present_forwarders", return_value=({}, [])), \
                patch.object(census, "crate_sources", return_value={"fixture": [Path("fixture.rs")]}), \
                patch.object(census, "collect_items", side_effect=ValueError("incomplete declaration")), \
                patch.object(census, "absent_key_source_files", return_value=[]), \
                contextlib.redirect_stderr(output):
            status = census.main()
        self.assertEqual(status, 1)
        self.assertIn("incomplete declaration", output.getvalue())

    def test_macro_type_arm_does_not_hide_a_later_optional_key(self) -> None:
        source = 'macro_rules! record { (@type field, $kind:ident) => { Vec<u8> }; }\n'
        findings = self.run_absent_key_census({"wire.rs": source + self.OMITTED % ""})
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("Wire.key", findings[0])

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

    def test_a_finding_names_the_field_attribute_line(self) -> None:
        status, output = self.run_absent_key_main({"wire.rs": self.OMITTED % ""})
        self.assertEqual(status, 1)
        self.assertIn("wire.rs:4 ", output)

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

    run_absent_key_main = AbsentKeyFixtures.run_absent_key_main

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


if __name__ == "__main__":
    unittest.main()
