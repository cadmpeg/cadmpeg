#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check runtime panic macros outside test code."""

from __future__ import annotations

import argparse
import importlib.util
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = Path(__file__).with_name("check-source-policy.py")
SOURCE_POLICY = sys.modules.get("shared_rust_source_policy")
if SOURCE_POLICY is None:
    SPEC = importlib.util.spec_from_file_location("panic_source_policy", SCRIPT)
    if SPEC is None or SPEC.loader is None:
        raise SystemExit(f"cannot load {SCRIPT}")
    SOURCE_POLICY = importlib.util.module_from_spec(SPEC)
    sys.modules[SPEC.name] = SOURCE_POLICY
    SPEC.loader.exec_module(SOURCE_POLICY)

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


def production_files(sources: dict[Path, str] | None = None) -> tuple[set[Path], set[Path]]:
    """Files the walk reaches outside a test module, and the ones it gates."""
    kept: set[Path] = set()
    gated: set[Path] = set()
    pending = [(root.resolve(), False) for root in crate_roots()]
    while pending:
        path, in_test = pending.pop()
        if path in (gated if in_test else kept):
            continue
        (gated if in_test else kept).add(path)
        source = sources.get(path) if sources is not None else None
        if source is None:
            source = path.read_text(encoding="utf-8")
        for child, child_gated in declared_modules(path, source):
            pending.append((child, in_test or child_gated))
    return kept, gated - kept


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


@SOURCE_POLICY.analysis_session
def panic_findings(sources: dict[Path, str] | None = None) -> list[str]:
    kept, gated = production_files(sources)
    findings = []
    for path in scope():
        resolved = path.resolve()
        if resolved not in kept and resolved in gated:
            continue
        relative = path.relative_to(ROOT).as_posix()
        if test_only_reason(relative) is not None:
            continue
        source = sources.get(resolved) if sources is not None else None
        if source is None:
            source = path.read_text(encoding="utf-8")
        code, _ = SOURCE_POLICY.production_source(source)
        for offset in panic_macro_sites(code):
            line = code.count("\n", 0, offset) + 1
            findings.append(f"{relative}:{line}: runtime panic macro")
    return findings


@SOURCE_POLICY.analysis_session
def check_panic_macros() -> int:
    findings = panic_findings()
    for finding in findings:
        print(finding)
    if not findings:
        print("panic macros: ok")
    return int(bool(findings))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="check production panic macros")
    parser.parse_args()
    return check_panic_macros()


if __name__ == "__main__":
    raise SystemExit(main())
