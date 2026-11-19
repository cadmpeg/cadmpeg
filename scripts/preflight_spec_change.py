# SPDX-License-Identifier: Apache-2.0
"""A branch that edits the specification outside the generated Annex A changes no crate.

`scripts/check-spec-change.py` states the rule; CI runs it on each pull
request against the pull request's base.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

NAME = "spec-change"
ORDER = 46

_RULE = importlib.util.spec_from_file_location(
    "check_spec_change", Path(__file__).with_name("check-spec-change.py"))
assert _RULE and _RULE.loader
rule = importlib.util.module_from_spec(_RULE)
_RULE.loader.exec_module(rule)


def run(context):
    changed = context.changed_paths()
    if rule.SPEC not in changed:
        return context.passed(f"{rule.SPEC} is unchanged")
    crates = rule.crate_changes(changed, context.read_base(rule.SPEC), context.read(rule.SPEC))
    if not crates:
        return context.passed(f"no crate file changes with an edit of {rule.SPEC} "
                              "outside the generated Annex A body")
    return context.failed(f"{rule.SPEC} changes outside the generated Annex A with "
                          f"{len(crates)} crate file(s)", *crates[:20], f"fix: {rule.MESSAGE}")
