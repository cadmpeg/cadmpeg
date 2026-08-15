#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check crate-root pub names of cadmpeg-codec-* against docs/codec-facade.toml.

Codec crate roots are facades. Each ``lib.rs`` crate-root ``pub`` name must
match the ledger. Implementation stays ``pub(crate)`` or private. Do not fold
this into ``scripts/check-public-api-ledger.py``.
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER_REL = Path("docs") / "codec-facade.toml"
FUZZ_VALUES = frozenset({"required", "absent"})
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
ITEM_KINDS = (
    "struct",
    "enum",
    "fn",
    "mod",
    "type",
    "trait",
    "union",
    "static",
)
PREFIX_KINDS = frozenset({"async", "unsafe", "const", "extern"})


@dataclass(frozen=True)
class PubItem:
    name: str
    hidden: bool
    kind: str


@dataclass(frozen=True)
class CrateRow:
    crate_id: str
    names: tuple[str, ...]
    hidden: tuple[str, ...]
    fuzz: str
    fuzz_reason: str | None


def codec_ids(root: Path) -> list[str]:
    """Return sorted directory suffixes under ``crates/cadmpeg-codec-*``."""
    ids: list[str] = []
    crates = root / "crates"
    if not crates.is_dir():
        return ids
    for path in sorted(crates.glob("cadmpeg-codec-*")):
        if path.is_dir():
            ids.append(path.name.removeprefix("cadmpeg-codec-"))
    return ids


def _ident_char(char: str) -> bool:
    return char.isalnum() or char == "_"


def _word_at(text: str, index: int, word: str) -> bool:
    if not text.startswith(word, index):
        return False
    if index > 0 and _ident_char(text[index - 1]):
        return False
    end = index + len(word)
    return end >= len(text) or not _ident_char(text[end])


def _can_start_literal(text: str, index: int) -> bool:
    return index == 0 or not _ident_char(text[index - 1])


def _raw_string_open(text: str, index: int) -> tuple[int, int] | None:
    """Return ``(quote_index, hash_count)`` when a raw string starts at ``index``."""
    if not _can_start_literal(text, index):
        return None
    cursor = index
    if cursor < len(text) and text[cursor] in {"b", "c"}:
        cursor += 1
    if cursor >= len(text) or text[cursor] != "r":
        return None
    cursor += 1
    hashes = 0
    while cursor < len(text) and text[cursor] == "#":
        hashes += 1
        cursor += 1
    if cursor < len(text) and text[cursor] == '"':
        return cursor, hashes
    return None


def mask_literals(text: str) -> str:
    """Replace comment, string, and char-literal contents with spaces."""
    out = list(text)
    index = 0
    length = len(text)
    while index < length:
        raw = _raw_string_open(text, index)
        if raw is not None:
            quote, hashes = raw
            closer = '"' + ("#" * hashes)
            end = text.find(closer, quote + 1)
            stop = length if end < 0 else end + len(closer)
            for pos in range(index, stop):
                out[pos] = " "
            index = stop
            continue
        char = text[index]
        nxt = text[index + 1] if index + 1 < length else ""
        if char == "/" and nxt == "/":
            end = text.find("\n", index)
            stop = length if end < 0 else end
            for pos in range(index, stop):
                out[pos] = " "
            index = stop
            continue
        if char == "/" and nxt == "*":
            end = text.find("*/", index + 2)
            stop = length if end < 0 else end + 2
            for pos in range(index, stop):
                out[pos] = " "
            index = stop
            continue
        if char == '"' and _can_start_literal(text, index):
            cursor = index + 1
            while cursor < length:
                if text[cursor] == "\\":
                    cursor += 2
                    continue
                if text[cursor] == '"':
                    cursor += 1
                    break
                cursor += 1
            for pos in range(index, cursor):
                out[pos] = " "
            index = cursor
            continue
        if (
            char in {"b", "c"}
            and nxt == '"'
            and _can_start_literal(text, index)
        ):
            cursor = index + 2
            while cursor < length:
                if text[cursor] == "\\":
                    cursor += 2
                    continue
                if text[cursor] == '"':
                    cursor += 1
                    break
                cursor += 1
            for pos in range(index, cursor):
                out[pos] = " "
            index = cursor
            continue
        if char == "'" and _can_start_literal(text, index):
            if nxt == "\\":
                end = text.find("'", index + 2)
                stop = length if end < 0 else end + 1
                for pos in range(index, stop):
                    out[pos] = " "
                index = stop
                continue
            if index + 2 < length and text[index + 2] == "'":
                for pos in range(index, index + 3):
                    out[pos] = " "
                index += 3
                continue
        index += 1
    return "".join(out)


def _skip_ws(text: str, index: int) -> int:
    length = len(text)
    while index < length and text[index].isspace():
        index += 1
    return index


def _match_delimited(text: str, index: int, open_c: str, close_c: str) -> int:
    depth = 0
    length = len(text)
    while index < length:
        if text[index] == open_c:
            depth += 1
        elif text[index] == close_c:
            depth -= 1
            if depth == 0:
                return index + 1
        index += 1
    return length


def _split_top_commas(text: str) -> list[str]:
    parts: list[str] = []
    start = 0
    depth = 0
    for index, char in enumerate(text):
        if char == "{":
            depth += 1
        elif char == "}":
            depth = max(0, depth - 1)
        elif char == "," and depth == 0:
            parts.append(text[start:index])
            start = index + 1
    parts.append(text[start:])
    return parts


def parse_use_names(tree: str) -> list[str]:
    """Record exported names from the tree after ``pub use``."""
    names: list[str] = []
    _parse_use_node(tree.strip(), names)
    return names


def _parse_use_node(tree: str, names: list[str]) -> None:
    tree = tree.strip()
    if not tree:
        return
    brace = _top_level_char(tree, "{")
    if brace is not None:
        inner_end = _match_delimited(tree, brace, "{", "}")
        inner = tree[brace + 1 : inner_end - 1] if inner_end > brace + 1 else ""
        for part in _split_top_commas(inner):
            _parse_use_node(part, names)
        return
    if tree.rstrip().endswith("*"):
        names.append("*")
        return
    alias = re.split(r"\bas\b", tree)
    if len(alias) > 1:
        name = alias[-1].strip()
        if IDENT.fullmatch(name):
            names.append(name)
        return
    ident = None
    for match in IDENT.finditer(tree):
        ident = match.group()
    if ident and ident not in {"crate", "super", "self"}:
        names.append(ident)


def _top_level_char(text: str, needle: str) -> int | None:
    depth = 0
    for index, char in enumerate(text):
        if char == "{":
            if needle == "{" and depth == 0:
                return index
            depth += 1
        elif char == "}":
            depth = max(0, depth - 1)
    return None


def collect_root_pubs(text: str) -> list[PubItem]:
    """Return crate-root ``pub`` items. Depth-0 only; restricted vis is ignored."""
    masked = mask_literals(text)
    length = len(masked)
    index = 0
    depth = 0
    pending: list[str] = []
    items: list[PubItem] = []
    while index < length:
        if masked[index].isspace():
            index += 1
            continue
        if depth == 0 and masked.startswith("#[", index):
            end = _match_delimited(masked, index + 1, "[", "]")
            pending.append(masked[index:end])
            index = end
            continue
        if depth == 0 and _word_at(masked, index, "pub"):
            cursor = _skip_ws(masked, index + 3)
            if cursor < length and masked[cursor] == "(":
                index = _match_delimited(masked, cursor, "(", ")")
                pending = []
                continue
            attrs = pending
            pending = []
            hidden = any("doc(hidden)" in attr for attr in attrs)
            cfg_test = any("cfg(test)" in attr for attr in attrs)
            cursor = _skip_ws(masked, cursor)
            if _word_at(masked, cursor, "use"):
                use_start = _skip_ws(masked, cursor + 3)
                semi = use_start
                brace_depth = 0
                while semi < length:
                    if masked[semi] == "{":
                        brace_depth += 1
                    elif masked[semi] == "}":
                        brace_depth = max(0, brace_depth - 1)
                    elif masked[semi] == ";" and brace_depth == 0:
                        break
                    semi += 1
                if not cfg_test:
                    for name in parse_use_names(masked[use_start:semi]):
                        items.append(PubItem(name, hidden, "use"))
                index = min(semi + 1, length)
                continue
            kind, name, after = _item_kind_and_name(masked, cursor)
            if kind is not None and name is not None and not cfg_test:
                items.append(PubItem(name, hidden, kind))
            index = after if after != cursor else cursor + 1
            continue
        if depth == 0:
            pending = []
        if masked[index] == "{":
            depth += 1
        elif masked[index] == "}":
            depth = max(0, depth - 1)
        index += 1
    return items


def _item_kind_and_name(text: str, index: int) -> tuple[str | None, str | None, int]:
    cursor = _skip_ws(text, index)
    if _word_at(text, cursor, "impl"):
        return None, None, cursor
    while cursor < len(text):
        match = IDENT.match(text, cursor)
        if match is None:
            break
        word = match.group()
        after_word = _skip_ws(text, match.end())
        if word in PREFIX_KINDS:
            if word == "const" and not _word_at(text, after_word, "fn"):
                name_match = IDENT.match(text, after_word)
                if name_match:
                    return "const", name_match.group(), name_match.end()
                return None, None, after_word
            if word == "extern":
                cursor = after_word
                continue
            cursor = after_word
            continue
        if word in ITEM_KINDS:
            name_match = IDENT.match(text, after_word)
            if name_match:
                return word, name_match.group(), name_match.end()
            return None, None, after_word
        break
    return None, None, cursor


def parse_ledger(path: Path) -> list[CrateRow]:
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    raw_rows = data.get("crate", [])
    if not isinstance(raw_rows, list):
        raise ValueError("ledger [[crate]] is not an array")
    rows: list[CrateRow] = []
    for index, raw in enumerate(raw_rows):
        if not isinstance(raw, dict):
            raise ValueError(f"crate[{index}] is not a table")
        crate_id = raw.get("id")
        names = raw.get("names", [])
        hidden = raw.get("hidden", [])
        fuzz = raw.get("fuzz")
        fuzz_reason = raw.get("fuzz_reason")
        if not isinstance(crate_id, str) or not crate_id:
            raise ValueError(f"crate[{index}] missing id")
        if not isinstance(names, list) or not all(isinstance(name, str) for name in names):
            raise ValueError(f"crate[{crate_id}] names must be an array of strings")
        if not isinstance(hidden, list) or not all(isinstance(name, str) for name in hidden):
            raise ValueError(f"crate[{crate_id}] hidden must be an array of strings")
        if fuzz not in FUZZ_VALUES:
            raise ValueError(f"crate[{crate_id}] fuzz must be required or absent")
        if fuzz_reason is not None and not isinstance(fuzz_reason, str):
            raise ValueError(f"crate[{crate_id}] fuzz_reason must be a string")
        rows.append(
            CrateRow(
                crate_id=crate_id,
                names=tuple(names),
                hidden=tuple(hidden),
                fuzz=fuzz,
                fuzz_reason=fuzz_reason,
            )
        )
    return rows


def check_crate(row: CrateRow, items: list[PubItem]) -> list[str]:
    failures: list[str] = []
    prefix = row.crate_id
    by_name = {item.name: item for item in items}
    observed = set(by_name)
    allowed = set(row.names) | set(row.hidden)
    if "fuzz" in observed:
        allowed.add("fuzz")
    extras = sorted(observed - allowed)
    for name in extras:
        failures.append(f"{prefix}: extra pub name {name}")
    for name in row.names:
        if name not in observed:
            failures.append(f"{prefix}: missing pub name {name}")
    for name in row.hidden:
        item = by_name.get(name)
        if item is None:
            failures.append(f"{prefix}: missing hidden name {name}")
        elif not item.hidden:
            failures.append(f"{prefix}: hidden name {name} is not #[doc(hidden)]")
    fuzz_item = by_name.get("fuzz")
    if row.fuzz == "required":
        if fuzz_item is None or fuzz_item.kind != "mod":
            failures.append(f"{prefix}: fuzz is required but missing")
        elif not fuzz_item.hidden:
            failures.append(f"{prefix}: fuzz is required but not #[doc(hidden)]")
    elif fuzz_item is not None:
        failures.append(f"{prefix}: fuzz is absent but pub mod fuzz exists")
    if row.fuzz == "absent" and not row.fuzz_reason:
        failures.append(f"{prefix}: fuzz=absent requires fuzz_reason")
    return failures


def check(root: Path) -> list[str]:
    ledger_path = root / LEDGER_REL
    if not ledger_path.is_file():
        return [f"missing ledger {ledger_path.as_posix()}"]
    try:
        rows = parse_ledger(ledger_path)
    except (OSError, tomllib.TOMLDecodeError, ValueError) as err:
        return [f"ledger parse failed: {err}"]
    failures: list[str] = []
    seen: set[str] = set()
    for row in rows:
        if row.crate_id in seen:
            failures.append(f"ledger duplicate crate {row.crate_id}")
        seen.add(row.crate_id)
    dirs = set(codec_ids(root))
    for crate_id in sorted(dirs - seen):
        failures.append(f"ledger missing crate {crate_id}")
    for crate_id in sorted(seen - dirs):
        failures.append(f"ledger has {crate_id} but no crates/cadmpeg-codec-{crate_id}")
    for row in rows:
        if row.crate_id not in dirs:
            continue
        lib = root / "crates" / f"cadmpeg-codec-{row.crate_id}" / "src" / "lib.rs"
        if not lib.is_file():
            failures.append(f"{row.crate_id}: missing src/lib.rs")
            continue
        items = collect_root_pubs(lib.read_text(encoding="utf-8"))
        failures.extend(check_crate(row, items))
    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root",
        nargs="?",
        type=Path,
        default=ROOT,
        help="repository root (default: parent of scripts/)",
    )
    args = parser.parse_args(argv)
    root = args.root.resolve()
    failures = check(root)
    if failures:
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
        return 1
    print(f"codec facade: ok ({len(codec_ids(root))} crates)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
