#!/usr/bin/env python3
"""Materialize IGES golden outputs for the independent FreeCAD gate."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def parse_args() -> argparse.Namespace:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, required=True)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument(
        "--expectations",
        type=Path,
        default=root / "scripts/iges-freecad-expectations.json",
        help="manifest selecting the exact geometry profile (the default)",
    )
    selection.add_argument(
        "--all-writable",
        action="store_true",
        help="materialize every golden with a successful writer output",
    )
    parser.add_argument(
        "--golden-dir",
        type=Path,
        default=root / "crates/cadmpeg-codec-iges/tests/golden/encode",
    )
    return parser.parse_args()


def validate_filename(filename: str, source: Path) -> Path:
    output_path = Path(filename)
    if (
        output_path.name != filename
        or output_path.suffix.lower() not in {".igs", ".iges"}
    ):
        raise SystemExit(f"{source}: invalid output filename: {filename}")
    return output_path


def golden_output(path: Path) -> str:
    golden = json.loads(path.read_text(encoding="utf-8"))
    output = golden.get("output") if isinstance(golden, dict) else None
    if not isinstance(output, str):
        raise SystemExit(f"{path}: output is not a string")
    return output


def manifest_outputs(expectations: Path, golden_dir: Path) -> list[tuple[Path, str]]:
    manifest = json.loads(expectations.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or manifest.get("version") != 1:
        raise SystemExit(f"{expectations}: expected JSON object with version 1")
    files = manifest.get("files")
    if not isinstance(files, dict) or not all(
        isinstance(filename, str) for filename in files
    ):
        raise SystemExit(f"{expectations}: files must be an object")

    outputs = []
    for filename in sorted(files):
        output_path = validate_filename(filename, expectations)
        golden_path = golden_dir / f"{output_path.stem}.json"
        if not golden_path.is_file():
            raise SystemExit(f"missing golden output: {golden_path}")
        outputs.append((output_path, golden_output(golden_path)))
    return outputs


def all_writable_outputs(golden_dir: Path) -> list[tuple[Path, str]]:
    outputs = []
    for golden_path in sorted(golden_dir.glob("*.json")):
        output = json.loads(golden_path.read_text(encoding="utf-8"))
        if not isinstance(output, dict) or not isinstance(output.get("output"), str):
            continue
        outputs.append(
            (validate_filename(f"{golden_path.stem}.igs", golden_path), output["output"])
        )
    if not outputs:
        raise SystemExit(f"{golden_dir}: no successful writer outputs found")
    return outputs


def materialize(output_dir: Path, outputs: list[tuple[Path, str]]) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    for output_path, output in outputs:
        target = output_dir / output_path
        if target.exists() and target.read_text(encoding="utf-8") != output:
            raise SystemExit(f"refusing to overwrite different file: {target}")
        target.write_text(output, encoding="utf-8")
        print(target)


def main() -> None:
    args = parse_args()
    outputs = (
        all_writable_outputs(args.golden_dir)
        if args.all_writable
        else manifest_outputs(args.expectations, args.golden_dir)
    )
    materialize(args.output_dir, outputs)


if __name__ == "__main__":
    main()
