#!/usr/bin/env python3
"""Verify cadmpeg-written 3DM files with the independent rhino3dm reader.

Run with plain Python (rhino3dm is McNeel's standalone reader, independent
of cadmpeg), for example::

    CADMPEG_RHINO_INPUT_DIR=out \\
        CADMPEG_RHINO_MANIFEST=out/manifest.json \\
        CADMPEG_RHINO_REPORT=out/rhino-report.json \\
        python3 scripts/verify-rhino-rhino3dm.py

Every `*.3dm` file under the input directory (including `damaged/`) must
open in rhino3dm. When `CADMPEG_RHINO_MANIFEST` names the contract suite's
`manifest.json`, the file set must agree exactly, and each entry carrying
counts must match: object count is the number of objects in the rhino3dm
object table, layer count the number of layers. An entry carrying `error`
needs only a successful open; the cadmpeg re-decode counts do not exist
for it.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

sys.dont_write_bytecode = True

try:
    import rhino3dm
except ImportError as error:
    print(f"rhino3dm is required: {error}", file=sys.stderr)
    raise SystemExit(2)


def verify(path: Path) -> dict[str, int]:
    model = rhino3dm.File3dm.Read(str(path))
    if model is None:
        raise ValueError("rhino3dm refused to open the file")
    return {
        "objects": len(list(model.Objects)),
        "layers": len(list(model.Layers)),
    }


def load_manifest(path: Path) -> dict:
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or manifest.get("version") != 1:
        raise ValueError(f"{path}: expected JSON object with version 1")
    files = manifest.get("files")
    if not isinstance(files, dict) or not all(
        isinstance(name, str) and isinstance(entry, dict)
        for name, entry in files.items()
    ):
        raise ValueError(f"{path}: files must map names to objects")
    return files


def main() -> None:
    input_directory = Path(os.environ["CADMPEG_RHINO_INPUT_DIR"])
    files = sorted(
        (
            path
            for path in input_directory.rglob("*")
            if path.is_file() and path.suffix.lower() == ".3dm"
        ),
        key=lambda path: path.relative_to(input_directory).as_posix(),
    )
    if not files:
        print(f"no 3DM files found in {input_directory}", file=sys.stderr)
        raise SystemExit(1)

    manifest_path = os.environ.get("CADMPEG_RHINO_MANIFEST")
    manifest = load_manifest(Path(manifest_path)) if manifest_path else None
    if manifest is not None:
        actual = {path.relative_to(input_directory).as_posix() for path in files}
        expected = set(manifest)
        missing = sorted(actual - expected)
        extra = sorted(expected - actual)
        if missing or extra:
            details = []
            if missing:
                details.append(f"files without a manifest entry: {', '.join(missing)}")
            if extra:
                details.append(f"manifest entries without a file: {', '.join(extra)}")
            raise ValueError("; ".join(details))

    results = []
    failures = []
    for path in files:
        filename = path.relative_to(input_directory).as_posix()
        entry = manifest.get(filename) if manifest is not None else None
        try:
            observed = verify(path)
        except Exception as error:
            failures.append({"filename": filename, "error": str(error)})
            print(f"FAIL {path}: {error}", file=sys.stderr)
            continue
        errors = []
        if entry is not None:
            if "error" in entry:
                pass
            else:
                for field in ("objects", "layers"):
                    expected = entry.get(field)
                    if not isinstance(expected, int) or isinstance(expected, bool):
                        errors.append(f"manifest {field} for {filename} is not an integer")
                    elif observed[field] != expected:
                        errors.append(
                            f"{field}: expected {expected}, "
                            f"observed {observed[field]}"
                        )
            unknown = set(entry) - {"objects", "layers", "error"}
            errors.extend(f"unknown manifest field: {field}" for field in sorted(unknown))
        if errors:
            failures.append(
                {"filename": filename, "error": "; ".join(errors), "observed": observed}
            )
            print(f"FAIL {path}: {'; '.join(errors)}", file=sys.stderr)
        else:
            results.append({"filename": filename, **observed})
    report = {"manifest": manifest_path, "failures": failures, "files": results}
    report_json = json.dumps(report, indent=2, sort_keys=True) + "\n"
    report_path = os.environ.get("CADMPEG_RHINO_REPORT")
    if report_path:
        Path(report_path).write_text(report_json, encoding="utf-8")
    print(report_json, end="", flush=True)
    if failures:
        raise SystemExit(1)


main()
