#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Census of a call spelling in non-test crate source.

``--check`` fails on any production ``.expect(``, ``.unwrap(``, ``panic!`` or
``unreachable!``. A ``panic!`` or ``unreachable!`` inside a ``const``
initializer is a compile-time refusal of a source literal, not a runtime panic
route, and is not counted; neither is a crate listed in ``TEST_ONLY_CRATES``,
each with the reason it is outside the check.

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
or bare ``#[test]`` item is masked by attribute parsing and brace matching. An ``include!`` target
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
PANIC_MACRO = re.compile(r"(?<![\w:])(?:panic|unreachable)\s*!")
CONST_ITEM = re.compile(r"(?<![\w:])const\s+(?:[A-Za-z_]\w*|_)\s*:")
CONST_BLOCK = re.compile(r"(?<![\w:])const\s*\{")
CONST_FN = re.compile(r"(?<![\w:])const\s+fn\b")
OPENERS = {"(": ")", "[": "]", "{": "}"}
CLOSERS = {")", "]", "}"}

# Crates whose refusals are the test's own assertion route, with the reason
# each is outside the production panic check. The key is the crate directory.
TEST_ONLY_CRATES = {
    "crates/cadmpeg-test-support": (
        "every dependent names it under [dev-dependencies], so no production "
        "build links it; its `panic!` is the failing test's own assertion"
    ),
}
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


MODULE_TOKEN = re.compile(
    r"#\s*\[|(?<![\w#])mod\s+(?:r#)?(?P<name>[^\W\d]\w*)\s*(?P<form>[;{])|[{}]"
)
MODULE_VISIBILITY = re.compile(r"(?:pub(?:\s*\([^)]*\))?\s*)?")


def declared_modules(path: Path, source: str) -> list[tuple[Path, bool]]:
    """Each file module and include, with the exact production item scope."""
    code = SOURCE_POLICY.mask_rust_non_code(source)
    production, _ = SOURCE_POLICY.production_source(source)
    children: list[tuple[Path, bool]] = []
    attributes: list[str] = []
    attribute_tail = 0
    blocks: list[tuple[str, int]] = []
    depth = 0
    cursor = 0
    while token := MODULE_TOKEN.search(code, cursor):
        cursor = token.end()
        if SOURCE_POLICY.OUTER_ATTRIBUTE.match(code, token.start()):
            end = SOURCE_POLICY.attribute_end(code, token.start())
            if end is None:
                break
            if code[attribute_tail:token.start()].strip():
                attributes = []
            attributes.append(source[token.start():end])
            cursor = attribute_tail = end
            continue
        if token.group("name") is not None:
            if not MODULE_VISIBILITY.fullmatch(code[attribute_tail:token.start()].strip()):
                attributes = []
            name = token.group("name")
            if token.group("form") == ";":
                explicit = next((target for attribute in attributes
                                 if (target := SOURCE_POLICY.path_attr_target(attribute)) is not None), None)
                directory = SOURCE_POLICY.child_module_dir(path)
                for block_name, _ in blocks:
                    directory = directory / block_name
                target = SOURCE_POLICY.resolve_module_target(path, directory, name, explicit)
                if target is not None:
                    gated = not production[token.start():token.start() + 3].strip()
                    children.append((target.resolve(), gated))
            else:
                blocks.append((name, depth))
                depth += 1
        elif token.group() == "{":
            depth += 1
        else:
            depth -= 1
            while blocks and depth <= blocks[-1][1]:
                blocks.pop()
        attributes = []
        attribute_tail = cursor
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


def matching_brace(code: str, opening: int) -> int | None:
    """The offset one past the ``}`` that closes the ``{`` at ``opening``."""
    depth = 0
    cursor = opening
    while cursor < len(code):
        if code[cursor] == "{":
            depth += 1
        elif code[cursor] == "}":
            depth -= 1
            if depth == 0:
                return cursor + 1
        cursor += 1
    return None


def const_evaluated_spans(code: str) -> list[tuple[int, int]]:
    """Every span a ``const`` evaluation owns, as half-open byte offsets.

    Three forms: a ``const NAME: T = ...;`` initializer, an inline ``const {
    ... }`` block, and a ``const fn`` body. A ``panic!`` inside one refuses a
    source literal when the item is evaluated, so it opens no runtime panic
    route at a ``const`` call site. A ``const fn`` called from a runtime
    position would panic there; this census applies the rule rather than
    proving every call site, which is the same limit its module walk states.
    """
    spans = []
    for block in CONST_BLOCK.finditer(code):
        opening = code.index("{", block.start())
        end = matching_brace(code, opening)
        if end is not None:
            spans.append((opening, end))
    for item in CONST_FN.finditer(code):
        cursor = item.end()
        depth = 0
        while cursor < len(code):
            character = code[cursor]
            if character in "([":
                depth += 1
            elif character in ")]":
                depth -= 1
            elif character == "{" and depth == 0:
                break
            elif character == ";" and depth == 0:
                cursor = len(code)
                break
            cursor += 1
        if cursor < len(code):
            end = matching_brace(code, cursor)
            if end is not None:
                spans.append((cursor, end))
    for item in CONST_ITEM.finditer(code):
        cursor = item.end()
        depth = []
        start = None
        while cursor < len(code):
            character = code[cursor]
            if character in OPENERS:
                depth.append(OPENERS[character])
            elif character in CLOSERS:
                if not depth:
                    break
                if depth[-1] != character:
                    break
                depth.pop()
            elif not depth:
                if character == "=" and code[cursor:cursor + 2] != "==" and start is None:
                    start = cursor
                elif character == ";":
                    if start is not None:
                        spans.append((start, cursor))
                    break
            cursor += 1
    return spans


def panic_macro_sites(code: str) -> list[int]:
    """Every runtime ``panic!``/``unreachable!`` offset in production code."""
    spans = const_evaluated_spans(code)
    return [
        site.start()
        for site in PANIC_MACRO.finditer(code)
        if not any(start <= site.start() < end for start, end in spans)
    ]


def test_only_reason(relative: str) -> str | None:
    """The reason ``relative`` is outside the production panic check."""
    for crate, reason in TEST_ONLY_CRATES.items():
        if relative.startswith(f"{crate}/"):
            return reason
    return None


def census_panic_calls(listing: bool = False, check: bool = False) -> int:
    """Print the ``.expect(`` and ``.unwrap(`` census."""
    kept, gated = production_files()
    raw_lines = 0
    non_test_lines = 0
    expect_lines = 0
    unreached = 0
    unwrap_buckets: dict[str, int] = {}
    macro_buckets: dict[str, int] = {}
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
        macro_sites = panic_macro_sites(code)
        if macro_sites:
            macro_buckets[relative] = len(macro_sites)
        if relative != SEED_GENERATOR and test_only_reason(relative) is None:
            remaining_calls += len(PANIC_CALL.findall(code)) + len(macro_sites)
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
    print(f"non-test runtime panic!/unreachable! sites: {sum(macro_buckets.values())}")
    for relative, count in sorted(macro_buckets.items()):
        print(f"  {relative} {count}")
    if check:
        print(f"check excludes the seed-generation tool: {SEED_GENERATOR}")
        for crate, reason in sorted(TEST_ONLY_CRATES.items()):
            print(f"check excludes {crate}: {reason}")
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
        help="fail on production expect/unwrap calls and runtime panic!/unreachable! sites, "
             "except the declared seed-generation tool and the declared test-only crates",
    )
    arguments = parser.parse_args()
    if arguments.check and arguments.pattern is not None:
        parser.error("--check applies to the panic census, not an arbitrary pattern")
    if arguments.pattern is None:
        return census_panic_calls(arguments.list, arguments.check)
    return census_pattern(
        re.compile(arguments.pattern), arguments.label or arguments.pattern, arguments.list
    )


if __name__ == "__main__":
    sys.exit(main())
