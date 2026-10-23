# SPDX-License-Identifier: Apache-2.0
"""A change to a shared crate runs the golden tests of every crate that depends on it.

A change to `cadmpeg-ir`, `cadmpeg-core`, `cadmpeg-asm`, or another crate that
codecs build on can change the goldens of a codec the branch never touched,
for example a body kind that the SAT goldens pin. Running only the changed
crate's tests misses that drift, and CI then fails. The check selects the
reached crates the same way CI does (`scripts/ci_scope.py`) and lists the
dependents that have golden tests. With `--full` it runs them.
"""

from __future__ import annotations

import importlib.util
import shlex
import sys
import tomllib
from pathlib import Path

NAME = "dependent-goldens"
ORDER = 75

SCOPE = "scripts/ci_scope.py"
GOLDEN_MARKERS = ("src/golden_tests.rs", "src/golden_tests", "tests/golden")


def packages(root: Path) -> dict[str, str]:
    """Package name -> crate directory, for crates/*."""
    found = {}
    for manifest in sorted(root.glob("crates/*/Cargo.toml")):
        name = tomllib.loads(manifest.read_text()).get("package", {}).get("name")
        if name:
            found[name] = manifest.parent.relative_to(root).as_posix()
    return found


def has_goldens(root: Path, directory: str) -> bool:
    return any((root / directory / marker).exists() for marker in GOLDEN_MARKERS)


def reached(context) -> list[str] | None:
    spec = importlib.util.spec_from_file_location("preflight_dependent_scope",
                                                  context.root / SCOPE)
    if spec is None or spec.loader is None:
        return None
    scope = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = scope
    spec.loader.exec_module(scope)
    selection = scope.select(context.root, context.changed_paths(),
                             base_lock=context.read_base("Cargo.lock"))
    return selection.get("packages", []) if selection.get("rust") else []


def run(context):
    if not context.worktree:
        return context.skipped("the crate graph is read from the working tree; rerun without --rev")
    if not (context.root / SCOPE).is_file():
        return context.skipped(f"no {SCOPE}")
    directories = packages(context.root)
    changed = {name for name, directory in directories.items()
               if any(path.startswith(directory + "/") for path in context.changed_paths())}
    manifests = [path for path in context.changed_paths()
                 if path.rsplit("/", 1)[-1] in {"Cargo.toml", "Cargo.lock"}]
    if not changed and not manifests:
        return context.passed("no crate or manifest changed")
    dependents = sorted(name for name in (reached(context) or [])
                        if name not in changed and name in directories
                        and has_goldens(context.root, directories[name]))
    if not dependents:
        return context.passed("no other crate's goldens depend on the changed crates")
    command = ["cargo", "test", "-q", "--no-fail-fast",
               *(argument for name in dependents for argument in ("-p", name)), "golden"]
    shown = " ".join(shlex.quote(part) for part in command)
    changed_list = ", ".join(sorted(changed)) or "shared inputs"
    if not context.options.full:
        return context.warned(
            f"changes to {changed_list} reach the goldens of {len(dependents)} other crate(s)",
            *dependents, f"run: {shown}", "or rerun preflight with --full",
        )
    status, output = context.run(command)
    if status:
        return context.failed(
            f"golden tests of dependent crates fail after changing {changed_list}",
            *output.splitlines()[-25:],
            "fix: if the new output is intended, regenerate with UPDATE_GOLDEN=1 and the same "
            "command, review every changed snapshot, and commit it; otherwise fix the change",
        )
    return context.passed(f"goldens of {len(dependents)} dependent crate(s) pass", *dependents)
