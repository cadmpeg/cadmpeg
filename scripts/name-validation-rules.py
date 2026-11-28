#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check validation rule references and list admission candidates."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DRAFT_CORE = {
    "Identity", "ReferentialIntegrity", "NativeLinks", "CoedgePairing",
    "ShellTopology", "WireTopology", "CarrierReachability", "ParameterDomain",
    "GeometricConsistency",
}
# ---------------------------------------------------------------- tokenizer

@dataclass
class Tok:
    kind: str  # ident, str, char, life, punct, num
    text: str
    start: int
    end: int


RAW_RE = re.compile(r'(b?r)(#*)"')
CHAR_RE = re.compile(r"'(\\.[^']*|[^\\'])'")
LIFE_RE = re.compile(r"'[A-Za-z_][A-Za-z0-9_]*")
IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
NUM_RE = re.compile(r"[0-9][0-9A-Za-z_.]*")


def tokenize(src: str) -> list[Tok]:
    toks: list[Tok] = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c.isspace():
            i += 1
            continue
        if src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
            continue
        if src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            i = j
            continue
        m = RAW_RE.match(src, i) if c in "br" else None
        if m:
            hashes = m.group(2)
            close = '"' + hashes
            j = src.find(close, m.end())
            j = n if j < 0 else j + len(close)
            toks.append(Tok("str", src[i:j], i, j))
            i = j
            continue
        if c == '"' or (c == "b" and i + 1 < n and src[i + 1] == '"'):
            j = i + (2 if c == "b" else 1)
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            j += 1
            toks.append(Tok("str", src[i:j], i, j))
            i = j
            continue
        if c == "'":
            m = CHAR_RE.match(src, i)
            if m:
                toks.append(Tok("char", m.group(0), i, m.end()))
                i = m.end()
                continue
            m = LIFE_RE.match(src, i)
            j = m.end() if m else i + 1
            toks.append(Tok("life", src[i:j], i, j))
            i = j
            continue
        m = IDENT_RE.match(src, i)
        if m:
            j = m.end()
            toks.append(Tok("ident", m.group(0), i, j))
            i = j
            continue
        m = NUM_RE.match(src, i)
        if m:
            j = m.end()
            toks.append(Tok("num", m.group(0), i, j))
            i = j
            continue
        toks.append(Tok("punct", c, i, i + 1))
        i += 1
    return toks


def string_value(literal: str) -> str:
    """The text of a Rust string literal, line continuations removed."""
    m = re.match(r'b?r(#*)"(.*)"\1$', literal, re.S)
    if m:
        return m.group(2)
    body = literal[literal.index('"') + 1 : -1]
    body = re.sub(r"\\\n\s*", "", body)
    return body.replace('\\"', '"').replace("\\\\", "\\").replace("\\n", " ")


@dataclass
class FileScan:
    path: str
    src: str
    toks: list[Tok]
    sites: list[tuple[str, int, str | None]] = field(default_factory=list)
    # rule names a route or a test refers to, with their offsets
    references: list[tuple[str, int]] = field(default_factory=list)


def rust_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "crates/*.rs", "crates/**/*.rs"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.split()
    return sorted(set(out))


PAIRS: dict[int, tuple[list[Tok], dict[int, int]]] = {}


def matching(toks: list[Tok], open_index: int) -> int:
    """The index of the bracket that closes the one at `open_index`."""
    entry = PAIRS.get(id(toks))
    if entry is None or entry[0] is not toks:
        table: dict[int, int] = {}
        stack: list[int] = []
        for k, t in enumerate(toks):
            if t.kind == "punct" and t.text in "([{":
                stack.append(k)
            elif t.kind == "punct" and t.text in ")]}" and stack:
                table[stack.pop()] = k
        entry = (toks, table)
        PAIRS[id(toks)] = entry
    table = entry[1]
    if open_index in table:
        return table[open_index]
    return _matching_scan(toks, open_index)


def _matching_scan(toks: list[Tok], open_index: int) -> int:
    pairs = {"(": ")", "{": "}", "[": "]"}
    want = pairs[toks[open_index].text]
    depth = 0
    for k in range(open_index, len(toks)):
        t = toks[k]
        if t.kind != "punct":
            continue
        if t.text in pairs:
            depth += 1
        elif t.text in pairs.values():
            depth -= 1
            if depth == 0:
                assert t.text == want, (toks[open_index].start, t.start)
                return k
    raise ValueError("unbalanced")


def callee_before(toks: list[Tok], paren: int) -> tuple[str, int]:
    """The name called by the parenthesis at `paren`, and its token index."""
    k = paren - 1
    if k >= 0 and toks[k].kind == "punct" and toks[k].text == "!":
        k -= 1
    if k >= 0 and toks[k].kind == "ident":
        return toks[k].text, k
    return "", k


def finding_rule_position(toks: list[Tok], i: int) -> bool:
    """Whether the `rule_name!` at `i` states the rule of a finding: the
    value of a `rule:` field, or paired with a `Check::` in a tuple."""
    k = i - 1
    while k >= 2 and toks[k].text == ":" and toks[k - 1].text == ":" and toks[k - 2].kind == "ident":
        k -= 3
    if k >= 1 and toks[k].text == ":" and toks[k - 1].text == "rule":
        return True
    if toks[k].text == "," and k >= 1:
        return toks[k - 2].text == ":" and toks[k - 3].text == ":" \
            and toks[k - 4].text == "Check"
    return False


def checks_in(toks: list[Tok], start: int, end: int) -> list[str]:
    found = []
    for k in range(start, end - 2):
        if (toks[k].kind == "ident" and toks[k].text == "Check"
                and toks[k + 1].text == ":" and toks[k + 2].text == ":"):
            found.append(toks[k + 3].text)
    return found


def scan_file(path: str) -> FileScan:
    src = (ROOT / path).read_text()
    if "Finding" not in src and "rule!(" not in src and "rule_name!(" not in src:
        return FileScan(path, src, [])
    return scan_source(path, src)


def scan_source(path: str, src: str) -> FileScan:
    toks = tokenize(src)
    scan = FileScan(path, src, toks)
    stack: list[int] = []
    for i, t in enumerate(toks):
        if t.kind == "punct" and t.text in "([{":
            stack.append(i)
            continue
        if t.kind == "punct" and t.text in ")]}":
            if stack:
                stack.pop()
            continue
        if not (t.kind == "ident" and t.text in ("rule", "rule_name")
                and i + 3 < len(toks) and toks[i + 1].text == "!"
                and toks[i + 2].text == "(" and toks[i + 3].kind == "str"):
            continue
        name = string_value(toks[i + 3].text)
        check = None
        if t.text == "rule":
            check = enclosing_check(path, src, toks, stack, i)
        elif not finding_rule_position(toks, i):
            scan.references.append((name, t.start))
            continue
        else:
            for k in range(i, -1, -1):
                if toks[k].text == "Finding" and toks[k + 1].text == "{":
                    found = checks_in(toks, k, matching(toks, k + 1))
                    check = found[0] if len(found) == 1 else None
                    break
                if toks[k].text == "(" and toks[k - 1].text != "!":
                    found = checks_in(toks, k, matching(toks, k))
                    check = found[0] if len(found) == 1 else None
                    break
        scan.sites.append((name, t.start, check))
    return scan


HELPERS: dict[str, dict[str, tuple[int, str | None]]] = {}


def enclosing_check(path: str, src: str, toks: list[Tok], stack: list[int], i: int) -> str | None:
    """The check that the finding helper called around token `i` reports:
    a `Check::` argument of that call, or the one check of the helper's body,
    through a closure that forwards to a helper."""
    outer = next((stack[k] for k in range(len(stack) - 1, -1, -1)
                  if toks[stack[k]].text == "("), None)
    if outer is None:
        return None
    found = checks_in(toks, outer, matching(toks, outer))
    if len(found) == 1:
        return found[0]
    _, callee = callee_before(toks, outer)
    if callee < 0:
        return None
    helper = resolve_helper(path, src, toks, callee)
    if helper is not None:
        return helper[1]
    # a closure `let mut name = |…| helper(…)` forwards to a helper
    name = toks[callee].text
    for k in range(callee - 1, 2, -1):
        if toks[k].text == name and toks[k - 1].text in ("mut", "let") and toks[k + 1].text == "=":
            for m in range(k + 2, min(k + 60, len(toks) - 1)):
                if toks[m].kind == "ident" and toks[m + 1].text == "(" and toks[m].text != name:
                    helper = resolve_helper(path, src, toks, m)
                    if helper is not None:
                        return helper[1]
            break
    return None


def module_file(path: str, up: int) -> str | None:
    """The file of the module `up` levels above the module of `path`."""
    current = Path(path)
    for _ in range(up):
        directory = current.parent if current.name == "mod.rs" else current.with_suffix("")
        parent_dir = directory.parent
        candidates = [parent_dir.with_suffix(".rs"), parent_dir / "mod.rs"]
        found = next((c for c in candidates if (ROOT / c).exists()), None)
        if found is None:
            return None
        current = found
    return str(current)


def helpers_of(path: str) -> dict[str, tuple[int, str | None]]:
    """Functions of `path` that take a finding message (`fmt::Arguments<'_>`
    or `Ruled<'_>`): the argument index, a method's receiver not counted, and
    the one check their body reports, where it reports one."""
    if path not in HELPERS:
        if not (ROOT / path).exists():
            HELPERS[path] = {}
            return HELPERS[path]
        toks = tokenize((ROOT / path).read_text())
        found: dict[str, tuple[int, str | None]] = {}
        for k, t in enumerate(toks):
            if not (t.kind == "ident" and t.text == "fn" and k + 2 < len(toks)):
                continue
            open_index = k + 2
            if toks[open_index].text == "<":
                depth = 0
                for m in range(open_index, len(toks)):
                    if toks[m].text == "<":
                        depth += 1
                    elif toks[m].text == ">":
                        depth -= 1
                        if depth == 0:
                            open_index = m + 1
                            break
            if toks[open_index].text != "(":
                continue
            ranges = call_arguments(toks, open_index)
            receiver = bool(ranges) and any(toks[m].text == "self" for m in range(*ranges[0]))
            close = matching(toks, open_index)
            body = close + 1
            while body < len(toks) and toks[body].text != "{":
                body += 1
            checks = set(checks_in(toks, body, matching(toks, body))) if body < len(toks) else set()
            for index, (start, end) in enumerate(ranges):
                if any(toks[m].kind == "ident" and toks[m].text in ("Ruled", "Arguments")
                       for m in range(start, end)):
                    found[toks[k + 1].text] = (index - (1 if receiver else 0),
                                               next(iter(checks)) if len(checks) == 1 else None)
        HELPERS[path] = found
    return HELPERS[path]


def resolve_helper(path: str, src: str, toks: list[Tok], i: int) -> tuple[int, str | None] | None:
    """The message argument of the finding helper called at `i`, resolved
    through `super::` paths and `use super::…` imports."""
    name = toks[i].text
    up = 0
    k = i - 1
    while k >= 2 and toks[k].text == ":" and toks[k - 1].text == ":" \
            and toks[k - 2].text == "super":
        up += 1
        k -= 3
    if k >= 0 and toks[k].text in (".", ":"):
        return None
    if up == 0:
        own = helpers_of(path)
        if name in own:
            return own[name]
        imported = re.search(r"use ((?:super::)+)(?:\{[^}]*\b" + re.escape(name)
                             + r"\b[^}]*\}|" + re.escape(name) + r")\s*;", src)
        if not imported:
            return None
        up = imported.group(1).count("super")
    target = module_file(path, up)
    return helpers_of(target).get(name) if target else None


def call_arguments(toks: list[Tok], open_index: int) -> list[tuple[int, int]]:
    """Token ranges of the arguments in the parentheses at `open_index`."""
    close = matching(toks, open_index)
    ranges, start, depth = [], open_index + 1, 0
    for m in range(open_index + 1, close):
        t = toks[m]
        if t.kind == "punct" and t.text in "([{<":
            depth += 1
        elif t.kind == "punct" and t.text in ")]}>":
            depth -= 1
        elif t.kind == "punct" and t.text == "," and depth == 0:
            ranges.append((start, m))
            start = m + 1
    if start < close:
        ranges.append((start, close))
    return ranges


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--admission", action="store_true")
    args = parser.parse_args()
    scans = [scan_file(p) for p in rust_files()]
    if args.check:
        stated = {name for scan in scans for name, _, _ in scan.sites}
        failed = False
        for scan in scans:
            for name, offset in scan.references:
                if name not in stated and not name.startswith("test.") and "/test." not in name:
                    line = scan.src.count("\n", 0, offset) + 1
                    print(f"{scan.path}:{line}: `{name}` names no rule any finding states")
                    failed = True
        return int(failed)
    rows = [(name, check or "?", f"{scan.path}:{scan.src.count(chr(10), 0, offset) + 1}")
            for scan in scans for name, offset, check in scan.sites]
    core = sorted({name for name, check, _ in rows
                   if check in DRAFT_CORE and not name.startswith(("design.", "process."))
                   and "/" not in name and not name.startswith("test.")})
    unknown = sorted({(name, where) for name, check, where in rows
                      if check == "?" and "/" not in name and not name.startswith("test.")})
    for name in core:
        print(f'    crate::rule_name!("{name}"),')
    for name, where in unknown:
        print(f"unresolved check: {name} {where}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
