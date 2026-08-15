#!/usr/bin/env python3
"""Materialize the IGES golden outputs used by the independent FreeCAD gate."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def parse_args() -> argparse.Namespace:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument(
        "--expectations",
        type=Path,
        default=root / "scripts/iges-freecad-expectations.json",
    )
    parser.add_argument(
        "--golden-dir",
        type=Path,
        default=root / "crates/cadmpeg-codec-iges/tests/golden/encode",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    manifest = json.loads(args.expectations.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or manifest.get("version") != 1:
        raise SystemExit(f"{args.expectations}: expected JSON object with version 1")
    files = manifest.get("files")
    if not isinstance(files, dict) or not all(
        isinstance(filename, str) for filename in files
    ):
        raise SystemExit(f"{args.expectations}: files must be an object")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    for filename in sorted(files):
        output_path = Path(filename)
        if (
            output_path.name != filename
            or output_path.suffix.lower() not in {".igs", ".iges"}
        ):
            raise SystemExit(f"{args.expectations}: invalid output filename: {filename}")
        golden_path = args.golden_dir / f"{output_path.stem}.json"
        if not golden_path.is_file():
            raise SystemExit(f"missing golden output: {golden_path}")
        golden = json.loads(golden_path.read_text(encoding="utf-8"))
        output = golden.get("output") if isinstance(golden, dict) else None
        if not isinstance(output, str):
            raise SystemExit(f"{golden_path}: output is not a string")
        target = args.output_dir / output_path
        if target.exists() and target.read_text(encoding="utf-8") != output:
            raise SystemExit(f"refusing to overwrite different file: {target}")
        target.write_text(output, encoding="utf-8")
        print(target)


if __name__ == "__main__":
    main()
