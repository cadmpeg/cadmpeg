#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-fuzz-seeds.py``. These never run cargo."""

from __future__ import annotations

import importlib.util
import io
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("check-fuzz-seeds.py")
SPEC = importlib.util.spec_from_file_location("check_fuzz_seeds", SCRIPT)
assert SPEC and SPEC.loader
seeds = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = seeds
SPEC.loader.exec_module(seeds)


def write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def plant_tree(tree: Path) -> Path:
    """Write one three-file seed tree and return its root."""
    write(tree / "f3d_container" / "empty_zip", b"PK\x05\x06")
    write(tree / "f3d_container" / "empty_zip.mut_flip", b"\xaf\x05\x06")
    write(tree / "ir_from_json" / "minimal.json", b"{}\n")
    return tree


def build_pair(root: Path) -> tuple[Path, Path]:
    """Write two equal seed trees and return them."""
    return plant_tree(root / "committed"), plant_tree(root / "generated")


class RelativeFiles(unittest.TestCase):
    def test_lists_nested_regular_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, _ = build_pair(Path(tmp))
            self.assertEqual(
                seeds.relative_files(committed),
                {
                    "f3d_container/empty_zip",
                    "f3d_container/empty_zip.mut_flip",
                    "ir_from_json/minimal.json",
                },
            )

    def test_absent_tree_is_empty(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(seeds.relative_files(Path(tmp) / "absent"), set())


class CompareTrees(unittest.TestCase):
    def test_equal_trees_report_nothing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, generated = build_pair(Path(tmp))
            self.assertEqual(seeds.compare_trees(committed, generated, authored=()), [])

    def test_one_changed_byte_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, generated = build_pair(Path(tmp))
            write(generated / "ir_from_json" / "minimal.json", b"{ }\n")
            self.assertEqual(
                seeds.compare_trees(committed, generated, authored=()),
                ["differs: ir_from_json/minimal.json"],
            )

    def test_equal_length_different_bytes_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, generated = build_pair(Path(tmp))
            write(generated / "f3d_container" / "empty_zip", b"PK\x05\x07")
            self.assertEqual(
                seeds.compare_trees(committed, generated, authored=()),
                ["differs: f3d_container/empty_zip"],
            )

    def test_file_the_generators_do_not_write_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, generated = build_pair(Path(tmp))
            (generated / "ir_from_json" / "minimal.json").unlink()
            self.assertEqual(
                seeds.compare_trees(committed, generated, authored=()),
                ["missing from the generator run: ir_from_json/minimal.json"],
            )

    def test_file_not_checked_in_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, generated = build_pair(Path(tmp))
            write(generated / "nx_container" / "single_part", b"\x00\x01")
            self.assertEqual(
                seeds.compare_trees(committed, generated, authored=()),
                ["extra in the generator run: nx_container/single_part"],
            )

    def test_every_difference_is_named_and_sorted(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, generated = build_pair(Path(tmp))
            write(generated / "ir_from_json" / "minimal.json", b"{ }\n")
            (generated / "f3d_container" / "empty_zip.mut_flip").unlink()
            write(generated / "zz_extra" / "seed", b"x")
            self.assertEqual(
                seeds.compare_trees(committed, generated, authored=()),
                [
                    "missing from the generator run: "
                    "f3d_container/empty_zip.mut_flip",
                    "differs: ir_from_json/minimal.json",
                    "extra in the generator run: zz_extra/seed",
                ],
            )

    def test_absent_generated_tree_reports_every_committed_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, _ = build_pair(Path(tmp))
            differences = seeds.compare_trees(committed, Path(tmp) / "absent", authored=())
            self.assertEqual(len(differences), 3)
            self.assertTrue(
                all(d.startswith("missing from the generator run: ") for d in differences)
            )


class AuthoredSeeds(unittest.TestCase):
    """The seeds no generator writes are pinned one by one."""

    def test_authored_seed_absent_from_the_run_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, generated = build_pair(Path(tmp))
            write(committed / "step_lexer" / "minimal", b"ISO-10303-21;\n")
            self.assertEqual(
                seeds.compare_trees(
                    committed, generated, authored=("step_lexer/minimal",)
                ),
                [],
            )

    def test_unlisted_committed_seed_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, generated = build_pair(Path(tmp))
            write(committed / "step_lexer" / "minimal", b"ISO-10303-21;\n")
            self.assertEqual(
                seeds.compare_trees(committed, generated, authored=()),
                ["missing from the generator run: step_lexer/minimal"],
            )

    def test_listed_seed_that_the_run_writes_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, generated = build_pair(Path(tmp))
            self.assertEqual(
                seeds.compare_trees(
                    committed,
                    generated,
                    authored=("ir_from_json/minimal.json",),
                ),
                ["listed as hand-authored but generated: ir_from_json/minimal.json"],
            )

    def test_listed_seed_that_is_not_checked_in_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            committed, generated = build_pair(Path(tmp))
            self.assertEqual(
                seeds.compare_trees(
                    committed, generated, authored=("step_lexer/deleted",)
                ),
                ["listed as hand-authored but not checked in: step_lexer/deleted"],
            )

    def test_every_listed_seed_is_checked_in_and_unique(self) -> None:
        self.assertEqual(len(set(seeds.AUTHORED)), len(seeds.AUTHORED))
        self.assertEqual(list(seeds.AUTHORED), sorted(seeds.AUTHORED))
        for name in seeds.AUTHORED:
            self.assertTrue(
                (seeds.COMMITTED_SEEDS / name).is_file(),
                f"{name} is listed as hand-authored but is not in the seed tree",
            )


class WorkDirectory(unittest.TestCase):
    def test_work_dir_inside_the_repository_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            inside = root / "crates"
            inside.mkdir()
            stderr = io.StringIO()
            with patch.object(seeds, "ROOT", root), redirect_stderr(stderr):
                self.assertEqual(seeds.main(["--work-dir", str(inside)]), 2)
            self.assertIn("inside the repository", stderr.getvalue())

    def test_repository_root_itself_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            with patch.object(seeds, "ROOT", root), redirect_stderr(io.StringIO()):
                self.assertEqual(seeds.main(["--work-dir", str(root)]), 2)

    def test_absent_work_dir_exits_two(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            absent = Path(tmp) / "absent"
            stderr = io.StringIO()
            with redirect_stderr(stderr):
                self.assertEqual(seeds.main(["--work-dir", str(absent)]), 2)
            self.assertIn(str(absent), stderr.getvalue())


class Reporting(unittest.TestCase):
    """Drive ``main`` with the build and the generators stubbed out."""

    def run_main(self, work_dir: Path, plant) -> tuple[int, str, str]:
        def fake_run(seed_root: Path) -> None:
            plant(seed_root)
            return None

        stdout, stderr = io.StringIO(), io.StringIO()
        with patch.object(seeds, "build_generators", return_value=0), patch.object(
            seeds, "run_generators", side_effect=fake_run
        ), redirect_stdout(stdout), redirect_stderr(stderr):
            code = seeds.main(["--work-dir", str(work_dir)])
        return code, stdout.getvalue(), stderr.getvalue()

    def test_matching_tree_exits_zero_with_one_line(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            committed, _ = build_pair(root)
            with patch.object(seeds, "COMMITTED_SEEDS", committed), patch.object(
                seeds, "AUTHORED", ()
            ):
                code, out, _ = self.run_main(root, plant_tree)
            self.assertEqual(code, 0)
            self.assertEqual(len(out.strip().splitlines()), 1)
            self.assertIn("3 seed file(s) reproduce", out)
            self.assertIn("0 hand-authored", out)

    def test_difference_exits_one_and_names_the_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            committed, _ = build_pair(root)

            def plant(seed_root: Path) -> None:
                write(seed_root / "ir_from_json" / "minimal.json", b"{ }\n")

            with patch.object(seeds, "COMMITTED_SEEDS", committed), patch.object(
                seeds, "AUTHORED", ()
            ):
                code, out, err = self.run_main(root, plant)
            self.assertEqual(code, 1)
            self.assertIn("differs: ir_from_json/minimal.json", out)
            self.assertIn("missing from the generator run: f3d_container/empty_zip", out)
            self.assertIn("difference(s)", err)

    def test_build_failure_exits_two(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            stderr = io.StringIO()
            with patch.object(seeds, "build_generators", return_value=101), redirect_stderr(
                stderr
            ):
                self.assertEqual(seeds.main(["--work-dir", tmp]), 2)
            self.assertIn("cargo build failed", stderr.getvalue())

    def test_generator_failure_exits_two(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            stderr = io.StringIO()
            with patch.object(seeds, "build_generators", return_value=0), patch.object(
                seeds, "run_generators", return_value="generate_seeds exited 1"
            ), redirect_stderr(stderr):
                self.assertEqual(seeds.main(["--work-dir", tmp]), 2)
            self.assertIn("generate_seeds exited 1", stderr.getvalue())


class GeneratorSequence(unittest.TestCase):
    def test_seven_generators_in_the_documented_order(self) -> None:
        self.assertEqual(
            seeds.GENERATORS,
            (
                "generate_seeds",
                "generate_comprehensive_seeds",
                "generate_all_seeds",
                "generate_submodule_seeds",
                "generate_rhino_seeds",
                "generate_iges_seeds",
                "generate_fcstd_seeds",
            ),
        )

    def test_readme_names_the_same_sequence(self) -> None:
        readme = (seeds.CRATE / "README.md").read_text(encoding="utf-8")
        order = [
            line.rsplit("--bin ", 1)[1].strip()
            for line in readme.splitlines()
            if "--bin generate_" in line
        ]
        self.assertEqual(tuple(order), seeds.GENERATORS)

    def test_missing_generator_binary_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(seeds, "CRATE", Path(tmp)):
                failure = seeds.run_generators(Path(tmp) / "seeds")
            self.assertEqual(failure, "generator binary not built: generate_seeds")


if __name__ == "__main__":
    unittest.main()
