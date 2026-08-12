#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Shadow-compare full validate_neutral admit/reject vs route Check subsets.

Decodes golden fixtures under the current (full) gate semantics and compares
admit/reject outcomes against the documented admit predicates. Requires zero
divergence before production gates switch onto the predicates.

Usage:
  python3 scripts/admissibility-shadow.py [--timeout SEC] [--json OUT]
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from dataclasses import dataclass, asdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

ROUTES = {
    "rhino": {
        "fixture_glob": "crates/cadmpeg-codec-rhino/tests/golden/fixtures/*",
        "checks": "rhino_draft",  # draft+instance both ⊆ core+annotations; instance ⊆ draft
    },
    "catia": {
        "fixture_glob": "crates/cadmpeg-codec-catia/tests/golden/fixtures/*",
        "checks": "catia",
    },
    "iges": {
        "fixture_glob": "crates/cadmpeg-codec-iges/tests/golden/fixtures/*",
        "checks": "iges_full",
    },
    "sldprt": {
        "fixture_glob": "crates/cadmpeg-codec-sldprt/tests/golden/fixtures/*",
        "checks": "sldprt_export",
    },
}


@dataclass
class FileResult:
    route: str
    path: str
    status: str  # agree_accept | agree_reject | diverge | decode_fail | timeout
    full_ok: bool | None = None
    admit_ok: bool | None = None
    detail: str = ""


def list_fixtures(pattern: str) -> list[Path]:
    # Expand via pathlib relative to ROOT
    parent, name = pattern.rsplit("/", 1)
    base = ROOT / parent
    if not base.is_dir():
        return []
    return sorted(p for p in base.glob(name) if p.is_file())


def decode_to_cadir(binary: Path, src: Path, out: Path, timeout: int) -> tuple[str, str]:
    try:
        proc = subprocess.run(
            [str(binary), "decode", str(src), "-o", str(out), "--force"],
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=ROOT,
        )
    except subprocess.TimeoutExpired:
        return "timeout", "decode timed out"
    if proc.returncode != 0:
        return "decode_fail", (proc.stderr or proc.stdout or "decode failed")[:400]
    return "ok", ""


def validate_report(binary: Path, cadir: Path, timeout: int) -> tuple[bool | None, str]:
    try:
        proc = subprocess.run(
            [str(binary), "validate", str(cadir), "--json"],
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=ROOT,
        )
    except subprocess.TimeoutExpired:
        return None, "validate timed out"
    # validate exits 1 on findings; still parse JSON from stdout when present
    text = proc.stdout.strip()
    if not text:
        return None, (proc.stderr or "empty validate stdout")[:400]
    try:
        start = text.find("{")
        if start < 0:
            return None, "no json object in validate stdout"
        payload = json.loads(text[start:])
    except json.JSONDecodeError as exc:
        return None, f"json: {exc}"
    report = payload.get("validation_report") or payload
    findings = report.get("findings") or []
    ok = not any(f.get("severity") in ("error", "blocking") for f in findings)
    return ok, ""


def admit_ok_from_findings(findings: list[dict], allowed: set[str]) -> bool:
    for finding in findings:
        if finding.get("severity") not in ("error", "blocking"):
            continue
        if finding.get("check") in allowed:
            return False
    return True


CHECK_SETS = {
    "draft_core": {
        "identity",
        "referential_integrity",
        "native_links",
        "loop_closure",
        "coedge_pairing",
        "shell_topology",
        "wire_topology",
        "carrier_reachability",
        "parameter_domain",
        "bounds",
        "geometric_consistency",
    },
    "rhino_draft": None,  # filled below
    "catia": None,
    "sldprt_export": None,
    "iges_full": None,  # special: admit == full
}

CHECK_SETS["rhino_draft"] = CHECK_SETS["draft_core"] | {"annotations"}
CHECK_SETS["catia"] = set(CHECK_SETS["draft_core"])
CHECK_SETS["sldprt_export"] = set(CHECK_SETS["draft_core"])


def findings_from_validate(binary: Path, cadir: Path, timeout: int) -> tuple[list[dict] | None, str]:
    try:
        proc = subprocess.run(
            [str(binary), "validate", str(cadir), "--json"],
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=ROOT,
        )
    except subprocess.TimeoutExpired:
        return None, "validate timed out"
    text = proc.stdout.strip()
    if not text:
        return None, (proc.stderr or "empty validate stdout")[:400]
    try:
        start = text.find("{")
        if start < 0:
            return None, "no json object in validate stdout"
        payload = json.loads(text[start:])
    except json.JSONDecodeError as exc:
        return None, f"json: {exc}"
    report = payload.get("validation_report") or payload
    return list(report.get("findings") or []), ""


def run_route(binary: Path, route: str, timeout: int) -> list[FileResult]:
    cfg = ROUTES[route]
    fixtures = list_fixtures(cfg["fixture_glob"])
    results: list[FileResult] = []
    checks_key = cfg["checks"]
    with tempfile.TemporaryDirectory(prefix=f"admit-shadow-{route}-") as tmp:
        tmp_path = Path(tmp)
        for src in fixtures:
            cadir = tmp_path / (src.stem + ".cadir.json")
            status, detail = decode_to_cadir(binary, src, cadir, timeout)
            if status != "ok":
                results.append(
                    FileResult(route=route, path=str(src.relative_to(ROOT)), status=status, detail=detail)
                )
                continue
            findings, err = findings_from_validate(binary, cadir, timeout)
            if findings is None:
                results.append(
                    FileResult(
                        route=route,
                        path=str(src.relative_to(ROOT)),
                        status="decode_fail",
                        detail=err,
                    )
                )
                continue
            full_ok = not any(f.get("severity") in ("error", "blocking") for f in findings)
            if checks_key == "iges_full":
                admit = full_ok
            else:
                admit = admit_ok_from_findings(findings, CHECK_SETS[checks_key])
            if full_ok == admit:
                st = "agree_accept" if full_ok else "agree_reject"
            else:
                st = "diverge"
            results.append(
                FileResult(
                    route=route,
                    path=str(src.relative_to(ROOT)),
                    status=st,
                    full_ok=full_ok,
                    admit_ok=admit,
                )
            )
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timeout", type=int, default=60)
    parser.add_argument("--json", type=Path, default=None)
    parser.add_argument(
        "--binary",
        type=Path,
        default=ROOT / "target" / "debug" / "cadmpeg",
    )
    parser.add_argument(
        "--routes",
        nargs="+",
        default=list(ROUTES),
        choices=list(ROUTES),
    )
    args = parser.parse_args()
    if not args.binary.is_file():
        print(f"missing binary {args.binary}; build with: cargo build -q -p cadmpeg", file=sys.stderr)
        return 2

    all_results: list[FileResult] = []
    for route in args.routes:
        all_results.extend(run_route(args.binary, route, args.timeout))

    counts: dict[str, int] = {}
    for r in all_results:
        counts[r.status] = counts.get(r.status, 0) + 1
    diverge = [r for r in all_results if r.status == "diverge"]

    summary = {
        "total": len(all_results),
        "counts": counts,
        "diverge": [asdict(r) for r in diverge],
        "results": [asdict(r) for r in all_results],
    }
    if args.json:
        args.json.write_text(json.dumps(summary, indent=2) + "\n")

    print(f"total\t{summary['total']}")
    for key in sorted(counts):
        print(f"{key}\t{counts[key]}")
    if diverge:
        print("DIVERGENCE:", file=sys.stderr)
        for r in diverge:
            print(f"  {r.route}\t{r.path}\tfull={r.full_ok}\tadmit={r.admit_ok}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
