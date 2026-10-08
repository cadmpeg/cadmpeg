#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check current source against repository policy; no Git history or ledger is needed.

Reports rule, file, line, and explanation. Exit 1 means violations were found.
Use --json for structured findings. See docs/source-policy.md for scope and limits.
"""

from __future__ import annotations

import argparse
import ast
from bisect import bisect_right
from dataclasses import asdict, dataclass
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TEST_LINE_LIMIT = 2000
PRODUCTION_LINE_LIMIT = 10000


@dataclass(frozen=True)
class Finding:
    rule: str
    path: str
    line: int
    message: str


FROM_ENDIAN = re.compile(r"\bfrom_(?:le|be)_bytes\b")
MALFORMED_FORMAT = re.compile(r"CodecError::Malformed\s*\(\s*format!", re.MULTILINE)
LOSS_NOTE_LIT = re.compile(r"\bLossNote\s*\{")
LOSS_NOTE_PATH = r"(?:::\s*)?(?:(?:r#)?[^\W\d]\w*\s*::\s*)*(?:r#)?(?P<name>LossNote)"
LOSS_NOTE_RETURN = re.compile(r"->\s*" + LOSS_NOTE_PATH + r"\s*\{")
LOSS_NOTE_STRUCT = re.compile(r"\bstruct\s+(?:r#)?(?P<name>LossNote)\s*\{")
LOSS_NOTE_IMPL = re.compile(r"\bimpl(?:<[^>]*>)?\s+" + LOSS_NOTE_PATH + r"\s*\{")
LOSS_NOTE_TRAIT_IMPL = re.compile(r"\bfor\s+" + LOSS_NOTE_PATH + r"\s*\{")
BARE_TOLERANCE = re.compile(
    r"(?<![0-9A-Za-z_.])1(?:\.0+)?[eE]-(?:6|7|8|9|10|11|12)\b"
)
# A whole-file pattern anchored at a line start takes its indentation with [^\S\n]*, never
# \s*: production_source blanks test code to whitespace, and \s* would run through every
# following blank line and backtrack, making the scan quadratic in the blanked length.
NAMED_TOLERANCE_DECL = re.compile(
    r"^[^\S\n]*(?:(?:pub(?:\([^)]*\))?|unsafe)\s+)*"
    r"(?:const|static)(?:\s+mut)?\s+[A-Za-z_][A-Za-z0-9_]*"
    r"\s*(?::[^=;]+)?=\s*",
    re.MULTILINE,
)
# The scanner below identifies `vec![value; count]` repeats structurally. A
# regular expression cannot distinguish the repeat separator from semicolons
# inside nested arrays or blocks. Comments and literals are already masked.
VEC_MACRO = re.compile(r"\bvec!\s*\[")
VEC_REPEAT_LITERAL = re.compile(r"^(?:0x[0-9a-fA-F]+|\d+)$")
ADMITTED_LEN_REPEAT = re.compile(
    r"^(?:[A-Za-z_][A-Za-z0-9_]*\.)*[A-Za-z_][A-Za-z0-9_]*\.len\(\)"
    r"(?:\s*[+-]\s*\d+)?$"
)
CFG_ATTR = re.compile(r"#\s*\[\s*cfg\s*\((.*)\)\s*\]\s*$", re.DOTALL)
PATH_ATTR = re.compile(r'#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]\s*$', re.DOTALL)
MOD_DECL = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*(;|\{)"
)


def is_production_rs(path: Path) -> bool:
    """True when ``path`` is a production ``.rs`` file under the filter."""
    if path.suffix != ".rs":
        return False
    parts = path.parts
    if any(part in {"tests", "test_support", "golden_tests", "integration_tests", "benches"} for part in parts):
        return False
    name = path.name
    if name == "tests.rs" or re.search(r"test", name, re.IGNORECASE):
        return False
    return "src" in parts


OUTER_ATTRIBUTE = re.compile(r"#\s*\[")


def attribute_end(code: str, start: int) -> int | None:
    """End of one outer attribute in masked Rust, retaining exact offsets."""
    opening = OUTER_ATTRIBUTE.match(code, start)
    if opening is None:
        return None
    depth = 1
    for index in range(opening.end(), len(code)):
        if code[index] == "[":
            depth += 1
        elif code[index] == "]":
            depth -= 1
            if depth == 0:
                return index + 1
    return None


def item_end(code: str, start: int) -> int | None:
    """End one attributed item without consuming a following same-line item.

    Nested delimiters in signatures and initializers do not end the item.
    An incomplete item is retained conservatively by the caller.
    """
    stack: list[str] = []
    closing = {")": "(", "]": "[", "}": "{"}
    for index in range(start, len(code)):
        char = code[index]
        if char in "([{":
            stack.append(char)
        elif char in closing:
            if not stack or stack.pop() != closing[char]:
                return None
            if char == "}" and not stack:
                end = index + 1
                after = end
                while after < len(code) and code[after].isspace():
                    after += 1
                return after + 1 if code[after:after + 1] == ";" else end
        elif char == ";" and not stack:
            return index + 1
    return None


def production_source(source: str) -> tuple[str, int]:
    """Mask non-code and test-only items; return code and production line count."""
    code = mask_rust_non_code(source)
    spans: list[tuple[int, int]] = []
    cursor = 0
    while match := OUTER_ATTRIBUTE.search(code, cursor):
        start = match.start()
        cursor = start
        attributes = []
        while OUTER_ATTRIBUTE.match(code, cursor):
            end = attribute_end(code, cursor)
            if end is None:
                cursor = len(code)
                break
            attributes.append(code[cursor:end])
            cursor = end
            while cursor < len(code) and code[cursor].isspace():
                cursor += 1
        if not any(attr_is_test_cfg(attribute) for attribute in attributes):
            continue
        end = item_end(code, cursor)
        if end is not None:
            spans.append((start, end))
            cursor = end
    if not spans:
        return code, len(code.splitlines())
    line_starts = [0, *(match.end() for match in re.finditer("\n", code))]
    affected_lines: set[int] = set()
    pieces = []
    cursor = 0
    for start, end in spans:
        pieces.extend((code[cursor:start], re.sub(r"[^\r\n]", " ", code[start:end])))
        affected_lines.update(range(
            bisect_right(line_starts, start) - 1,
            bisect_right(line_starts, end - 1),
        ))
        cursor = end
    pieces.append(code[cursor:])
    production = "".join(pieces)
    lines = production.splitlines()
    count = len(lines) - sum(not lines[index].strip() for index in affected_lines)
    return production, count


NON_CODE_START = re.compile(r'//|/\*|(?:br|cr|r)(?P<hashes>#{0,255})"|"|\'')
CHAR_LITERAL = re.compile(r"'(?:[^'\\\r\n]|\\(?:[nrt0\\'\"]|x[\da-fA-F]{2}|u\{[\da-fA-F_]+\}))'")
COMMENT_BOUNDARY = re.compile(r"/\*|\*/")
LINE_END = re.compile(r"[\r\n]")


def rust_non_code_spans(text: str):
    """Yield comment and literal spans, leaving lifetimes and labels intact."""
    index = 0
    while match := NON_CODE_START.search(text, index):
        start = match.start()
        index = match.end()
        token = match.group(0)
        if token == "//":
            end = LINE_END.search(text, index)
            index = len(text) if end is None else end.start()
        elif token == "/*":
            depth = 1
            while depth and index < len(text):
                boundary = COMMENT_BOUNDARY.search(text, index)
                if boundary is None:
                    index = len(text)
                    break
                depth += 1 if boundary.group(0) == "/*" else -1
                index = boundary.end()
        elif match.group("hashes") is not None:
            delimiter = '"' + match.group("hashes")
            end = text.find(delimiter, index)
            index = len(text) if end == -1 else end + len(delimiter)
        elif token == '"':
            while index < len(text):
                character = text[index]
                index += 1
                if character == "\\":
                    index = min(index + 1, len(text))
                elif character == '"':
                    break
        else:
            literal = CHAR_LITERAL.match(text, start)
            if literal is None:
                continue
            index = literal.end()
        yield start, index


def mask_rust_non_code(text: str) -> str:
    """Blank comments and literals while preserving positions and newlines."""

    pieces = []
    end = 0
    for start, stop in rust_non_code_spans(text):
        pieces.append(text[end:start])
        pieces.append(re.sub(r"[^\r\n]", " ", text[start:stop]))
        end = stop
    pieces.append(text[end:])
    return "".join(pieces)


ENDIAN_EXCEPTIONS = {"reconstructed-scalar", "packed-color-order"}
ENDIAN_MARKER = re.compile(r"^\s*// endian-exception: ([a-z-]+)\s*$")
# A `let _ = ...` in production source drops a value the code has already
# computed. The value is a refusal to thread, a binding to delete, or a
# side effect whose answer has no reader; only the third is a discard, and it
# states its own reason on the line above.
DISCARD = re.compile(r"(?<![\w:])let\s+_\s*(?::[^=;]+)?=")
DISCARD_MARKER = re.compile(r"^\s*// discarded-value: (\S.*?)\s*$")
# Fuzz entry points, with the reason each is outside the rule. A wrapper's whole
# contract is to run a parser over arbitrary bytes and drop the answer: the
# fuzzer reads the crash, never the value, and a refusal threaded out of one
# would narrow the input the parser sees.
DISCARD_EXEMPT_FILES = {
    "crates/cadmpeg-codec-nx/src/fuzz.rs":
        "fuzz entry points drop every parser answer by contract",
}


def standalone_markers(source: str, pattern: re.Pattern[str]) -> dict[int, str]:
    """Read standalone line comments, excluding lookalikes inside Rust literals."""
    markers = {}
    for start, end in rust_non_code_spans(source):
        marker = pattern.fullmatch(source[start:end])
        if marker is None:
            continue
        line_start = source.rfind("\n", 0, start) + 1
        if not source[line_start:start].strip():
            markers[source.count("\n", 0, start)] = marker[1]
    return markers


def endian_markers(source: str) -> dict[int, str]:
    """Read the standalone endian-exception comments of a source file."""
    return standalone_markers(source, ENDIAN_MARKER)


def discard_markers(source: str) -> dict[int, str]:
    """Read the standalone discarded-value comments of a source file."""
    return standalone_markers(source, DISCARD_MARKER)


def _vec_repeat_count(text: str, macro: re.Match[str]) -> str | None:
    stack = ["["]
    separator = None
    index = macro.end()
    while index < len(text) and stack:
        character = text[index]
        if character in "([{":
            stack.append(character)
        elif character in ")]}":
            expected = {")": "(", "]": "[", "}": "{"}[character]
            if stack[-1] == expected:
                stack.pop()
                if not stack:
                    return None if separator is None else text[separator + 1 : index].strip()
        elif character == ";" and len(stack) == 1 and separator is None:
            separator = index
        index += 1
    return None


def iter_vec_repeats(code: str):
    """Yield repeat offsets and counts from already-masked Rust code."""
    for macro in VEC_MACRO.finditer(code):
        count = _vec_repeat_count(code, macro)
        if count is not None:
            yield macro.start(), count


def relative_path(path: Path) -> str:
    return path.resolve().relative_to(ROOT.resolve()).as_posix()


def line_count(data: str) -> int:
    if not data:
        return 0
    return data.count("\n") + (0 if data.endswith("\n") else 1)


def is_crate_root_tests_rs(path: Path) -> bool:
    rel = path.resolve().relative_to(ROOT.resolve())
    return (
        len(rel.parts) == 4
        and rel.parts[0] == "crates"
        and rel.parts[2] == "src"
        and rel.parts[3] == "tests.rs"
    )


def structural_test_kind(path: Path) -> str | None:
    rel = path.resolve().relative_to(ROOT.resolve())
    parts = rel.parts
    if is_crate_root_tests_rs(path):
        return "test"
    if path.name == "golden_tests.rs" or "golden_tests" in parts[:-1]:
        return "golden"
    if path.name == "integration_tests.rs" or "integration_tests" in parts[:-1]:
        return "test"
    if path.name == "test_support.rs" or "test_support" in parts[:-1]:
        return "test"
    if "tests" in parts[:-1]:
        return "test"
    return None


def child_module_dir(path: Path) -> Path:
    if path.name in {"lib.rs", "main.rs", "mod.rs"}:
        return path.parent
    return path.parent / path.stem


def is_trivia_line(stripped: str) -> bool:
    return (
        not stripped
        or stripped.startswith("//")
        or stripped.startswith("/*")
        or stripped.startswith("*")
    )


def collect_attribute(lines: list[str], start: int) -> tuple[str, int]:
    depth = 0
    pieces: list[str] = []
    i = start
    while i < len(lines):
        line = lines[i]
        pieces.append(line)
        for ch in line:
            if ch == "[":
                depth += 1
            elif ch == "]":
                depth -= 1
        i += 1
        if depth <= 0:
            break
    return "".join(pieces), i


def attr_is_test_cfg(attr: str) -> bool:
    # The built-in test attribute removes its function from ordinary builds.
    if re.fullmatch(r"#\s*\[\s*test\s*\]", mask_rust_non_code(attr).strip()):
        return True
    match = CFG_ATTR.match(attr.strip())
    if match is None:
        return False
    body = mask_rust_non_code(match.group(1)).strip()
    # ponytail: recognize test and flat all(..., test, ...) gates only.
    # Retain other expressions as production; extend if new test-only forms occur.
    if body == "test":
        return True
    if not body.startswith("all(") or not body.endswith(")"):
        return False
    terms = body[4:-1]
    return "(" not in terms and ")" not in terms and any(
        term.strip() == "test" for term in terms.split(",")
    )


def path_attr_target(attr: str) -> str | None:
    match = PATH_ATTR.match(attr.strip())
    return match.group(1) if match is not None else None


def skip_item(lines: list[str], start: int) -> int:
    saw_brace = False
    depth = 0
    grouping = 0
    i = start
    while i < len(lines):
        for ch in lines[i]:
            if ch == "{":
                depth += 1
                saw_brace = True
            elif ch == "}":
                if saw_brace:
                    depth -= 1
            elif ch in "([":
                grouping += 1
            elif ch in ")]":
                grouping -= 1
            elif ch == ";" and not saw_brace and grouping <= 0:
                # A `;` inside a parameter list or an array type, such as
                # `Option<[f64; 3]>`, is not the end of the item.
                return i + 1
        i += 1
        if saw_brace and depth <= 0:
            return i
    return i


def find_matching_brace_end(lines: list[str], start: int) -> int:
    saw_brace = False
    depth = 0
    for i in range(start, len(lines)):
        for ch in lines[i]:
            if ch == "{":
                depth += 1
                saw_brace = True
            elif ch == "}":
                if saw_brace:
                    depth -= 1
            if saw_brace and depth == 0:
                return i
    return len(lines) - 1


def resolve_module_target(
    current_file: Path, child_dir: Path, module_name: str, explicit_path: str | None
) -> Path | None:
    if explicit_path is not None:
        candidate = (current_file.parent / explicit_path).resolve()
        return candidate if candidate.is_file() else None
    for candidate in (
        child_dir / f"{module_name}.rs",
        child_dir / module_name / "mod.rs",
    ):
        if candidate.is_file():
            return candidate
    return None


# Hand-written one-item charge followed by a map or set insert. The core
# operations charge only when a new key is inserted; a charge before the
# insert also bills a key that was already present.
CHARGE_ONE_CALL = re.compile(r"(?<![\w])charge_collection_items\s*\(")
CHARGE_ONE_ARG = re.compile(r"1|u64_from_index\s*\(\s*1\s*\)")
CHARGE_RECEIVER = re.compile(r"[\w.\s]*")
INSERT_STATEMENT = re.compile(
    r"(?:let\s+[^=;]*?=\s*)?(?P<place>[A-Za-z_]\w*(?:\s*\.\s*[A-Za-z_]\w*)*)"
    r"\s*\.\s*(?P<method>insert|entry)\s*\("
)
SKIPPED_STATEMENT = re.compile(
    r"(?:let\b|(?:[\w.\s]*\.\s*)?(?:charge_retained|charge_work)\s*\(|[\w.\s]*\.\s*grow\s*\()"
)
STOPPING_STATEMENT = re.compile(r"(?:return|continue|break)\b")
KEYED_COLLECTION = r"(?:BTreeMap|HashMap|BTreeSet|HashSet)"
FN_ITEM = re.compile(r"\bfn\s+[A-Za-z_]\w*")
COLLECTION_CHARGE_EXEMPT_FILES = {
    "crates/cadmpeg-core/src/decode/collect.rs",
    "crates/cadmpeg-core/src/decode/context.rs",
}
COLLECTION_CHARGE_MESSAGE = (
    "Use ctx.insert_btree_set / insert_btree_map / admit_btree_entry (or the hash forms "
    "insert_hash_set / insert_hash_map / admit_hash_map_entry), which charge only a new key, "
    "instead of charging one collection item by hand before the insert."
)
_OPENERS = {"(": ")", "[": "]", "{": "}"}


def matching_close(code: str, open_index: int) -> int | None:
    """Index of the delimiter closing the one at ``open_index`` in masked Rust."""
    stack: list[str] = []
    for index in range(open_index, len(code)):
        char = code[index]
        if char in _OPENERS:
            stack.append(_OPENERS[char])
        elif char in ")]}":
            if not stack or stack.pop() != char:
                return None
            if not stack:
                return index
    return None


def top_level_arguments(code: str, open_index: int, close_index: int) -> list[str]:
    """Split the arguments between a call's parentheses at depth-zero commas."""
    arguments, depth, start = [], 0, open_index + 1
    for index in range(open_index + 1, close_index):
        char = code[index]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == "," and depth == 0:
            arguments.append(code[start:index])
            start = index + 1
    arguments.append(code[start:close_index])
    if len(arguments) > 1 and not arguments[-1].strip():
        arguments.pop()
    return [argument.strip() for argument in arguments]


def next_statement(code: str, start: int) -> tuple[int, str, int] | None:
    """Offset, text and end of the statement at ``start``; None at the block's end."""
    index = start
    while index < len(code) and code[index].isspace():
        index += 1
    if index >= len(code) or code[index] == "}":
        return None
    begin, depth = index, 0
    while index < len(code):
        char = code[index]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            if depth == 0:
                return begin, code[begin:index], index
            depth -= 1
            if char == "}" and depth == 0:
                after = index + 1
                while after < len(code) and code[after].isspace():
                    after += 1
                if code[after:after + 1] not in {";", ".", "?"} and not code.startswith("else", after):
                    return begin, code[begin:index + 1], index + 1
        elif char == ";" and depth == 0:
            return begin, code[begin:index], index + 1
        index += 1
    return begin, code[begin:], len(code)


def enclosing_function(code: str, offset: int) -> str:
    """Signature and body of the innermost function containing ``offset``."""
    best = None
    for match in FN_ITEM.finditer(code, 0, offset):
        brace = code.find("{", match.end())
        semicolon = code.find(";", match.end())
        if brace == -1 or (semicolon != -1 and semicolon < brace):
            continue
        close = matching_close(code, brace)
        if close is not None and close >= offset:
            best = (match.start(), close + 1)
    return code[best[0]:best[1]] if best else ""


def declares_keyed_collection(function: str, name: str) -> bool:
    """True when ``name`` is a parameter or let binding of a map or set in ``function``."""
    ident = re.escape(name)
    if re.search(
        rf"(?<![\w.]){ident}\s*:\s*(?:&\s*(?:'\w+\s*)?(?:mut\s+)?)?(?:\w+\s*::\s*)*{KEYED_COLLECTION}\b",
        function,
    ):
        return True
    for binding in re.finditer(rf"\blet\s+(?:mut\s+)?{ident}\b", function):
        end, depth = binding.end(), 0
        while end < len(function):
            char = function[end]
            if char in "([{":
                depth += 1
            elif char in ")]}":
                depth -= 1
            elif char == ";" and depth <= 0:
                break
            end += 1
        if re.search(rf"\b{KEYED_COLLECTION}\b", function[binding.end():end]):
            return True
    return False


def keyed_insert(statement: str, function: str) -> bool:
    """True when ``statement`` inserts into, or takes an entry of, a local map or set."""
    match = INSERT_STATEMENT.match(statement)
    if match is None:
        return False
    open_index = match.end() - 1
    close = matching_close(statement, open_index)
    if close is None:
        return False
    arguments = top_level_arguments(statement, open_index, close)
    place = re.sub(r"\s+", "", match["place"])
    if "." in place or not declares_keyed_collection(function, place):
        return False
    if match["method"] == "entry":
        return True
    return len(arguments) in {1, 2}


def charges_nested_member(code: str, statement: str, cursor: int, charge: int) -> bool:
    """Recognize a charged vector append or singleton set below an admitted map entry."""
    match = INSERT_STATEMENT.match(statement)
    if match is None:
        return False
    opening = match.end() - 1
    close = matching_close(statement, opening)
    if close is None:
        return False
    arguments = top_level_arguments(statement, opening, close)
    if match["method"] == "entry":
        binding = re.match(r"let\s+(?:mut\s+)?([A-Za-z_]\w*)\s*=", statement)
        if binding is None or not re.fullmatch(r"\s*\.\s*or_default\s*\(\s*\)\s*", statement[close + 1:]):
            return False
        following = next_statement(code, cursor)
        if following is None:
            return False
        _, reserve, after_reserve = following
        if not re.match(r"(?:[A-Za-z_]\w*\s*::\s*)*reserve_admitted_vec\s*\(", reserve):
            return False
        reserve_open = reserve.index("(")
        reserve_close = matching_close(reserve, reserve_open)
        if reserve_close is None:
            return False
        reserve_args = top_level_arguments(reserve, reserve_open, reserve_close)
        if len(reserve_args) < 2 or reserve_args[:2] != [binding[1], "1"]:
            return False
        append = next_statement(code, after_reserve)
        return append is not None and re.match(re.escape(binding[1]) + r"\s*\.\s*push\s*\(", append[1]) is not None
    if match["method"] != "insert" or len(arguments) != 2:
        return False
    singleton = re.fullmatch(r"(?:[A-Za-z_]\w*\s*::\s*)*BTreeSet\s*::\s*from\s*\(\s*\[([^\[\]]*)\]\s*\)", arguments[1])
    if singleton is None or "," in singleton[1] or not singleton[1].strip():
        return False
    place = re.sub(r"\s+", "", match["place"])
    key = re.sub(r"\s+", "", arguments[0])
    prefix = code[:charge]
    admission = re.compile(r"\.\s*admit_btree_entry\s*\(")
    for admitted in admission.finditer(prefix):
        admission_close = matching_close(prefix, admitted.end() - 1)
        if admission_close is None:
            continue
        args = top_level_arguments(prefix, admitted.end() - 1, admission_close)
        if len(args) >= 2 and re.sub(r"\s+", "", args[0]).removeprefix("&mut").removeprefix("&") == place and re.sub(r"\s+", "", args[1]).removeprefix("&") == key:
            tail = prefix[admission_close + 1:]
            if re.fullmatch(r"\s*\?\s*;\s*[\w.\s]*", tail):
                return True
    return False


def scan_hand_charged_inserts(code: str):
    """Yield (charge offset, insert offset) for each hand-charged map or set insert."""
    for call in CHARGE_ONE_CALL.finditer(code):
        open_index = call.end() - 1
        close = matching_close(code, open_index)
        if close is None:
            continue
        arguments = top_level_arguments(code, open_index, close)
        if not arguments or not CHARGE_ONE_ARG.fullmatch(arguments[0]):
            continue
        lead = max(code.rfind(";", 0, call.start()), code.rfind("{", 0, call.start()),
                   code.rfind("}", 0, call.start()))
        if not CHARGE_RECEIVER.fullmatch(code[lead + 1:call.start()]):
            continue
        after = close + 1
        while code[after:after + 1].isspace() or code[after:after + 1] == "?":
            after += 1
        if code[after:after + 1] != ";":
            continue
        cursor, function = after + 1, None
        while (found := next_statement(code, cursor)) is not None:
            begin, statement, cursor = found
            if STOPPING_STATEMENT.match(statement):
                break
            if function is None:
                function = enclosing_function(code, call.start())
            if keyed_insert(statement, function):
                if charges_nested_member(code, statement, cursor, call.start()):
                    break
                yield call.start(), begin
                break
            if not SKIPPED_STATEMENT.match(statement):
                break


def scan_saturating_arithmetic(path: Path, code: str) -> list[Finding]:
    """Require checked arithmetic with an explicit overflow branch."""
    return [Finding(
        "saturating_arithmetic", relative_path(path), code.count("\n", 0, match.start()) + 1,
        "Use checked arithmetic and propagate a resource refusal or typed error on overflow.",
    ) for match in re.finditer(r"\bsaturating_\w+\s*(?:::\s*<[^;{}]*>)?\s*\(", code)]


def integer_limit_default(words: list[str]) -> bool:
    """Recognize a bound expression or a closure returning that bound."""
    words = list(words)
    if words[:1] == ["move"]:
        words = words[1:]
    if words[:1] == ["|"]:
        tokens, _, parents = evaluation_tokens(" ".join(words))
        closing = next((at for at in range(1, len(tokens))
                        if tokens[at][0] == "|" and at not in parents), None)
        if closing is None:
            return False
        words = words[closing + 1:]
        if words[:1] == ["->"]:
            if "{" not in words:
                return False
            words = words[words.index("{"):]
    while len(words) >= 2 and words[0] in {"(", "{"}:
        # Remove only delimiters enclosing the complete expression.
        tokens, local_pairs, _ = evaluation_tokens(" ".join(words))
        if local_pairs.get(0) != len(tokens) - 1:
            break
        words = words[1:-1]
    if words[:1] == ["return"]:
        words = words[1:]
        if words[-1:] == [";"]:
            words = words[:-1]
    bound = "".join(words)
    primitive = r"(?:::)?(?:(?:std|core)::(?:primitive::)?)?[ui](?:8|16|32|64|128|size)"
    return re.fullmatch(r"(?:" + primitive + r"|<" + primitive + r">)::(?:MAX|MIN)", bound) is not None


def scan_integer_clamps(path: Path, code: str) -> list[Finding]:
    """Reject integer-bound defaults in Option and Result combinators."""
    findings = []
    tokens, pairs, parents = evaluation_tokens(code)
    words = [token[0] for token in tokens]
    for index, word in enumerate(words):
        if word not in {"unwrap_or", "unwrap_or_else", "map_or", "map_or_else"}:
            continue
        opening = evaluation_call_open(words, index)
        if opening is None or opening not in pairs:
            continue
        end = pairs[opening]
        argument_start = opening + 1
        if words[argument_start:argument_start + 1] == ["move"]:
            argument_start += 1
        if words[argument_start:argument_start + 1] == ["|"]:
            # Parameter commas, including generic type arguments, stay inside
            # the closure's two top-level pipes.
            closing = next((at for at in range(argument_start + 1, end)
                            if words[at] == "|" and parents.get(at) == opening), None)
            if closing is None:
                continue
            argument_start = closing + 1
        for argument_end in range(argument_start, end):
            if words[argument_end] == "," and parents.get(argument_end) == opening:
                end = argument_end
                break
        if integer_limit_default(words[opening + 1:end]):
            findings.append(Finding(
                "integer_clamp", relative_path(path), code.count("\n", 0, tokens[index].start()) + 1,
                "Use an exact conversion or an explicit refusal branch; represent a missing bound as Option instead of an integer limit.",
            ))
    return findings


OPTIONAL_DECODE_CONTEXT = re.compile(
    r"\bOption\s*<\s*&\s*(?:'[A-Za-z_]\w*\s*)?(?:mut\s+)?"
    r"(?:::\s*)?(?:(?:[A-Za-z_]\w*)\s*::\s*)*DecodeContext\b"
)


def scan_optional_decode_contexts(path: Path, code: str) -> list[Finding]:
    """Require a decode context in function parameters, never an optional one."""
    findings = []
    tokens, pairs, _ = evaluation_tokens(code)
    words = [token[0] for token in tokens]
    reported: set[int] = set()
    for index, word in enumerate(words):
        if word != "fn":
            continue
        opening = index + (1 if words[index + 1:index + 2] == ["("] else 2)
        if words[index + 1:index + 3] == ["r", "#"]:
            opening += 2
        if words[opening:opening + 1] == ["<"]:
            depth = 1
            opening += 1
            while opening < len(words) and depth:
                depth += (words[opening] == "<") - (words[opening] == ">")
                opening += 1
        if words[opening:opening + 1] != ["("] or opening not in pairs:
            continue
        start = tokens[opening].end()
        end = tokens[pairs[opening]].start()
        for match in OPTIONAL_DECODE_CONTEXT.finditer(code, start, end):
            if match.start() in reported:
                continue
            reported.add(match.start())
            findings.append(Finding(
                "optional_decode_context", relative_path(path), code.count("\n", 0, match.start()) + 1,
                "The decode path takes its caller DecodeContext; context-free reconstruction and writing take no context.",
            ))
    return findings


CONVERSION_EXPECTATION = {
    "clippy::as_conversions", "clippy::cast_possible_truncation",
    "clippy::cast_precision_loss", "clippy::cast_sign_loss",
}


def scan_lint_suppressions(path: Path, source: str, code: str) -> list[Finding]:
    """Keep lint suppressions in tests and the one checked conversion module."""
    findings = []
    tokens, pairs, parents = evaluation_tokens(code)
    words = [token[0] for token in tokens]
    admitted_conversion = False
    for index, word in enumerate(words):
        if word != "#":
            continue
        inner = words[index + 1:index + 2] == ["!"]
        opening = index + (2 if inner else 1)
        if words[opening:opening + 1] != ["["] or opening not in pairs:
            continue
        end = pairs[opening]
        suppressions = [at for at in range(opening + 1, end)
                        if words[at] in {"allow", "expect"} and words[at + 1:at + 2] == ["("]]
        production_suppressions = []
        for suppression in suppressions:
            ancestor = parents.get(suppression)
            test_only = False
            while ancestor is not None and ancestor != opening:
                if ancestor > 0 and words[ancestor - 1] == "cfg_attr":
                    condition_end = next((at for at in range(ancestor + 1, pairs[ancestor])
                                          if words[at] == "," and parents.get(at) == ancestor), None)
                    if condition_end is not None:
                        condition = "".join(words[ancestor + 1:condition_end])
                        if attr_is_test_cfg("#[cfg(" + condition + ")]"):
                            test_only = True
                            break
                ancestor = parents.get(ancestor)
            if not test_only:
                production_suppressions.append(suppression)
        suppressions = production_suppressions
        if not suppressions:
            continue
        allowed = False
        if (relative_path(path) == "crates/cadmpeg-core/src/convert.rs"
                and not admitted_conversion and inner and index not in parents
                and words[opening + 1:opening + 3] == ["expect", "("]):
            attribute = code[tokens[opening].end():tokens[end].start()]
            lints = set(re.findall(r"clippy\s*::\s*([A-Za-z_]\w*)", attribute))
            lints = {"clippy::" + lint for lint in lints}
            remaining = re.sub(r"clippy\s*::\s*[A-Za-z_]\w*", "", attribute)
            remaining = re.sub(r"reason\s*=", "", remaining)
            remaining = re.sub(r"[\s,()]|expect", "", remaining)
            raw = source[tokens[opening].end():tokens[end].start()]
            reason = re.search(r'reason\s*=\s*"([^"\n]*)"', raw)
            allowed = (lints == CONVERSION_EXPECTATION and not remaining
                       and reason is not None and bool(reason[1].strip())
                       and len(suppressions) == 1)
            admitted_conversion = allowed
        if not allowed:
            findings.append(Finding(
                "lint_suppression", relative_path(path), code.count("\n", 0, tokens[index].start()) + 1,
                "Fix the lint instead of suppressing it; only tests and the single module expectation in core convert.rs are exempt.",
            ))
    return findings


WRAPPING_CALL = re.compile(r"\bwrapping_\w+\s*(?:::\s*<[^;{}]*>)?\s*\(")
WRAPPING_MARKER = re.compile(r"^\s*// wrapping-exception: (\S.*?)\s*$")


def scan_wrapping_arithmetic(path: Path, source: str, code: str) -> list[Finding]:
    """Admit one format-defined modular operation per local reason."""
    findings = []
    markers = standalone_markers(source, WRAPPING_MARKER)
    # Mark comment positions with identifiers before masking test items. This
    # keeps test-only markers out of stale-marker checks without parsing Rust twice per marker.
    probe = source.splitlines(keepends=True)
    for index in markers:
        probe[index] = re.sub(r"[^\r\n]", "x", probe[index])
    active, _ = production_source("".join(probe))
    active_lines = active.splitlines()
    markers = {index: reason for index, reason in markers.items() if active_lines[index].strip()}
    calls_by_line: dict[int, int] = {}
    for match in WRAPPING_CALL.finditer(code):
        index = code.count("\n", 0, match.start())
        calls_by_line[index] = calls_by_line.get(index, 0) + 1
    for index in markers:
        if calls_by_line.get(index + 1, 0) != 1:
            findings.append(Finding(
                "wrapping_exception", relative_path(path), index + 1,
                "Stale wrapping exception; annotate exactly one modular call on the next line.",
            ))
    for index, count in calls_by_line.items():
        if index - 1 in markers and count == 1:
            continue
        for _ in range(count):
            findings.append(Finding(
                "wrapping_arithmetic", relative_path(path), index + 1,
                "Use checked arithmetic; format-defined modular arithmetic requires a standalone wrapping-exception reason on the preceding line.",
            ))
    return findings


def scan_patterns(path: Path, source: str) -> list[Finding]:
    """Inspect each source pattern once and report its location."""
    code, size = production_source(source)
    findings = scan_saturating_arithmetic(path, code)
    findings.extend(scan_wrapping_arithmetic(path, source, code))
    findings.extend(scan_lint_suppressions(path, source, code))
    findings.extend(scan_optional_decode_contexts(path, code))

    def report(rule: str, line: int, message: str) -> None:
        findings.append(Finding(rule, relative_path(path), line, message))

    if size > PRODUCTION_LINE_LIMIT:
        report("production_size", 1,
               f"Production file has {size} lines excluding cfg(test) items; limit is {PRODUCTION_LINE_LIMIT}.")

    markers = endian_markers(source)
    lines = code.splitlines()
    for index, reason in markers.items():
        following = lines[index + 1] if index + 1 < len(lines) else ""
        if reason not in ENDIAN_EXCEPTIONS or len(FROM_ENDIAN.findall(following)) != 1:
            report("endian_exception", index + 1, "Unknown or stale endian exception; annotate exactly one call on the next line.")
    for index, line in enumerate(lines):
        calls = list(FROM_ENDIAN.finditer(line))
        if markers.get(index - 1) in ENDIAN_EXCEPTIONS:
            calls = calls[1:]
        for _ in calls:
            report("unapproved_endian_read", index + 1, "Use a bounded View read; reconstructed scalars and packed color ordering require a local endian exception.")

    if relative_path(path) not in DISCARD_EXEMPT_FILES:
        discards = discard_markers(source)
        for index in discards:
            following = lines[index + 1] if index + 1 < len(lines) else ""
            if len(DISCARD.findall(following)) != 1:
                report("discarded_value", index + 1,
                       "Stale discarded-value reason; state exactly one `let _ =` on the next line.")
        for index, line in enumerate(lines):
            sites = list(DISCARD.finditer(line))
            if index - 1 in discards:
                sites = sites[1:]
            for _ in sites:
                report("discarded_value", index + 1,
                       "This `let _ =` drops a computed value. Thread its refusal, delete the binding, or state why the answer has no reader in a `// discarded-value:` comment on the line above.")

    # Exclude the type occurrence, not its entire line: the same function may
    # construct a LossNote immediately after its return type and opening brace.
    loss_note_types = {
        match.start("name")
        for pattern in (LOSS_NOTE_RETURN, LOSS_NOTE_STRUCT, LOSS_NOTE_IMPL, LOSS_NOTE_TRAIT_IMPL)
        for match in pattern.finditer(code)
    }
    for match in LOSS_NOTE_LIT.finditer(code):
        if match.start() not in loss_note_types:
            report("loss_note_literal", code.count("\n", 0, match.start()) + 1,
                   "Construct loss notes through the owning loss code's note method.")

    for match in MALFORMED_FORMAT.finditer(code):
        report("formatted_malformed_error", code.count("\n", 0, match.start()) + 1,
               "Use a structured codec error instead of Malformed(format!(...)).")
    findings.extend(scan_integer_clamps(path, code))
    declarations = {match.end() for match in NAMED_TOLERANCE_DECL.finditer(code)}
    for match in BARE_TOLERANCE.finditer(code):
        if match.start() not in declarations:
            report("bare_tolerance", code.count("\n", 0, match.start()) + 1,
                   "Give the tolerance a module-local constant name that states its intent.")
    if relative_path(path) not in COLLECTION_CHARGE_EXEMPT_FILES:
        for charge, _insert in scan_hand_charged_inserts(code):
            report("hand_charged_collection_insert", code.count("\n", 0, charge) + 1,
                   COLLECTION_CHARGE_MESSAGE)
    for offset, count in iter_vec_repeats(code):
        if not VEC_REPEAT_LITERAL.fullmatch(count) and not ADMITTED_LEN_REPEAT.fullmatch(count):
            report("unchecked_vec_repeat", code.count("\n", 0, offset) + 1,
                   "Use checked allocation for a repeat whose count is not a literal or an admitted collection length.")
    return findings


def scan_placement(sources: dict[Path, str]) -> list[Finding]:
    """Walk test ownership once, including standard and explicit module paths."""
    findings = []
    scanned_tests: set[Path] = set()

    def report(rule: str, path: Path, line: int, message: str) -> None:
        findings.append(Finding(rule, relative_path(path), line, message))

    def scan_test(path: Path) -> None:
        path = path.resolve()
        if path in scanned_tests:
            return
        scanned_tests.add(path)
        if path not in sources:
            sources[path] = path.read_text(encoding="utf-8", errors="replace")
        text = sources[path]
        size = line_count(text)
        if structural_test_kind(path) != "golden" and size > TEST_LINE_LIMIT:
            report("test_size", path, 1, f"Test file has {size} lines; limit is {TEST_LINE_LIMIT}.")
        scan_block(text, path, child_module_dir(path), True, False, False, 0)

    def scan_block(
        text: str, path: Path, child_dir: Path, file_test: bool,
        parent_test: bool, counted_inline: bool, line_offset: int,
    ) -> None:
        lines = text.splitlines(keepends=True)
        i = 0
        pending_attrs = []
        pending_start = 0
        while i < len(lines):
            stripped = lines[i].lstrip()
            if stripped.startswith("#["):
                if not pending_attrs:
                    pending_start = i
                attr, i = collect_attribute(lines, i)
                pending_attrs.append(attr)
                continue
            if is_trivia_line(stripped):
                i += 1
                continue
            match = MOD_DECL.match(lines[i])
            if match is None:
                pending_attrs = []
                i += 1
                continue
            module_name, marker = match.groups()
            attrs = pending_attrs
            attr_start = pending_start if pending_attrs else i
            pending_attrs = []
            explicit_path = None
            for attr in attrs:
                explicit_path = path_attr_target(attr) or explicit_path
            module_test = file_test or parent_test or any(attr_is_test_cfg(attr) for attr in attrs)
            if explicit_path is not None and module_test:
                report("test_path_include", path, line_offset + attr_start + 1,
                       f"Test module uses #[path = {explicit_path!r}]; use its owning module's standard path.")
            if marker == ";":
                target = resolve_module_target(path, child_dir, module_name, explicit_path)
                if target is not None and (module_test or structural_test_kind(target) is not None):
                    scan_test(target)
                i += 1
                continue
            end = find_matching_brace_end(lines, i)
            block = "".join(lines[i:end + 1])
            opening, closing = block.find("{"), block.rfind("}")
            body = block[opening + 1:closing] if opening >= 0 and closing > opening else ""
            nested_counted = counted_inline
            if module_test and not file_test and not counted_inline:
                size = line_count("".join(lines[attr_start:end + 1]))
                if size > TEST_LINE_LIMIT:
                    report("test_size", path, line_offset + attr_start + 1,
                           f"Inline test module {module_name} has {size} lines; limit is {TEST_LINE_LIMIT}.")
                nested_counted = True
            scan_block(body, path, child_dir / module_name, file_test, module_test,
                       nested_counted, line_offset + i + block[:opening + 1].count("\n"))
            i = end + 1

    paths = sorted(sources)
    for path in paths:
        if is_crate_root_tests_rs(path):
            report("crate_root_tests", path, 1, "Declare unit tests under their production owners, not src/tests.rs.")
        if structural_test_kind(path) is not None:
            scan_test(path)
    for path in paths:
        if is_production_rs(path) and path not in scanned_tests:
            scan_block(sources[path], path, child_module_dir(path), False, False, False, 0)
    return findings


MOD_DECL_VIS = re.compile(
    r"^\s*(?:(?P<vis>pub)(?:\s*\((?P<scope>[^)]*)\))?\s+)?mod\s+"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*(?P<marker>;|\{)"
)
# A module-level item at column zero. An associated item, a struct field and an
# enum variant are indented, so the anchor alone keeps them out of the rule.
PATH_ONLY_ITEM = re.compile(
    r"^pub\s*\(\s*(?P<scope>[^)]*?)\s*\)\s+"
    r"(?:(?:unsafe|async|extern\s+\"[^\"]*\")\s+)*"
    r"(?:fn|const|static)\b"
)
REEXPORT_USE = re.compile(r"^[^\S\n]*pub(?:\s*\([^)]*\))?\s+use\s+(?P<path>[^;]*);", re.MULTILINE)
IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def narrower_scope(left: tuple[str, ...], right: tuple[str, ...]) -> tuple[str, ...]:
    """Whichever of two module paths lies inside the other."""
    return left if len(left) >= len(right) else right


def spelled_scope(parent: tuple[str, ...], text: str) -> tuple[str, ...] | None:
    """The module a restriction written inside ``parent`` names, or None for bare `pub`."""
    text = text.strip()
    if text == "crate":
        return ()
    if text == "self":
        return parent
    if text == "super":
        return parent[:-1]
    if text.startswith("in "):
        segments = tuple(part.strip() for part in text[3:].split("::") if part.strip())
        if segments[:1] == ("crate",):
            return segments[1:]
        if segments[:1] == ("self",):
            return parent + segments[1:]
        if segments[:1] == ("super",):
            return parent[:-1] + segments[1:]
        return segments
    return None


def module_scopes(crate_root: Path) -> list[tuple[Path, tuple[str, ...], tuple[str, ...]]]:
    """Every file module of one crate, with the module subtree that can name it.

    ``scope`` is the widest module from which a path naming the module can be
    written. The crate root has the whole crate. A child declared ``pub``
    inherits its parent's scope, ``pub(crate)`` widens to the whole crate, and a
    declaration with no marker caps the child at the parent, because no outside
    module can spell the parent's private child.
    """
    modules: list[tuple[Path, tuple[str, ...], tuple[str, ...]]] = []
    seen: set[Path] = set()

    def walk(path: Path, module: tuple[str, ...], scope: tuple[str, ...]) -> None:
        path = path.resolve()
        if path in seen:
            return
        seen.add(path)
        modules.append((path, module, scope))
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines(keepends=True)
        body(path, lines, 0, len(lines), child_module_dir(path), module, scope)

    def body(
        path: Path, lines: list[str], index: int, end: int,
        child_dir: Path, module: tuple[str, ...], scope: tuple[str, ...],
    ) -> None:
        attrs: list[str] = []
        while index < end:
            stripped = lines[index].lstrip()
            if stripped.startswith("#["):
                attr, index = collect_attribute(lines, index)
                attrs.append(attr)
                continue
            if is_trivia_line(stripped):
                index += 1
                continue
            match = MOD_DECL_VIS.match(lines[index])
            pending, attrs = attrs, []
            if match is None:
                index = skip_item(lines, index)
                continue
            test_gated = any(attr_is_test_cfg(attr) for attr in pending)
            child = module + (match.group("name"),)
            if match.group("vis") is None:
                child_scope = module
            else:
                spelled = spelled_scope(module, match.group("scope") or "")
                child_scope = scope if spelled is None else narrower_scope(scope, spelled)
            if match.group("marker") == "{":
                stop = find_matching_brace_end(lines, index) + 1
                if not test_gated:
                    body(path, lines, index + 1, stop,
                         child_dir / match.group("name"), child, child_scope)
                index = stop
                continue
            index += 1
            if test_gated:
                continue
            explicit = None
            for attr in pending:
                explicit = path_attr_target(attr) or explicit
            target = resolve_module_target(path, child_dir, match.group("name"), explicit)
            if target is not None:
                walk(target, child, child_scope)

    walk(crate_root, (), ())
    return modules


# A serde wire mirror is the type a serde conversion container attribute names.
# The mirror spells the wire
# shape of the admitted type, and its member documentation is what the
# published JSON schema reads as each property's `description`. The rule covers
# the shared crates that publish JSON Schema through their `schema` features. A
# codec crate's own records stay outside it because they generate no schema and
# state their shape through `NativeRecord`.
#
# A member the mirror carries with `#[serde(flatten)]` publishes no property of
# its own: the properties are the members of the flattened type, so that type is
# a mirror as well and its members carry the same rule. The flattened type is
# resolved as a Rust path by the same rules as the initial mirror. A type the
# flattened type reaches through anything other than a further
# `#[serde(flatten)]` is not resolved.
WIRE_MIRROR_DOC_ROOTS = (
    "crates/cadmpeg-ir",
    "crates/cadmpeg-core",
    "crates/cadmpeg-asm",
)
SERDE_ATTRIBUTE = re.compile(r"#\s*\[\s*serde\s*\(")
SERDE_MIRROR_TARGET = re.compile(
    r"(?<![\w.])(?:try_from|from|into)\s*=\s*\"(?P<target>[^\"]+)\""
)
SERDE_FLATTEN = re.compile(r"(?<![\w.])flatten\b")
TYPE_PATH = re.compile(
    r"(?P<path>(?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*))"
)
RUST_TYPE_PATH = re.compile(
    r"^(?:::)?[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*$"
)
USE_IMPORT = re.compile(
    r"^[^\S\n]*(?:pub(?:\([^)]*\))?\s+)?use\s+"
    r"(?P<path>(?:::)?[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)"
    r"(?:\s+as\s+(?P<alias>[A-Za-z_][A-Za-z0-9_]*))?\s*;",
    re.MULTILINE,
)
TYPE_DECL = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?P<kind>struct|enum)\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
)
DOC_COMMENT = re.compile(r"^\s*///")
DOC_ATTRIBUTE = re.compile(r"^#\s*\[\s*doc\b")
MIRROR_FIELD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*:")
MIRROR_BODY_FIELD = re.compile(
    r"(?:\s|#\s*\[[^]]*\])*?(?:pub(?:\([^)]*\))?\s+)?"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*:",
    re.DOTALL,
)
MIRROR_BODY_VARIANT = re.compile(
    r"(?:\s|#\s*\[[^]]*\])*?(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
    r"\s*(?:\{|\(|=|,|})",
    re.DOTALL,
)


def type_declarations(lines: list[str], masked: list[str]):
    """Yield every `struct`/`enum` declaration with the attributes above it.

    Attributes are read from the source and delimited on the masked copy, so a
    bracket inside a string literal cannot end one early and a doc comment
    above a multi-line attribute stays attached to the declaration below it.
    """
    attrs: list[str] = []
    index = 0
    while index < len(lines):
        if masked[index].lstrip().startswith("#["):
            _, stop = collect_attribute(masked, index)
            attrs.append("".join(lines[index:stop]))
            index = stop
            continue
        if is_trivia_line(lines[index].strip()):
            index += 1
            continue
        match = TYPE_DECL.match(masked[index])
        if match is not None:
            yield match.group("kind"), match.group("name"), index, tuple(attrs)
        attrs = []
        index += 1


def mirror_targets(attrs: tuple[str, ...]) -> set[str]:
    """Named mirror types one declaration's serde attributes convert from."""
    targets: set[str] = set()
    for attr in attrs:
        if SERDE_ATTRIBUTE.search(attr) is None:
            continue
        for match in SERDE_MIRROR_TARGET.finditer(attr):
            target = re.sub(r"\s+", "", match.group("target"))
            if RUST_TYPE_PATH.match(target):
                targets.add(target)
    return targets


def member_has_doc(
    lines: list[str], masked: list[str], index: int, start_line: int, floor: int
) -> bool:
    """Whether a doc comment or `#[doc]` stands above the member at ``index``.

    The walk steps over blank lines, ordinary comments and whole attribute
    blocks, so a doc comment above a multi-line `#[serde(…)]` still documents
    the member below it.
    """
    if start_line == index:
        return False
    line = index - 1
    while line > floor:
        text = lines[line].strip()
        if not text or (text.startswith("//") and not DOC_COMMENT.match(lines[line])):
            line -= 1
            continue
        if DOC_COMMENT.match(lines[line]):
            return True
        if not masked[line].rstrip().endswith("]"):
            return False
        depth = 0
        start = line
        while start > floor:
            depth += masked[start].count("]") - masked[start].count("[")
            if depth <= 0:
                break
            start -= 1
        if DOC_ATTRIBUTE.match(lines[start].strip()):
            return True
        line = start - 1
    return False


def mirror_members(lines: list[str], masked: list[str], kind: str, index: int):
    """Yield each documented-or-not member of one mirror declaration.

    A field of the declaration, a variant of an enum, and a field of a
    struct-shaped variant each carry their own schema property, so each is a
    member. A tuple or unit declaration states no member.
    """
    body = index
    while body < len(masked) and "{" not in masked[body]:
        if ";" in masked[body]:
            return
        body += 1
    if body >= len(masked):
        return
    text = "\n".join(masked)
    body_offset = sum(len(line) + 1 for line in masked[:body]) + masked[body].index("{")
    depth = 1
    parentheses = 0
    brackets = 0
    candidates = [(body_offset + 1, depth)]
    position = body_offset + 1
    while position < len(text) and depth:
        character = text[position]
        if character == "{":
            depth += 1
            candidates.append((position + 1, depth))
        elif character == "}":
            depth -= 1
        elif character == "(":
            parentheses += 1
        elif character == ")":
            parentheses -= 1
        elif character == "[":
            brackets += 1
        elif character == "]":
            brackets -= 1
        elif character == "," and parentheses == 0 and brackets == 0:
            candidates.append((position + 1, depth))
        position += 1

    for start, inner in candidates:
        if inner == 1:
            pattern = MIRROR_BODY_VARIANT if kind == "enum" else MIRROR_BODY_FIELD
        elif inner == 2 and kind == "enum":
            pattern = MIRROR_BODY_FIELD
        else:
            continue
        match = pattern.match(text, start)
        if match is None:
            continue
        name_start = match.start("name")
        line = text.count("\n", 0, name_start)
        yield match.group("name"), line, text.count("\n", 0, start)


def member_attribute_blocks(lines: list[str], masked: list[str], index: int, floor: int):
    """Yield the source text of each attribute block above the member at ``index``.

    The walk steps over blank lines, ordinary comments and doc comments, so an
    attribute under a doc comment is still read as the member's own.
    """
    line = index - 1
    while line > floor:
        text = lines[line].strip()
        if not text or text.startswith("//"):
            line -= 1
            continue
        if not masked[line].rstrip().endswith("]"):
            return
        depth = 0
        start = line
        while start > floor:
            depth += masked[start].count("]") - masked[start].count("[")
            if depth <= 0:
                break
            start -= 1
        yield "".join(lines[start:line + 1])
        line = start - 1


def member_type_text(masked: list[str], index: int, end: int) -> str:
    """The type a field declares, from its colon to the comma that ends it."""
    match = MIRROR_FIELD.match(masked[index])
    if match is None:
        return ""
    pieces: list[str] = []
    depth = 0
    for line in range(index, end):
        text = masked[line][match.end():] if line == index else masked[line]
        for character in text:
            if character in "<([{":
                depth += 1
            elif character in ">)]}":
                depth -= 1
                if depth < 0:
                    return "".join(pieces)
            elif character == "," and depth == 0:
                return "".join(pieces)
            pieces.append(character)
        pieces.append(" ")
    return "".join(pieces)


def flattened_member_types(lines: list[str], masked: list[str], kind: str, index: int) -> set[str]:
    """Type names the declaration at ``index`` publishes through `serde(flatten)`.

    A flattened member states no property of its own, so the names here are the
    types that hold the properties the wire carries in its place.
    """
    body = index
    while body < len(masked) and "{" not in masked[body]:
        if ";" in masked[body]:
            return set()
        body += 1
    if body >= len(masked):
        return set()
    end = find_matching_brace_end(masked, body)
    names: set[str] = set()
    for _, line, _ in mirror_members(lines, masked, kind, index):
        flattened = any(
            SERDE_ATTRIBUTE.search(block) is not None and SERDE_FLATTEN.search(block) is not None
            for block in member_attribute_blocks(lines, masked, line, index)
        )
        if not flattened:
            continue
        for match in TYPE_PATH.finditer(member_type_text(masked, line, end)):
            names.add(re.sub(r"\s+", "", match.group("path")))
    return names


def scan_wire_mirror_docs(sources: dict[Path, str]) -> list[Finding]:
    """Report a serde wire mirror member that carries no doc comment.

    The mirror is the shape the wire states, and the published JSON schema
    reads each member's doc as that property's `description`. A member with no
    doc leaves the schema silent about the value the wire carries.

    A type the mirror flattens holds the properties that member publishes, so
    it is a mirror too and its members carry the same rule.

    The scan reads the whole tree: a declaration names its mirror by type name,
    and the mirror itself is often declared in another file, so no per-file
    scan can pair the two.
    """
    files = sorted(path for path in sources if is_production_rs(path))
    parsed: dict[Path, tuple[list[str], list[str]]] = {}
    targets: list[tuple[Path, tuple[str, ...], str]] = []
    in_file: dict[Path, dict[str, list[tuple[str, int]]]] = {}
    in_crate: dict[str, dict[str, list[tuple[Path, tuple[str, ...], str, int]]]] = {}
    by_module: dict[str, dict[tuple[str, ...], dict[str, list[tuple[Path, int]]]]] = {}
    crate_modules: dict[str, set[tuple[str, ...]]] = {}
    imports: dict[Path, list[tuple[str, str]]] = {}
    declaration_modules: dict[tuple[Path, int], tuple[str, ...]] = {}
    kinds: dict[tuple[Path, int], str] = {}

    def file_module(path: Path) -> tuple[str, ...]:
        """The conventional module path of a Rust source file."""
        parts = Path(relative_path(path)).parts
        source = parts[parts.index("src") + 1:]
        if not source or source[-1] in ("lib.rs", "main.rs"):
            return ()
        stem = Path(source[-1]).stem
        if stem == "mod":
            return tuple(source[:-1])
        return tuple(source[:-1]) + (stem,)

    def modules_by_line(masked: list[str], base: tuple[str, ...]) -> list[tuple[str, ...]]:
        """Map declarations inside inline modules to their Rust module path."""
        modules = [base] * len(masked)

        def visit(start: int, stop: int, module: tuple[str, ...]) -> None:
            index = start
            while index < stop:
                match = MOD_DECL_VIS.match(masked[index])
                if match is None or match.group("marker") != "{":
                    index += 1
                    continue
                end = find_matching_brace_end(masked, index)
                child = module + (match.group("name"),)
                for line in range(index + 1, min(end + 1, len(modules))):
                    modules[line] = child
                visit(index + 1, end, child)
                index = end + 1

        visit(0, len(masked), base)
        return modules

    for path in files:
        lines = sources[path].splitlines()
        code, _ = production_source(sources[path])
        masked = code.splitlines()
        parsed[path] = (lines, masked)
        crate = relative_path(path).split("/")[1]
        line_modules = modules_by_line(masked, file_module(path))
        imported: list[tuple[str, str]] = []
        for match in USE_IMPORT.finditer(code):
            imported_path = match.group("path")
            imported.append((match.group("alias") or imported_path.rsplit("::", 1)[-1], imported_path))
        imports[path] = imported
        for kind, name, index, attrs in type_declarations(lines, masked):
            module = line_modules[index]
            crate_modules.setdefault(crate, set()).add(module)
            for depth in range(len(module)):
                crate_modules[crate].add(module[:depth])
            for target in mirror_targets(attrs):
                targets.append((path, module, target))
            in_file.setdefault(path, {}).setdefault(name, []).append((kind, index))
            in_crate.setdefault(crate, {}).setdefault(name, []).append(
                (path, module, kind, index)
            )
            by_module.setdefault(crate, {}).setdefault(module, {}).setdefault(name, []).append(
                (path, index)
            )
            declaration_modules[(path, index)] = module
            kinds[(path, index)] = kind

    def resolve_qualified(
        crate: str, owner_module: tuple[str, ...], target: str,
    ) -> list[tuple[Path, int]]:
        """Resolve a qualified target only when it names this crate's module tree."""
        segments = tuple(part for part in target.removeprefix("::").split("::") if part)
        if len(segments) < 2:
            return []
        if segments[0] == "crate":
            module = segments[1:-1]
        elif segments[0] == "self":
            module = owner_module + segments[1:-1]
        elif segments[0] == "super":
            parent = owner_module
            offset = 0
            while offset < len(segments) - 1 and segments[offset] == "super":
                if not parent:
                    return []
                parent = parent[:-1]
                offset += 1
            module = parent + segments[offset:-1]
        else:
            relative_module = owner_module + segments[:-1]
            root_module = segments[:-1]
            known = crate_modules.get(crate, set())
            if relative_module in known:
                module = relative_module
            elif root_module in known:
                module = root_module
            else:
                return []
        return list(by_module.get(crate, {}).get(module, {}).get(segments[-1], []))

    # An unqualified target follows file-local declarations, then imports, then
    # a unique crate-wide declaration. A qualified target must select the exact
    # module it names; an unknown leading segment names another crate.
    def resolve(
        path: Path, owner_module: tuple[str, ...], target: str,
    ) -> list[tuple[Path, int]]:
        crate = relative_path(path).split("/")[1]
        if "::" in target:
            return resolve_qualified(crate, owner_module, target)
        local = in_file.get(path, {}).get(target)
        if local is not None:
            return [(path, index) for _, index in local]
        imported_paths = [imported for alias, imported in imports.get(path, []) if alias == target]
        imported_matches = {
            found
            for imported in imported_paths
            for found in resolve_qualified(crate, owner_module, imported)
        }
        if imported_paths:
            return list(imported_matches) if len(imported_matches) == 1 else []
        crate_wide = in_crate.get(crate, {}).get(target, [])
        if len(crate_wide) == 1:
            other, _, _, index = crate_wide[0]
            return [(other, index)]
        return []

    mirrors: set[tuple[Path, int]] = set()
    for path, module, target in targets:
        mirrors.update(resolve(path, module, target))
    # A flattened member publishes the flattened type's properties, so that type
    # is a mirror as well. The walk repeats until it finds nothing new, which
    # carries the rule through a mirror that flattens a type that flattens
    # another.
    pending = list(mirrors)
    while pending:
        path, index = pending.pop()
        lines, masked = parsed[path]
        for target in flattened_member_types(lines, masked, kinds[(path, index)], index):
            for found in resolve(path, declaration_modules[(path, index)], target):
                if found not in mirrors:
                    mirrors.add(found)
                    pending.append(found)
    findings: list[Finding] = []
    for path in files:
        relative = relative_path(path)
        if not relative.startswith(WIRE_MIRROR_DOC_ROOTS):
            continue
        lines, masked = parsed[path]
        for name, entries in in_file.get(path, {}).items():
            for kind, index in entries:
                if (path, index) not in mirrors:
                    continue
                for member, line, start_line in mirror_members(lines, masked, kind, index):
                    if member_has_doc(lines, masked, line, start_line, index):
                        continue
                    findings.append(Finding(
                        "undocumented_wire_mirror", relative, line + 1,
                        f"Member `{member}` of the serde wire mirror `{name}` carries "
                        "no doc comment; the published schema reads that doc as the "
                        "property's description.",
                    ))
    return findings


def scan_module_visibility(sources: dict[Path, str]) -> list[Finding]:
    """Report a module-level item claiming more reach than its module can grant.

    A module whose declaration chain caps it below the crate root cannot be named
    from outside that cap, so no code outside the cap can write a path to the
    items it declares. A `pub(crate)` marker on such an item therefore spells
    reach the module already denies, and the honest marker is the narrower one
    its readers need.

    The rule covers a module-level `fn`, `const` or `static` only. Those are
    reachable by path alone, so the module's cap is the whole story. An
    associated item, a struct field and an enum variant are reachable on a value
    obtained outside the module, with no path written, and a type is subject to
    the private-interface rule when a wider signature names it; for all of those
    the compiler can require the wider marker, and the check would report a
    marker the build needs. Those forms stay outside the rule rather than in an
    exemption list.

    An item re-exported upward keeps the reach the re-export grants, so a module
    named by any non-private `use` in its crate is outside the rule as well.
    """
    findings: list[Finding] = []
    for crate_dir in sorted(ROOT.glob("crates/*")):
        roots = [crate_dir / "src" / name for name in ("lib.rs", "main.rs")]
        roots = [root for root in roots if root.is_file()]
        if not roots:
            continue
        reexported: set[str] = set()
        for path in sorted(crate_dir.glob("src/**/*.rs")):
            if path not in sources:
                sources[path] = path.read_text(encoding="utf-8", errors="replace")
            if not REEXPORT_USE.search(sources[path]):
                continue  # masking is the expensive step; skip a file with no candidate.
            for match in REEXPORT_USE.finditer(mask_rust_non_code(sources[path])):
                reexported.update(IDENTIFIER.findall(match.group("path")))
        for root in roots:
            for path, module, scope in module_scopes(root):
                if not scope or not is_production_rs(path) or module[-1] in reexported:
                    continue
                if path not in sources:
                    sources[path] = path.read_text(encoding="utf-8", errors="replace")
                code, _ = production_source(sources[path])
                for number, line in enumerate(code.splitlines(), start=1):
                    match = PATH_ONLY_ITEM.match(line)
                    if match is None:
                        continue
                    granted = spelled_scope(module, match.group("scope"))
                    if granted is None:
                        continue
                    if len(granted) >= len(scope) or scope[:len(granted)] != granted:
                        continue
                    findings.append(Finding(
                        "overwide_module_visibility", relative_path(path), number,
                        f"Module `{'::'.join(module)}` can be named only inside "
                        f"`crate::{'::'.join(scope)}`; this marker spells wider reach "
                        "than the module grants.",
                    ))
    return findings


# An absolute filesystem path of an authoring machine is corpus provenance and
# must not be checked in. A URL path segment is not one: it follows a host name,
# so the character before the segment is alphanumeric.
AUTHORING_PATH = re.compile(r"(?<![0-9A-Za-z])/(?:home|Users|root)/[0-9A-Za-z._-]+/")
# Sources, documents and the recorded-evidence triple a check leaves behind: the
# command line, its saved output and its exit status. A recorded command line and
# a saved log carry an authoring path as readily as a source file does.
AUTHORING_PATH_TEXT_SUFFIXES = frozenset(
    {".rs", ".json", ".md", ".toml", ".txt", ".py", ".sh", ".command", ".log", ".exit"}
)
AUTHORING_PATH_ROOTS = ("crates", "docs")


def scan_authoring_paths() -> list[Finding]:
    """Report every checked-in absolute path of an authoring machine."""
    findings: list[Finding] = []
    for root in AUTHORING_PATH_ROOTS:
        for path in sorted((ROOT / root).rglob("*")):
            if not path.is_file() or path.suffix not in AUTHORING_PATH_TEXT_SUFFIXES:
                continue
            text = path.read_text(encoding="utf-8", errors="replace")
            for number, line in enumerate(text.splitlines(), start=1):
                for match in AUTHORING_PATH.finditer(line):
                    findings.append(Finding(
                        "authoring_path",
                        str(path.relative_to(ROOT)),
                        number,
                        f"Checked-in absolute path {match.group(0)!r} names an authoring "
                        "machine; elide or digest the value instead.",
                    ))
    return findings


def is_main_guard(node: ast.AST) -> bool:
    """Whether one statement is the ``if __name__ == "__main__":`` block."""
    if not isinstance(node, ast.If) or not isinstance(node.test, ast.Compare):
        return False
    left = node.test.left
    if not isinstance(left, ast.Name) or left.id != "__name__":
        return False
    return any(
        isinstance(value, ast.Constant) and value.value == "__main__"
        for value in node.test.comparators
    )


def declares_tests(node: ast.ClassDef) -> bool:
    """Whether one class is a test case: a `TestCase` base or a `test_` method."""
    for base in node.bases:
        name = base.attr if isinstance(base, ast.Attribute) else getattr(base, "id", "")
        if name.endswith("TestCase"):
            return True
    return any(
        isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef))
        and child.name.startswith("test_")
        for child in node.body
    )


def script_test_definitions(tree: ast.Module):
    """Each test class and free test function a script declares, with its line."""
    methods = {
        id(child)
        for node in ast.walk(tree) if isinstance(node, ast.ClassDef)
        for child in node.body
    }
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef):
            if declares_tests(node):
                yield node.lineno, f"class {node.name}"
        elif (
            isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
            and node.name.startswith("test_")
            and id(node) not in methods
        ):
            yield node.lineno, f"function {node.name}"


def scan_script_tests() -> list[Finding]:
    """Every test a script declares where unittest discovery cannot reach it.

    Discovery imports the module and never runs its ``__main__`` block, so a
    test declared after that block, or inside it, is collected by nothing.
    """
    findings: list[Finding] = []
    for path in sorted(ROOT.glob("scripts/test_*.py")):
        relative = str(path.relative_to(ROOT))
        text = path.read_text(encoding="utf-8", errors="replace")
        try:
            tree = ast.parse(text)
        except SyntaxError as error:
            findings.append(Finding(
                "script_test_collection", relative, error.lineno or 1,
                f"{path.name} does not parse, so the tests it collects cannot "
                f"be read: {error.msg}.",
            ))
            continue
        guards = [node.lineno for node in ast.walk(tree) if is_main_guard(node)]
        if not guards:
            continue
        guard = min(guards)
        for line, declaration in script_test_definitions(tree):
            if line > guard:
                findings.append(Finding(
                    "script_test_collection", relative, line,
                    f"{path.name} declares test {declaration} at or after its "
                    f'if __name__ == "__main__" block on line {guard}; unittest '
                    "discovery imports the module without running that block, "
                    "so the test is collected by nothing.",
                ))
    return findings


# Results carrying EvaluationFailure retain the ResourceLimit arm until a
# propagating conversion. Discover names from signatures, rather than keeping
# a second catalog of the evaluator API.
RUST_TOKEN = re.compile(r"[A-Za-z_][A-Za-z0-9_]*|::|->|=>|\.\.|[^\s]")
EVALUATION_DROPS = {
    "ok", "is_ok", "is_ok_and", "is_err", "is_err_and", "err",
    "unwrap_or", "unwrap_or_else", "unwrap_or_default", "map_or", "map_or_else",
}


def evaluation_tokens(code: str):
    tokens = list(RUST_TOKEN.finditer(code))
    pairs: dict[int, int] = {}
    parents: dict[int, int] = {}
    stack: list[int] = []
    closing = {")": "(", "]": "[", "}": "{"}
    for index, token in enumerate(tokens):
        if stack:
            parents[index] = stack[-1]
        if token[0] in "([{":
            stack.append(index)
        elif token[0] in closing and stack and tokens[stack[-1]][0] == closing[token[0]]:
            opening = stack.pop()
            pairs[opening] = index
            pairs[index] = opening
    return tokens, pairs, parents


def evaluation_call_open(words: list[str], index: int) -> int | None:
    opening = index + 1
    if words[opening:opening + 2] == ["::", "<"]:
        opening += 2
        depth = 1
        while opening < len(words) and depth:
            depth += (words[opening] == "<") - (words[opening] == ">")
            opening += 1
    return opening if words[opening:opening + 1] == ["("] else None


def evaluation_expression_start(words, pairs, index):
    start = index
    while start >= 2 and words[start - 1] in {"::", "."}:
        start -= 2
        if words[start] in {")", "]"} and start in pairs:
            closing = words[start]
            start = pairs[start]
            if closing == ")" and start and re.fullmatch(r"[A-Za-z_]\w*", words[start - 1]):
                start -= 1
            elif closing == "]" and start:
                start -= 1
    return start


def evaluation_signatures(tokens, pairs, parents):
    """Yield declarations with return types and their enclosing impl type."""
    for index, token in enumerate(tokens):
        if token[0] != "fn" or index + 2 >= len(tokens):
            continue
        name = tokens[index + 1][0]
        opening = index + 2
        if tokens[opening][0] == "<":
            depth = 1
            opening += 1
            while opening < len(tokens) and depth:
                depth += (tokens[opening][0] == "<") - (tokens[opening][0] == ">")
                opening += 1
        if opening not in pairs or tokens[opening][0] != "(":
            continue
        stop = pairs[opening] + 1
        end = stop
        while end < len(tokens) and tokens[end][0] not in {"{", ";"}:
            # Array types such as `[u8; 4]` carry a semicolon inside the signature.
            end = pairs[end] + 1 if tokens[end][0] in "([" and end in pairs else end + 1
        output = [t[0] for t in tokens[stop:end]]
        owner = None
        parent = parents.get(index)
        while parent is not None:
            head = parent - 1
            while head >= 0 and tokens[head][0] not in {";", "{", "}"}:
                head -= 1
            header = [t[0] for t in tokens[head + 1:parent]]
            if "impl" in header:
                # The self type follows `for` for trait impls, or `impl` and
                # its generic parameters for inherent impls.
                start = header.index("for") + 1 if "for" in header else header.index("impl") + 1
                if start < len(header) and header[start] == "<":
                    depth = 1
                    start += 1
                    while start < len(header) and depth:
                        depth += (header[start] == "<") - (header[start] == ">")
                        start += 1
                if start < len(header):
                    owner = header[start]
                break
            parent = parents.get(parent)
        yield index, name, output, owner


def evaluation_inline_module(tokens, parents, index) -> tuple[str, ...]:
    modules = []
    parent = parents.get(index)
    while parent is not None:
        if tokens[parent][0] == "{" and parent >= 2 and tokens[parent - 2][0] == "mod":
            modules.append(tokens[parent - 1][0])
        parent = parents.get(parent)
    return tuple(reversed(modules))


def evaluation_module(path: Path) -> tuple[str, ...]:
    parts = Path(relative_path(path)).parts
    source = parts[parts.index("src") + 1:]
    crate = parts[1].replace("-", "_")
    stem = Path(source[-1]).stem
    return (crate, *source[:-1], *((stem,) if stem not in {"lib", "main", "mod"} else ()))


def evaluation_imports(tokens, pairs):
    """Expand grouped imports, including renamed functions and modules."""
    imports = {}

    def visit(start, stop, prefix):
        path = list(prefix)
        index = start
        while index < stop:
            word = tokens[index][0]
            if word == "{":
                end = pairs.get(index, stop)
                visit(index + 1, end, tuple(path))
                index = end + 1
            elif word == ",":
                path = list(prefix)
                index += 1
            elif word == "as" and path and index + 1 < stop:
                imports[tokens[index + 1][0]] = tuple(path)
                index += 2
            elif word == "::":
                index += 1
            else:
                path.append(word)
                if index + 1 == stop or tokens[index + 1][0] in {",", "}"}:
                    imports[word] = tuple(path[:-1] if word == "self" else path)
                index += 1

    for index, token in enumerate(tokens):
        if token[0] == "use":
            stop = index + 1
            while stop < len(tokens) and tokens[stop][0] != ";":
                stop += 1
            visit(index + 1, stop, ())
    return imports


def scan_evaluation_refusals(sources: dict[Path, str]) -> list[Finding]:
    parsed = {}
    production = {}
    functions = {}
    methods = {}
    returns = {}
    retained = {}
    for path, source in sources.items():
        if not is_production_rs(path):
            continue
        code, _ = production_source(source)
        tokens, pairs, parents = evaluation_tokens(code)
        signatures = list(evaluation_signatures(tokens, pairs, parents))
        production[path] = code
        # Keep token trees only for evaluator owners. Consumer files are
        # scanned one at a time, so workspace size does not retain every token.
        if "EvaluationFailure" in code:
            parsed[path] = tokens, pairs, parents, signatures
        words = [t[0] for t in tokens]
        for index, name, output, owner in signatures:
            opening = evaluation_call_open(words, index + 1)
            if opening in pairs:
                parameters = words[opening + 1:pairs[opening]]
                body = pairs[opening] + 1 + len(output)
                if body in pairs and words[body] == "{":
                    contents = words[body + 1:pairs[body]]
                    for parameter in range(len(parameters) - 2):
                        if parameters[parameter + 1:parameter + 3] == [":", "ResourceLimit"]:
                            limit = parameters[parameter]
                            if any(contents[offset:offset + 6] == ["resource", ":", "Some", "(", limit, ")"]
                                   for offset in range(len(contents) - 5)):
                                retained.setdefault(path, set()).add((owner, name))
            result_type = next((word for word in output if re.fullmatch(r"[A-Za-z_]\w*", word)
                                and word not in {"Result", "Option", "Self"}), None)
            if result_type:
                returns.setdefault(name, set()).add(result_type)
            holds = "EvaluationFailure" in output
            # Count the Result wrappers around the failure, including the
            # outer admitted Result whose first `?` leaves an inner Result.
            levels = output.count("Result") if holds else 0
            if owner:
                methods.setdefault((owner, name), set()).add(levels)
            else:
                functions[evaluation_module(path) + evaluation_inline_module(tokens, parents, index) + (name,)] = levels
    names = {key[-1] for key, levels in functions.items() if levels}
    method_names = {name for (_, name), levels in methods.items() if any(levels)}
    findings = []
    call_names = re.compile(r"\b(?:" + "|".join(sorted(names | method_names)) + r")\b") if names or method_names else None
    for path, code in production.items():
        if call_names is None or not call_names.search(code):
            continue
        if path in parsed:
            tokens, pairs, parents, signatures = parsed[path]
        else:
            tokens, pairs, parents = evaluation_tokens(code)
            signatures = list(evaluation_signatures(tokens, pairs, parents))
        words = [t[0] for t in tokens]
        module = evaluation_module(path)
        imports = evaluation_imports(tokens, pairs)
        local = {name: output.count("Result") if "EvaluationFailure" in output else 0
                 for _, name, output, owner in signatures if owner is None}
        declarations = {index + 1 for index, _, _, _ in signatures}
        refused: dict[int, int] = {}
        bindings: dict[str, list[tuple[int, int, int]]] = {}
        candidate_names = names | method_names | imports.keys()

        def qualify(parts):
            parts = list(parts)
            if parts and parts[0] in imports:
                parts = list(imports[parts[0]]) + parts[1:]
            if parts and parts[0] == "crate":
                return (module[0], *parts[1:])
            if parts and parts[0] == "self":
                return (*module, *parts[1:])
            if parts and parts[0] == "super":
                base = module
                while parts and parts[0] == "super":
                    base = base[:-1]
                    parts.pop(0)
                return (*base, *parts)
            absolute = tuple(parts)
            return absolute if absolute in functions else (*module, *parts)

        def function_level(index):
            name = words[index]
            if index in declarations:
                return 0
            if index > 0 and words[index - 1] == ".":
                if name not in method_names:
                    return 0
                # Ambiguous standard method names need a stated receiver type.
                receiver = words[index - 2] if index > 1 else ""
                candidates = {owner for owner, method in methods if method == name
                              and any(methods[(owner, method)])}
                for owner in candidates:
                    enclosing = [item for item in signatures if item[0] < index]
                    if receiver == "self" and enclosing and enclosing[-1][3] == owner:
                        return max(methods[(owner, name)])
                    if receiver == ")" and index - 2 in pairs:
                        constructor = pairs[index - 2]
                        if words[constructor - 3:constructor - 1] == [owner, "::"]:
                            return max(methods[(owner, name)])
                    if re.search(r"\b" + re.escape(receiver) + r"\s*:\s*(?:&\s*(?:'\w+\s*)?(?:mut\s*)?)?(?:\w+::)*"
                                 + re.escape(owner) + r"\b", code):
                        return max(methods[(owner, name)])
                    if re.search(r"\blet\s+(?:mut\s+)?" + re.escape(receiver)
                                 + r"\s*=\s*(?:[\w]+::)*" + re.escape(owner) + r"::", code):
                        return max(methods[(owner, name)])
                    for constructor, result_types in returns.items():
                        if result_types == {owner} and re.search(
                            r"\blet\s+(?:mut\s+)?" + re.escape(receiver)
                            + r"\s*=\s*(?:[\w]+::)*" + re.escape(constructor) + r"\s*\(", code
                        ):
                            return max(methods[(owner, name)])
                if name not in {"map", "first", "second", "point", "normal", "evaluate", "tangent", "partials"}:
                    return max(level for (owner, method), levels in methods.items()
                               if method == name for level in levels)
                return 0
            if name not in names and name not in method_names and name not in imports:
                return 0
            start = index
            while start >= 2 and words[start - 1] == "::":
                start -= 2
            parts = words[start:index + 1:2]
            if start == index and name in local:
                return local[name]
            if len(parts) >= 2 and (parts[-2], name) in methods:
                return max(methods[(parts[-2], name)])
            return functions.get(qualify(parts), 0)

        def report(index, form):
            findings.append(Finding(
                "evaluation_refusal", relative_path(path),
                code.count("\n", 0, tokens[index].start()) + 1,
                f"{form} drops an evaluator resource refusal; use finite_or_refusal, non_finite, or an explicit propagating ResourceLimit arm.",
            ))

        def carries_refusal(body, limit_bindings):
            for call, name in enumerate(body):
                if body[call + 1:call + 2] != ["("]:
                    continue
                closing = call + 2
                depth = 1
                while closing < len(body) and depth:
                    depth += (body[closing] == "(") - (body[closing] == ")")
                    closing += 1
                argument = body[call + 2:closing - 1]
                if name == "Err":
                    for limit in limit_bindings:
                        if argument in ([limit], [limit, ".", "into", "(", ")"]):
                            return True
                        if len(argument) >= 4 and argument[-4:] == ["ResourceLimit", "(", limit, ")"]:
                            return True
                for owner, constructor in retained.get(path, set()):
                    if name == constructor and (owner is None or body[call - 2:call] == [owner, "::"]):
                        if any(argument in ([limit], ["*", limit]) for limit in limit_bindings):
                            return True
            return False

        def consume(start, end, level):
            # Parentheses and value-preserving adapters retain the same error.
            while level and end < len(words):
                if words[end] == "?":
                    level -= 1
                    end += 1
                elif words[end] == ")" and pairs.get(end) == start - 1:
                    start -= 1
                    end += 1
                elif words[end] == "." and end + 1 < len(words) and evaluation_call_open(words, end + 1) is not None:
                    name = words[end + 1]
                    opening = evaluation_call_open(words, end + 1)
                    close = pairs.get(opening)
                    if close is None:
                        break
                    if name in EVALUATION_DROPS:
                        report(end + 1, "." + name)
                        return close + 1, 0
                    if name == "map_err":
                        body = words[opening + 1:close]
                        while body and body[0] in {"move", "async"}:
                            body = body[1:]
                        if body and body[0] == "|" and "|" in body[1:]:
                            delimiter = body.index("|", 1)
                            pattern = body[1:delimiter]
                            if ":" in pattern:
                                pattern = pattern[:pattern.index(":")]
                            arguments = {word for word in pattern
                                         if re.fullmatch(r"[A-Za-z_]\w*", word) and word not in {"mut", "ref", "_"}}
                            if not arguments.intersection(body[delimiter + 1:]):
                                report(end + 1, ".map_err with an ignored failure")
                                return close + 1, 0
                    if name not in {"map", "map_err", "and_then", "as_ref", "as_mut", "copied", "cloned"}:
                        break
                    end = close + 1
                else:
                    break
            return end, level

        for index, word in enumerate(words):
            # A new lexical binding shadows the old result even when its
            # initializer has no evaluator type.
            if index and (words[index - 1] == "let" or index > 1 and words[index - 2:index] == ["let", "mut"]):
                parent = parents.get(index)
                while parent is not None and words[parent] != "{":
                    parent = parents.get(parent)
                bindings.setdefault(word, []).append((index, 0, pairs.get(parent, len(words))))
            level = function_level(index) if word in candidate_names else 0
            opening = evaluation_call_open(words, index)
            if level and opening in pairs:
                refused[index] = level
            elif level:
                # A named evaluator callback is an implicit call for every
                # iterator item. Result's IntoIterator discards its Err arm.
                parent = parents.get(index)
                if parent is not None and words[parent] == "(" and parent:
                    adapter = words[parent - 1]
                    close = pairs.get(parent, parent)
                    if adapter in {"filter_map", "find_map", "flat_map"}:
                        report(index, adapter + " evaluator callback")
                    elif adapter == "map" and words[close + 1:close + 4] == [".", "flatten", "("]:
                        report(index, "map(evaluator).flatten() iterator")
            # Bound results retain the evaluator type; do not confuse a
            # later lexical scope's binding.
            if word in bindings:
                active = [binding for binding in bindings[word] if index < binding[2]]
                bindings[word] = active
                if active:
                    origin, bound_level, _ = active[-1]
                    if bound_level and index > origin and (index == 0 or words[index - 1] not in {".", "::"}):
                        refused[index] = bound_level
            if index not in refused:
                continue
            start = evaluation_expression_start(words, pairs, index)
            end = pairs[opening] + 1 if opening in pairs else index + 1
            end, level = consume(start, end, refused[index])
            if not level:
                continue
            # Only an unconverted RHS can bind an evaluation result. A nested
            # finite_or_refusal call has a different return type.
            head = start - 1
            while head >= 0 and words[head] not in {";", "{", "}"}:
                head -= 1
            prefix = words[head + 1:start]
            if len(prefix) >= 3 and prefix[0] == "let" and prefix[-1] == "=" and "(" not in prefix:
                name = prefix[2] if prefix[1] == "mut" else prefix[1]
                parent = parents.get(start)
                while parent is not None and words[parent] != "{":
                    parent = parents.get(parent)
                bindings.setdefault(name, []).append((index, level, pairs.get(parent, len(words))))
            if end < len(words) and words[end] in {"{", "else"}:
                # if/while let Ok and let Ok ... else skip every error kind.
                if "let" in prefix and "Ok" in prefix and "=" in prefix:
                    report(index, "Ok pattern")
                if "match" in prefix and words[end] == "{" and end in pairs:
                    explicit = False
                    arm = end + 1
                    while arm < pairs[end]:
                        arrow = arm
                        while arrow < pairs[end] and words[arrow] != "=>":
                            arrow = pairs[arrow] + 1 if arrow in pairs and words[arrow] in {"(", "[", "{"} else arrow + 1
                        if arrow == pairs[end]:
                            break
                        body_end = arrow + 1
                        if words[body_end] == "{":
                            body_end = pairs.get(body_end, body_end) + 1
                        else:
                            while body_end < pairs[end] and words[body_end] != ",":
                                body_end = pairs[body_end] + 1 if body_end in pairs and words[body_end] in {"(", "[", "{"} else body_end + 1
                        pattern = words[arm:arrow]
                        body = words[arrow + 1:body_end]
                        if level > 1 and len(pattern) == 4 and pattern[:2] == ["Ok", "("] and pattern[-1] == ")":
                            bindings.setdefault(pattern[2], []).append((arrow, level - 1, body_end))
                        if "ResourceLimit" in pattern and "if" not in pattern:
                            # An explicit refusal arm must return its limit;
                            # naming the variant alone does not propagate it.
                            limit_bindings = {word for word in pattern
                                              if re.fullmatch(r"[A-Za-z_]\w*", word)
                                              and word not in {"Err", "EvaluationFailure", "ResourceLimit", "_"}}
                            explicit = carries_refusal(body, limit_bindings)
                            if not explicit:
                                report(index, "non-propagating ResourceLimit arm")
                                break
                        elif not explicit and (pattern[:1] == ["_"] or any(pattern[offset:offset + 3] in
                                               (["Err", "(", "_"], ["Err", "(", ".."]) for offset in range(len(pattern) - 2))):
                            report(index, "wildcard error arm")
                            break
                        arm = body_end + (words[body_end:body_end + 1] == [","])
                elif "let" in prefix and "Err" in prefix and "ResourceLimit" in prefix and words[end] == "{" and end in pairs:
                    resource = prefix.index("ResourceLimit")
                    limit_bindings = set(prefix[resource + 2:]) - {"(", ")", "=", "&"}
                    body = words[end + 1:pairs[end]]
                    if "return" in body and carries_refusal(body, limit_bindings):
                        # The returning guard removes the resource variant
                        # from the result read after it, including borrowed guards.
                        parent = parents.get(start)
                        while parent is not None and words[parent] != "{":
                            parent = parents.get(parent)
                        bindings.setdefault(word, []).append((index, 0, pairs.get(parent, len(words))))
                elif "let" in prefix and "Err" in prefix and "ResourceLimit" not in prefix and ("_" in prefix or ".." in prefix):
                    report(index, "wildcard error pattern")
            parent = parents.get(start)
            while parent is not None:
                if words[parent] == "(" and parent > 1:
                    adapter = words[parent - 1]
                    close = pairs.get(parent, parent)
                    if adapter == "matches" or (adapter == "!" and parent > 2 and words[parent - 2] == "matches"):
                        pattern = words[end:close]
                        if "Ok" in pattern or ("Err" in pattern and "ResourceLimit" not in pattern and ("_" in pattern or ".." in pattern)):
                            report(index, "matches! pattern")
                    if adapter in {"filter_map", "find_map", "flat_map"}:
                        report(index, adapter + " iterator")
                    if adapter == "map" and words[close + 1:close + 4] == [".", "flatten", "("]:
                        report(index, "map(...).flatten() iterator")
                parent = parents.get(parent)
    return findings


SLICE_SORT_METHODS = {
    "sort", "sort_by", "sort_by_key", "sort_unstable", "sort_unstable_by",
    "sort_unstable_by_key", "sort_by_cached_key",
}
DECODE_SORT_EXEMPT_FILES = {
    "crates/cadmpeg-core/src/decode/context.rs",
    "crates/cadmpeg-core/src/decode/sort.rs",
}
# Decode crates sort input-derived values in helpers that receive no context;
# their encoders sort values of an already admitted document.
DECODE_SORT_CRATE = re.compile(r"cadmpeg-(?:codec-[a-z0-9]+|container|asm|parasolid|protein)")
ENCODE_SORT_PATH = re.compile(
    r"crates/[^/]+/src/(?:"
    r"(?:.+/)?writer(?:/.*|\.rs|_[a-z_]+\.rs)"
    r"|bin/.*"
    r"|history/(?:encode|write)/.*"
    r"|resolved_features/(?:sketch_write|write_generate|write_prepare)\.rs"
    r"|zip_write\.rs"
    r"|export\.rs"
    r")"
)
DECODE_CONTEXT_BINDING = re.compile(
    r"\b([A-Za-z_]\w*)\s*:\s*(?:&\s*(?:'[A-Za-z_]\w*\s*)?(?:mut\s+)?)?"
    r"(?:::\s*)?(?:[A-Za-z_]\w*\s*::\s*)*DecodeContext\b"
)

IR_DECODE_SORT_PATH = re.compile(
    r"crates/cadmpeg-ir/src/(?:native/.*|math/.*|validate/.*|eval/.*|"
    r"document\.rs|hash\.rs|eval\.rs|codec\.rs)"
)
DECODE_RECEIVER_BINDING = re.compile(
    r"\b([A-Za-z_]\w*)\s*:\s*(?:&\s*(?:'[A-Za-z_]\w*\s*)?(?:mut\s+)?)?"
    r"(?:::\s*)?(?:[A-Za-z_]\w*\s*::\s*)*([A-Za-z_]\w*)\b"
)


class FixedSortSyntax:
    """A closed source grammar for fixed array sorts, not Rust type inference.

    Unknown syntax fails closed. The compiler remains responsible for resolved
    identities, reachability and operations outside this grammar.
    """

    SCALARS = {"bool", "char", "u8", "u16", "u32", "u64", "u128", "usize",
               "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64"}
    IDENTIFIER = re.compile(r"[A-Za-z_]\w*\Z")
    NUMBER = re.compile(
        r"-?(?:0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|"
        r"[0-9][0-9_]*(?:\.[0-9_]*)?(?:[eE][+-]?[0-9_]+)?)"
        r"(?:_?(?:u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize|f32|f64))?\Z"
    )

    def __init__(self, code: str, records: dict[str, dict[str, str]], shadowed: set[str]):
        self.code = code
        self.records = records
        self.shadowed = shadowed

    @staticmethod
    def split(text: str, separator: str) -> list[str]:
        """Split only at top-level delimiters; nested types/expressions stay whole."""
        tokens, pairs, _ = evaluation_tokens(text)
        pieces = []
        start = index = 0
        while index < len(tokens):
            word = tokens[index][0]
            if word in "([{":
                if index not in pairs:
                    return []
                index = pairs[index] + 1
                continue
            if word == separator:
                pieces.append(text[start:tokens[index].start()].strip())
                start = tokens[index].end()
            index += 1
        pieces.append(text[start:].strip())
        return pieces

    def type_shape(self, text: str):
        text = text.strip()
        borrowed = re.match(r"^&\s*(?:'[A-Za-z_]\w*\s*)?(?:mut\b\s*)?", text)
        if borrowed:
            child = self.type_shape(text[borrowed.end():])
            return ("ref", child) if child else None
        text = re.sub(r"\s+", "", text)
        if text in self.SCALARS and text not in self.shadowed:
            return (text,)
        for prefix in ("::core::primitive::", "core::primitive::", "::std::primitive::", "std::primitive::"):
            if text.startswith(prefix) and text[len(prefix):] in self.SCALARS:
                # Root/module identity is verified by the compiler, not spelling.
                if prefix.strip(":").split("::")[0] not in self.shadowed:
                    return (text[len(prefix):],)
        if text.startswith("[") and text.endswith("]"):
            parts = self.split(text[1:-1], ";")
            if len(parts) == 2 and parts[1] and (element := self.type_shape(parts[0])):
                return ("array", element)
        if text.startswith("(") and text.endswith(")"):
            parts = self.split(text[1:-1], ",")
            children = [self.type_shape(part) for part in parts if part]
            if children and all(children):
                return ("tuple", *children)
        if text in self.records:
            return ("record", text)
        return None

    @staticmethod
    def peel_refs(shape):
        while shape and shape[0] == "ref":
            shape = shape[1]
        return shape

    @staticmethod
    def standard_order(shape) -> bool:
        shape = FixedSortSyntax.peel_refs(shape)
        if shape is None or shape[0] == "record":
            return False
        if shape[0] in {"array", "tuple"}:
            return all(FixedSortSyntax.standard_order(child) for child in shape[1:])
        return shape[0] in FixedSortSyntax.SCALARS

    def expression_shape(self, text: str, bindings: dict):
        text = text.strip()
        if text in bindings:
            return bindings[text]
        if self.NUMBER.fullmatch(text):
            suffix = re.search(r"(u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize|f32|f64)$", text)
            decimal = not text.lstrip("-").startswith(("0x", "0X", "0b", "0B", "0o", "0O"))
            floating = decimal and any(c in text for c in ".eE")
            return (suffix[0] if suffix else "f64" if floating else "i32",)
        if text in {"true", "false"}:
            return ("bool",)
        tokens, pairs, _ = evaluation_tokens(text)
        if not tokens:
            return None
        words = [token[0] for token in tokens]
        # Parentheses, borrows and dereferences do not change the element cost.
        if words[0] in {"(", "{"} and pairs.get(0) == len(words) - 1:
            inside = text[tokens[0].end():tokens[-1].start()]
            parts = self.split(inside, ",") if words[0] == "(" else [inside]
            if len(parts) == 1:
                return self.expression_shape(parts[0], bindings)
            children = [self.expression_shape(part, bindings) for part in parts if part]
            return ("tuple", *children) if children and all(children) else None
        if words[0] in {"&", "*"}:
            start = 2 if words[:2] == ["&", "mut"] else 1
            inner = self.expression_shape(text[tokens[start].start():], bindings) if start < len(tokens) else None
            if words[0] == "&":
                return ("ref", inner) if inner else None
            return inner[1] if inner and inner[0] == "ref" else None
        if words[0] == "[" and pairs.get(0) == len(words) - 1:
            inside = text[tokens[0].end():tokens[-1].start()]
            repeat = self.split(inside, ";")
            parts = [repeat[0]] if len(repeat) == 2 and repeat[1] else self.split(inside, ",")
            children = [self.expression_shape(part, bindings) for part in parts if part]
            return ("array", children[0]) if children and all(child == children[0] for child in children) else None
        if len(words) >= 3 and words[-2] == ".":
            base = self.peel_refs(self.expression_shape(text[:tokens[-2].start()], bindings))
            if base and base[0] == "record":
                return self.type_shape(self.records[base[1]].get(words[-1], ""))
        # Numeric tuple fields are tokenized one digit at a time.
        field = re.fullmatch(r"(.+)\.\s*([0-9]+)", text, re.DOTALL)
        if field:
            base = self.peel_refs(self.expression_shape(field[1], bindings))
            index = int(field[2]) + 1
            return base[index] if base and base[0] == "tuple" and index < len(base) else None
        if words[-1] == "]" and (opening := pairs.get(len(words) - 1)) not in {None, 0}:
            base = self.peel_refs(self.expression_shape(text[:tokens[opening].start()], bindings))
            if base and base[0] == "array":
                index_text = text[tokens[opening].end():tokens[-1].start()].strip()
                bound = r"(?:[A-Za-z_]\w*|[0-9][0-9_]*)"
                if not re.fullmatch(rf"(?:{bound})?(?:\s*\.\.=?(?:\s*{bound})?)?", index_text):
                    return None
                index = words[opening + 1:-1]
                # Bounds can select fewer elements; they cannot enlarge an array.
                if ".." in index:
                    return base
                if index and all(word.isdigit() or self.IDENTIFIER.fullmatch(word) for word in index):
                    return base[1]
        return None

    def bindings_before(self, tokens, pairs, parents, function, call: int) -> dict:
        """Read visible parameters and locals; unknown/shadowing bindings erase facts."""
        index, body, _, owner = function
        opening = index + 2
        while opening < body and tokens[opening][0] != "(":
            opening += 1
        parameters = self.code[tokens[opening].end():tokens[pairs[opening]].start()]
        bindings = {"self": ("record", owner)} if owner in self.records else {}
        for parameter in self.split(parameters, ","):
            name, sep, type_text = parameter.partition(":")
            name = name.strip().removeprefix("mut ").strip()
            if sep and self.IDENTIFIER.fullmatch(name):
                bindings[name] = self.type_shape(type_text)
        ancestors = set()
        parent = parents.get(call)
        while parent is not None:
            ancestors.add(parent)
            parent = parents.get(parent)
        cursor = body + 1
        while cursor < call:
            if tokens[cursor][0] != "let" or parents.get(cursor) not in ancestors:
                cursor += 1
                continue
            start = cursor + 1
            stop = start
            while stop < call and tokens[stop][0] != ";":
                stop = pairs[stop] + 1 if tokens[stop][0] in "([{" and stop in pairs else stop + 1
            declaration = self.code[tokens[start].start():tokens[stop].start()] if stop < call else ""
            # A declaration's initializer must see the previous binding.
            pattern, sep, value = declaration.partition("=")
            name, colon, type_text = pattern.partition(":")
            name = name.strip().removeprefix("mut ").strip()
            if self.IDENTIFIER.fullmatch(name):
                bindings[name] = self.type_shape(type_text) if colon else self.expression_shape(value, bindings) if sep else None
            else:
                for name in re.findall(r"[A-Za-z_]\w*", pattern):
                    bindings[name] = None
            cursor = stop + 1
        return bindings

    def accepts(self, tokens, pairs, parents, function, call: int) -> bool:
        words = [token[0] for token in tokens]
        end = call - 2
        start = end
        # Recover only a closed receiver grammar. Calls and macros stay unknown.
        if words[start] in {"]", ")"} and start in pairs:
            start = pairs[start]
            if words[start] == "[" and start and (self.IDENTIFIER.fullmatch(words[start - 1]) or words[start - 1] == "]"):
                start -= 1
        while start >= 2 and words[start - 1] == ".":
            start -= 2
        receiver = self.code[tokens[start].start():tokens[end].end()]
        # A closure parameter can shadow a visible binding without a `let`.
        prefix = self.code[tokens[function[1]].end():tokens[call].start()]
        root = re.match(r"[A-Za-z_]\w*", receiver)
        if root:
            for closure in re.finditer(r"(?=\|([^|]*)\|)", prefix):
                for parameter in self.split(closure[1], ","):
                    name = parameter.partition(":")[0].strip().removeprefix("mut ").strip()
                    if name == root[0]:
                        return False
        bindings = self.bindings_before(tokens, pairs, parents, function, call)
        shape = self.peel_refs(self.expression_shape(receiver, bindings))
        if not shape or shape[0] != "array":
            return False
        method = words[call]
        if method in {"sort", "sort_unstable"}:
            return self.standard_order(shape[1])
        opening = evaluation_call_open(words, call)
        if opening not in pairs:
            return False
        callback = self.code[tokens[opening].end():tokens[pairs[opening]].start()].strip().rstrip(",").strip()
        if callback in {"f32::total_cmp", "f64::total_cmp"}:
            scalar = callback.split("::")[0]
            return method in {"sort_by", "sort_unstable_by"} and shape[1] == (scalar,) and scalar not in self.shadowed
        closure = re.fullmatch(r"\|\s*([A-Za-z_]\w*)(?:\s*,\s*([A-Za-z_]\w*))?\s*\|\s*(.+)", callback, re.DOTALL)
        if not closure:
            return False
        first, second, expression = closure.groups()
        callback_bindings = {first: ("ref", shape[1])}
        if second:
            callback_bindings[second] = ("ref", shape[1])
        if method in {"sort_by_key", "sort_unstable_by_key", "sort_by_cached_key"}:
            return second is None and self.standard_order(self.expression_shape(expression, callback_bindings))
        if second is None or first == second:
            return False
        comparison = re.fullmatch(r"(.+)\.\s*(cmp|total_cmp)\s*\((.+)\)", expression, re.DOTALL)
        if not comparison:
            return False
        left = self.peel_refs(self.expression_shape(comparison[1], callback_bindings))
        right = self.peel_refs(self.expression_shape(comparison[3], callback_bindings))
        return left == right and self.standard_order(left) and (comparison[2] != "total_cmp" or left[0] in {"f32", "f64"})


def scan_decode_sorts(sources: dict[Path, str]) -> list[Finding]:
    """Reject slice sorts in functions borrowing a decode context and in decode crate code.

    Function scopes exclude nested function items. Closures keep their enclosing
    context. Annotated receiver types identify owned and borrowed context fields.
    """
    parsed = {}
    context_fields: dict[tuple[str, str], set[str]] = {}
    record_candidates: dict[tuple[Path, str], list[dict[str, str]]] = {}
    record_shadowed: dict[Path, set[str]] = {}
    glob_imports: set[Path] = set()
    shadowed: dict[str, set[str]] = {}
    for path, source in sources.items():
        if not is_production_rs(path):
            continue
        code, _ = production_source(source)
        tokens, pairs, parents = evaluation_tokens(code)
        words = [token[0] for token in tokens]
        crate = Path(relative_path(path)).parts[1]
        parsed[path] = code, tokens, pairs, parents, words, crate
        # Primitive-looking user types must not supply an implicit custom Ord.
        names = set(re.findall(r"\b(?:struct|enum|type|trait|mod)\s+([A-Za-z_]\w*)", code))
        imports = evaluation_imports(tokens, pairs)
        names.update(imports)
        aliases = set(re.findall(r"\b(?:type|trait)\s+([A-Za-z_]\w*)", code)) | set(imports)
        if "*" in imports:
            glob_imports.add(path)
        for match in re.finditer(r"\bextern\s+crate\s+([A-Za-z_]\w*)(?:\s+as\s+([A-Za-z_]\w*))?", code):
            names.add(match[2] or match[1])
        for match in re.finditer(r"\b(?:fn|struct|enum|impl)\b[^;{}]*<([^>{}]*)>", code):
            parameters = set(re.findall(r"[A-Za-z_]\w*", match[1]))
            names.update(parameters)
            aliases.update(parameters)
        record_shadowed[path] = aliases
        shadowed.setdefault(crate, set()).update(names & (FixedSortSyntax.SCALARS | {"core", "std"}))
        for index, word in enumerate(words):
            if word != "struct" or index + 1 >= len(words):
                continue
            opening = index + 2
            while opening < len(words) and words[opening] not in {"{", ";", "("}:
                opening += 1
            if opening not in pairs or words[opening] != "{":
                continue
            fields = DECODE_CONTEXT_BINDING.findall(
                code[tokens[opening].end():tokens[pairs[opening]].start()])
            context_fields.setdefault((crate, words[index + 1]), set()).update(fields)
            field_text = code[tokens[opening].end():tokens[pairs[opening]].start()]
            record = {}
            for field in FixedSortSyntax.split(field_text, ","):
                match = re.fullmatch(r"\s*(?:pub(?:\([^)]*\))?\s+)?([A-Za-z_]\w*)\s*:\s*(.+)", field, re.DOTALL)
                if match:
                    record[match[1]] = match[2]
            record_candidates.setdefault((path, words[index + 1]), []).append(record)

    findings = []
    for path, (code, tokens, pairs, parents, words, crate) in parsed.items():
        if relative_path(path) in DECODE_SORT_EXEMPT_FILES:
            continue
        records = {name: candidates[0] for (record_path, name), candidates in record_candidates.items()
                   if record_path == path and len(candidates) == 1 and name not in record_shadowed[path]}
        uncertain = shadowed.get(crate, set())
        if path in glob_imports:
            uncertain = uncertain | FixedSortSyntax.SCALARS | {"core", "std"}
            records = {}
        fixed = FixedSortSyntax(code, records, uncertain)
        decode_scope = (bool(DECODE_SORT_CRATE.fullmatch(crate)) or bool(IR_DECODE_SORT_PATH.fullmatch(relative_path(path)))) and not ENCODE_SORT_PATH.fullmatch(relative_path(path))
        functions = []
        for index, _, _, owner in evaluation_signatures(tokens, pairs, parents):
            opening = index + 2
            if words[opening] == "<":
                depth = 1
                opening += 1
                while opening < len(words) and depth:
                    depth += (words[opening] == "<") - (words[opening] == ">")
                    opening += 1
            body = pairs[opening] + 1
            while body < len(words) and words[body] not in {"{", ";"}:
                body = pairs[body] + 1 if words[body] in "([" and body in pairs else body + 1
            if body not in pairs or words[body] != "{":
                continue
            functions.append((index, body, pairs[body], owner))
        # A nested function does not borrow its enclosing function's locals.
        contexts = {}
        for index, body, end, owner in functions:
            pieces = []
            cursor = tokens[index].start()
            for child, _, child_end, _ in functions:
                if body < child < end:
                    if tokens[child].start() >= cursor:
                        pieces.append(code[cursor:tokens[child].start()])
                        cursor = tokens[child_end].end()
            pieces.append(code[cursor:tokens[end].end()])
            scope = "".join(pieces)
            receivers = {
                name: context_fields.get((crate, type_name), set())
                for name, type_name in DECODE_RECEIVER_BINDING.findall(scope)
            }
            receivers["self"] = context_fields.get((crate, owner), set())
            bindings = set(DECODE_CONTEXT_BINDING.findall(scope))
            has_context = bool(bindings) or any(
                re.search(r"\b" + re.escape(receiver) + r"\s*\.\s*" + re.escape(field) + r"\b", scope)
                for receiver, fields in receivers.items() for field in fields
            )
            contexts[index] = bindings, receivers, has_context
        for index, word in enumerate(words):
            if word not in SLICE_SORT_METHODS or index == 0 or words[index - 1] != ".":
                continue
            if evaluation_call_open(words, index) is None:
                continue
            enclosing = [scope for scope in functions if scope[1] < index < scope[2]]
            if not enclosing:
                continue
            function = max(enclosing, key=lambda scope: scope[0])
            bindings, receivers, has_context = contexts[function[0]]
            if not has_context and not decode_scope:
                continue
            # The context operation shares the slice method's unstable name.
            receiver = words[index - 2] if index >= 2 else ""
            field_owner = words[index - 4] if index >= 4 and words[index - 3] == "." else ""
            if receiver in bindings or receiver in receivers.get(field_owner, set()):
                continue
            if fixed.accepts(tokens, pairs, parents, function, index):
                continue
            findings.append(Finding(
                "uncharged_decode_sort", relative_path(path),
                code.count("\n", 0, tokens[index].start()) + 1,
                f"Slice .{word} in decode code needs a proven fixed array and closed scalar comparison, or ctx.stable_sort_by or ctx.sort_unstable_by to admit comparison work and scratch"
                + ("." if has_context else "; pass the decode context to this function."),
            ))
    return findings


def check_source() -> list[Finding]:
    sources = {
        path.resolve(): path.read_text(encoding="utf-8", errors="replace")
        for path in sorted(ROOT.glob("crates/**/*.rs"))
    }
    findings = scan_placement(sources)
    for path, source in sources.items():
        if is_production_rs(path):
            findings.extend(scan_patterns(path, source))
    findings.extend(scan_decode_sorts(sources))
    findings.extend(scan_evaluation_refusals(sources))
    findings.extend(scan_wire_mirror_docs(sources))
    findings.extend(scan_module_visibility(sources))
    findings.extend(scan_authoring_paths())
    findings.extend(scan_script_tests())
    return sorted(findings, key=lambda item: (item.path, item.line, item.rule))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="emit structured findings")
    parser.add_argument("--crate", action="append", default=[], metavar="NAME",
                        help="report findings only for this crate (repeatable)")
    args = parser.parse_args(argv)
    findings = check_source()
    if args.crate:
        for name in args.crate:
            if not re.fullmatch(r"[A-Za-z0-9_-]+", name) or not (ROOT / "crates" / name).is_dir():
                parser.error(f"unknown crate: {name}")
        roots = {f"crates/{name}/" for name in args.crate}
        findings = [item for item in findings if any(item.path.startswith(root) for root in roots)]
    if args.json:
        print(json.dumps({"status": "fail" if findings else "ok",
                          "findings": [asdict(item) for item in findings]}, indent=2))
    elif findings:
        for item in findings:
            print(f"{item.path}:{item.line}: {item.rule}: {item.message}")
    else:
        print("source-policy: ok")
    return int(bool(findings))


if __name__ == "__main__":
    raise SystemExit(main())
