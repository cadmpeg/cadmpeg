#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Count mechanical anti-patterns and fail when any count exceeds the ledger.

Patterns (production filter — see ``docs/convergence-ledger.toml``):

* ``from_le_bytes`` / ``from_be_bytes`` in ``crates/**/src`` (pressure: documented
  exceptions remain, so the honest end state is not zero)
* ``CodecError::Malformed(format!`` (multiline-aware)
* ``LossNote {`` struct literals (not ``-> LossNote {`` or the struct definition)
* bare scientific-notation tolerance values from ``1e-6`` through ``1e-12``
  at use sites in ``crates/**/src``; the numeric initializer of a named
  ``const`` or ``static`` is the declaration that gives the threshold its
  required intent
* non-literal ``vec![value; count]`` repeats (parsed-count allocations)

Modes:

* check (default): exit 1 when any count is above the ledger ceiling
* ``--update``: write new counts when every count is ≤ its ledger ceiling
* ``--json``: emit machine-readable counts (and check result) on stdout

A deliberate increase is a manual ledger edit: raise the ceiling and record a
reason under ``[reasons]`` for that key in the same commit. ``check`` compares
working-tree ceilings to ``HEAD:docs/convergence-ledger.toml`` and fails a
raise that has no reason.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs" / "convergence-ledger.toml"

LEGACY_METRIC_KEYS = (
    "from_endian_bytes",
    "codec_error_malformed_format",
    "loss_note_struct_literals",
    "bare_tolerance_literals",
    "nonliteral_vec_repeat",
)

PLACEMENT_KEYS = (
    "crate_root_tests_rs",
    "path_test_includes",
    "test_line_debt",
    "production_line_debt",
)

# Pressure keys have no zero destination (exceptions remain). Convergence keys
# have a [targets] completion criterion.
PRESSURE_KEYS = (
    "from_endian_bytes",
)
METRIC_KEYS = LEGACY_METRIC_KEYS + PLACEMENT_KEYS
CONVERGENCE_KEYS = tuple(key for key in METRIC_KEYS if key not in PRESSURE_KEYS)
TARGET_KEYS = CONVERGENCE_KEYS
KIND_BY_KEY = {
    key: ("pressure" if key in PRESSURE_KEYS else "convergence") for key in METRIC_KEYS
}
MEASURED_AT_SHA = re.compile(r"^[0-9a-f]{40}$")

FROM_ENDIAN = re.compile(r"\bfrom_(?:le|be)_bytes\b")
MALFORMED_FORMAT = re.compile(r"CodecError::Malformed\s*\(\s*format!", re.MULTILINE)
LOSS_NOTE_LIT = re.compile(r"LossNote\s*\{")
LOSS_NOTE_RETURN = re.compile(r"->\s*LossNote\s*\{")
LOSS_NOTE_STRUCT = re.compile(r"\b(?:pub(?:\([^)]*\))?\s+)?struct\s+LossNote\s*\{")
LOSS_NOTE_IMPL = re.compile(r"\bimpl(?:\s*<[^>]*>)?\s+LossNote\s*\{")
BARE_TOLERANCE = re.compile(
    r"(?<![0-9A-Za-z_.])1(?:\.0+)?[eE]-(?:6|7|8|9|10|11|12)\b"
)
NAMED_TOLERANCE_DECL = re.compile(
    r"^\s*(?:(?:pub(?:\([^)]*\))?|unsafe)\s+)*"
    r"(?:const|static)(?:\s+mut)?\s+[A-Za-z_][A-Za-z0-9_]*"
    r"\s*(?::[^=;]+)?=\s*"
)
# `vec![value; count]` where count is not a decimal/hex literal.
VEC_REPEAT = re.compile(r"vec!\s*\[(?:[^\];]|;)*;\s*([^\]]+)\]", re.MULTILINE)
VEC_REPEAT_LITERAL = re.compile(r"^(?:0x[0-9a-fA-F]+|\d+)$")
CFG_TEST_ATTR = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")
CFG_ATTR = re.compile(r"#\s*\[\s*cfg\s*\((.*)\)\s*\]\s*$", re.DOTALL)
PATH_ATTR = re.compile(r'#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]\s*$', re.DOTALL)
MOD_DECL = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*(;|\{)"
)

FILTER_DESCRIPTION = (
    "legacy metrics: production .rs under crates/**/src via is_production_rs; "
    "exclude tests/ and benches/ path segments; exclude files named tests.rs or "
    "*test*.rs; lexically mask Rust comments and literals, then strip "
    "cfg(test)-attributed items with blank-preserving elision. "
    "bare tolerance literals count at use sites; named const/static numeric "
    "initializers are declarations and are excluded. "
    "from_endian_bytes uses that same crates/**/src glob (not codec crates only). "
    "Placement metrics: scan crates/**/*.rs by ownership, structural entry "
    "points, standard mod resolution, and test-only #[path] ancestry; "
    "golden_tests files stay out of test-line debt. production_line_debt "
    "reuses is_production_rs and elides cfg(test) items without blank placeholders. "
    "[kinds] marks each key pressure (no zero destination) or convergence "
    "([targets] is the completion criterion)."
)


def is_production_rs(path: Path) -> bool:
    """True when ``path`` is a production ``.rs`` file under the filter."""
    if path.suffix != ".rs":
        return False
    parts = path.parts
    if "tests" in parts or "benches" in parts:
        return False
    name = path.name
    if name == "tests.rs" or re.search(r"test", name, re.IGNORECASE):
        return False
    return "src" in parts


def strip_cfg_test_items(text: str, masked_text: str | None = None) -> str:
    """Remove ``#[cfg(test)]``-attributed items and their bodies when practical."""
    lines = text.splitlines(keepends=True)
    masked = (
        masked_text.splitlines(keepends=True)
        if masked_text is not None
        else masked_lines(lines)
    )
    out: list[str] = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if CFG_TEST_ATTR.search(masked[i]):
            out.append("\n" if line.endswith("\n") else "")
            i += 1
            while i < len(lines):
                stripped = lines[i].lstrip()
                if (
                    stripped.startswith("#[")
                    or stripped.startswith("//!")
                    or stripped.startswith("///")
                ):
                    out.append("\n" if lines[i].endswith("\n") else "")
                    i += 1
                    continue
                break
            if i >= len(lines):
                break
            item = lines[i]
            if "{" not in masked[i]:
                out.append("\n" if item.endswith("\n") else "")
                i += 1
                continue
            depth = 0
            while i < len(lines):
                for ch in masked[i]:
                    if ch == "{":
                        depth += 1
                    elif ch == "}":
                        depth -= 1
                out.append("\n" if lines[i].endswith("\n") else "")
                i += 1
                if depth <= 0:
                    break
            continue
        out.append(line)
        i += 1
    return "".join(out)


def elide_cfg_test_items(text: str) -> str:
    """Remove cfg(test) items and their bodies without leaving blank lines."""
    lines = text.splitlines(keepends=True)
    masked = masked_lines(lines)
    out: list[str] = []
    i = 0
    while i < len(lines):
        stripped = lines[i].lstrip()
        if not stripped.startswith("#["):
            out.append(lines[i])
            i += 1
            continue
        attrs: list[str] = []
        start = i
        while i < len(lines):
            stripped = lines[i].lstrip()
            if stripped.startswith("#["):
                attr, i = collect_attribute(lines, i, masked)
                attrs.append(attr)
                continue
            if is_trivia_line(stripped):
                i += 1
                continue
            break
        if not any(attr_is_test_cfg(attr) for attr in attrs):
            out.extend(lines[start:i])
            continue
        if i >= len(lines):
            break
        i = skip_item(lines, i, masked)
    return "".join(out)


def iter_src_files(glob: str) -> list[Path]:
    return sorted(p for p in ROOT.glob(glob) if is_production_rs(p))


def metric_source_text(path: Path) -> str:
    """Return production Rust code with test-only items and non-code masked."""

    masked = mask_rust_non_code(path.read_text(encoding="utf-8", errors="replace"))
    return strip_cfg_test_items(masked, masked)


def count_legacy_metrics() -> dict[str, int]:
    counts = {
        "from_endian_bytes": 0,
        "codec_error_malformed_format": 0,
        "loss_note_struct_literals": 0,
        "bare_tolerance_literals": 0,
        "nonliteral_vec_repeat": 0,
    }
    for path in iter_src_files("crates/**/src/**/*.rs"):
        text = metric_source_text(path)
        counts["from_endian_bytes"] += len(FROM_ENDIAN.findall(text))
        counts["codec_error_malformed_format"] += len(MALFORMED_FORMAT.findall(text))
        for line in text.splitlines():
            if not LOSS_NOTE_LIT.search(line):
                continue
            if (
                LOSS_NOTE_RETURN.search(line)
                or LOSS_NOTE_STRUCT.search(line)
                or LOSS_NOTE_IMPL.search(line)
            ):
                continue
            counts["loss_note_struct_literals"] += 1
        counts["bare_tolerance_literals"] += count_bare_tolerance_literals(text)
        for match in VEC_REPEAT.finditer(text):
            if not VEC_REPEAT_LITERAL.fullmatch(match.group(1).strip()):
                counts["nonliteral_vec_repeat"] += 1
    return counts


def count_from_endian_bytes() -> int:
    total = 0
    for path in iter_src_files("crates/**/src/**/*.rs"):
        text = metric_source_text(path)
        total += len(FROM_ENDIAN.findall(text))
    return total


def count_malformed_format() -> int:
    total = 0
    for path in iter_src_files("crates/**/src/**/*.rs"):
        text = metric_source_text(path)
        total += len(MALFORMED_FORMAT.findall(text))
    return total


def count_loss_note_literals() -> int:
    total = 0
    for path in iter_src_files("crates/**/src/**/*.rs"):
        text = metric_source_text(path)
        for line in text.splitlines():
            if not LOSS_NOTE_LIT.search(line):
                continue
            if (
                LOSS_NOTE_RETURN.search(line)
                or LOSS_NOTE_STRUCT.search(line)
                or LOSS_NOTE_IMPL.search(line)
            ):
                continue
            total += 1
    return total


def count_bare_tolerances() -> int:
    total = 0
    for path in iter_src_files("crates/**/src/**/*.rs"):
        text = metric_source_text(path)
        total += count_bare_tolerance_literals(text)
    return total


def count_bare_tolerance_literals(text: str) -> int:
    """Count tolerance literals that are not named threshold definitions."""
    total = 0
    for line in text.splitlines():
        for match in BARE_TOLERANCE.finditer(line):
            if NAMED_TOLERANCE_DECL.match(line[: match.start()]):
                continue
            total += 1
    return total


def count_nonliteral_vec_repeat() -> int:
    total = 0
    for path in iter_src_files("crates/**/src/**/*.rs"):
        text = metric_source_text(path)
        for match in VEC_REPEAT.finditer(text):
            count_expr = match.group(1).strip()
            if VEC_REPEAT_LITERAL.fullmatch(count_expr):
                continue
            total += 1
    return total


def relative_path(path: Path) -> str:
    return path.resolve().relative_to(ROOT.resolve()).as_posix()


def line_count(data: str) -> int:
    if not data:
        return 0
    return data.count("\n") + (0 if data.endswith("\n") else 1)


def debt_over_limit(lines: int, limit: int) -> int:
    return max(0, lines - limit)


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
    """Blank comments and literals while preserving source positions.

    The legacy counters inspect Rust source text with regular expressions. A
    lexical mask keeps those expressions from treating a tolerance in a
    comment, string, or raw string as a Rust expression. Newlines are retained
    so line-oriented diagnostics remain useful.
    """

    def blank(match: re.Match[str]) -> str:
        return "".join(
            character if character in "\r\n" else " " for character in match.group(0)
        )

    return RUST_NON_CODE.sub(blank, text)


def masked_lines(lines: list[str]) -> list[str]:
    """Return a lexical mask with the same line boundaries as ``lines``."""

    return mask_rust_non_code("".join(lines)).splitlines(keepends=True)


def collect_attribute(
    lines: list[str], start: int, masked: list[str] | None = None
) -> tuple[str, int]:
    depth = 0
    pieces: list[str] = []
    masked = masked or [mask_rust_non_code(lines[start])]
    i = start
    while i < len(lines):
        line = lines[i]
        pieces.append(line)
        masked_line = masked[i] if i < len(masked) else mask_rust_non_code(line)
        for ch in masked_line:
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
    body = re.sub(r'"(?:\\.|[^"\\])*"', '""', match.group(1))
    return re.search(r"(?<![\w:])test(?![\w:])", body) is not None


def path_attr_target(attr: str) -> str | None:
    match = PATH_ATTR.match(attr.strip())
    return match.group(1) if match is not None else None


def skip_item(
    lines: list[str], start: int, masked: list[str] | None = None
) -> int:
    saw_brace = False
    depth = 0
    masked = masked or masked_lines(lines)
    i = start
    while i < len(lines):
        for ch in masked[i]:
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


def find_matching_brace_end(
    lines: list[str], start: int, masked: list[str] | None = None
) -> int:
    saw_brace = False
    depth = 0
    masked = masked or masked_lines(lines)
    for i in range(start, len(lines)):
        for ch in masked[i]:
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


def empty_contributors() -> dict[str, list[dict[str, object]]]:
    return {key: [] for key in PLACEMENT_KEYS}


def scan_block(
    text: str,
    file_path: Path,
    child_dir: Path,
    file_test_only: bool,
    parent_module_test_only: bool,
    inside_counted_inline: bool,
    module_prefix: str,
    contributors: dict[str, list[dict[str, object]]],
    scanned_test_files: set[Path],
    scanned_prod_files: set[Path],
    known_test_files: set[Path],
    counted_test_files: set[Path],
    masked_text: str | None = None,
) -> None:
    lines = text.splitlines(keepends=True)
    masked = (
        masked_text.splitlines(keepends=True)
        if masked_text is not None
        else masked_lines(lines)
    )
    i = 0
    pending_attrs: list[str] = []
    pending_start = 0
    while i < len(lines):
        stripped = lines[i].lstrip()
        if stripped.startswith("#["):
            if not pending_attrs:
                pending_start = i
            attr, i = collect_attribute(lines, i, masked)
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
        module_has_test_cfg = False
        for attr in attrs:
            explicit_path = path_attr_target(attr) or explicit_path
            module_has_test_cfg = module_has_test_cfg or attr_is_test_cfg(attr)
        module_test_only = (
            file_test_only or parent_module_test_only or module_has_test_cfg
        )
        if explicit_path is not None and module_test_only:
            contributors["path_test_includes"].append(
                {
                    "path": relative_path(file_path),
                    "target": explicit_path,
                    "debt": 1,
                }
            )
        if marker == ";":
            target = resolve_module_target(file_path, child_dir, module_name, explicit_path)
            if target is not None:
                target_kind = structural_test_kind(target)
                if module_test_only or target_kind is not None:
                    scan_test_file(
                        target,
                        contributors,
                        scanned_test_files,
                        scanned_prod_files,
                        known_test_files,
                        counted_test_files,
                    )
            i += 1
            continue
        end = find_matching_brace_end(lines, i, masked)
        block_text = "".join(lines[i : end + 1])
        masked_block_text = "".join(masked[i : end + 1])
        open_index = masked_block_text.find("{")
        close_index = masked_block_text.rfind("}")
        body = (
            block_text[open_index + 1 : close_index]
            if open_index >= 0 and close_index > open_index
            else ""
        )
        nested_inside_counted = inside_counted_inline
        if module_test_only and not file_test_only and not inside_counted_inline:
            inline_text = "".join(lines[attr_start : end + 1])
            lines_in_module = line_count(inline_text)
            debt = debt_over_limit(lines_in_module, 2000)
            if debt > 0:
                contributors["test_line_debt"].append(
                    {
                        "path": f"{relative_path(file_path)}::{module_prefix}{module_name}",
                        "lines": lines_in_module,
                        "debt": debt,
                    }
                )
            nested_inside_counted = True
        scan_block(
            body,
            file_path,
            child_dir / module_name,
            file_test_only,
            module_test_only,
            nested_inside_counted,
            f"{module_prefix}{module_name}::",
            contributors,
            scanned_test_files,
            scanned_prod_files,
            known_test_files,
            counted_test_files,
            masked_block_text[open_index + 1 : close_index]
            if open_index >= 0 and close_index > open_index
            else "",
        )
        i = end + 1


def scan_test_file(
    path: Path,
    contributors: dict[str, list[dict[str, object]]],
    scanned_test_files: set[Path],
    scanned_prod_files: set[Path],
    known_test_files: set[Path],
    counted_test_files: set[Path],
) -> None:
    path = path.resolve()
    if path in scanned_test_files:
        return
    scanned_test_files.add(path)
    known_test_files.add(path)
    text = path.read_text(encoding="utf-8", errors="replace")
    if path not in counted_test_files and structural_test_kind(path) != "golden":
        counted_test_files.add(path)
        lines_in_file = line_count(text)
        debt = debt_over_limit(lines_in_file, 2000)
        if debt > 0:
            contributors["test_line_debt"].append(
                {
                    "path": relative_path(path),
                    "lines": lines_in_file,
                    "debt": debt,
                }
            )
    scan_block(
        text,
        path,
        child_module_dir(path),
        True,
        False,
        False,
        "",
        contributors,
        scanned_test_files,
        scanned_prod_files,
        known_test_files,
        counted_test_files,
    )


def collect_placement_contributors() -> dict[str, list[dict[str, object]]]:
    contributors = empty_contributors()
    all_rs = sorted(ROOT.glob("crates/**/*.rs"))
    for path in all_rs:
        if is_crate_root_tests_rs(path):
            contributors["crate_root_tests_rs"].append(
                {"path": relative_path(path), "debt": 1}
            )
    scanned_test_files: set[Path] = set()
    scanned_prod_files: set[Path] = set()
    known_test_files: set[Path] = set()
    counted_test_files: set[Path] = set()
    for path in all_rs:
        if structural_test_kind(path) is not None:
            scan_test_file(
                path,
                contributors,
                scanned_test_files,
                scanned_prod_files,
                known_test_files,
                counted_test_files,
            )
    for path in all_rs:
        if not is_production_rs(path) or path.resolve() in known_test_files:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        scanned_prod_files.add(path.resolve())
        scan_block(
            text,
            path.resolve(),
            child_module_dir(path.resolve()),
            False,
            False,
            False,
            "",
            contributors,
            scanned_test_files,
            scanned_prod_files,
            known_test_files,
            counted_test_files,
        )
    for path in iter_src_files("crates/**/*.rs"):
        text = elide_cfg_test_items(path.read_text(encoding="utf-8", errors="replace"))
        lines_in_file = line_count(text)
        debt = debt_over_limit(lines_in_file, 10000)
        if debt > 0:
            contributors["production_line_debt"].append(
                {
                    "path": relative_path(path),
                    "lines": lines_in_file,
                    "debt": debt,
                }
            )
    for key in PLACEMENT_KEYS:
        contributors[key].sort(
            key=lambda item: (str(item["path"]), str(item.get("target", "")))
        )
    return contributors


def measure_all() -> tuple[dict[str, int], dict[str, list[dict[str, object]]]]:
    counts = count_legacy_metrics()
    contributors = collect_placement_contributors()
    counts.update(
        {
            "crate_root_tests_rs": len(contributors["crate_root_tests_rs"]),
            "path_test_includes": len(contributors["path_test_includes"]),
            "test_line_debt": sum(
                int(item["debt"]) for item in contributors["test_line_debt"]
            ),
            "production_line_debt": sum(
                int(item["debt"]) for item in contributors["production_line_debt"]
            ),
        }
    )
    return counts, contributors


def measure() -> dict[str, int]:
    counts, _ = measure_all()
    return counts


def strip_toml_comment(raw: str) -> str:
    """Strip TOML comments while preserving ``#`` inside quoted strings."""
    in_string = False
    escaped = False
    out: list[str] = []
    for ch in raw:
        if escaped:
            out.append(ch)
            escaped = False
            continue
        if ch == "\\" and in_string:
            out.append(ch)
            escaped = True
            continue
        if ch == '"':
            out.append(ch)
            in_string = not in_string
            continue
        if ch == "#" and not in_string:
            break
        out.append(ch)
    return "".join(out)


def parse_ledger_text(text: str) -> dict[str, object]:
    """Minimal TOML reader for the ledger shape used here."""
    data: dict[str, object] = {
        "targets": {},
        "ceilings": {},
        "reasons": {},
        "kinds": {},
    }
    section: str | None = None
    for raw in text.splitlines():
        line = strip_toml_comment(raw).strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1].strip()
            continue
        if "=" not in line:
            continue
        key, _, value = line.partition("=")
        key = key.strip()
        value = value.strip()
        if value.startswith('"') and value.endswith('"'):
            parsed: object = (
                value[1:-1].replace('\\"', '"').replace("\\\\", "\\")
            )
        else:
            parsed = int(value)
        if section is None:
            data[key] = parsed
        elif section == "ceilings":
            cast = data["ceilings"]
            assert isinstance(cast, dict)
            cast[key] = parsed
        elif section == "targets":
            cast = data["targets"]
            assert isinstance(cast, dict)
            cast[key] = parsed
        elif section == "reasons":
            cast = data["reasons"]
            assert isinstance(cast, dict)
            cast[key] = parsed
        elif section == "kinds":
            cast = data["kinds"]
            assert isinstance(cast, dict)
            cast[key] = parsed
    return data


def parse_ledger(path: Path) -> dict[str, object]:
    return parse_ledger_text(path.read_text(encoding="utf-8"))


def head_ledger() -> dict[str, object] | None:
    """Parse the committed ledger, or None when HEAD has no copy."""
    try:
        raw = subprocess.check_output(
            ["git", "show", "HEAD:docs/convergence-ledger.toml"],
            cwd=ROOT,
            stderr=subprocess.DEVNULL,
        )
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    return parse_ledger_text(raw.decode())


def render_ledger(
    measured_at: str,
    filter_description: str,
    targets: dict[str, int],
    ceilings: dict[str, int],
    reasons: dict[str, str],
) -> str:
    lines = [
        "# Convergence ratchet ceilings. Counts may only fall.",
        "# A decrease updates this file in the same commit. A deliberate increase",
        "# requires a reason under [reasons] for every raised key.",
        "# [kinds] is pressure (no zero destination) or convergence ([targets]).",
        f'measured_at = "{measured_at}"',
        f'filter = "{filter_description}"',
        "",
        "[kinds]",
    ]
    for key in METRIC_KEYS:
        lines.append(f'{key} = "{KIND_BY_KEY[key]}"')
    lines.extend(["", "[targets]"])
    for key in TARGET_KEYS:
        lines.append(f"{key} = {targets[key]}")
    lines.extend(["", "[ceilings]"])
    for key in METRIC_KEYS:
        lines.append(f"{key} = {ceilings[key]}")
    if reasons:
        lines.append("")
        lines.append("[reasons]")
        for key in METRIC_KEYS:
            if key in reasons:
                escaped = reasons[key].replace("\\", "\\\\").replace('"', '\\"')
                lines.append(f'{key} = "{escaped}"')
    lines.append("")
    return "\n".join(lines)


def git_head() -> str:
    return (
        subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT)
        .decode()
        .strip()
    )


def check(
    counts: dict[str, int],
    ceilings: dict[str, int],
    targets: dict[str, int],
    kinds: dict[str, str] | None = None,
    reasons: dict[str, str] | None = None,
    previous_ceilings: dict[str, int] | None = None,
    measured_at: object | None = None,
) -> list[str]:
    failures: list[str] = []
    if measured_at is not None and not (
        isinstance(measured_at, str) and MEASURED_AT_SHA.fullmatch(measured_at)
    ):
        failures.append("ledger measured_at is not a 40-char git SHA")
    for key in METRIC_KEYS:
        if key not in ceilings:
            failures.append(f"ledger missing ceiling for {key}")
            continue
        ceiling = ceilings[key]
        value = counts[key]
        if value > ceiling:
            failures.append(f"{key}: {value} > ledger {ceiling}")
    for key in TARGET_KEYS:
        if key not in targets:
            failures.append(f"ledger missing target for {key}")
    for key in PRESSURE_KEYS:
        if key in targets:
            failures.append(f"{key}: pressure key must not have a [targets] entry")
    if kinds is not None:
        for key in METRIC_KEYS:
            kind = kinds.get(key)
            expected = KIND_BY_KEY[key]
            if kind is None:
                failures.append(f"ledger missing kind for {key}")
            elif kind != expected:
                failures.append(f"{key}: kind {kind!r} != {expected!r}")
    if previous_ceilings is not None:
        recorded = reasons or {}
        for key in METRIC_KEYS:
            new = ceilings.get(key)
            if new is None:
                continue
            old = previous_ceilings.get(key)
            if old is None or new > old:
                if not str(recorded.get(key, "")).strip():
                    failures.append(f"{key}: ceiling raised without [reasons].{key}")
    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--update",
        action="store_true",
        help="rewrite the ledger when every count is ≤ its ceiling",
    )
    parser.add_argument("--json", action="store_true", help="emit JSON on stdout")
    args = parser.parse_args(argv)

    if not LEDGER.is_file():
        print(f"error: missing ledger {LEDGER}", file=sys.stderr)
        return 2

    ledger = parse_ledger(LEDGER)
    targets_raw = ledger.get("targets")
    if not isinstance(targets_raw, dict):
        print("error: ledger has no [targets] table", file=sys.stderr)
        return 2
    targets = {str(k): int(v) for k, v in targets_raw.items()}  # type: ignore[arg-type]
    ceilings_raw = ledger.get("ceilings")
    if not isinstance(ceilings_raw, dict):
        print("error: ledger has no [ceilings] table", file=sys.stderr)
        return 2
    ceilings = {str(k): int(v) for k, v in ceilings_raw.items()}  # type: ignore[arg-type]
    reasons_raw = ledger.get("reasons")
    reasons = (
        {str(k): str(v) for k, v in reasons_raw.items()}
        if isinstance(reasons_raw, dict)
        else {}
    )
    kinds_raw = ledger.get("kinds")
    kinds = (
        {str(k): str(v) for k, v in kinds_raw.items()}
        if isinstance(kinds_raw, dict)
        else {}
    )
    previous = head_ledger()
    previous_ceilings: dict[str, int] | None = None
    if previous is not None:
        previous_raw = previous.get("ceilings")
        if isinstance(previous_raw, dict):
            previous_ceilings = {
                str(k): int(v) for k, v in previous_raw.items()  # type: ignore[arg-type]
            }

    counts, contributors = measure_all()
    failures = check(
        counts,
        ceilings,
        targets,
        kinds=kinds,
        reasons=reasons,
        previous_ceilings=previous_ceilings,
        measured_at=ledger.get("measured_at"),
    )

    if args.update:
        if failures:
            print(
                "error: --update refuses increases; raise the ceiling and add "
                "[reasons] manually for a deliberate increase",
                file=sys.stderr,
            )
            for failure in failures:
                print(f"error: {failure}", file=sys.stderr)
            return 1
        # Drop reasons for keys that are no longer above any prior justification
        # need — keep only keys still listed whose ceiling was previously raised.
        kept_reasons = {
            key: reasons[key]
            for key in METRIC_KEYS
            if key in reasons and counts[key] >= ceilings.get(key, counts[key])
        }
        # After a decrease, clear reasons that applied to the old higher ceiling.
        kept_reasons = {
            key: reason
            for key, reason in kept_reasons.items()
            if counts[key] == ceilings.get(key)
        }
        LEDGER.write_text(
            render_ledger(git_head(), FILTER_DESCRIPTION, targets, counts, kept_reasons),
            encoding="utf-8",
        )
        if args.json:
            print(
                json.dumps(
                    {
                        "status": "updated",
                        "counts": counts,
                        "ceilings": counts,
                        "previous_ceilings": ceilings,
                        "targets": targets,
                        "failures": [],
                        "contributors": contributors,
                    },
                    indent=2,
                    sort_keys=True,
                )
            )
        else:
            for key in METRIC_KEYS:
                if key in LEGACY_METRIC_KEYS:
                    prev = ceilings.get(key)
                    marker = "" if prev == counts[key] else f" (was {prev})"
                    print(f"{key}\t{counts[key]}{marker}")
                    continue
                prev = ceilings.get(key)
                marker = "" if prev == counts[key] else f" (was {prev})"
                print(
                    f"{key}\t{counts[key]}\t(ceiling {counts[key]}, target {targets[key]}){marker}"
                )
                for item in contributors[key]:
                    if key in {"test_line_debt", "production_line_debt"}:
                        print(
                            f"  {item['path']}\tdebt {item['debt']}\tlines {item['lines']}"
                        )
                    elif key == "path_test_includes":
                        print(
                            f"  {item['path']}\tdebt {item['debt']}\ttarget {item['target']}"
                        )
                    else:
                        print(f"  {item['path']}\tdebt {item['debt']}")
            print(f"updated {LEDGER.relative_to(ROOT)}", file=sys.stderr)
        return 0

    status = "ok" if not failures else "fail"
    if args.json:
        print(
            json.dumps(
                {
                    "status": status,
                    "counts": counts,
                    "ceilings": ceilings,
                    "targets": targets,
                    "failures": failures,
                    "contributors": contributors,
                },
                indent=2,
                sort_keys=True,
            )
        )
    else:
        for key in LEGACY_METRIC_KEYS:
            print(f"{key}\t{counts[key]}\t(ceiling {ceilings.get(key, '?')})")
        for key in PLACEMENT_KEYS:
            print(
                f"{key}\t{counts[key]}\t(ceiling {ceilings.get(key, '?')}, target {targets.get(key, '?')})"
            )
            for item in contributors[key]:
                if key in {"test_line_debt", "production_line_debt"}:
                    print(
                        f"  {item['path']}\tdebt {item['debt']}\tlines {item['lines']}"
                    )
                elif key == "path_test_includes":
                    print(
                        f"  {item['path']}\tdebt {item['debt']}\ttarget {item['target']}"
                    )
                else:
                    print(f"  {item['path']}\tdebt {item['debt']}")
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
