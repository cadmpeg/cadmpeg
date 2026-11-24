#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Name the rule of every validation finding site.

A finding states its rule with ``rule!("<area>.<detail>", "message", ...)``
where the validator takes a message, and with
``rule: rule_name!("<area>.<detail>")`` in a ``Finding { .. }`` literal. This
program writes those names where a site has none:

* In the validator tree (``crates/cadmpeg-ir/src/validate.rs`` and
  ``crates/cadmpeg-ir/src/validate/``) every ``format_args!(`` whose call is a
  finding helper becomes ``rule!("<name>", ``, and every message parameter or
  closure argument typed ``fmt::Arguments<'_>`` becomes ``Ruled<'_>``.
* In every crate, every ``Finding { .. }`` literal that is not a struct update
  gains a ``rule`` field.

``<area>`` is the validator module (the first path component under
``validate/``), ``<codec>/<file stem>`` in a codec crate, and ``test`` (after
the codec prefix) in test code. ``<detail>`` is the first significant words of
the message template. A site that already names its rule is left as it is, so
the program can run again after a merge: ``--reuse-from REV`` gives an unnamed
site the name that ``REV`` states for the same area and template (for a
template with no words of its own, in the same function). ``--write`` formats
the files it changes with rustfmt. A site whose template has no words of its
own and no name at ``REV`` is listed, and nothing is written, until it is named
by hand.

Usage:
    scripts/name-validation-rules.py --write [--reuse-from REV]
    scripts/name-validation-rules.py --check      # exit 1 if a site has no name
    scripts/name-validation-rules.py --list       # name, check, site, template
    scripts/name-validation-rules.py --admission  # the model-class core rules
"""

from __future__ import annotations

import argparse
import hashlib
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VALIDATOR = ("crates/cadmpeg-ir/src/validate.rs", "crates/cadmpeg-ir/src/validate/")
# Calls whose `format_args!` argument is no finding message.
NOT_FINDING_CALLEES = {
    "malformed", "custom", "write", "writeln", "format", "panic", "expect",
    "enter_nested", "unreachable", "assert", "assert_eq", "debug_assert",
    # takes the kind of the missing record; the helper states its own message
    "ref_error",
}
STOPWORDS = {
    "a", "an", "the", "of", "its", "is", "are", "and", "to", "in", "on", "by",
    "with", "that", "which", "as", "at", "it", "be", "for", "from", "this",
    "has", "have", "or", "so", "s", "was", "were", "their", "each", "every",
    "any", "than", "into", "there", "these", "those", "whose", "but",
}
SLUG_WORDS = 6
MAX_SLUG_WORDS = 10
DRAFT_CORE = {
    "Identity", "ReferentialIntegrity", "NativeLinks", "CoedgePairing",
    "ShellTopology", "WireTopology", "CarrierReachability", "ParameterDomain",
    "GeometricConsistency",
}
NAME_RE = re.compile(r"^([a-z0-9]+(-[a-z0-9]+)*/)?[a-z][a-z0-9_]*\.[a-z0-9]+(-[a-z0-9]+)*$")


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


# ------------------------------------------------------------------ naming

def slug_words(template: str) -> list[str]:
    text = re.sub(r"\{\{|\}\}", " ", template)
    text = re.sub(r"\{[^{}]*\}", " ", text)
    words = [w for w in re.split(r"[^a-z0-9]+", text.lower()) if w]
    return [w for w in words if w not in STOPWORDS]


def area_of(path: str, test: bool) -> str:
    parts = path.split("/")
    crate = parts[1]
    if crate == "cadmpeg-ir":
        if test:
            return "test"
        rel = parts[3:]
        if rel[0] == "validate.rs":
            return "validate"
        if rel[0] == "validate":
            return Path(rel[1]).stem
        return Path(rel[0]).stem
    prefix = crate.removeprefix("cadmpeg-codec-").removeprefix("cadmpeg-")
    prefix = prefix.replace("_", "-")
    if test:
        return f"{prefix}/test"
    return f"{prefix}/{Path(parts[-1]).stem}"


def is_test_path(path: str) -> bool:
    name = Path(path).name
    return (
        "/tests/" in path
        or "/tests." in path
        or name == "tests.rs"
        or name.endswith("_tests.rs")
        or "/integration_tests" in path
        or "/golden_tests" in path
        or "/test_support" in path
        or name == "fixture.rs"
        or "/benches/" in path
    )


def test_start(src: str) -> int:
    """Offset of the first inline `#[cfg(test)] mod … {`, or the end."""
    m = re.search(r"#\[cfg\(test\)\]\s*(pub(\([^)]*\))?\s+)?mod\s+\w+\s*\{", src)
    return m.start() if m else len(src)


class Names:
    """Rule names by area and template, stable across runs."""

    def __init__(self) -> None:
        self.by_template: dict[tuple[str, str], str] = {}
        # a template with no words of its own is told apart by its function
        self.by_function: dict[tuple[str, str, str], str] = {}
        self.templates: dict[str, set[str]] = {}

    def remember(self, area: str, template: str, name: str, function: str = "") -> None:
        if slug_words(template):
            self.by_template.setdefault((area, template), name)
        else:
            self.by_function.setdefault((area, template, function), name)
        self.templates.setdefault(name, set()).add(template)

    def name(self, area: str, template: str, function: str) -> str | None:
        known = self.by_template.get((area, template)) or \
            self.by_function.get((area, template, function))
        if known:
            return known
        words = slug_words(template)
        if not words:
            return None
        for count in range(SLUG_WORDS, MAX_SLUG_WORDS + 1):
            candidate = f"{area}.{'-'.join(words[:count])}"
            taken = self.templates.get(candidate, set()) - {template}
            if not taken:
                break
            if count >= len(words):
                digest = hashlib.sha1(template.encode()).hexdigest()[:4]
                candidate = f"{area}.{'-'.join(words[:SLUG_WORDS])}-{digest}"
                break
        self.remember(area, template, candidate)
        return candidate


# --------------------------------------------------------------- the sites

@dataclass
class Site:
    path: str
    offset: int
    area: str
    template: str
    function: str
    check: str | None
    name: str | None = None
    named: bool = False
    # the source range the name replaces or is inserted at
    edit: tuple[int, int] | None = None
    # a `Finding { .. }` literal gains a field
    literal: bool = False
    # a bare string literal is wrapped in `rule!(…)` up to this offset
    wrap_end: int | None = None


@dataclass
class FileScan:
    path: str
    src: str
    toks: list[Tok]
    sites: list[Site] = field(default_factory=list)
    type_edits: list[tuple[int, int, str]] = field(default_factory=list)
    # rule names a route or a test refers to, with their offsets
    references: list[tuple[str, int]] = field(default_factory=list)


def rust_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "crates/*.rs", "crates/**/*.rs"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.split()
    return sorted(set(out))


def enclosing_fn(toks: list[Tok], index: int) -> str:
    for k in range(index, -1, -1):
        if toks[k].kind == "ident" and toks[k].text == "fn" and k + 1 < len(toks):
            return toks[k + 1].text
    return ""


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
    if "Finding" not in src and "rule!(" not in src and "rule_name!(" not in src \
            and not path.startswith(VALIDATOR):
        return FileScan(path, src, [])
    return scan_source(path, src)


def scan_source(path: str, src: str) -> FileScan:
    toks = tokenize(src)
    scan = FileScan(path, src, toks)
    validator = path.startswith(VALIDATOR)
    tests_from = 0 if is_test_path(path) else test_start(src)
    own_area, test_area = area_of(path, False), area_of(path, True)
    # Paren stack for enclosing calls.
    stack: list[int] = []
    for i, t in enumerate(toks):
        if t.kind == "punct" and t.text in "([{":
            stack.append(i)
            continue
        if t.kind == "punct" and t.text in ")]}":
            if stack:
                stack.pop()
            continue
        test = t.start >= tests_from
        area = test_area if test else own_area
        # rule!("name", "template" … ) already named
        if (t.kind == "ident" and t.text in ("rule", "rule_name")
                and i + 2 < len(toks) and toks[i + 1].text == "!"
                and toks[i + 2].text == "(" and i + 3 < len(toks)
                and toks[i + 3].kind == "str"):
            name = string_value(toks[i + 3].text)
            template, check = "", None
            if t.text == "rule" and i + 5 < len(toks) and toks[i + 5].kind == "str":
                template = format_template(toks, i + 2)
                check = enclosing_check(path, src, toks, stack, i)
            elif t.text == "rule_name":
                if not finding_rule_position(toks, i):
                    scan.references.append((name, t.start))
                    continue
                for k in range(i, -1, -1):
                    if toks[k].text == "Finding" and toks[k + 1].text == "{":
                        template = literal_message(toks, k)
                        found = checks_in(toks, k, matching(toks, k + 1))
                        check = found[0] if len(found) == 1 else None
                        break
                    if toks[k].text == "(" and toks[k - 1].text != "!":
                        found = checks_in(toks, k, matching(toks, k))
                        check = found[0] if len(found) == 1 else None
                        break
            scan.sites.append(Site(path, t.start, area, template, enclosing_fn(toks, i),
                                   check, name, True))
            continue
        if validator and t.kind == "ident" and t.text == "format_args" \
                and i + 2 < len(toks) and toks[i + 1].text == "!" and toks[i + 2].text == "(":
            callee = ""
            outer = None
            built = False
            for k in range(len(stack) - 1, -1, -1):
                if toks[stack[k]].text == "{" and stack[k] and toks[stack[k] - 1].text == "Ruled":
                    built = True
                    break
                if toks[stack[k]].text == "(":
                    outer = stack[k]
                    callee, _ = callee_before(toks, outer)
                    break
            if built or callee in NOT_FINDING_CALLEES:
                continue
            template = format_template(toks, i + 2)
            check = None
            if outer is not None:
                close = matching(toks, outer)
                found = checks_in(toks, outer, close)
                check = found[0] if len(found) == 1 else None
            site = Site(path, t.start, area, template, enclosing_fn(toks, i), check)
            # `format_args!(` becomes `rule!("<name>", `
            site.edit = (t.start, toks[i + 2].end)
            scan.sites.append(site)
            continue
        if validator and t.kind == "ident" and t.text == "Arguments" and i >= 2 \
                and toks[i - 1].text == ":" and toks[i - 2].text == ":" \
                and i + 2 < len(toks) and toks[i + 1].text == "<" and toks[i + 2].kind == "life":
            # `std::fmt::Arguments<'_>` or `fmt::Arguments<'_>` as a message type
            k = i - 3
            while k >= 1 and toks[k].kind == "ident" and toks[k - 1].text == ":" \
                    and toks[k - 2].text == ":":
                k -= 3
            first = k if toks[k].kind == "ident" else k + 1
            # only a parameter or closure argument type: preceded by `name:` or `FnMut(`
            before = toks[first - 1] if first >= 1 else None
            if before is not None and (before.text in (":", "(")):
                end = i + 3 if toks[i + 3].text == ">" else i + 3
                scan.type_edits.append((toks[first].start, toks[end].end,
                                        f"Ruled<{toks[i + 2].text}>"))
            continue
        if t.kind == "ident" and t.text == "Finding" and i + 1 < len(toks) \
                and toks[i + 1].text == "{" and finding_literal(toks, i):
            close = matching(toks, i + 1)
            if "rule" in literal_fields(toks, i):
                continue
            template = literal_message(toks, i)
            found = checks_in(toks, i, close)
            check = found[0] if found else None
            site = Site(path, t.start, area, template, enclosing_fn(toks, i), check,
                        edit=(toks[i + 1].end, toks[i + 1].end), literal=True)
            scan.sites.append(site)
    if validator:
        for i, t in enumerate(toks):
            if t.kind != "ident" or i + 1 >= len(toks) or toks[i + 1].text != "(" \
                    or (i and toks[i - 1].text == "fn"):
                continue
            helper = resolve_helper(path, src, toks, i)
            if helper is None:
                continue
            index, check = helper
            arguments = call_arguments(toks, i + 1)
            if index >= len(arguments):
                continue
            start, end = arguments[index]
            test = toks[start].start >= tests_from
            first = start + 1 if toks[start].text == "&" else start
            if end - start == 1 and toks[start].kind == "str":
                scan.sites.append(Site(
                    path, toks[start].start, area_of(path, test),
                    string_value(toks[start].text), enclosing_fn(toks, i), check,
                    edit=(toks[start].start, toks[start].start), wrap_end=toks[start].end))
            elif toks[first].text == "format" and toks[first + 1].text == "!" \
                    and toks[first + 2].text == "(":
                scan.sites.append(Site(
                    path, toks[start].start, area_of(path, test),
                    format_template(toks, first + 2), enclosing_fn(toks, i), check,
                    edit=(toks[start].start, toks[first + 2].end)))
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


def format_template(toks: list[Tok], open_index: int) -> str:
    """The template of the format call whose `(` is at `open_index`; where it
    has no words of its own (`"{}"`), the first string literal argument that
    has."""
    close = matching(toks, open_index)
    literals = [string_value(toks[k].text) for k in range(open_index + 1, close)
                if toks[k].kind == "str"]
    for literal in literals:
        if slug_words(literal):
            return literal
    return literals[0] if literals else ""


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


def finding_literal(toks: list[Tok], i: int) -> bool:
    """`Finding {` that builds a value: preceded by an expression position."""
    prev = toks[i - 1] if i else None
    if prev is not None and prev.kind == "ident" and prev.text in (
            "struct", "let", "enum", "impl", "for", "fn"):
        return False
    if prev is not None and prev.text == ">" and i >= 2 and toks[i - 2].text == "-":
        return False
    names = literal_fields(toks, i)
    return names is not None and {"message", "severity", "check"} <= names


def literal_fields(toks: list[Tok], i: int) -> set[str] | None:
    """The field names of the struct literal `Name { … }` at `i`, or `None`
    for a struct update or a pattern with `..`."""
    close = matching(toks, i + 1)
    names, depth, start = set(), 0, True
    for k in range(i + 2, close):
        text = toks[k].text
        if toks[k].kind == "punct" and text in "([{":
            depth += 1
        elif toks[k].kind == "punct" and text in ")]}":
            depth -= 1
        elif depth == 0 and text == "." and toks[k + 1].text == ".":
            return None
        if depth == 0 and start and toks[k].kind == "ident":
            names.add(text)
        start = depth == 0 and text == ","
    return names


def literal_message(toks: list[Tok], i: int) -> str:
    """The first string literal after `message` inside the literal at `i`."""
    close = matching(toks, i + 1) if toks[i + 1].text == "{" else len(toks) - 1
    for k in range(i, min(close, len(toks) - 2)):
        if toks[k].kind == "ident" and toks[k].text == "message" and toks[k + 1].text == ":":
            for m in range(k + 2, min(k + 12, close)):
                if toks[m].kind == "str":
                    return string_value(toks[m].text)
                if toks[m].text == ",":
                    break
    return ""


# ----------------------------------------------------------------- harvest

def files_at(rev: str) -> dict[str, str]:
    """The Rust files at `rev` that state a rule name, by path."""
    listed = subprocess.run(
        ["git", "grep", "-l", "-e", "rule!(", "-e", "rule_name!(", rev, "--", "crates/*.rs"],
        cwd=ROOT, capture_output=True, text=True,
    ).stdout.split()
    result = {}
    for entry in listed:
        path = entry.split(":", 1)[1]
        result[path] = subprocess.run(["git", "show", f"{rev}:{path}"], cwd=ROOT,
                                      capture_output=True, text=True, check=True).stdout
    return result


# -------------------------------------------------------------------- main

def macro_path(path: str) -> str:
    return "crate" if path.startswith("crates/cadmpeg-ir/") else "cadmpeg_ir"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--list", action="store_true")
    mode.add_argument("--admission", action="store_true")
    parser.add_argument("--reuse-from", metavar="REV")
    args = parser.parse_args()

    scans = [scan_file(p) for p in rust_files()]
    names = Names()
    if args.reuse_from:
        for path, src in files_at(args.reuse_from).items():
            for site in scan_source(path, src).sites:
                if site.named and site.name:
                    names.remember(site.area, site.template, site.name, site.function)
    for scan in scans:
        for site in scan.sites:
            if site.named and site.name:
                names.remember(site.area, site.template, site.name, site.function)

    unnamed, unnameable = [], []
    for scan in scans:
        for site in scan.sites:
            if site.named:
                continue
            site.name = names.name(site.area, site.template, site.function)
            if site.name is None:
                unnameable.append(site)
            unnamed.append(site)

    if args.check:
        failed = False
        for site in unnamed:
            print(f"{site.path}:{line_of(site)}: finding site names no rule")
            failed = True
        stated = {site.name for scan in scans for site in scan.sites if site.named}
        for scan in scans:
            for site in scan.sites:
                if site.named and site.name and not NAME_RE.match(site.name):
                    print(f"{site.path}:{line_of(site)}: `{site.name}` is no rule name")
                    failed = True
            for name, offset in scan.references:
                if name not in stated and not name.startswith("test.") and "/test." not in name:
                    line = scan.src.count("\n", 0, offset) + 1
                    print(f"{scan.path}:{line}: `{name}` names no rule any finding states")
                    failed = True
        return 1 if failed else 0

    if args.list or args.admission:
        rows = []
        for scan in scans:
            for site in scan.sites:
                rows.append((site.name or "?", site.check or "?", f"{site.path}:{line_of(site)}",
                             site.template))
        if args.list:
            for row in sorted(rows):
                print("\t".join(row))
            return 0
        core = sorted({name for name, check, where, _ in rows
                       if check in DRAFT_CORE and not name.startswith(("design.", "process."))
                       and "/" not in name and not name.startswith("test.")})
        unknown = sorted({(name, where) for name, check, where, _ in rows
                          if check == "?" and "/" not in name and not name.startswith("test.")})
        for name in core:
            print(f'    crate::rule_name!("{name}"),')
        for name, where in unknown:
            print(f"unresolved check: {name} {where}", file=sys.stderr)
        return 0

    # --write
    if unnameable:
        for site in unnameable:
            print(f"{site.path}:{line_of(site)}: the message has no words of its own; "
                  "name this site's rule by hand with rule!/rule_name!", file=sys.stderr)
        return 1
    written = []
    for scan in scans:
        src = rewritten(scan)
        if src is not None:
            (ROOT / scan.path).write_text(src)
            written.append(scan.path)
    if written:
        subprocess.run(["rustfmt", "--edition", "2021", *written], cwd=ROOT, check=True)
    return 0


def rewritten(scan: FileScan) -> str | None:
    """The source of `scan` with its named sites written, or `None` where
    nothing changes."""
    edits = list(scan.type_edits)
    for site in scan.sites:
        if site.named or site.edit is None or site.name is None:
            continue
        assert NAME_RE.match(site.name), site.name
        start, end = site.edit
        if site.literal:
            text = f"\n rule: {macro_path(scan.path)}::rule_name!(\"{site.name}\"),"
        else:
            text = f'{macro_path(scan.path)}::rule!("{site.name}", '
        edits.append((start, end, text))
        if site.wrap_end is not None:
            edits.append((site.wrap_end, site.wrap_end, ")"))
    if not edits:
        return None
    src = scan.src
    for start, end, text in sorted(edits, reverse=True):
        src = src[:start] + text + src[end:]
    if scan.type_edits and "Ruled" not in scan.src:
        src = add_ruled_import(src)
    if re.search(r"^use std::fmt;\n", src, re.M) and not re.search(r"\bfmt::", src):
        src = re.sub(r"^use std::fmt;\n", "", src, count=1, flags=re.M)
    return src


def add_ruled_import(src: str) -> str:
    m = re.search(r"^use [^\n]*;\n", src, re.M)
    line = "use crate::report::check::Ruled;\n"
    if m:
        return src[:m.start()] + line + src[m.start():]
    return line + src


def line_of(site: Site) -> int:
    return (ROOT / site.path).read_text().count("\n", 0, site.offset) + 1


if __name__ == "__main__":
    sys.exit(main())
