#!/usr/bin/env python3
"""Build and run the independent openNURBS Rhino transfer witness."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import tempfile


def run(command: list[str], cwd: Path, env: dict[str, str] | None = None) -> None:
    subprocess.run(command, cwd=cwd, env=env, check=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("opennurbs", type=Path)
    parser.add_argument("--jobs", type=int, default=2)
    args = parser.parse_args()
    root = args.opennurbs.resolve()
    repo = Path(__file__).resolve().parents[1]
    run(["make", "-s", f"-j{args.jobs}", "example_read/example_read"], root)
    with tempfile.TemporaryDirectory(prefix="cadmpeg-rhino-witness-") as temporary:
        generated = Path(temporary)
        generator = generated / "rhino-witness"
        run(
            [
                "c++",
                "-std=c++14",
                f"-I{root}",
                str(repo / "tools/rhino_opennurbs_witness.cpp"),
                f"-L{root}",
                "-lopennurbs_public",
                "-lm",
                "-o",
                str(generator),
            ],
            repo,
        )
        for version in (50, 60, 70, 80):
            run([str(generator), str(version), str(generated / f"witness-v{version}.3dm")], repo)
        env = os.environ.copy()
        env["OPENNURBS_ROOT"] = str(root)
        env["OPENNURBS_SYNTH_DIR"] = str(generated)
        run([
            "cargo",
            "test",
            "-q",
            "-p",
            "cadmpeg-codec-rhino",
            "--lib",
            "--",
            "external_transfer_tests::opennurbs_object_walk_and_transfer_floor",
            "--ignored",
        ], repo, env)


if __name__ == "__main__":
    main()
