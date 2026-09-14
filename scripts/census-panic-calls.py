#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Census of ``.expect(`` and ``.unwrap(`` calls in non-test crate source.

The rule is this file, not prose. Test source is found by parsing, never by a
basename: the walk starts at every crate root (``src/lib.rs``, ``src/main.rs``
and ``src/bin/*.rs``), follows every file module the source declares -- including
one declared inside an inline ``mod`` block, whose directory the block names --
and marks a module test source when its own declaration carries ``#[cfg(test)]``,
when an enclosing inline ``mod`` block does, or when the module that declares it
is already test source. Inside a file that the walk kept, every ``#[cfg(test)]``
item is masked by attribute parsing and brace matching. An ``include!`` target
is followed the same way as a module. Comments and string literals are masked before counting, so a call
spelled inside one does not count.

Prints the raw line count over the whole scope, the same count after the
exclusions, and the per-file buckets of bare ``.unwrap()``.
"""

from __future__ import annotations

import importlib.util
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = Path(__file__).with_name("check-source-policy.py")
SPEC = importlib.util.spec_from_file_location("source_policy", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise SystemExit(f"cannot load {SCRIPT}")
SOURCE_POLICY = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SOURCE_POLICY
SPEC.loader.exec_module(SOURCE_POLICY)

PANIC_CALL = re.compile(r"\.(?:expect|unwrap)\(")
EXPECT_CALL = re.compile(r"\.expect\(")
BARE_UNWRAP = re.compile(r"\.unwrap\(\)")
INCLUDE = re.compile(r'include!\s*\(\s*"([^"]+)"\s*\)')


def scope() -> list[Path]:
    """Every ``.rs`` file under ``crates/*/src``, in path order."""
    return sorted(path for path in ROOT.glob("crates/*/src/**/*.rs"))


def crate_roots() -> list[Path]:
    """Every compilation root under ``crates/*/src``, in path order."""
    roots = []
    for crate in sorted(ROOT.glob("crates/*/src")):
        roots.extend(
            path for path in (crate / "lib.rs", crate / "main.rs") if path.is_file()
        )
        roots.extend(sorted(crate.glob("bin/*.rs")))
    return roots


def declared_modules(path: Path, source: str) -> list[tuple[Path, bool]]:
    """Each file module the source declares, with whether it is test-gated."""
    lines = SOURCE_POLICY.mask_rust_non_code(source).splitlines(keepends=True)
    children: list[tuple[Path, bool]] = []
    pending: list[str] = []
    blocks: list[tuple[str, int, bool]] = []
    depth = 0
    index = 0
    while index < len(lines):
        stripped = lines[index].lstrip()
        if stripped.startswith("#["):
            attribute, index = SOURCE_POLICY.collect_attribute(lines, index)
            pending.append(attribute)
            continue
        if not stripped.strip():
            index += 1
            continue
        declaration = SOURCE_POLICY.MOD_DECL.match(lines[index])
        gated = any(SOURCE_POLICY.attr_is_test_cfg(attribute) for attribute in pending)
        if declaration is not None and declaration.group(2) == ";":
            explicit = None
            for attribute in pending:
                explicit = explicit or SOURCE_POLICY.path_attr_target(attribute)
            directory = SOURCE_POLICY.child_module_dir(path)
            for name, _, _ in blocks:
                directory = directory / name
            target = SOURCE_POLICY.resolve_module_target(
                path, directory, declaration.group(1), explicit
            )
            if target is not None:
                enclosing = any(block_gated for _, _, block_gated in blocks)
                children.append((target.resolve(), gated or enclosing))
        elif declaration is not None:
            blocks.append((declaration.group(1), depth, gated))
        pending = []
        depth += lines[index].count("{") - lines[index].count("}")
        while blocks and depth <= blocks[-1][1]:
            blocks.pop()
        index += 1
    for included in INCLUDE.finditer(source):
        target = (path.parent / included.group(1)).resolve()
        if target.is_file():
            children.append((target, False))
    return children


def production_files() -> tuple[set[Path], set[Path]]:
    """Files the walk reaches outside a test module, and the ones it gates."""
    kept: set[Path] = set()
    gated: set[Path] = set()
    pending = [(root.resolve(), False) for root in crate_roots()]
    while pending:
        path, in_test = pending.pop()
        if path in (gated if in_test else kept):
            continue
        (gated if in_test else kept).add(path)
        source = path.read_text(encoding="utf-8")
        for child, child_gated in declared_modules(path, source):
            pending.append((child, in_test or child_gated))
    return kept - gated, gated


def main() -> int:
    """Print the census and answer 0."""
    kept, gated = production_files()
    raw_lines = 0
    non_test_lines = 0
    expect_lines = 0
    unreached = 0
    unwrap_buckets: dict[str, int] = {}
    files_in_scope = 0
    for path in scope():
        files_in_scope += 1
        source = path.read_text(encoding="utf-8")
        raw_lines += sum(1 for line in source.splitlines() if PANIC_CALL.search(line))
        resolved = path.resolve()
        if resolved not in kept:
            if resolved not in gated:
                unreached += 1
            continue
        code, _ = SOURCE_POLICY.production_source(source)
        relative = path.relative_to(ROOT).as_posix()
        for line in code.splitlines():
            if PANIC_CALL.search(line):
                non_test_lines += 1
            if EXPECT_CALL.search(line):
                expect_lines += 1
            bare = len(BARE_UNWRAP.findall(line))
            if bare:
                unwrap_buckets[relative] = unwrap_buckets.get(relative, 0) + bare
    print("census: .expect( and .unwrap( in crate source")
    print(f"scope: crates/*/src/**/*.rs, {files_in_scope} files")
    print(f"files the module walk keeps as non-test: {len(kept)}")
    print(f"files the walk reaches only through a #[cfg(test)] mod: {len(gated)}")
    print(f"files in scope the walk never reaches: {unreached}")
    print(f"raw matching lines over the whole scope: {raw_lines}")
    print(f"non-test matching lines: {non_test_lines}")
    print(f"non-test lines calling .expect(: {expect_lines}")
    print(f"non-test bare .unwrap() calls: {sum(unwrap_buckets.values())}")
    for relative, count in sorted(unwrap_buckets.items()):
        print(f"  {relative} {count}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
