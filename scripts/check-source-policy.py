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
INTEGER_CLAMP = re.compile(
    r"\bunwrap_or(?:_else\s*\(\s*\|_?\|\s*|\s*\(\s*)"
    r"[ui](?:8|16|32|64|128|size)::(?:MAX|MIN)\s*\)"
)
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


def scan_patterns(path: Path, source: str) -> list[Finding]:
    """Inspect each source pattern once and report its location."""
    code, size = production_source(source)
    findings = []

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
    for match in INTEGER_CLAMP.finditer(code):
        report("integer_clamp", code.count("\n", 0, match.start()) + 1,
               "Do not clamp to an integer bound. Widen a usize with cadmpeg_core::decode::u64_from_index, or return the refusal.")
    declarations = {match.end() for match in NAMED_TOLERANCE_DECL.finditer(code)}
    for match in BARE_TOLERANCE.finditer(code):
        if match.start() not in declarations:
            report("bare_tolerance", code.count("\n", 0, match.start()) + 1,
                   "Give the tolerance a module-local constant name that states its intent.")
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
    r"\b([A-Za-z_]\w*)\s*:\s*&\s*(?:'[A-Za-z_]\w*\s*)?(?:mut\s+)?"
    r"(?:::\s*)?(?:[A-Za-z_]\w*\s*::\s*)*DecodeContext\b"
)


def scan_decode_sorts(sources: dict[Path, str]) -> list[Finding]:
    """Reject slice sorts in functions borrowing a decode context and in decode crate code.

    Function scopes exclude nested function items. Closures keep their enclosing
    context. Struct fields identify context access through a method's self value.
    """
    parsed = {}
    context_fields: dict[tuple[str, str], set[str]] = {}
    for path, source in sources.items():
        if not is_production_rs(path):
            continue
        code, _ = production_source(source)
        tokens, pairs, parents = evaluation_tokens(code)
        words = [token[0] for token in tokens]
        crate = Path(relative_path(path)).parts[1]
        parsed[path] = code, tokens, pairs, parents, words, crate
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

    findings = []
    for path, (code, tokens, pairs, parents, words, crate) in parsed.items():
        if relative_path(path) in DECODE_SORT_EXEMPT_FILES:
            continue
        decode_scope = bool(DECODE_SORT_CRATE.fullmatch(crate)) and not ENCODE_SORT_PATH.fullmatch(relative_path(path))
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
            fields = context_fields.get((crate, owner), set())
            bindings = set(DECODE_CONTEXT_BINDING.findall(scope))
            contexts[index] = bindings, fields, bool(bindings) or any(
                re.search(r"\bself\s*\.\s*" + re.escape(field) + r"\b", scope)
                for field in fields
            )
        for index, word in enumerate(words):
            if word not in SLICE_SORT_METHODS or index == 0 or words[index - 1] != ".":
                continue
            if evaluation_call_open(words, index) is None:
                continue
            enclosing = [scope for scope in functions if scope[1] < index < scope[2]]
            if not enclosing:
                continue
            bindings, fields, has_context = contexts[max(enclosing, key=lambda scope: scope[0])[0]]
            if not has_context and not decode_scope:
                continue
            # The context operation shares the slice method's unstable name.
            receiver = words[index - 2] if index >= 2 else ""
            if receiver in bindings or (receiver in fields and words[index - 4:index - 2] == ["self", "."]):
                continue
            findings.append(Finding(
                "uncharged_decode_sort", relative_path(path),
                code.count("\n", 0, tokens[index].start()) + 1,
                f"Slice .{word} in decode code must use ctx.stable_sort_by or ctx.sort_unstable_by to admit comparison work and scratch"
                + ("." if has_context else "; pass the decode context to this function."),
            ))
    return findings


DECODE_RESOURCE_CRATE = re.compile(
    r"cadmpeg-(?:core|ir|codec-[a-z0-9]+|container|asm|parasolid|protein)"
)


def decode_context_functions(sources):
    """Yield production function scopes holding the caller's decode context.

    Nested functions have independent bindings. Closures retain their enclosing
    bindings. Context fields resolve across files of the same crate.
    """
    parsed = []
    fields = {}
    declarations = {}
    for path, source in sources.items():
        relative = relative_path(path)
        parts = Path(relative).parts
        if (not is_production_rs(path) or len(parts) < 3
                or not DECODE_RESOURCE_CRATE.fullmatch(parts[1])
                or ENCODE_SORT_PATH.fullmatch(relative)):
            continue
        code, _ = production_source(source)
        tokens, pairs, parents = evaluation_tokens(code)
        imported = evaluation_imports(tokens, pairs)
        context_types = {"DecodeContext"} | {name for name, target in imported.items()
                                             if target[-1:] == ("DecodeContext",)}
        context_binding = re.compile(
            r"\b([A-Za-z_]\w*)\s*:\s*(?:&\s*(?:'[A-Za-z_]\w*\s*)?(?:mut\s+)?)?"
            r"(?:::\s*)?(?:[A-Za-z_]\w*\s*::\s*)*(?:" +
            "|".join(re.escape(name) for name in sorted(context_types)) + r")\b")
        words = [token[0] for token in tokens]
        parsed.append((path, source, code, tokens, pairs, parents, words, context_binding, context_types))
        for i, word in enumerate(words):
            if word != "struct" or i + 1 >= len(words):
                continue
            opening = i + 2
            while opening < len(words) and words[opening] not in {"{", ";", "("}:
                opening += 1
            if opening in pairs and words[opening] == "{":
                declarations.setdefault((parts[1], words[i + 1]), set()).add(path)
                fields[(parts[1], words[i + 1], path)] = set(context_binding.findall(
                    code[tokens[opening].end():tokens[pairs[opening]].start()]))
    for path, source, code, tokens, pairs, parents, words, context_binding, context_types in parsed:
        functions = []
        for index, name, _, owner in evaluation_signatures(tokens, pairs, parents):
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
            if body in pairs and words[body] == "{":
                functions.append((index, body, pairs[body], name, owner))
        for index, body, end, name, owner in functions:
            excluded = [(child, child_end) for child, _, child_end, _, _ in functions
                        if body < child < end]
            active = {i for i in range(index, end + 1)
                      if not any(start <= i <= stop for start, stop in excluded)}
            scope = "".join(code[tokens[i].start():tokens[i].end()] + " "
                            for i in sorted(active))
            bindings = set(context_binding.findall(scope))
            for context_type in context_types:
                bindings.update(re.findall(
                    r"\blet\s+(?:mut\s+)?([A-Za-z_]\w*)\s*=\s*&?\s*" +
                    re.escape(context_type) + r"\b", scope))
                bindings.update(re.findall(
                    r"\blet\s*\(\s*([A-Za-z_]\w*)\s*,[^;=]*\)\s*=\s*" +
                    r"(?:[A-Za-z_]\w*\s*::\s*)*" + re.escape(context_type) +
                    r"\s*::\s*(?:from_root_bytes(?:_limit)?|read_root)\b", scope))
            if owner == "DecodeContext":
                bindings.add("self")
            owner_key = (Path(relative_path(path)).parts[1], owner)
            owners = declarations.get(owner_key, set())
            declaring_path = path if path in owners else next(iter(owners)) if len(owners) == 1 else None
            context_fields = fields.get((*owner_key, declaring_path), set())
            receivers = bindings | {"self." + field for field in context_fields}
            for match in re.finditer(r"\blet\s+([A-Za-z_]\w*)\s*=\s*&?\s*([A-Za-z_]\w*(?:\s*\.\s*[A-Za-z_]\w*)?)\s*;", scope):
                if re.sub(r"\s", "", match[2]) in receivers:
                    receivers.add(match[1])
            if not receivers:
                continue
            yield path, source, code, tokens, pairs, parents, words, body, end, active, receivers


def decode_fixed_text(source, code, start, end):
    """A literal with no runtime operand has a fixed allocation size."""
    raw = source[start:end].strip()
    return not code[start:end].strip() and bool(re.fullmatch(
        r'(?:r\#*".*"\#*|"(?:[^"\\]|\\.)*")', raw, re.DOTALL))


def decode_static_values(source, code, start, end):
    """Only literals and separators prove a fixed allocation independent of input."""
    masked = code[start:end]
    return bool(source[start:end].strip()) and not re.sub(
        r"\b(?:true|false|(?:0x[0-9a-fA-F_]+|[0-9][0-9_]*)(?:_?[ui](?:8|16|32|64|128|size))?)\b|[\s,;.+\-]", "", masked)


def decode_fixed_format(source, code, tokens, pairs, words, opening, scope_start):
    """Static fields and primitive arguments have an input-independent bound."""
    first = opening + 1
    raw = source[tokens[opening].end():tokens[first].start()].strip()
    if not decode_fixed_text(source, code, tokens[opening].end(), tokens[first].start()):
        return False
    text = raw.replace("{{", "").replace("}}", "")
    fields = re.findall(r"\{([^{}]*)\}", text)
    # A runtime width or precision can make even primitive formatting unbounded.
    if any("$" in field or "*" in field for field in fields):
        return False
    supplied = set()
    cursor = first
    while cursor < pairs[opening]:
        if words[cursor] != ",":
            return False
        start = cursor + 1
        if start == pairs[opening] and not source[tokens[cursor].end():tokens[start].start()].strip():
            break
        cursor = start
        while cursor < pairs[opening] and words[cursor] != ",":
            cursor = pairs[cursor] + 1 if cursor in pairs and words[cursor] in "([{" else cursor + 1
        expression = "".join(words[start:cursor])
        if words[start + 1:start + 2] == ["="]:
            supplied.add(words[start])
            start += 2
            expression = "".join(words[start:cursor])
        begin = tokens[start - 1].end()
        finish = tokens[cursor].start()
        if not (decode_fixed_text(source, code, begin, finish)
                or decode_static_values(source, code, begin, finish)
                or decode_scalar_receiver(code[scope_start:], expression,
                                          tokens[opening].start() - scope_start)):
            return False
    for field in fields:
        name = field.split(":", 1)[0]
        if (name and not name.isdigit() and name not in supplied
                and not decode_scalar_receiver(code[scope_start:], name,
                                               tokens[opening].start() - scope_start)):
            return False
    return True


def scan_decode_allocations(sources: dict[Path, str]) -> list[Finding]:
    """Require charged text construction; unresolved operands are not exemptions."""
    findings = []
    imports = {}
    for (path, source, code, tokens, pairs, parents, words, body, end,
         active, receivers) in decode_context_functions(sources):
        if path not in imports:
            imports[path] = evaluation_imports(tokens, pairs)
        for i in sorted(active):
            word = words[i]
            if i <= body:
                continue
            if word == "format" and words[i + 1:i + 3] == ["!", "("]:
                opening = i + 2
                if opening not in pairs:
                    continue
                if decode_fixed_format(source, code, tokens, pairs, words,
                                       opening, tokens[min(active)].start()):
                    continue
                replacement = "ctx.format_retained(format_args!(...), operation)?"
            elif (word in {"to_string", "to_owned"} and i > 1
                  and words[i - 1] == "." and words[i + 1:i + 3] == ["(", ")"]):
                # Literals are masked to spaces. Their receiver is the gap after
                # the preceding token; a named receiver remains unresolved.
                previous = tokens[i - 2].end()
                if decode_fixed_text(source, code, previous, tokens[i - 1].start()):
                    continue
                replacement = "ctx.copy_retained_text(text, operation)? (or ctx.format_retained for Display)"
            elif word == "vec" and words[i + 1:i + 3] == ["!", "["]:
                opening = i + 2
                if opening not in pairs:
                    continue
                first, last = tokens[opening].end(), tokens[pairs[opening]].start()
                if not source[first:last].strip() or decode_static_values(source, code, first, last):
                    continue
                replacement = "ctx.alloc_filled for Copy values or ctx.push_vec for admitted elements"
            elif word in {"to_vec", "collect", "clone", "cloned"} and i > 1 and words[i - 1] == ".":
                if evaluation_call_open(words, i) is None:
                    continue
                replacement = {
                    "to_vec": "ctx.copy_slice for Copy elements or ctx.copy_retained_strings for text",
                    "collect": "ctx.collect_vec / ctx.try_collect_vec / the charged map or set operation",
                    "clone": "ctx.copy_retained_text / ctx.copy_slice / explicit field construction with charged child copies; copy a Copy value directly",
                    "cloned": "a charged child-copy map; use .copied() for Copy elements",
                }[word]
            elif word == "alloc_filled" and words[i - 1:i] == ["."]:
                opening = evaluation_call_open(words, i)
                if opening not in pairs:
                    continue
                comma = opening + 1
                while comma < pairs[opening] and words[comma] != ",":
                    comma = pairs[comma] + 1 if comma in pairs and words[comma] in "([{" else comma + 1
                start = comma + 1
                stop = start
                while stop < pairs[opening] and words[stop] != ",":
                    stop = pairs[stop] + 1 if stop in pairs and words[stop] in "([{" else stop + 1
                initializer = "".join(words[start:stop])
                if (initializer in {"None", "Vec::new()", "String::new()"}
                        or decode_static_values(source, code, tokens[start - 1].end(), tokens[stop].start())
                        or decode_fixed_text(source, code, tokens[start - 1].end(), tokens[stop].start())
                        or decode_scalar_receiver(code[tokens[min(active)].start():], initializer,
                                                  tokens[i].start() - tokens[min(active)].start())):
                    continue
                replacement = "ctx.collect_indexed_vec with explicit Copy assignments or charged child construction"
            elif (word in {"from", "clone", "cloned", "to_owned", "to_string", "to_vec", "collect"}
                  and words[i - 1:i] == ["::"]):
                opening = evaluation_call_open(words, i)
                if opening is None or opening not in pairs:
                    continue
                if word == "from" and words[i - 2] != "String":
                    continue
                if word == "clone":
                    owner = i - 2
                    while owner > 1 and words[owner - 1] == "::":
                        owner -= 2
                    owner_words = words[owner:i - 1]
                    resolved = imports[path].get(owner_words[0], (owner_words[0],)) + tuple(owner_words[2::2])
                    if resolved in {("std", "rc", "Rc"), ("std", "sync", "Arc")}:
                        continue
                first, last = tokens[opening].end(), tokens[pairs[opening]].start()
                if word == "from" and decode_fixed_text(source, code, first, last):
                    continue
                replacement = "the matching ctx copy, format or collection operation"
            else:
                continue
            findings.append(Finding(
                "uncharged_decode_allocation", relative_path(path),
                code.count("\n", 0, tokens[i].start()) + 1,
                f"{word} creates unadmitted owned storage or has unresolved ownership; use {replacement}. "
                "Raw text construction is not admitted by a separate charge."))
    return findings


DECODE_SCAN_METHODS = {
    "any", "all", "position", "rposition", "find", "rfind", "find_map",
    "min_by_key", "max_by_key", "min_by", "max_by", "min", "max",
    "contains", "starts_with", "ends_with", "eq", "cmp", "partial_cmp",
    "fold", "try_fold", "reduce", "sum", "count", "for_each", "try_for_each",
}


def decode_receiver(words, pairs, index):
    """Read the complete receiver of a method, including chained calls."""
    stop = index - 1
    start = stop - 1
    while start >= 0:
        if words[start] in {")", "]"}:
            opening = pairs.get(start)
            if opening is None:
                break
            start = opening
            if opening and (re.fullmatch(r"[A-Za-z_]\w*", words[opening - 1])
                            or words[opening - 1] in {")", "]"}):
                if words[opening - 1] not in {"in", "if", "return", "while", "match"}:
                    start = opening - 1
                    continue
            if start > 0 and words[start - 1] in {".", "::"}:
                start -= 2
                continue
            break
        if start > 0 and words[start - 1] in {".", "::"}:
            start -= 2
            continue
        break
    return "".join(words[max(0, start):stop])


def decode_work_charge(words, pairs, index, receivers):
    """A charge must use this context and propagate its Result with ?."""
    if words[index] not in {"charge_work", "charge_work_limit"}:
        return None
    if decode_receiver(words, pairs, index) not in receivers:
        return None
    opening = evaluation_call_open(words, index)
    if not decode_propagated_call(words, pairs, opening):
        return None
    stop = opening + 1
    while stop < pairs[opening]:
        if words[stop] == ",":
            break
        stop = pairs[stop] + 1 if stop in pairs and words[stop] in "([{" else stop + 1
    return "".join(words[opening + 1:stop])


def decode_propagated_call(words, pairs, opening):
    """A direct refusal or resource-payload conversion reaches the caller."""
    if opening not in pairs:
        return False
    following = pairs[opening] + 1
    if words[following:following + 2] == [".", "map_err"]:
        conversion = following + 2
        if conversion not in pairs:
            return False
        constructor = words[conversion + 1:pairs[conversion]]
        if (not constructor or constructor[-1] != "ResourceLimit"
                or not re.fullmatch(r"(?:[A-Za-z_]\w*::)*ResourceLimit", "".join(constructor))):
            return False
        following = pairs[conversion] + 1
    return words[following:following + 1] == ["?"]


DECODE_BUDGET_OPERATIONS = {
    "charge_input", "charge_decompressed", "charge_retained", "charge_retained_limit",
    "charge_entities", "charge_collection_items", "charge_collection_items_limit",
    "charge_work", "charge_work_limit", "reserve_scoped", "reserve_scoped_limit",
    "enter_nested",
}


def decode_charged_methods(sources):
    """Resolve context operations through their core method call graph."""
    dependencies = {}
    charged = set(DECODE_BUDGET_OPERATIONS)
    for path, source in sources.items():
        if not relative_path(path).startswith("crates/cadmpeg-core/src/decode/"):
            continue
        code, _ = production_source(source)
        tokens, pairs, parents = evaluation_tokens(code)
        words = [token[0] for token in tokens]
        for index, name, _, owner in evaluation_signatures(tokens, pairs, parents):
            if owner != "DecodeContext":
                continue
            opening = index + 2
            if words[opening:opening + 1] == ["<"]:
                depth = 1
                opening += 1
                while opening < len(words) and depth:
                    depth += (words[opening] == "<") - (words[opening] == ">")
                    opening += 1
            if opening not in pairs:
                continue
            body = pairs[opening] + 1
            while body < len(words) and words[body] not in {"{", ";"}:
                body = pairs[body] + 1 if body in pairs and words[body] in "([" else body + 1
            if body not in pairs:
                continue
            calls = dependencies.setdefault(name, set())
            for i in range(body + 1, pairs[body]):
                if evaluation_call_open(words, i) is None:
                    continue
                receiver = decode_receiver(words, pairs, i) if words[i - 1:i] == ["."] else ""
                if receiver == "self":
                    calls.add(words[i])
                elif receiver == "self.budget" and words[i] in DECODE_BUDGET_OPERATIONS:
                    charged.add(name)
    while True:
        additions = {name for name, calls in dependencies.items() if calls & charged} - charged
        if not additions:
            return charged
        charged.update(additions)


def decode_call_arguments(words, pairs, opening):
    """Keep each direct argument separate from nested calls and closures."""
    start = opening + 1
    stop = start
    while stop < pairs[opening]:
        if words[stop] == ",":
            yield start, stop
            start = stop + 1
        stop = pairs[stop] + 1 if stop in pairs and words[stop] in "([{" else stop + 1
    if start < stop:
        yield start, stop


def decode_charged_call(words, pairs, index, receivers, methods):
    """A context operation or a context-taking call owns its admitted work."""
    opening = evaluation_call_open(words, index)
    if not decode_propagated_call(words, pairs, opening):
        return False
    if words[index - 1:index] == ["."] and decode_receiver(words, pairs, index) in receivers:
        if words[index] not in methods:
            return False
        arguments = list(decode_call_arguments(words, pairs, opening))
        if words[index] in DECODE_BUDGET_OPERATIONS - {"enter_nested"} and arguments:
            first = "".join(words[slice(*arguments[0])])
            return decode_positive_step(first)
        return True
    return any("".join(words[start:stop]).lstrip("&") in receivers
               for start, stop in decode_call_arguments(words, pairs, opening))


def decode_paid_prefix(words, pairs, start, stop, receivers, methods):
    """Every branch must reach admission before its first effect.

    Binding a value and reading its fixed metadata have no input-sized effect.
    A call's arguments run before the call. A closure's body runs separately.
    """
    pure_calls = {"len", "capacity", "position", "u64_from_index", "as_str", "as_bytes"}
    cursor = start
    statement = start
    while cursor < stop:
        word = words[cursor]
        if word in {"return", "break", "continue", "for", "while", "loop"}:
            return False
        if word in {"if", "match"}:
            branch = cursor + 1
            while branch < stop and words[branch] != "{":
                branch = pairs[branch] + 1 if branch in pairs and words[branch] in "([" else branch + 1
            if branch not in pairs:
                return False
            condition = decode_paid_prefix(words, pairs, cursor + 1, branch, receivers, methods)
            if condition is True:
                return True
            if condition is False:
                return False
            following = pairs[branch] + 1
            if word == "if":
                paths = [decode_paid_prefix(words, pairs, branch + 1, pairs[branch], receivers, methods)]
                if words[following:following + 1] == ["else"]:
                    alternative = following + 1
                    alternative_stop = pairs.get(alternative, stop) if words[alternative] == "{" else stop
                    alternative_start = alternative + 1 if words[alternative] == "{" else alternative
                    paths.append(decode_paid_prefix(words, pairs, alternative_start, alternative_stop, receivers, methods))
                    following = alternative_stop + 1
                else:
                    paths.append(None)
            else:
                paths = []
                arm = branch + 1
                while arm < pairs[branch]:
                    arrow = arm
                    while arrow < pairs[branch] and words[arrow] != "=>":
                        # Guards have their own effects and cannot be skipped.
                        if words[arrow] == "if":
                            return False
                        arrow = pairs[arrow] + 1 if arrow in pairs and words[arrow] in "([{" else arrow + 1
                    if arrow == pairs[branch]:
                        return False
                    arm_end = arrow + 1
                    while arm_end < pairs[branch] and words[arm_end] != ",":
                        if words[arm_end] == "{" and arm_end == arrow + 1:
                            arm_end = pairs[arm_end] + 1
                            break
                        arm_end = pairs[arm_end] + 1 if arm_end in pairs and words[arm_end] in "([{" else arm_end + 1
                    paths.append(decode_paid_prefix(words, pairs, arrow + 1, arm_end, receivers, methods))
                    arm = arm_end + (words[arm_end:arm_end + 1] == [","])
            if any(path is False for path in paths):
                return False
            if paths and all(path is True for path in paths):
                return True
            cursor = following
            statement = cursor
            continue
        if word == "{":
            return decode_paid_prefix(words, pairs, cursor + 1, pairs[cursor], receivers, methods)
        opening = evaluation_call_open(words, cursor) if re.fullmatch(r"[A-Za-z_]\w*", word) else None
        if opening in pairs:
            if decode_charged_call(words, pairs, cursor, receivers, methods):
                if word in DECODE_BUDGET_OPERATIONS:
                    return True
                # An eager uncharged argument must not be hidden by its parent.
                for first, last in decode_call_arguments(words, pairs, opening):
                    if words[first:first + 1] == ["|"] or words[first:first + 2] == ["move", "|"]:
                        continue
                    for child in range(first, last):
                        child_open = evaluation_call_open(words, child)
                        if child_open in pairs and words[child] not in pure_calls:
                            if not decode_charged_call(words, pairs, child, receivers, methods):
                                return False
                return True
            if word not in pure_calls:
                return False
            cursor = pairs[opening] + 1
            continue
        # Assignment to existing state is an effect; a local binding is not.
        if (word == "=" and words[cursor - 1:cursor] not in [["="], ["!"], ["<"], [">"]]
                and words[cursor + 1:cursor + 2] != ["="] and "let" not in words[statement:cursor]):
            return False
        if word in {"|", "&"} and words[cursor + 1:cursor + 2] == [word]:
            return False
        if word == ";":
            statement = cursor + 1
        cursor += 1
    return None


def decode_extent(expression):
    """Normalize direct collection iteration and counted ranges to their extent."""
    expression = expression.lstrip("&")
    if ".." in expression:
        expression = expression.split("..", 1)[1].lstrip("=")
    for suffix in (".enumerate()", ".rev()", ".copied()", ".cloned()",
                   ".into_iter()", ".iter_mut()", ".iter()", ".as_bytes()", ".keys()", ".values()"):
        expression = expression.replace(suffix, "")
    return expression


def decode_charge_covers(amount, extent):
    """Accept an exact named extent or its length wrapped by u64_from_index."""
    amount = re.sub(r"(?:(?:[A-Za-z_]\w*::)*u64_from_index|u64::from)\(([^()]*(?:\.(?:len|capacity)\(\))?)\)", r"\1", amount)
    return amount in {extent, extent + ".len()", extent + ".capacity()"}


def decode_block(parents, words, index):
    """Closest block owns the charge; call parentheses do not change control flow."""
    parent = parents.get(index)
    while parent is not None and words[parent] != "{":
        parent = parents.get(parent)
    return parent


def decode_scalar_receiver(code, name, position):
    """An explicit primitive binding with no later rebinding has fixed work."""
    if not re.fullmatch(r"[A-Za-z_]\w*", name):
        return False
    declarations = list(re.finditer(
        r"\b" + re.escape(name) + r"\s*:\s*&?\s*(?:bool|char|[ui](?:8|16|32|64|128|size)|f(?:32|64))\b",
        code[:position]))
    if not declarations:
        return False
    declaration = declarations[-1]
    tail = code[declaration.end():position]
    depth = 0
    for character in tail:
        depth += (character == "{") - (character == "}")
        if depth < 0:
            return False
    prefix = code[:declaration.start()]
    pipe = prefix.rfind("|")
    if (pipe >= 0 and decode_closure_open(prefix, pipe)
            and not re.search(r"[{};]", prefix[pipe + 1:])):
        closing = tail.find("|")
        if closing < 0:
            return False
        expression = tail[closing + 1:]
        stack = []
        block_body = expression.lstrip().startswith("{")
        for character in expression:
            if character in "([{":
                stack.append(character)
            elif character in ")]}":
                if not stack:
                    return False
                stack.pop()
                if not stack and block_body:
                    return False
            elif character in ",;" and not stack:
                return False
    if re.search(r"\blet\s+(?:mut\s+)?" + re.escape(name) + r"\b", tail):
        return False
    for pipe in re.finditer(r"\|", tail):
        if not decode_closure_open(tail, pipe.start()):
            continue
        closing = tail.find("|", pipe.end())
        if closing >= 0 and re.search(r"\b" + re.escape(name) + r"\b", tail[pipe.end():closing]):
            return False
    return True


def decode_closure_open(code, pipe):
    """A closing parameter pipe cannot introduce a new shadow binding."""
    prefix = code[:pipe].rstrip()
    return not prefix or prefix[-1] in "=(,{;" or bool(re.search(r"\bmove$", prefix))


def decode_positive_step(amount):
    """A literal positive amount or checked positive increment admits a step."""
    if re.fullmatch(r"[0-9][0-9_]*(?:_?u(?:8|16|32|64|128|size))?", amount):
        return int(re.sub(r"u(?:8|16|32|64|128|size)$", "", amount).replace("_", "")) > 0
    increment = re.search(r"\.checked_add\([1-9][0-9_]*(?:_?u64)?\)", amount)
    if increment is None or not amount.endswith("?"):
        return False
    tail = amount[increment.end():]
    conversion = re.match(r"\.ok_or(?:_else)?\(", tail)
    if conversion is None:
        return False
    depth = 1
    for index in range(conversion.end(), len(tail)):
        depth += (tail[index] == "(") - (tail[index] == ")")
        if depth == 0:
            return tail[index + 1:] == "?"
    return False


DECODE_PRIMITIVES = {"bool", "char"} | {
    prefix + width for prefix in "ui" for width in ("8", "16", "32", "64", "128", "size")
} | {"f32", "f64"}


def decode_split(expression, separator):
    """Split type or value components at their own delimiter depth."""
    tokens, pairs, _ = evaluation_tokens(expression)
    start = 0
    cursor = 0
    while cursor < len(tokens):
        word = tokens[cursor][0]
        if word == separator:
            yield expression[start:tokens[cursor].start()].strip()
            start = tokens[cursor].end()
        cursor = pairs[cursor] + 1 if cursor in pairs and word in "([{" else cursor + 1
    yield expression[start:].strip()


def decode_constant_expression(expression, constants):
    """Literal arithmetic and declared integer constants have fixed bounds."""
    expression = re.sub(r"\b(?:" + "|".join(sorted(DECODE_PRIMITIVES)) +
                        r")::(?:MAX|MIN|BITS)\b", "0", expression)
    expression = re.sub(r"\b(?:0x[0-9a-fA-F_]+|[0-9][0-9_]*)(?:_?[ui](?:8|16|32|64|128|size))?\b", "0", expression)
    for name in sorted(constants, key=len, reverse=True):
        expression = re.sub(r"\b" + re.escape(name) + r"\b", "0", expression)
    return "0" in expression and not re.sub(r"[0\s()+*/%<>&|^!~.\-]", "", expression)


def decode_type_shape(expression, constants, copy_types):
    """Return fixed cardinality and fixed comparison work independently."""
    expression = re.sub(r"^&\s*(?:'[A-Za-z_]\w*\s*)?(?:mut\s+)?", "", expression.strip())
    if expression in DECODE_PRIMITIVES or expression in copy_types:
        return True, True
    if expression.startswith("[") and expression.endswith("]"):
        parts = list(decode_split(expression[1:-1], ";"))
        if len(parts) == 2 and decode_constant_expression(parts[1], constants):
            return True, decode_type_shape(parts[0], constants, copy_types)[1]
    if expression.startswith("(") and expression.endswith(")"):
        parts = [part for part in decode_split(expression[1:-1], ",") if part]
        return True, all(decode_type_shape(part, constants, copy_types)[1] for part in parts)
    return False, False


def decode_fixed_catalog(sources):
    """Read integer constants and structural Copy scalar types per crate."""
    catalogs = {}
    records = []
    for path, source in sources.items():
        if not is_production_rs(path):
            continue
        parts = Path(relative_path(path)).parts
        if len(parts) < 3:
            continue
        constants, copy_types = catalogs.setdefault(parts[1], (set(), set()))
        code, _ = production_source(source)
        tokens, pairs, parents = evaluation_tokens(code)
        words = [token[0] for token in tokens]
        functions = {index for index, word in enumerate(words) if word == "fn"}
        function_blocks = set()
        for index in functions:
            cursor = index + 1
            while cursor < len(words) and words[cursor] not in {"{", ";"}:
                cursor = pairs[cursor] + 1 if cursor in pairs and words[cursor] in "([" else cursor + 1
            function_blocks.add(cursor)
        for i, word in enumerate(words):
            if word == "const" and words[i + 2:i + 3] == [":"] and words[i + 3] in DECODE_PRIMITIVES:
                parent = parents.get(i)
                while parent is not None and parent not in function_blocks:
                    parent = parents.get(parent)
                if parent is None:
                    constants.add(words[i + 1])
            if word not in {"struct", "enum"}:
                continue
            opening = i + 2
            # Generic records require their instantiated field types.
            if words[opening:opening + 1] == ["<"]:
                continue
            if opening not in pairs or words[opening] not in {"{", "("}:
                continue
            prefix = code[:tokens[i].start()]
            attributes = prefix[max(prefix.rfind("}"), prefix.rfind(";")) + 1:]
            if not re.search(r"\bderive\s*\([^)]*\bCopy\b", attributes):
                continue
            contents = code[tokens[opening].end():tokens[pairs[opening]].start()]
            if word == "struct":
                fields = [part.split(":", 1)[1] if words[opening] == "{" and ":" in part else part
                          for part in decode_split(contents, ",") if part]
            else:
                # Unit variants are scalar. Payload variants are resolved below.
                fields = []
                for variant in decode_split(contents, ","):
                    if "(" in variant:
                        fields.extend(decode_split(variant[variant.index("(") + 1:variant.rfind(")")], ","))
                    elif "{" in variant:
                        fields.extend(part.split(":", 1)[-1] for part in decode_split(
                            variant[variant.index("{") + 1:variant.rfind("}")], ",") if part)
            records.append((parts[1], words[i + 1], fields))
    while True:
        additions = 0
        for crate, name, fields in records:
            constants, copy_types = catalogs[crate]
            if name not in copy_types and all(decode_type_shape(field, constants, copy_types)[1] for field in fields):
                copy_types.add(name)
                additions += 1
        if not additions:
            return catalogs


def decode_binding_visible(code, declaration, position, name):
    """A proof cannot escape its block, closure or a shadowing declaration."""
    tail = code[declaration.end():position]
    depth = 0
    for character in tail:
        depth += (character == "{") - (character == "}")
        if depth < 0:
            return False
    if re.search(r"\blet\s+(?:mut\s+)?" + re.escape(name) + r"\b", tail):
        return False
    prefix = code[:declaration.start()]
    pipe = prefix.rfind("|")
    if pipe >= 0 and decode_closure_open(prefix, pipe) and not re.search(r"[{};]", prefix[pipe + 1:]):
        closing = tail.find("|")
        if closing < 0:
            return False
        expression = tail[closing + 1:]
        tokens, pairs, _ = evaluation_tokens(expression)
        if tokens and tokens[0][0] == "{" and pairs.get(0) is not None:
            return False
        if any(token[0] in {",", ";"} and index not in pairs
               for index, token in enumerate(tokens)):
            return False
    for pipe in re.finditer(r"\|", tail):
        if decode_closure_open(tail, pipe.start()):
            closing = tail.find("|", pipe.end())
            if closing >= 0 and re.search(r"\b" + re.escape(name) + r"\b", tail[pipe.end():closing]):
                return False
    return True


def decode_expression_shape(expression, code, position, constants, copy_types, seen=frozenset()):
    """Prove fixed extents without treating an input-sized range as constant."""
    expression = expression.strip().lstrip("&*").strip()
    if not expression or expression in seen:
        return False, False
    seen = seen | {expression}
    tokens, pairs, _ = evaluation_tokens(expression)
    words = [token[0] for token in tokens]
    if words[-1:] == [")"] and pairs.get(len(words) - 1) is not None:
        opening = pairs[len(words) - 1]
        if opening >= 2 and words[opening - 2] == ".":
            method = words[opening - 1]
            if method in {"iter", "iter_mut", "into_iter", "enumerate", "rev", "copied", "cloned", "map", "as_slice", "as_bytes"}:
                shape = decode_expression_shape(expression[:tokens[opening - 2].start()], code,
                                                position, constants, copy_types, seen)
                return shape[0], shape[1] and method not in {"map", "enumerate"}
        if opening == 0:
            inner = expression[1:-1]
            parts = [part for part in decode_split(inner, ",") if part]
            if len(parts) == 1:
                return decode_expression_shape(inner, code, position, constants, copy_types, seen)
            return True, all(decode_expression_shape(part, code, position, constants, copy_types, seen)[1] for part in parts)
    if ".." in words:
        index = words.index("..")
        return (decode_constant_expression("".join(words[:index]), constants)
                and decode_constant_expression("".join(words[index + 1:]).lstrip("="), constants)), False
    if expression.startswith("[") and expression.endswith("]"):
        inner = expression[1:-1]
        parts = list(decode_split(inner, ";"))
        if len(parts) == 2:
            return (decode_constant_expression(parts[1], constants),
                    decode_constant_expression(parts[1], constants)
                    and decode_expression_shape(parts[0], code, position, constants, copy_types, seen)[1])
        return True, all(decode_expression_shape(part, code, position, constants, copy_types, seen)[1]
                         for part in decode_split(inner, ",") if part)
    if decode_constant_expression(expression, constants) or expression in {"true", "false"}:
        return True, True
    if not re.fullmatch(r"[A-Za-z_]\w*", expression):
        return False, False
    declarations = list(re.finditer(r"\b" + re.escape(expression) + r"\s*:\s*|\blet\s+(?:mut\s+)?" + re.escape(expression) + r"\s*=\s*", code[:position]))
    if not declarations:
        return False, False
    declaration = declarations[-1]
    if not decode_binding_visible(code, declaration, position, expression):
        return False, False
    remainder = code[declaration.end():position]
    value_tokens, value_pairs, _ = evaluation_tokens(remainder)
    stop = 0
    while stop < len(value_tokens) and value_tokens[stop][0] not in {",", ";", "=", ")", "|"}:
        stop = value_pairs[stop] + 1 if stop in value_pairs and value_tokens[stop][0] in "([{" else stop + 1
    value = remainder[:value_tokens[stop].start()] if stop < len(value_tokens) else remainder
    if ":" in declaration[0]:
        return decode_type_shape(value, constants, copy_types)
    return decode_expression_shape(value, code, declaration.start(), constants, copy_types, seen)


def scan_decode_work(sources: dict[Path, str]) -> list[Finding]:
    """Reject unadmitted scans; unresolved linear-work forms require an explicit charge."""
    findings = []
    methods = decode_charged_methods(sources)
    fixed_catalog = decode_fixed_catalog(sources)
    for (path, source, code, tokens, pairs, parents, words, body, end,
         active, receivers) in decode_context_functions(sources):
        constants, copy_types = fixed_catalog.get(Path(relative_path(path)).parts[1], (set(), set()))
        scope_start = tokens[min(active)].start()
        scope_code = code[scope_start:tokens[end].end()]
        # Const generic bounds are fixed for each instantiated function.
        constants = constants | set(re.findall(r"\bconst\s+([A-Za-z_]\w*)\s*:", scope_code))
        charges = {}
        aliases = {}
        # Simple extent aliases retain identity, not arbitrary arithmetic.
        for i in sorted(active):
            if words[i] == "let" and words[i + 2:i + 3] == ["="]:
                stop = i + 3
                while stop < end and words[stop] != ";":
                    stop = pairs[stop] + 1 if stop in pairs and words[stop] in "([{" else stop + 1
                expr = "".join(words[i + 3:stop])
                aliases.setdefault(words[i + 1], []).append((i, expr))
            amount = decode_work_charge(words, pairs, i, receivers) if i > body else None
            if amount is not None:
                charges[i] = amount
        slice_names = set(re.findall(
            r"\b([A-Za-z_]\w*)\s*:\s*(?:&\s*(?:'[A-Za-z_]\w*\s*)?(?:mut\s+)?\[|Vec\s*<|String\b|&\s*str\b)",
            code[tokens[min(active)].start():tokens[body].start()]))
        consumed = set()
        for i in sorted(active):
            if i <= body:
                continue
            word = words[i]
            loop_body = None
            extent = None
            if word in {"for", "while", "loop"}:
                # Ignore impl/trait headers and higher-ranked type declarations.
                opening = i + 1
                while opening < end and words[opening] not in {"{", ";"}:
                    opening = pairs[opening] + 1 if opening in pairs and words[opening] in "([" else opening + 1
                if opening not in pairs or words[opening] != "{":
                    continue
                loop_body = opening
                header = words[i + 1:opening]
                if word == "for":
                    if "in" not in header:
                        continue
                    extent = decode_extent("".join(header[header.index("in") + 1:]))
                    in_index = i + 1 + header.index("in")
                    expression = "".join(words[in_index + 1:opening])
                    if decode_expression_shape(expression, scope_code, tokens[i].start() - scope_start,
                                               constants, copy_types)[0]:
                        continue
                else:
                    extent = "".join(header)
            elif word in DECODE_SCAN_METHODS and words[i - 1:i] == ["."]:
                if evaluation_call_open(words, i) is None:
                    continue
                receiver = decode_receiver(words, pairs, i)
                if receiver in receivers:
                    continue
                # Scalar min/max are constant-time; iterator variants have no operand.
                opening = evaluation_call_open(words, i)
                if word in {"min", "max"} and pairs.get(opening) != opening + 1:
                    continue
                if word in {"position", "rposition", "eq", "cmp", "partial_cmp"} and pairs.get(opening) == opening + 1:
                    continue
                if word in {"eq", "cmp", "partial_cmp"} and decode_scalar_receiver(
                        code[tokens[min(active)].start():], receiver,
                        tokens[i].start() - tokens[min(active)].start()):
                    continue
                extent = decode_extent(receiver)
                shape = decode_expression_shape(receiver, scope_code, tokens[i].start() - scope_start,
                                                constants, copy_types)
                if shape[1] or (shape[0] and word not in {"contains", "starts_with", "ends_with", "eq", "cmp", "partial_cmp"}):
                    continue
            elif word in {"=", "!"} and words[i + 1:i + 2] == ["="]:
                left, right = i - 1, i + 2
                if words[left:left + 1] in [["="], ["!"], ["<"], [">"]]:
                    continue
                left_name = words[left] if left >= 0 else ""
                right_name = words[right] if right < len(words) else ""
                if any(decode_expression_shape(name, scope_code, tokens[i].start() - scope_start,
                                               constants, copy_types)[1] for name in (left_name, right_name)):
                    continue
                count_queries = [[".", method, "(", ")"] for method in {"len", "capacity", "position"}]
                right_query = right
                while words[right_query + 1:right_query + 2] == ["."]:
                    right_query += 2
                if (words[i - 4:i] in count_queries
                        or (words[right_query:right_query + 1] in [[method] for method in {"len", "capacity", "position"}]
                            and words[right_query + 1:right_query + 3] == ["(", ")"])):
                    continue
                # Byte and character literals keep a b prefix in the masked
                # token stream. Their comparison has a fixed upper bound.
                raw_right = source[tokens[i + 1].end():].lstrip()
                if re.match(r"(?:b|br\#*)?[\"']", raw_right):
                    continue
                if (decode_scalar_receiver(code[tokens[min(active)].start():], left_name,
                                           tokens[i].start() - tokens[min(active)].start())
                        or decode_scalar_receiver(code[tokens[min(active)].start():], right_name,
                                                  tokens[i].start() - tokens[min(active)].start())):
                    continue
                indexed = words[left:left + 1] == ["]"] or words[right + 1:right + 2] == ["["]
                if indexed:
                    raw_left = decode_receiver(words, pairs, i + 1)
                    extent = raw_left.split("[", 1)[0]
                    if ".." not in raw_left and ".." not in "".join(words[right:right + 12]):
                        continue
                elif left_name in slice_names or right_name in slice_names:
                    extent = left_name if left_name in slice_names else right_name
                else:
                    # Literal-size comparisons and explicitly scalar operands
                    # have constant work. Unresolved named comparisons do not.
                    if (not re.fullmatch(r"[A-Za-z_]\w*", left_name)
                            or not re.fullmatch(r"[A-Za-z_]\w*", right_name)
                            or right_name in {"true", "false", "None"}):
                        continue
                    extent = left_name
            else:
                continue
            admitted = False
            if loop_body is not None:
                admitted = decode_paid_prefix(words, pairs, loop_body + 1,
                                              pairs[loop_body], receivers, methods)
            if not admitted:
                parent = decode_block(parents, words, i)
                for charge, amount in charges.items():
                    if charge >= i or charge in consumed or decode_block(parents, words, charge) != parent:
                        continue
                    resolved = amount
                    for declared, expression in reversed(aliases.get(amount, [])):
                        if declared < charge and decode_block(parents, words, declared) == parent:
                            resolved = expression
                            break
                    between = " ".join(words[pairs[evaluation_call_open(words, charge)] + 2:i])
                    root = extent.split(".", 1)[0]
                    mutated = re.search(
                        r"(?:&\s*mut\s+" + re.escape(root) + r"\b|\b" +
                        re.escape(root) + r"\s*(?:=|\.\s*(?:push|insert|extend|append|resize|retain|clear)\s*\())",
                        between)
                    if decode_charge_covers(resolved, extent) and not mutated:
                        admitted = True
                        consumed.add(charge)
                        break
            if not admitted:
                findings.append(Finding(
                    "uncharged_decode_work", relative_path(path),
                    code.count("\n", 0, tokens[i].start()) + 1,
                    f"{word} scans decoded data or has unresolved work; use a propagated ctx.charge_work "
                    "for this extent immediately before the scan, or as the first loop-body statement. "
                    "Use ctx.position_by / ctx.equal_bytes for fallible search or byte equality."))
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
    findings.extend(scan_decode_allocations(sources))
    findings.extend(scan_decode_work(sources))
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
