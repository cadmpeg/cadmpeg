# SPDX-License-Identifier: Apache-2.0
"""With `--full`, CI's clippy gate passes for the crates the change reaches.

CI selects the changed crates and their workspace dependents with
`scripts/ci_scope.py` and runs the `lint` job's `ci_cargo.py clippy` command
over them, so a lint in a crate the branch did not mean to touch still fails
the PR. When the selection reaches `cadmpeg-ir`, `cadmpeg-core`, or
`cadmpeg-asm`, CI also runs the schema feature gate. This check reads both
commands from `.github/workflows/ci.yml` and runs them with the same
selection. Without `--full` it prints the commands.
"""

from __future__ import annotations

import importlib.util
import json
import re
import shlex
import sys

NAME = "clippy"
ORDER = 70

WORKFLOW = ".github/workflows/ci.yml"
SCOPE = "scripts/ci_scope.py"
CLIPPY = re.compile(r"^\s*(?:run:\s*)?(python3 scripts/ci_cargo\.py clippy\b.*?)\s*$", re.MULTILINE)
STEP = re.compile(r"^\s*- name:\s*(.*?)\s*$")


def schema_commands(workflow: str) -> list[str]:
    """The cargo commands of the "Schema feature" step, continuation lines joined."""
    commands: list[str] = []
    inside = False
    pending = ""
    for line in workflow.splitlines():
        step = STEP.match(line)
        if step:
            inside = step.group(1) == "Schema feature"
            continue
        if not inside:
            continue
        text = line.strip()
        if pending or text.startswith("cargo "):
            pending += " " + text.rstrip("\\").strip()
            if not text.endswith("\\"):
                commands.append(" ".join(pending.split()))
                pending = ""
    return commands


def selection(context) -> dict | None:
    spec = importlib.util.spec_from_file_location("preflight_ci_scope", context.root / SCOPE)
    if spec is None or spec.loader is None:
        return None
    scope = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = scope
    spec.loader.exec_module(scope)
    lock = context.read_base("Cargo.lock")
    return scope.select(context.root, context.changed_paths(), base_lock=lock)


def run(context):
    if not context.worktree:
        return context.skipped("clippy builds the working tree; rerun without --rev")
    workflow = context.read(WORKFLOW) or ""
    clippy = CLIPPY.search(workflow)
    if not clippy or not (context.root / SCOPE).is_file():
        return context.skipped(f"no ci_cargo.py clippy command in {WORKFLOW}")
    scope = selection(context)
    if not scope or not scope.get("rust"):
        return context.passed("the change reaches no Rust crate")
    packages = scope["packages"]
    env = {"CI_PACKAGES": json.dumps(packages), "CI_FULL": "true" if scope["full"] else "false",
           "CI_CODEC_FEATURES": json.dumps(scope.get("codec_scope", []))}
    commands = [clippy.group(1)]
    if scope.get("schema"):
        commands += schema_commands(workflow)
    shown = [f"CI_PACKAGES='{env['CI_PACKAGES']}' CI_FULL={env['CI_FULL']} "
             f"CI_CODEC_FEATURES='{env['CI_CODEC_FEATURES']}' {commands[0]}",
             *commands[1:]]
    reach = "the whole workspace" if scope["full"] else f"{len(packages)} crate(s)"
    if not context.options.full:
        return context.skipped(f"CI lints {reach}; rerun with --full to build them",
                               *(f"run: {command}" for command in shown))
    for command in commands:
        status, output = context.run(shlex.split(command), env=env)
        if status:
            return context.failed(f"{command} fails over {reach}", *output.splitlines()[-25:],
                                  "fix: correct the lints, including in crates the branch "
                                  "reaches but did not change")
    return context.passed(f"CI's clippy gate passes over {reach}", *packages[:30])
