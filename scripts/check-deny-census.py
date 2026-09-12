#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check that every non-test deserializable item refuses an unknown wire key.

A document read must not silently accept a key no type declares. The golden
sweeps prove this only for shapes the goldens carry; this checker proves the
static property over every declaration in the two wire crates.

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
* every variant is a unit variant - the read is a bare name with no key to deny.

Anything else fails, unless it is one of the named exceptions below.

Exit code 0 prints ``deny census: ok``; any failure prints one
``file:line type`` line per offending item and exits 1.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOTS = ("crates/cadmpeg-ir/src", "crates/cadmpeg-core/src")

# Items admitted by name, each with the reason it cannot carry a deny. Every
# entry is load-bearing: the item derives ``Deserialize``, reaches the check,
# and fails it without the entry. An item with a hand-written or
# ``Serialize``-only impl never reaches the check and states nothing here.
EXCEPTIONS = {
    "NativeUnknownRecord": "retained unknown record inside /native: its field map is the payload",
    "UnknownRecordWire": "wire form of a retained unknown record inside /native",
    "VersionProbe": "private one-field pre-pass; the document is re-read through CadIrReadWire, which denies",
}

SKIP_BASENAMES = {
    "tests.rs",
    "golden_tests.rs",
    "integration_tests.rs",
    "test_support.rs",
}
SKIP_DIRS = {"tests", "golden_tests", "integration_tests", "test_support"}

ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(struct|enum)\s+(\$?\w+)"
)
VARIANT_RE = re.compile(r"^\s*(?:#\[[^\]]*\]\s*)?(\$?\w+)\s*(.?)")


class Item:
    def __init__(self, path, line, kind, name, attrs, body):
        self.path = path
        self.line = line
        self.kind = kind
        self.name = name
        self.attrs = attrs
        self.body = body


def source_files():
    for root in ROOTS:
        base = Path(root)
        for path in sorted(base.rglob("*.rs")):
            parts = set(path.parts)
            if path.name in SKIP_BASENAMES or parts & SKIP_DIRS:
                continue
            yield path


def read_attribute(lines, index):
    """Consume one ``#[...]`` attribute starting at ``index``."""
    text = ""
    depth = 0
    while index < len(lines):
        line = lines[index]
        text += line
        depth += line.count("[") - line.count("]")
        index += 1
        if depth <= 0:
            break
    return text, index


def skip_block(lines, index):
    """Skip the item starting at ``index``, brace-matched or ``;``-terminated."""
    depth = 0
    seen_brace = False
    while index < len(lines):
        line = lines[index]
        depth += line.count("{") - line.count("}")
        if "{" in line:
            seen_brace = True
        index += 1
        if seen_brace and depth <= 0:
            return index
        if not seen_brace and line.rstrip().endswith(";"):
            return index
    return index


def collect_items(path):
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
    items = []
    index = 0
    while index < len(lines):
        stripped = lines[index].lstrip()
        if not stripped.startswith("#["):
            index += 1
            continue
        attrs = []
        start = index
        test_only = False
        while index < len(lines) and lines[index].lstrip().startswith("#["):
            text, index = read_attribute(lines, index)
            if re.search(r"#\[cfg\(\s*test\s*\)\]", text):
                test_only = True
            attrs.append(text)
        while index < len(lines) and (
            not lines[index].strip() or lines[index].lstrip().startswith("///")
            or lines[index].lstrip().startswith("//")
        ):
            index += 1
        if index >= len(lines):
            break
        match = ITEM_RE.match(lines[index])
        if match is None:
            if test_only:
                index = skip_block(lines, index)
            continue
        body_end = skip_block(lines, index)
        if not test_only:
            items.append(
                Item(
                    path,
                    start + 1,
                    match.group(1),
                    match.group(2),
                    "".join(attrs),
                    "".join(lines[index:body_end]),
                )
            )
        index = body_end
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


def all_unit_variants(item):
    if item.kind != "enum":
        return False
    variants = [
        strip_variant_attributes(raw)
        for raw in split_variants(strip_macro_repetition(enum_body(item)))
    ]
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
    return re.findall(r"[A-Z]\w*", text)


def main():
    index = {}
    order = []
    for path in source_files():
        for item in collect_items(path):
            order.append(item)
            index.setdefault(item.name, item)

    failures = []
    arm_failures = []
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
                    if name in index and not passes(index[name]):
                        admitted = False
        return admitted

    def passes(item):
        if item.name in EXCEPTIONS:
            return True
        if item.name in checking:
            return True
        checking.add(item.name)
        try:
            attrs = serde_attrs(item.attrs)
            if re.search(r"\bdeny_unknown_fields\b", attrs):
                return True
            if re.search(r"\btransparent\b", attrs):
                return True
            target = re.search(r"\b(?:try_from|from)\s*=\s*\"([^\"]+)\"", attrs)
            if target:
                return all(
                    passes(index[name])
                    for name in named_types(target.group(1))
                    if name in index
                )
            if re.search(r"\buntagged\b", attrs):
                return check_untagged_arms(item, untagged_arms(item))
            if all_unit_variants(item):
                return True
            return False
        finally:
            checking.discard(item.name)

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
            f"deny census: {len(reported)} item(s) accept an unknown wire key",
            file=sys.stderr,
        )
        return 1
    print("deny census: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
