#!/usr/bin/env python3
"""Materialize source-less FCStd outputs for native-application acceptance."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile


def prepare(cadmpeg: Path, fixtures: Path, output: Path) -> int:
    output.mkdir(parents=True, exist_ok=True)
    sources = sorted(fixtures.glob("*.FCStd"))
    if len(sources) != 12:
        raise RuntimeError(f"expected 12 public FCStd fixtures, found {len(sources)}")
    with tempfile.TemporaryDirectory(prefix="cadmpeg-fcstd-synth-") as temporary:
        temporary = Path(temporary)
        for source in sources:
            document = temporary / f"{source.stem}.cadir.json"
            subprocess.run([str(cadmpeg), "dump", str(source), "-o", str(document)], check=True)
            # A CADIR document without fidelity must take source-less synthesis.
            for sidecar in temporary.glob("*.fidelity.json"):
                sidecar.unlink()
            report = output / f"{source.stem}.report.json"
            subprocess.run([
                str(cadmpeg), "convert", str(document), "-o", str(output / source.name),
                "--to", "fcstd:schema-4", "--allow-empty", "--report", str(report),
            ], check=True)
            value = json.loads(report.read_text())
            export = value.get("export") if isinstance(value, dict) else None
            write_path = export.get("write_path") if isinstance(export, dict) else None
            if (not isinstance(value, dict) or not isinstance(write_path, dict)
                    or value.get("ir_version") != "8"
                    or value.get("status") != "ok"
                    or value.get("command") != "convert"
                    or write_path.get("path") != "synthesized"):
                raise RuntimeError(f"{source.name}: acceptance input did not have a successful wire-v8 synthesized export report")
            target = output / source.name
            if not target.is_file() or target.stat().st_size == 0:
                raise RuntimeError(f"{source.name}: synthesis did not produce a nonempty FCStd output")
    return len(sources)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cadmpeg", required=True, type=Path)
    parser.add_argument("--fixtures", type=Path, default=Path("corpus/freecad_fcstd/fixtures"))
    parser.add_argument("--output-dir", required=True, type=Path)
    arguments = parser.parse_args()
    print(f"materialized {prepare(arguments.cadmpeg, arguments.fixtures, arguments.output_dir)} synthesized FCStd outputs")


if __name__ == "__main__":
    main()
