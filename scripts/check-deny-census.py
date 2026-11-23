#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check that every non-test deserializable item refuses an unknown wire key.

A document read must not silently accept a key no type declares. The golden
sweeps test shapes the goldens carry; this checker follows declared
read routes in the three wire crates. Explicit lexical imports are resolved
before applying the unique-declaration fallback for bare names; this is not a
complete Rust name-resolution proof. Visible unresolved wildcards and cyclic
bindings fail the static check. A qualified path names the module it spells:
the module's own declaration, or what its ``use`` declarations bring in under
that name, and never a same-named declaration in a descendant module. A path
that starts with a type parameter of its item is that parameter.
Declarations without attributes are included when following a read route.
Type aliases are followed to their targets; an alias with a ``where`` clause
fails the static check when used as a read route.

A type name is resolved by its module path, so same-named declarations in two
modules are each checked against their own declaration. A generic parameter is
proved over its default and every argument a declaration states for that one
declaration: a use is attributed by resolving its spelled path where it is
written, and a use that resolves to no one declaration fails the proof. A
payload spelled ``P::Name`` or ``<P as Trait>::Name`` for a type parameter
``P`` is proved through the one bound trait that declares ``Name``: every impl
of that trait in the wire crates states the type, and each is proved in the
impl's own module. An unreadable bound, an impl that states no readable type,
a type that depends on what fills the impl's own parameters, an impl anywhere
in the workspace outside the wire crates' sources, a call naming the trait to
a macro that spells ``impl``, and a trait without an impl fail the proof.

Every ``impl`` keyword in item position is a candidate, wherever it sits, and
the header parser proves it. A candidate the parser does not read, whose
tokens up to its body name the trait, fails the trait's proof with its file
and line: the census fails where its parser does not follow, and never drops.
A raw identifier ``r#Name`` is read as ``Name``, as rustc reads it.

A generic reader's parameter is proved over every instantiation the wire
sources write outside a function body: a declaration, a function signature, a
const, a static, a const block, an impl header, and what an impl or trait
body writes outside its functions. A parameter of an enclosing function,
trait or impl, passed bare, is filled where that item is used; one inside a
larger argument makes a type the census does not read, and fails. A
``macro_rules!`` body is read where it is written, so a use whose argument is
a fragment fails. An attribute that names the reader, and a call naming it to
a macro that can bind a name, leave its parameters unread; any other call is
read in place, where a bare argument nothing in scope declares is a parameter
the expansion binds. One unproved instantiation fails the reader, however many
proved ones it has.

A reader the census accepts by its path - ``cadmpeg_core::tag_only::read``,
the absent-key and distinct-key helpers, ``named_optional_field!``, a standard
or registry reader - is that reader only where the path's root binds its real
crate: the using crate's manifest names the workspace's own package (or the
registry crate) by that root, and no token of the using crate binds the root
to anything else. ``crate::`` names a reader only in the crate that owns it.

The proof is static: it reads what source tokens name. An impl or a use that
no token names where the census reads is outside it by design, and a runtime
sweep covers it: a proc-macro derive or attribute expansion, ``include!`` of a
file that is not a ``.rs`` source of the crate, a ``#[path]`` module, a module
in a skipped ``test_support`` or ``tests.rs`` file, a function body, and
another crate's use of a wire reader.

A second, independent rule runs over every crate: an optional key whose writer
omits it for ``None`` (``skip_serializing_if = "Option::is_none"``) must read
through a reader declared by ``cadmpeg_core::named_optional_field!``, so an
absent key is that key's one spelling of ``None``, ``null`` is refused, and the
refusal names the key it refuses. A key that states neither the ``default`` nor
such a reader is an undeclared key and is named here.

A third, independent rule reads every field of a deserializable item in the
three wire crates, a transparent newtype's and an enum payload's included: a
set or a map it reads (``BTreeSet``, ``HashSet``, ``IndexSet``, ``BTreeMap``,
``HashMap``, ``IndexMap``, ``serde_json::Map``, also through a type alias or
inside another type) reads through a ``cadmpeg_core::distinct_keys`` reader,
since serde's own readers take a set in any order, merge a repeated member,
and keep the last value of a repeated key (§2.5: a set is an array sorted by
value, a map with any key but a JSON string ``[key, value]`` pairs sorted by
key, and each refuses a repeat). A map whose key is a JSON string is an object
read by ``distinct_keys::btree_map``, which refuses a key stated twice and
admits the keys in any order: member order is not part of a JSON object's
value (RFC 8259 §4), as it is not of a struct's fields, and §2.5 states none.
A ``named_optional_field!`` shim is read as its wire type, and a helper that
forwards to one, or that calls a ``distinct_keys`` reader, as what it calls. A
hand-written ``Deserialize`` impl of a type holding a set or a map passes when
it calls a ``distinct_keys`` reader or reads through the derived reader of a
type the wire crates declare, which this rule reads in turn.

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
* the enum has no variant at all - it is uninhabited, no document can name a
  variant of it, and serde refuses every input before a key is read.

An enum whose variants hold no data, without a tag, ``untagged``, a
conversion or ``transparent``, is read from its variant's name. It passes when
it derives ``cadmpeg_derive::DeserializeName`` (bare under a ``use`` of that
path, or by the path itself, whose root binds the workspace's
``cadmpeg-derive``), which reads the name and nothing else. Such an enum that
derives serde's ``Deserialize`` fails, ``deny_unknown_fields`` or not: serde
also reads ``{"<variant>": null}`` as the variant, a second spelling.

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
import tomllib
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
    "crates/cadmpeg-codec-f3d/src/records/topology/face.rs:postlude_value":
        "the key states a scalar run, not an option: its reader reads a Vec<i32>, "
        "which refuses null, and reads the empty run as None",
    "crates/cadmpeg-codec-f3d/src/records/feature/scope.rs:previous_history_state_id_offset":
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

# Set or map fields admitted by declaring path and field label, each with the
# reason its reader admits a second spelling. An entry is interim: it names
# the gap whose ruling removes it.
SET_SPELLING_EXCEPTIONS = {
    "crates/cadmpeg-ir/src/pmi.rs:PmiDimension.modifiers":
        "interim (RULINGS-P P7-D5, gap 20261119-ERR1-C-I6-3): reading adds no "
        "refusal main lacks before errata item Q, and main read the modifiers "
        "as a list in any order, so the set reads every list main reads",
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

    def __init__(self, path, scope, line=0, name="a helper body"):
        self.path = path
        self.scope = scope
        self.line = line
        self.name = name
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

RAW_IDENTIFIER = re.compile(r"(?<![\w#])r#([A-Za-z_]\w*)")
# Strict and reserved keywords: written bare, each is the keyword.
RUST_KEYWORDS = {
    "abstract", "as", "async", "await", "become", "box", "break", "const",
    "continue", "crate", "do", "dyn", "else", "enum", "extern", "false",
    "final", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "macro",
    "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true",
    "try", "type", "typeof", "unsafe", "unsized", "use", "virtual", "where",
    "while", "yield",
}


def unraw(source):
    """Spell each raw identifier ``r#name`` as ``name``, the name rustc reads.

    A keyword keeps its prefix, since bare it would be the keyword.
    """
    return RAW_IDENTIFIER.sub(
        lambda raw: raw.group() if raw.group(1) in RUST_KEYWORDS else raw.group(1),
        source,
    )


@SOURCE_POLICY.cached_analysis
def read_source(path):
    """Read each file once within a census invocation, raw identifiers unraw."""
    return unraw(path.read_text(encoding="utf-8"))


@SOURCE_POLICY.cached_analysis
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


@SOURCE_POLICY.cached_analysis
def collect_items(path):
    source = read_source(path)
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
    source = read_source(path)
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
    source = read_source(path)
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
    source = read_source(path)
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


DECLARATION_MACROS = (
    "id_type|local_id_type|checked_scalar|checked_feature_geometry|checked_quantity"
)


def macro_templates(path, items):
    """Return recognized declaration macro templates found in ``path``."""
    source = read_source(path)
    code, _ = SOURCE_POLICY.production_source(source)
    templates = {}
    for definition in re.finditer(
        rf"\bmacro_rules\s*!\s*(?P<name>{DECLARATION_MACROS})\s*\{{",
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
            # The template derives its reader through an unconditional
            # conversion route; the generated declaration proves that route's
            # target like any other `try_from` declaration.
            "checked_quantity": bool(
                template is not None
                and derives_deserialize(template.attrs)
                and any(
                    key == "try_from" and not conditional
                    for key, _, conditional in serde_options(template.attrs)
                )
            ),
        }
        name = definition.group("name")
        if (
            template is not None
            and (name == "checked_quantity" or has_serde_flag(template.attrs, "transparent"))
            and body_contract[name]
        ):
            contract = template
        else:
            # Keep an unproved definition visible to the caller. A valid
            # definition with the same macro name in another module must not
            # silently certify this one.
            contract = None
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
    source = read_source(path)
    code, _ = SOURCE_POLICY.production_source(source)
    offsets, scopes = lexical_scopes(code)
    if templates is None:
        templates = macro_templates(path, items)
    generated = []
    call_re = re.compile(rf"\b(?P<macro>{DECLARATION_MACROS})\s*!\s*\(")
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
    source = read_source(path)
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


# The package that defines each forwarding macro, and the module a path
# through that package's root names it by.
DESERIALIZER_MACRO_OWNERS = {
    "selection_field_deserializer": ("cadmpeg-ir", "features"),
    "named_field": ("cadmpeg-ir", "units"),
    "named_optional_field": ("cadmpeg-core", "absent_key"),
}


def deserializer_helpers(path, contracts=None):
    """Collect local macro-generated deserializers with source proof.

    ``selection_field_deserializer!`` forwards to the selected type's
    ``Deserialize`` implementation. ``named_field!`` and
    ``named_optional_field!`` do the same through the units helper, with the
    declared target type visible at the call site. These are forwarding
    contracts in the macro bodies; names alone are never admitted.
    """
    source = read_source(path)
    code, _ = SOURCE_POLICY.production_source(source)
    if contracts is None:
        contracts = deserializer_macro_contracts(path)
    forwarders = set()
    named = {}
    call_re = re.compile(
        r"(?P<prefix>(?:::)?(?:[A-Za-z_$]\w*\s*::\s*)*)(?P<macro>"
        r"selection_field_deserializer|named_field|named_optional_field)\s*!\s*\("
    )
    crate = Path(*path.parts[:path.parts.index("src")])
    for call in call_re.finditer(code):
        end = delimited_end(code, call.end() - 1, "(", ")")
        if end is None:
            raise ValueError(f"{path}: incomplete deserializer macro at byte {call.start()}")
        owner, module = DESERIALIZER_MACRO_OWNERS[call.group("macro")]
        prefix = canonical_path(call.group("prefix"))
        if prefix:
            spelled = call.group("prefix") + call.group("macro")
            segments = canonical_path(spelled).lstrip(":").split("::")
            if reader_path_failure(
                spelled, crate, (owner,), module if len(segments) > 2 else None
            ) is not None:
                continue
        elif (manifest(crate) or {}).get("package", {}).get("name") != owner:
            continue
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
        if call.group("macro") == "named_optional_field":
            # `Wire as Value` reads `Wire` and converts it; `Wire` is the
            # reader that meets the document.
            target = wire_before_as(target)
        if not target:
            continue
        named[name] = (target, call.group("macro") == "named_optional_field")
    return forwarders, named


def wire_before_as(target):
    """The type a ``Wire as Value`` argument reads, or the argument itself."""
    depth = 0
    for match in re.finditer(r"[<>]|\s+as\s+", target):
        token = match.group(0)
        if token == "<":
            depth += 1
        elif token == ">":
            depth -= 1
        elif depth == 0:
            return target[:match.start()].strip()
    return target


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
    """Whether ``attrs`` derive serde's ``Deserialize`` or
    ``cadmpeg_derive::DeserializeTagged``, which reads the serde attributes of
    a tagged enum as serde's derive does."""
    for arguments, _ in attribute_arguments(attrs, "derive"):
        for name in split_metadata(arguments):
            code = SOURCE_POLICY.mask_rust_non_code(name).strip()
            if re.fullmatch(r"(?:::\s*)?(?:\w+\s*::\s*)*Deserialize(?:Tagged)?", code):
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
    def __init__(self, name, kind, types, untagged, skipped_newtype=False, tag_only=False,
                 tag_only_failure=None):
        self.name = name
        self.kind = kind
        self.types = types
        self.untagged = untagged
        self.skipped_newtype = skipped_newtype
        self.tag_only = tag_only
        # Why a path that spells the tag-only reader names another reader.
        self.tag_only_failure = tag_only_failure


# A reader the census accepts by its path is that crate's reader only where
# the path's root binds that crate. The using crate's manifest maps a
# wire-crate root to the workspace's own package of that name, a registry
# root to the registry package of that name, and a standard root to no
# dependency. A token of the using crate's sources that spells the root other
# than as a path's first segment can bind it to something else: a module, a
# `use` rename, an `extern crate .. as`, an item, a macro argument. A glob
# import from outside the crate can bring in a module of that name. Either
# leaves the path unproved. `crate::` names the reader only in the crate that
# is the workspace's package of it.
PATH_READER_PACKAGES = {
    "cadmpeg_core": "cadmpeg-core",
    "cadmpeg_derive": "cadmpeg-derive",
    "cadmpeg_ir": "cadmpeg-ir",
}
STANDARD_ROOTS = {"std", "core", "alloc"}
REGISTRY_ROOTS = {"serde_json", "serde_bytes", "indexmap"}
DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


@SOURCE_POLICY.cached_analysis
def manifest(directory):
    """The parsed ``Cargo.toml`` of ``directory``, or ``None``."""
    try:
        return tomllib.loads((Path(directory) / "Cargo.toml").read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return None


@SOURCE_POLICY.cached_analysis
def workspace_root(crate):
    """The nearest directory at or above ``crate`` whose manifest is a workspace."""
    here = Path(crate).resolve()
    for directory in (here, *here.parents):
        stated = manifest(directory)
        if stated is not None and "workspace" in stated:
            return directory
    return None


@SOURCE_POLICY.cached_analysis
def package_directory(crate, package):
    """The directory of the workspace's own ``package``, read from ``crate``'s
    workspace, or ``None``."""
    root = workspace_root(crate)
    if root is None:
        return None
    found = {
        (root / spec["path"]).resolve()
        for key, spec in manifest(root).get("workspace", {}).get("dependencies", {}).items()
        if isinstance(spec, dict) and spec.get("package", key) == package and "path" in spec
    }
    if len(found) != 1:
        return None
    directory = found.pop()
    if (manifest(directory) or {}).get("package", {}).get("name") != package:
        return None
    return directory


def dependency_source(crate, key, spec):
    """``(package, directory, registry)`` of one dependency entry, or ``None``.

    A ``workspace = true`` entry is read from the workspace's own table, and
    a path is resolved from the manifest that states it.
    """
    if isinstance(spec, str):
        return key, None, True
    if not isinstance(spec, dict):
        return None
    base = Path(crate).resolve()
    if spec.get("workspace") is True:
        root = workspace_root(crate)
        inherited = (
            manifest(root).get("workspace", {}).get("dependencies", {}).get(key)
            if root is not None else None
        )
        if isinstance(inherited, str):
            inherited = {"version": inherited}
        if not isinstance(inherited, dict):
            return None
        spec = {**inherited, **{name: value for name, value in spec.items() if name != "workspace"}}
        base = root
    package = spec.get("package", key)
    if "path" in spec:
        return package, (base / spec["path"]).resolve(), False
    return package, None, "git" not in spec


@SOURCE_POLICY.cached_analysis
def manifest_binding(crate, root):
    """Why ``crate``'s manifest does not bind ``root`` to its real crate, or ``None``."""
    stated = manifest(crate)
    where = Path(crate).as_posix()
    if stated is None:
        return f"{where} has no Cargo.toml the census reads, so {root} names no crate it can prove"
    tables = [stated.get(table, {}) for table in DEPENDENCY_TABLES]
    for target in stated.get("target", {}).values():
        tables += [target.get(table, {}) for table in DEPENDENCY_TABLES]
    sources = []
    for table in tables:
        for key, spec in table.items():
            if key.replace("-", "_") != root:
                continue
            source = dependency_source(crate, key, spec)
            if source is None:
                return f"the dependency {key} of {where} is no entry the census reads"
            sources.append(source)
    if root in STANDARD_ROOTS:
        return f"{where} names a dependency {root}, which shadows the standard crate" if sources else None
    if not sources:
        return f"{where} names no dependency {root}"
    for package, directory, registry in sources:
        if root in PATH_READER_PACKAGES:
            wanted = PATH_READER_PACKAGES[root]
            if package != wanted or directory is None or directory != package_directory(crate, wanted):
                return f"the dependency {root} of {where} is not the workspace's {wanted}"
        elif package.replace("-", "_") != root or not registry:
            return f"the dependency {root} of {where} is not the registry crate {root}"
    return None


@SOURCE_POLICY.cached_analysis
def crate_source_files(crate):
    """Every non-test Rust source under ``crate``'s ``src``."""
    files = []
    for directory, children, names in os.walk(Path(crate) / "src"):
        children[:] = sorted(name for name in children if name not in SKIP_DIRS)
        files.extend(
            Path(directory) / name for name in sorted(names)
            if name.endswith(".rs") and name not in SKIP_BASENAMES
        )
    return tuple(files)


@SOURCE_POLICY.cached_analysis
def root_shadowing(crate, root, absolute=False):
    """Where a token of ``crate``'s sources can bind ``root`` to anything but
    the crate its manifest names, or ``None``.

    A path's first segment and a ``use`` rename's original bind nothing. An
    ``absolute`` path starts at the extern prelude, which only ``extern crate
    .. as`` rebinds, so for it the census reads the ``as`` and every macro
    token tree that could spell one.
    """
    named = re.compile(rf"(?<![\w$]){re.escape(root)}(?![\w$])")
    files = crate_source_files(crate)
    modules = set()
    for path in files:
        code, _ = SOURCE_POLICY.production_source(read_source(path))
        modules.update(re.findall(r"(?<![\w$])mod\s+([A-Za-z_]\w*)", code))
    for path in files:
        source = read_source(path)
        code, _ = SOURCE_POLICY.production_source(source)
        trees = [(tree.start, tree.end) for tree in macro_token_trees(path)] if absolute else []
        for mention in named.finditer(code):
            before = code[max(0, mention.start() - 64):mention.start()].rstrip()
            after = code[mention.end():mention.end() + 64]
            head = re.match(r"\s*(?:::|as\b)", after) is not None
            if absolute:
                binds = re.search(r"(?<![\w$])as\Z", before) is not None or (
                    not head and any(start < mention.start() < end for start, end in trees)
                )
            else:
                binds = not head or (
                    before.endswith("::") and re.search(r"[\w$>]\Z", before[:-2].rstrip())
                )
            if binds:
                line = source.count("\n", 0, mention.start()) + 1
                return f"{path.as_posix()}:{line} spells {root} where it can bind another item"
        if absolute:
            continue
        for binding in collect_imports(path):
            if binding.alias != "*":
                continue
            head = binding.target.split("::", 1)[0]
            if binding.target.startswith("::") or head not in {"crate", "self", "super"} | modules:
                return (
                    f"{path.as_posix()} imports every name of {binding.target}, "
                    f"which can bring in a {root}"
                )
    return None


def extern_root_failure(crate, root, absolute=False):
    """Why ``root``, written in ``crate``, can name anything but its real crate."""
    return manifest_binding(crate, root) or root_shadowing(crate, root, absolute)


def module_declared(home, module):
    """Whether the crate root at ``home`` declares the module ``module``."""
    try:
        source = read_source(Path(home) / "src" / "lib.rs")
    except OSError:
        return False
    code, _ = SOURCE_POLICY.production_source(source)
    offsets, scopes = lexical_scopes(code)
    return any(
        not scopes[bisect_right(offsets, declared.start()) - 1]
        for declared in re.finditer(rf"(?<![\w$])mod\s+{re.escape(module)}\s*[;{{]", code)
    )


def reader_path_failure(value, crate, packages, module=None):
    """Why the path ``value``, written in ``crate``, names no reader of the
    workspace's ``packages``, or ``None``. A path through ``module`` needs the
    package's root to declare that module."""
    if "$crate" in value:
        return f"{value} names $crate, which a path in a string does not resolve"
    path = canonical_path(value)
    head = path.lstrip(":").split("::", 1)[0]
    package = PATH_READER_PACKAGES.get(head)
    if head == "crate" and not path.startswith("::"):
        stated = (manifest(crate) or {}).get("package", {}).get("name")
        if stated not in packages or package_directory(crate, stated) != Path(crate).resolve():
            return f"{path} is written outside {' and '.join(packages)}"
        home = Path(crate)
    elif package in packages:
        reason = extern_root_failure(crate, head, path.startswith("::"))
        if reason is not None:
            return reason
        home = package_directory(crate, package)
    else:
        return f"{path} names no root of {' or '.join(packages)}"
    if module is not None and not module_declared(home, module):
        return f"the root of {home.as_posix()} declares no module {module}"
    return None


def external_path_failure(path, crate):
    """Why a standard or registry path written in ``crate`` can name another
    item, or ``None`` for any other path."""
    if "::" not in path:
        return None
    root = path.lstrip(":").split("::", 1)[0]
    if root not in STANDARD_ROOTS | REGISTRY_ROOTS:
        return None
    return extern_root_failure(crate, root, path.startswith("::"))


# A unit arm read through `cadmpeg_core::tag_only::read` refuses every key
# beside its tag by name.
TAG_ONLY_READER = "cadmpeg_core::tag_only::read"
TAG_ONLY_OWNER = "cadmpeg-core"


def reads_by_tag_alone(raw, item):
    """Whether a unit arm's one unconditional reader is the tag-only reader,
    and why not when its path spells that reader."""
    readers = [(reader_type(value), conditional)
               for key, value, conditional in serde_options(raw)
               if key == "deserialize_with"]
    if len(readers) != 1:
        return False, None
    reader, conditional = readers[0]
    if conditional or reader is None:
        return False, None
    if canonical_path(reader).lstrip(":") not in {TAG_ONLY_READER, "crate::tag_only::read"}:
        return False, None
    reason = reader_path_failure(reader, item.crate, (TAG_ONLY_OWNER,), "tag_only")
    return reason is None, reason


# A data-free enum reads through the derive `cadmpeg_derive::DeserializeName`,
# which accepts the variant's name alone. serde's derive also reads the
# one-key object `{"<variant>": null}`, a second spelling of the same value.
NAME_READER = "cadmpeg_derive::DeserializeName"
NAME_READER_OWNER = "cadmpeg-derive"
# The container options that give an enum a wire form other than its
# variant's name.
NAMELESS_FORMS = {"tag", "content", "untagged", "from", "try_from", "transparent", "remote"}


def imported_path(item, name):
    """The one path ``name`` is imported from in the module of ``item``, or
    ``None``. A block between the item and its module, such as a
    ``macro_rules!`` body, sees the module's imports."""
    scope = item.scope
    while True:
        targets = {
            binding.target for binding in collect_imports(item.path)
            if binding.alias == name and binding.scope == scope
        }
        if targets or not scope or not scope[-1].startswith("@"):
            return targets.pop() if len(targets) == 1 else None
        scope = scope[:-1]


@SOURCE_POLICY.cached_analysis
def name_reader_derive(item):
    """``(derives, failure)``: whether ``item`` derives ``DeserializeName``,
    and why the derive is not ``cadmpeg_derive::DeserializeName`` reading a
    data-free enum by its variant's name, or ``None``."""
    derives = False
    for arguments, conditional in attribute_arguments(item.attrs, "derive"):
        for name in split_metadata(arguments):
            path = canonical_path(SOURCE_POLICY.mask_rust_non_code(name))
            if path.rsplit("::", 1)[-1] != "DeserializeName":
                continue
            derives = True
            if conditional:
                return True, "the derive is conditional"
            if "::" not in path:
                path = imported_path(item, path)
                if path is None:
                    return True, f"its scope imports no one {NAME_READER}"
            if canonical_path(path).lstrip(":") != NAME_READER:
                return True, f"{path} is not {NAME_READER}"
            reason = reader_path_failure(path, item.crate, (NAME_READER_OWNER,))
            if reason is not None:
                return True, reason
    if not derives:
        return False, None
    if item.kind != "enum" or not all_unit_variants(item):
        return True, "it reads an enum whose variants hold no data"
    forms = sorted({key for key, _, _ in serde_options(item.attrs)} & NAMELESS_FORMS)
    if forms:
        return True, f"serde {', '.join(forms)} gives the enum another wire form"
    return True, None


def name_form_failure(item):
    """Why ``item``, an enum read from its variant's name, does not read that
    name alone, or ``None``."""
    named, failure = name_reader_derive(item)
    if named:
        return None if failure is None else f"derives DeserializeName, but {failure}"
    if (
        item.kind == "enum"
        and derives_deserialize(item.attrs)
        and all_unit_variants(item)
        and not {key for key, _, _ in serde_options(item.attrs)} & NAMELESS_FORMS
    ):
        return (
            "serde's derived Deserialize reads a data-free enum from "
            f'{{"<variant>": null}} beside its name; derive {NAME_READER}'
        )
    return None


GENERIC_HEADER_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:struct|enum|type)\s+[A-Za-z_$]\w*\s*<"
)
IMPL_GENERIC_HEADER_RE = re.compile(r"^\s*impl\s*<")


def generic_parameters(item):
    """Return the item's declared type parameters as ``(name, default)``.

    An impl block's parameters are read from its ``impl<..>`` header. Lifetimes
    and const parameters cannot name a reader, so they are not returned. A
    header this parser cannot read returns an empty list, which keeps the
    caller on its fail-closed route.
    """
    header = IMPL_GENERIC_HEADER_RE if item.kind == "impl" else GENERIC_HEADER_RE
    opening = header.match(item.body)
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


def generic_arguments(declaration, position, items, resolve, values=(), names=None):
    """Return every argument a workspace declaration states at ``position``.

    Declarations are read, and each ``(value, context)`` of ``values``: a
    type an impl states for an associated type, read in that impl, and every
    type written outside a declaration, read where it is written (see
    [`written_type_uses`]). A use that supplies fewer arguments than
    ``position`` leaves the parameter on its default and contributes
    nothing. A use is spelled ``Name<..>`` or ``Name::<..>``, where ``Name``
    is the declaration's name or one of ``names``, the names a ``use``
    renames it to.

    A use is an argument for ``declaration`` only when ``resolve`` reads its
    spelled path, from the declaration that states it, as ``declaration``
    itself: a same-named type in another module is another parameter. The
    second list holds why each use whose path resolves to no one
    declaration, or whose arguments do not close, leaves its argument unread.
    """
    names = {declaration.name} | set(names or ())
    alternatives = "|".join(re.escape(name) for name in sorted(names))
    use = re.compile(rf"\b(?P<name>{alternatives})\s*(?:::\s*)?<")
    arguments = []
    unread = []
    sources = [(item.body, item, True) for item in items]
    sources += [(value, item, False) for value, item in values]
    for text, item, declared in sources:
        code = SOURCE_POLICY.mask_rust_non_code(text)
        header = GENERIC_HEADER_RE.match(code) if declared else None
        own_header = header.end() - 1 if header is not None else None
        for match in use.finditer(code):
            if match.end() - 1 == own_header:
                # The declaration's own header states parameters, not
                # arguments; its default is read by the caller.
                continue
            spelled = spelled_path(code, match.start(), match.group("name"))
            end = generic_end(code, match.end() - 1)
            if end is None:
                unread.append(
                    f"the use of {spelled} in {item.name} states arguments the census cannot read"
                )
                continue
            target = resolve(spelled, item)
            if target is None:
                unread.append(f"the use of {spelled} in {item.name} resolves to no one declaration")
                continue
            if target is not declaration:
                continue
            parts = split_tuple_types(text[match.end():end - 1])
            if position < len(parts):
                arguments.append((parts[position], item))
    return arguments, unread


FN_HEADER = re.compile(r"(?<![\w$])fn\s+([A-Za-z_]\w*)\s*")
TRAIT_HEADER = re.compile(r"(?<![\w$])trait\s+([A-Za-z_]\w*)\s*")


def signature_end(code, start):
    """The ``{`` or ``;`` that ends a signature from ``start``, outside its
    parentheses and brackets, or ``None``."""
    depth = 0
    for index in range(start, len(code)):
        char = code[index]
        if char in "([":
            depth += 1
        elif char in ")]" and depth:
            depth -= 1
        elif char in "{;" and not depth:
            return index
    return None


def header_parameters(code, cursor):
    """The parameter names of a ``<..>`` list at ``cursor``, lifetimes left
    out, or ``[]`` when none opens there."""
    if code[cursor:cursor + 1] != "<":
        return []
    end = generic_end(code, cursor)
    if end is None:
        return []
    names = []
    for part in split_tuple_types(code[cursor + 1:end - 1]):
        if part.startswith("'"):
            continue
        names.append(re.split(r"[:=]", re.sub(r"^const\s+", "", part), maxsplit=1)[0].strip())
    return names


def path_start(code, start):
    """Where the path whose last segment starts at ``start`` begins."""
    cursor = start
    while True:
        stripped = code[:cursor].rstrip()
        if not stripped.endswith("::"):
            return cursor
        before = stripped[:-2].rstrip()
        segment = re.search(r"[A-Za-z_$]\w*$", before[-256:])
        if segment is None:
            return len(stripped) - 2
        cursor = len(before) - len(segment.group())


@SOURCE_POLICY.cached_analysis
def type_contexts(path):
    """One file's production code with every function body and every
    declaration the item walk reads blanked, the span of each function
    signature, trait and impl as ``(start, end, label, parameters)``, and the
    span of each function body.

    ``parameters`` is ``None`` for an impl whose header the parser does not
    read, so what it can name is unread.
    """
    source = read_source(path)
    code, _ = SOURCE_POLICY.production_source(source)
    blanked = list(code)

    def clear(start, end):
        for index in range(start, end):
            if blanked[index] != "\n":
                blanked[index] = " "

    spans = []
    bodies = []
    for item in collect_items(path):
        clear(item.start, item.end)
    for header in FN_HEADER.finditer(code):
        stop = signature_end(code, header.end())
        if stop is None:
            continue
        spans.append((header.start(), stop, f"fn {header.group(1)}",
                      header_parameters(code, header.end())))
        if code[stop] == "{":
            end = delimited_end(code, stop)
            bodies.append((stop, len(code) if end is None else end))
            clear(*bodies[-1])
    for header in TRAIT_HEADER.finditer(code):
        opening = body_open(code, header.end())
        end = None if opening is None else delimited_end(code, opening)
        spans.append((header.start(), len(code) if end is None else end,
                      f"trait {header.group(1)}", header_parameters(code, header.end())))
    for keyword in IMPL_KEYWORD.finditer(code):
        start = keyword.start()
        if type_position(code, start):
            continue
        header = impl_header(code, start)
        if header[0] == "rejected":
            spans.append((start, header[1], "an impl the census does not parse", None))
            continue
        brace = header[1] if header[0] == "inherent" else header[3]
        end = delimited_end(code, brace)
        parameters = generic_parameters(Item(path, 0, "impl", "impl", "", code[start:brace]))
        spans.append((start, len(code) if end is None else end, "impl",
                      [name for name, _ in parameters]))
    return "".join(blanked), spans, bodies


@SOURCE_POLICY.cached_analysis
def written_type_uses(path, names):
    """Each use of one of ``names`` with arguments that one file writes
    outside a declaration the item walk reads and outside a function body,
    as ``(text, context)``.

    A function signature (its parameters, its return and its bounds), a
    const, a static, a const block, an impl header and what an impl or trait
    body writes outside its functions each instantiate what they spell: serde
    monomorphizes ``Name<Arg>`` for that item. ``text`` is the use from its
    path's first segment. ``context`` resolves its paths where it is written
    and carries ``unread_parameters``: the parameters of each enclosing
    function, trait and impl, which no source the census reads fills, or
    ``None`` inside an impl header the parser does not read. Its
    ``in_call`` states that the use is an argument of a macro call, whose
    expansion can bind a parameter the call names.
    """
    source = read_source(path)
    code, spans, _ = type_contexts(path)
    offsets, scopes = lexical_scopes(SOURCE_POLICY.production_source(source)[0])
    calls = [(tree.start, tree.end) for tree in macro_token_trees(path) if tree.kind == "call"]
    use = re.compile(rf"\b(?:{'|'.join(re.escape(name) for name in names)})\s*(?:::\s*)?<")
    found = []
    for match in use.finditer(code):
        start = path_start(code, match.start())
        end = generic_end(code, match.end() - 1)
        enclosing = [span for span in spans if span[0] <= match.start() < span[1]]
        label = enclosing[-1][2] if enclosing else "an item"
        line = source.count("\n", 0, match.start()) + 1
        context = Item(
            path, line, "signature", f"{label} at {path.as_posix()}:{line}", "", "",
            scopes[bisect_right(offsets, match.start()) - 1], match.start(), match.start(),
        )
        context.unread_parameters = (
            None if any(span[3] is None for span in enclosing)
            else {name for span in enclosing for name in span[3]}
        )
        context.in_call = any(start < match.start() < end for start, end in calls)
        found.append((code[start:match.end() if end is None else end], context))
    return found


def spelled_path(code, start, name):
    """The whole path a use spells, read back from its final segment."""
    path = [name]
    cursor = start
    while True:
        prefix = code[:cursor].rstrip()
        if not prefix.endswith("::"):
            break
        prefix = prefix[:-2].rstrip()
        segment = re.search(r"[A-Za-z_$]\w*$", prefix[-256:])
        if segment is None:
            return "::" + "::".join(path)
        path.insert(0, segment.group())
        cursor = len(prefix) - len(segment.group())
    return canonical_path("::".join(path))


def split_top_level(text, separator):
    """Split masked type text at one separator outside every delimiter."""
    code = SOURCE_POLICY.mask_rust_non_code(text)
    depth = start = 0
    parts = []
    for index, char in enumerate(code):
        if char in "([{<":
            depth += 1
        elif char in ")]}>" and depth:
            depth -= 1
        elif char == separator and depth == 0:
            parts.append(text[start:index])
            start = index + 1
    parts.append(text[start:])
    return [part.strip() for part in parts if part.strip()]


def where_clause(code, start):
    """The where clause between a declaration's header and its body, if any."""
    depth = 0
    clause = None
    for index in range(start, len(code)):
        char = code[index]
        if char in "([<":
            depth += 1
        elif char in ")]>" and depth:
            depth -= 1
        elif char in "{;" and depth == 0:
            return None if clause is None else code[clause:index]
        elif (
            depth == 0
            and clause is None
            and code.startswith("where", index)
            and (index == 0 or not (code[index - 1].isalnum() or code[index - 1] == "_"))
            and not (code[index + 5:index + 6].isalnum() or code[index + 5:index + 6] == "_")
        ):
            clause = index + len("where")
    return None if clause is None else code[clause:]


def generic_bounds(item):
    """Return each type parameter's trait bounds as written paths.

    The header and the where clause are both read. A predicate on anything
    other than a bare parameter, a lifetime and a `?Sized` relaxation bound
    no associated type and are not returned.
    """
    code = SOURCE_POLICY.mask_rust_non_code(item.body)
    opening = GENERIC_HEADER_RE.match(code)
    if opening is None:
        return {}
    end = generic_end(code, opening.end() - 1)
    if end is None:
        return {}
    predicates = split_top_level(code[opening.end():end - 1], ",")
    clause = where_clause(code, end)
    if clause is not None:
        predicates.extend(split_top_level(clause, ","))
    bounds = {}
    for predicate in predicates:
        name, colon, rest = predicate.partition(":")
        name = name.strip()
        if not colon or not re.fullmatch(r"[A-Za-z_]\w*", name):
            continue
        rest = split_top_level(rest, "=")
        for bound in split_top_level(rest[0], "+") if rest else ():
            if bound.startswith(("'", "?")):
                continue
            path = re.match(r"(?:::)?(?:[A-Za-z_]\w*\s*::\s*)*[A-Za-z_]\w*", bound)
            bounds.setdefault(name, []).append(
                canonical_path(path.group()) if path else bound
            )
    return bounds


ASSOCIATED_TYPE = re.compile(
    r"\s*(?P<parameter>[A-Za-z_]\w*)\s*::\s*(?P<name>[A-Za-z_]\w*)\s*"
)
QUALIFIED_ASSOCIATED_TYPE = re.compile(
    r"\s*<\s*(?P<parameter>[A-Za-z_]\w*)\s+as\s+"
    r"(?P<trait>(?:::)?(?:[A-Za-z_]\w*\s*::\s*)*[A-Za-z_]\w*)\s*>"
    r"\s*::\s*(?P<name>[A-Za-z_]\w*)\s*"
)


def associated_type_path(text):
    """Read ``P::Name`` or ``<P as Trait>::Name`` as (P, trait or None, Name)."""
    code = type_without_attributes(text)
    qualified = QUALIFIED_ASSOCIATED_TYPE.fullmatch(code)
    if qualified is not None:
        return (
            qualified.group("parameter"),
            canonical_path(qualified.group("trait")),
            qualified.group("name"),
        )
    bare = ASSOCIATED_TYPE.fullmatch(code)
    if bare is not None:
        return bare.group("parameter"), None, bare.group("name")
    return None


# Standard and serde traits declare no associated type a payload can name.
# A bound that resolves to no workspace trait and is not one of these may
# declare one the census cannot read, which fails the proof.
TRAITS_WITHOUT_ASSOCIATED_TYPES = {
    "Clone", "Copy", "Debug", "Display", "Default", "PartialEq", "Eq",
    "PartialOrd", "Ord", "Hash", "Send", "Sync", "Sized", "Unpin",
    "Serialize", "Deserialize", "DeserializeOwned", "JsonSchema",
}
TRAIT_RE = re.compile(
    r"\b(?:pub(?:\s*\([^)]*\))?\s+)?(?:unsafe\s+)?trait\s+(?P<name>[A-Za-z_]\w*)"
)


@SOURCE_POLICY.cached_analysis
def collect_traits(path):
    """Every trait declaration of one file, as an item with its body."""
    source = read_source(path)
    code, _ = SOURCE_POLICY.production_source(source)
    offsets, scopes = lexical_scopes(code)
    traits = []
    for match in TRAIT_RE.finditer(code):
        brace = body_open(code, match.end())
        if brace is None:
            continue
        end = delimited_end(code, brace)
        if end is None:
            raise ValueError(f"{path}: incomplete trait at byte {match.start()}")
        traits.append(Item(
            path,
            source.count("\n", 0, match.start()) + 1,
            "trait",
            match.group("name"),
            "",
            source[match.start():end],
            scopes[bisect_right(offsets, match.start()) - 1],
            match.start(),
            end,
        ))
    return traits


def trait_associated_type(trait, name):
    """Whether ``trait`` declares the associated type ``name``.

    Returns ``"declared"``, ``None`` when the trait states no such type, or a
    reason the declaration is no proof: a default is a value no impl states,
    and a generic associated type takes arguments this census does not read.
    """
    code = SOURCE_POLICY.mask_rust_non_code(trait.body)
    brace = body_open(code, 0)
    end = None if brace is None else delimited_end(code, brace)
    if end is None:
        return f"the trait {trait.name} has no body the census can read"
    body = code[brace + 1:end - 1]
    depth = 0
    for index, char in enumerate(body):
        if char in "([{":
            depth += 1
        elif char in ")]}" and depth:
            depth -= 1
        if depth or not body.startswith("type", index):
            continue
        if index and (body[index - 1].isalnum() or body[index - 1] == "_"):
            continue
        match = re.compile(rf"type\s+{re.escape(name)}\b\s*").match(body, index)
        if match is None:
            continue
        if body.startswith("<", match.end()):
            return f"{trait.name}::{name} is a generic associated type"
        nested = 0
        for char in body[match.end():]:
            if char in "([{<":
                nested += 1
            elif char in ")]}>" and nested:
                nested -= 1
            elif char == "=" and not nested:
                return f"{trait.name}::{name} states a default"
            elif char == ";" and not nested:
                break
        return "declared"
    return None


class TraitImpl:
    """One ``impl Trait for Type`` block and the associated types it states."""

    def __init__(self, path, line, trait, self_type, context, definitions):
        self.path = path
        self.line = line
        self.trait = trait
        self.self_type = self_type
        # An item over the impl block, so its associated types resolve in the
        # impl's own module and imports.
        self.context = context
        # Associated type name -> each value the block states, or ``None``
        # for a definition this parser cannot read.
        self.definitions = definitions


IMPL_KEYWORD = re.compile(r"(?<![\w$])impl\b")
HEADER_WORD = re.compile(r"[A-Za-z_]\w*")
HEADER_CLOSES = {")": "(", "]": "[", "}": "{", ">": "<"}
TRAIT_PATH = re.compile(
    r"\s*(?P<path>(?:::)?(?:[A-Za-z_$]\w*\s*::\s*)*[A-Za-z_$]\w*)\s*(?:<.*>)?\s*",
    re.S,
)
TYPE_POSITION_WORD = re.compile(r"(?:(?<![\w$])(?:mut|as|dyn)|'[A-Za-z_]\w*)\Z")


def type_position(code, start):
    """Whether the ``impl`` at ``start`` writes a type, as in ``-> impl Trait``
    or ``x: impl Trait``, rather than an impl item.

    An item follows the start of a file, a ``;``, a brace, an attribute, a
    qualifier, a macro arm's ``=>`` or a repetition's ``$(``; a type follows
    a token that starts or separates types.
    """
    before = code[max(0, start - 64):start].rstrip()
    if before.endswith("=>"):
        return False
    if before.endswith("->") or TYPE_POSITION_WORD.search(before):
        return True
    if before.endswith("("):
        opener = before[:-1].rstrip()
        return not (opener.endswith("$") or opener.endswith("=>"))
    return before[-1:] in {":", "<", ",", "=", "&", "[", "|"}


def header_tokens(code, start):
    """Each token from ``start`` as ``(index, token, depth)``, ``depth`` being
    the delimiters open after it.

    A token is a word, a delimiter or one other character. An angle bracket
    delimits where a type is written: at the top level and inside another
    angle bracket, never inside a parenthesis, bracket or brace, so the
    ``<<`` of an array length and the ``>`` of a const block delimit nothing,
    and neither does the ``>`` of ``->``. A closer that matches no opener ends
    the walk with the token ``None``.
    """
    stack = []
    index = start
    while index < len(code):
        char = code[index]
        word = HEADER_WORD.match(code, index)
        if word is not None and not (
            index and (code[index - 1].isalnum() or code[index - 1] in "_$")
        ):
            yield index, word.group(), len(stack)
            index = word.end()
            continue
        angle = not stack or stack[-1] == "<"
        if char in "([{" or (char == "<" and angle):
            stack.append(char)
        elif char in ")]}" or (char == ">" and angle and code[index - 1:index] != "-"):
            if not stack or stack[-1] != HEADER_CLOSES[char]:
                yield index, None, len(stack)
                return
            stack.pop()
        yield index, char, len(stack)
        index += 1


def crude_header_end(code, start):
    """The first ``{`` or ``;`` after ``start`` outside parentheses and
    brackets, or the end of the code: the most an impl header can span when
    its delimiters do not balance."""
    depth = 0
    for index in range(start, len(code)):
        char = code[index]
        if char in "([":
            depth += 1
        elif char in ")]" and depth:
            depth -= 1
        elif char in "{;" and not depth:
            return index
    return len(code)


def impl_header(code, start):
    """Read the impl item whose ``impl`` keyword is at ``start``.

    Returns ``("trait", trait path, self type, brace, negative)`` for a trait
    impl, ``("inherent", brace)`` for an inherent impl, or ``("rejected",
    header end)`` for a header this parser does not read.
    """
    state = "start"
    trait_from = start + len("impl")
    for_at = where_at = brace = None
    negative = False
    for index, token, depth in header_tokens(code, trait_from):
        if token is None:
            break
        if token.isspace():
            continue
        if state == "start":
            state = "qualifiers"
            if token == "<":
                state = "generics"
                continue
        if state == "generics":
            if token == ">" and depth == 0:
                state = "qualifiers"
                trait_from = index + 1
            continue
        if state == "qualifiers":
            if token == "const":
                trait_from = index + len(token)
                continue
            state = "path"
            if token == "!":
                negative = True
                trait_from = index + 1
                continue
        if token == "{" and depth == 1:
            brace = index
            break
        if depth:
            continue
        if token == ";":
            break
        if token == "for" and for_at is None and where_at is None:
            for_at = index
        elif token == "where" and where_at is None:
            where_at = index
    if brace is None:
        return "rejected", crude_header_end(code, start)
    if for_at is None:
        return "inherent", brace
    trait = TRAIT_PATH.fullmatch(code, trait_from, for_at)
    if trait is None:
        return "rejected", brace
    self_type = code[for_at + len("for"):brace if where_at is None else where_at]
    return (
        "trait",
        canonical_path(trait.group("path")),
        " ".join(self_type.split()),
        brace,
        negative,
    )


def associated_type_definitions(code):
    """Each ``type Name = Value;`` at the top level of one impl body."""
    depth = 0
    depths = []
    for char in code:
        depths.append(depth)
        if char in "([{":
            depth += 1
        elif char in ")]}" and depth:
            depth -= 1
    definitions = {}
    for match in re.finditer(r"\btype\s+(?P<name>[A-Za-z_]\w*)\s*", code):
        if depths[match.start()] != 0:
            continue
        end = statement_end(code, match.end())
        value = None
        if end is not None and code[match.end():match.end() + 1] == "=":
            value = code[match.end() + 1:end - 1].strip() or None
        definitions.setdefault(match.group("name"), []).append(value)
    return definitions


@SOURCE_POLICY.cached_analysis
def impl_items(path):
    """Every impl item of one file's production code.

    Returns the trait impls the header parser reads, each as ``(start,
    brace, end, trait, self type)``, and the impls it does not read, each as
    ``(start, header end)``. A negative impl states no associated type and an
    inherent impl implements no trait, so neither is either.
    """
    source = read_source(path)
    code, _ = SOURCE_POLICY.production_source(source)
    blocks = []
    rejected = []
    for opening in IMPL_KEYWORD.finditer(code):
        start = opening.start()
        if type_position(code, start):
            continue
        header = impl_header(code, start)
        if header[0] == "rejected":
            rejected.append((start, header[1]))
            continue
        if header[0] == "inherent":
            continue
        _, trait, self_type, brace, negative = header
        end = delimited_end(code, brace)
        if end is None:
            rejected.append((start, brace))
        elif not negative:
            blocks.append((start, brace, end, trait, self_type))
    return blocks, rejected


def trait_impl_blocks(path):
    """Each trait impl of one file as ``(start, brace, end, trait, self type)``."""
    return impl_items(path)[0]


def rejected_impl_naming(path, names):
    """The first impl of ``path`` whose header the parser does not read and
    whose tokens name one of ``names``, as ``(line, name)``, or ``None``."""
    code, _ = SOURCE_POLICY.production_source(read_source(path))
    pattern = name_pattern(names)
    for start, end in impl_items(path)[1]:
        mention = pattern.search(code, start, end)
        if mention is not None:
            return read_source(path).count("\n", 0, start) + 1, mention.group()
    return None


@SOURCE_POLICY.cached_analysis
def collect_trait_impls(path):
    """Every trait impl of one file with the associated types it states."""
    source = read_source(path)
    code, _ = SOURCE_POLICY.production_source(source)
    offsets, scopes = lexical_scopes(code)
    impls = []
    for start, brace, end, trait, self_type in trait_impl_blocks(path):
        line = source.count("\n", 0, start) + 1
        context = Item(
            path, line, "impl", f"impl {trait} for {self_type}", "",
            source[start:end],
            scopes[bisect_right(offsets, start) - 1],
            start, end,
        )
        impls.append(TraitImpl(
            path, line, trait, self_type, context,
            associated_type_definitions(code[brace + 1:end - 1]),
        ))
    return impls


@SOURCE_POLICY.cached_analysis
def implementing_macros(path):
    """Each ``macro_rules!`` of one file whose body spells ``impl``.

    Its invocations can name the trait an expansion implements, so a call
    that names a trait leaves that trait's impls unread.
    """
    source = read_source(path)
    code, _ = SOURCE_POLICY.production_source(source)
    macros = []
    for definition in re.finditer(r"\bmacro_rules\s*!\s*([A-Za-z_]\w*)\s*\{", code):
        end = delimited_end(code, definition.end() - 1)
        if end is not None and re.search(r"\bimpl\b", code[definition.end():end]):
            macros.append(definition.group(1))
    return macros


class MacroTree:
    """One token tree a macro reads: a ``macro_rules!`` body, the input of a
    macro call, or an attribute, which a derive or attribute macro reads."""

    def __init__(self, path, kind, name, line, start, end):
        self.path = path
        self.kind = kind
        self.name = name
        self.line = line
        # Offsets of the tree's delimiters in the file's production code.
        self.start = start
        self.end = end

    def describe(self):
        where = f"{self.path.as_posix()}:{self.line}"
        if self.kind == "definition":
            return f"the macro_rules! {self.name} at {where}"
        if self.kind == "call":
            return f"the macro call at {where}"
        return f"the attribute at {where}"


MACRO_DEFINITION = re.compile(r"\bmacro_rules\s*!\s*([A-Za-z_]\w*)\s*([({\[])")
MACRO_CALL = re.compile(r"(?<![\w$])(\$?[A-Za-z_]\w*)\s*!\s*([({\[])")
ATTRIBUTE_OPEN = re.compile(r"#\s*!?\s*\[")
# Keywords that can stand before a `!` that negates a parenthesized operand.
NEGATING_KEYWORDS = {
    "if", "while", "match", "return", "in", "else", "let", "mut", "ref",
    "move", "as", "break", "yield", "where", "for", "loop", "unsafe",
}
CLOSING = {"(": ")", "{": "}", "[": "]"}


@SOURCE_POLICY.cached_analysis
def macro_token_trees(path):
    """Every token tree of one file's production code that a macro reads.

    A tree that does not close runs to the end of the file, so its tokens
    are still read.
    """
    source = read_source(path)
    code, _ = SOURCE_POLICY.production_source(source)
    trees = []

    def add(kind, name, at, opening, left, right):
        end = delimited_end(code, opening, left, right)
        trees.append(MacroTree(
            path, kind, name, source.count("\n", 0, at) + 1,
            opening, len(code) if end is None else end,
        ))

    for match in MACRO_DEFINITION.finditer(code):
        left = match.group(2)
        add("definition", match.group(1), match.start(), match.end() - 1, left, CLOSING[left])
    for match in MACRO_CALL.finditer(code):
        name = match.group(1)
        if name == "macro_rules" or name in NEGATING_KEYWORDS:
            continue
        left = match.group(2)
        add("call", name, match.start(), match.end() - 1, left, CLOSING[left])
    for match in ATTRIBUTE_OPEN.finditer(code):
        add("attribute", None, match.start(), match.end() - 1, "[", "]")
    return trees


# Standard macros that expand to an expression and declare no item of their own.
EXPRESSION_MACROS = {
    "assert", "assert_eq", "assert_ne", "cfg", "column", "compile_error",
    "concat", "dbg", "debug_assert", "debug_assert_eq", "debug_assert_ne",
    "env", "eprint", "eprintln", "file", "format", "format_args", "line",
    "matches", "module_path", "option_env", "panic", "print", "println",
    "stringify", "todo", "unimplemented", "unreachable", "vec", "write",
    "writeln",
}
# The keywords with which an expansion binds a name a type argument can spell.
BINDING_KEYWORD = re.compile(r"(?<![\w$])(?:struct|enum|union|type|use|mod|extern|trait)\b")


def binding_macro(name, definitions, seen=frozenset()):
    """Whether a call to the macro ``name`` can bind a name a type can spell.

    ``definitions`` maps a macro name to every ``macro_rules!`` tree of that
    name. A macro the census has no definition of, other than a standard
    expression macro, can write anything; one whose body spells a binding
    keyword, or calls a macro that can bind, binds.
    """
    if name not in definitions:
        return name not in EXPRESSION_MACROS
    for tree in definitions[name]:
        code, _ = SOURCE_POLICY.production_source(read_source(tree.path))
        if BINDING_KEYWORD.search(code, tree.start, tree.end):
            return True
        for nested in macro_token_trees(tree.path):
            if nested.kind != "call" or not tree.start < nested.start < tree.end:
                continue
            if nested.name.startswith("$"):
                return True
            if nested.name not in seen | {name} and binding_macro(
                nested.name, definitions, seen | {name}
            ):
                return True
    return False


def name_pattern(names):
    """Match any of ``names`` as a whole identifier."""
    alternatives = "|".join(re.escape(name) for name in sorted(names))
    return re.compile(rf"(?<![\w$])(?:{alternatives})\b")


@SOURCE_POLICY.cached_analysis
def use_renames(path):
    """Each ``original as alias`` a ``use`` declaration of one file states."""
    code, _ = SOURCE_POLICY.production_source(read_source(path))
    renames = []
    for match in re.finditer(r"\buse\b", code):
        end = code.find(";", match.end())
        tree = code[match.end():len(code) if end < 0 else end]
        for rename in re.finditer(r"(?<![\w$])([A-Za-z_]\w*)\s+as\s+([A-Za-z_]\w*)\b", tree):
            if rename.group(2) != "_":
                renames.append((rename.group(1), rename.group(2)))
    return renames


def renamed_names(name, paths):
    """``name`` and every name a ``use`` in ``paths`` renames it to, transitively.

    A rename of another declaration of the same name is included too: each
    use the extra name finds is still resolved, so it widens what is read and
    never proves anything.
    """
    names = {name}
    read = set()
    while True:
        pattern = name_pattern(names)
        pending = [
            path for path in paths
            if path not in read and pattern.search(read_source(path))
        ]
        if not pending:
            return names
        for path in pending:
            read.add(path)
            for original, alias in use_renames(path):
                if original in names:
                    names.add(alias)


def impl_header_names(path, offset, names):
    """Whether ``offset`` is the trait path of an impl header the census reads.

    That is a block whose trait's last segment is one of ``names``, with
    ``offset`` between its ``impl`` and its ``for``.
    """
    code, _ = SOURCE_POLICY.production_source(read_source(path))
    for start, brace, _, trait, _ in trait_impl_blocks(path):
        if (
            start < offset < brace
            and trait.rsplit("::", 1)[-1] in names
            and not re.search(r"\bfor\b", code[start:offset])
        ):
            return True
    return False


def macro_tree_naming(path, names, wire):
    """The first macro token tree of ``path`` that names one of ``names``.

    Returns ``(tree, name)`` or ``None``. A ``macro_rules!`` body in a wire
    crate may spell a literal ``impl Trait for $name`` header, which the
    census reads as an impl; every other spelling of a name in a token tree
    can expand to an impl the census does not read: a call can pass the name
    to a macro that implements it (renamed, wrapped by another macro, or from
    another crate), and a body can pass it on to another macro.
    """
    source = read_source(path)
    if not name_pattern(names).search(source):
        return None
    code, _ = SOURCE_POLICY.production_source(source)
    pattern = name_pattern(names)
    for tree in macro_token_trees(path):
        for mention in pattern.finditer(code, tree.start, tree.end):
            if (
                wire
                and tree.kind == "definition"
                and impl_header_names(path, mention.start(), names)
            ):
                continue
            return tree, mention.group()
    return None


def trait_impl_source_files():
    """Every non-test Rust source of the workspace outside the wire crates.

    Only an impl in a wire crate's ``src`` states a type the declaration
    index holds. An impl anywhere else - another crate, ``examples/``,
    ``benches/``, a build script - states one the census cannot prove.
    The build output and hidden directories at the workspace root hold no
    workspace source.
    """

    def fail(error):
        raise error

    roots = [Path(root) for root in ROOTS]
    for directory, children, files in os.walk(".", onerror=fail):
        children[:] = sorted(
            name for name in children
            if name not in SKIP_DIRS
            and not (directory == "." and (name == "target" or name.startswith(".")))
        )
        for name in sorted(files):
            if not name.endswith(".rs") or name in SKIP_BASENAMES:
                continue
            path = Path(directory) / name
            if not any(root in path.parents for root in roots):
                yield path


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
NONZERO_INTEGER_TYPES = {
    f"NonZero{sign}{width}"
    for sign in ("U", "I")
    for width in ("8", "16", "32", "64", "128", "size")
}
QUALIFIED_SCALAR_TYPES = {
    *{f"core::primitive::{name}" for name in SCALAR_TYPES if name not in {"String"}},
    *{f"std::primitive::{name}" for name in SCALAR_TYPES if name not in {"String"}},
    *{f"{root}::num::{name}" for root in ("core", "std") for name in NONZERO_INTEGER_TYPES},
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
# Every bare type name the standard prelude and this census's readers give a
# meaning without a declaration in scope.
PRELUDE_NAMES = (
    SCALAR_TYPES | SEQUENCE_TYPES | MAP_TYPES | OPTIONAL_TYPES | DELEGATING_TYPES
    | {"Self", "Result", "Cow", "PhantomData"}
)
ABSENT_KEY_HELPERS = {
    "crate::absent_key::nullable",
    "cadmpeg_core::absent_key::nullable",
}
DISTINCT_MAP_HELPERS = {
    "crate::distinct_keys::btree_map",
    "crate::distinct_keys::json_object",
    "cadmpeg_core::distinct_keys::btree_map",
    "cadmpeg_core::distinct_keys::json_object",
}
LOCAL_ID_HELPERS = {
    "crate::ids::deserialize_local_id",
}
# The packages whose module each helper and keyless adapter path names, and
# that module.
PATH_HELPER_OWNERS = {
    "crate::absent_key": (("cadmpeg-core",), "absent_key"),
    "cadmpeg_core::absent_key": (("cadmpeg-core",), "absent_key"),
    "crate::distinct_keys": (("cadmpeg-core",), "distinct_keys"),
    "cadmpeg_core::distinct_keys": (("cadmpeg-core",), "distinct_keys"),
    "crate::ids": (("cadmpeg-ir",), "ids"),
    "crate::bytes": (("cadmpeg-ir",), "bytes"),
    "cadmpeg_ir::bytes": (("cadmpeg-ir",), "bytes"),
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
            tag_only, failure = reads_by_tag_alone(raw, item)
            arms.append(Arm(name, "unit", [], untagged,
                            tag_only=tag_only, tag_only_failure=failure))
    return arms


def alias_target(item):
    """Read a type alias's target; a `where` clause needs explicit proof.

    A generic alias's parameters reach only its target's arguments, so the
    target's own generic-parameter proof follows them to every instantiation
    of the alias.
    """
    code = SOURCE_POLICY.mask_rust_non_code(item.body)
    header = GENERIC_HEADER_RE.match(code)
    if header is not None:
        end = generic_end(code, header.end() - 1)
        if end is None:
            return None
        code = code[:header.end() - 1] + code[end:]
    match = re.fullmatch(
        r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?type\s+\w+\s*=\s*(.*?)\s*;\s*",
        code,
        re.S,
    )
    return match.group(1) if match else None


def resolve_item(index, name, owner, ambiguities, reexports=None, seen=frozenset()):
    """Resolve qualified declarations and distinguish lexical item scopes.

    A bare name may have been imported from a different file. A unique
    declaration remains the census's imported-name fallback; duplicates require
    a declaration in the current lexical scope. Qualified paths never use that
    fallback. A qualified path names the module it spells: when that module
    declares no such item, ``reexports(index, crate, module, name, seen)``
    states what the module's own ``use`` declarations bring in under the
    name. A declaration in a descendant module is never the meaning of a
    shorter path. An unresolved qualified path fails closed.
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
        if not matches and reexports is not None:
            matches = reexports(index, crate, wanted, parts[-1], seen)
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
# A line comment consumes its newline or reaches the end of the gap. Repeated
# comment markers within that line cannot form separate matches.
ATTRIBUTE_GAP = re.compile(r"(?:\s|//[^\n]*(?:\n|\Z))*\Z")
OMITTED_OPTIONAL_KEY = re.compile(r'skip_serializing_if\s*=\s*"Option::is_none"')
DESERIALIZE_WITH = re.compile(r'deserialize_with\s*=\s*"([\w:]+)"')
SERDE_WITH = re.compile(r'(?<!\w)with\s*=\s*"([\w:]+)"')
ABSENT_KEY_NULLABLE = re.compile(
    r'deserialize_with\s*=\s*"(?P<path>(?:crate|cadmpeg_core)::absent_key::nullable)"'
)
# The one workspace macro that declares a named reader forwarding to
# ``absent_key::named_present``. Its expansion carries the same absence
# spelling and names the key in whatever it refuses. The macro path is read,
# not the bare name: a ``macro_rules! named_optional_field`` anywhere else
# expands to whatever it likes, so the census names that definition instead.
# The whole path is read, so a root it spells must bind the real crate.
PRESENT_FORWARDER_MACRO = re.compile(
    r"(?<![\w$])(?P<path>(?:::\s*)?(?:[A-Za-z_$]\w*\s*::\s*)*)named_optional_field\s*!\s*\("
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
    """The reader name, key literal and wire type one ``named_optional_field!``
    invocation states.

    The arguments are read from the raw source: ``production_source`` masks
    string literals, and the key is one. The wire type is the type the reader
    deserializes: the one before ``as`` in the ``Wire as Value`` form.
    """
    end = delimited_end(source, opening, "(", ")")
    if end is None:
        return None, None, None
    name = DECLARED_READER_NAME.match(raw, opening + 1)
    if name is None:
        return None, None, None
    depth = 0
    commas = []
    for index in range(opening + 1, end - 1):
        character = source[index]
        if character in "(<[":
            depth += 1
        elif character in ")>]":
            depth -= 1
        elif character == "," and depth == 0:
            commas.append(index)
    wire = None
    if len(commas) >= 2:
        wire = wire_before_as(raw[commas[-2] + 1:commas[-1]].strip())
    literal = raw[commas[-1] + 1 if commas else opening + 1:end - 1].strip()
    if len(literal) < 2 or literal[0] != '"' or literal[-1] != '"':
        return name.group(1), None, wire
    return name.group(1), literal[1:-1], wire


def present_forwarders(files=None):
    """Every ``named_optional_field!`` declaration of ``files`` (by default
    every crate's sources), and the macro's own failures.

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
    for path in absent_key_source_files() if files is None else files:
        raw = read_source(path)
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
            qualifier = canonical_path(match.group("path")).lstrip(":").split("::", 1)[0]
            if not qualifier:
                continue
            failure = reader_path_failure(
                match.group("path") + "named_optional_field", crate, ("cadmpeg-core",)
            )
            if failure is not None:
                failures.append((
                    path,
                    raw.count("\n", 0, match.start()) + 1,
                    f"named_optional_field!: {failure}",
                ))
                continue
            if qualifier == "crate" and crate not in macro_crates:
                continue
            name, key, wire = macro_invocation_key(raw, source, match.end() - 1)
            if name is None:
                continue
            scope = scopes[bisect_right(offsets, match.start()) - 1]
            declarations.setdefault(name, []).append(
                (path, module_of(path, scope), key, wire)
            )
    return forwarders, failures


@SOURCE_POLICY.cached_analysis
def file_imports(path):
    """The module path each ``use`` declaration of one file names."""
    source, _ = SOURCE_POLICY.production_source(read_source(path))
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


def reached_declarations(declarations, path, item, reader):
    """The ``named_optional_field!`` declarations a reader path reaches from
    the module of ``item`` in ``path``, or ``None`` for an empty path."""
    segments = [part for part in reader.split("::") if part]
    if not segments:
        return None
    owner_module = tuple(part for part in item.scope if not part.startswith("@"))
    candidates = declarations.get(segments[-1], ())
    prefix = tuple(segments[:-1])
    if prefix:
        wanted = reader_module(owner_module, prefix)
        return [candidate for candidate in candidates if candidate[1] == wanted]
    resolved = [candidate for candidate in candidates if candidate[0] == path]
    if not resolved:
        imported = file_imports(path).get(segments[-1])
        if imported is not None:
            wanted = reader_module(owner_module, imported)
            resolved = [candidate for candidate in candidates if candidate[1] == wanted]
    return resolved


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
    reasons = []
    for reader in named:
        resolved = reached_declarations(declarations, path, item, reader)
        if resolved is None:
            continue
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
    for declaration in FIELD_DECLARATION.finditer(body):
        if declaration.group(1) != field:
            continue
        match = FIELD_TYPE.match(body, declaration.start(1))
        if match is None:
            return None
        text = match.group(2).strip()
        option = OPTION_TYPE.match(text)
        if option is not None:
            text = option.group(1).strip()
        head = TYPE_HEAD.match(text)
        if head is not None:
            return head.group(1)
    return None


@SOURCE_POLICY.cached_analysis
def reader_function_index(path):
    source = read_source(path)
    code, _ = SOURCE_POLICY.production_source(source)
    functions = {}
    for match in re.finditer(r"\bfn\s+([^\s<(]+)\s*[<(]", code):
        opening = body_open(code, match.end() - 1)
        if opening is None:
            continue
        end = delimited_end(code, opening)
        if end is None:
            continue
        read = frozenset(BODY_DESERIALIZE_CALL.findall(code[opening:end]))
        generics = FUNCTION_GENERICS.match(code, match.end() - 1)
        parameters = frozenset(
            parameter.split(":", 1)[0].strip()
            for parameter in (generics.group(1).split(",") if generics else ())
            if not parameter.strip().startswith("'")
        )
        functions.setdefault(match.group(1), []).append((opening, end, read, parameters))
    return functions


def reader_functions(last, crate_paths, crate_items, owner, field):
    """Structs declared or deserialized by the named reader function.

    A reader that deserializes its own type parameter forwards the field type.
    """
    functions = []
    for path in crate_paths:
        for opening, end, declared, parameters in reader_function_index(path).get(last, ()):
            read = set(declared)
            if read & parameters:
                forwarded = field_type_name(owner.body, field)
                if forwarded is not None:
                    read.add(forwarded)
            for candidate_path in crate_paths:
                for item in crate_items[candidate_path]:
                    if item.kind != "struct":
                        continue
                    inside = candidate_path == path and opening <= item.start < end
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
            nullable = ABSENT_KEY_NULLABLE.search(text)
            if nullable is not None and not declared:
                reason = reader_path_failure(
                    nullable.group("path"), item.crate, ("cadmpeg-core",), "absent_key"
                )
                if reason is None:
                    continue
            if declared:
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


@SOURCE_POLICY.analysis_session
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


# The set and map types serde reads in any order. Its set reader merges a
# repeated member and its map reader keeps the last value of a repeated key, so
# each admits a second spelling of one value.
SET_TYPES = {"BTreeSet", "HashSet", "IndexSet"}
SPELLED_TYPES = SET_TYPES | MAP_TYPES
# The readers of `cadmpeg_core::distinct_keys` and what each reads: an array
# sorted by value, an array of `[key, value]` pairs sorted by key, and a JSON
# object that states each key once.
DISTINCT_READERS = {
    "set": "set",
    "set::deserialize": "set",
    "pairs": "pairs",
    "pairs::deserialize": "pairs",
    "btree_map": "object",
    "json_object": "object",
}
DISTINCT_READER_ROOTS = ("crate", "cadmpeg_core")
DISTINCT_READER_CALL = re.compile(
    r"(?<![\w$])(?:::\s*)?(?:crate|cadmpeg_core)\s*::\s*distinct_keys\s*::\s*"
    r"(?:set\s*::\s*deserialize|pairs\s*::\s*deserialize|btree_map|json_object)\b"
)
# Keys a JSON object cannot spell once: §2.5 writes a map with such a key as
# `[key, value]` pairs.
NON_STRING_KEYS = (SCALAR_TYPES - {"char", "str", "String"}) | NONZERO_INTEGER_TYPES
READER_CALL = re.compile(r"(?<![\w$:.])([A-Za-z_]\w*)\s*(?:::\s*<[^<>]*>\s*)?\(")
DEFAULT_SET_FIX = "read it with cadmpeg_core::distinct_keys::set"
DEFAULT_MAP_FIX = (
    "read it with cadmpeg_core::distinct_keys::btree_map (a key that is a JSON "
    "string) or cadmpeg_core::distinct_keys::pairs (any other key)"
)


def split_offsets(text):
    """``split_metadata`` with each part's offset in ``text``."""
    offset = 0
    for part in split_metadata(text):
        yield offset, part
        offset += len(part) + 1


def leading_attributes(raw):
    """``(attributes, rest)`` of one field or variant text."""
    code = SOURCE_POLICY.mask_rust_non_code(raw)
    cursor = 0
    while True:
        while cursor < len(code) and code[cursor].isspace():
            cursor += 1
        if SOURCE_POLICY.OUTER_ATTRIBUTE.match(code, cursor) is None:
            return raw[:cursor], raw[cursor:]
        end = SOURCE_POLICY.attribute_end(code, cursor)
        if end is None:
            raise ValueError("incomplete field attribute")
        cursor = end


VISIBILITY = re.compile(r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?")
NAMED_FIELD = re.compile(r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?(\w+)\s*:(.*)\Z", re.S)
VARIANT_NAME = re.compile(r"\s*(\$?\w+)\s*")


def delimited_fields(body, opening, label):
    """``(label, offset, attributes, type)`` of each field one ``(..)`` or
    ``{..}`` list at ``opening`` of ``body`` declares."""
    code = SOURCE_POLICY.mask_rust_non_code(body)
    right = ")" if code[opening] == "(" else "}"
    end = delimited_end(code, opening, code[opening], right)
    if end is None:
        return
    inner = body[opening + 1:end - 1]
    position = 0
    for offset, part in split_offsets(inner):
        attributes, rest = leading_attributes(part)
        if not SOURCE_POLICY.mask_rust_non_code(rest).strip():
            continue
        if right == ")":
            name = str(position)
            field_type = rest[VISIBILITY.match(rest).end():]
            position += 1
        else:
            named = NAMED_FIELD.match(rest)
            if named is None:
                continue
            name, field_type = named.group(1), named.group(2)
        yield f"{label}.{name}", opening + 1 + offset, attributes, field_type.strip()


def read_fields(item):
    """``(label, offset, attributes, type)`` of each field serde reads of a
    struct or enum item; a variant's own attributes join a newtype payload's."""
    if any(
        key in {"from", "try_from", "remote"}
        for key, _, _ in serde_options(item.attrs)
    ):
        return
    body = strip_macro_repetition(item.body) if "$(" in item.body else item.body
    code = SOURCE_POLICY.mask_rust_non_code(body)
    header = re.match(r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:struct|enum)\s+\$?\w+\s*", code)
    if header is None:
        return
    cursor = header.end()
    if cursor < len(code) and code[cursor] == "<":
        cursor = generic_end(code, cursor)
        if cursor is None:
            return
    opening = re.compile(r"[({;]").search(code, cursor)
    if opening is None or code[opening.start()] == ";":
        return
    if item.kind == "struct":
        yield from delimited_fields(body, opening.start(), item.name)
        return
    start = opening.start() + 1
    end = delimited_end(code, opening.start())
    if end is None:
        return
    for offset, variant in split_offsets(body[start:end - 1]):
        attributes, rest = leading_attributes(variant)
        if has_serde_flag(attributes, "skip") or has_serde_flag(attributes, "skip_deserializing"):
            continue
        name = VARIANT_NAME.match(rest)
        if name is None or not name.group(1):
            continue
        at = len(attributes) + name.end()
        if at >= len(variant) or variant[at] not in "({":
            continue
        fields = list(delimited_fields(variant, at, f"{item.name}::{name.group(1)}"))
        if variant[at] == "(" and len(fields) == 1:
            label, field_offset, field_attributes, field_type = fields[0]
            fields = [(label, field_offset, attributes + "\n" + field_attributes, field_type)]
        for label, field_offset, field_attributes, field_type in fields:
            yield label, start + offset + field_offset, field_attributes, field_type


class SetSpelling:
    """The aliases, readers and helper functions the set rule reads."""

    def __init__(self, paths, items, declarations):
        self.items = items
        self.declarations = declarations
        self.aliases = {}
        self.functions = {}
        self.declared = set()
        for path in paths:
            for item in items[path]:
                self.declared.add(item.name)
                # An associated type is no alias: its name is read through
                # the trait, never bare.
                if (
                    item.kind == "type"
                    and not item.name.startswith("$")
                    and not any(part.startswith("@") for part in item.scope)
                ):
                    target = alias_target(item)
                    if target is not None:
                        self.aliases.setdefault(item.name, []).append((item, target))
            for function in collect_deserialize_functions(path):
                self.functions.setdefault(function.name, []).append(function)

    def spelled(self, text, item, seen=frozenset()):
        """The set and map types serde's own reader reads in a type, through
        the crate's aliases it names bare or by a module path."""
        found = []
        code = SOURCE_POLICY.mask_rust_non_code(text)
        imports = file_imports(item.path)
        for match in re.finditer(r"(?<![\w$:])((?:\w+\s*::\s*)*)([A-Za-z_]\w*)", code):
            qualifier = canonical_path(match.group(1)).rstrip(":")
            name = match.group(2)
            root = qualifier.split("::", 1)[0]
            if name in SPELLED_TYPES:
                found.append(name)
            elif name == "Map" and (
                qualifier == "serde_json"
                or (not qualifier and imports.get("Map") == ("serde_json",))
            ):
                found.append("serde_json::Map")
            elif (
                name in self.aliases
                and name not in seen
                and (not qualifier or root.islower())
                and root not in REGISTRY_ROOTS | STANDARD_ROOTS
            ):
                for alias, target in self.aliases[name]:
                    if alias.crate == item.crate:
                        found.extend(self.spelled(target, alias, seen | {name}))
        return found

    def delegates(self, reader):
        """Whether a hand-written reader reads through the derived reader of
        a type the wire crates declare, which this rule checks in turn, and
        through no standard collection."""
        body = SOURCE_POLICY.mask_rust_non_code(reader.body)
        read = [
            canonical_path(match.group("path")).rsplit("::", 1)[-1]
            for match in DESERIALIZE_CALL_RE.finditer(body)
        ]
        return bool(read) and all(
            name in self.declared and name not in SPELLED_TYPES | SEQUENCE_TYPES
            for name in read
        )

    def outer(self, text, item):
        """The outer type and its arguments, through a nongeneric alias."""
        path, arguments = type_parts(text)
        name = path.rsplit("::", 1)[-1] if path else None
        if name in self.aliases and not arguments:
            targets = [target for alias, target in self.aliases[name]
                       if alias.crate == item.crate]
            if len(targets) == 1:
                return type_parts(targets[0])
        return path, arguments

    def distinct_reader(self, reader, item):
        """``(kind, failure)`` of a reader path naming a distinct-keys reader."""
        path = canonical_path(reader).lstrip(":")
        root, _, rest = path.partition("::")
        module, _, function = rest.partition("::")
        if root not in DISTINCT_READER_ROOTS or module != "distinct_keys":
            return None, None
        kind = DISTINCT_READERS.get(function)
        if kind is None:
            return None, None
        return kind, reader_path_failure(path, item.crate, ("cadmpeg-core",), "distinct_keys")

    def under_distinct(self, kind, field_type, item):
        """Why a field read by one distinct-keys reader has a second spelling."""
        path, arguments = self.outer(field_type, item)
        name = path.rsplit("::", 1)[-1] if path else None
        expected = SET_TYPES if kind == "set" else MAP_TYPES | {"Map"}
        if name not in expected or not arguments:
            return f"the distinct_keys::{kind} reader reads {field_type}, which the census cannot read"
        if kind == "object" and name != "Map":
            key_path, _ = type_parts(arguments[0])
            if key_path in {"[", "("} or (
                key_path and key_path.rsplit("::", 1)[-1] in NON_STRING_KEYS
            ):
                return (
                    f"a JSON object key {arguments[0]} is not a JSON string; §2.5 "
                    "writes such a map as [key, value] pairs: read it with "
                    "cadmpeg_core::distinct_keys::pairs"
                )
        for argument in arguments:
            failure = self.under_default(argument, item)
            if failure is not None:
                return failure
        return None

    def under_default(self, field_type, item):
        """Why serde's own reader of a type admits a second spelling."""
        spelled = self.spelled(field_type, item)
        if not spelled:
            return None
        name = spelled[0]
        if name in SET_TYPES:
            return (
                f"serde reads the {name} in {field_type.strip()} in any order and "
                f"merges a repeated member; {DEFAULT_SET_FIX}"
            )
        return (
            f"serde reads the {name} in {field_type.strip()} keeping the last "
            f"value of a repeated key; {DEFAULT_MAP_FIX}"
        )

    def helper(self, name, item):
        """The one function of ``name`` in the file of ``item``, else its
        directory, or ``None``."""
        candidates = [
            function for function in self.functions.get(name, ())
            if function.path == item.path
        ] or [
            function for function in self.functions.get(name, ())
            if function.path.parent == item.path.parent
        ]
        return candidates[0] if len(candidates) == 1 else None

    def forwarded(self, reader, item, seen=frozenset()):
        """Why a custom reader of a field holding a set or a map is not one
        this rule follows to a reader that reads one spelling, or ``None``."""
        declarations = self.declarations.get(item.crate, {})
        resolved = reached_declarations(declarations, item.path, item, reader)
        if resolved:
            if len(resolved) > 1:
                return f"the reader {reader} resolves to {len(resolved)} declarations"
            wire = resolved[0][3]
            if wire is None:
                return f"the reader {reader} states no wire type the census can read"
            return self.under_default(wire, item)
        name = canonical_path(reader).rsplit("::", 1)[-1]
        function = self.helper(name, item)
        if function is None or name in seen:
            return (
                f"the reader {reader} is no one function the census follows to a "
                "cadmpeg_core::distinct_keys reader"
            )
        body = SOURCE_POLICY.mask_rust_non_code(function.body)
        if DISTINCT_READER_CALL.search(body):
            return None
        calls = [
            call for call in dict.fromkeys(READER_CALL.findall(body))
            if call != name and (
                reached_declarations(declarations, item.path, item, call)
                or self.helper(call, item) is not None
            )
        ]
        if not calls:
            return (
                f"the reader {reader} calls no cadmpeg_core::distinct_keys reader "
                "and no reader the census follows"
            )
        for call in calls:
            failure = self.forwarded(call, item, seen | {name})
            if failure is not None:
                return failure
        return None

    def field_failure(self, item, attributes, field_type):
        """Why one field serde reads states a set or a map in two spellings."""
        if has_serde_flag(attributes, "skip") or has_serde_flag(attributes, "skip_deserializing"):
            return None
        readers = [
            (key, reader_type(value), conditional)
            for key, value, conditional in serde_options(attributes)
            if key in {"with", "deserialize_with"}
        ]
        if not readers:
            return self.under_default(field_type, item)
        if len(readers) != 1 or readers[0][2] or readers[0][1] is None:
            if self.spelled(field_type, item):
                return "a conditional or repeated reader leaves the set or map unproved"
            return None
        key, reader, _ = readers[0]
        kind, failure = self.distinct_reader(reader, item)
        if failure is not None:
            return failure
        if kind is not None:
            return self.under_distinct(kind, field_type, item)
        if canonical_path(reader) in ABSENT_KEY_HELPERS:
            return self.under_default(field_type, item)
        if not self.spelled(field_type, item):
            return None
        return self.forwarded(reader, item)


def set_spelling_lines(item):
    """The line of each offset in ``item.body``."""
    source = read_source(item.path)
    start = item.end - len(item.body)
    first = source.count("\n", 0, start) + 1
    return lambda offset: first + item.body.count("\n", 0, offset)


@SOURCE_POLICY.analysis_session
def set_spelling_failures():
    """Every set or map a wire type reads in more than one spelling."""
    paths = list(source_files())
    items = {}
    for path in paths:
        try:
            items[path] = collect_items(path)
        except ValueError:
            items[path] = []
    declarations, _ = present_forwarders(paths)
    spelling = SetSpelling(paths, items, declarations)
    found = []
    for path in paths:
        for item in items[path]:
            if item.kind not in {"struct", "enum"} or not derives_deserialize(item.attrs):
                continue
            line = set_spelling_lines(item)
            for label, offset, attributes, field_type in read_fields(item):
                if f"{path.as_posix()}:{label}" in SET_SPELLING_EXCEPTIONS:
                    continue
                failure = spelling.field_failure(item, attributes, field_type)
                if failure is not None:
                    found.append((path, line(offset), f"{label}: {failure}"))
        for reader in collect_deserialize_impls(path):
            held = sorted({
                name
                for item in items[path]
                if item.name == reader.name and item.kind in {"struct", "enum"}
                for name in spelling.spelled(item.body, item)
            })
            if not held or spelling.delegates(reader) or DISTINCT_READER_CALL.search(
                SOURCE_POLICY.mask_rust_non_code(reader.body)
            ):
                continue
            found.append((
                path,
                reader.line,
                f"{reader.name}: a hand-written reader of a type holding a "
                f"{', '.join(held)} calls no cadmpeg_core::distinct_keys reader, so "
                "the census cannot show it refuses a repeat or an array out of order",
            ))
    return found


@SOURCE_POLICY.analysis_session
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
    traits = {}
    trait_impls = {}
    implementing = set()
    for path in paths:
        implementing.update(implementing_macros(path))
        for trait in collect_traits(path):
            traits.setdefault(trait.name, []).append(trait)
        for implementation in collect_trait_impls(path):
            trait_impls.setdefault(
                implementation.trait.rsplit("::", 1)[-1], []
            ).append(implementation)
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

    # The type each wire-crate impl states for an associated type, read in
    # that impl: a generic declaration it names is instantiated there.
    impl_values = [
        (value, implementation.context)
        for implementations in trait_impls.values()
        for implementation in implementations
        for values in implementation.definitions.values()
        for value in values
        if value is not None
    ]

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

    module_bindings = {}
    for bindings in imports.values():
        for binding in bindings:
            if any(part.startswith("@") for part in binding.scope):
                continue
            crate = Path(*binding.path.parts[:binding.path.parts.index("src")])
            module_bindings.setdefault((crate, binding.scope), []).append(binding)

    def reexports(declarations, crate, scope, name, seen):
        """What the ``use`` declarations of one module bring in as ``name``.

        A named binding shadows a glob, so globs are read only when no
        binding names ``name``. Each binding's target is resolved from the
        module that states it. A cycle, an ambiguous import there, and two
        globs bringing in two declarations leave the name unresolved.
        """
        bindings = module_bindings.get((crate, scope), ())
        named = [binding for binding in bindings if binding.alias == name]
        found = []
        for binding in named or [binding for binding in bindings if binding.alias == "*"]:
            if binding in seen:
                return []
            target = binding.target if named else f"{binding.target}::{name}"
            context = SourceContext(binding.path, binding.scope)
            if target.split("::", 1)[0] not in {"", "crate", "self", "super"}:
                # A path keyword names a module no import can rebind.
                imported = imported_path(target, context)
                if imported is import_ambiguous:
                    return []
                if imported is not import_missing:
                    target = imported
            resolved = resolve_item(
                declarations, target, context, [], reexports, seen | {binding}
            )
            if resolved is not None and resolved not in found:
                found.append(resolved)
        return found

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
        return resolve_item(index, name, owner, ambiguities, reexports)

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
            if accepted_path_failure(path, context) is not None:
                return False
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
            if accepted_path_failure(path, item) is not None:
                return False
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

    def accepted_path_failure(path, owner):
        """Why a standard, registry or keyless-adapter path written for
        ``owner`` can name another item than the one the census accepts."""
        spelled = path.lstrip(":")
        if spelled in QUALIFIED_KEYLESS_ADAPTERS:
            return reader_path_failure(path, owner.crate, *PATH_HELPER_OWNERS[spelled])
        return external_path_failure(path, owner.crate)

    def target_for_type(path, owner):
        path = path.replace("$crate", "crate")
        imported = imported_path(path, owner)
        if imported is import_ambiguous:
            return None
        if imported is not import_missing:
            path = imported
        return resolve_item(index, path, owner, ambiguities, reexports)

    def declaration_for(path, owner, declarations):
        """Resolve one spelled path from ``owner`` without reporting a miss.

        The caller states what an unresolved path means for its proof.
        """
        imported = imported_path(path, owner)
        if imported is import_ambiguous:
            return None
        if imported is not import_missing:
            path = imported
        return resolve_item(declarations, path, owner, [], reexports)

    unread_traits = {}
    foreign_sources = []
    renamed = {}

    def workspace_sources():
        """The wire sources, then every other workspace source."""
        if not foreign_sources:
            foreign_sources.append(list(trait_impl_source_files()))
        return paths + foreign_sources[0]

    def names_of(name, wire_only=False):
        """``name`` and each name a ``use`` renames it to: anywhere, or in
        the wire crates when ``wire_only``."""
        key = (name, wire_only)
        if key not in renamed:
            renamed[key] = renamed_names(name, paths if wire_only else workspace_sources())
        return renamed[key]

    macro_definitions = {}

    def macro_instantiation(names):
        """The first wire macro token tree naming ``Name`` whose uses of it
        the census cannot read in place.

        Returns ``(tree, name)`` for one of ``names``, or ``None``. A
        ``macro_rules!`` body is read where it is written, as every other
        type outside a function body is, so a use whose argument is a
        fragment such as ``$t`` is unread there. An attribute is read by a
        derive or attribute macro the census does not expand. A call to a
        macro that can bind a name, a struct or a ``use`` rename, can join
        its arguments into a type the census does not see, as in
        ``host!(Name, Arg)``, wherever it is written. Any other call writes
        functions and impls whose parameters are the names it binds; outside
        a function body it is read in place, and inside one it is a type
        written in a function body.
        """
        if not macro_definitions:
            for path in paths:
                for tree in macro_token_trees(path):
                    if tree.kind == "definition":
                        macro_definitions.setdefault(tree.name, []).append(tree)
        named = name_pattern(names)
        for path in paths:
            source = read_source(path)
            if not named.search(source):
                continue
            code, _ = SOURCE_POLICY.production_source(source)
            for tree in macro_token_trees(path):
                spelled = named.search(code, tree.start, tree.end)
                if spelled is None or tree.kind == "definition":
                    continue
                if tree.kind == "attribute" or binding_macro(tree.name, macro_definitions):
                    return tree, spelled.group()
        return None

    def bound_by_expansion(candidate, source):
        """Whether ``candidate``, an argument a macro call writes, is a bare
        name nothing in scope declares, which only the call's expansion can
        bind, as a parameter of what it writes."""
        name = candidate.strip()
        return (
            re.fullmatch(r"[A-Za-z_]\w*", name) is not None
            and name not in index
            and name not in PRELUDE_NAMES
            and imported_path(name, source) is import_missing
        )

    def written_uses(names):
        """Every use of ``names`` the wire sources write outside a declaration."""
        names = tuple(sorted(names))
        return [use for path in paths for use in written_type_uses(path, names)]

    def unparsed_impl(path, names):
        """Why an impl of ``path`` that names one of ``names`` and whose
        header the parser does not read leaves the trait unread, or ``None``."""
        found = rejected_impl_naming(path, names)
        if found is None:
            return None
        line, name = found
        return (
            f"the impl at {path.as_posix()}:{line} names {name}, and the census "
            "cannot parse its header, so its associated types are unread"
        )

    def unread_trait(trait):
        """Why the census cannot read every impl of ``trait``, or ``None``.

        The trait is named by its own name and by every name a ``use``
        renames it to. An impl outside the wire crates names a type the
        declaration index does not hold. An impl anywhere whose tokens name
        the trait and whose header the parser does not read is unread. A
        macro token tree that names the trait can expand to an impl no source
        states, and the census expands no macro call: a call naming it, an
        attribute naming it, and a ``macro_rules!`` body naming it anywhere
        but a literal impl header the census reads each leave its impls
        unread.
        """
        identity = (trait.path, trait.start)
        if identity in unread_traits:
            return unread_traits[identity]
        names = names_of(trait.name)
        pattern = name_pattern(names)
        reason = None
        foreign = [
            path for path in workspace_sources()[len(paths):]
            if pattern.search(read_source(path))
        ]
        for path in foreign:
            try:
                blocks = trait_impl_blocks(path)
            except ValueError:
                reason = (
                    f"{path.as_posix()} names {trait.name} outside the wire "
                    "crates and the census cannot read its impls"
                )
                break
            outside = next(
                (block for block in blocks
                 if block[3].rsplit("::", 1)[-1] in names),
                None,
            )
            if outside is not None:
                line = read_source(path).count("\n", 0, outside[0]) + 1
                reason = (
                    f"impl {outside[3]} for {outside[4]} at "
                    f"{path.as_posix()}:{line} is outside the "
                    "wire crates, so its associated types are unread"
                )
                break
            reason = unparsed_impl(path, names)
            if reason is not None:
                break
            found = macro_tree_naming(path, names, wire=False)
            if found is not None:
                tree, name = found
                reason = (
                    f"{tree.describe()} names {name} outside the wire crates, "
                    "so an impl it expands is unread"
                )
                break
        for path in () if reason else paths:
            reason = unparsed_impl(path, names)
            if reason is not None:
                break
            found = macro_tree_naming(path, names, wire=True)
            if found is None:
                continue
            tree, name = found
            if tree.kind == "definition":
                why = f"names {name} outside an impl header the census reads"
            elif tree.kind == "call" and tree.name in implementing:
                why = f"names {name} and its macro spells impl"
            else:
                why = f"names {name} and the census does not expand it"
            reason = f"{tree.describe()} {why}, so an impl it expands is unread"
            break
        unread_traits[identity] = reason
        return reason

    def associated_type_proof(owner, parameter, trait_path, name):
        """Prove ``parameter::name`` through every impl of its trait.

        A type parameter names no reader of its own, but every type that can
        fill it implements the trait its bound names, and each such impl
        states the associated type. The trait is the one bound of the
        parameter that declares ``name``; each wire-crate impl of it is read
        and its associated type proved in the impl's own module. An impl of a
        trait of that name outside the wire crates, a call naming the trait to
        a macro that spells ``impl``, an impl that states no readable value,
        and a trait with no impl leave the payload unproved.
        """
        label = f"{parameter}::{name}"
        bounds = [trait_path] if trait_path else generic_bounds(owner).get(parameter, [])
        if not bounds:
            return False, (
                f"generic parameter {parameter} of {owner.name} states no trait "
                f"bound that declares {name}"
            )
        declaring = []
        for bound in bounds:
            trait = declaration_for(bound, owner, traits)
            if trait is None:
                base = bound.rsplit("::", 1)[-1]
                if base in TRAITS_WITHOUT_ASSOCIATED_TYPES and base not in traits:
                    continue
                return False, (
                    f"the bound {bound} of {parameter} is no workspace trait "
                    f"the census can read, so {label} is unread"
                )
            declared = trait_associated_type(trait, name)
            if declared == "declared":
                declaring.append(trait)
            elif declared is not None:
                return False, declared
        if len(declaring) != 1:
            return False, (
                f"{len(declaring)} bounds of {parameter} declare {name}, so "
                f"{label} names no one trait"
            )
        trait = declaring[0]
        unread = unread_trait(trait)
        if unread is not None:
            return False, unread
        impls = []
        written = [
            implementation
            for name in sorted(names_of(trait.name))
            for implementation in trait_impls.get(name, ())
        ]
        for implementation in written:
            resolved = declaration_for(implementation.trait, implementation.context, traits)
            if resolved is None:
                return False, (
                    f"the trait of impl {implementation.trait} for "
                    f"{implementation.self_type} at "
                    f"{implementation.path.as_posix()}:{implementation.line} "
                    "resolves to no one declaration"
                )
            if resolved is trait:
                impls.append(implementation)
        if not impls:
            return False, f"{trait.name} has no impl in the wire crates, so {label} is unread"
        for implementation in impls:
            where = (
                f"impl {trait.name} for {implementation.self_type} at "
                f"{implementation.path.as_posix()}:{implementation.line}"
            )
            values = implementation.definitions.get(name)
            if not values:
                return False, f"{where} states no type {name}"
            for value in values:
                if value is None:
                    return False, f"{where} states a type {name} the census cannot read"
                ok, reason = payload_proof(value, implementation.context)
                if not ok:
                    return False, f"{where}: type {name} = {value}: {reason}"
        return True, f"every impl of {trait.name} states a checked {name}"

    def generic_parameter_proof(owner, parameter, position, default):
        """Prove every type the workspace can put in one generic parameter.

        A generic parameter names no reader of its own. The set that can
        reach it is the declared default plus the argument every workspace
        declaration states at this position. A forwarded parameter adds its
        own default and instantiations to that set. Each concrete argument is
        proved against the declaration that spells it, so its imports resolve
        there. A cycle contributes no concrete argument; the traversal must
        find and prove at least one concrete reader.
        """
        pending = [(owner, parameter, position, default, "")]
        visited = set()
        concrete = 0
        while pending:
            current, name, at, fallback, route = pending.pop()
            identity = (current.path, current.start, at)
            if identity in visited:
                continue
            visited.add(identity)
            candidates = [] if fallback is None else [(fallback, current)]
            # Arguments are read from wire declarations, which name a wire
            # crate's renames only.
            names = names_of(current.name, wire_only=True)
            spelled = macro_instantiation(names)
            if spelled is not None:
                tree, written = spelled
                return False, (
                    f"{route}{tree.describe()} names {written}, and the "
                    f"census does not expand it, so its argument for {name} is unread"
                )
            arguments, unread = generic_arguments(
                current, at, order,
                lambda spelled, source: declaration_for(spelled, source, index),
                impl_values + written_uses(names),
                names,
            )
            if unread:
                return False, f"{route}{unread[0]}, so its argument for {name} is unread"
            candidates.extend(arguments)
            if not candidates:
                return False, (
                    f"{route}generic parameter {name} has no default and no "
                    "workspace instantiation"
                )
            for candidate, source in candidates:
                # A bare parameter of an enclosing function, trait or impl,
                # or one only a macro call's expansion binds, forwards what
                # fills it where that item is used; one inside a larger
                # argument makes a type the census does not read.
                foreign = getattr(source, "unread_parameters", set())
                if foreign is not None and candidate.strip() in foreign:
                    continue
                if getattr(source, "in_call", False) and bound_by_expansion(candidate, source):
                    continue
                mentioned = (
                    None if foreign is None
                    else next((word for word in re.findall(r"[A-Za-z_]\w*", candidate)
                               if word in foreign), False)
                )
                if mentioned is not False:
                    return False, (
                        f"{route}{source.name} fills {name} with {candidate.strip()}, "
                        + ("in an impl whose header the census does not parse"
                           if mentioned is None else
                           f"which names the parameter {mentioned} there")
                        + ", and the census does not read what fills it"
                    )
                argument, _ = type_parts(candidate)
                forwarded = None
                # An impl's own parameter has no instantiation to follow; the
                # payload proof below refuses it.
                if argument is not None and "::" not in argument and source.kind != "impl":
                    forwarded = next(
                        (
                            (parameter_index, candidate_default)
                            for parameter_index, (candidate_name, candidate_default)
                            in enumerate(generic_parameters(source))
                            if argument == candidate_name
                        ),
                        None,
                    )
                if forwarded is not None:
                    next_at, next_default = forwarded
                    next_route = (
                        f"{route}generic parameter {name} is instantiated with "
                        f"the generic parameter {argument} of {source.name}: "
                    )
                    pending.append((source, argument, next_at, next_default, next_route))
                    continue
                ok, reason = payload_proof(candidate, source)
                if not ok:
                    return False, (
                        f"{route}generic parameter {name} instantiated as "
                        f"{candidate.strip()}: {reason}"
                    )
                concrete += 1
        if not concrete:
            return False, (
                f"generic parameter {parameter} has no concrete workspace "
                "instantiation"
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

        # A type parameter shadows every module, import and declaration of its
        # name, so a path that starts with one is read as that parameter.
        parameters = generic_parameters(owner)
        names = {parameter for parameter, _ in parameters}
        associated = associated_type_path(field)
        if associated is not None and associated[0] in names:
            if owner.kind == "impl":
                return False, (
                    f"the type {associated[0]}::{associated[2]} depends on what "
                    "fills the impl, which the census does not read"
                )
            if custom:
                return False, (
                    "a custom deserializer on an associated-type payload is "
                    "not a source proof"
                )
            return associated_type_proof(owner, *associated)

        path, args = type_parts(field)
        if path is None:
            return False, "unsupported payload type syntax"
        head = path.split("::", 1)[0]
        if head in names and (owner.kind == "impl" or head != path):
            return False, (
                f"the type {path} depends on what fills the generic parameter "
                f"{head} of {owner.name}, which the census does not read"
            )
        imported = import_missing if path in names else imported_path(path, owner)
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
            named_by_path = PATH_HELPER_OWNERS.get(helper.rsplit("::", 1)[0].lstrip(":"))
            if helper in ABSENT_KEY_HELPERS | LOCAL_ID_HELPERS | DISTINCT_MAP_HELPERS:
                packages, module = named_by_path
                failure = reader_path_failure(custom[0], owner.crate, packages, module)
                if failure is not None:
                    return False, f"helper {helper}: {failure}"
            if helper in ABSENT_KEY_HELPERS or (
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

        if path in names:
            position = [parameter for parameter, _ in parameters].index(path)
            return generic_parameter_proof(
                owner, path, position, parameters[position][1]
            )

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

        failure = accepted_path_failure(resolved_path, owner)
        if failure is not None:
            return False, failure
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
            failure = name_form_failure(item)
            if failure is not None:
                arm_failures.append((item.path, item.line, f"{item.name}: {failure}"))
                return False
            if name_reader_derive(item)[0]:
                return True
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
                         and ((arm.kind == "unit" and not arm.tag_only) or arm.skipped_newtype)]
                for arm in units:
                    arm_failures.append((
                        item.path, item.line,
                        f"{item.name}::{arm.name}: an internally tagged unit arm "
                        + (
                            f"names {TAG_ONLY_READER}, but {arm.tag_only_failure}"
                            if arm.tag_only_failure else
                            "ignores unknown keys even with deny_unknown_fields; "
                            f"read it through {TAG_ONLY_READER} or use an empty "
                            "struct arm"
                        ),
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
            return False
        finally:
            checking.discard(identity)

    # A template that a macro instantiates states its derives for every
    # instance, whichever macro calls the census expands.
    for path in paths:
        for item in raw_items[path]:
            if item.name.startswith("$") and (failure := name_form_failure(item)):
                arm_failures.append((item.path, item.line, f"{item.name}: {failure}"))

    for item in order:
        if not (
            derives_deserialize(item.attrs)
            or item.reader_proven
            or name_reader_derive(item)[0]
        ):
            continue
        if not passes(item):
            failures.append((item.path, item.line, item.name))
        if item.kind == "enum" and not has_serde_flag(item.attrs, "untagged"):
            arms = [arm for arm in untagged_arms(item) if arm.untagged]
            if arms and not check_untagged_arms(item, arms):
                failures.append((item.path, item.line, item.name))

    failures.extend(absent_key_failures())
    arm_failures.extend(set_spelling_failures())

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
        f"{Path(path).as_posix()}:{line} {label}"
        for path, line, label in arm_failures
    ] + [
        f"{Path(path).as_posix()}:{line} {label}"
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
