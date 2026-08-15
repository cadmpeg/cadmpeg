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
        self.path = Path(self.directory.name) / "decode.report.json"

    def tearDown(self) -> None:
        self.directory.cleanup()

    def write(self, payload: object) -> None:
        self.path.write_text(json.dumps(payload), encoding="utf-8")

    def fake_cadmpeg(self, source: str) -> Path:
        executable = Path(self.directory.name) / "fake-cadmpeg"
        executable.write_text("#!/usr/bin/env python3\n" + source, encoding="utf-8")
        executable.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
        return executable

    def test_accepts_v6_decode_refusal(self) -> None:
        refusal = {
            "stage": "decode",
            "code": "decode_failed",
            "message": "decode failed: malformed container",
        }
        self.write(
            {
                "schema_version": 6,
                "command": "decode",
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
                "schema_version": 6,
                "command": "decode",
                "status": "ok",
                "refusal": None,
            }
        )

        actual, error = MODULE.load_decode_refusal(self.path)

        self.assertIsNone(actual)
        self.assertEqual(error, "decode refusal report status is not refused")

    def test_rejects_unversioned_refusal(self) -> None:
        self.write(
            {
                "schema_version": 5,
                "command": "decode",
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
        self.assertEqual(error, "decode refusal report does not use schema_version 6")

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
        self.assertEqual(result["status"], "decode_error")

    def test_gate_accepts_only_structured_decode_refusal(self) -> None:
        input_path = Path(self.directory.name) / "input.igs"
        input_path.write_bytes(b"input")
        executable = self.fake_cadmpeg(
            """import json
import pathlib
import sys

report = pathlib.Path(sys.argv[sys.argv.index('--report') + 1])
report.write_text(json.dumps({
    'schema_version': 6,
    'command': 'decode',
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
        self.assertEqual(result["status"], "decode_refused")


if __name__ == "__main__":
    unittest.main()
