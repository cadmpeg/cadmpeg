#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Validate ``docs/public-api-ledger.toml`` without cargo-public-api.

CI does not install cargo-public-api. This checker confirms the ledger TOML
parses, required fields are present, every ``commit`` is a 40-character SHA
and a known git object when history is available, and each snapshot file under
``docs/api-baseline/`` exists and starts with ``# generated at``.

Regenerate snapshots with ``cargo +nightly public-api`` as documented in
``docs/api-baseline/README.md``.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs" / "public-api-ledger.toml"

REQUIRED_TOP = ("baseline_commit", "api_baseline_dir", "measured_at")
REQUIRED_CHANGE = ("commit", "crate", "kind", "item", "reason")
KINDS = frozenset(
    {
        "deletion",
        "move",
        "visibility",
        "signature",
        "trait_impl",
        "field",
        "variant",
    }
)
SHA = re.compile(r"^[0-9a-f]{40}$")
GENERATED = re.compile(r"^# generated at [0-9a-f]+$")


def parse_ledger(path: Path) -> dict[str, object]:
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    if not isinstance(data, dict):
        raise ValueError("ledger root must be a table")
    return data


def git_is_commit(sha: str) -> bool:
    try:
        out = subprocess.check_output(
            ["git", "cat-file", "-t", sha],
            cwd=ROOT,
            stderr=subprocess.DEVNULL,
        )
    except subprocess.CalledProcessError:
        return False
    return out.decode().strip() == "commit"


def git_is_shallow() -> bool:
    try:
        out = subprocess.check_output(
            ["git", "rev-parse", "--is-shallow-repository"],
            cwd=ROOT,
            stderr=subprocess.DEVNULL,
        )
    except subprocess.CalledProcessError:
        return False
    return out.decode().strip() == "true"


def collect_commit_fields(data: dict[str, object]) -> list[str]:
    shas: list[str] = []
    for key in ("baseline_commit", "audit_baseline_commit", "measured_at"):
        value = data.get(key)
        if isinstance(value, str):
            shas.append(value)
    changes = data.get("change", [])
    if isinstance(changes, list):
        for row in changes:
            if isinstance(row, dict):
                commit = row.get("commit")
                if isinstance(commit, str):
                    shas.append(commit)
    return shas


def check_shape(data: dict[str, object]) -> list[str]:
    failures: list[str] = []
    for key in REQUIRED_TOP:
        if key not in data:
            failures.append(f"ledger missing {key}")
    changes = data.get("change", [])
    if changes and not isinstance(changes, list):
        failures.append("ledger [[change]] is not an array")
        return failures
    if not isinstance(changes, list):
        changes = []
    for index, row in enumerate(changes):
        if not isinstance(row, dict):
            failures.append(f"change[{index}] is not a table")
            continue
        for key in REQUIRED_CHANGE:
            if key not in row or not isinstance(row[key], str) or not row[key]:
                failures.append(f"change[{index}] missing {key}")
        kind = row.get("kind")
        if isinstance(kind, str) and kind not in KINDS:
            failures.append(f"change[{index}] unknown kind {kind!r}")
    for sha in collect_commit_fields(data):
        if not SHA.fullmatch(sha):
            failures.append(f"commit {sha!r} is not a 40-character lowercase SHA")
    return failures


def check_snapshots(data: dict[str, object], root: Path) -> list[str]:
    failures: list[str] = []
    rel = data.get("api_baseline_dir")
    if not isinstance(rel, str) or not rel:
        return failures
    baseline = root / rel
    if not baseline.is_dir():
        failures.append(f"missing api baseline dir {rel}")
        return failures
    snapshots = sorted(baseline.glob("*.txt"))
    if not snapshots:
        failures.append(f"no snapshot .txt files under {rel}")
    for path in snapshots:
        text = path.read_text(encoding="utf-8")
        first = text.splitlines()[0] if text else ""
        if not GENERATED.fullmatch(first):
            failures.append(
                f"{path.relative_to(root)}: first line must be '# generated at <sha>'"
            )
    return failures


def check_git_objects(data: dict[str, object]) -> list[str]:
    failures: list[str] = []
    seen: set[str] = set()
    for sha in collect_commit_fields(data):
        if sha in seen or not SHA.fullmatch(sha):
            continue
        seen.add(sha)
        if not git_is_commit(sha):
            failures.append(f"commit {sha} is not a known git object")
    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--skip-git-objects",
        action="store_true",
        help="skip git object resolution (shallow clones)",
    )
    args = parser.parse_args(argv)

    if not LEDGER.is_file():
        print(f"error: missing ledger {LEDGER}", file=sys.stderr)
        return 2

    try:
        data = parse_ledger(LEDGER)
    except (OSError, tomllib.TOMLDecodeError, ValueError) as err:
        print(f"error: ledger parse failed: {err}", file=sys.stderr)
        return 2

    failures = check_shape(data)
    failures.extend(check_snapshots(data, ROOT))

    skip_git = args.skip_git_objects or git_is_shallow()
    if skip_git:
        print("public-api ledger: skipping git object check (shallow or requested)")
    else:
        failures.extend(check_git_objects(data))

    if failures:
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
        return 1
    n_changes = len(data.get("change", []) or [])
    print(f"public-api ledger: ok ({n_changes} change rows)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
