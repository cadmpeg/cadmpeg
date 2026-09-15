#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check that every non-test deserializable item refuses an unknown wire key.

A document read must not silently accept a key no type declares. The golden
sweeps test shapes the goldens carry; this checker follows declared
read routes in the three wire crates. Imported bare names use
a unique-declaration fallback; this is not a Rust name-resolution proof.
Declarations without attributes are included when following a read route.
Nongeneric type aliases are followed to their targets; other alias shapes
fail the static check when used as a read route.

An item that derives ``Deserialize`` passes when its serde attributes state one
of:

* ``deny_unknown_fields`` - it refuses the key itself;
* ``try_from = "T"`` or ``from = "T"`` - the read goes through ``T``, which is
  checked in turn;
* ``transparent`` - the read is the inner type's read, with no key of its own;
* ``untagged`` - every arm is a unit arm, or a newtype arm whose single type
  is checked in turn. serde has no variant-level ``deny_unknown_fields``, so an
  inline struct arm, or a tuple arm carrying more than one type, can never
  refuse a key and fails. The same rule holds for a single arm that carries
  ``#[serde(untagged)]`` inside an otherwise tagged enum, where the container's
  deny does not reach the arm's own keys;
* every variant is a unit variant - the read is a bare name with no key to deny;
* the enum has no variant at all - it is uninhabited, no document can name a
  variant of it, and serde refuses every input before a key is read.

Anything else fails, unless it is one of the named exceptions below.

Exit code 0 prints ``deny census: ok``; any failure prints one
``file:line type`` line per offending item and exits 1.
"""

from __future__ import annotations

import importlib.util
from bisect import bisect_right
import os
import re
import stat
import sys
from pathlib import Path

# Share the source-policy lexer so comments, literals and cfg(test) bodies do
# not create declarations or alter lexical scope in this census.
_SPEC = importlib.util.spec_from_file_location(
    "deny_source_policy", Path(__file__).with_name("check-source-policy.py")
)
if _SPEC is None or _SPEC.loader is None:
    raise SystemExit("cannot load the source-policy lexer")
SOURCE_POLICY = importlib.util.module_from_spec(_SPEC)
sys.modules[_SPEC.name] = SOURCE_POLICY
_SPEC.loader.exec_module(SOURCE_POLICY)


ROOTS = (
    "crates/cadmpeg-ir/src",
    "crates/cadmpeg-core/src",
    "crates/cadmpeg-asm/src",
)

# Items admitted by declaring file and name, each with the reason it cannot
# carry a deny. Every entry is load-bearing: the item derives ``Deserialize``,
# reaches the check, and fails it without the entry. An item with a
# hand-written or ``Serialize``-only impl never reaches the check and states
# nothing here. The key is the declaring path, so a type that reuses an
# exception's name in another file is not admitted by it.
EXCEPTIONS = {
    "crates/cadmpeg-ir/src/document.rs:VersionProbe": "private one-field pre-pass; the document is re-read through CadIrReadWire, which denies",
}

SKIP_BASENAMES = {
    "tests.rs",
    "golden_tests.rs",
    "integration_tests.rs",
    "test_support.rs",
}
SKIP_DIRS = {"tests", "golden_tests", "integration_tests", "test_support"}

ITEM_RE = re.compile(
    r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?(struct|enum|type)\s+(\$?\w+)"
)
DECLARATION_OR_ATTRIBUTE = re.compile(
    r"#\s*\[|\b(?:pub(?:\s*\([^)]*\))?\s+)?(?:struct|enum|type)\s+\$?\w+"
)


class Item:
    def __init__(self, path, line, kind, name, attrs, body, scope=None):
        self.path = path
        self.line = line
        self.kind = kind
        self.name = name
        self.attrs = attrs
        self.body = body
        src = path.parts.index("src")
        self.crate = Path(*path.parts[:src])
        parts = path.parts[src + 1:]
        module = parts[:-1] + (() if path.name in {"lib.rs", "main.rs", "mod.rs"} else (path.stem,))
        self.scope = module if scope is None else module + scope


def source_files():
    def fail(error):
        raise error

    for root in ROOTS:
        base = Path(root)
        if not stat.S_ISDIR(base.stat().st_mode):
            raise NotADirectoryError(base)
        for directory, children, files in os.walk(base, onerror=fail):
            children[:] = sorted(name for name in children if name not in SKIP_DIRS)
            for name in sorted(files):
                if name.endswith(".rs") and name not in SKIP_BASENAMES:
                    yield Path(directory) / name


SCOPE_TOKEN = re.compile(r"\bmod\s+(\w+)\s*\{|[{}]")


def lexical_scopes(code):
    """Scope after each brace, preserving inline modules and local blocks."""
    offsets = [0]
    scopes = [()]
    stack = []
    for token in SCOPE_TOKEN.finditer(code):
        if token.group() == "}":
            if stack:
                stack.pop()
        else:
            stack.append(token.group(1) or f"@{token.start()}")
        offsets.append(token.end())
        scopes.append(tuple(stack))
    return offsets, scopes


def collect_items(path):
    source = path.read_text(encoding="utf-8")
    code, _ = SOURCE_POLICY.production_source(source)
    offsets, scopes = lexical_scopes(code)
    items = []
    cursor = 0
    while opening := DECLARATION_OR_ATTRIBUTE.search(code, cursor):
        start = cursor = opening.start()
        attrs = []
        while SOURCE_POLICY.OUTER_ATTRIBUTE.match(code, cursor):
            end = SOURCE_POLICY.attribute_end(code, cursor)
            if end is None:
                raise ValueError(f"{path}: incomplete attribute at byte {cursor}")
            attrs.append(source[cursor:end])
            cursor = end
            while cursor < len(code) and code[cursor].isspace():
                cursor += 1
        match = ITEM_RE.match(code, cursor)
        if match is None:
            continue
        body_end = SOURCE_POLICY.item_end(code, cursor)
        if body_end is None:
            raise ValueError(f"{path}: incomplete declaration at byte {cursor}")
        items.append(Item(
            path,
            source.count("\n", 0, start) + 1,
            match.group(1),
            match.group(2),
            "\n".join(attrs),
            source[cursor:body_end],
            scopes[bisect_right(offsets, cursor) - 1],
        ))
        cursor = body_end
    return items


def serde_attrs(attrs):
    """Serde attribute text, including the ``cfg_attr``-wrapped forms."""
    parts = []
    for match in re.finditer(r"serde\s*\(", attrs):
        start = match.end()
        depth = 1
        pos = start
        while pos < len(attrs) and depth:
            if attrs[pos] == "(":
                depth += 1
            elif attrs[pos] == ")":
                depth -= 1
            pos += 1
        parts.append(attrs[start : pos - 1])
    return ",".join(parts)


def derives_deserialize(attrs):
    for match in re.finditer(r"derive\s*\(([^)]*)\)", attrs):
        if re.search(r"\bDeserialize\b", match.group(1)):
            return True
    return False


def enum_body(item):
    open_brace = item.body.find("{")
    if open_brace < 0:
        return ""
    return item.body[open_brace + 1 : item.body.rfind("}")]


def strip_macro_repetition(body):
    """Drop ``macro_rules!`` repetition tokens so variants read as variants.

    ``label_enum!`` declares its arms inside ``$( ... )*``; without this the
    repetition parentheses read as a tuple variant.
    """
    body = re.sub(r"\$\(", " ", body)
    body = re.sub(r"\)\s*,?\s*[*+?]", " ", body)
    return body


def split_variants(body):
    """Split an enum body into its top-level variant texts, attributes kept."""
    depth = 0
    variants = []
    current = ""
    for char in body:
        if char in "({[":
            depth += 1
        elif char in ")}]":
            depth -= 1
        if char == "," and depth == 0:
            variants.append(current)
            current = ""
        else:
            current += char
    variants.append(current)
    out = []
    for raw in variants:
        if strip_variant_attributes(raw):
            out.append(raw)
    return out


def strip_variant_attributes(raw):
    """The variant text with its attributes and comments removed."""
    text = re.sub(r"#\[[^\]]*\]", " ", raw)
    return re.sub(r"//[^\n]*", " ", text).strip()


def enum_variants(item):
    if item.kind != "enum":
        return None
    return [
        strip_variant_attributes(raw)
        for raw in split_variants(strip_macro_repetition(enum_body(item)))
    ]


def is_uninhabited_enum(item):
    """An enum with no variant: no document can name one, so no key is read."""
    variants = enum_variants(item)
    return variants is not None and not variants


def all_unit_variants(item):
    variants = enum_variants(item)
    if not variants:
        return False
    return all("(" not in text and "{" not in text for text in variants)


def split_tuple_types(text):
    """Split the inside of a tuple variant into its top-level type texts."""
    depth = 0
    types = []
    current = ""
    for char in text:
        if char in "({[<":
            depth += 1
        elif char in ")}]>":
            depth -= 1
        if char == "," and depth == 0:
            types.append(current)
            current = ""
        else:
            current += char
    types.append(current)
    return [part for part in (item.strip() for item in types) if part]


class Arm:
    def __init__(self, name, kind, types, untagged):
        self.name = name
        self.kind = kind
        self.types = types
        self.untagged = untagged


def untagged_arms(item):
    """The arms of an enum body, classified as unit, tuple or struct.

    ``untagged`` records whether the arm itself carries ``#[serde(untagged)]``,
    which makes it an untagged read inside an otherwise tagged enum.
    """
    arms = []
    for raw in split_variants(strip_macro_repetition(enum_body(item))):
        untagged = bool(re.search(r"\buntagged\b", serde_attrs(raw)))
        text = strip_variant_attributes(raw)
        match = re.match(r"(\$?\w+)\s*(.*)", text, re.S)
        if match is None:
            continue
        name = match.group(1)
        rest = match.group(2).lstrip()
        if rest.startswith("{"):
            arms.append(Arm(name, "struct", [], untagged))
        elif rest.startswith("("):
            inner = rest[1 : rest.rfind(")")]
            arms.append(Arm(name, "tuple", split_tuple_types(inner), untagged))
        else:
            arms.append(Arm(name, "unit", [], untagged))
    return arms


def named_types(text):
    # Retain the whole path: other::Wire cannot inherit a local Wire's deny.
    # Rust permits lower-case type aliases and declarations too.
    return re.findall(r"(?<![\w:'])(?:::)?(?:[A-Za-z_]\w*::)*[A-Za-z_]\w*", text)


def alias_target(item):
    """Read a nongeneric type alias; other alias shapes need explicit proof."""
    code = SOURCE_POLICY.mask_rust_non_code(item.body)
    match = re.fullmatch(
        r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?type\s+\w+\s*=\s*(.*?)\s*;\s*",
        code,
        re.S,
    )
    return match.group(1) if match else None


def resolve_item(index, name, owner, ambiguities):
    """Resolve qualified declarations and distinguish lexical item scopes.

    A bare name may have been imported from a different file. A unique
    declaration remains the census's imported-name fallback; duplicates require
    a declaration in the current lexical scope. Qualified paths never use that
    fallback. An unresolved qualified path fails closed.
    """
    parts = name.lstrip(":").split("::")
    candidates = index.get(parts[-1], ())
    if len(parts) > 1:
        prefix = parts[:-1]
        crate = owner.crate
        scope = tuple(part for part in owner.scope if not part.startswith("@"))
        if prefix[0] == "crate":
            scope = ()
            prefix = prefix[1:]
        elif prefix[0] == "self":
            prefix = prefix[1:]
        elif prefix[0] == "super":
            while prefix and prefix[0] == "super":
                scope = scope[:-1]
                prefix = prefix[1:]
        else:
            external = [candidate.crate for candidate in candidates
                        if candidate.crate.name.replace("-", "_") == prefix[0]]
            if external:
                crate = external[0]
                scope = ()
                prefix = prefix[1:]
        wanted = scope + tuple(prefix)
        matches = [candidate for candidate in candidates
                   if candidate.crate == crate and candidate.scope == wanted]
    else:
        scope = owner.scope
        while True:
            matches = [candidate for candidate in candidates
                       if candidate.path == owner.path and candidate.scope == scope]
            if matches or not scope or not scope[-1].startswith("@"):
                break
            scope = scope[:-1]
        if not matches and len(candidates) == 1 and candidates[0].path != owner.path:
            matches = list(candidates)
    if len(matches) == 1:
        return matches[0]
    if candidates or len(parts) > 1:
        ambiguities.append((owner.path, owner.line, owner.name, name,
                            tuple(candidate.path for candidate in candidates)))
    return None


def main():
    index = {}
    order = []
    for path in source_files():
        for item in collect_items(path):
            order.append(item)
            index.setdefault(item.name, []).append(item)

    failures = []
    arm_failures = []
    ambiguities = []
    checking = set()

    def check_untagged_arms(item, arms):
        """Every untagged arm is a unit arm or a newtype over a checked type."""
        admitted = True
        for arm in arms:
            if arm.kind == "struct":
                arm_failures.append(
                    (
                        item.path,
                        item.line,
                        f"{item.name}::{arm.name}: an inline struct arm read "
                        "untagged has no way to refuse a key",
                    )
                )
                admitted = False
            elif arm.kind == "tuple" and len(arm.types) != 1:
                arm_failures.append(
                    (
                        item.path,
                        item.line,
                        f"{item.name}::{arm.name}: a tuple arm read untagged "
                        f"carries {len(arm.types)} types, so no single type "
                        "states its keys",
                    )
                )
                admitted = False
            elif arm.kind == "tuple":
                for name in named_types(arm.types[0]):
                    target = resolve_item(index, name, item, ambiguities)
                    if target is not None and not passes(target):
                        admitted = False
                    elif target is None and (name in index or "::" in name):
                        admitted = False
        return admitted

    def passes(item):
        if f"{item.path.as_posix()}:{item.name}" in EXCEPTIONS:
            return True
        identity = (item.path.as_posix(), item.line, item.name)
        if identity in checking:
            return True
        checking.add(identity)
        try:
            if item.kind == "type":
                target = alias_target(item)
                if target is None:
                    return False
                admitted = True
                for name in named_types(target):
                    target_item = resolve_item(index, name, item, ambiguities)
                    if target_item is not None and not passes(target_item):
                        admitted = False
                    elif target_item is None and (name in index or "::" in name):
                        admitted = False
                return admitted
            attrs = serde_attrs(item.attrs)
            if re.search(r"\bdeny_unknown_fields\b", attrs):
                return True
            if re.search(r"\btransparent\b", attrs):
                return True
            target = re.search(r"\b(?:try_from|from)\s*=\s*\"([^\"]+)\"", attrs)
            if target:
                admitted = True
                for name in named_types(target.group(1)):
                    target_item = resolve_item(index, name, item, ambiguities)
                    if target_item is not None and not passes(target_item):
                        admitted = False
                    elif target_item is None and (name in index or "::" in name):
                        admitted = False
                return admitted
            if re.search(r"\buntagged\b", attrs):
                return check_untagged_arms(item, untagged_arms(item))
            if is_uninhabited_enum(item):
                return True
            if all_unit_variants(item):
                return True
            return False
        finally:
            checking.discard(identity)

    for item in order:
        if not derives_deserialize(item.attrs):
            continue
        if not passes(item):
            failures.append((item.path, item.line, item.name))
        if item.kind == "enum" and not re.search(
            r"\buntagged\b", serde_attrs(item.attrs)
        ):
            arms = [arm for arm in untagged_arms(item) if arm.untagged]
            if arms and not check_untagged_arms(item, arms):
                failures.append((item.path, item.line, item.name))

    for path, line, owner, name, candidates in sorted(set(ambiguities), key=str):
        locations = ", ".join(candidate.as_posix() for candidate in candidates)
        failures.append(
            (
                path,
                line,
                f"{owner}: cannot resolve type {name} uniquely in its scope; candidates: {locations or 'none'}",
            )
        )

    detailed = {(path, line) for path, line, _ in arm_failures}
    lines = [
        f"{path}:{line} {label}"
        for path, line, label in arm_failures
    ] + [
        f"{path}:{line} {label}"
        for path, line, label in failures
        if (path, line) not in detailed
    ]
    reported = sorted(dict.fromkeys(lines))
    if reported:
        for line in reported:
            print(line, file=sys.stderr)
        print(
            f"deny census: {len(reported)} item(s) lack checked unknown-key refusal",
            file=sys.stderr,
        )
        return 1
    print("deny census: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
