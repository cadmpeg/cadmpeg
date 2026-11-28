#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check classifications beside production resource-cap constants."""

from __future__ import annotations

import importlib.util
import re
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC_REL = Path("docs") / "decode-resource-caps.md"
SOURCE_SCRIPT = Path(__file__).with_name("check-source-policy.py")
SOURCE_POLICY = sys.modules.get("shared_rust_source_policy")
if SOURCE_POLICY is None:
    SPEC = importlib.util.spec_from_file_location("resource_caps_source_policy", SOURCE_SCRIPT)
    if SPEC is None or SPEC.loader is None:
        raise SystemExit(f"cannot load {SOURCE_SCRIPT}")
    SOURCE_POLICY = importlib.util.module_from_spec(SPEC)
    sys.modules[SPEC.name] = SOURCE_POLICY
    SPEC.loader.exec_module(SOURCE_POLICY)

CONST_MAX = re.compile(
    r"(?<![\w:])(?:pub(?:\s*\([^)]*\))?\s+)?const\s+(MAX_[A-Z0-9_]+)\b"
)
STORED_TOTAL = re.compile(r"Production `MAX_\*` constants: \d+\.")
TOTALS_HEADING = re.compile(r"^## Totals\s*$", re.MULTILINE)
KINDS = ("1", "2", "3", "4", "N")
SESSION_CEILING = "3"


@dataclass(frozen=True)
class Constant:
    relative: str
    name: str
    line: int
    kind: str | None


MARKER = re.compile(r"// resource-cap: (MAX_[A-Z0-9_]+) = ([124N3])\s*$")


def scan_markers(relative: str, source: str) -> tuple[list[Constant], list[str]]:
    code, _ = SOURCE_POLICY.production_source(source)
    masked = SOURCE_POLICY.mask_rust_non_code(source)
    test_spans = SOURCE_POLICY.test_only_spans(masked)
    markers: dict[int, str] = {}
    findings: list[str] = []
    for start, end in SOURCE_POLICY.rust_non_code_spans(source):
        comment = source[start:end]
        if not comment.startswith("// resource-cap:"):
            continue
        if any(a <= start < b for a, b in test_spans):
            continue
        line = source.count("\n", 0, start) + 1
        line_start = source.rfind("\n", 0, start) + 1
        if source[line_start:start].strip():
            findings.append(f"{relative}:{line}: resource-cap marker must be standalone")
            continue
        markers[line] = comment

    declarations = []
    consumed: set[int] = set()
    for match in CONST_MAX.finditer(code):
        name = match.group(1)
        line = code.count("\n", 0, match.start()) + 1
        line_start = code.rfind("\n", 0, match.start()) + 1
        if code[line_start:match.start()].strip():
            findings.append(f"{relative}:{line}: {name} must start its own line")
        marker_lines = []
        previous = line - 1
        while previous in markers:
            marker_lines.append(previous)
            previous -= 1
        kind = None
        if any(marker_line in consumed for marker_line in marker_lines):
            findings.append(f"{relative}:{line}: resource-cap marker reused for {name}")
        elif len(marker_lines) != 1:
            detail = "missing" if not marker_lines else "duplicate"
            findings.append(f"{relative}:{line}: {detail} resource-cap marker for {name}")
        else:
            marker_line = marker_lines[0]
            marker = MARKER.fullmatch(markers[marker_line])
            if marker is None or marker.group(1) != name:
                findings.append(f"{relative}:{marker_line}: invalid resource-cap marker for {name}")
            elif marker.group(2) == SESSION_CEILING:
                findings.append(f"{relative}:{marker_line}: {name} is kind 3; only ResourceLimits fields are session ceilings")
            else:
                kind = marker.group(2)
        consumed.update(marker_lines)
        declarations.append(Constant(relative, name, line, kind))
    for line in sorted(markers.keys() - consumed):
        findings.append(f"{relative}:{line}: stale resource-cap marker")
    return declarations, findings


def census(root: Path, sources: dict[Path, str] | None = None) -> tuple[list[Constant], list[str]]:
    constants: list[Constant] = []
    findings: list[str] = []
    paths = sorted(root.glob("crates/**/*.rs")) if sources is None else sorted(sources)
    for path in paths:
        relative = path.relative_to(root)
        if SOURCE_POLICY.is_production_rs(relative):
            text = path.read_text(encoding="utf-8") if sources is None else sources[path]
            found, errors = scan_markers(relative.as_posix(), text)
            constants.extend(found)
            findings.extend(errors)
    doc = root / DOC_REL
    if not doc.is_file():
        findings.append(f"{DOC_REL.as_posix()}: missing")
    else:
        findings.extend(stored_totals(doc.read_text(encoding="utf-8")))
    return constants, findings


def stored_totals(text: str) -> list[str]:
    """Hand-kept counts, which the checker computes and prints instead."""
    findings: list[str] = []
    for pattern in (TOTALS_HEADING, STORED_TOTAL):
        for match in pattern.finditer(text):
            line = text.count("\n", 0, match.start()) + 1
            findings.append(
                f"{DOC_REL.as_posix()}:{line}: stored total {match.group(0).strip()!r}; "
                f"delete it, python3 {Path('scripts') / Path(__file__).name} prints the counts"
            )
    return findings


def counts(constants: list[Constant]) -> list[str]:
    kinds = Counter(c.kind for c in constants)
    crates = Counter(Path(c.relative).parts[1] for c in constants)
    return [
        f"production MAX_* constants: {len(constants)}",
        "by kind: " + ", ".join(f"{kind} {kinds.get(kind, 0)}" for kind in KINDS),
        "by crate: " + ", ".join(f"{crate} {crates[crate]}" for crate in sorted(crates)),
    ]


@SOURCE_POLICY.analysis_session
def main() -> int:
    constants, findings = census(ROOT)
    if findings:
        for line in findings:
            print(line, file=sys.stderr)
        print(f"resource-caps census: {len(findings)} mismatch(es)", file=sys.stderr)
        return 1
    print("resource-caps census: ok")
    for line in counts(constants):
        print(f"  {line}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
