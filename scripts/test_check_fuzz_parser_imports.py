#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-fuzz-parser-imports.py``."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-fuzz-parser-imports.py")
SPEC = importlib.util.spec_from_file_location("check_fuzz_parser_imports", SCRIPT)
assert SPEC and SPEC.loader
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)


def _tree(files: dict[str, str]) -> Path:
    root = Path(tempfile.mkdtemp())
    directory = root / "crates" / "cadmpeg-fuzz" / "fuzz_targets"
    directory.mkdir(parents=True)
    for name, text in files.items():
        (directory / name).write_text(text, encoding="utf-8")
    return root


class ScanSource(unittest.TestCase):
    def test_fuzz_path_is_allowed(self) -> None:
        self.assertEqual(
            checker.scan_source("cadmpeg_codec_nx::fuzz::geometry_points(data);"),
            [],
        )

    def test_codec_type_is_allowed(self) -> None:
        self.assertEqual(
            checker.scan_source("use cadmpeg_codec_nx::NxCodec;"),
            [],
        )

    def test_parser_path_is_banned(self) -> None:
        self.assertEqual(
            checker.scan_source("use cadmpeg_codec_nx::geometry::points;"),
            ["cadmpeg_codec_nx::geometry"],
        )

    def test_brace_import_is_banned(self) -> None:
        self.assertEqual(
            checker.scan_source("use cadmpeg_codec_nx::{container, parasolid};"),
            ["cadmpeg_codec_nx::container", "cadmpeg_codec_nx::parasolid"],
        )

    def test_creo_scalar_is_banned(self) -> None:
        self.assertEqual(
            checker.scan_source(
                "use cadmpeg_codec_creo::scalar::{decode, ScalarCache};"
            ),
            ["cadmpeg_codec_creo::scalar"],
        )


class CheckTree(unittest.TestCase):
    def test_missing_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(FileNotFoundError):
                checker.check(Path(tmp))

    def test_allows_fuzz_imports(self) -> None:
        root = _tree(
            {
                "nx_geometry_points.rs": "cadmpeg_codec_nx::fuzz::geometry_points(data);\n",
                "creo_scalar.rs": "use cadmpeg_codec_creo::fuzz::scalar;\n",
            }
        )
        self.assertEqual(checker.check(root), [])

    def test_reports_parser_imports(self) -> None:
        root = _tree(
            {
                "nx_geometry_points.rs": "use cadmpeg_codec_nx::geometry::points;\n",
            }
        )
        failures = checker.check(root)
        self.assertEqual(len(failures), 1)
        self.assertIn("nx_geometry_points.rs", failures[0])
        self.assertIn("cadmpeg_codec_nx::geometry", failures[0])

    def test_excludes_container_targets(self) -> None:
        root = _tree(
            {
                "nx_container.rs": "use cadmpeg_codec_nx::container;\n",
                "creo_container.rs": "use cadmpeg_codec_creo::container::scan_bytes;\n",
                "nx_om.rs": "use cadmpeg_codec_nx::fuzz;\n",
            }
        )
        self.assertEqual(checker.check(root), [])

    def test_ignores_unrelated_targets(self) -> None:
        root = _tree(
            {
                "decode_pipeline_mutated.rs": "use cadmpeg_codec_nx::geometry;\n",
                "nx_deltas.rs": "use cadmpeg_codec_nx::fuzz;\n",
            }
        )
        self.assertEqual(checker.check(root), [])


if __name__ == "__main__":
    unittest.main()
