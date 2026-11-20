#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Generate Annex A of ``docs/cadir-v11.md`` from the CADIR JSON Schema.

§2.7 of the specification: "The types this document names and does not state
here or in its parts are listed in Annex A." This script computes that list and
writes each type's JSON Schema below the annex's opening paragraph.

A type is *stated* when a Rust sketch of the specification declares it
(``struct``, ``enum``, ``type``, or ``trait``). A type is *named* when a sketch
uses it in a type position: a field's type, a variant's payload, a generic
argument, or an alias's target. The annex holds every named type that is not
stated, except the id types (§2.7 states them as newtypes over a string) and the
types the specification defines in a sentence (``PROSE``), and with them every
type their schemas reference that is not stated either. A named type that is
neither stated, nor in ``PROSE``, nor in the schema is an error: the
specification names a type nothing defines.

The schema is ``crates/cadmpeg-ir/schema/cadir.schema.json``, the committed
output of ``cadmpeg_ir::cadir_json_schema()``. ``--schema FILE`` reads it from
another file.

Usage:
    scripts/generate-annex-a.py [--schema FILE] --write   rewrite the annex
    scripts/generate-annex-a.py [--schema FILE] [--check] compare, print a diff

Exit codes: 0 the annex matches the schema, 1 it does not (``--check``), 2 the
specification or the schema does not have the expected shape.
"""

from __future__ import annotations

import argparse
import difflib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = ROOT / "docs" / "cadir-v11.md"
HEADING = "## Annex A. Record types stated by schema"
SCHEMA = ROOT / "crates" / "cadmpeg-ir" / "schema" / "cadir.schema.json"
REF = "#/$defs/"

# Types the specification defines in a sentence rather than a sketch, with the
# section that defines each.
PROSE = {
    "Axis": "§5.6, a construction kind marker",
    "CellAddress": "§5.12",
    "CodecId": "§2.2",
    "Driver": "§5.10",
    "Frame": "§5.6, a construction kind marker",
    "MemberOrder": "§5.6",
    "Members": "§5.6",
    "NativeRecord": "§2.7, a record of `Native`",
    "OperationFamily": "§6",
    "Ordered": "§5.6",
    "Plane": "§5.6, a construction kind marker",
    "Unordered": "§5.6",
}

# Types the specification names that the reference implementation does not yet
# define as types of their own: the analytic leaf carriers that §5.6's
# `InlineCurve` and `InlineSurface` hold, whose fields `SolvedCurveGeometry` and
# `SolvedSurfaceGeometry` state inline today. The annex states them once the
# schema defines them; a name here that the schema defines is an error, so the
# unit that adds one removes it here and regenerates.
AWAITING = {
    "CircleCurve", "ConeSurface", "CylinderSurface", "EllipseCurve", "HyperbolaCurve", "LineCurve",
    "ParabolaCurve", "PlaneSurface", "PolylineCurve", "SphereSurface", "TorusSurface",
}

# Rust's own types and the traits a sketch bounds a parameter by.
RUST = {
    "Box", "BTreeMap", "BTreeSet", "NonZeroI8", "NonZeroU32", "Option", "String", "Vec",
    "Clone", "Debug", "DeserializeOwned", "Eq", "JsonSchema", "Ord", "PartialEq", "PartialOrd",
    "Serialize", "Self",
}

RUST_BLOCK = re.compile(r"^[ \t]*```rust\n(.*?)^[ \t]*```", re.S | re.M)
TOKEN = re.compile(r"[A-Za-z_]\w*|::|->|[{}()\[\]<>,:;=&+]")
DECLARES = {"struct", "enum", "type", "trait"}
TYPE_CONTEXT = {":", "=", "->", "::", "&", "+", "dyn", "impl", "for"}


class ShapeError(Exception):
    """The specification or the schema is not shaped as this script reads it."""


def sketch_code(spec: str) -> str:
    code = "\n".join(RUST_BLOCK.findall(spec))
    code = re.sub(r"/\*.*?\*/", " ", code, flags=re.S)
    return re.sub(r"//[^\n]*", " ", code)


def stated_and_named(spec: str) -> tuple[set[str], set[str]]:
    """The types the sketches declare, and the types they use in a type position."""
    tokens = TOKEN.findall(sketch_code(spec))
    stated, named, brackets, previous = set(), set(), [], None
    for token in tokens:
        if token in "{([<":
            brackets.append(token)
        elif token in "})]>":
            if brackets:
                brackets.pop()
        elif token[0].isupper() and len(token) > 1:
            if previous in DECLARES:
                stated.add(token)
            elif (brackets and brackets[-1] in "([<") or previous in TYPE_CONTEXT:
                named.add(token)
        previous = token
    return stated, named - RUST


def is_id_type(name: str) -> bool:
    return name.endswith("Id") and name[:-2][:1].isupper()


def references(schema) -> set[str]:
    found = set()
    if isinstance(schema, dict):
        target = schema.get("$ref")
        if isinstance(target, str) and target.startswith(REF):
            found.add(target[len(REF):])
        for value in schema.values():
            found |= references(value)
    elif isinstance(schema, list):
        for value in schema:
            found |= references(value)
    return found


def annex_types(spec: str, defs: dict) -> list[str]:
    """Every named type that is not stated, and the unstated types their schemas reach."""
    stated, named = stated_and_named(spec)
    defined = sorted(name for name in AWAITING if name in defs)
    if defined:
        raise ShapeError("the schema now defines " + ", ".join(defined) + "; remove them from AWAITING")
    outside = lambda name: (name not in stated and name not in PROSE and name not in AWAITING
                            and not is_id_type(name))
    missing = sorted(name for name in named if outside(name) and name not in defs)
    if missing:
        raise ShapeError("the specification names types that it does not state and the schema does not define: "
                         + ", ".join(missing))
    annex, pending = set(), [name for name in named if outside(name)]
    while pending:
        name = pending.pop()
        if name in annex:
            continue
        annex.add(name)
        pending.extend(sorted(target for target in references(defs[name]) if outside(target) and target in defs))
    return sorted(annex)


def render_body(names: list[str], defs: dict) -> str:
    parts = []
    for name in names:
        schema = json.dumps(defs[name], indent=2, ensure_ascii=False)
        parts.append(f"### `{name}`\n\n```json\n{schema}\n```\n")
    return "\n".join(parts)


def split_annex(spec: str) -> tuple[str, str]:
    """The specification up to and including the annex's opening paragraph, and the rest."""
    start = spec.find(f"\n{HEADING}\n")
    if start < 0 or spec.count(f"\n{HEADING}\n") != 1:
        raise ShapeError(f"`{HEADING}` is not one heading of the specification")
    after_heading = start + len(HEADING) + 2
    paragraph = re.compile(r"\n*[^\n]+\n").match(spec, after_heading)
    if not paragraph:
        raise ShapeError("the annex has no opening paragraph")
    return spec[: paragraph.end()], spec[paragraph.end():]


def generate(spec: str, schema: dict) -> str:
    defs = schema.get("$defs")
    if not isinstance(defs, dict):
        raise ShapeError("the schema has no `$defs` object")
    head, _ = split_annex(spec)
    return f"{head}\n{render_body(annex_types(spec, defs), defs)}"


def load_schema(path: str | None) -> dict:
    return json.loads((Path(path) if path else SCHEMA).read_text(encoding="utf-8"))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--schema", help=f"read the schema from FILE instead of {SCHEMA.relative_to(ROOT)}")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="compare the annex with the schema (the default)")
    mode.add_argument("--write", action="store_true", help="rewrite the annex from the schema")
    args = parser.parse_args(argv)
    try:
        spec = SPEC.read_text(encoding="utf-8")
        expected = generate(spec, load_schema(args.schema))
    except (ShapeError, OSError, json.JSONDecodeError) as error:
        print(f"generate-annex-a: {error}", file=sys.stderr)
        return 2
    if args.write:
        if expected != spec:
            SPEC.write_text(expected, encoding="utf-8")
        return 0
    if expected == spec:
        return 0
    diff = difflib.unified_diff(spec.splitlines(keepends=True), expected.splitlines(keepends=True),
                                "docs/cadir-v11.md", "docs/cadir-v11.md (from the schema)", n=1)
    sys.stdout.writelines(list(diff)[:200])
    print("generate-annex-a: Annex A differs from the schema; run scripts/generate-annex-a.py --write",
          file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
