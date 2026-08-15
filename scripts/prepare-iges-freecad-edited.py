#!/usr/bin/env python3
"""Create a deterministic edited IGES document for native-application checks."""

from __future__ import annotations

import argparse
import json
import math
import os
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Sequence


EDITED_POSITION = {"x": 11.0, "y": 22.0, "z": 33.0}


def parse_args() -> argparse.Namespace:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--cadmpeg",
        default=os.environ.get("CADMPEG_BIN", "cadmpeg"),
        help="cadmpeg executable (default: CADMPEG_BIN or cadmpeg)",
    )
    parser.add_argument(
        "--input",
        type=Path,
        default=root / "crates/cadmpeg-codec-iges/tests/golden/fixtures/point.igs",
        help="source fixture to decode before editing",
    )
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument(
        "--limits",
        choices=("desktop", "service"),
        default="service",
        help="cadmpeg resource profile (default: service)",
    )
    parser.add_argument(
        "--iges-target",
        choices=("5.1", "5.2", "5.3"),
        default="5.3",
        help="IGES target version (default: 5.3)",
    )
    parser.add_argument(
        "--output-name",
        default="edited_point.igs",
        help="output filename within --output-dir (default: edited_point.igs)",
    )
    return parser.parse_args()


def edit_point(document: dict[str, Any]) -> None:
    model = document.get("model")
    points = model.get("points") if isinstance(model, dict) else None
    if not isinstance(points, list) or len(points) != 1:
        raise SystemExit("edited IGES fixture must contain exactly one model point")
    point = points[0]
    if not isinstance(point, dict):
        raise SystemExit("edited IGES fixture point is not an object")
    position = point.get("position")
    if not isinstance(position, dict) or set(position) != {"x", "y", "z"}:
        raise SystemExit("edited IGES fixture point has no complete position")
    for coordinate in position.values():
        if isinstance(coordinate, bool) or not isinstance(coordinate, (int, float)):
            raise SystemExit("edited IGES fixture point position is not numeric")
        if not math.isfinite(float(coordinate)):
            raise SystemExit("edited IGES fixture point position is not finite")
    point["position"] = dict(EDITED_POSITION)


def run(command: Sequence[str], label: str) -> None:
    completed = subprocess.run(
        list(command),
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    if completed.returncode == 0:
        return
    detail = " ".join((completed.stderr or "").split())
    if len(detail) > 1024:
        detail = detail[-1024:]
    suffix = f": {detail}" if detail else ""
    raise SystemExit(f"{label} failed with exit code {completed.returncode}{suffix}")


def main() -> None:
    args = parse_args()
    input_path = args.input.expanduser().resolve()
    if not input_path.is_file():
        raise SystemExit(f"input fixture does not exist: {input_path}")
    output_name = Path(args.output_name)
    if output_name.name != args.output_name or output_name.suffix.lower() not in {
        ".igs",
        ".iges",
    }:
        raise SystemExit(f"invalid output filename: {args.output_name}")
    output_dir = args.output_dir.expanduser().resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="iges-edited-", dir=output_dir) as name:
        scratch = Path(name)
        decoded = scratch / "decoded.cadir.json"
        decode_report = scratch / "decode.report.json"
        run(
            [
                args.cadmpeg,
                "decode",
                str(input_path),
                "--limits",
                args.limits,
                "--output",
                str(decoded),
                "--report",
                str(decode_report),
                "--force",
            ],
            "IGES edit fixture decode",
        )
        try:
            document = json.loads(decoded.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise SystemExit(f"decoded edit fixture is not valid JSON: {error}") from error
        if not isinstance(document, dict):
            raise SystemExit("decoded edit fixture is not a JSON object")
        edit_point(document)
        edited = scratch / "edited.cadir.json"
        edited.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")

        output = output_dir / output_name
        convert_report = output_dir / f"{output.stem}.convert.report.json"
        run(
            [
                args.cadmpeg,
                "convert",
                str(edited),
                "--format",
                "iges",
                "--iges-target",
                args.iges_target,
                "--limits",
                args.limits,
                "--allow-empty",
                "--output",
                str(output),
                "--report",
                str(convert_report),
                "--force",
            ],
            "edited IGES conversion",
        )
    if not output.is_file() or output.stat().st_size == 0:
        raise SystemExit(f"edited IGES output is missing or empty: {output}")
    print(output)


if __name__ == "__main__":
    main()
