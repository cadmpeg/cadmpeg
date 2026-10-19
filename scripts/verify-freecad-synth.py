#!/usr/bin/env python3
"""Run in FreeCAD's Python environment against synthesized public FCStd files."""
import json
import os
import sys
import tempfile
from pathlib import Path


def open_with_diagnostics(app, path: Path):
    """Capture native restore errors even when openDocument returns a document."""
    document = None
    error = None
    sys.stderr.flush()
    saved = os.dup(2)
    with tempfile.TemporaryFile(mode="w+b") as capture:
        try:
            os.dup2(capture.fileno(), 2)
            try:
                document = app.openDocument(str(path))
            except Exception as exception:
                error = exception
        finally:
            sys.stderr.flush()
            os.dup2(saved, 2)
            os.close(saved)
        capture.seek(0)
        diagnostics = capture.read().decode("utf-8", errors="replace")
    if diagnostics:
        print(diagnostics, end="", file=sys.stderr, flush=True)
    return document, diagnostics, error


def verify(app, root: Path, report: Path) -> int:
    files = sorted(root.glob("*.FCStd"))
    results = []
    for path in files:
        document = None
        result = {"file": path.name, "accepted": False}
        try:
            provenance = json.loads(path.with_suffix(".report.json").read_text())
            export = provenance.get("export") if isinstance(provenance, dict) else None
            write_path = export.get("write_path") if isinstance(export, dict) else None
            if (not isinstance(provenance, dict) or not isinstance(write_path, dict)
                    or provenance.get("ir_version") != "7"
                    or provenance.get("status") != "ok"
                    or provenance.get("command") != "convert"
                    or write_path.get("path") != "synthesized"):
                raise RuntimeError("input does not have a successful wire-v7 synthesized export report")
            check = provenance.get("check_report", {})
            counts = check.get("entity_counts", {}) if isinstance(check, dict) else {}
            expected_bodies = counts.get("bodies", 0) if isinstance(counts, dict) else 0
            result["reported_bodies"] = expected_bodies
            print(f"CADMPEG_NATIVE_BEGIN {path.name}", file=sys.stderr, flush=True)
            document, diagnostics, open_error = open_with_diagnostics(app, path)
            result["restore_diagnostics"] = diagnostics
            if open_error is not None:
                raise open_error
            restore_failed = any(marker in diagnostics.lower() for marker in
                                 ("invalid document.xml", "fatal error", "<exception>",
                                  "error reading compression", "restore failed", "reading failed from embedded file",
                                  "cannot create object", "lost link to", "failed to restore",
                                  "xml attribute:", "no extension found", "extension is not a python addable"))
            if document is None:
                raise RuntimeError("openDocument returned no document")
            shape_objects = [obj for obj in document.Objects if hasattr(obj, "Shape")]
            nonnull = [obj for obj in shape_objects if not obj.Shape.isNull()]
            invalid = [obj.Name for obj in nonnull if not obj.Shape.isValid()]
            errors = [{"object": obj.Name, "state": list(getattr(obj, "State", []))}
                      for obj in document.Objects
                      if any(str(state).lower() in {"invalid", "error", "restore", "restoreerror"}
                             for state in getattr(obj, "State", []))]
            result.update(objects=len(document.Objects), shape_objects=len(shape_objects),
                          nonnull_shapes=len(nonnull), null_shapes=len(shape_objects) - len(nonnull),
                          invalid_shapes=invalid, object_errors=errors,
                          faces=sum(len(getattr(obj.Shape, "Faces", [])) for obj in nonnull),
                          solids=sum(len(getattr(obj.Shape, "Solids", [])) for obj in nonnull))
            if restore_failed:
                raise RuntimeError("native restore diagnostics report a document error")
            if invalid:
                raise RuntimeError(f"invalid shape objects: {invalid}")
            if errors:
                raise RuntimeError(f"object restore or evaluation errors: {errors}")
            if expected_bodies and not nonnull:
                raise RuntimeError("export reported geometry bodies but FreeCAD restored no non-null Shape")
            result["accepted"] = True
        except Exception as error:
            result["error"] = str(error)
        finally:
            if document is not None:
                try:
                    app.closeDocument(document.Name)
                except Exception as error:
                    previous = result.get("error")
                    result.update(accepted=False, error=(f"{previous}; " if previous else "") + f"closeDocument failed: {error}")
        print(f"CADMPEG_NATIVE_END {path.name} accepted={result['accepted']}", file=sys.stderr, flush=True)
        results.append(result)
    report.write_text(json.dumps({"files": results}, indent=2) + "\n")
    if len(results) != 12 or not all(result["accepted"] for result in results):
        raise RuntimeError("synthesized FCStd acceptance failed; see report")
    return len(results)


def main() -> None:
    import FreeCAD as App

    verify(App, Path(os.environ["CADMPEG_FCSTD_INPUT_DIR"]), Path(os.environ["CADMPEG_FCSTD_REPORT"]))


# FreeCADCmd executes Python files under their filename stem rather than
# __main__. Importing this module for tests still leaves verification explicit.
if __name__ in {"__main__", "verify-freecad-synth"}:
    main()
