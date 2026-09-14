#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Census of a call spelling in non-test crate source.

With no argument the census is ``.expect(`` and ``.unwrap(``. ``--pattern`` takes
any Python regular expression and censuses that spelling over the same scope and
the same walk, so a clamp census and a panic census can never come from two
different scopes.

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

import argparse
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

PANIC_CALL = re.compile(r"\.\s*(?:expect|unwrap)\s*\(")
EXPECT_CALL = re.compile(r"\.\s*expect\s*\(")
BARE_UNWRAP = re.compile(r"\.\s*unwrap\s*\(\s*\)")
INCLUDE = re.compile(r'include!\s*\(\s*"([^"]+)"\s*\)')
SEED_GENERATOR = "crates/cadmpeg-fuzz/src/bin/generate_all_seeds.rs"


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
    original_lines = source.splitlines(keepends=True)
    children: list[tuple[Path, bool]] = []
    pending: list[str] = []
    blocks: list[tuple[str, int, bool]] = []
    depth = 0
    index = 0
    while index < len(lines):
        stripped = lines[index].lstrip()
        if stripped.startswith("#["):
            start = index
            _, index = SOURCE_POLICY.collect_attribute(lines, index)
            attribute = "".join(original_lines[start:index])
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
    code = SOURCE_POLICY.mask_rust_non_code(source)
    production, _ = SOURCE_POLICY.production_source(source)
    for included in INCLUDE.finditer(source):
        if not code[included.start():included.start() + len("include!")].strip():
            continue
        target = (path.parent / included.group(1)).resolve()
        if target.is_file():
            gated = not production[included.start():included.start() + len("include!")].strip()
            children.append((target, gated))
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
    return kept, gated - kept


def matching_lines(code: str, pattern: re.Pattern[str]) -> int:
    """Count distinct starting lines, including calls split across lines."""
    return len({code.count("\n", 0, match.start()) for match in pattern.finditer(code)})


def list_sites(relative: str, source: str, code: str, pattern: re.Pattern[str]) -> None:
    """Print each site's location and source, retaining its message expression."""
    for match in pattern.finditer(code):
        end = match.end()
        if code[end - 1:end] == "(":
            depth = 1
            while end < len(code) and depth:
                if code[end] == "(":
                    depth += 1
                elif code[end] == ")":
                    depth -= 1
                end += 1
        line = code.count("\n", 0, match.start()) + 1
        spelling = " ".join(source[match.start():end].splitlines())
        print(f"{relative}:{line}\t{spelling}")


def census_pattern(pattern: re.Pattern[str], label: str, listing: bool = False) -> int:
    """Print the census of one spelling over the crate source scope."""
    kept, gated = production_files()
    raw_sites = 0
    non_test_sites = 0
    unreached = 0
    buckets: dict[str, int] = {}
    files_in_scope = 0
    for path in scope():
        files_in_scope += 1
        source = path.read_text(encoding="utf-8")
        raw_sites += len(pattern.findall(source))
        resolved = path.resolve()
        if resolved not in kept:
            if resolved not in gated:
                unreached += 1
            else:
                continue
        code, _ = SOURCE_POLICY.production_source(source)
        relative = path.relative_to(ROOT).as_posix()
        found = len(pattern.findall(code))
        if found:
            non_test_sites += found
            buckets[relative] = found
            if listing:
                list_sites(relative, source, code, pattern)
    print(f"census: {label} in crate source")
    print(f"scope: crates/*/src/**/*.rs, {files_in_scope} files")
    print(f"files the module walk keeps as non-test: {len(kept)}")
    print(f"files the walk reaches only through a #[cfg(test)] mod: {len(gated)}")
    print(f"files in scope the walk never reaches: {unreached}")
    print(f"raw matching sites over the whole scope: {raw_sites}")
    print(f"non-test matching sites: {non_test_sites}")
    for relative, count in sorted(buckets.items()):
        print(f"  {relative} {count}")
    return 0


def census_panic_calls(listing: bool = False, check: bool = False) -> int:
    """Print the ``.expect(`` and ``.unwrap(`` census."""
    kept, gated = production_files()
    raw_lines = 0
    non_test_lines = 0
    expect_lines = 0
    unreached = 0
    unwrap_buckets: dict[str, int] = {}
    files_in_scope = 0
    remaining_calls = 0
    for path in scope():
        files_in_scope += 1
        source = path.read_text(encoding="utf-8")
        raw_lines += matching_lines(source, PANIC_CALL)
        resolved = path.resolve()
        if resolved not in kept:
            if resolved not in gated:
                unreached += 1
            else:
                continue
        code, _ = SOURCE_POLICY.production_source(source)
        relative = path.relative_to(ROOT).as_posix()
        non_test_lines += matching_lines(code, PANIC_CALL)
        expect_lines += matching_lines(code, EXPECT_CALL)
        bare = len(BARE_UNWRAP.findall(code))
        if bare:
            unwrap_buckets[relative] = bare
        if listing:
            list_sites(relative, source, code, PANIC_CALL)
        if relative != SEED_GENERATOR:
            remaining_calls += len(PANIC_CALL.findall(code))
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
    if check:
        print(f"check excludes the seed-generation tool: {SEED_GENERATOR}")
        print(f"remaining production panic calls: {remaining_calls}")
    return int(check and remaining_calls != 0)


def main() -> int:
    """Print the census and answer 0."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--pattern",
        help="census this regular expression instead of .expect( and .unwrap(",
    )
    parser.add_argument(
        "--label",
        help="the spelling the census names in its first line; defaults to --pattern",
    )
    parser.add_argument(
        "--list", action="store_true",
        help="print every counted file:line and call, including its message expression",
    )
    parser.add_argument(
        "--check", action="store_true",
        help="fail on production expect/unwrap calls, except the declared seed-generation tool",
    )
    arguments = parser.parse_args()
    if arguments.check and arguments.pattern is not None:
        parser.error("--check applies to expect/unwrap calls, not an arbitrary pattern")
    if arguments.pattern is None:
        return census_panic_calls(arguments.list, arguments.check)
    return census_pattern(
        re.compile(arguments.pattern), arguments.label or arguments.pattern, arguments.list
    )


if __name__ == "__main__":
    sys.exit(main())
