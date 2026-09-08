#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Count golden fixture files and fail when any codec is below its floor.

Count: regular files under ``crates/cadmpeg-codec-<id>/tests/golden`` (recursive),
excluding any path component that starts with ``.``. These are mesh-size floors,
not branch-coverage measurements.

Modes:

* check (default): exit 1 when any codec is below its floor, a codec crate has
  no floor, or a floor names a crate that is not present
* ``--update``: write current counts as floors when every count is ≥ its floor
* ``--json``: emit machine-readable counts (and check result) on stdout

Floors may only rise when fixtures are added. A decrease is a manual ledger
edit and must not happen silently.
"""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs" / "golden-coverage-floors.toml"


def codec_ids() -> list[str]:
    """Return sorted codec directory suffixes under ``crates/cadmpeg-codec-*``."""
    ids: list[str] = []
    for path in sorted(ROOT.glob("crates/cadmpeg-codec-*")):
        if path.is_dir():
            ids.append(path.name.removeprefix("cadmpeg-codec-"))
    return ids


def is_counted_file(path: Path, golden: Path) -> bool:
    """True when ``path`` is a regular non-dotfile under ``golden``."""
    if not path.is_file() or path.is_symlink():
        return False
    rel = path.relative_to(golden)
    return not any(part.startswith(".") for part in rel.parts)


def count_golden(codec_id: str) -> int:
    """Count regular golden files for one codec crate."""
    golden = ROOT / "crates" / f"cadmpeg-codec-{codec_id}" / "tests" / "golden"
    if not golden.is_dir():
        return 0
    return sum(1 for path in golden.rglob("*") if is_counted_file(path, golden))


def measure() -> dict[str, int]:
    return {codec_id: count_golden(codec_id) for codec_id in codec_ids()}


def render_ledger(
    floors: dict[str, int],
    notes: dict[str, str],
) -> str:
    lines = [
        "# Per-codec golden breadth floors.",
        "# Count: regular files under crates/cadmpeg-codec-<id>/tests/golden (recursive).",
        "# These are mesh-size floors, not branch-coverage measurements.",
        "# Floors may only rise when fixtures are added; they must never silently fall.",
        "",
        "[floors]",
    ]
    for key in sorted(floors):
        lines.append(f"{key} = {floors[key]}")
    if notes:
        lines.append("")
        lines.append("[notes]")
        for key in sorted(notes):
            escaped = notes[key].replace("\\", "\\\\").replace('"', '\\"')
            lines.append(f'{key} = "{escaped}"')
    lines.append("")
    return "\n".join(lines)


def check(counts: dict[str, int], floors: dict[str, int]) -> list[str]:
    failures: list[str] = []
    for key in sorted(set(counts) | set(floors)):
        if key not in floors:
            failures.append(f"ledger missing floor for {key}")
            continue
        if key not in counts:
            failures.append(f"floor for {key} but no crates/cadmpeg-codec-{key}")
            continue
        floor = floors[key]
        value = counts[key]
        if value < floor:
            failures.append(f"{key}: {value} < floor {floor}")
    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--update",
        action="store_true",
        help="rewrite the ledger when every count is ≥ its floor",
    )
    parser.add_argument("--json", action="store_true", help="emit JSON on stdout")
    args = parser.parse_args(argv)

    if not LEDGER.is_file():
        print(f"error: missing ledger {LEDGER}", file=sys.stderr)
        return 2

    try:
        with LEDGER.open("rb") as handle:
            ledger = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        print(f"error: {LEDGER}: {error}", file=sys.stderr)
        return 2
    floors_raw = ledger.get("floors")
    if not isinstance(floors_raw, dict):
        print("error: ledger has no [floors] table", file=sys.stderr)
        return 2
    floors = {str(k): int(v) for k, v in floors_raw.items()}  # type: ignore[arg-type]
    notes_raw = ledger.get("notes")
    notes = (
        {str(k): str(v) for k, v in notes_raw.items()}
        if isinstance(notes_raw, dict)
        else {}
    )

    counts = measure()
    failures = check(counts, floors)

    if args.update:
        if failures:
            print(
                "error: --update refuses decreases; raise floors only when every "
                "codec is at or above its floor",
                file=sys.stderr,
            )
            for failure in failures:
                print(f"error: {failure}", file=sys.stderr)
            return 1
        kept_notes = {key: notes[key] for key in notes if key in counts}
        LEDGER.write_text(
            render_ledger(counts, kept_notes),
            encoding="utf-8",
        )
        if args.json:
            print(
                json.dumps(
                    {
                        "status": "updated",
                        "counts": counts,
                        "previous_floors": floors,
                    },
                    indent=2,
                    sort_keys=True,
                )
            )
        else:
            for key in sorted(counts):
                prev = floors.get(key)
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
                    "floors": floors,
                    "failures": failures,
                },
                indent=2,
                sort_keys=True,
            )
        )
    else:
        for key in sorted(counts):
            print(f"{key}\t{counts[key]}\t(floor {floors.get(key, '?')})")
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
