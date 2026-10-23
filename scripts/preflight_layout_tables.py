# SPDX-License-Identifier: Apache-2.0
"""Layout-table pages and constants are regenerated from their tables, not edited.

`docs/layouts/<format>.toml` is the source. The rendered `<format>.md` and the
owning crate's `src/layout.rs` are generated from it (docs/layouts/README.md).
A rebase that conflicts in the derived files is resolved by keeping both
table records and regenerating, never by merging the derived text by hand.

Without `--run-layout` the check compares which of the three files changed
and prints the regeneration commands. With `--run-layout` it regenerates
both derived files, reports what regeneration changed, and runs the
`layout_tables` test. That builds the `cadmpeg` test target. A derived file
that changed without its table passes when regeneration reproduces it byte
for byte.
"""

from __future__ import annotations

import re
from pathlib import PurePosixPath

NAME = "layout-tables"
ORDER = 40

GENERATOR = "crates/cadmpeg/tests/layout_tables/generate.rs"
MAPPING = re.compile(r'\("(\w+)",\s*"([^"]+/layout\.rs)"\)')
CONFLICT = re.compile(r"^(<{7}|={7}|>{7})( |$)", re.MULTILINE)
TEST = ["cargo", "test", "-q", "-p", "cadmpeg", "--test", "layout_tables"]
REGENERATE = ("UPDATE_LAYOUT_DOCS=1 UPDATE_LAYOUT_CODE=1 "
              "cargo test -p cadmpeg --test layout_tables")


def generated_code(context) -> dict[str, str]:
    """Format name -> generated layout.rs path, from the layout test's own list."""
    return dict(MAPPING.findall(context.read(GENERATOR) or ""))


def run(context):
    code = generated_code(context)
    owners = {path: name for name, path in code.items()}
    changed = context.changed_paths()
    tables, pages, sources = set(), set(), set()
    for path in changed:
        pure = PurePosixPath(path)
        if path.startswith("docs/layouts/") and pure.suffix == ".toml":
            tables.add(pure.stem)
        elif path.startswith("docs/layouts/") and pure.suffix == ".md" and pure.stem != "README":
            pages.add(pure.stem)
        elif path in owners:
            sources.add(owners[path])
    if not (tables or pages or sources):
        return context.passed("no layout table or derived file changed")
    conflicts = []
    for name in sorted(tables | pages | sources):
        for path in (f"docs/layouts/{name}.toml", f"docs/layouts/{name}.md", code.get(name)):
            if path and CONFLICT.search(context.read(path) or ""):
                conflicts.append(f"{path}: conflict markers")
    if conflicts:
        return context.failed("layout files contain conflict markers", *conflicts)
    problems = []
    for name in sorted(tables - pages):
        problems.append(f"docs/layouts/{name}.toml changed but docs/layouts/{name}.md did not")
    for name in sorted((pages | sources) - tables):
        edited = [path for path in (f"docs/layouts/{name}.md", code.get(name))
                  if path in changed]
        problems.append(f"{', '.join(edited)} changed without docs/layouts/{name}.toml; "
                        "derived files are never edited by hand")
    if problems and not (context.options.run_layout and context.worktree):
        disagree = context.failed("layout tables and derived files disagree", *problems,
                                  "fix: keep every table record in the .toml, then run "
                                  f"{REGENERATE} and commit the result",
                                  "or rerun preflight with --run-layout: a derived file "
                                  "that regenerates byte for byte passes")
        if not context.options.run_layout:
            return disagree
        return [disagree, context.skipped("--run-layout needs the working tree, not --rev")]
    if not context.options.run_layout:
        return context.warned(
            f"layout table(s) changed: {', '.join(sorted(tables))}; regeneration not verified",
            f"verify: {REGENERATE}, then git diff, then {' '.join(TEST)}",
            "or rerun preflight with --run-layout",
        )
    if not context.worktree:
        return context.skipped("--run-layout needs the working tree, not --rev")
    derived = [path for name in sorted(code) for path in (f"docs/layouts/{name}.md", code[name])]
    before = context.git("diff", "--", *derived).stdout
    status, output = context.run(TEST, env={"UPDATE_LAYOUT_DOCS": "1", "UPDATE_LAYOUT_CODE": "1"})
    after = context.git("diff", "--", *derived).stdout
    if status:
        return context.failed("layout regeneration failed", *output.splitlines()[-15:])
    status, output = context.run(TEST)
    if status:
        return context.failed("layout_tables test fails after regeneration",
                              *output.splitlines()[-15:])
    if before != after:
        rewritten = context.git("diff", "--name-only", "--", *derived).stdout.split()
        return context.failed("regeneration changed derived files; review and commit them",
                              *rewritten)
    return context.passed("derived layout files match a regeneration; layout_tables passes")
