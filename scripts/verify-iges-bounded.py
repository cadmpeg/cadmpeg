#!/usr/bin/env python3
"""Run the bounded IGES decode, validation, and optional round-trip gate.

Each input receives an independent timeout for every command. A non-zero
initial decode exit is a terminal, classified refusal. A timeout, crash,
launcher failure, missing output, validation failure, conversion failure, or
generated-file validation failure fails the gate. ``--roundtrip`` adds
conversion to IGES 5.3 and decode plus validation of the generated file.

Temporary decoded artifacts are created below ``$HOME/side2/tmp/iges-l9`` by
default. Set ``--scratch`` when a CI runner needs another dedicated scratch
directory.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Sequence


IGES_SUFFIXES = {".igs", ".iges"}
DEFAULT_SCRATCH = Path.home() / "side2" / "tmp" / "iges-l9"


@dataclass(frozen=True)
class CommandResult:
    command: list[str]
    status: str
    elapsed_ms: int
    returncode: int | None
    detail: str | None


def stderr_tail(path: Path) -> str | None:
    data = path.read_bytes()
    if not data:
        return None
    return " ".join(data[-1024:].decode("utf-8", errors="replace").split())


def run_command(command: Sequence[str], timeout: float, stderr_path: Path) -> CommandResult:
    started = time.monotonic()
    status = "exited"
    returncode: int | None
    with stderr_path.open("wb") as stderr:
        try:
            completed = subprocess.run(
                list(command),
                stdout=subprocess.DEVNULL,
                stderr=stderr,
                check=False,
                timeout=timeout,
            )
        except subprocess.TimeoutExpired:
            status = "timeout"
            returncode = None
        except OSError as error:
            status = "launcher_error"
            returncode = None
            detail = str(error)
        else:
            returncode = completed.returncode
            if returncode < 0:
                status = "crash"
            detail = None

    if status == "launcher_error":
        elapsed_ms = round((time.monotonic() - started) * 1000)
        return CommandResult(list(command), status, elapsed_ms, returncode, detail)

    detail = stderr_tail(stderr_path)
    if status == "timeout":
        elapsed_ms = round((time.monotonic() - started) * 1000)
        return CommandResult(list(command), status, elapsed_ms, returncode, detail)

    elapsed_ms = round((time.monotonic() - started) * 1000)
    return CommandResult(list(command), status, elapsed_ms, returncode, detail)


def input_files(root: Path) -> list[Path]:
    return sorted(
        (
            path
            for path in root.rglob("*")
            if path.is_file() and path.suffix.lower() in IGES_SUFFIXES
        ),
        key=lambda path: path.relative_to(root).as_posix(),
    )


def verify_file(
    path: Path,
    root: Path,
    cadmpeg: str,
    limits: str,
    timeout: float,
    scratch: Path,
    roundtrip: bool,
) -> dict[str, object]:
    relative_name = path.relative_to(root).as_posix()
    with tempfile.TemporaryDirectory(prefix="iges-bounded-", dir=scratch) as directory:
        temporary = Path(directory)
        cadir = temporary / "decoded.cadir.json"
        decode_stderr = temporary / "decode.stderr"
        decode_command = [
            cadmpeg,
            "decode",
            str(path),
            "--limits",
            limits,
            "--output",
            str(cadir),
            "--force",
        ]
        decode = run_command(decode_command, timeout, decode_stderr)
        result: dict[str, object] = {
            "filename": relative_name,
            "gate_pass": False,
            "decode": asdict(decode),
        }

        if decode.status != "exited":
            result["status"] = f"decode_{decode.status}"
            return result
        if decode.returncode != 0:
            result["gate_pass"] = True
            result["status"] = "decode_refused"
            return result
        if not cadir.is_file() or cadir.stat().st_size == 0:
            result["status"] = "decode_missing_output"
            return result

        validate_stderr = temporary / "validate.stderr"
        validate_command = [cadmpeg, "validate", str(cadir), "--limits", limits]
        validate = run_command(validate_command, timeout, validate_stderr)
        result["validate"] = asdict(validate)
        if validate.status != "exited":
            result["status"] = f"validation_{validate.status}"
            return result
        if validate.returncode != 0:
            result["status"] = "validation_error"
            return result

        if roundtrip:
            generated = temporary / "roundtrip.igs"
            convert_stderr = temporary / "convert.stderr"
            convert_command = [
                cadmpeg,
                "convert",
                str(cadir),
                "--format",
                "iges",
                "--iges-target",
                "5.3",
                "--limits",
                limits,
                "--allow-empty",
                "--output",
                str(generated),
                "--force",
            ]
            convert = run_command(convert_command, timeout, convert_stderr)
            result["convert"] = asdict(convert)
            if convert.status != "exited":
                result["status"] = f"convert_{convert.status}"
                return result
            if convert.returncode != 0:
                result["status"] = "convert_error"
                return result
            if not generated.is_file() or generated.stat().st_size == 0:
                result["status"] = "convert_missing_output"
                return result

            redecoded = temporary / "redecoded.cadir.json"
            redecode_stderr = temporary / "redecode.stderr"
            redecode_command = [
                cadmpeg,
                "decode",
                str(generated),
                "--limits",
                limits,
                "--output",
                str(redecoded),
                "--force",
            ]
            redecode = run_command(redecode_command, timeout, redecode_stderr)
            result["redecode"] = asdict(redecode)
            if redecode.status != "exited":
                result["status"] = f"redecode_{redecode.status}"
                return result
            if redecode.returncode != 0:
                result["status"] = "redecode_error"
                return result
            if not redecoded.is_file() or redecoded.stat().st_size == 0:
                result["status"] = "redecode_missing_output"
                return result

            revalidate_stderr = temporary / "revalidate.stderr"
            revalidate_command = [
                cadmpeg,
                "validate",
                str(redecoded),
                "--limits",
                limits,
            ]
            revalidate = run_command(revalidate_command, timeout, revalidate_stderr)
            result["revalidate"] = asdict(revalidate)
            if revalidate.status != "exited":
                result["status"] = f"revalidation_{revalidate.status}"
                return result
            if revalidate.returncode != 0:
                result["status"] = "revalidation_error"
                return result

        result["gate_pass"] = True
        result["status"] = "success"
        return result


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input_dir", type=Path, help="directory containing .igs or .iges files")
    parser.add_argument(
        "--cadmpeg",
        default=os.environ.get("CADMPEG_BIN", "cadmpeg"),
        help="cadmpeg executable (default: CADMPEG_BIN or cadmpeg)",
    )
    parser.add_argument(
        "--limits",
        choices=("desktop", "service"),
        default="service",
        help="cadmpeg resource profile (default: service)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=30.0,
        help="independent timeout in seconds for each command (default: 30)",
    )
    parser.add_argument(
        "--scratch",
        type=Path,
        default=Path(os.environ.get("CADMPEG_IGES_SCRATCH", DEFAULT_SCRATCH)),
        help="dedicated temporary-artifact directory",
    )
    parser.add_argument(
        "--report",
        type=Path,
        help="write the JSON report to this path instead of standard output",
    )
    parser.add_argument(
        "--roundtrip",
        action="store_true",
        help="convert each successful decode to IGES 5.3 and re-decode and validate it",
    )
    args = parser.parse_args(argv)
    if args.timeout <= 0:
        parser.error("--timeout must be greater than zero")
    return args


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    root = args.input_dir.expanduser().resolve()
    if not root.is_dir():
        print(f"input directory does not exist: {root}", file=sys.stderr)
        return 2

    files = input_files(root)
    if not files:
        print(f"no IGES files found in {root}", file=sys.stderr)
        return 2

    scratch = args.scratch.expanduser().resolve()
    scratch.mkdir(parents=True, exist_ok=True)
    results = [
        verify_file(
            path,
            root,
            args.cadmpeg,
            args.limits,
            args.timeout,
            scratch,
            args.roundtrip,
        )
        for path in files
    ]
    failures = [result for result in results if not result["gate_pass"]]
    report = {
        "status": "passed" if not failures else "failed",
        "file_count": len(results),
        "failure_count": len(failures),
        "files": results,
    }
    report_json = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.report is not None:
        report_path = args.report.expanduser()
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(report_json, encoding="utf-8")
    else:
        print(report_json, end="")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
