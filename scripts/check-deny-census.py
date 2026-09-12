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
* ``untagged`` - each arm's type is checked in turn;
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

# Items admitted by name, each with the reason it cannot carry a deny.
EXCEPTIONS = {
    "NativeRecord": "codec-private record body: an arbitrary source key is the payload, not a defect",
    "UnknownRecord": "codec-private record body: an arbitrary source key is the payload, not a defect",
    "RecordShape": "shape of a codec-private record: its field map is the payload",
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


def all_unit_variants(item):
    if item.kind != "enum":
        return False
    body = strip_macro_repetition(enum_body(item))
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
    found = False
    for raw in variants:
        text = re.sub(r"#\[[^\]]*\]", " ", raw)
        text = re.sub(r"//[^\n]*", " ", text).strip()
        if not text:
            continue
        found = True
        if "(" in text or "{" in text:
            return False
    return found


def untagged_arm_types(item):
    body = strip_macro_repetition(enum_body(item))
    body = re.sub(r"#\[[^\]]*\]", " ", body)
    body = re.sub(r"//[^\n]*", " ", body)
    return re.findall(r"[A-Z]\w*", body)


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
    checking = set()

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
                return all(
                    passes(index[name])
                    for name in untagged_arm_types(item)
                    if name in index
                )
            if all_unit_variants(item):
                return True
            return False
        finally:
            checking.discard(item.name)

    for item in order:
        if not derives_deserialize(item.attrs):
            continue
        if not passes(item):
            failures.append(item)

    if failures:
        for item in failures:
            print(f"{item.path}:{item.line} {item.name}", file=sys.stderr)
        print(
            f"deny census: {len(failures)} item(s) accept an unknown wire key",
            file=sys.stderr,
        )
        return 1
    print("deny census: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
