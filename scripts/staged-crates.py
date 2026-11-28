#!/usr/bin/env python3
"""Map staged files to the workspace crates that own them.

Reads the staged path list from `git diff --cached --name-only` and maps each
path to the workspace crate that owns it (longest manifest-directory prefix).
The pre-commit hook passes the result to `cargo fmt -p ...` and
`cargo clippy -p ...`. CI lints the dependents of a shared crate.

Output: one crate name per line. Exit status 0 with the sentinel line
`WORKSPACE` when scoping is not safe (a staged Rust file is outside every
workspace crate). The hook handles the separate fuzz workspace. Empty output
means no staged file belongs to a main-workspace crate.
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
    staged = [line for line in run(["git", "diff", "--cached", "--name-only"]).splitlines()
              if line and not line.startswith("crates/cadmpeg-fuzz/")]
    if not staged:
        return 0

    meta = json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
    # Crate directory (repo-relative, no trailing slash) -> package name.
    crate_dirs: dict[str, str] = {}
    for pkg in meta["packages"]:
        crate_dir = os.path.relpath(os.path.dirname(pkg["manifest_path"]), repo_root)
        crate_dirs[crate_dir] = pkg["name"]

    affected: set[str] = set()
    for path in staged:
        owner = None
        best = -1
        for crate_dir, name in crate_dirs.items():
            if path.startswith(crate_dir + "/") and len(crate_dir) > best:
                owner = name
                best = len(crate_dir)
        if owner is not None:
            affected.add(owner)
        elif path.endswith(".rs"):
            # A Rust file outside every workspace crate: scoping is not safe.
            print("WORKSPACE")
            return 0

    for name in sorted(affected):
        print(name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
