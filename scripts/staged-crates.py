#!/usr/bin/env python3
"""Map staged files to the workspace crates the Rust gate must check.

Reads the staged path list from `git diff --cached --name-only`, maps each
path to the workspace crate that owns it (longest manifest-directory prefix),
and expands the set with every workspace crate that depends on an affected
crate, transitively. The pre-commit hook passes the result to
`cargo clippy -p ...` and `cargo test -p ...` so a leaf-crate commit does not
re-lint the whole workspace, while a change to a shared crate (for example
`cadmpeg-ir`) still gates every dependent codec.

Output: one crate name per line. Exit status 0 with the sentinel line
`WORKSPACE` when scoping is not safe (a staged Rust file is outside every
workspace crate). Empty output means no staged file belongs to any crate.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys


def run(args: list[str]) -> str:
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def main() -> int:
    repo_root = run(["git", "rev-parse", "--show-toplevel"]).strip()
    staged = [line for line in run(["git", "diff", "--cached", "--name-only"]).splitlines() if line]

    meta = json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
    # Crate directory (repo-relative, no trailing slash) -> package name.
    crate_dirs: dict[str, str] = {}
    for pkg in meta["packages"]:
        crate_dir = os.path.relpath(os.path.dirname(pkg["manifest_path"]), repo_root)
        crate_dirs[crate_dir] = pkg["name"]

    # Reverse dependencies among workspace members only.
    names = set(crate_dirs.values())
    rdeps: dict[str, set[str]] = {name: set() for name in names}
    for pkg in meta["packages"]:
        for dep in pkg["dependencies"]:
            if dep["name"] in names:
                rdeps[dep["name"]].add(pkg["name"])

    affected: set[str] = set()
    for path in staged:
        owner = None
        best = -1
        for crate_dir, name in crate_dirs.items():
            if (path.startswith(crate_dir + "/") or crate_dir == ".") and len(crate_dir) > best:
                owner = name
                best = len(crate_dir)
        if owner is not None:
            affected.add(owner)
        elif path.endswith(".rs"):
            # A Rust file outside every workspace crate: scoping is not safe.
            print("WORKSPACE")
            return 0

    # Transitive closure over reverse dependencies.
    frontier = list(affected)
    while frontier:
        for dependent in rdeps.get(frontier.pop(), ()):
            if dependent not in affected:
                affected.add(dependent)
                frontier.append(dependent)

    for name in sorted(affected):
        print(name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
