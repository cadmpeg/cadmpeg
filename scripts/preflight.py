#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check a branch against the upstream default branch before a pull request.

Each check is a module ``scripts/preflight_<name>.py`` that defines ``NAME``,
``ORDER``, and ``run(context)``. This program loads every such module, runs the
checks against the merge base with the upstream default branch, and prints one
summary line per result:

    preflight: PASS  branch      up to date with origin/main
    preflight: FAIL  ir-version  IR_VERSION is 10 but 1 file(s) still pin "ir_version": "8"
      fix: ...

A FAIL is a problem that review or CI rejects. A WARN asks for a human
decision. The exit status is 1 when any check fails.

The upstream remote is the one whose URL names ``cadmpeg/cadmpeg``, or
``origin`` when no remote does. ``--upstream REF`` selects another base.

Usage: scripts/preflight.py [--upstream REF] [--rev REV] [--no-fetch]
                            [--full] [--run-layout] [--only NAME ...]
With no ``--rev`` it checks the working tree, including uncommitted and
untracked files. With ``--rev`` it checks that commit; checks that run
programs over the tree are skipped.
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = Path(__file__).resolve().parent
UPSTREAM_URL = re.compile(r"github\.com[:/]cadmpeg/cadmpeg(?:\.git)?/?$")
STATUSES = ("FAIL", "WARN", "PASS", "SKIP")
TEST_SEGMENTS = {"tests", "golden_tests", "integration_tests", "test_support", "benches"}
HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


@dataclass
class Result:
    status: str
    check: str
    summary: str
    details: list[str] = field(default_factory=list)


@dataclass
class Options:
    full: bool = False
    run_layout: bool = False
    offline: bool = False


class Context:
    """The tree under test, its merge base, and the upstream tip."""

    def __init__(self, root: Path, upstream: str, base: str, rev: str | None = None,
                 options: Options | None = None) -> None:
        self.root = root
        self.upstream = upstream
        self.base = base
        self.rev = rev
        self.options = options or Options()
        self.check = ""
        self._paths: list[str] | None = None

    # Results.
    def result(self, status: str, summary: str, *details: str) -> Result:
        return Result(status, self.check, summary, list(details))

    def passed(self, summary: str, *details: str) -> Result:
        return self.result("PASS", summary, *details)

    def failed(self, summary: str, *details: str) -> Result:
        return self.result("FAIL", summary, *details)

    def warned(self, summary: str, *details: str) -> Result:
        return self.result("WARN", summary, *details)

    def skipped(self, summary: str, *details: str) -> Result:
        return self.result("SKIP", summary, *details)

    # Git and processes.
    def git(self, *arguments: str, check: bool = False) -> subprocess.CompletedProcess:
        return subprocess.run(["git", *arguments], cwd=self.root, capture_output=True,
                              text=True, errors="replace", check=check)

    def run(self, command: list[str], env: dict[str, str] | None = None) -> tuple[int, str]:
        """Run a program in the tree root and return its status and combined output."""
        merged = {**os.environ, **(env or {})}
        process = subprocess.run(command, cwd=self.root, capture_output=True, text=True,
                                 errors="replace", env=merged)
        return process.returncode, (process.stdout + process.stderr).strip()

    @property
    def worktree(self) -> bool:
        return self.rev is None

    # Content.
    def changed_paths(self) -> list[str]:
        """Paths that differ between the merge base and the tree under test."""
        if self._paths is None:
            arguments = ["diff", "--no-renames", "--name-only", "-z", self.base]
            if self.rev:
                arguments.append(self.rev)
            paths = set(filter(None, self.git(*arguments, check=True).stdout.split("\0")))
            if self.worktree:
                untracked = self.git("ls-files", "--others", "--exclude-standard", "-z").stdout
                paths |= set(filter(None, untracked.split("\0")))
            self._paths = sorted(paths)
        return self._paths

    def show(self, revision: str, path: str) -> str | None:
        process = self.git("show", f"{revision}:{path}")
        return process.stdout if process.returncode == 0 else None

    def read(self, path: str) -> str | None:
        """The file in the tree under test, or None when it does not exist there."""
        if self.rev:
            return self.show(self.rev, path)
        file = self.root / path
        if not file.is_file():
            return None
        return file.read_text(errors="replace")

    def read_base(self, path: str) -> str | None:
        return self.show(self.base, path)

    def read_upstream(self, path: str) -> str | None:
        return self.show(self.upstream, path)

    def added_lines(self, path: str) -> list[tuple[int, str]]:
        """Added lines of a changed file as (line number, text) in the new file."""
        arguments = ["diff", "--no-renames", "-U0", self.base]
        if self.rev:
            arguments.append(self.rev)
        diff = self.git(*arguments, "--", path).stdout
        if not diff and self.read_base(path) is None:
            text = self.read(path) or ""
            return list(enumerate(text.splitlines(), 1))
        added: list[tuple[int, str]] = []
        number = 0
        for line in diff.splitlines():
            hunk = HUNK.match(line)
            if hunk:
                number = int(hunk.group(1))
            elif line.startswith("+") and not line.startswith("+++"):
                added.append((number, line[1:]))
                number += 1
        return added


def is_test_path(path: str) -> bool:
    """Whether a Rust path holds only test code by its location."""
    parts = Path(path).parts
    stem = Path(path).stem
    return (bool(TEST_SEGMENTS & set(parts[:-1])) or stem in TEST_SEGMENTS
            or stem.endswith("_tests"))


def crate_of(path: str) -> str | None:
    """The crate directory name of a path under crates/, or None."""
    parts = Path(path).parts
    return parts[1] if len(parts) > 2 and parts[0] == "crates" else None


def upstream_remote(root: Path) -> str:
    process = subprocess.run(["git", "remote", "-v"], cwd=root, capture_output=True, text=True)
    for line in process.stdout.splitlines():
        fields = line.split()
        if len(fields) >= 2 and UPSTREAM_URL.search(fields[1]):
            return fields[0]
    return "origin"


def default_branch(root: Path, remote: str) -> str:
    process = subprocess.run(["git", "symbolic-ref", "--short", f"refs/remotes/{remote}/HEAD"],
                             cwd=root, capture_output=True, text=True)
    name = process.stdout.strip()
    return name if process.returncode == 0 and name else f"{remote}/main"


def load_checks(directory: Path = SCRIPTS) -> list:
    """Import every preflight_<name>.py module in ORDER."""
    modules = []
    for path in sorted(directory.glob("preflight_*.py")):
        spec = importlib.util.spec_from_file_location(path.stem, path)
        if spec is None or spec.loader is None:
            continue
        module = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = module
        spec.loader.exec_module(module)
        if hasattr(module, "run") and hasattr(module, "NAME"):
            modules.append(module)
    return sorted(modules, key=lambda module: (getattr(module, "ORDER", 100), module.NAME))


def run_checks(context: Context, modules: list, only: set[str] | None = None) -> list[Result]:
    results: list[Result] = []
    for module in modules:
        if only and module.NAME not in only:
            continue
        context.check = module.NAME
        try:
            outcome = module.run(context)
        except Exception as error:  # A broken check must not hide the others.
            outcome = context.failed(f"check raised {type(error).__name__}: {error}")
        if isinstance(outcome, Result):
            outcome = [outcome]
        results.extend(outcome or [context.passed("nothing to check")])
    return results


def report(results: list[Result], out=sys.stdout) -> int:
    width = max((len(result.check) for result in results), default=0)
    for result in results:
        print(f"preflight: {result.status}  {result.check:<{width}}  {result.summary}", file=out)
        for detail in result.details:
            print(f"  {detail}", file=out)
    counts = {status: sum(result.status == status for result in results) for status in STATUSES}
    print("preflight: " + ", ".join(f"{counts[status]} {status}" for status in STATUSES), file=out)
    return 1 if counts["FAIL"] else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", type=Path, default=ROOT,
                        help="repository root to check (default: this checkout)")
    parser.add_argument("--upstream", help="base ref (default: the upstream default branch)")
    parser.add_argument("--rev", help="check this commit instead of the working tree")
    parser.add_argument("--no-fetch", action="store_true", help="do not fetch the upstream branch")
    parser.add_argument("--offline", action="store_true", help="no network: no fetch, no GitHub")
    parser.add_argument("--full", action="store_true", help="also run CI's Cargo lint gate")
    parser.add_argument("--run-layout", action="store_true",
                        help="regenerate and test the layout tables when they changed")
    parser.add_argument("--only", nargs="+", metavar="NAME", help="run only these checks")
    parser.add_argument("--list", action="store_true", help="list the checks and exit")
    args = parser.parse_args(argv)

    modules = load_checks()
    if args.list:
        for module in modules:
            print(f"{module.NAME}: {(module.__doc__ or '').strip().splitlines()[0]}")
        return 0
    root = args.repo.resolve()
    upstream = args.upstream
    if upstream is None:
        remote = upstream_remote(root)
        upstream = default_branch(root, remote)
        if not (args.no_fetch or args.offline):
            branch = upstream.split("/", 1)[1]
            fetch = subprocess.run(["git", "fetch", "-q", remote, branch], cwd=root,
                                   capture_output=True, text=True)
            if fetch.returncode:
                print(f"preflight: could not fetch {remote} {branch}; using the local "
                      f"{upstream}", file=sys.stderr)
    merge_base = subprocess.run(["git", "merge-base", upstream, args.rev or "HEAD"], cwd=root,
                                capture_output=True, text=True)
    if merge_base.returncode:
        print(f"preflight: no merge base with {upstream}: {merge_base.stderr.strip()}",
              file=sys.stderr)
        return 2
    options = Options(full=args.full, run_layout=args.run_layout, offline=args.offline)
    context = Context(root, upstream, merge_base.stdout.strip(), args.rev, options)
    print(f"preflight: {args.rev or 'working tree'} against {upstream} "
          f"(merge base {context.base[:10]}, {len(context.changed_paths())} changed paths)")
    return report(run_checks(context, modules, set(args.only) if args.only else None))


if __name__ == "__main__":
    raise SystemExit(main())
