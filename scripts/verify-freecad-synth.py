#!/usr/bin/env python3
"""Run in FreeCAD's Python environment against synthesized FCStd files.

Without ``CADMPEG_FCSTD_MANIFEST`` this checks the source-less synthesis
gate: every ``*.FCStd`` file in the input directory needs a successful
wire-v10 synthesized export report, opens in FreeCAD, recomputes without an
object left in an ``Invalid`` or ``Error`` state, and restores the geometry
bodies its report claims. A report without a file, or a file without a
report, fails the gate.

When ``CADMPEG_FCSTD_MANIFEST`` names the contract suite's ``manifest.json``,
this instead checks every §5.1 and §5.2 FreeCAD output: the file set must
agree with the manifest exactly (files under ``damaged/`` keep that prefix
as their key), each file opens and recomputes with no object ``Invalid`` or
in ``Error``, and the FreeCAD solid and face counts equal the cadmpeg
re-decode counts in the matching entry. An entry carrying ``error`` needs
only a successful open and recompute; its counts do not exist.
"""
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


def load_manifest(path: Path):
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


def check_object_states(document):
    invalid = [obj.Name for obj in document.Objects
               if hasattr(obj, "Shape") and not obj.Shape.isNull() and not obj.Shape.isValid()]
    errors = [{"object": obj.Name, "state": list(getattr(obj, "State", []))}
              for obj in document.Objects
              if any(str(state).lower() in {"invalid", "error", "restore", "restoreerror"}
                     for state in getattr(obj, "State", []))]
    return invalid, errors


def close_document(app, document, result) -> bool:
    try:
        app.closeDocument(document.Name)
    except Exception as error:
        previous = result.get("error")
        result.update(accepted=False, error=(f"{previous}; " if previous else "") + f"closeDocument failed: {error}")
        return False
    return True


def accepted_shape_counts(document):
    shape_objects = [obj for obj in document.Objects if hasattr(obj, "Shape")]
    nonnull = [obj for obj in shape_objects if not obj.Shape.isNull()]
    return {
        "objects": len(document.Objects),
        "shape_objects": len(shape_objects),
        "nonnull_shapes": len(nonnull),
        "null_shapes": len(shape_objects) - len(nonnull),
        "faces": sum(len(getattr(obj.Shape, "Faces", [])) for obj in nonnull),
        "solids": sum(len(getattr(obj.Shape, "Solids", [])) for obj in nonnull),
    }


def check_manifest_counts(counts, entry, filename):
    if "error" in entry:
        return []
    errors = []
    for field in ("solids", "faces"):
        expected = entry.get(field)
        if isinstance(expected, bool) or not isinstance(expected, int):
            errors.append(f"manifest {field} for {filename} is not an integer")
        elif counts[field] != expected:
            errors.append(f"{field}: expected {expected}, observed {counts[field]}")
    unknown = set(entry) - {"solids", "faces", "error"}
    errors.extend(f"unknown manifest field: {field}" for field in sorted(unknown))
    return errors


def verify(app, root: Path, report: Path, manifest=None) -> int:
    if manifest is None:
        return verify_synth(app, root, report)
    return verify_manifest(app, root, report, manifest)


def open_recompute_and_count(app, path: Path, result) -> bool:
    """Open, recompute, and record shape counts. Returns False on failure."""
    document = None
    ok = False
    try:
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
        try:
            document.recompute()
        except Exception as error:
            raise RuntimeError(f"recompute failed: {error}")
        invalid, errors = check_object_states(document)
        result.update(accepted_shape_counts(document),
                      invalid_shapes=invalid, object_errors=errors)
        if restore_failed:
            raise RuntimeError("native restore diagnostics report a document error")
        if invalid:
            raise RuntimeError(f"invalid shape objects: {invalid}")
        if errors:
            raise RuntimeError(f"object restore or evaluation errors: {errors}")
        ok = True
    except Exception as error:
        result["error"] = str(error)
    finally:
        if document is not None:
            ok = close_document(app, document, result) and ok
    return ok


def verify_manifest(app, root: Path, report: Path, manifest) -> int:
    files = sorted(
        (path for path in root.rglob("*") if path.is_file() and path.suffix == ".FCStd"),
        key=lambda path: path.relative_to(root).as_posix(),
    )
    actual = {path.relative_to(root).as_posix() for path in files}
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
    for path in files:
        filename = path.relative_to(root).as_posix()
        result = {"file": filename, "accepted": False}
        print(f"CADMPEG_NATIVE_BEGIN {path.name}", file=sys.stderr, flush=True)
        if open_recompute_and_count(app, path, result):
            count_errors = check_manifest_counts(result, manifest[filename], filename)
            if count_errors:
                result.update(accepted=False, error="; ".join(count_errors))
            else:
                result["accepted"] = True
        print(f"CADMPEG_NATIVE_END {path.name} accepted={result['accepted']}", file=sys.stderr, flush=True)
        results.append(result)
    report.write_text(json.dumps({"manifest": True, "files": results}, indent=2) + "\n")
    if not results or not all(result["accepted"] for result in results):
        raise RuntimeError("FCStd interop acceptance failed; see report")
    return len(results)


def verify_synth(app, root: Path, report: Path) -> int:
    files = sorted(root.glob("*.FCStd"))
    results = []
    for path in files:
        result = {"file": path.relative_to(root).as_posix(), "accepted": False}
        print(f"CADMPEG_NATIVE_BEGIN {path.name}", file=sys.stderr, flush=True)
        try:
            provenance = json.loads(path.with_suffix(".report.json").read_text())
            export = provenance.get("export") if isinstance(provenance, dict) else None
            write_path = export.get("write_path") if isinstance(export, dict) else None
            if (not isinstance(provenance, dict) or not isinstance(write_path, dict)
                    or provenance.get("ir_version") != "10"
                    or provenance.get("status") != "ok"
                    or provenance.get("command") != "convert"
                    or write_path.get("path") != "synthesized"):
                raise RuntimeError("input does not have a successful wire-v10 synthesized export report")
            check = provenance.get("check_report", {})
            counts = check.get("entity_counts", {}) if isinstance(check, dict) else {}
            expected_bodies = counts.get("bodies", 0) if isinstance(counts, dict) else 0
            result["reported_bodies"] = expected_bodies
            if not open_recompute_and_count(app, path, result):
                raise RuntimeError(result["error"])
            if expected_bodies and not result["nonnull_shapes"]:
                raise RuntimeError("export reported geometry bodies but FreeCAD restored no non-null Shape")
            result["accepted"] = True
        except Exception as error:
            result["error"] = str(error)
        print(f"CADMPEG_NATIVE_END {path.name} accepted={result['accepted']}", file=sys.stderr, flush=True)
        results.append(result)
    orphans = sorted(path.name for path in root.glob("*.report.json")
                     if root / (path.name[:-len(".report.json")] + ".FCStd") not in files)
    if orphans:
        results.append({"file": ";".join(orphans), "accepted": False,
                        "error": f"provenance reports without a synthesized file: {', '.join(orphans)}"})
    report.write_text(json.dumps({"manifest": None, "files": results}, indent=2) + "\n")
    if not results or not all(result["accepted"] for result in results):
        raise RuntimeError("synthesized FCStd acceptance failed; see report")
    return len(results)


def main() -> None:
    import FreeCAD as App

    manifest_path = os.environ.get("CADMPEG_FCSTD_MANIFEST")
    manifest = load_manifest(Path(manifest_path)) if manifest_path else None
    try:
        verify(App, Path(os.environ["CADMPEG_FCSTD_INPUT_DIR"]), Path(os.environ["CADMPEG_FCSTD_REPORT"]),
               manifest)
    except RuntimeError as error:
        # FreeCADCmd propagates SystemExit but swallows any other uncaught
        # exception with a zero exit, which would let a failed gate pass.
        print(f"FAILED: {error}", file=sys.stderr, flush=True)
        raise SystemExit(1)


# FreeCADCmd executes Python files under their filename stem rather than
# __main__. Importing this module for tests still leaves verification explicit.
if __name__ in {"__main__", "verify-freecad-synth"}:
    main()
