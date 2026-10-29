#!/usr/bin/env python3
"""Tests for the structured refusal contract of the IGES bounded gate."""

from __future__ import annotations

import importlib.util
import json
import stat
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("verify-iges-bounded.py")
SPEC = importlib.util.spec_from_file_location("verify_iges_bounded", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class DecodeRefusalReportTests(unittest.TestCase):
    def setUp(self) -> None:
        MODULE.DEFAULT_SCRATCH.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=MODULE.DEFAULT_SCRATCH)
        self.path = Path(self.directory.name) / "dump.report.json"

    def tearDown(self) -> None:
        self.directory.cleanup()

    def write(self, payload: object) -> None:
        self.path.write_text(json.dumps(payload), encoding="utf-8")

    def fake_cadmpeg(self, source: str) -> Path:
        executable = Path(self.directory.name) / "fake-cadmpeg"
        executable.write_text("#!/usr/bin/env python3\n" + source, encoding="utf-8")
        executable.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
        return executable

    def test_accepts_v8_decode_refusal(self) -> None:
        refusal = {
            "stage": "decode",
            "code": "decode_failed",
            "message": "decode failed: malformed container",
        }
        self.write(
            {
                "ir_version": "8",
                "command": "dump",
                "status": "refused",
                "refusal": refusal,
            }
        )

        actual, error = MODULE.load_decode_refusal(self.path)

        self.assertIsNone(error)
        self.assertEqual(actual, refusal)

    def test_rejects_success_report_as_refusal(self) -> None:
        self.write(
            {
                "ir_version": "8",
                "command": "dump",
                "status": "ok",
                "refusal": None,
            }
        )

        actual, error = MODULE.load_decode_refusal(self.path)

        self.assertIsNone(actual)
        self.assertEqual(error, "decode refusal report status is not refused")

    def test_rejects_unsupported_refusal_version(self) -> None:
        for version in ("5", "6", "7", "9", "10", "11", "37"):
            with self.subTest(version=version):
                self.write(
                    {
                        "ir_version": version,
                        "command": "dump",
                        "status": "refused",
                        "refusal": {
                            "stage": "decode",
                            "code": "decode_failed",
                            "message": "old report",
                        },
                    }
                )

                actual, error = MODULE.load_decode_refusal(self.path)

                self.assertIsNone(actual)
                self.assertEqual(error, 'decode refusal report does not use ir_version "8"')

    def test_gate_rejects_unclassified_decode_exit(self) -> None:
        input_path = Path(self.directory.name) / "input.igs"
        input_path.write_bytes(b"input")
        executable = self.fake_cadmpeg("import sys\nsys.exit(2)\n")

        result = MODULE.verify_file(
            input_path,
            Path(self.directory.name),
            str(executable),
            "service",
            2.0,
            MODULE.DEFAULT_SCRATCH,
            False,
        )

        self.assertFalse(result["gate_pass"])
        self.assertEqual(result["status"], "dump_error")

    def test_gate_accepts_only_structured_decode_refusal(self) -> None:
        input_path = Path(self.directory.name) / "input.igs"
        input_path.write_bytes(b"input")
        executable = self.fake_cadmpeg(
            """import json
import pathlib
import sys

report = pathlib.Path(sys.argv[sys.argv.index('--report') + 1])
assert sys.argv[1] == 'dump'
report.write_text(json.dumps({
    'ir_version': '8',
    'command': 'dump',
    'status': 'refused',
    'refusal': {
        'stage': 'decode',
        'code': 'decode_failed',
        'message': 'synthetic refusal',
    },
}))
sys.exit(1)
"""
        )

        result = MODULE.verify_file(
            input_path,
            Path(self.directory.name),
            str(executable),
            "service",
            2.0,
            MODULE.DEFAULT_SCRATCH,
            False,
        )

        self.assertTrue(result["gate_pass"])
        self.assertEqual(result["status"], "dump_refused")


class DamagedFixtureTests(unittest.TestCase):
    def test_discovers_regular_and_damaged_inputs_in_relative_path_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "damaged/deep").mkdir(parents=True)
            for name in ("z.IGES", "a.igs", "damaged/a.igs", "damaged/deep/cut.iges", "damaged/ignore.txt"):
                (root / name).write_bytes(b"fixture")
            (root / "directory.igs").mkdir()
            self.assertEqual([path.relative_to(root).as_posix() for path in MODULE.input_files(root)], ["a.igs", "damaged/a.igs", "damaged/deep/cut.iges", "z.IGES"])

    def test_bounded_gate_keeps_roundtrip_checks_for_damaged_successes_and_classifies_refusals(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixtures = root / "fixtures"
            (fixtures / "damaged").mkdir(parents=True)
            for name in ("regular.igs", "damaged/partial.igs", "damaged/cut.iges"):
                (fixtures / name).write_bytes(b"fixture")
            calls = root / "calls.jsonl"
            executable = root / "fake-cadmpeg"
            executable.write_text("#!/usr/bin/env python3\n" + f"CALLS = {str(calls)!r}\n" + r'''import json
import pathlib
import sys
with open(CALLS, "a") as log:
    log.write(json.dumps(sys.argv[1:]) + "\n")
command = sys.argv[1]
if command == "check":
    sys.exit(0)
output = pathlib.Path(sys.argv[sys.argv.index("--output") + 1])
if command == "dump" and pathlib.Path(sys.argv[2]).name == "cut.iges":
    report = pathlib.Path(sys.argv[sys.argv.index("--report") + 1])
    report.write_text(json.dumps({"ir_version": "8", "command": "dump", "status": "refused", "refusal": {"stage": "decode", "code": "decode_failed", "message": "cut source"}}))
    sys.exit(1)
output.write_text("{}")
''')
            executable.chmod(0o755)
            report = root / "report.json"
            self.assertEqual(MODULE.main([str(fixtures), "--cadmpeg", str(executable), "--scratch", str(root / "scratch"), "--timeout", "2", "--roundtrip", "--report", str(report)]), 0)
            payload = json.loads(report.read_text())
            self.assertEqual(payload["file_count"], 3)
            self.assertEqual(payload["failure_count"], 0)
            files = {row["filename"]: row for row in payload["files"]}
            self.assertEqual(set(files), {"regular.igs", "damaged/partial.igs", "damaged/cut.iges"})
            for name in ("regular.igs", "damaged/partial.igs"):
                self.assertEqual(files[name]["status"], "success")
                for stage in ("dump", "check", "convert", "redump", "recheck"):
                    self.assertEqual(files[name][stage]["returncode"], 0)
            self.assertEqual(files["damaged/cut.iges"]["status"], "dump_refused")
            commands = [json.loads(line) for line in calls.read_text().splitlines()]
            self.assertEqual(len(commands), 11)
            self.assertEqual(sum(command[0] == "convert" for command in commands), 2)


if __name__ == "__main__":
    unittest.main()
