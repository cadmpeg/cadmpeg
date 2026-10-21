#!/usr/bin/env python3
"""Tests for source-less FCStd materialization and native acceptance gates."""
from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import runpy
import stat
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


def load(name: str, filename: str):
    specification = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    assert specification is not None and specification.loader is not None
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


PREPARE = load("prepare_freecad_synth", "prepare-freecad-synth.py")
VERIFY = load("verify_freecad_synth", "verify-freecad-synth.py")
SUCCESS = {"ir_version": "8", "command": "convert", "status": "ok",
           "export": {"write_path": {"path": "synthesized"}}}


class MaterializationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.fixtures = self.root / "fixtures"
        self.fixtures.mkdir()
        for index in range(12):
            (self.fixtures / f"fixture_{index:02}.FCStd").write_bytes(b"source archive")
        self.output = self.root / "output"
        self.log = self.root / "calls.jsonl"
        self.cli = self.root / "fake-cadmpeg"
        self.cli.write_text('''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys
args = sys.argv[1:]
mode = os.environ.get("CADMPEG_SYNTH_TEST_MODE", "ok")
with Path(os.environ["CADMPEG_SYNTH_TEST_LOG"]).open("a") as log:
    log.write(json.dumps(args) + "\\n")
if mode == "dump_failure" and args[0] == "dump":
    sys.exit(3)
target = Path(args[args.index("-o") + 1])
if args[0] == "dump":
    target.write_text(json.dumps({"ir_version": "8", "scope": {"model": True, "design": True}}))
    target.with_suffix(".fidelity.json").write_text("retained source bytes")
else:
    if list(Path(args[1]).parent.glob("*.fidelity.json")):
        sys.exit(4)
    if mode != "missing_output":
        target.write_bytes(b"authored FCStd")
    report = {"ir_version": "8", "command": "convert", "status": "ok",
              "export": {"write_path": {"path": "synthesized"}}}
    if mode == "replay":
        report["export"]["write_path"]["path"] = "verbatim_replay"
    if mode == "old_wire":
        report["ir_version"] = "6"
    if mode == "refused":
        report["status"] = "refused"
    if mode == "malformed_report":
        report["export"] = None
    Path(args[args.index("--report") + 1]).write_text(json.dumps(report))
''', encoding="utf-8")
        self.cli.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
        self.environment = patch.dict(os.environ, {"CADMPEG_SYNTH_TEST_LOG": str(self.log)})
        self.environment.start()

    def tearDown(self) -> None:
        self.environment.stop()
        self.temporary.cleanup()

    def prepare(self, mode: str = "ok", fixtures: Path | None = None) -> int:
        with patch.dict(os.environ, {"CADMPEG_SYNTH_TEST_MODE": mode}):
            return PREPARE.prepare(self.cli, fixtures or self.fixtures, self.output)

    def test_materializes_all_twelve_public_fixtures_without_fidelity(self) -> None:
        public = Path(__file__).resolve().parents[1] / "corpus/freecad_fcstd/fixtures"
        self.assertEqual(self.prepare(fixtures=public), 12)
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertEqual(len(calls), 24)
        self.assertEqual(len(list(self.output.glob("*.FCStd"))), 12)
        self.assertEqual(len(list(self.output.glob("*.report.json"))), 12)
        for arguments in calls[1::2]:
            self.assertEqual(arguments[0], "convert")
            self.assertEqual(arguments[arguments.index("--to") + 1], "fcstd:schema-4")
            self.assertIn("--allow-empty", arguments)
        for report in self.output.glob("*.report.json"):
            self.assertEqual(json.loads(report.read_text()), SUCCESS)

    def test_wrong_fixture_count_refuses_before_invoking_cli(self) -> None:
        (self.fixtures / "fixture_00.FCStd").unlink()
        with self.assertRaisesRegex(RuntimeError, "expected 12.*found 11"):
            self.prepare()
        self.assertFalse(self.log.exists())

    def test_requires_successful_current_wire_synthesized_reports(self) -> None:
        for mode in ["replay", "old_wire", "refused", "malformed_report"]:
            with self.subTest(mode=mode), self.assertRaisesRegex(RuntimeError, "successful wire-v8 synthesized"):
                self.prepare(mode)

    def test_missing_materialized_output_is_refused(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "nonempty FCStd output"):
            self.prepare("missing_output")

    def test_cli_decode_failure_propagates_without_materializing(self) -> None:
        with self.assertRaises(subprocess.CalledProcessError) as error:
            self.prepare("dump_failure")
        self.assertEqual(error.exception.returncode, 3)
        self.assertFalse(list(self.output.glob("*.FCStd")))


class NativeAcceptanceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.report = self.root / "native-acceptance.json"
        for index in range(12):
            path = self.root / f"fixture_{index:02}.FCStd"
            path.write_bytes(b"authored archive")
            path.with_suffix(".report.json").write_text(json.dumps(SUCCESS))
        self.opened = []
        self.closed = []

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def application(self, mode: str = "ok"):
        def open_document(filename: str):
            path = Path(filename)
            self.opened.append(path.name)
            special = path.stem == "fixture_00"
            if special and mode == "open_failure":
                raise RuntimeError("FreeCAD cannot load this archive")
            if special and mode == "no_document":
                return None
            shape = SimpleNamespace(isNull=lambda: special and mode == "null_shape",
                                    isValid=lambda: not (special and mode == "invalid_shape"))
            return SimpleNamespace(Name=path.stem, Objects=[SimpleNamespace(Name="Body", Shape=shape),
                                                          SimpleNamespace(Name="Parameter")])

        def close_document(name: str) -> None:
            self.closed.append(name)
            if name == "fixture_00" and mode == "close_failure":
                raise RuntimeError("application cleanup failed")

        return SimpleNamespace(openDocument=open_document, closeDocument=close_document)

    def results(self):
        return json.loads(self.report.read_text())["files"]

    def test_all_twelve_inputs_open_validate_and_close(self) -> None:
        self.assertEqual(VERIFY.verify(self.application(), self.root, self.report), 12)
        self.assertEqual(len(self.opened), 12)
        self.assertEqual(len(self.closed), 12)
        self.assertTrue(all(result["accepted"] and result["objects"] == 2 for result in self.results()))

    def test_native_failures_are_reported_while_other_inputs_continue(self) -> None:
        for mode, reason in [("open_failure", "cannot load"), ("no_document", "returned no document"),
                             ("invalid_shape", "invalid shape"), ("close_failure", "closeDocument failed")]:
            with self.subTest(mode=mode):
                self.opened.clear()
                self.closed.clear()
                with self.assertRaisesRegex(RuntimeError, "acceptance failed"):
                    VERIFY.verify(self.application(mode), self.root, self.report)
                results = self.results()
                self.assertEqual(len(results), 12)
                self.assertEqual(len(self.opened), 12)
                self.assertEqual(sum(not result["accepted"] for result in results), 1)
                self.assertIn(reason, results[0]["error"])
                self.assertEqual(len(self.closed), 11 if mode in ["open_failure", "no_document"] else 12)

    def test_null_shape_and_nonshape_objects_are_valid_empty_content(self) -> None:
        self.assertEqual(VERIFY.verify(self.application("null_shape"), self.root, self.report), 12)

    def test_native_restore_error_is_rejected_even_when_document_is_returned(self) -> None:
        application = self.application()
        original_open = application.openDocument
        def open_with_error(filename):
            if Path(filename).stem == "fixture_00":
                os.write(2, b"Invalid Document.xml: Count not found\n")
            return original_open(filename)
        application.openDocument = open_with_error
        with self.assertRaisesRegex(RuntimeError, "acceptance failed"):
            VERIFY.verify(application, self.root, self.report)
        result = self.results()[0]
        self.assertFalse(result["accepted"])
        self.assertIn("Invalid Document.xml", result["restore_diagnostics"])
        self.assertIn("native restore diagnostics", result["error"])
        self.assertEqual(len(self.closed), 12)

    def test_null_shapes_reject_an_export_that_reported_geometry(self) -> None:
        provenance = json.loads(json.dumps(SUCCESS))
        provenance["check_report"] = {"entity_counts": {"bodies": 1}}
        (self.root / "fixture_00.report.json").write_text(json.dumps(provenance))
        with self.assertRaisesRegex(RuntimeError, "acceptance failed"):
            VERIFY.verify(self.application("null_shape"), self.root, self.report)
        result = self.results()[0]
        self.assertFalse(result["accepted"])
        self.assertEqual(result["reported_bodies"], 1)
        self.assertEqual(result["nonnull_shapes"], 0)
        self.assertEqual(result["null_shapes"], 1)
        self.assertIn("no non-null Shape", result["error"])
        self.assertEqual(len(self.closed), 12)

    def test_replay_input_is_not_accepted_as_a_synthesis_proof(self) -> None:
        provenance = self.root / "fixture_00.report.json"
        replay = json.loads(json.dumps(SUCCESS))
        replay["export"]["write_path"]["path"] = "patched"
        provenance.write_text(json.dumps(replay))
        with self.assertRaisesRegex(RuntimeError, "acceptance failed"):
            VERIFY.verify(self.application(), self.root, self.report)
        self.assertEqual(len(self.opened), 11)
        self.assertIn("synthesized export report", self.results()[0]["error"])

    def test_missing_provenance_is_reported_without_opening_that_input(self) -> None:
        (self.root / "fixture_00.report.json").unlink()
        with self.assertRaisesRegex(RuntimeError, "acceptance failed"):
            VERIFY.verify(self.application(), self.root, self.report)
        self.assertFalse(self.results()[0]["accepted"])
        self.assertEqual(len(self.opened), 11)

    def test_incomplete_suite_is_rejected_with_its_partial_report(self) -> None:
        (self.root / "fixture_00.FCStd").unlink()
        with self.assertRaisesRegex(RuntimeError, "acceptance failed"):
            VERIFY.verify(self.application(), self.root, self.report)
        self.assertEqual(len(self.results()), 11)

    def test_native_entry_point_uses_configured_directory_and_report(self) -> None:
        with patch.dict(sys.modules, {"FreeCAD": self.application()}), patch.dict(os.environ, {
                "CADMPEG_FCSTD_INPUT_DIR": str(self.root), "CADMPEG_FCSTD_REPORT": str(self.report)}):
            runpy.run_path(str(Path(__file__).with_name("verify-freecad-synth.py")), run_name="__main__")
        self.assertEqual(len(self.results()), 12)


if __name__ == "__main__":
    unittest.main()
