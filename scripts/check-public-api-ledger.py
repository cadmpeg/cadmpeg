#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Validate ``docs/public-api-ledger.toml`` and its API snapshots.

This checker confirms the ledger TOML parses, required fields are present,
every ``commit`` is a 40-character SHA and a known git object when history is
available, and each snapshot file under ``docs/api-baseline/`` exists and
starts with ``# generated at``.

Regenerate snapshots with ``cargo +nightly public-api`` as documented in
``docs/api-baseline/README.md``.

Run ``--self-test`` to execute the unit tests in
``scripts/test_check_public_api_ledger.py``. Run ``--diff`` to regenerate and
compare snapshots for crates whose source is staged, or all snapshot crates
when the index is empty. Add ``--require-tooling`` to fail instead of skipping
the diff when nightly cargo-public-api is unavailable.
"""

from __future__ import annotations

import argparse
import difflib
import re
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs" / "public-api-ledger.toml"
SELF_TEST_REL = Path("scripts") / "test_check_public_api_ledger.py"

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
ADDED_CHANGE = re.compile(r"^\+\s*\[\[change\]\]\s*(?:#.*)?$")
ADDED_CRATE = re.compile(r'^\+\s*crate\s*=\s*"([^"]+)"\s*(?:#.*)?$')
ADDED_TABLE = re.compile(r"^\+\s*\[+[^]]+\]+")
CRATE_SOURCE = re.compile(r"^crates/([^/]+)/src(?:/|$)")
DIFF_LINE_LIMIT = 60


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


def added_change_crates(staged_diff: str) -> set[str]:
    """Return crates named by added breaking-change rows in a git diff."""
    crates: set[str] = set()
    in_added_change = False
    crate: str | None = None

    def finish() -> None:
        if crate is not None:
            crates.add(crate)

    for line in staged_diff.splitlines():
        if ADDED_CHANGE.fullmatch(line):
            if in_added_change:
                finish()
            in_added_change = True
            crate = None
            continue
        if line.startswith("+") and not line.startswith("+++"):
            if ADDED_TABLE.match(line):
                if in_added_change:
                    finish()
                in_added_change = False
                continue
            if in_added_change:
                if match := ADDED_CRATE.fullmatch(line):
                    crate = match.group(1)
            continue
        if in_added_change:
            finish()
            in_added_change = False
    if in_added_change:
        finish()
    return crates


def regen_command(crate: str) -> str:
    """Return the documented one-line command for one snapshot."""
    return (
        "SHORT=$(git rev-parse --short HEAD); "
        f"cargo +nightly public-api -p {crate} --color never -sss "
        f'| {{ echo "# generated at $SHORT"; cat; }} '
        f"> docs/api-baseline/{crate}.txt"
    )


def check_staged_coupling(staged_diff: str, staged_paths: set[str]) -> list[str]:
    """Require each breaking-change row to have a staged snapshot."""
    failures: list[str] = []
    for crate in sorted(added_change_crates(staged_diff)):
        snapshot = f"docs/api-baseline/{crate}.txt"
        if snapshot not in staged_paths:
            failures.append(
                f"{crate}: staged [[change]] row requires staged {snapshot}; "
                f"regenerate with: {regen_command(crate)}"
            )
    return failures


def staged_git_state(root: Path) -> tuple[set[str], str] | None:
    """Return staged paths and the staged ledger diff, or None outside git."""
    try:
        inside = subprocess.run(
            ["git", "rev-parse", "--is-inside-work-tree"],
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            check=False,
            text=True,
        )
    except OSError:
        return None
    if inside.returncode != 0 or inside.stdout.strip() != "true":
        return None
    names = subprocess.run(
        ["git", "diff", "--cached", "--name-only", "-z"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if names.returncode != 0:
        raise RuntimeError("cannot list staged paths")
    staged_paths = {
        item.decode("utf-8", errors="surrogateescape")
        for item in names.stdout.split(b"\0")
        if item
    }
    if not staged_paths:
        return set(), ""
    diff = subprocess.run(
        [
            "git",
            "diff",
            "--cached",
            "--unified=0",
            "--",
            "docs/public-api-ledger.toml",
        ],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
        text=True,
    )
    if diff.returncode != 0:
        raise RuntimeError("cannot read staged public API ledger diff")
    return staged_paths, diff.stdout


def strip_generated_header(snapshot: bytes) -> bytes:
    """Remove one generated-at header line from snapshot bytes."""
    first, separator, remainder = snapshot.partition(b"\n")
    header = first.rstrip(b"\r").decode("ascii", errors="ignore")
    if separator and GENERATED.fullmatch(header):
        return remainder
    return snapshot


def snapshot_matches(snapshot: bytes, generated: bytes) -> bool:
    """Compare generated API bytes with header-free snapshot content."""
    return strip_generated_header(snapshot) == generated


def crates_for_diff(snapshot_dir: Path, staged_paths: set[str]) -> list[str]:
    """Select staged source crates, or every snapshot crate for an empty index."""
    snapshot_crates = {path.stem for path in snapshot_dir.glob("*.txt")}
    if not staged_paths:
        return sorted(snapshot_crates)
    changed = {
        match.group(1)
        for path in staged_paths
        if (match := CRATE_SOURCE.match(path)) is not None
    }
    return sorted(snapshot_crates & changed)


def public_api_available(root: Path) -> bool:
    """Return whether nightly cargo-public-api can start."""
    try:
        probe = subprocess.run(
            ["cargo", "+nightly", "public-api", "--version"],
            cwd=root,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    except OSError:
        return False
    return probe.returncode == 0


def bounded_api_diff(crate: str, expected: bytes, actual: bytes) -> list[str]:
    """Return at most ``DIFF_LINE_LIMIT`` unified-diff lines."""
    lines = list(
        difflib.unified_diff(
            expected.decode("utf-8", errors="replace").splitlines(),
            actual.decode("utf-8", errors="replace").splitlines(),
            fromfile=f"docs/api-baseline/{crate}.txt (checked in)",
            tofile=f"{crate} (generated)",
            lineterm="",
        )
    )
    if len(lines) > DIFF_LINE_LIMIT:
        return lines[:DIFF_LINE_LIMIT] + [
            f"... diff truncated after {DIFF_LINE_LIMIT} lines"
        ]
    return lines


def check_api_diff(
    root: Path, staged_paths: set[str], require_tooling: bool = False
) -> tuple[list[str], bool]:
    """Regenerate selected APIs and return failures and a tooling-skip flag."""
    snapshot_dir = root / "docs" / "api-baseline"
    crates = crates_for_diff(snapshot_dir, staged_paths)
    if not crates:
        return [], False
    if not public_api_available(root):
        if require_tooling:
            return ["public API diff requires nightly cargo-public-api"], False
        return [], True
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="cadmpeg-public-api-") as tmp:
        temp_root = Path(tmp)
        for crate in crates:
            generated_path = temp_root / f"{crate}.txt"
            with generated_path.open("wb") as output:
                result = subprocess.run(
                    [
                        "cargo",
                        "+nightly",
                        "public-api",
                        "-p",
                        crate,
                        "--color",
                        "never",
                        "-sss",
                    ],
                    cwd=root,
                    stdout=output,
                    stderr=subprocess.PIPE,
                    check=False,
                )
            if result.returncode != 0:
                detail = result.stderr.decode("utf-8", errors="replace").strip()
                failures.append(f"{crate}: public API generation failed: {detail}")
                continue
            snapshot = (snapshot_dir / f"{crate}.txt").read_bytes()
            generated = generated_path.read_bytes()
            if snapshot_matches(snapshot, generated):
                continue
            failures.append(f"{crate}: checked-in public API snapshot is stale")
            failures.extend(
                bounded_api_diff(crate, strip_generated_header(snapshot), generated)
            )
            failures.append(f"regenerate with: {regen_command(crate)}")
    return failures, False


def self_test() -> int:
    """Run this checker's unit tests."""
    suite = unittest.defaultTestLoader.discover(
        start_dir=str(ROOT / "scripts"),
        pattern=SELF_TEST_REL.name,
        top_level_dir=str(ROOT / "scripts"),
    )
    result = unittest.TextTestRunner(verbosity=1).run(suite)
    return 0 if result.wasSuccessful() else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--skip-git-objects",
        action="store_true",
        help="skip git object resolution (shallow clones)",
    )
    parser.add_argument(
        "--diff",
        action="store_true",
        help="regenerate and compare APIs for staged source crates",
    )
    parser.add_argument(
        "--require-tooling",
        action="store_true",
        help="fail a requested API diff when nightly cargo-public-api is unavailable",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the unit tests instead of checking the ledger",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()

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

    staged_paths: set[str] = set()
    try:
        staged = staged_git_state(ROOT)
    except RuntimeError as err:
        failures.append(f"staged public API check failed: {err}")
    else:
        if staged is not None:
            staged_paths, staged_diff = staged
            failures.extend(check_staged_coupling(staged_diff, staged_paths))

    if args.diff:
        diff_failures, tooling_skipped = check_api_diff(
            ROOT, staged_paths, require_tooling=args.require_tooling
        )
        failures.extend(diff_failures)
        if tooling_skipped:
            print("warning: public API diff skipped: nightly cargo-public-api is unavailable")

    if failures:
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
        return 1
    n_changes = len(data.get("change", []) or [])
    print(f"public-api ledger: ok ({n_changes} change rows)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
