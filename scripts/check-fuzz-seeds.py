#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check that the checked-in fuzz seed tree is the output of its generators.

Build the seven seed generators in ``crates/cadmpeg-fuzz``, run them in the
fixed order of that crate's README into a temporary tree outside the
repository, and compare the result with ``crates/cadmpeg-fuzz/seeds`` file by
file.

``CADMPEG_FUZZ_SEED_ROOT`` redirects ``seed_dir`` in the generators to the
temporary tree. The donated fixture a generator reads is not redirected.

The seeds in ``AUTHORED`` are hand-authored parser inputs no generator writes.
Every other checked-in seed must equal the run byte for byte.

Exit 0 and one line when every file matches. Exit 1 and one line per differing,
missing, or extra path otherwise, keeping the generated tree for inspection.
Exit 2 when the build or a generator fails, or the work directory is unusable.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATE = ROOT / "crates" / "cadmpeg-fuzz"
COMMITTED_SEEDS = CRATE / "seeds"
SEED_ROOT_ENV = "CADMPEG_FUZZ_SEED_ROOT"

# The order fixes the container targets that more than one generator writes.
GENERATORS = (
    "generate_seeds",
    "generate_comprehensive_seeds",
    "generate_all_seeds",
    "generate_submodule_seeds",
    "generate_rhino_seeds",
    "generate_iges_seeds",
    "generate_fcstd_seeds",
)


# Seeds no generator writes. These are hand-authored parser inputs and reduced
# crash artifacts promoted into the tree, one per line so a new one is a
# deliberate edit here and drift cannot hide behind a wildcard.
AUTHORED = (
    "decode_pipeline_mutated/iges_point",
    "decode_pipeline_mutated/sat_sphere",
    "decode_pipeline_mutated/step_minimal",
    "f3d_writer/directed_subd_sum.json",
    "f3d_writer/minimal.json",
    "f3d_writer/unit_cube.json",
    "fcstd_write/binary_exact_shape.FCStd",
    "fcstd_write/external_component.FCStd",
    "iges_cards/point_5_3",
    "iges_cards/trimmed_plane_5_3",
    "iges_cards/truncated",
    "iges_directory/point_5_3",
    "iges_directory/trimmed_plane_5_3",
    "iges_directory/truncated",
    "iges_global/point_5_3",
    "iges_global/trimmed_plane_5_3",
    "iges_global/truncated",
    "iges_parameters/point_5_3",
    "iges_parameters/trimmed_plane_5_3",
    "iges_parameters/truncated",
    "sat_container/text_sphere",
    "sat_container/truncated_header",
    "step_decode/minimal",
    "step_geometry_degenerate/ap242_minimal.p21",
    "step_geometry_degenerate/noncanonical_solid_angle.p21",
    "step_geometry_degenerate/strings.p21",
    "step_lexer/escapes",
    "step_lexer/minimal",
    "step_parser/edition3",
    "step_parser/minimal",
    "step_reader/minimal",
    "step_reader/units",
)


def relative_files(tree: Path) -> set[str]:
    """Return every regular file under ``tree`` as a POSIX relative path."""
    if not tree.is_dir():
        return set()
    return {
        path.relative_to(tree).as_posix()
        for path in tree.rglob("*")
        if path.is_file()
    }


def compare_trees(
    committed: Path,
    generated: Path,
    authored: tuple[str, ...] = AUTHORED,
) -> list[str]:
    """Report every path whose bytes differ, or that only one tree holds.

    ``authored`` names the checked-in seeds no generator writes. Each one must
    be present and must stay outside the generator run; every other checked-in
    seed must match the run byte for byte.
    """
    left = relative_files(committed)
    right = relative_files(generated)
    known = set(authored)
    differences: list[str] = []
    for name in sorted((left - known) | right):
        if name not in right:
            differences.append(f"missing from the generator run: {name}")
        elif name not in left:
            differences.append(f"extra in the generator run: {name}")
        elif (committed / name).read_bytes() != (generated / name).read_bytes():
            differences.append(f"differs: {name}")
    for name in sorted(known):
        if name in right:
            differences.append(f"listed as hand-authored but generated: {name}")
        elif name not in left:
            differences.append(f"listed as hand-authored but not checked in: {name}")
    return differences


def build_generators() -> int:
    """Build the seven generator binaries. Return the cargo exit status."""
    command = ["cargo", "build", "--quiet"]
    for name in GENERATORS:
        command += ["--bin", name]
    result = subprocess.run(command, cwd=CRATE, capture_output=True, text=True)
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
    return result.returncode


def run_generators(seed_root: Path) -> str | None:
    """Run the generators in order into ``seed_root``. Return a failure line."""
    environment = dict(os.environ)
    environment[SEED_ROOT_ENV] = str(seed_root)
    for name in GENERATORS:
        binary = CRATE / "target" / "debug" / name
        if not binary.is_file():
            return f"generator binary not built: {name}"
        result = subprocess.run(
            [str(binary)],
            cwd=CRATE,
            env=environment,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            sys.stderr.write(result.stdout)
            sys.stderr.write(result.stderr)
            return f"{name} exited {result.returncode}"
    return None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--work-dir",
        default=tempfile.gettempdir(),
        help="directory to create the generated tree in (default: system temp)",
    )
    args = parser.parse_args(argv)

    work_dir = Path(args.work_dir).resolve()
    if work_dir == ROOT or ROOT in work_dir.parents:
        print(
            f"error: work directory {work_dir} is inside the repository",
            file=sys.stderr,
        )
        return 2
    try:
        generated = Path(tempfile.mkdtemp(prefix="cadmpeg-fuzz-seeds-", dir=work_dir))
    except OSError as error:
        print(f"error: {work_dir}: {error}", file=sys.stderr)
        return 2

    try:
        if build_generators() != 0:
            print("error: cargo build failed for the seed generators", file=sys.stderr)
            return 2
        failure = run_generators(generated)
        if failure is not None:
            print(f"error: {failure}", file=sys.stderr)
            return 2
        differences = compare_trees(COMMITTED_SEEDS, generated, AUTHORED)
    except OSError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    if differences:
        for difference in differences:
            print(difference)
        print(
            f"check-fuzz-seeds: {len(differences)} difference(s); "
            f"the generator run is in {generated}",
            file=sys.stderr,
        )
        return 1

    count = len(relative_files(COMMITTED_SEEDS)) - len(AUTHORED)
    shutil.rmtree(generated, ignore_errors=True)
    print(
        f"check-fuzz-seeds: {count} seed file(s) reproduce from the generators, "
        f"{len(AUTHORED)} hand-authored"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
