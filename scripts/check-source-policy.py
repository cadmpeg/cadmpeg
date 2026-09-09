#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check current source against repository policy; no Git history or ledger is needed.

Reports rule, file, line, and explanation. Exit 1 means violations were found.
Use --json for structured findings. See docs/source-policy.md for scope and limits.
"""

from __future__ import annotations

import argparse
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
LOSS_NOTE_LIT = re.compile(r"LossNote\s*\{")
LOSS_NOTE_RETURN = re.compile(r"->\s*LossNote\s*\{")
LOSS_NOTE_STRUCT = re.compile(r"\b(?:pub(?:\([^)]*\))?\s+)?struct\s+LossNote\s*\{")
LOSS_NOTE_IMPL = re.compile(r"\bimpl(?:<[^>]*>)?\s+LossNote\s*\{")
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


def production_source(source: str) -> tuple[str, int]:
    """Mask non-code and test-only items; return code and production line count."""
    lines = mask_rust_non_code(source).splitlines(keepends=True)
    production_lines = len(lines)
    i = 0
    while i < len(lines):
        if not lines[i].lstrip().startswith("#["):
            i += 1
            continue
        attrs = []
        start = i
        while i < len(lines):
            stripped = lines[i].lstrip()
            if stripped.startswith("#["):
                attr, i = collect_attribute(lines, i)
                attrs.append(attr)
            elif not stripped.strip():
                i += 1
            else:
                break
        if not any(attr_is_test_cfg(attr) for attr in attrs):
            continue
        i = skip_item(lines, i)
        production_lines -= i - start
        for index in range(start, i):
            lines[index] = re.sub(r"[^\r\n]", " ", lines[index])
    return "".join(lines), production_lines


RUST_NON_CODE = re.compile(
    r"//[^\r\n]*"
    r"|/\*(?:[^*]|\*(?!/))*\*/"
    r'|(?:br|r)(?P<raw_hashes>#+)"(?:(?!"(?P=raw_hashes)).)*"(?P=raw_hashes)'
    r'|(?:br|r)"(?:\\.|[^"\\])*"'
    r'|"(?:\\.|[^"\\])*"'
    r"|'(?:\\.|[^'\\\r\n])*'",
    re.DOTALL,
)


def mask_rust_non_code(text: str) -> str:
    """Blank comments and literals while preserving positions and newlines."""

    def blank(match: re.Match[str]) -> str:
        return "".join(
            character if character in "\r\n" else " " for character in match.group(0)
        )

    return RUST_NON_CODE.sub(blank, text)


ENDIAN_EXCEPTIONS = {"reconstructed-scalar", "packed-color-order"}
ENDIAN_MARKER = re.compile(r"^\s*// endian-exception: ([a-z-]+)\s*$")


def endian_markers(source: str) -> dict[int, str]:
    """Read standalone line comments, excluding lookalikes inside Rust literals."""
    markers = {}
    for token in RUST_NON_CODE.finditer(source):
        marker = ENDIAN_MARKER.fullmatch(token[0])
        if marker is None:
            continue
        start = source.rfind("\n", 0, token.start()) + 1
        if not source[start:token.start()].strip():
            markers[source.count("\n", 0, token.start())] = marker[1]
    return markers


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
    i = start
    while i < len(lines):
        for ch in lines[i]:
            if ch == "{":
                depth += 1
                saw_brace = True
            elif ch == "}":
                if saw_brace:
                    depth -= 1
            elif ch == ";" and not saw_brace:
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
        if LOSS_NOTE_LIT.search(line) and not any(
            pattern.search(line) for pattern in (LOSS_NOTE_RETURN, LOSS_NOTE_STRUCT, LOSS_NOTE_IMPL)
        ):
            report("loss_note_literal", index + 1, "Construct loss notes through the owning loss code's note method.")

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


def check_source() -> list[Finding]:
    sources = {
        path.resolve(): path.read_text(encoding="utf-8", errors="replace")
        for path in sorted(ROOT.glob("crates/**/*.rs"))
    }
    findings = scan_placement(sources)
    for path, source in sources.items():
        if is_production_rs(path):
            findings.extend(scan_patterns(path, source))
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
