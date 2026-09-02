#!/usr/bin/env python3
"""Run the bounded IGES dump, check, and optional round-trip gate.

Each input receives an independent timeout for every command. An initial dump
exit is accepted only when it is exit code 1 and the command wrote a
schema-versioned decode refusal report. A timeout, crash, launcher failure,
unclassified dump exit, missing output, check failure, conversion failure, or
generated-file check failure fails the gate. ``--roundtrip`` adds conversion
to IGES 5.3 and dump plus check of the generated file.

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


def load_decode_refusal(report_path: Path) -> tuple[dict[str, object] | None, str | None]:
    """Read and validate the v8 refusal envelope emitted by ``dump``."""

    try:
        payload = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return None, f"decode refusal report is unavailable or invalid: {error}"
    if not isinstance(payload, dict):
        return None, "decode refusal report is not a JSON object"
    if payload.get("schema_version") != 8:
        return None, "decode refusal report does not use schema_version 8"
    if payload.get("command") != "dump":
        return None, "decode refusal report command is not dump"
    if payload.get("status") != "refused":
        return None, "decode refusal report status is not refused"
    refusal = payload.get("refusal")
    if not isinstance(refusal, dict):
        return None, "decode refusal report has no refusal object"
    if refusal.get("stage") != "decode":
        return None, "decode refusal report refusal stage is not decode"
    for field in ("code", "message"):
        value = refusal.get(field)
        if not isinstance(value, str) or not value:
            return None, f"decode refusal report refusal.{field} is empty or not text"
    return refusal, None


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
        dump_stderr = temporary / "dump.stderr"
        dump_report = temporary / "dump.report.json"
        dump_command = [
            cadmpeg,
            "dump",
            str(path),
            "--limits",
            limits,
            "--output",
            str(cadir),
            "--force",
            "--report",
            str(dump_report),
        ]
        dump = run_command(dump_command, timeout, dump_stderr)
        result: dict[str, object] = {
            "filename": relative_name,
            "gate_pass": False,
            "dump": asdict(dump),
        }

        if dump.status != "exited":
            result["status"] = f"dump_{dump.status}"
            return result
        if dump.returncode != 0:
            refusal, report_error = load_decode_refusal(dump_report)
            if dump.returncode == 1 and refusal is not None:
                result["gate_pass"] = True
                result["status"] = "dump_refused"
                result["refusal"] = refusal
            else:
                result["status"] = (
                    "dump_unclassified_refusal"
                    if dump.returncode == 1
                    else "dump_error"
                )
                if report_error is not None:
                    result["classification_error"] = report_error
            return result
        if not cadir.is_file() or cadir.stat().st_size == 0:
            result["status"] = "dump_missing_output"
            return result

        check_stderr = temporary / "check.stderr"
        check_command = [cadmpeg, "check", str(cadir), "--limits", limits]
        check = run_command(check_command, timeout, check_stderr)
        result["check"] = asdict(check)
        if check.status != "exited":
            result["status"] = f"check_{check.status}"
            return result
        if check.returncode != 0:
            result["status"] = "check_error"
            return result

        if roundtrip:
            generated = temporary / "roundtrip.igs"
            convert_stderr = temporary / "convert.stderr"
            convert_command = [
                cadmpeg,
                "convert",
                str(cadir),
                "--to",
                "iges:5.3-fixed-ascii",
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

            redumped = temporary / "redumped.cadir.json"
            redump_stderr = temporary / "redump.stderr"
            redump_report = temporary / "redump.report.json"
            redump_command = [
                cadmpeg,
                "dump",
                str(generated),
                "--limits",
                limits,
                "--output",
                str(redumped),
                "--force",
                "--report",
                str(redump_report),
            ]
            redump = run_command(redump_command, timeout, redump_stderr)
            result["redump"] = asdict(redump)
            if redump.status != "exited":
                result["status"] = f"redump_{redump.status}"
                return result
            if redump.returncode != 0:
                result["status"] = "redump_error"
                return result
            if not redumped.is_file() or redumped.stat().st_size == 0:
                result["status"] = "redump_missing_output"
                return result

            recheck_stderr = temporary / "recheck.stderr"
            recheck_command = [
                cadmpeg,
                "check",
                str(redumped),
                "--limits",
                limits,
            ]
            recheck = run_command(recheck_command, timeout, recheck_stderr)
            result["recheck"] = asdict(recheck)
            if recheck.status != "exited":
                result["status"] = f"recheck_{recheck.status}"
                return result
            if recheck.returncode != 0:
                result["status"] = "recheck_error"
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
        help="convert each successful dump to IGES 5.3 and dump and check it again",
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
