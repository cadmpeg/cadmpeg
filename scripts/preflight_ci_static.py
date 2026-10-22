# SPDX-License-Identifier: Apache-2.0
"""CI's static checks and `cargo fmt --all --check` pass on the working tree.

The commands come from `.github/workflows/ci.yml`: every `python3` line of
the `static` job, and the `cargo fmt` line of the `lint` job, so the list
follows CI. The Python tooling tests run only when `scripts/` changed.
Formatting covers every crate, including ones the branch did not mean to
touch. None of these commands compiles Rust.
"""

from __future__ import annotations

import re
import shlex
from concurrent.futures import ThreadPoolExecutor

NAME = "ci-static"
ORDER = 60

WORKFLOW = ".github/workflows/ci.yml"
JOB = re.compile(r"^  ([\w-]+):\s*$")
COMMAND = re.compile(r"^\s*(?:-\s*)?(?:run:\s*)?((?:python3|cargo fmt)\s.*?)\s*$")


def job_commands(workflow: str, job: str, prefix: str) -> list[str]:
    """Single-line commands of one job that start with `prefix`."""
    commands = []
    current = None
    for line in workflow.splitlines():
        header = JOB.match(line)
        if header:
            current = header.group(1)
            continue
        if current != job:
            continue
        match = COMMAND.match(line)
        if match and match.group(1).startswith(prefix) and not match.group(1).endswith("\\"):
            commands.append(match.group(1))
    return commands


def commands(workflow: str, scripts_changed: bool) -> list[str]:
    found = job_commands(workflow, "static", "python3 ")
    if not scripts_changed:
        found = [command for command in found if "unittest" not in command]
    return found + job_commands(workflow, "lint", "cargo fmt")


def run(context):
    if not context.worktree:
        return context.skipped("CI's static checks read the working tree; rerun without --rev")
    workflow = context.read(WORKFLOW)
    if workflow is None:
        return context.skipped(f"no {WORKFLOW}")
    scripts_changed = any(path.startswith("scripts/") for path in context.changed_paths())
    selected = commands(workflow, scripts_changed)
    if not selected:
        return context.skipped(f"no static commands found in {WORKFLOW}")
    with ThreadPoolExecutor(max_workers=4) as pool:
        outcomes = list(pool.map(lambda command: context.run(shlex.split(command)), selected))
    results = []
    for command, (status, output) in zip(selected, outcomes):
        if status:
            fix = ("fix: cargo fmt --all" if command.startswith("cargo fmt")
                   else f"fix: read the findings above, then rerun {command}")
            results.append(context.failed(command, *output.splitlines()[-20:], fix))
    passed = len(selected) - len(results)
    if passed:
        results.append(context.passed(f"{passed} of {len(selected)} CI static command(s) pass"))
    return results
