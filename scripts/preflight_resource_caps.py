# SPDX-License-Identifier: Apache-2.0
"""Check source-local resource-cap classifications."""

from __future__ import annotations

import importlib.util
import sys

NAME = "resource-caps"
ORDER = 50

CHECKER = "scripts/check-resource-caps.py"
DEPENDENCIES = {CHECKER, "scripts/check-source-policy.py", "docs/decode-resource-caps.md"}


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
    paths = context.changed_paths()
    if not any(path in DEPENDENCIES or path.startswith("crates/") and path.endswith(".rs")
               for path in paths):
        return context.passed("no resource-cap inputs changed")
    if not context.worktree:
        return context.skipped("the resource-cap check reads the working tree; rerun without --rev")
    if not (context.root / CHECKER).is_file():
        return context.skipped(f"{CHECKER} is not in this tree")
    checker = load_checker(context.root)
    constants, findings = checker.census(context.root)
    if findings:
        return context.failed(
            f"resource-cap classifications disagree with the tree ({len(constants)} production MAX_* constants)",
            *findings[:20],
            f"fix: update the resource-cap markers, then run python3 {CHECKER}",
        )
    return context.passed(f"classifications match {len(constants)} production MAX_* constants")
