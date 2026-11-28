#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the absence spelling of optional wire keys."""

from __future__ import annotations

import importlib.util
from bisect import bisect_right
import os
import re
import stat
import sys
import tomllib
from pathlib import Path

_SPEC = importlib.util.spec_from_file_location(
    "deny_source_policy", Path(__file__).with_name("check-source-policy.py")
)

if _SPEC is None or _SPEC.loader is None:
    raise SystemExit("cannot load the source-policy lexer")

SOURCE_POLICY = importlib.util.module_from_spec(_SPEC)
sys.modules[_SPEC.name] = SOURCE_POLICY
_SPEC.loader.exec_module(SOURCE_POLICY)


ABSENT_KEY_ROOT = "crates"


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
        self.path_text = str(path)
        self.crate_parts = path.parts[:path.parts.index("src")]
        self.scope = scope
        self.alias = alias
        self.target = target
        self.offsets = offsets
        self.scopes = scopes
        self.module_scope = module_scope


SCOPE_TOKEN = re.compile(r"\bmod\s+(\w+)\s*\{|[{}]")


RAW_IDENTIFIER = re.compile(r"(?<![\w#])r#([A-Za-z_]\w*)")


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
        if start > 0 and code[start - 1] == "@":
            cursor = opening.end()
            continue
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
            source.count("\n", 0, cursor) + 1,
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


PATH_READER_PACKAGES = {
    "cadmpeg_core": "cadmpeg-core",
    "cadmpeg_derive": "cadmpeg-derive",
    "cadmpeg_ir": "cadmpeg-ir",
}


STANDARD_ROOTS = {"std", "core", "alloc"}


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


ANY_ATTRIBUTE = re.compile(r"#\s*\[(?:[^\[\]]|\[(?:[^\[\]]|\[[^\[\]]*\])*\])*\]")


ATTRIBUTE_GAP = re.compile(r"(?:\s|//[^\n]*(?:\n|\Z))*\Z")


OMITTED_OPTIONAL_KEY = re.compile(r'skip_serializing_if\s*=\s*"Option::is_none"')


DESERIALIZE_WITH = re.compile(r'deserialize_with\s*=\s*"([\w:]+)"')


SERDE_WITH = re.compile(r'(?<!\w)with\s*=\s*"([\w:]+)"')


ABSENT_KEY_NULLABLE = re.compile(
    r'deserialize_with\s*=\s*"(?P<path>(?:crate|cadmpeg_core)::absent_key::nullable)"'
)


PRESENT_FORWARDER_MACRO = re.compile(
    r"(?<![\w$])(?P<path>(?:::\s*)?(?:[A-Za-z_$]\w*\s*::\s*)*)named_optional_field\s*!\s*\("
)


DECLARED_READER_NAME = re.compile(r"\s*(?:pub\s*(?:\([^)]*\)\s*)?)?(\w+)")


PRESENT_FORWARDER_DEFINITION = re.compile(r"macro_rules!\s*named_optional_field\b")


PRESENT_FORWARDER_OWNER = ("cadmpeg-core", "absent_key.rs")


SERDE_RENAME = re.compile(r'(?<!\w)rename\s*=\s*"([^"]+)"')


USE_DECLARATION = re.compile(r"\buse\s+([^;]+);")


SERDE_DEFAULT = re.compile(r"(?:^|[(,])\s*default\s*(?:[,)]|=)")


SERDE_FLATTEN = re.compile(r"(?:^|[(,])\s*flatten\s*[,)]")


FIELD_NAME = re.compile(r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?(\w+)\s*:")


OPTION_FIELD = re.compile(r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?\w+\s*:\s*Option\s*<")


FIELD_PREFIX = (
    r"(?:\s*(?:#\s*\[(?:[^\[\]]|\[(?:[^\[\]]|\[[^\[\]]*\])*\])*\]|//[^\n]*))*"
)


FIELD_DECLARATION = re.compile(
    r"(?:^|[,{])" + FIELD_PREFIX + r"\s*"
    r"(?:pub(?:\s*\([^)]*\))?\s+)?(\w+)\s*:\s*(Option\s*<)?"
)


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
    unreadable = set()
    for crate, crate_paths in crates.items():
        for path in crate_paths:
            try:
                crate_items[path] = collect_items(path)
            except ValueError as error:
                crate_items[path] = []
                unreadable.add(path)
                found.append((path, 1, str(error)))
    for crate, crate_paths in crates.items():
        found.extend(
            flattened_reader_failures(crate, crate_paths, crate_items, declarations)
        )
    for path in absent_key_source_files():
        crate = Path(*path.parts[:path.parts.index("src")])
        try:
            items = collect_items(path)
        except ValueError as error:
            if path not in unreadable:
                found.append((path, 1, str(error)))
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
    failures = absent_key_failures()
    if failures:
        for path, line, message in sorted(failures, key=str):
            print(f"{path.as_posix()}:{line} {message}", file=sys.stderr)
        print(f"absent-key census: {len(failures)} failure(s)", file=sys.stderr)
        return 1
    print("absent-key census: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
