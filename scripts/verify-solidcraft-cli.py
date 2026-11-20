#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check that SolidCraft itself accepts the designs the cadmpeg writer emits.

The writer test `write_files_for_the_native_check` writes a source design and
its edited copy (parameter `w` from 20 mm to 35 mm), as a container with a
model cache and as plain JSON, and the same block generated from the model
without its source, in both encodings. This script opens each file with
`solidcraft-cli eval`, which rebuilds the model from the history, and checks:

* no feature or parameter reports an error;
* the unedited file evaluates to the source box (x extent 20 mm);
* a generated design evaluates to the source block (20 x 10 x 5 mm);
* the edited file follows the edit (x extent 35 mm) and its volume grows by
  the ratio of the extents, within tolerance, so the edit reached the model
  and the dropped cache was rebuilt.

Usage: python3 scripts/verify-solidcraft-cli.py [--cli PATH]
The CLI defaults to $SOLIDCRAFT_CLI, then to the SolidCraft checkout's
target/ci build under the user's home, then to the installed application.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CLIS = [
    Path.home() / "Dev" / "solidcraft" / "target" / "ci" / "solidcraft-cli.exe",
    Path("C:/Program Files/SolidCraft/solidcraft-cli.exe"),
]
DEFAULT_CLI = next((path for path in DEFAULT_CLIS if path.exists()), DEFAULT_CLIS[0])
# The container's block has holes whose volume does not scale with the
# extent, so its ratio sits a little over 35/20.
VOLUME_TOLERANCE = 0.1


def evaluate(cli: Path, design: Path, out: Path) -> dict:
    subprocess.run(
        [str(cli), "eval", str(design), "--json-out", str(out)],
        check=True,
        capture_output=True,
    )
    return json.loads(out.read_text(encoding="utf-8"))


def errors(result: dict) -> list:
    found = [p for p in result.get("params", []) if p.get("error")]
    found += [t for t in result.get("timeline", []) if t.get("error")]
    return found


def extent_x(result: dict) -> float:
    box = result["bodies"][0]["bbox"]
    return box["max"][0] - box["min"][0]


def volume(result: dict) -> float:
    return result["bodies"][0]["volume_mm3"]


def check(cli: Path, folder: Path, stem: str) -> list[str]:
    failures = []
    source = evaluate(cli, folder / f"source{stem}.sc", folder / f"source{stem}.json")
    edited = evaluate(cli, folder / f"edited{stem}.sc", folder / f"edited{stem}.json")
    for label, result in (("source", source), ("edited", edited)):
        if errors(result):
            failures.append(f"{stem or 'container'} {label}: errors {errors(result)}")
    if abs(extent_x(source) - 20.0) > 1e-6:
        failures.append(f"{stem or 'container'} source: x extent {extent_x(source)}")
    if abs(extent_x(edited) - 35.0) > 1e-6:
        failures.append(f"{stem or 'container'} edited: x extent {extent_x(edited)}")
    ratio = volume(edited) / volume(source)
    if abs(ratio - 1.75) > VOLUME_TOLERANCE:
        failures.append(f"{stem or 'container'}: volume ratio {ratio:.3f}")
    print(
        f"{stem or 'container'}: x {extent_x(source):g} -> {extent_x(edited):g} mm, "
        f"volume {volume(source):.1f} -> {volume(edited):.1f} mm3"
    )
    return failures


def check_generated(cli: Path, folder: Path, stem: str) -> list[str]:
    """A design generated without its source evaluates to the source box."""
    label = f"generated{stem or ' container'}"
    result = evaluate(cli, folder / f"generated{stem}.sc", folder / f"generated{stem}.json")
    failures = [f"{label}: errors {errors(result)}"] if errors(result) else []
    box = result["bodies"][0]["bbox"]
    size = [high - low for low, high in zip(box["min"], box["max"])]
    if any(abs(a - b) > 1e-6 for a, b in zip(size, [20.0, 10.0, 5.0])):
        failures.append(f"{label}: extent {size}")
    if abs(volume(result) - 1000.0) > 1e-6:
        failures.append(f"{label}: volume {volume(result)}")
    print(f"{label}: extent {size} mm, volume {volume(result):.1f} mm3")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--cli", type=Path)
    args = parser.parse_args()
    cli = args.cli or Path(os.environ.get("SOLIDCRAFT_CLI", DEFAULT_CLI))
    if not cli.exists():
        print(f"solidcraft-cli not found at {cli}", file=sys.stderr)
        return 2
    with tempfile.TemporaryDirectory() as folder:
        folder = Path(folder)
        env = dict(os.environ, SOLIDCRAFT_WRITE_DIR=str(folder))
        subprocess.run(
            [
                "cargo", "test", "-q", "-p", "cadmpeg-codec-solidcraft",
                "write_files_for_the_native_check", "--", "--ignored",
            ],
            cwd=ROOT,
            env=env,
            check=True,
        )
        failures = (
            check(cli, folder, "")
            + check(cli, folder, ".json")
            + check_generated(cli, folder, "")
            + check_generated(cli, folder, ".json")
        )
    for failure in failures:
        print(f"FAIL {failure}", file=sys.stderr)
    print("solidcraft native check:", "failed" if failures else "ok")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
