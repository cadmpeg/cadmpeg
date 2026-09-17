#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check that every non-test deserializable item refuses an unknown wire key.

A document read must not silently accept a key no type declares. The golden
sweeps test shapes the goldens carry; this checker follows declared
read routes in the three wire crates. Explicit lexical imports are resolved
before applying the unique-declaration fallback for bare names; this is not a
complete Rust name-resolution proof. Visible unresolved wildcards and cyclic
bindings fail the static check. Re-export traversal remains a static limit.
Declarations without attributes are included when following a read route.
Nongeneric type aliases are followed to their targets; other alias shapes
fail the static check when used as a read route.

A second, independent rule runs over every crate: an optional key whose writer
omits it for ``None`` (``skip_serializing_if = "Option::is_none"``) must read
through a reader declared by ``cadmpeg_core::named_optional_field!``, so an
absent key is that key's one spelling of ``None``, ``null`` is refused, and the
refusal names the key it refuses. A key that states neither the ``default`` nor
such a reader is an undeclared key and is named here.

An item that derives ``Deserialize`` passes when its serde attributes state one
of:

* ``deny_unknown_fields`` - it refuses the container's own keys, and every
  tuple payload reader that can receive an object is checked as well;
* ``try_from = "T"`` or ``from = "T"`` - the read goes through ``T``, which is
  checked in turn;
* ``transparent`` - the read is the inner type's read, with no key of its own;
* ``untagged`` - unit arms read null, tuple fields are checked in turn, and
  inline struct arms require the container's ``deny_unknown_fields``. This
  also holds for an arm carrying ``#[serde(untagged)]`` inside a tagged enum;
* every variant is a unit variant without an internal or adjacent tag - the
  read is a bare name with no key to deny;
* the enum has no variant at all - it is uninhabited, no document can name a
  variant of it, and serde refuses every input before a key is read.

Anything else fails, unless it is one of the named exceptions below.
Only actual unconditional metadata supplies refusal evidence. Conditional
reader replacements require separate proof and fail this static check.

Exit code 0 prints ``deny census: ok``; any failure prints one
``file:line type`` line per offending item and exits 1.
"""

from __future__ import annotations

import importlib.util
from bisect import bisect_right
import json
import os
import re
import stat
import sys
from pathlib import Path

# Share the source-policy lexer so comments, literals and cfg(test) bodies do
# not create declarations or alter lexical scope in this census.
_SPEC = importlib.util.spec_from_file_location(
    "deny_source_policy", Path(__file__).with_name("check-source-policy.py")
)
if _SPEC is None or _SPEC.loader is None:
    raise SystemExit("cannot load the source-policy lexer")
SOURCE_POLICY = importlib.util.module_from_spec(_SPEC)
sys.modules[_SPEC.name] = SOURCE_POLICY
_SPEC.loader.exec_module(SOURCE_POLICY)


ROOTS = (
    "crates/cadmpeg-ir/src",
    "crates/cadmpeg-core/src",
    "crates/cadmpeg-asm/src",
)

# The absent-key rule is a separate census with its own scope. The unknown-key
# rule follows the read routes of the three wire crates; the absent-key rule
# follows a writer's own spelling, so it holds for every crate that writes an
# optional key, codec-private records included. Every crate is in scope.
ABSENT_KEY_ROOT = "crates"

# Optional keys admitted by declaring path and field name, each with the
# reason it states its own absence spelling without the shared helper.
ABSENT_KEY_EXCEPTIONS = {
    "crates/cadmpeg-codec-f3d/src/records/topology.rs:postlude_value":
        "the key states a scalar run, not an option: its reader reads a Vec<i32>, "
        "which refuses null, and reads the empty run as None",
    "crates/cadmpeg-codec-f3d/src/records/feature.rs:previous_history_state_id_offset":
        "the writer states this key for every scope as a u64 and the format spells "
        "the absent preceding state as offset 0, which no record header can occupy; "
        "serialize_absent_u64_offset and deserialize_absent_u64_offset are that one "
        "spelling, and neither an absent key nor null is admitted",
}

# Read-only projections, admitted by declaring file and item name. Each
# states why an absent key and `null` are both the read artifact's own
# spellings. The admission is honoured only for an item that derives no
# writer of its own, so a round-trip wire type cannot take one.
COMMAND_REPORT_PROJECTION = (
    "a read-only projection over the command-report family: a section is "
    "absent from a report whose payload type has no such section and `null` "
    "in a report whose payload type has it and did not run it, so both "
    "spellings are the family's own; each writing type states its own "
    "spelling where it is written"
)
DOCUMENT_PROJECTION = (
    "a read-only projection over a CADIR document and its decode sidecar: a "
    "section this build does not find is one the writing build did not "
    "state; each writing type states its own spelling where it is written"
)
ABSENT_KEY_PROJECTIONS = {
    "crates/cadmpeg/src/query/mod.rs:ReportProbe": COMMAND_REPORT_PROJECTION,
    "crates/cadmpeg/src/query/mod.rs:RefusalProbe": COMMAND_REPORT_PROJECTION,
    "crates/cadmpeg/src/query/mod.rs:ContainerSummaryProbe":
        COMMAND_REPORT_PROJECTION,
    "crates/cadmpeg/src/query/mod.rs:ExportReportProbe":
        COMMAND_REPORT_PROJECTION,
    "crates/cadmpeg/src/query/mod.rs:DecodeReportProbe":
        COMMAND_REPORT_PROJECTION,
    "crates/cadmpeg/src/query/mod.rs:DecodeTransferProbe":
        COMMAND_REPORT_PROJECTION,
    "crates/cadmpeg/src/query/mod.rs:FindingProbe": COMMAND_REPORT_PROJECTION,
    "crates/cadmpeg/src/query/mod.rs:LossProbe": COMMAND_REPORT_PROJECTION,
    "crates/cadmpeg/src/query/mod.rs:CadirProbe": DOCUMENT_PROJECTION,
    "crates/cadmpeg/src/query/mod.rs:SidecarProbe": DOCUMENT_PROJECTION,
}


# Items admitted by declaring file and name, each with the reason it cannot
# carry a deny. Every entry is load-bearing: the item derives ``Deserialize``,
# reaches the check, and fails it without the entry. An item with a
# hand-written or ``Serialize``-only impl never reaches the check and states
# nothing here. The key is the declaring path, so a type that reuses an
# exception's name in another file is not admitted by it.
EXCEPTIONS = {
    "crates/cadmpeg-ir/src/document.rs:VersionProbe": "private one-field pre-pass; the document is re-read through CadIrReadWire, which denies",
}

SKIP_BASENAMES = {
    "tests.rs",
    "golden_tests.rs",
    "integration_tests.rs",
    "test_support.rs",
}
SKIP_DIRS = {"tests", "golden_tests", "integration_tests", "test_support"}

ITEM_RE = re.compile(
    r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?(struct|enum|type)\s+(\$?\w+)"
)
DECLARATION_OR_ATTRIBUTE = re.compile(
    r"#\s*\[|\b(?:pub(?:\s*\([^)]*\))?\s+)?(?:struct|enum|type)\s+\$?\w+"
)


class Item:
    def __init__(self, path, line, kind, name, attrs, body, scope=None,
                 start=0, end=None, reader_proven=False):
        self.path = path
        self.line = line
        self.kind = kind
        self.name = name
        self.attrs = attrs
        self.body = body
        self.start = start
        self.end = len(body) if end is None else end
        # Declaration macros can contain a handwritten Deserialize impl that
        # the source declaration parser cannot associate with the expansion.
        # This flag is set only from a recognized macro body, never from a
        # caller's name or an exception table.
        self.reader_proven = reader_proven
        src = path.parts.index("src")
        self.crate = Path(*path.parts[:src])
        parts = path.parts[src + 1:]
        module = parts[:-1] + (() if path.name in {"lib.rs", "main.rs", "mod.rs"} else (path.stem,))
        self.scope = module if scope is None else module + scope


class ImportBinding:
    """One source-level import or module binding in a lexical scope."""

    def __init__(self, path, scope, alias, target, offsets=(), scopes=(),
                 module_scope=()):
        self.path = path
        self.scope = scope
        self.alias = alias
        self.target = target
        self.offsets = offsets
        self.scopes = scopes
        self.module_scope = module_scope


class SourceContext:
    """Path and lexical scope used to resolve names inside a helper body."""

    def __init__(self, path, scope):
        self.path = path
        self.scope = scope
        source_index = path.parts.index("src")
        self.crate = Path(*path.parts[:source_index])


def source_files():
    def fail(error):
        raise error

    for root in ROOTS:
        base = Path(root)
        if not stat.S_ISDIR(base.stat().st_mode):
            raise NotADirectoryError(base)
        for directory, children, files in os.walk(base, onerror=fail):
            children[:] = sorted(name for name in children if name not in SKIP_DIRS)
            for name in sorted(files):
                if name.endswith(".rs") and name not in SKIP_BASENAMES:
                    yield Path(directory) / name


SCOPE_TOKEN = re.compile(r"\bmod\s+(\w+)\s*\{|[{}]")


def lexical_scopes(code):
    """Scope after each brace, preserving inline modules and local blocks."""
    offsets = [0]
    scopes = [()]
    stack = []
    for token in SCOPE_TOKEN.finditer(code):
        if token.group() == "}":
            if stack:
                stack.pop()
        else:
            stack.append(token.group(1) or f"@{token.start()}")
        offsets.append(token.end())
        scopes.append(tuple(stack))
    return offsets, scopes


def collect_items(path):
    source = path.read_text(encoding="utf-8")
    code, _ = SOURCE_POLICY.production_source(source)
    offsets, scopes = lexical_scopes(code)
    items = []
    cursor = 0
    while opening := DECLARATION_OR_ATTRIBUTE.search(code, cursor):
        start = cursor = opening.start()
        attrs = []
        while SOURCE_POLICY.OUTER_ATTRIBUTE.match(code, cursor):
            end = SOURCE_POLICY.attribute_end(code, cursor)
            if end is None:
                raise ValueError(f"{path}: incomplete attribute at byte {cursor}")
            attrs.append(source[cursor:end])
            cursor = end
            while cursor < len(code) and code[cursor].isspace():
                cursor += 1
        match = ITEM_RE.match(code, cursor)
        if match is None:
            continue
        body_end = SOURCE_POLICY.item_end(code, cursor)
        if body_end is None:
            raise ValueError(f"{path}: incomplete declaration at byte {cursor}")
        items.append(Item(
            path,
            source.count("\n", 0, start) + 1,
            match.group(1),
            match.group(2),
            "\n".join(attrs),
            source[cursor:body_end],
            scopes[bisect_right(offsets, cursor) - 1],
            start,
            body_end,
        ))
        cursor = body_end
    return items


class DeserializeImpl:
    """One handwritten ``Deserialize`` impl and its source body."""

    def __init__(self, path, line, name, body, scope, start, body_start, end,
                 input_bindings):
        self.path = path
        self.line = line
        self.name = name
        self.body = body
        self.scope = scope
        self.start = start
        self.body_start = body_start
        self.end = end
        self.input_bindings = input_bindings


class DeserializeFunction:
    """One helper function that owns a custom field deserializer."""

    def __init__(self, path, line, name, body, scope, start, end,
                 input_bindings):
        self.path = path
        self.line = line
        self.name = name
        self.body = body
        self.scope = scope
        self.start = start
        self.end = end
        self.input_bindings = input_bindings


DESERIALIZE_CALL_RE = re.compile(
    r"(?P<path>(?:::)?(?:[A-Za-z_$]\w*\s*::\s*)*[A-Za-z_$]\w*)"
    r"(?P<generic>\s*::\s*<(?P<args>[^{}]*)>)?\s*::\s*deserialize\b"
)
ARRAY_DESERIALIZE_CALL_RE = re.compile(
    r"<\s*\[[^{}]*\]\s*>\s*::\s*deserialize\b"
)


def delimited_end(code, opening, left="{", right="}"):
    """Return the exclusive end of one balanced delimiter sequence."""
    depth = 0
    for index in range(opening, len(code)):
        char = code[index]
        if char == left:
            depth += 1
        elif char == right:
            depth -= 1
            if depth == 0:
                return index + 1
            if depth < 0:
                return None
    return None


def body_open(code, start):
    """Find an impl body, ignoring delimiters in generic parameters."""
    depths = {"<": 0, "(": 0, "[": 0}
    closing = {")": "(",
        "]": "[",
        ">": "<",
    }
    for index in range(start, len(code)):
        char = code[index]
        if char in depths:
            depths[char] += 1
        elif char in closing:
            opener = closing[char]
            if depths[opener]:
                depths[opener] -= 1
        elif char == "{" and not any(depths.values()):
            return index
    return None


def parameter_open(code, start):
    """Find a function's parameter group after its name and generics."""
    angle = bracket = 0
    for index in range(start, len(code)):
        char = code[index]
        if char == "<":
            angle += 1
        elif char == ">" and angle:
            angle -= 1
        elif char == "[":
            bracket += 1
        elif char == "]" and bracket:
            bracket -= 1
        elif char == "(" and not angle and not bracket:
            return index
    return None


def parameter_bindings(text):
    """Return simple identifier bindings from a function parameter list."""
    bindings = set()
    code = SOURCE_POLICY.mask_rust_non_code(text)
    for parameter in split_metadata(code):
        colon = parameter.find(":")
        if colon < 0:
            continue
        pattern = parameter[:colon].strip()
        pattern = re.sub(r"^&\s*(?:'[_A-Za-z]\w*\s*)?", "", pattern)
        pattern = re.sub(r"^mut\s+", "", pattern)
        if re.fullmatch(r"[A-Za-z_]\w*", pattern) and pattern != "self":
            bindings.add(pattern)
    return bindings


def function_input_bindings(code, name="deserialize"):
    """Return simple input bindings for one function in ``code``."""
    match = re.search(rf"\bfn\s+{re.escape(name)}\b", code)
    if match is None:
        return set()
    opening = parameter_open(code, match.end())
    if opening is None:
        return set()
    end = delimited_end(code, opening, "(", ")")
    if end is None:
        return set()
    return parameter_bindings(code[opening + 1:end - 1])


def call_arguments(code, call):
    """Return a call's argument source, or ``None`` for a path-only mention."""
    opening = call.end()
    while opening < len(code) and code[opening].isspace():
        opening += 1
    if opening >= len(code) or code[opening] != "(":
        return None
    end = delimited_end(code, opening, "(", ")")
    if end is None:
        return None
    return code[opening + 1:end - 1]


def direct_input_argument(arguments, bindings):
    """Whether one call argument is exactly the reader's input binding."""
    for argument in split_metadata(arguments):
        value = argument.strip()
        value = re.sub(r"^&\s*(?:'[_A-Za-z]\w*\s*)?", "", value)
        value = re.sub(r"^mut\s+", "", value)
        if re.fullmatch(r"[A-Za-z_]\w*", value) and value in bindings:
            return True
    return False


def call_has_direct_input(code, call, bindings):
    """Prove direct input use only for an unaliased, unrebound parameter.

    Each candidate occurs once in its signature and once at the read call.
    Additional uses require binding analysis this lexical checker does not
    provide; a spelling reused by a local, closure or pattern is not proof.
    """
    single_use = {
        binding for binding in bindings
        if len(re.findall(rf"\b{re.escape(binding)}\b", code)) == 2
    }
    arguments = call_arguments(code, call)
    return arguments is not None and direct_input_argument(arguments, single_use)


def direct_input_calls(code, bindings):
    """Return every recognized deserializer call bound to the input parameter."""
    calls = list(ARRAY_DESERIALIZE_CALL_RE.finditer(code))
    calls.extend(DESERIALIZE_CALL_RE.finditer(code))
    return [call for call in calls if call_has_direct_input(code, call, bindings)]


def has_unproved_input_route(code, bindings):
    """Require an unconditional read whose failure leaves the reader."""
    return any(
        not call_is_unconditional(code, call)
        or not call_propagates_error(code, call)
        for call in direct_input_calls(code, bindings)
    )


def call_propagates_error(code, call):
    """Prove direct ``?`` propagation or a returned Result expression.

    Only Result adapters that preserve Err are followed. Discarded results,
    error recovery and local result aliases do not establish refusal.
    """
    opening = call.end()
    while opening < len(code) and code[opening].isspace():
        opening += 1
    if opening == len(code) or code[opening] != "(":
        return False
    end = delimited_end(code, opening, "(", ")")
    if end is None:
        return False
    while True:
        while end < len(code) and code[end].isspace():
            end += 1
        if code[end:end + 1] == "?":
            return True
        adapter = re.match(r"\.\s*(?:map|map_err|and_then)\s*\(", code[end:])
        if adapter is None:
            break
        end = delimited_end(code, end + adapter.end() - 1, "(", ")")
        if end is None:
            return False

    statement_start = max(
        code.rfind(";", 0, call.start()),
        code.rfind("{", 0, call.start()),
        code.rfind("}", 0, call.start()),
    ) + 1
    return (
        not code[statement_start:call.start()].strip()
        and re.fullmatch(r"\s*}\s*", code[end:]) is not None
    )


def source_module_scope(path):
    """Return the module path represented by a Rust source file path."""
    source_index = path.parts.index("src")
    parts = list(path.parts[source_index + 1:])
    file_name = parts.pop()
    stem = Path(file_name).stem
    if stem not in {"lib", "main", "mod"}:
        parts.append(stem)
    return tuple(parts)


USE_RE = re.compile(
    r"\b(?:pub(?:\s*\([^)]*\))?\s+)?use\b"
)
USE_PATH_RE = re.compile(
    r"(?:::)?(?:[A-Za-z_$]\w*\s*::\s*)*[A-Za-z_$]\w*"
)


def statement_end(code, start):
    """Find a semicolon at the top level of one Rust item statement."""
    depths = {"(": 0, "[": 0, "{": 0, "<": 0}
    closing = {
        ")": "(",
        "]": "[",
        "}": "{",
        ">": "<",
    }
    for index in range(start, len(code)):
        char = code[index]
        if char in depths:
            depths[char] += 1
        elif char in closing:
            opener = closing[char]
            if depths[opener]:
                depths[opener] -= 1
        elif char == ";" and not any(depths.values()):
            return index + 1
    return None


def use_path(path, prefix):
    """Join one use-tree child to its path prefix."""
    path = canonical_path(path)
    prefix = canonical_path(prefix).rstrip(":")
    if not prefix:
        return path
    if path.startswith("::"):
        return canonical_path(f"{prefix}{path}")
    return canonical_path(f"{prefix}::{path}")


def use_tree_bindings(text, prefix=""):
    """Yield ``(alias, target)`` entries from a use tree.

    This parser handles the path and grouped forms used by the wire crates.
    Wildcards retain an unresolved binding. Paths are taken from masked
    source, so comments and literals cannot manufacture an import. Syntax
    outside this parser still needs independent admission-route verification.
    """
    code = SOURCE_POLICY.mask_rust_non_code(text).strip()
    if not code:
        return

    brace = None
    depth = 0
    for index, char in enumerate(code):
        if char in "([<":
            depth += 1
        elif char in ")]>":
            depth -= 1
        elif char == "{" and depth == 0:
            brace = index
            break
    if brace is not None:
        end = delimited_end(code, brace)
        if end is None or code[end:].strip():
            return
        parent = code[:brace].strip()
        if parent.endswith("::"):
            parent = parent[:-2].rstrip()
        if not parent:
            for child in split_metadata(code[brace + 1:end - 1]):
                yield from use_tree_bindings(child, prefix)
            return
        for child in split_metadata(code[brace + 1:end - 1]):
            yield from use_tree_bindings(child, use_path(parent, prefix))
        return

    alias_match = re.search(r"\s+as\s+([A-Za-z_]\w*)\s*$", code)
    if alias_match is not None:
        path = code[:alias_match.start()].strip()
        alias = alias_match.group(1)
        if alias == "_" or USE_PATH_RE.fullmatch(path) is None:
            return
        yield alias, use_path(path, prefix)
        return

    if code == "*" and prefix:
        yield None, canonical_path(prefix)
        return

    if code.endswith("::*"):
        path = code[:-3].rstrip()
        if USE_PATH_RE.fullmatch(path) is not None:
            yield None, use_path(path, prefix)
        return

    if USE_PATH_RE.fullmatch(code) is None:
        return
    path = use_path(code, prefix)
    parts = path.lstrip(":").split("::")
    if not parts:
        return
    if parts[-1] == "self" and len(parts) > 1:
        alias = parts[-2]
    else:
        alias = parts[-1]
    if alias == "_":
        return
    yield alias, path


def collect_imports(path):
    """Collect imports and modules before recognizing external reader paths."""
    source = path.read_text(encoding="utf-8")
    code, _ = SOURCE_POLICY.production_source(source)
    offsets, scopes = lexical_scopes(code)
    module_scope = source_module_scope(path)
    imports = []
    cursor = 0
    while match := USE_RE.search(code, cursor):
        end = statement_end(code, match.end())
        if end is None:
            break
        statement = code[match.end():end - 1]
        scope = module_scope + scopes[bisect_right(offsets, match.start()) - 1]
        for alias, target in use_tree_bindings(statement):
            imports.append(
                ImportBinding(
                    path,
                    scope,
                    "*" if alias is None else alias,
                    target,
                    tuple(offsets),
                    tuple(scopes),
                    module_scope,
                )
            )
        cursor = end
    for match in re.finditer(r"\bmod\s+([A-Za-z_]\w*)\s*(?:;|\{)", code):
        scope = module_scope + scopes[bisect_right(offsets, match.start()) - 1]
        name = match.group(1)
        imports.append(ImportBinding(
            path, scope, name, "::".join(("crate", *scope, name)),
            tuple(offsets), tuple(scopes), module_scope,
        ))
    return imports


def control_block_kind(code, opening):
    """Classify a brace that can make a reader call conditional."""
    prefix = code[:opening].rstrip()
    if re.search(r"\bmacro_rules\s*!\s*[A-Za-z_]\w*$", prefix):
        return "macro"
    closure = re.search(
        r"(?:^|[;{}(=,:])\s*(?:async\s+)?(?:move\s+)?"
        r"\|[^|{};\n]*\|\s*(?:->\s*[^{};\n]+)?$",
        prefix,
    )
    if closure is not None:
        return "closure"
    if re.search(r"(?:^|[;{}(=,:])\s*async(?:\s+move)?$", prefix):
        return "closure"

    segment = re.split(r"[;{}]", prefix)[-1].strip()
    if re.search(r"\b(?:if|else|match|while|for|loop)\b", segment):
        return "conditional"
    return None


def call_is_unconditional(code, call):
    """Prove a reader call is reached on the direct route being inspected.

    The census does not evaluate Rust. Calls in closures, branch or loop
    bodies, and short-circuit expressions are therefore not consumption
    evidence. A call in an unsupported control shape fails closed; this is a
    static proof limit and does not reject the corresponding Rust value.
    """
    # An earlier exit can make a later direct read unreachable. This lexical
    # proof does not establish whether a containing branch takes that exit.
    if re.search(r"\b(?:return|break|continue)\b", code[:call.start()]):
        return False
    # Macro arguments are tokens, not evaluated expressions. Expansion can
    # discard a read even when its source text contains a propagated result.
    for invocation in re.finditer(r"\b[A-Za-z_$]\w*\s*!\s*([({\[])", code[:call.start()]):
        left = invocation.group(1)
        right = {"(": ")", "{": "}", "[": "]"}[left]
        end = delimited_end(code, invocation.end() - 1, left, right)
        if end is None or call.start() < end:
            return False
    stack = []
    for index, char in enumerate(code[:call.start()]):
        if char == "{":
            stack.append(control_block_kind(code, index))
        elif char == "}" and stack:
            stack.pop()
    if any(kind is not None for kind in stack):
        return False

    statement_start = max(
        code.rfind(";", 0, call.start()),
        code.rfind("{", 0, call.start()),
        code.rfind("}", 0, call.start()),
    )
    statement = code[statement_start + 1:call.start()]
    if re.search(r"&&|\|\|", statement):
        return False

    # A closure with a single expression has no body brace to classify above.
    # The statement boundary keeps ordinary logical-or expressions from being
    # mistaken for a closure introducer.
    if re.search(
        r"(?:^|[=(:,]|\breturn\s+)(?:async\s+)?(?:move\s+)?"
        r"\|[^|{};\n]*\|\s*(?:->\s*[^{};\n]+)?[^;{}]*$",
        statement,
    ):
        return False
    return True


def macro_reader_contract(code, expected_path):
    """Prove one direct deserializer route in a declaration macro.

    Only the body of the generated ``deserialize`` method can certify the
    expansion. A helper function elsewhere in the macro may use the expected
    reader and must not become evidence for the method's input route.
    """
    code = SOURCE_POLICY.mask_rust_non_code(code)
    methods = []
    for method in re.finditer(r"\bfn\s+deserialize\b", code):
        opening = parameter_open(code, method.end())
        if opening is None:
            continue
        brace = body_open(code, method.end())
        if brace is None:
            continue
        end = delimited_end(code, brace)
        if end is None:
            continue
        methods.append(code[method.start():end])
    if len(methods) != 1:
        return False
    method = methods[0]
    bindings = function_input_bindings(method)
    if has_unproved_input_route(method, bindings):
        return False
    calls = [
        call for call in DESERIALIZE_CALL_RE.finditer(method)
        if call_has_direct_input(method, call, bindings)
    ]
    if len(calls) != 1:
        return False
    return canonical_path(calls[0].group("path")) == expected_path


def deserialize_impl_target(header):
    """Return the target only when the impl trait itself is Deserialize."""
    code = SOURCE_POLICY.mask_rust_non_code(header)
    implementation = re.match(r"\s*impl\b", code)
    if implementation is None:
        return None
    trait_start = implementation.end()
    while trait_start < len(code) and code[trait_start].isspace():
        trait_start += 1
    if trait_start < len(code) and code[trait_start] == "<":
        trait_start = generic_end(code, trait_start)
        if trait_start is None:
            return None
    for_match = re.search(r"\bfor\b", code[trait_start:])
    if for_match is None:
        return None
    trait_end = trait_start + for_match.start()
    trait = code[trait_start:trait_end].strip()
    if re.fullmatch(
        r"(?:::)?(?:[A-Za-z_$]\w*\s*::\s*)*Deserialize"
        r"\s*(?:<[^{}]*>)?",
        trait,
    ) is None:
        return None
    target = re.match(
        r"(?P<target>(?:::)?(?:[A-Za-z_$]\w*\s*::\s*)*"
        r"[A-Za-z_$]\w*(?:\s*<[^{}]*>)?)",
        code[trait_start + for_match.end():].lstrip(),
    )
    return target.group("target").strip() if target else None


def collect_deserialize_impls(path):
    """Collect handwritten readers without accepting comments or literals."""
    source = path.read_text(encoding="utf-8")
    code, _ = SOURCE_POLICY.production_source(source)
    offsets, scopes = lexical_scopes(code)
    readers = []
    for opening in re.finditer(r"\bimpl\b", code):
        brace = body_open(code, opening.end())
        if brace is None:
            continue
        header = code[opening.start():brace]
        target = deserialize_impl_target(header)
        if target is None:
            continue
        name_match = re.search(r"([A-Za-z_$]\w*)\s*(?:<|$)", target.rsplit("::", 1)[-1])
        if name_match is None:
            continue
        name = name_match.group(1)
        if name.startswith("$"):
            continue
        end = delimited_end(code, brace)
        if end is None:
            raise ValueError(f"{path}: incomplete Deserialize impl at byte {opening.start()}")
        implementation = code[brace + 1:end - 1]
        method_match = re.search(r"\bfn\s+deserialize\b", implementation)
        if method_match is None:
            raise ValueError(f"{path}: Deserialize impl for {name} has no method body")
        method_start = brace + 1 + method_match.start()
        method_brace = body_open(code, brace + 1 + method_match.end())
        if method_brace is None:
            raise ValueError(f"{path}: incomplete deserialize method for {name}")
        method_end = delimited_end(code, method_brace)
        if method_end is None:
            raise ValueError(f"{path}: incomplete deserialize method for {name}")
        method_code = code[method_start:method_end]
        readers.append(DeserializeImpl(
            path,
            source.count("\n", 0, method_start) + 1,
            name,
            source[method_start:method_end],
            scopes[bisect_right(offsets, opening.start()) - 1],
            method_start,
            method_brace + 1,
            method_end,
            function_input_bindings(method_code),
        ))
    return readers


def collect_deserialize_functions(path):
    """Collect named helper functions used by custom field readers."""
    source = path.read_text(encoding="utf-8")
    code, _ = SOURCE_POLICY.production_source(source)
    offsets, scopes = lexical_scopes(code)
    module_scope = source_module_scope(path)
    functions = []
    for opening in re.finditer(r"\bfn\s+(?P<name>[A-Za-z_]\w*)\s*", code):
        brace = body_open(code, opening.end())
        if brace is None:
            continue
        end = delimited_end(code, brace)
        if end is None:
            raise ValueError(f"{path}: incomplete helper function at byte {opening.start()}")
        function_code = code[opening.start():end]
        functions.append(DeserializeFunction(
            path,
            source.count("\n", 0, opening.start()) + 1,
            opening.group("name"),
            source[opening.start():end],
            module_scope + scopes[bisect_right(offsets, opening.start()) - 1],
            opening.start(),
            end,
            function_input_bindings(function_code, opening.group("name")),
        ))
    return functions


def macro_templates(path, items):
    """Return recognized declaration macro templates found in ``path``."""
    source = path.read_text(encoding="utf-8")
    code, _ = SOURCE_POLICY.production_source(source)
    templates = {}
    for definition in re.finditer(
        r"\bmacro_rules\s*!\s*(?P<name>"
        r"id_type|local_id_type|checked_scalar|checked_feature_geometry)\s*\{",
        code,
    ):
        body_end = delimited_end(code, definition.end() - 1)
        if body_end is None:
            raise ValueError(f"{path}: incomplete macro definition at byte {definition.start()}")
        template = next(
            (
                item for item in items
                if item.name == "$name"
                and item.kind == "struct"
                and definition.end() <= item.start < body_end
            ),
            None,
        )
        macro_body = code[definition.end():body_end - 1]
        body_contract = {
            "id_type": derives_deserialize(template.attrs) if template else False,
            "local_id_type": derives_deserialize(template.attrs) if template else False,
            "checked_scalar": bool(
                re.search(
                    r"impl\s*(?:<[^{}]*>)?\s*"
                    r"(?:[A-Za-z_$]\w*\s*::\s*)*Deserialize"
                    r"[^{}]*for\s+\$name\b",
                    macro_body,
                )
                and macro_reader_contract(macro_body, "f64")
            ),
            "checked_feature_geometry": bool(
                re.search(
                    r"impl\s*(?:<[^{}]*>)?\s*"
                    r"(?:[A-Za-z_$]\w*\s*::\s*)*Deserialize"
                    r"[^{}]*for\s+\$name\b",
                    macro_body,
                )
                and macro_reader_contract(macro_body, "$raw")
            ),
        }
        if (
            template is not None
            and has_serde_flag(template.attrs, "transparent")
            and body_contract[definition.group("name")]
        ):
            contract = template
        else:
            # Keep an unproved definition visible to the caller. A valid
            # definition with the same macro name in another module must not
            # silently certify this one.
            contract = None
        name = definition.group("name")
        if name in templates:
            # Duplicate macro definitions are not a deterministic expansion
            # contract, even when both happen to look valid.
            templates[name] = None
        else:
            templates[name] = contract
    return templates


def macro_generated_items(path, items, templates=None):
    """Expand declaration macros whose template is visible in this source.

    Rust expands the recognized declaration macros before deriving or
    implementing their readers. The census has no Rust macro expansion, so it
    reads each template declaration and instantiates the names at each call
    site. This is source proof from the macro body, not an exception list of
    accepted identifiers.
    """
    source = path.read_text(encoding="utf-8")
    code, _ = SOURCE_POLICY.production_source(source)
    offsets, scopes = lexical_scopes(code)
    if templates is None:
        templates = macro_templates(path, items)
    generated = []
    call_re = re.compile(
        r"\b(?P<macro>"
        r"id_type|local_id_type|checked_scalar|checked_feature_geometry)\s*!\s*\("
    )
    for call in call_re.finditer(code):
        end = delimited_end(code, call.end() - 1, "(", ")")
        if end is None:
            raise ValueError(f"{path}: incomplete declaration macro at byte {call.start()}")
        args = code[call.end():end - 1]
        parts = list(split_metadata(args))
        first_argument = next(
            (strip_variant_attributes(part) for part in parts
             if strip_variant_attributes(part)),
            "",
        )
        name_match = re.fullmatch(r"[A-Za-z_]\w*", first_argument)
        if name_match is None:
            continue
        # Documentation and the checked-scalar condition/error are macro
        # arguments too.  The first identifier is the ``$name`` argument;
        # taking the last one turns ``Length, value, true, ...`` into an
        # invented declaration such as ``true``.
        name = name_match.group(0)
        template = templates.get(call.group("macro"))
        if template is None:
            continue
        body = template.body.replace("$name", name)
        if call.group("macro") == "checked_feature_geometry":
            raw_argument = (
                strip_variant_attributes(parts[1]).strip()
                if len(parts) > 1 else ""
            )
            if not re.fullmatch(
                r"(?:::)?(?:[A-Za-z_]\w*\s*::\s*)*[A-Za-z_]\w*",
                raw_argument,
            ):
                continue
            body = body.replace("$raw", raw_argument)
        generated.append(Item(
            path,
            source.count("\n", 0, call.start()) + 1,
            template.kind,
            name,
            template.attrs,
            body,
            scopes[bisect_right(offsets, call.start()) - 1],
            call.start(),
            end,
            True,
        ))
    return generated


def deserializer_macro_contracts(path):
    """Read forwarding contracts from the macro bodies that define them."""
    source = path.read_text(encoding="utf-8")
    code, _ = SOURCE_POLICY.production_source(source)
    contracts = {}
    for definition in re.finditer(
        r"\bmacro_rules\s*!\s*(?P<name>"
        r"selection_field_deserializer|named_field|named_optional_field)\s*\{",
        code,
    ):
        body_end = delimited_end(code, definition.end() - 1)
        if body_end is None:
            raise ValueError(f"{path}: incomplete deserializer macro at byte {definition.start()}")
        body = code[definition.end():body_end - 1]
        name = definition.group("name")
        if name == "selection_field_deserializer":
            contract = "forward" if re.search(
                r"\bT\s*::\s*deserialize\b", body
            ) else None
        else:
            target = (
                r"\$crate\s*::\s*absent_key\s*::\s*named_present"
                if name == "named_optional_field"
                else r"\$crate\s*::\s*units\s*::\s*deserialize_named"
            )
            contract = "named" if re.search(rf"{target}\b", body) else None
        if name in contracts:
            # A same-named macro with a different body leaves no reliable
            # forwarding contract, including valid plus unproved bodies.
            contracts[name] = contract if contracts[name] == contract else None
        else:
            contracts[name] = contract
    return contracts


def deserializer_helpers(path, contracts=None):
    """Collect local macro-generated deserializers with source proof.

    ``selection_field_deserializer!`` forwards to the selected type's
    ``Deserialize`` implementation. ``named_field!`` and
    ``named_optional_field!`` do the same through the units helper, with the
    declared target type visible at the call site. These are forwarding
    contracts in the macro bodies; names alone are never admitted.
    """
    source = path.read_text(encoding="utf-8")
    code, _ = SOURCE_POLICY.production_source(source)
    if contracts is None:
        contracts = deserializer_macro_contracts(path)
    forwarders = set()
    named = {}
    call_re = re.compile(
        r"(?:::)?(?:[A-Za-z_]\w*::)*(?P<macro>"
        r"selection_field_deserializer|named_field|named_optional_field)\s*!\s*\("
    )
    for call in call_re.finditer(code):
        end = delimited_end(code, call.end() - 1, "(", ")")
        if end is None:
            raise ValueError(f"{path}: incomplete deserializer macro at byte {call.start()}")
        args = code[call.end():end - 1]
        parts = list(split_metadata(args))
        if not parts:
            continue
        # A shim whose value type mentions its container's type parameters
        # names them after the shim: `deserialize_side<S>`.
        name = strip_variant_attributes(parts[0]).strip().split("<", 1)[0].strip()
        if not re.fullmatch(r"[A-Za-z_]\w*", name):
            continue
        if call.group("macro") == "selection_field_deserializer":
            if contracts.get(call.group("macro")) == "forward":
                forwarders.add(name)
            continue
        expected_contract = "named" if call.group("macro") in {
            "named_field", "named_optional_field"
        } else None
        if contracts.get(call.group("macro")) != expected_contract:
            continue
        if len(parts) < 2:
            continue
        target = parts[1].strip()
        if not target:
            continue
        named[name] = (target, call.group("macro") == "named_optional_field")
    return forwarders, named


def split_metadata(text):
    """Split metadata at unquoted top-level commas, retaining original slices."""
    code = SOURCE_POLICY.mask_rust_non_code(text)
    depth = start = 0
    for pos, char in enumerate(code):
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == "<":
            depth += 1
        elif char == ">" and depth:
            depth -= 1
        elif char == "," and depth == 0:
            yield text[start:pos]
            start = pos + 1
    yield text[start:]


def attribute_arguments(text, wanted):
    """Yield actual leading attribute arguments and their conditional status.

    Masked source supplies positions; strings and comments cannot name metadata.
    A cfg_attr condition is not evaluated, so its children remain conditional.
    """
    def arguments(meta, conditional):
        code = SOURCE_POLICY.mask_rust_non_code(meta)
        match = re.fullmatch(r"\s*(\w+)\s*\((.*)\)\s*", code, re.S)
        if match is None:
            return
        inner = meta[match.start(2):match.end(2)]
        if match.group(1) == wanted:
            yield inner, conditional
        elif match.group(1) == "cfg_attr":
            for child in list(split_metadata(inner))[1:]:
                yield from arguments(child, True)

    code = SOURCE_POLICY.mask_rust_non_code(text)
    cursor = 0
    while True:
        while cursor < len(code) and code[cursor].isspace():
            cursor += 1
        opening = SOURCE_POLICY.OUTER_ATTRIBUTE.match(code, cursor)
        if opening is None:
            return
        end = SOURCE_POLICY.attribute_end(code, cursor)
        if end is None:
            raise ValueError("incomplete attribute metadata")
        yield from arguments(text[opening.end():end - 1], False)
        cursor = end


def serde_options(attrs):
    """Yield option names, original value tokens and conditional status."""
    for arguments, conditional in attribute_arguments(attrs, "serde"):
        for option in split_metadata(arguments):
            code = SOURCE_POLICY.mask_rust_non_code(option)
            match = re.match(r"\s*(\w+)", code)
            if match:
                yield match.group(1), option[match.end():].strip(), conditional


def has_serde_flag(attrs, name, include_conditional=False):
    return any(key == name and not SOURCE_POLICY.mask_rust_non_code(value).strip()
               and (include_conditional or not conditional)
               for key, value, conditional in serde_options(attrs))


def reader_type(value):
    """Read a type-name string; unsupported literal forms have no static proof."""
    code = SOURCE_POLICY.mask_rust_non_code(value)
    match = re.fullmatch(r"\s*=\s+", code)
    if match is None:
        return None
    literals = [value[start:end] for start, end in SOURCE_POLICY.rust_non_code_spans(value)
                if not value[start:end].startswith(("//", "/*"))]
    if len(literals) != 1:
        return None
    literal = literals[0]
    raw = re.fullmatch(r'r(#{0,})"(.*)"\1', literal, re.S)
    if raw:
        return raw.group(2)
    try:
        return json.loads(literal)
    except (ValueError, TypeError):
        return None


def canonical_path(value):
    """Normalize a Rust path without treating a suffix as ownership proof."""
    return re.sub(r"\s*::\s*", "::", value).replace("$crate", "crate").strip()


def derives_deserialize(attrs):
    for arguments, _ in attribute_arguments(attrs, "derive"):
        for name in split_metadata(arguments):
            code = SOURCE_POLICY.mask_rust_non_code(name).strip()
            if re.fullmatch(r"(?:::\s*)?(?:\w+\s*::\s*)*Deserialize", code):
                return True
    return False


def derives_serialize(attrs):
    for arguments, _ in attribute_arguments(attrs, "derive"):
        for name in split_metadata(arguments):
            code = SOURCE_POLICY.mask_rust_non_code(name).strip()
            if re.fullmatch(r"(?:::\s*)?(?:\w+\s*::\s*)*Serialize", code):
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
    return [raw for raw in split_metadata(body) if strip_variant_attributes(raw)]


def strip_variant_attributes(raw):
    """The variant text with its attributes and comments removed."""
    code = SOURCE_POLICY.mask_rust_non_code(raw)
    cursor = 0
    while True:
        while cursor < len(code) and code[cursor].isspace():
            cursor += 1
        if SOURCE_POLICY.OUTER_ATTRIBUTE.match(code, cursor) is None:
            return code[cursor:].strip()
        end = SOURCE_POLICY.attribute_end(code, cursor)
        if end is None:
            raise ValueError("incomplete variant attribute")
        cursor = end


def strip_variant_attributes_source(raw):
    """Remove leading variant attributes while retaining their source text.

    Structural classification uses the masked form above. Payload metadata
    also needs the original ``deserialize_with`` value so that a helper can
    be classified by its actual path.
    """
    code = SOURCE_POLICY.mask_rust_non_code(raw)
    cursor = 0
    while True:
        while cursor < len(code) and code[cursor].isspace():
            cursor += 1
        if SOURCE_POLICY.OUTER_ATTRIBUTE.match(code, cursor) is None:
            return raw[cursor:].strip()
        end = SOURCE_POLICY.attribute_end(code, cursor)
        if end is None:
            raise ValueError("incomplete variant attribute")
        cursor = end


def enum_variants(item):
    if item.kind != "enum":
        return None
    return [
        strip_variant_attributes(raw)
        for raw in split_variants(strip_macro_repetition(enum_body(item)))
    ]


def is_uninhabited_enum(item):
    """An enum with no variant: no document can name one, so no key is read."""
    variants = enum_variants(item)
    return variants is not None and not variants


def all_unit_variants(item):
    variants = enum_variants(item)
    if not variants:
        return False
    return all("(" not in text and "{" not in text for text in variants)


def split_tuple_types(text):
    """Split the inside of a tuple variant into its top-level type texts."""
    code = SOURCE_POLICY.mask_rust_non_code(text)
    depth = 0
    types = []
    current = ""
    for char, masked in zip(text, code):
        if masked in "({[<":
            depth += 1
        elif masked in ")}]>":
            depth -= 1
        if masked == "," and depth == 0:
            types.append(current)
            current = ""
        else:
            current += char
    types.append(current)
    return [part for part in (item.strip() for item in types) if part]


class Arm:
    def __init__(self, name, kind, types, untagged, skipped_newtype=False):
        self.name = name
        self.kind = kind
        self.types = types
        self.untagged = untagged
        self.skipped_newtype = skipped_newtype


GENERIC_HEADER_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:struct|enum|type)\s+[A-Za-z_$]\w*\s*<"
)


def generic_parameters(item):
    """Return the item's declared type parameters as ``(name, default)``.

    Lifetimes and const parameters cannot name a reader, so they are not
    returned. A header this parser cannot read returns an empty list, which
    keeps the caller on its fail-closed route.
    """
    opening = GENERIC_HEADER_RE.match(item.body)
    if opening is None:
        return []
    end = generic_end(item.body, opening.end() - 1)
    if end is None:
        return []
    parameters = []
    for part in split_tuple_types(item.body[opening.end():end - 1]):
        if part.startswith("'") or part.startswith("const "):
            continue
        name, _, default = part.partition("=")
        # A bound belongs to the parameter, not to its default.
        name = name.split(":", 1)[0].strip()
        if not re.fullmatch(r"[A-Za-z_]\w*", name):
            return []
        parameters.append((name, default.strip() or None))
    return parameters


def generic_arguments(name, position, items):
    """Return every argument a workspace declaration states at ``position``.

    Only declarations are read. A function signature forwards its caller's
    parameter and instantiates no reader, so it is not a source of arguments
    here; a declaration that supplies fewer arguments than ``position``
    leaves the parameter on its default and contributes nothing.
    """
    use = re.compile(rf"\b{re.escape(name)}\s*<")
    arguments = []
    for item in items:
        header = GENERIC_HEADER_RE.match(item.body)
        own_header = header.end() - 1 if header is not None else None
        for match in use.finditer(item.body):
            if match.end() - 1 == own_header:
                # The declaration's own header states parameters, not
                # arguments; its default is read by the caller.
                continue
            end = generic_end(item.body, match.end() - 1)
            if end is None:
                continue
            parts = split_tuple_types(item.body[match.end():end - 1])
            if position < len(parts):
                arguments.append((parts[position], item))
    return arguments


def type_without_attributes(text):
    """Return a field or tuple type after its leading serde attributes."""
    code = strip_variant_attributes(text).strip()
    return re.sub(r"^pub(?:\s*\([^)]*\))?\s+", "", code)


def type_parts(text):
    """Return a type path and its top-level generic arguments.

    The census only needs the outer type constructor. Rust's parser is not
    available here, so a malformed or unsupported expression returns
    ``(None, None)`` and is handled as an unproved route.
    """
    code = type_without_attributes(text)
    if not code:
        return None, None
    if code.startswith("&"):
        # A reference to a scalar still cannot consume an object. A reference
        # to an arbitrary type keeps the inner route visible to the caller.
        remainder = code[1:].lstrip()
        remainder = re.sub(r"^'[_A-Za-z]\w*\s*", "", remainder)
        if remainder.startswith("mut "):
            remainder = remainder[4:].lstrip()
        return type_parts(remainder)
    if code.startswith("[") or code.startswith("("):
        return code[0], []
    match = re.match(
        r"(?P<path>(?:::)?(?:[A-Za-z_$]\w*\s*::\s*)*[A-Za-z_$]\w*)",
        code,
    )
    if match is None:
        return None, None
    path = re.sub(r"\s*::\s*", "::", match.group("path"))
    path = path.replace("$crate", "crate")
    cursor = match.end()
    while cursor < len(code) and code[cursor].isspace():
        cursor += 1
    if cursor == len(code):
        return path, []
    if code[cursor] != "<":
        return None, None
    end = generic_end(code, cursor)
    if end is None or code[end:].strip():
        return None, None
    return path, split_tuple_types(code[cursor + 1:end - 1])


def generic_end(code, opening):
    """Return the exclusive end of a generic argument list."""
    depth = 0
    for index in range(opening, len(code)):
        if code[index] == "<":
            depth += 1
        elif code[index] == ">":
            depth -= 1
            if depth == 0:
                return index + 1
            if depth < 0:
                return None
    return None


SCALAR_TYPES = {
    "bool", "char", "str", "String",
    "i8", "i16", "i32", "i64", "i128", "isize",
    "u8", "u16", "u32", "u64", "u128", "usize",
    "f32", "f64",
}
SEQUENCE_TYPES = {
    "Vec", "VecDeque", "LinkedList", "BinaryHeap", "HashSet", "BTreeSet",
    "ByteBuf",
}
MAP_TYPES = {"HashMap", "BTreeMap", "IndexMap"}
QUALIFIED_SCALAR_TYPES = {
    *{f"core::primitive::{name}" for name in SCALAR_TYPES if name not in {"String"}},
    *{f"std::primitive::{name}" for name in SCALAR_TYPES if name not in {"String"}},
    "alloc::string::String",
    "std::string::String",
}
QUALIFIED_SEQUENCE_TYPES = {
    "alloc::vec::Vec",
    "std::vec::Vec",
    "alloc::collections::VecDeque",
    "std::collections::VecDeque",
    "alloc::collections::LinkedList",
    "std::collections::LinkedList",
    "alloc::collections::BinaryHeap",
    "std::collections::BinaryHeap",
    "std::collections::HashSet",
    "std::collections::BTreeSet",
    "serde_bytes::ByteBuf",
}
QUALIFIED_MAP_TYPES = {
    "std::collections::HashMap",
    "std::collections::BTreeMap",
    "indexmap::IndexMap",
}
QUALIFIED_KEYLESS_ADAPTERS = {
    "crate::bytes",
    "cadmpeg_ir::bytes",
}
OPTIONAL_TYPES = {"Option"}
DELEGATING_TYPES = {"Box", "Rc", "Arc", "Pin", "RefCell", "Cell", "Mutex", "RwLock"}
ABSENT_KEY_HELPERS = {
    "crate::absent_key::nullable",
    "cadmpeg_core::absent_key::nullable",
}
DISTINCT_MAP_HELPERS = {
    "crate::distinct_keys::btree_map",
    "crate::distinct_keys::hash_map",
    "crate::distinct_keys::json_object",
    "cadmpeg_core::distinct_keys::btree_map",
    "cadmpeg_core::distinct_keys::hash_map",
    "cadmpeg_core::distinct_keys::json_object",
}
LOCAL_ID_HELPERS = {
    "crate::ids::deserialize_local_id",
}
OPEN_READER_ROUTE = re.compile(
    r"\bdeserialize_(?:any|map|struct|enum|identifier|ignored_any|"
    r"newtype_struct|seq|tuple)\s*\(|"
    r"\b(?:MapAccess|MapDeserializer|Visitor|visit_map|visit_seq)\b"
)


def is_free_form_map(path):
    """Whether a fully qualified map path intentionally admits arbitrary keys."""
    return path.lstrip(":") in {
        "serde_json::Value",
        "serde_json::Map",
        "std::collections::HashMap",
        "std::collections::BTreeMap",
        "indexmap::IndexMap",
    }


def is_builtin_path(path, names, index):
    """Whether ``path`` names a known standard reader without a suffix guess."""
    path = path.lstrip(":")
    if "::" not in path:
        # A declaration in the census index can shadow a prelude name. The
        # caller resolves that declaration before invoking this shortcut.
        return path in names and path not in index
    if names is SCALAR_TYPES:
        return path in QUALIFIED_SCALAR_TYPES
    if names is SEQUENCE_TYPES:
        return path in QUALIFIED_SEQUENCE_TYPES
    if names is MAP_TYPES:
        return path in QUALIFIED_MAP_TYPES
    return False


def is_keyless_path(path, index):
    return (
        is_builtin_path(path, SCALAR_TYPES, index)
        or is_builtin_path(path, SEQUENCE_TYPES, index)
        or path.lstrip(":") in QUALIFIED_KEYLESS_ADAPTERS
    )


def untagged_arms(item):
    """The arms of an enum body, classified as unit, tuple or struct.

    ``untagged`` is ``always``, ``conditional`` or absent. A conditional arm
    must be checked under both its tagged and its untagged read routes.
    """
    arms = []
    for raw in split_variants(strip_macro_repetition(enum_body(item))):
        if has_serde_flag(raw, "skip") or has_serde_flag(raw, "skip_deserializing"):
            continue
        untagged = ("always" if has_serde_flag(raw, "untagged") else
                    "conditional" if has_serde_flag(raw, "untagged", include_conditional=True)
                    else None)
        text = strip_variant_attributes_source(raw)
        match = re.match(r"(\$?\w+)\s*(.*)", text, re.S)
        if match is None:
            continue
        name = match.group(1)
        rest = match.group(2).lstrip()
        if rest.startswith("{"):
            arms.append(Arm(name, "struct", [], untagged))
        elif rest.startswith("("):
            inner = rest[1 : rest.rfind(")")]
            fields = split_tuple_types(inner)
            skipped_newtype = len(fields) == 1 and any(
                has_serde_flag(fields[0], flag, include_conditional=True)
                for flag in ("skip", "skip_deserializing")
            )
            arms.append(Arm(name, "tuple", fields, untagged, skipped_newtype))
        else:
            arms.append(Arm(name, "unit", [], untagged))
    return arms


def alias_target(item):
    """Read a nongeneric type alias; other alias shapes need explicit proof."""
    code = SOURCE_POLICY.mask_rust_non_code(item.body)
    match = re.fullmatch(
        r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?type\s+\w+\s*=\s*(.*?)\s*;\s*",
        code,
        re.S,
    )
    return match.group(1) if match else None


def resolve_item(index, name, owner, ambiguities):
    """Resolve qualified declarations and distinguish lexical item scopes.

    A bare name may have been imported from a different file. A unique
    declaration remains the census's imported-name fallback; duplicates require
    a declaration in the current lexical scope. Qualified paths never use that
    fallback. An unresolved qualified path fails closed.
    """
    parts = name.lstrip(":").split("::")
    candidates = index.get(parts[-1], ())
    if len(parts) > 1:
        prefix = parts[:-1]
        crate = owner.crate
        scope = tuple(part for part in owner.scope if not part.startswith("@"))
        if prefix[0] == "crate":
            scope = ()
            prefix = prefix[1:]
        elif prefix[0] == "self":
            prefix = prefix[1:]
        elif prefix[0] == "super":
            while prefix and prefix[0] == "super":
                scope = scope[:-1]
                prefix = prefix[1:]
        else:
            external = [candidate.crate for candidate in candidates
                        if candidate.crate.name.replace("-", "_") == prefix[0]]
            if external:
                crate = external[0]
                scope = ()
                prefix = prefix[1:]
        wanted = scope + tuple(prefix)
        matches = [candidate for candidate in candidates
                   if candidate.crate == crate and candidate.scope == wanted]
        if not matches:
            # A public re-export such as ``crate::geometry::NurbsCurve`` can
            # own its declaration in the private ``geometry::carriers``
            # module. A unique descendant is source evidence for that module
            # route; ambiguous descendants still fail closed.
            descendants = [
                candidate for candidate in candidates
                if candidate.crate == crate
                and candidate.scope[:len(wanted)] == wanted
            ]
            if len(descendants) == 1:
                matches = descendants
    else:
        scope = owner.scope
        while True:
            matches = [candidate for candidate in candidates
                       if candidate.path == owner.path and candidate.scope == scope]
            if matches or not scope or not scope[-1].startswith("@"):
                break
            scope = scope[:-1]
        if not matches and len(candidates) == 1 and candidates[0].path != owner.path:
            matches = list(candidates)
    if len(matches) == 1:
        return matches[0]
    if candidates or len(parts) > 1:
        ambiguities.append((owner.path, owner.line, owner.name, name,
                            tuple(candidate.path for candidate in candidates)))
    return None


ANY_ATTRIBUTE = re.compile(r"#\s*\[(?:[^\[\]]|\[(?:[^\[\]]|\[[^\[\]]*\])*\])*\]")
ATTRIBUTE_GAP = re.compile(r"(?:\s|///[^\n]*|//![^\n]*|//[^\n]*)*\Z")
OMITTED_OPTIONAL_KEY = re.compile(r'skip_serializing_if\s*=\s*"Option::is_none"')
DESERIALIZE_WITH = re.compile(r'deserialize_with\s*=\s*"([\w:]+)"')
SERDE_WITH = re.compile(r'(?<!\w)with\s*=\s*"([\w:]+)"')
ABSENT_KEY_NULLABLE = re.compile(
    r'deserialize_with\s*=\s*"(?:crate|cadmpeg_core)::absent_key::nullable"'
)
# The one workspace macro that declares a named reader forwarding to
# ``absent_key::named_present``. Its expansion carries the same absence
# spelling and names the key in whatever it refuses. The macro path is read,
# not the bare name: a ``macro_rules! named_optional_field`` anywhere else
# expands to whatever it likes, so the census names that definition instead.
PRESENT_FORWARDER_MACRO = re.compile(
    r"(?:(cadmpeg_core|crate)\s*::\s*)?named_optional_field!\s*\("
)
DECLARED_READER_NAME = re.compile(r"\s*(?:pub\s*(?:\([^)]*\)\s*)?)?(\w+)")
PRESENT_FORWARDER_DEFINITION = re.compile(r"macro_rules!\s*named_optional_field\b")
# The crate and file that own the macro's definition.
PRESENT_FORWARDER_OWNER = ("cadmpeg-core", "absent_key.rs")
# The serde key a field states, where it is not the field's own name.
SERDE_RENAME = re.compile(r'(?<!\w)rename\s*=\s*"([^"]+)"')
USE_DECLARATION = re.compile(r"\buse\s+([^;]+);")
SERDE_DEFAULT = re.compile(r"(?:^|[(,])\s*default\s*(?:[,)]|=)")
# A flattened field has no key of its own, so an absent key names nothing
# about it: its own reader states which keys it reads and what their absence
# means.
SERDE_FLATTEN = re.compile(r"(?:^|[(,])\s*flatten\s*[,)]")
FIELD_NAME = re.compile(r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?(\w+)\s*:")
# The declared field type, read only for the one question the absent-key rule
# asks: is this key optional? A key whose writer omits it has an absence to
# spell whatever its write-side metadata says.
OPTION_FIELD = re.compile(r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?\w+\s*:\s*Option\s*<")
# Every field of a struct body, attributed or not, with its optionality.
FIELD_PREFIX = (
    r"(?:\s*(?:#\s*\[(?:[^\[\]]|\[(?:[^\[\]]|\[[^\[\]]*\])*\])*\]|//[^\n]*))*"
)
FIELD_DECLARATION = re.compile(
    r"(?:^|[,{])" + FIELD_PREFIX + r"\s*"
    r"(?:pub(?:\s*\([^)]*\))?\s+)?(\w+)\s*:\s*(Option\s*<)?"
)
# A container that reads through another type, reads transparently, or has no
# key of its own is not read field by field. Its fields' write-side metadata
# states nothing about any reader.
ROUTED_CONTAINER_KEYS = {"from", "try_from", "transparent", "untagged"}


def absent_key_source_files():
    """Every non-test Rust source file under the workspace's crates."""

    def fail(error):
        raise error

    base = Path(ABSENT_KEY_ROOT)
    if not stat.S_ISDIR(base.stat().st_mode):
        raise NotADirectoryError(base)
    for directory, children, files in os.walk(base, onerror=fail):
        children[:] = sorted(name for name in children if name not in SKIP_DIRS)
        for name in sorted(files):
            if not name.endswith(".rs") or name in SKIP_BASENAMES:
                continue
            path = Path(directory) / name
            if "src" not in path.parts:
                continue
            yield path


def field_attribute_runs(body):
    """Each field's whole attribute run, as (offset, text, field name)."""
    attributes = list(ANY_ATTRIBUTE.finditer(body))
    runs = []
    index = 0
    while index < len(attributes):
        end = index
        while (
            end + 1 < len(attributes)
            and ATTRIBUTE_GAP.match(body, attributes[end].end(), attributes[end + 1].start())
        ):
            end += 1
        field = FIELD_NAME.match(body, attributes[end].end())
        if field is not None:
            runs.append((
                attributes[index].start(),
                body[attributes[index].start():attributes[end].end()],
                field.group(1),
                OPTION_FIELD.match(body, attributes[end].end()) is not None,
            ))
        index = end + 1
    return runs


def struct_fields(body):
    """Each field's attribute run, name and optionality; unattributed too."""
    attributed = {
        name: (offset, text)
        for offset, text, name, _ in field_attribute_runs(body)
    }
    seen = set()
    for match in FIELD_DECLARATION.finditer(body):
        name = match.group(1)
        if name in seen:
            continue
        seen.add(name)
        offset, text = attributed.get(name, (match.start(1), ""))
        yield offset, text, name, match.group(2) is not None


def module_of(path, scope):
    """One declaration's module path from its crate root."""
    src = path.parts.index("src")
    parts = path.parts[src + 1:]
    module = parts[:-1] + (
        () if path.name in {"lib.rs", "main.rs", "mod.rs"} else (path.stem,)
    )
    return module + tuple(name for name in scope if not name.startswith("@"))


def macro_invocation_key(raw, source, opening):
    """The key literal one ``named_optional_field!`` invocation states.

    The arguments are read from the raw source: ``production_source`` masks
    string literals, and the key is one.
    """
    end = delimited_end(source, opening, "(", ")")
    if end is None:
        return None, None
    name = DECLARED_READER_NAME.match(raw, opening + 1)
    if name is None:
        return None, None
    depth = 0
    last = opening + 1
    for index in range(opening + 1, end - 1):
        character = source[index]
        if character in "(<[":
            depth += 1
        elif character in ")>]":
            depth -= 1
        elif character == "," and depth == 0:
            last = index + 1
    literal = raw[last:end - 1].strip()
    if len(literal) < 2 or literal[0] != '"' or literal[-1] != '"':
        return name.group(1), None
    return name.group(1), literal[1:-1]


def present_forwarders():
    """Every ``named_optional_field!`` declaration, and the macro's own failures.

    The macro is the one sanctioned declaration. A hand-written function that
    calls ``absent_key::named_present`` itself states the key as an argument
    the census cannot tie to the field, so it is not a declaration here. The
    declarations are keyed by crate and reader name, and each carries the file
    and module it was written in and the key literal it states, so a field is
    resolved to the declaration its own module reaches rather than to a name.
    """
    forwarders = {}
    failures = []
    sources = {}
    macro_crates = set()
    for path in absent_key_source_files():
        raw = path.read_text(encoding="utf-8")
        source, _ = SOURCE_POLICY.production_source(raw)
        sources[path] = (raw, source)
        definition = PRESENT_FORWARDER_DEFINITION.search(source)
        if definition is None:
            continue
        crate = Path(*path.parts[:path.parts.index("src")])
        macro_crates.add(crate)
        if (crate.name, path.name) != PRESENT_FORWARDER_OWNER:
            failures.append((
                path,
                raw.count("\n", 0, definition.start()) + 1,
                "named_optional_field: this file defines a macro of that name; "
                "the one declaration macro is "
                "cadmpeg_core::named_optional_field!",
            ))
    for path, (raw, source) in sources.items():
        crate = Path(*path.parts[:path.parts.index("src")])
        offsets, scopes = lexical_scopes(source)
        declarations = forwarders.setdefault(crate, {})
        for match in PRESENT_FORWARDER_MACRO.finditer(source):
            qualifier = match.group(1)
            if qualifier is None:
                continue
            if qualifier == "crate" and crate not in macro_crates:
                continue
            name, key = macro_invocation_key(raw, source, match.end() - 1)
            if name is None:
                continue
            scope = scopes[bisect_right(offsets, match.start()) - 1]
            declarations.setdefault(name, []).append(
                (path, module_of(path, scope), key)
            )
    return forwarders, failures


def file_imports(path):
    """The module path each ``use`` declaration of one file names."""
    source, _ = SOURCE_POLICY.production_source(path.read_text(encoding="utf-8"))
    bindings = {}
    for match in USE_DECLARATION.finditer(source):
        text = "".join(match.group(1).split())
        head, brace, rest = text.partition("{")
        if brace and ("{" in rest or "}" not in rest):
            # A nested import tree states more than one prefix. The census
            # reads none of it rather than the wrong one.
            continue
        prefix = tuple(part for part in head.split("::") if part)
        names = rest.rsplit("}", 1)[0].split(",") if brace else prefix[-1:]
        if not brace:
            prefix = prefix[:-1]
        for name in names:
            if name and "as" not in name.split("::") and "*" not in name:
                bindings[name.rsplit("::", 1)[-1]] = prefix
    return bindings


def reader_module(owner_module, prefix):
    """The module a reader path names, read from the field's own module."""
    if not prefix:
        return owner_module
    if prefix[0] == "crate":
        return tuple(prefix[1:])
    if prefix[0] == "self":
        return owner_module + tuple(prefix[1:])
    if prefix[0] == "super":
        depth = 0
        while depth < len(prefix) and prefix[depth] == "super":
            depth += 1
        kept = owner_module[:max(len(owner_module) - depth, 0)]
        return kept + tuple(prefix[depth:])
    return owner_module + tuple(prefix)


def declared_reader(declarations, path, item, name, text):
    """Why the readers one optional key names are no declaration, or None.

    ``(True, None)`` states the field names a declaration whose key literal is
    the field's own serde key. ``(False, reason)`` states a named reader that
    resolves to no declaration, to more than one, or to one that declares
    another key. ``(False, None)`` states a field that names no reader.
    """
    named = DESERIALIZE_WITH.findall(text)
    if not named:
        return False, None
    rename = SERDE_RENAME.search(text)
    key = rename.group(1) if rename is not None else name
    owner_module = tuple(
        part for part in item.scope if not part.startswith("@")
    )
    reasons = []
    for reader in named:
        segments = [part for part in reader.split("::") if part]
        if not segments:
            continue
        candidates = declarations.get(segments[-1], ())
        prefix = tuple(segments[:-1])
        if prefix:
            wanted = reader_module(owner_module, prefix)
            resolved = [
                candidate for candidate in candidates if candidate[1] == wanted
            ]
            if not resolved:
                resolved = [
                    candidate for candidate in candidates
                    if candidate[1][len(candidate[1]) - len(prefix):] == prefix
                ]
        else:
            resolved = [
                candidate for candidate in candidates if candidate[0] == path
            ]
            if not resolved:
                imported = file_imports(path).get(segments[-1])
                if imported is not None:
                    wanted = reader_module(owner_module, imported)
                    resolved = [
                        candidate for candidate in candidates
                        if candidate[1] == wanted
                    ]
        if not resolved:
            reasons.append(
                f"the reader `{reader}` is no declaration this module reaches; "
                "declare it with cadmpeg_core::named_optional_field! in the "
                "field's own file"
            )
            continue
        if len(resolved) > 1:
            where = ", ".join(sorted(
                f"{candidate[0].as_posix()}::{'::'.join(candidate[1])}"
                for candidate in resolved
            ))
            reasons.append(
                f"the reader `{reader}` resolves to {len(resolved)} "
                f"declarations ({where}); name the one that declares the key"
            )
            continue
        declared = resolved[0][2]
        if declared != key:
            reasons.append(
                f"the reader `{reader}` declares the key `{declared}`, and "
                f"this field's key is `{key}`"
            )
            continue
        return True, None
    return False, (reasons[0] if reasons else None)


def crate_sources():
    """Every in-scope source file grouped by its owning crate."""
    crates = {}
    for path in absent_key_source_files():
        crate = Path(*path.parts[:path.parts.index("src")])
        crates.setdefault(crate, []).append(path)
    return crates


def flatten_readers(item):
    """(field, reader) for each flattened field that names its own reader."""
    for _, text, name, _ in field_attribute_runs(item.body):
        if not SERDE_FLATTEN.search(text):
            continue
        for reader in DESERIALIZE_WITH.findall(text) + SERDE_WITH.findall(text):
            yield name, reader


def reader_modules(segments, crate_paths, crate_items):
    """Structs grouped by the module a reader path resolves to.

    The whole path is read, not its last segment: two modules of one name in
    one crate are told apart by the segments before them. A module's key is
    its file and its scope, so every struct it declares groups together.
    """
    groups = {}
    width = len(segments)
    for path in crate_paths:
        for item in crate_items[path]:
            if item.kind != "struct":
                continue
            scope = item.scope
            for index in range(len(scope), width - 1, -1):
                if scope[index - width:index] == segments:
                    groups.setdefault((path, scope[:index]), []).append(
                        (path, item)
                    )
                    break
    return groups


FUNCTION_GENERICS = re.compile(r"<([^<>]*)>")
FIELD_TYPE = re.compile(r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?(\w+)\s*:\s*([^,}]+)")
OPTION_TYPE = re.compile(r"Option\s*<\s*(.+)>\s*$", re.S)
TYPE_HEAD = re.compile(r"(?:\w+\s*::\s*)*(\w+)")


def field_type_name(body, field):
    """The named field's declared type, unwrapped from one ``Option``."""
    for match in FIELD_TYPE.finditer(body):
        if match.group(1) != field:
            continue
        text = match.group(2).strip()
        option = OPTION_TYPE.match(text)
        if option is not None:
            text = option.group(1).strip()
        head = TYPE_HEAD.match(text)
        if head is not None:
            return head.group(1)
    return None


def reader_functions(last, crate_paths, crate_items, owner, field):
    """The structs a named reader function reads.

    A function contributes the structs declared in its body and the struct it
    deserializes by name. A function that deserializes one of its own type
    parameters forwards the field's declared type, so that type is the struct
    it reads.
    """
    functions = []
    for path in crate_paths:
        source = path.read_text(encoding="utf-8")
        code, _ = SOURCE_POLICY.production_source(source)
        for match in re.finditer(r"\bfn\s+" + re.escape(last) + r"\s*[<(]", code):
            opening = body_open(code, match.end() - 1)
            if opening is None:
                continue
            end = delimited_end(code, opening)
            if end is None:
                continue
            body = code[opening:end]
            read = set(BODY_DESERIALIZE_CALL.findall(body))
            generics = FUNCTION_GENERICS.match(code, match.end() - 1)
            parameters = {
                parameter.split(":", 1)[0].strip()
                for parameter in (generics.group(1).split(",") if generics else ())
                if not parameter.strip().startswith("'")
            }
            if read & parameters:
                forwarded = field_type_name(owner.body, field)
                if forwarded is not None:
                    read.add(forwarded)
            for candidate_path in crate_paths:
                for item in crate_items[candidate_path]:
                    if item.kind != "struct":
                        continue
                    inside = (candidate_path == path
                              and opening <= item.start < end)
                    if inside or item.name in read:
                        functions.append((candidate_path, item))
    return functions


def reader_structs(reader, crate_paths, crate_items, owner, field):
    """The structs a flattened field's named reader reads, and its failures.

    The reader must resolve to exactly one module or function. One that
    resolves to no struct, and one that resolves to more than one module the
    field's own file cannot tell apart, are both failures: a census that
    follows nothing states nothing, and one that follows the wrong module
    states the wrong keys.
    """
    def failure(message):
        return [(owner.path, owner.line, f"{owner.name}.{field}: {message}")]

    segments = tuple(part for part in reader.split("::")
                     if part not in ("crate", "super", "self"))
    if not segments:
        return [], failure(f"the flattened reader `{reader}` names no path")
    groups = reader_modules(segments, crate_paths, crate_items)
    if len(groups) > 1:
        for narrower in (
            {key: items for key, items in groups.items() if key[0] == owner.path},
            {
                key: items
                for key, items in groups.items()
                if key[0].parent == owner.path.parent
            },
        ):
            if len(narrower) == 1:
                groups = narrower
                break
    if len(groups) == 1:
        return next(iter(groups.values())), []
    if groups:
        named = ", ".join(sorted(
            f"{path.as_posix()}::{'::'.join(scope)}" for path, scope in groups
        ))
        return [], failure(
            f"the flattened reader `{reader}` resolves to {len(groups)} modules "
            f"({named}); name the one that reads the key"
        )
    functions = reader_functions(
        segments[-1], crate_paths, crate_items, owner, field
    )
    if functions:
        return functions, []
    return [], failure(
        f"the flattened reader `{reader}` resolves to no struct; the census "
        "cannot read the keys it admits"
    )


BODY_DESERIALIZE_CALL = re.compile(r"\b(\w+)\s*(?:::\s*<[^<>]*>)?\s*::\s*deserialize\b")


def flattened_reader_failures(crate, crate_paths, crate_items, declarations):
    """Every optional key a flattened field's own reader reads undeclared."""
    found = []
    seen = set()
    for path in crate_paths:
        for owner in crate_items[path]:
            if owner.kind != "struct" or not derives_deserialize(owner.attrs):
                continue
            for field, reader in flatten_readers(owner):
                structs, failures = reader_structs(
                    reader, crate_paths, crate_items, owner, field
                )
                found.extend(failures)
                for read_path, item in structs:
                    key = (read_path, item.start)
                    if key in seen:
                        continue
                    seen.add(key)
                    # A routed container reads no key of its own: the type it
                    # names does, and that type is read where it is declared.
                    if any(
                        routed in ROUTED_CONTAINER_KEYS
                        for routed, _, _ in serde_options(item.attrs)
                    ):
                        continue
                    found.extend(
                        reader_option_failures(
                            read_path, item, owner, field, reader,
                            declarations.get(crate, {}),
                        )
                    )
    return found


def reader_option_failures(path, item, owner, field, reader, declarations):
    """Optional keys of one reader struct that state no absence spelling."""
    found = []
    for offset, text, name, is_option in struct_fields(item.body):
        if not is_option or SERDE_FLATTEN.search(text):
            continue
        if f"{path.as_posix()}:{name}" in ABSENT_KEY_EXCEPTIONS:
            continue
        declared, reason = declared_reader(declarations, path, item, name, text)
        if SERDE_DEFAULT.search(text):
            if declared:
                continue
            spelling = reason or (
                "an omitted optional key states no absence spelling; declare it "
                "with cadmpeg_core::named_optional_field! beside serde(default)"
            )
        else:
            if ABSENT_KEY_NULLABLE.search(text) or declared:
                continue
            spelling = reason or (
                "a stated optional key states no absence spelling; read it "
                "with cadmpeg_core::absent_key::nullable, or declare it with "
                "cadmpeg_core::named_optional_field! beside serde(default)"
            )
        found.append((
            path,
            item.line + item.body.count("\n", 0, offset),
            f"{item.name}.{name}: {reader} reads this key for "
            f"{owner.name}.{field}; {spelling}",
        ))
    return found


def absent_key_failures():
    """Optional keys whose writer omits them but whose reader admits ``null``."""
    found = []
    declarations, found_macro = present_forwarders()
    found.extend(found_macro)
    crates = crate_sources()
    crate_items = {}
    for crate, crate_paths in crates.items():
        for path in crate_paths:
            try:
                crate_items[path] = collect_items(path)
            except ValueError:
                crate_items[path] = []
    for crate, crate_paths in crates.items():
        found.extend(
            flattened_reader_failures(crate, crate_paths, crate_items, declarations)
        )
    for path in absent_key_source_files():
        crate = Path(*path.parts[:path.parts.index("src")])
        try:
            items = collect_items(path)
        except ValueError:
            # The unknown-key census raises on the same file and names it.
            continue
        for item in items:
            if item.kind != "struct" or not derives_deserialize(item.attrs):
                continue
            if any(
                key in ROUTED_CONTAINER_KEYS
                for key, _, _ in serde_options(item.attrs)
            ):
                continue
            # The body is read as written: a serde attribute is metadata, and
            # the lexer that masks string literals would erase the very
            # spellings this rule reads.
            if f"{path.as_posix()}:{item.name}" in ABSENT_KEY_PROJECTIONS:
                if derives_serialize(item.attrs):
                    found.append((
                        path,
                        item.line,
                        f"{item.name}: a projection admission states no writer, "
                        "but this item derives Serialize and writes its own keys",
                    ))
                continue
            for offset, text, name, is_option in field_attribute_runs(item.body):
                omitted = OMITTED_OPTIONAL_KEY.search(text) is not None
                # The rule reads the declared type, so a hand-written
                # serializer that omits keys without stating
                # ``skip_serializing_if`` cannot hide an optional key.
                if not (is_option or omitted):
                    continue
                if SERDE_FLATTEN.search(text):
                    continue
                if f"{path.as_posix()}:{name}" in ABSENT_KEY_EXCEPTIONS:
                    continue
                if not SERDE_DEFAULT.search(text):
                    # Without ``serde(default)`` serde names an absent key, so
                    # the writer states the key for every value and ``null`` is
                    # its own spelling. Only a stated omission is a defect.
                    if omitted:
                        found.append((
                            path,
                            item.line + item.body.count("\n", 0, offset),
                            f"{item.name}.{name}: an omitted optional key states no "
                            "serde(default), so its absence is no spelling at all",
                        ))
                    continue
                admitted, reason = declared_reader(
                    declarations.get(crate, {}), path, item, name, text
                )
                if admitted:
                    continue
                found.append((
                    path,
                    item.line + item.body.count("\n", 0, offset),
                    f"{item.name}.{name}: " + (reason or (
                        "an omitted optional key states no absence spelling; "
                        "declare it with cadmpeg_core::named_optional_field! "
                        "beside serde(default)"
                    )),
                ))
    return found


def main():
    paths = list(source_files())
    raw_items = {path: collect_items(path) for path in paths}

    # The id macro is declared in ids.rs and invoked from several sibling
    # modules. Gather templates first so qualified invocations are expanded
    # from the same source contract as local invocations.
    templates = {}
    deserializer_contracts = {}
    for path in paths:
        for name, template in macro_templates(path, raw_items[path]).items():
            if name not in templates:
                templates[name] = template
            elif templates[name] is None or template is None:
                templates[name] = None
            elif (
                templates[name].path != template.path
                or templates[name].start != template.start
            ):
                # Two definitions with the same recognized macro name do not
                # supply a deterministic expansion proof.
                templates[name] = None
        for name, contract in deserializer_macro_contracts(path).items():
            if name not in deserializer_contracts:
                deserializer_contracts[name] = contract
            elif deserializer_contracts[name] == contract:
                # Identical definitions have the same forwarding contract.
                # Keep that source proof available to their call sites.
                deserializer_contracts[name] = contract
            else:
                # A valid and an invalid definition with the same name must
                # not let the valid one certify the invalid one.
                deserializer_contracts[name] = None

    index = {}
    order = []
    readers = {}
    helper_functions = {}
    imports = {}
    helper_forwarders = set()
    helper_forwarder_sources = {}
    helper_targets = {}
    helper_target_sources = {}
    for path in paths:
        items = [item for item in raw_items[path] if not item.name.startswith("$")]
        generated = macro_generated_items(path, raw_items[path], templates)
        for item in items + generated:
            order.append(item)
            index.setdefault(item.name, []).append(item)
        for reader in collect_deserialize_impls(path):
            readers.setdefault((path, reader.name), []).append(reader)
        for function in collect_deserialize_functions(path):
            helper_functions.setdefault(function.name, []).append(function)
        for binding in collect_imports(path):
            imports.setdefault(binding.alias, []).append(binding)
        forwarders, named = deserializer_helpers(path, deserializer_contracts)
        helper_forwarders.update(forwarders)
        for name in forwarders:
            helper_forwarder_sources.setdefault(name, set()).add(path)
        for name, target in named.items():
            helper_target_sources.setdefault(name, set()).add(path)
            if name in helper_targets and helper_targets[name] != target:
                helper_targets[name] = None
            else:
                helper_targets[name] = target

    failures = []
    arm_failures = []
    ambiguities = []
    checking = set()

    import_missing = object()
    import_ambiguous = object()

    def import_visible(binding, owner, position, source_path=None,
                       source_scope=None):
        source_path = owner.path if source_path is None else source_path
        source_scope = owner.scope if source_scope is None else source_scope
        if binding.path != source_path:
            return binding.scope == source_scope
        if position is None:
            call_scope = source_scope
        else:
            scope_index = bisect_right(binding.offsets, position) - 1
            if scope_index < 0:
                return False
            call_scope = binding.module_scope + binding.scopes[scope_index]
        if call_scope == binding.scope:
            return True
        if not call_scope[:len(binding.scope)] == binding.scope:
            return False
        # A module import is visible inside functions and blocks in that
        # module. Child named modules have their own lexical namespace and do
        # not inherit the parent's bare imports.
        return all(part.startswith("@") for part in call_scope[len(binding.scope):])

    def imported_path(name, owner, position=None, source_path=None,
                      source_scope=None, seen=frozenset()):
        """Follow lexical bindings before any external-reader shortcut."""
        if name.startswith("::"):
            return import_missing
        prefix, separator, suffix = name.partition("::")
        source_path = owner.path if source_path is None else source_path
        source_scope = owner.scope if source_scope is None else source_scope
        source_crate = Path(*source_path.parts[:source_path.parts.index("src")])
        candidates = [
            binding for binding in imports.get(prefix, ())
            if binding.path.parts[:binding.path.parts.index("src")] == source_crate.parts
            and import_visible(
                binding,
                owner,
                position,
                source_path,
                source_scope,
            )
        ]
        if candidates:
            # A block-local use shadows an outer module binding. Same-scope
            # duplicates remain ambiguous because Rust would reject them or
            # require resolution information this source census does not have.
            deepest = max(len(binding.scope) for binding in candidates)
            candidates = [
                binding for binding in candidates
                if len(binding.scope) == deepest
            ]
        if len(candidates) == 1:
            binding = candidates[0]
            if binding in seen:
                return import_ambiguous
            target = binding.target + (f"::{suffix}" if separator else "")
            resolved = imported_path(
                target, owner, source_path=binding.path,
                source_scope=binding.scope, seen=seen | {binding},
            )
            return target if resolved is import_missing else resolved
        if len(candidates) > 1:
            return import_ambiguous
        if any(
            binding.path.parts[:binding.path.parts.index("src")] == source_crate.parts
            and import_visible(binding, owner, position, source_path, source_scope)
            for binding in imports.get("*", ())
        ):
            # A wildcard can introduce an alias absent from the declaration
            # index, including a name that otherwise looks like a prelude type.
            return import_ambiguous
        return import_missing

    def resolve_reader_target(name, owner, reader, position=None):
        """Resolve a call, preferring a local wire declared in its body."""
        imported = imported_path(name, owner, position)
        if imported is import_ambiguous:
            return None
        if imported is not import_missing:
            name = imported
        if "::" not in name:
            local = [
                candidate for candidate in index.get(name, ())
                if candidate.path == reader.path
                and reader.body_start <= candidate.start < reader.end
            ]
            if len(local) == 1:
                return local[0]
        return resolve_item(index, name, owner, ambiguities)

    def helper_source_applies(sources, owner):
        """Apply a macro-generated helper only from its unique module owner."""
        if len(sources) != 1:
            return False
        source = next(iter(sources))
        if source == owner.path:
            return True
        if source.name in {"lib.rs", "mod.rs"}:
            return owner.path.parent == source.parent or source.parent in owner.path.parents
        return source.parent / source.stem == owner.path.parent

    def helper_forwarder_applies(name, owner):
        return helper_source_applies(helper_forwarder_sources.get(name, set()), owner)

    def helper_target_applies(name, owner):
        return helper_source_applies(helper_target_sources.get(name, set()), owner)

    def resolve_helper_functions(name, owner):
        """Resolve a custom helper by its complete Rust module path."""
        path = canonical_path(name).lstrip(":")
        imported = imported_path(path, owner)
        if imported is import_ambiguous:
            return []
        if imported is not import_missing:
            path = canonical_path(imported).lstrip(":")
        parts = path.split("::")
        helper_name = parts[-1]
        candidates = helper_functions.get(helper_name, ())
        if not candidates:
            return []

        if len(parts) == 1:
            local = [
                function for function in candidates
                if function.path == owner.path and function.scope == owner.scope
            ]
            if local:
                return local
            same_file = [function for function in candidates if function.path == owner.path]
            if len(same_file) == 1:
                return same_file
            if len(candidates) == 1:
                return list(candidates)
            return []

        owner_scope = tuple(part for part in owner.scope if not part.startswith("@"))
        crate = owner.crate
        prefix = parts[:-1]
        if prefix[0] == "crate":
            scope = ()
            prefix = prefix[1:]
        elif prefix[0] == "self":
            scope = owner_scope
            prefix = prefix[1:]
        elif prefix[0] == "super":
            scope = owner_scope
            while prefix and prefix[0] == "super":
                scope = scope[:-1]
                prefix = prefix[1:]
        else:
            external = [
                function for function in candidates
                if function.path.parts[function.path.parts.index("src") - 1]
                == prefix[0].replace("-", "_")
            ]
            if external:
                source_index = external[0].path.parts.index("src")
                crate = Path(*external[0].path.parts[:source_index])
                prefix = prefix[1:]
                scope = ()
            else:
                scope = owner_scope

        wanted = tuple(prefix)
        if scope:
            wanted = scope + wanted
        return [
            function for function in candidates
            if function.path.parts[:function.path.parts.index("src")] == crate.parts
            and function.scope == wanted
        ]

    def helper_function_passes(function, owner, allow_free_form=False):
        """Prove a custom helper from the direct reader it invokes."""
        context = SourceContext(function.path, function.scope)
        code = SOURCE_POLICY.mask_rust_non_code(function.body)
        if OPEN_READER_ROUTE.search(code):
            return False
        bindings = function.input_bindings
        if has_unproved_input_route(code, bindings):
            return False
        array_route = any(
            call_has_direct_input(code, call, bindings)
            for call in ARRAY_DESERIALIZE_CALL_RE.finditer(code)
        )
        calls = [
            call for call in DESERIALIZE_CALL_RE.finditer(code)
            if call_has_direct_input(code, call, bindings)
        ]
        if not calls and not array_route:
            return False
        for call in calls:
            path = re.sub(r"\s*::\s*", "::", call.group("path"))
            path = path.replace("$crate", "crate")
            imported = imported_path(
                path,
                context,
                function.start + call.start(),
                function.path,
                function.scope,
            )
            if imported is import_ambiguous:
                return False
            if imported is not import_missing:
                path = imported
            base = path.rsplit("::", 1)[-1]
            if path in {"Self", "self"}:
                return False
            # A local declaration can shadow a prelude or conventional
            # container name. Resolve that declaration before applying the
            # built-in reader contract; otherwise `struct Vec<T>` or a
            # custom `String` reader could be admitted by its spelling.
            if "::" not in path and base in index:
                target = target_for_type(path, context)
                if target is None or not passes(target):
                    return False
                continue
            # A handwritten map/JSON reader is object-open. It must be
            # carried by an explicit free-form payload type, not hidden in a
            # custom wrapper behind a denying enum.
            if is_keyless_path(path, index):
                continue
            if is_free_form_map(path) or is_builtin_path(path, MAP_TYPES, index):
                if allow_free_form:
                    continue
                return False
            generic = call.group("args")
            if generic is not None:
                if is_builtin_path(path, SEQUENCE_TYPES, index):
                    continue
                ok, _ = payload_proof(f"{path}<{generic}>", context)
                if not ok:
                    return False
                continue
            target = target_for_type(path, context)
            if target is None or not passes(target):
                return False
        return True

    def manual_reader_passes(item, reader):
        """Prove a handwritten reader from every direct input route it uses."""
        code = SOURCE_POLICY.mask_rust_non_code(reader.body)
        if OPEN_READER_ROUTE.search(code):
            return False
        if has_unproved_input_route(code, reader.input_bindings):
            return False

        # This spelling is an array reader whose leading ``<`` is not a type
        # path token accepted by the call regex below.
        array_route = any(
            call_has_direct_input(code, call, reader.input_bindings)
            for call in ARRAY_DESERIALIZE_CALL_RE.finditer(code)
        )
        calls = [
            call for call in DESERIALIZE_CALL_RE.finditer(code)
            if call_has_direct_input(code, call, reader.input_bindings)
        ]
        if not calls and not array_route:
            return False
        for call in calls:
            path = re.sub(r"\s*::\s*", "::", call.group("path"))
            path = path.replace("$crate", "crate")
            imported = imported_path(path, item, reader.start + call.start())
            if imported is import_ambiguous:
                return False
            if imported is not import_missing:
                path = imported
            base = path.rsplit("::", 1)[-1]
            if path in {"Self", "self"}:
                return False
            if "::" not in path and base in index:
                target = resolve_reader_target(
                    path, item, reader, reader.start + call.start()
                )
                if target is None or not passes(target):
                    return False
                continue
            if is_free_form_map(path) or is_builtin_path(path, MAP_TYPES, index):
                return False
            if is_keyless_path(path, index):
                continue
            generic = call.group("args")
            if generic is not None:
                if is_builtin_path(path, SEQUENCE_TYPES, index):
                    continue
                ok, _ = payload_proof(f"{path}<{generic}>", item)
                if not ok:
                    return False
                continue
            target = resolve_reader_target(
                path, item, reader, reader.start + call.start()
            )
            if target is None:
                return False
            if not passes(target):
                return False
        return True

    def target_for_type(path, owner):
        path = path.replace("$crate", "crate")
        imported = imported_path(path, owner)
        if imported is import_ambiguous:
            return None
        if imported is not import_missing:
            path = imported
        return resolve_item(index, path, owner, ambiguities)

    def generic_parameter_proof(owner, parameter, position, default):
        """Prove every type the workspace can put in one generic parameter.

        A generic parameter names no reader of its own. The set that can
        reach it is the declared default plus the argument every workspace
        declaration states at this position; each is proved against the
        declaration that spells it, so its own imports resolve it. An empty
        set, or one argument this census cannot resolve, keeps the parameter
        unproved — the parameter is never called safe because its other
        instantiations are.
        """
        candidates = [] if default is None else [(default, owner)]
        candidates.extend(generic_arguments(owner.name, position, order))
        if not candidates:
            return False, (
                f"generic parameter {parameter} has no default and no "
                f"workspace instantiation"
            )
        for candidate, source in candidates:
            argument, _ = type_parts(candidate)
            if argument is not None and "::" not in argument:
                if any(
                    name == argument for name, _ in generic_parameters(source)
                ):
                    return False, (
                        f"generic parameter {parameter} is instantiated with "
                        f"the generic parameter {argument} of {source.name}"
                    )
            ok, reason = payload_proof(candidate, source)
            if not ok:
                return False, (
                    f"generic parameter {parameter} instantiated as "
                    f"{candidate.strip()}: {reason}"
                )
        return True, (
            f"every instantiation of generic parameter {parameter} is checked"
        )

    def payload_proof(field, owner):
        """Prove that an enum payload cannot consume arbitrary object keys."""
        for flag in ("skip", "skip_deserializing"):
            if has_serde_flag(field, flag):
                return True, "field is skipped unconditionally"
            if has_serde_flag(field, flag, include_conditional=True):
                return False, f"conditional {flag} leaves the reader unproved"

        custom = [
            reader_type(value)
            for key, value, conditional in serde_options(field)
            if key == "deserialize_with" and not conditional
        ]
        if any(
            key == "deserialize_with" and conditional
            for key, _, conditional in serde_options(field)
        ):
            return False, "conditional deserialize_with leaves the reader unproved"

        path, args = type_parts(field)
        if path is None:
            return False, "unsupported payload type syntax"
        imported = imported_path(path, owner)
        if imported is import_ambiguous:
            return False, f"ambiguous imported payload reader {path}"
        resolved_path = path if imported is import_missing else imported

        if custom:
            if len(custom) != 1 or custom[0] is None:
                return False, "custom deserializer path is not a source proof"
            helper = canonical_path(custom[0])
            helper_name = helper.rsplit("::", 1)[-1]
            if helper_name in helper_targets:
                if not helper_target_applies(helper_name, owner):
                    return False, f"helper {helper} is outside its source module"
                target = helper_targets[helper_name]
                if target is None:
                    return False, f"helper {helper} has conflicting macro contracts"
                target_type, optional = target
                if optional:
                    target_type = f"Option<{target_type}>"
                return payload_proof(target_type, owner)
            elif helper in ABSENT_KEY_HELPERS or (
                "::" not in helper
                and helper_name in helper_forwarders
                and helper_forwarder_applies(helper_name, owner)
            ):
                # These helpers forward the field's own type to Deserialize.
                pass
            elif helper in LOCAL_ID_HELPERS:
                if not is_builtin_path(resolved_path, SCALAR_TYPES, index):
                    return False, f"helper {helper} has no scalar reader proof"
            elif helper in DISTINCT_MAP_HELPERS and is_builtin_path(
                resolved_path, MAP_TYPES, index
            ):
                return True, "distinct-key map is intentionally free-form"
            else:
                candidates = resolve_helper_functions(helper, owner)
                if len(candidates) != 1 or not helper_function_passes(
                    candidates[0], owner,
                    allow_free_form=(
                        is_free_form_map(resolved_path)
                        or is_builtin_path(resolved_path, MAP_TYPES, index)
                    ),
                ):
                    return False, f"custom deserializer {helper} has no forwarding proof"

        # Names in the Rust type namespace can shadow prelude and standard
        # container types. A local or uniquely imported declaration therefore
        # gets its own reader proof before any outer-type shortcut is used.
        base = resolved_path.rsplit("::", 1)[-1]
        if "::" not in resolved_path and base in index:
            target = target_for_type(path, owner)
            if target is None:
                return False, f"cannot resolve shadowed payload reader {path}"
            if not passes(target):
                return False, f"payload reader {path} lacks checked unknown-key refusal"
            return True, f"payload reader {path} is checked"

        if is_free_form_map(resolved_path):
            return True, "explicit free-form map/value"
        if path in {"[", "("}:
            return True, "array or tuple reader rejects object input"
        if is_keyless_path(resolved_path, index) or is_builtin_path(
            resolved_path, MAP_TYPES, index
        ):
            return True, "scalar, sequence, or map outer reader"
        if base == "PhantomData":
            return True, "phantom reader has no object payload"
        if base in OPTIONAL_TYPES or base in DELEGATING_TYPES:
            if args is None or len(args) != 1:
                return False, f"{base} payload has unsupported generic arity"
            return payload_proof(args[0], owner)
        if base == "Result":
            if args is None or len(args) != 2:
                return False, "Result payload has unsupported generic arity"
            results = [payload_proof(arg, owner) for arg in args]
            if all(ok for ok, _ in results):
                return True, "both Result readers are checked"
            return False, next(reason for ok, reason in results if not ok)
        if base == "Cow":
            if not args:
                return False, "Cow payload has no borrowed target"
            return payload_proof(args[-1], owner)

        target = target_for_type(path, owner)
        if target is None:
            if imported is not import_missing:
                return False, (
                    f"cannot resolve imported payload reader {path} "
                    f"({resolved_path})"
                )
            if "::" in path or path in index:
                return False, f"cannot resolve payload reader {path}"
            parameters = generic_parameters(owner)
            position = next(
                (
                    at
                    for at, (parameter, _) in enumerate(parameters)
                    if parameter == path
                ),
                None,
            )
            if position is not None:
                return generic_parameter_proof(
                    owner, path, position, parameters[position][1]
                )
            # A bare imported generic is outside this lexical census. It can
            # be an object reader, so fail closed instead of fabricating a
            # declaration or claiming a generic instantiation is safe.
            return False, "cannot resolve bare generic/import statically"
        if not passes(target):
            return False, f"payload reader {path} lacks checked unknown-key refusal"
        return True, f"payload reader {path} is checked"

    def check_tuple_payloads(item, arms):
        admitted = True
        for arm in arms:
            if arm.kind != "tuple":
                continue
            for field in arm.types:
                result, reason = payload_proof(field, item)
                if not result:
                    admitted = False
                    label = type_without_attributes(field) or "payload"
                    arm_failures.append(
                        (
                            item.path,
                            item.line,
                            f"{item.name}::{arm.name}: payload {label}: {reason}",
                        )
                    )
        return admitted

    def check_untagged_arms(item, arms):
        """Check each untagged payload; container deny reaches inline structs."""
        admitted = True
        for arm in arms:
            if arm.kind == "struct" and not has_serde_flag(item.attrs, "deny_unknown_fields"):
                arm_failures.append(
                    (
                        item.path,
                        item.line,
                        f"{item.name}::{arm.name}: an inline struct arm read "
                        "untagged needs container deny_unknown_fields",
                    )
                )
                admitted = False
            elif arm.kind == "tuple" and not check_tuple_payloads(item, [arm]):
                admitted = False
        return admitted

    def transparent_field(item):
        code = SOURCE_POLICY.mask_rust_non_code(item.body)
        opening = re.search(
            r"\bstruct\s+\$?\w+\s*(?:<[^{}]*>)?\s*(?P<delimiter>[({])",
            code,
        )
        if opening is None:
            return None
        left = opening.start("delimiter")
        delimiter = code[left]
        right = ")" if delimiter == "(" else "}"
        end = delimited_end(code, left, delimiter, right)
        if end is None:
            return None
        if delimiter == "(":
            fields = split_tuple_types(item.body[left + 1:end - 1])
            return fields[0] if len(fields) == 1 else None

        fields = [
            field for field in split_metadata(item.body[left + 1:end - 1])
            if strip_variant_attributes(field)
        ]
        if len(fields) != 1:
            return None
        field = fields[0]
        field_code = SOURCE_POLICY.mask_rust_non_code(field)
        cursor = 0
        while True:
            while cursor < len(field_code) and field_code[cursor].isspace():
                cursor += 1
            if SOURCE_POLICY.OUTER_ATTRIBUTE.match(field_code, cursor) is None:
                break
            attribute_end = SOURCE_POLICY.attribute_end(field_code, cursor)
            if attribute_end is None:
                return None
            cursor = attribute_end
        colon = field_code.find(":", cursor)
        if colon < 0:
            return None
        return (field[:cursor] + field[colon + 1:]).strip()

    def route_target(value, owner):
        target_name = reader_type(value)
        if not isinstance(target_name, str) or not target_name.strip():
            return False
        return payload_proof(target_name, owner)[0]

    def passes(item):
        if f"{item.path.as_posix()}:{item.name}" in EXCEPTIONS:
            return True
        identity = (item.path.as_posix(), item.start, item.name)
        if identity in checking:
            # A conversion/transparent cycle is not a finite key-refusal
            # proof. A directly denied container is already the boundary for
            # this recursive route; other conversion/transparent cycles have
            # no finite reader proof and remain rejected.
            if has_serde_flag(item.attrs, "deny_unknown_fields"):
                return True
            return False
        checking.add(identity)
        try:
            manual = readers.get((item.path, item.name), ())
            if manual:
                return all(manual_reader_passes(item, reader) for reader in manual)
            if item.kind == "type":
                target = alias_target(item)
                if target is None:
                    return False
                return payload_proof(target, item)[0]
            if not derives_deserialize(item.attrs) and not item.reader_proven:
                return False

            options = list(serde_options(item.attrs))
            if any(conditional and key in {
                "from", "try_from", "transparent", "untagged", "tag", "content"
            } for key, _, conditional in options):
                return False
            target = next((value for key, value, conditional in options
                           if key in {"from", "try_from"} and not conditional), None)
            if target is not None:
                return route_target(target, item)

            if has_serde_flag(item.attrs, "transparent"):
                field = transparent_field(item)
                if field is None:
                    return False
                return payload_proof(field, item)[0]

            if any(key == "tag" for key, _, _ in options) and not any(
                key == "content" for key, _, _ in options
            ):
                units = [arm for arm in untagged_arms(item)
                         if arm.untagged != "always"
                         and (arm.kind == "unit" or arm.skipped_newtype)]
                for arm in units:
                    arm_failures.append((
                        item.path, item.line,
                        f"{item.name}::{arm.name}: an internally tagged unit arm "
                        "ignores unknown keys even with deny_unknown_fields; "
                        "use an empty struct arm",
                    ))
                if units:
                    return False

            if has_serde_flag(item.attrs, "deny_unknown_fields"):
                if item.kind == "enum":
                    return check_tuple_payloads(item, untagged_arms(item))
                return True
            if has_serde_flag(item.attrs, "untagged"):
                return check_untagged_arms(item, untagged_arms(item))
            if is_uninhabited_enum(item):
                return True
            if all_unit_variants(item):
                return not any(key in {"tag", "content"} for key, _, _ in options)
            return False
        finally:
            checking.discard(identity)

    for item in order:
        if not (
            derives_deserialize(item.attrs)
            or item.reader_proven
        ):
            continue
        if not passes(item):
            failures.append((item.path, item.line, item.name))
        if item.kind == "enum" and not has_serde_flag(item.attrs, "untagged"):
            arms = [arm for arm in untagged_arms(item) if arm.untagged]
            if arms and not check_untagged_arms(item, arms):
                failures.append((item.path, item.line, item.name))

    failures.extend(absent_key_failures())

    for path, line, owner, name, candidates in sorted(set(ambiguities), key=str):
        locations = ", ".join(candidate.as_posix() for candidate in candidates)
        failures.append(
            (
                path,
                line,
                f"{owner}: cannot resolve type {name} uniquely in its scope; candidates: {locations or 'none'}",
            )
        )

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
            f"deny census: {len(reported)} item(s) lack checked unknown-key refusal",
            file=sys.stderr,
        )
        return 1
    print("deny census: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
