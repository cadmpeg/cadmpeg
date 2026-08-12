#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Count mechanical anti-patterns and fail when any count exceeds the ledger.

Patterns (production filter — see ``docs/convergence-ledger.toml``):

* ``from_le_bytes`` / ``from_be_bytes`` in ``crates/cadmpeg-codec-*/src``
* ``le::*_at`` / ``be::*_at`` call sites outside ``cadmpeg-core`` (not ``use`` lines)
* ``CodecError::Malformed(format!`` (multiline-aware)
* ``LossNote {`` struct literals (not ``-> LossNote {`` or the struct definition)
* bare ``1e-6`` / ``1e-9`` / ``1e-10`` / ``1e-12`` in ``crates/**/src``
* non-literal ``vec![value; count]`` repeats (parsed-count allocations)

Modes:

* check (default): exit 1 when any count is above the ledger ceiling
* ``--update``: write new counts when every count is ≤ its ledger ceiling
* ``--json``: emit machine-readable counts (and check result) on stdout

A deliberate increase is a manual ledger edit: raise the ceiling and record a
reason under ``[reasons]`` for that key in the same commit.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs" / "convergence-ledger.toml"

METRIC_KEYS = (
    "from_endian_bytes",
    "le_be_at_outside_core",
    "codec_error_malformed_format",
    "loss_note_struct_literals",
    "bare_tolerance_literals",
    "nonliteral_vec_repeat",
)

FROM_ENDIAN = re.compile(r"\bfrom_(?:le|be)_bytes\b")
LE_BE_AT = re.compile(r"\b(?:le|be)::[A-Za-z_][A-Za-z0-9_]*_at\b")
MALFORMED_FORMAT = re.compile(r"CodecError::Malformed\s*\(\s*format!", re.MULTILINE)
LOSS_NOTE_LIT = re.compile(r"LossNote\s*\{")
LOSS_NOTE_RETURN = re.compile(r"->\s*LossNote\s*\{")
LOSS_NOTE_STRUCT = re.compile(r"\b(?:pub(?:\([^)]*\))?\s+)?struct\s+LossNote\s*\{")
BARE_TOLERANCE = re.compile(r"(?<![0-9A-Za-z_.])1[eE]-(?:6|9|10|12)\b")
# `vec![value; count]` where count is not a decimal/hex literal.
VEC_REPEAT = re.compile(r"vec!\s*\[(?:[^\];]|;)*;\s*([^\]]+)\]", re.MULTILINE)
VEC_REPEAT_LITERAL = re.compile(r"^(?:0x[0-9a-fA-F]+|\d+)$")
CFG_TEST_ATTR = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")

FILTER_DESCRIPTION = (
    "production .rs under crates/**/src; exclude tests/ and benches/ path segments; "
    "exclude files named tests.rs or *test*.rs; strip cfg(test)-attributed items "
    "and their brace bodies when the attribute immediately precedes the item"
)


def is_production_rs(path: Path) -> bool:
    """True when ``path`` is a production ``.rs`` file under the filter."""
    if path.suffix != ".rs":
        return False
    parts = path.parts
    if "tests" in parts or "benches" in parts:
        return False
    name = path.name
    if name == "tests.rs" or re.search(r"test", name, re.IGNORECASE):
        return False
    return "src" in parts


def strip_cfg_test_items(text: str) -> str:
    """Remove ``#[cfg(test)]``-attributed items and their bodies when practical."""
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if CFG_TEST_ATTR.search(line.split("//", 1)[0]):
            out.append("\n" if line.endswith("\n") else "")
            i += 1
            while i < len(lines):
                stripped = lines[i].lstrip()
                if (
                    stripped.startswith("#[")
                    or stripped.startswith("//!")
                    or stripped.startswith("///")
                ):
                    out.append("\n" if lines[i].endswith("\n") else "")
                    i += 1
                    continue
                break
            if i >= len(lines):
                break
            item = lines[i]
            if "{" not in item:
                out.append("\n" if item.endswith("\n") else "")
                i += 1
                continue
            depth = 0
            while i < len(lines):
                for ch in lines[i]:
                    if ch == "{":
                        depth += 1
                    elif ch == "}":
                        depth -= 1
                out.append("\n" if lines[i].endswith("\n") else "")
                i += 1
                if depth <= 0:
                    break
            continue
        out.append(line)
        i += 1
    return "".join(out)


def iter_src_files(glob: str) -> list[Path]:
    return sorted(p for p in ROOT.glob(glob) if is_production_rs(p))


def count_from_endian_bytes() -> int:
    total = 0
    for path in iter_src_files("crates/cadmpeg-codec-*/src/**/*.rs"):
        text = strip_cfg_test_items(path.read_text(encoding="utf-8", errors="replace"))
        total += len(FROM_ENDIAN.findall(text))
    return total


def count_le_be_at_outside_core() -> int:
    total = 0
    for path in iter_src_files("crates/**/src/**/*.rs"):
        if "cadmpeg-core" in path.parts:
            continue
        text = strip_cfg_test_items(path.read_text(encoding="utf-8", errors="replace"))
        for line in text.splitlines():
            code = line.split("//", 1)[0]
            if code.lstrip().startswith("use "):
                continue
            total += len(LE_BE_AT.findall(code))
    return total


def count_malformed_format() -> int:
    total = 0
    for path in iter_src_files("crates/**/src/**/*.rs"):
        text = strip_cfg_test_items(path.read_text(encoding="utf-8", errors="replace"))
        total += len(MALFORMED_FORMAT.findall(text))
    return total


def count_loss_note_literals() -> int:
    total = 0
    for path in iter_src_files("crates/**/src/**/*.rs"):
        text = strip_cfg_test_items(path.read_text(encoding="utf-8", errors="replace"))
        for line in text.splitlines():
            code = line.split("//", 1)[0]
            if not LOSS_NOTE_LIT.search(code):
                continue
            if LOSS_NOTE_RETURN.search(code) or LOSS_NOTE_STRUCT.search(code):
                continue
            total += 1
    return total


def count_bare_tolerances() -> int:
    total = 0
    for path in iter_src_files("crates/**/src/**/*.rs"):
        text = strip_cfg_test_items(path.read_text(encoding="utf-8", errors="replace"))
        total += len(BARE_TOLERANCE.findall(text))
    return total


def count_nonliteral_vec_repeat() -> int:
    total = 0
    for path in iter_src_files("crates/**/src/**/*.rs"):
        text = strip_cfg_test_items(path.read_text(encoding="utf-8", errors="replace"))
        for match in VEC_REPEAT.finditer(text):
            count_expr = match.group(1).strip()
            if VEC_REPEAT_LITERAL.fullmatch(count_expr):
                continue
            total += 1
    return total


def measure() -> dict[str, int]:
    return {
        "from_endian_bytes": count_from_endian_bytes(),
        "le_be_at_outside_core": count_le_be_at_outside_core(),
        "codec_error_malformed_format": count_malformed_format(),
        "loss_note_struct_literals": count_loss_note_literals(),
        "bare_tolerance_literals": count_bare_tolerances(),
        "nonliteral_vec_repeat": count_nonliteral_vec_repeat(),
    }


def parse_ledger(path: Path) -> dict[str, object]:
    """Minimal TOML reader for the ledger shape used here."""
    text = path.read_text(encoding="utf-8")
    data: dict[str, object] = {"ceilings": {}, "reasons": {}}
    section: str | None = None
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1].strip()
            continue
        if "=" not in line:
            continue
        key, _, value = line.partition("=")
        key = key.strip()
        value = value.strip()
        if value.startswith('"') and value.endswith('"'):
            parsed: object = (
                value[1:-1].replace('\\"', '"').replace("\\\\", "\\")
            )
        else:
            parsed = int(value)
        if section is None:
            data[key] = parsed
        elif section == "ceilings":
            cast = data["ceilings"]
            assert isinstance(cast, dict)
            cast[key] = parsed
        elif section == "reasons":
            cast = data["reasons"]
            assert isinstance(cast, dict)
            cast[key] = parsed
    return data


def render_ledger(
    measured_at: str,
    filter_description: str,
    ceilings: dict[str, int],
    reasons: dict[str, str],
) -> str:
    lines = [
        "# Convergence ratchet ceilings. Counts may only fall.",
        "# A decrease updates this file in the same commit. A deliberate increase",
        "# requires a reason under [reasons] for every raised key.",
        f'measured_at = "{measured_at}"',
        f'filter = "{filter_description}"',
        "",
        "[ceilings]",
    ]
    for key in METRIC_KEYS:
        lines.append(f"{key} = {ceilings[key]}")
    if reasons:
        lines.append("")
        lines.append("[reasons]")
        for key in METRIC_KEYS:
            if key in reasons:
                escaped = reasons[key].replace("\\", "\\\\").replace('"', '\\"')
                lines.append(f'{key} = "{escaped}"')
    lines.append("")
    return "\n".join(lines)


def git_head() -> str:
    return (
        subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT)
        .decode()
        .strip()
    )


def check(counts: dict[str, int], ceilings: dict[str, int]) -> list[str]:
    failures: list[str] = []
    for key in METRIC_KEYS:
        if key not in ceilings:
            failures.append(f"ledger missing ceiling for {key}")
            continue
        ceiling = ceilings[key]
        value = counts[key]
        if value > ceiling:
            failures.append(f"{key}: {value} > ledger {ceiling}")
    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--update",
        action="store_true",
        help="rewrite the ledger when every count is ≤ its ceiling",
    )
    parser.add_argument("--json", action="store_true", help="emit JSON on stdout")
    args = parser.parse_args(argv)

    if not LEDGER.is_file():
        print(f"error: missing ledger {LEDGER}", file=sys.stderr)
        return 2

    ledger = parse_ledger(LEDGER)
    ceilings_raw = ledger.get("ceilings")
    if not isinstance(ceilings_raw, dict):
        print("error: ledger has no [ceilings] table", file=sys.stderr)
        return 2
    ceilings = {str(k): int(v) for k, v in ceilings_raw.items()}  # type: ignore[arg-type]
    reasons_raw = ledger.get("reasons")
    reasons = (
        {str(k): str(v) for k, v in reasons_raw.items()}
        if isinstance(reasons_raw, dict)
        else {}
    )

    counts = measure()
    failures = check(counts, ceilings)

    if args.update:
        if failures:
            print(
                "error: --update refuses increases; raise the ceiling and add "
                "[reasons] manually for a deliberate increase",
                file=sys.stderr,
            )
            for failure in failures:
                print(f"error: {failure}", file=sys.stderr)
            return 1
        # Drop reasons for keys that are no longer above any prior justification
        # need — keep only keys still listed whose ceiling was previously raised.
        kept_reasons = {
            key: reasons[key]
            for key in METRIC_KEYS
            if key in reasons and counts[key] >= ceilings.get(key, counts[key])
        }
        # After a decrease, clear reasons that applied to the old higher ceiling.
        kept_reasons = {
            key: reason
            for key, reason in kept_reasons.items()
            if counts[key] == ceilings.get(key)
        }
        LEDGER.write_text(
            render_ledger(git_head(), FILTER_DESCRIPTION, counts, kept_reasons),
            encoding="utf-8",
        )
        if args.json:
            print(
                json.dumps(
                    {
                        "status": "updated",
                        "counts": counts,
                        "previous_ceilings": ceilings,
                    },
                    indent=2,
                    sort_keys=True,
                )
            )
        else:
            for key in METRIC_KEYS:
                prev = ceilings.get(key)
                marker = "" if prev == counts[key] else f" (was {prev})"
                print(f"{key}\t{counts[key]}{marker}")
            print(f"updated {LEDGER.relative_to(ROOT)}", file=sys.stderr)
        return 0

    status = "ok" if not failures else "fail"
    if args.json:
        print(
            json.dumps(
                {
                    "status": status,
                    "counts": counts,
                    "ceilings": ceilings,
                    "failures": failures,
                },
                indent=2,
                sort_keys=True,
            )
        )
    else:
        for key in METRIC_KEYS:
            print(f"{key}\t{counts[key]}\t(ceiling {ceilings.get(key, '?')})")
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
