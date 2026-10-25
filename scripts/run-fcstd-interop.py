#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run authored FCStd write, geometry, mesh-placement, and STEP native gates."""

import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess


ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cadmpeg", type=Path, required=True)
    parser.add_argument("--writer-example", type=Path, required=True)
    parser.add_argument("--source-less-example", type=Path, required=True)
    parser.add_argument("--freecad-command", required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    args = parser.parse_args()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    reference, output = work / "reference", work / "output"
    cadir, step = work / "cadir", work / "step"
    for directory in (reference, output, cadir, step):
        directory.mkdir(exist_ok=True)
    environment = {
        **os.environ,
        "QT_QPA_PLATFORM": "offscreen",
        "CADMPEG_FCSTD_WORK_DIR": str(work),
        "CADMPEG_FCSTD_INPUT_DIR": str(output),
        "CADMPEG_FCSTD_OUTPUT_DIR": str(work / "native-resave"),
        "CADMPEG_FCSTD_REFERENCE_DIR": str(reference),
        "CADMPEG_FCSTD_EXPECTATIONS": str(reference / "expectations.json"),
        "CADMPEG_FCSTD_REQUIRE_NATIVE_RESAVE": "1",
        "CADMPEG_FCSTD_REQUIRE_SEMANTIC_EQUIVALENCE": "1",
        "CADMPEG_FCSTD_REQUIRE_APPEARANCE": "1",
        "CADMPEG_FCSTD_REPORT": str(work / "write-report.json"),
    }
    command = shlex.split(args.freecad_command)

    def run(arguments, name):
        with (work / f"{name}.log").open("w") as log:
            subprocess.run([str(arg) for arg in arguments], check=True, timeout=120,
                           env=environment, stdout=log, stderr=subprocess.STDOUT)

    for report in ("write-report.json", "projection-report.json"):
        (work / report).unlink(missing_ok=True)
    (reference / "expectations.json").unlink(missing_ok=True)
    run([*command, ROOT / "corpus/freecad_fcstd/author_interop_cases.py"], "author")
    if not (reference / "expectations.json").is_file():
        raise RuntimeError("native authoring failed; see author.log")
    for name in ("ZFirst", "ASecond"):
        run([args.cadmpeg.resolve(), "inspect", "extract",
             reference / "restoration_two_boxes.FCStd", f"{name}.Shape.brp",
             "-o", reference / f"{name}.brp", "--force"], f"extract-{name}")
    run([args.writer_example.resolve(), reference, output], "write")
    run([*command, ROOT / "tools/validate_fcstd_interop.py"], "validate-write")
    report = json.loads((work / "write-report.json").read_text())
    files = report["files"]
    if report["failures"] or len(files) != 9 or not all(
        row["document_opened"] and row["semantic_equivalence_accepted"]
        and row["native_resave_accepted"] and row["appearance_compared"]
        for row in files
    ):
        raise RuntimeError("native write equivalence failed; see write-report.json")
    for name in ("restoration_two_boxes", "parametric_box", "filleted_box", "spline_domains"):
        source = reference / f"{name}.FCStd"
        rewritten = output / f"{name}.FCStd"
        run([args.cadmpeg.resolve(), "check", source], f"check-{name}")
        run([args.cadmpeg.resolve(), "measure", source, "--json", "--report",
             work / f"{name}.measure.json", "--force"], f"measure-{name}")
        run([args.cadmpeg.resolve(), "diff", source, rewritten, "--geometry", "--report",
             work / f"{name}.geometry-diff.json", "--force"], f"diff-{name}")
    for name in ("placed_mesh", "nested_mesh", "linked_mesh"):
        source = reference / f"{name}.FCStd"
        run([args.cadmpeg.resolve(), "check", source], f"check-{name}")
        run([args.cadmpeg.resolve(), "dump", source, "-o", cadir / f"{name}.json", "--force"],
            f"dump-{name}")
    for dialect in ("ap203-e1", "ap214", "ap242-e3"):
        destination = step / f"filleted_box_{dialect}.step"
        run([args.cadmpeg.resolve(), "convert", reference / "filleted_box.FCStd",
             "--to", f"step:{dialect}", "-o", destination, "--force"], f"export-{dialect}")
        run([args.cadmpeg.resolve(), "check", destination], f"check-{dialect}")
    run([args.source_less_example.resolve(), work / "source_less.FCStd"], "source-less")
    run([*command, ROOT / "tools/validate_fcstd_projection.py"], "validate-projection")
    projection = json.loads((work / "projection-report.json").read_text())
    if not projection["accepted"]:
        raise RuntimeError("native projection failed; see projection-report.json")
    print(json.dumps({"native_writes": len(files), **projection}, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
