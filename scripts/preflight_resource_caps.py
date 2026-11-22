# SPDX-License-Identifier: Apache-2.0
"""A new or removed `MAX_*` constant updates the resource-cap census.

`docs/decode-resource-caps.md` lists every production `MAX_*` constant with its
kind. `scripts/check-resource-caps.py` checks the rows against the tree and
computes the counts, so the document stores none.
"""

from __future__ import annotations

import importlib.util
import re
import sys

from preflight import is_test_path

NAME = "resource-caps"
ORDER = 50

CENSUS = "docs/decode-resource-caps.md"
CHECKER = "scripts/check-resource-caps.py"
CONSTANT = re.compile(r"(?<![\w:])(?:pub(?:\s*\([^)]*\))?\s+)?const\s+(MAX_[A-Z0-9_]+)\b")


def removed_lines(context, path: str) -> list[str]:
    """Lines of the merge-base file that the tree under test no longer has."""
    arguments = ["diff", "--no-renames", "-U0", context.base]
    if context.rev:
        arguments.append(context.rev)
    diff = context.git(*arguments, "--", path).stdout
    return [line[1:] for line in diff.splitlines()
            if line.startswith("-") and not line.startswith("---")]


def changed_constants(context) -> tuple[list[str], list[str]]:
    """Production `MAX_*` constants the diff adds and removes, by name per file.

    A constant whose line changes but whose name stays in the file is neither.
    """
    added: list[str] = []
    removed: list[str] = []
    for path in context.changed_paths():
        if path.startswith("crates/") and path.endswith(".rs") and not is_test_path(path):
            new = [(line, match.group(1)) for line, text in context.added_lines(path)
                   if (match := CONSTANT.search(text))]
            old = [match.group(1) for text in removed_lines(context, path)
                   if (match := CONSTANT.search(text))]
            new_names = {name for _, name in new}
            added += [f"{path}:{line}: {name}" for line, name in new if name not in old]
            removed += [f"{path}: {name} removed" for name in old if name not in new_names]
    return added, removed


def load_checker(root):
    spec = importlib.util.spec_from_file_location("preflight_resource_caps_checker",
                                                  root / CHECKER)
    if spec is None or spec.loader is None:
        return None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def run(context):
    added, removed = changed_constants(context)
    changed = added + removed
    if not changed and CENSUS not in context.changed_paths():
        return context.passed("no MAX_* constant added or removed and the census is unchanged")
    if not context.worktree:
        return context.skipped("the census check reads the working tree; rerun without --rev",
                               *changed)
    if not (context.root / CHECKER).is_file():
        return context.skipped(f"{CHECKER} is not in this tree", *changed)
    checker = load_checker(context.root)
    constants, _rows, findings = checker.census(context.root)
    total = len(constants)
    if findings:
        return context.failed(
            f"resource-cap census disagrees with the tree ({total} production MAX_* constants)",
            *changed, *findings[:20],
            f"fix: add a row with its kind to {CENSUS} for each new constant and drop the row "
            f"of each removed one; then python3 {CHECKER}",
        )
    return context.passed(f"census matches {total} production MAX_* constants", *changed)
