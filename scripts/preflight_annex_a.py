# SPDX-License-Identifier: Apache-2.0
"""Annex A of the specification is generated from the schema, never edited.

`scripts/generate-annex-a.py --check` builds the `emit_schema` example of
`cadmpeg-ir` and compares the annex with the schema. The check runs when the
specification or the generator changed, or when a changed source of
`cadmpeg-ir` or `cadmpeg-core` declares a type the annex states (its `### `
headings). CI's schema gate runs it on every change to those crates.
"""

from __future__ import annotations

import re

NAME = "annex-a"
ORDER = 45

SPEC = "docs/cadir-v11.md"
INPUTS = {SPEC, "scripts/generate-annex-a.py"}
SOURCES = ("crates/cadmpeg-ir/src/", "crates/cadmpeg-core/src/")
ANNEX = re.compile(r"^## Annex A\.", re.MULTILINE)
ENTRY = re.compile(r"^### `(\w+)`$", re.MULTILINE)
CHECK = ["python3", "scripts/generate-annex-a.py", "--check"]
WRITE = "python3 scripts/generate-annex-a.py --write"


def annex_types(spec: str) -> set[str]:
    start = ANNEX.search(spec)
    return set(ENTRY.findall(spec, start.end())) if start else set()


def declares(text: str, names: set[str]) -> list[str]:
    found = re.findall(r"\b(?:struct|enum|type)\s+(\w+)", text)
    return sorted(set(found) & names)


def run(context):
    changed = context.changed_paths()
    reasons = sorted(INPUTS & set(changed))
    names = annex_types(context.read(SPEC) or "")
    for path in changed:
        if path.startswith(SOURCES) and path.endswith(".rs"):
            reasons += [f"{path} declares `{name}`" for name in declares(context.read(path) or "", names)]
    if not reasons:
        return context.passed("no change reaches Annex A")
    if not context.worktree:
        return context.skipped("the Annex A check builds the schema; it needs the working tree, not --rev",
                               *reasons)
    status, output = context.run(CHECK)
    if status == 0:
        return context.passed("Annex A states the schema", *reasons)
    return context.failed("Annex A differs from the schema" if status == 1 else "the Annex A check failed",
                          *reasons, *output.splitlines()[-20:], f"fix: run {WRITE} and commit the result")
