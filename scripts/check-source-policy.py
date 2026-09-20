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
NAMED_TOLERANCE_DECL = re.compile(
    r"^\s*(?:(?:pub(?:\([^)]*\))?|unsafe)\s+)*"
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
REEXPORT_USE = re.compile(r"^\s*pub(?:\s*\([^)]*\))?\s+use\s+(?P<path>[^;]*);", re.MULTILINE)
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


# A serde wire mirror is the type a `#[serde(try_from = "…")]` or
# `#[serde(from = "…")]` container attribute names. The mirror spells the wire
# shape of the admitted type, and its member documentation is what the
# published JSON schema reads as each property's `description`. The rule covers
# the crates whose types that schema carries; a codec-private record generates
# no schema and states its shape through `NativeRecord`.
WIRE_MIRROR_DOC_ROOTS = ("crates/cadmpeg-ir",)
SERDE_ATTRIBUTE = re.compile(r"#\s*\[\s*serde\s*\(")
SERDE_MIRROR_TARGET = re.compile(r"(?<![\w.])(?:try_from|from)\s*=\s*\"(?P<target>[^\"]+)\"")
NAMED_TYPE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
TYPE_DECL = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?P<kind>struct|enum)\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
)
DOC_COMMENT = re.compile(r"^\s*///")
DOC_ATTRIBUTE = re.compile(r"^#\s*\[\s*doc\b")
MIRROR_FIELD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*:")
MIRROR_VARIANT = re.compile(r"^\s*(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*(?:\{|\(|=|,|$)")


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
            target = match.group("target").rsplit("::", 1)[-1].strip()
            if NAMED_TYPE.match(target):
                targets.add(target)
    return targets


def member_has_doc(lines: list[str], masked: list[str], index: int, floor: int) -> bool:
    """Whether a doc comment or `#[doc]` stands above the member at ``index``.

    The walk steps over blank lines, ordinary comments and whole attribute
    blocks, so a doc comment above a multi-line `#[serde(…)]` still documents
    the member below it.
    """
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
    end = find_matching_brace_end(masked, body)
    depth = 0
    for line in range(body, end):
        depth += masked[line].count("{") - masked[line].count("}")
        after = line + 1
        if after >= end:
            break
        inner = depth
        if inner == 1:
            pattern = MIRROR_VARIANT if kind == "enum" else MIRROR_FIELD
        elif inner == 2:
            pattern = MIRROR_FIELD
        else:
            continue
        match = pattern.match(masked[after])
        if match is None:
            continue
        yield match.group("name"), after


def scan_wire_mirror_docs(sources: dict[Path, str]) -> list[Finding]:
    """Report a serde wire mirror member that carries no doc comment.

    The mirror is the shape the wire states, and the published JSON schema
    reads each member's doc as that property's `description`. A member with no
    doc leaves the schema silent about the value the wire carries.

    The scan reads the whole tree: a declaration names its mirror by type name,
    and the mirror itself is often declared in another file, so no per-file
    scan can pair the two.
    """
    files = sorted(path for path in sources if is_production_rs(path))
    parsed: dict[Path, tuple[list[str], list[str]]] = {}
    targets: dict[Path, set[str]] = {}
    in_file: dict[Path, dict[str, list[tuple[str, int]]]] = {}
    in_crate: dict[str, dict[str, list[tuple[Path, str, int]]]] = {}
    for path in files:
        lines = sources[path].splitlines()
        code, _ = production_source(sources[path])
        masked = code.splitlines()
        parsed[path] = (lines, masked)
        crate = relative_path(path).split("/")[1]
        named: set[str] = set()
        for kind, name, index, attrs in type_declarations(lines, masked):
            named |= mirror_targets(attrs)
            in_file.setdefault(path, {}).setdefault(name, []).append((kind, index))
            in_crate.setdefault(crate, {}).setdefault(name, []).append((path, kind, index))
        targets[path] = named
    # A target name is a type path the compiler resolves where the attribute
    # stands, so the declaration in the same file answers first. A crate that
    # holds its mirrors in child modules answers next, and nothing outside the
    # crate can be named without a path the attribute would spell.
    mirrors: set[tuple[Path, int]] = set()
    for path, named in targets.items():
        crate = relative_path(path).split("/")[1]
        for target in named:
            local = in_file.get(path, {}).get(target)
            if local is not None:
                mirrors.update((path, index) for _, index in local)
                continue
            for other, _, index in in_crate.get(crate, {}).get(target, []):
                mirrors.add((other, index))
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
                for member, line in mirror_members(lines, masked, kind, index):
                    if member_has_doc(lines, masked, line, index):
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
AUTHORING_PATH_TEXT_SUFFIXES = frozenset(
    {".rs", ".json", ".md", ".toml", ".txt", ".py", ".sh"}
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


def check_source() -> list[Finding]:
    sources = {
        path.resolve(): path.read_text(encoding="utf-8", errors="replace")
        for path in sorted(ROOT.glob("crates/**/*.rs"))
    }
    findings = scan_placement(sources)
    for path, source in sources.items():
        if is_production_rs(path):
            findings.extend(scan_patterns(path, source))
    findings.extend(scan_wire_mirror_docs(sources))
    findings.extend(scan_module_visibility(sources))
    findings.extend(scan_authoring_paths())
    findings.extend(scan_script_tests())
    return sorted(findings, key=lambda item: (item.path, item.line, item.rule))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="emit structured findings")
    args = parser.parse_args(argv)
    findings = check_source()
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
