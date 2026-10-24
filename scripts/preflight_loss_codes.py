# SPDX-License-Identifier: Apache-2.0
"""A new loss code has a test that the decoder or encoder emits it.

A new drop path adds a `*LossCode` variant in the codec's `src/loss.rs`
(CLAUDE.md). The pinned-string test in `loss.rs` proves the spelling, not
that any decode or export path emits the code. This check warns when a
variant the branch adds is named by no test outside `loss.rs`. A test
counts when it sits in a test file (`tests.rs`, `tests/`) or inside a
`#[cfg(test)]` module or `#[test]` function of any source file of the crate;
a mention in comments or ordinary code does not.
"""

from __future__ import annotations

import re
from pathlib import PurePosixPath

from preflight import crate_of, is_test_path

NAME = "loss-codes"
ORDER = 85

LOSS = re.compile(r"^crates/[^/]+/src/loss\.rs$")
VARIANT = re.compile(r"^\s{4}([A-Z][A-Za-z0-9]+)\s*(?:[,({]|$)")
TEST_ATTRIBUTE = re.compile(r"#\[\s*(?:cfg\s*\(\s*test\s*\)|test)\s*\]")
ATTRIBUTE = re.compile(r"\s*#\[[^\]]*\]")
RAW_STRING = re.compile(r'b?r(#*)"')
CHAR = re.compile(r"'(?:\\.[^']*|[^\\'])'")
TEST_ITEM = re.compile(
    r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:(?:async|const|unsafe)\s+)*(?:mod|fn)\b")


def blank(text: str, strings: bool) -> str:
    """The text with comments (and string literals when asked) blanked out.

    Offsets and newlines are kept so spans line up with the original."""
    out = list(text)
    index, length = 0, len(text)

    def clear(start: int, end: int) -> None:
        for at in range(start, min(end, length)):
            if out[at] != "\n":
                out[at] = " "

    while index < length:
        if text.startswith("//", index):
            end = text.find("\n", index)
            end = length if end < 0 else end
            clear(index, end)
            index = end
        elif text.startswith("/*", index):
            depth, at = 1, index + 2
            while at < length and depth:
                if text.startswith("/*", at):
                    depth, at = depth + 1, at + 2
                elif text.startswith("*/", at):
                    depth, at = depth - 1, at + 2
                else:
                    at += 1
            clear(index, at)
            index = at
        elif (raw := RAW_STRING.match(text, index)) and (
                index == 0 or not (text[index - 1].isalnum() or text[index - 1] == "_")):
            close = '"' + raw.group(1)
            end = text.find(close, raw.end())
            end = length if end < 0 else end + len(close)
            if strings:
                clear(index + 1, end - 1)
            index = end
        elif text[index] == '"':
            at = index + 1
            while at < length and text[at] != '"':
                at += 2 if text[at] == "\\" else 1
            if strings:
                clear(index + 1, at)
            index = at + 1
        elif text[index] == "'" and (char := CHAR.match(text, index)):
            if strings:
                clear(index + 1, char.end() - 1)
            index = char.end()
        else:
            index += 1
    return "".join(out)


def test_spans(text: str) -> list[tuple[int, int]]:
    """Byte ranges of the `#[cfg(test)]` modules and `#[test]` functions."""
    code = blank(text, strings=True)
    spans = []
    for attribute in TEST_ATTRIBUTE.finditer(code):
        at = attribute.end()
        while (other := ATTRIBUTE.match(code, at)):
            at = other.end()
        if not TEST_ITEM.match(code, at):
            continue
        brace, semicolon = code.find("{", at), code.find(";", at)
        if brace < 0 or 0 <= semicolon < brace:
            continue
        depth, end = 0, brace
        while end < len(code):
            if code[end] == "{":
                depth += 1
            elif code[end] == "}":
                depth -= 1
                if depth == 0:
                    break
            end += 1
        spans.append((brace, end + 1))
    return spans


def named_in_test_code(text: str, name: str) -> bool:
    """Whether the name appears as a word inside an inline test item."""
    spans = test_spans(text)
    if not spans:
        return False
    code = blank(text, strings=False)
    word = re.compile(rf"(?<![A-Za-z0-9_]){re.escape(name)}(?![A-Za-z0-9_])")
    return any(start <= match.start() < end
               for match in word.finditer(code) for start, end in spans)


def added_variants(context, path: str) -> list[str]:
    return [match.group(1) for _, text in context.added_lines(path)
            if (match := VARIANT.match(text))]


def mentioned_in_tests(context, crate: str, name: str) -> bool:
    arguments = ["grep", "-l", "-w", "-F", "-e", name]
    if context.rev:
        arguments.append(context.rev)
    else:
        arguments.append("--untracked")
    arguments += ["--", f"crates/{crate}/"]
    for line in context.git(*arguments).stdout.splitlines():
        path = line.split(":", 1)[1] if context.rev else line
        parts = PurePosixPath(path).parts
        if "loss" in parts or path.endswith("/loss.rs"):
            continue
        if is_test_path(path) or "tests" in PurePosixPath(path).stem:
            return True
        if path.endswith(".rs") and named_in_test_code(context.read(path) or "", name):
            return True
    return False


def run(context):
    untested = []
    for path in context.changed_paths():
        if not LOSS.match(path):
            continue
        crate = crate_of(path)
        for name in added_variants(context, path):
            if not mentioned_in_tests(context, crate, name):
                untested.append(f"{path}: {name}")
    if not untested:
        return context.passed("every added loss code is named by a test")
    return context.warned(
        f"{len(untested)} added loss code(s) have no emission test",
        *untested,
        "fix: add a test that decodes or exports an input and asserts the code is emitted",
    )
