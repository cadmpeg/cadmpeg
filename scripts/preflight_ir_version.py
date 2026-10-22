# SPDX-License-Identifier: Apache-2.0
"""A CADIR wire change bumps IR_VERSION once and regenerates pinned fixtures.

Wire formats are exact-match on IR_VERSION and carry no migrations, so a wire
change bumps IR_VERSION once per merged change and regenerates the fixtures
that pin it (CLAUDE.md). Without the bump, a document with a new field carries
a version current readers accept, and `deny_unknown_fields` then refuses it.
A bump that the wire does not need invalidates every reader for nothing, so
the check compares only what serde writes and reads.

The wire types are the `Serialize` or `Deserialize` structs and enums of
`cadmpeg-ir` and `cadmpeg-core` that the IR_VERSION documents reach. The roots
are the serialized types with an `ir_version` field (the CADIR document and the
decode sidecar) and the CLI report payloads (`impl ReportBody for ...` in the
`cadmpeg` crate). A type reaches every type that its wire form, a type alias,
its hand-written `Serialize`, `Deserialize`, or `JsonSchema` impl, or the
`macro_rules!` that declares it names. Items under `#[cfg(test)]`, inline or as
module files, are not wire types.

Each wire type is reduced to its wire form: which of `Serialize` and
`Deserialize` it derives, its `serde(...)`, `rename*`, and `cfg` attributes, and
its field and variant lines without visibility, comments, or other attributes.
A type whose wire forms differ from the merge base fails without the bump. A
type removed from one file and added identically in another is unchanged. A
reached type whose name is defined more than once, so that the name may refer
to another type, is reported for review, as is a serialized `cadmpeg-ir` type
that no root reaches and a changed serialized type of another crate. A
`cadmpeg-core` type that no root reaches is not on the wire.
"""

from __future__ import annotations

import re
import subprocess
from collections import Counter
from dataclasses import dataclass, field
from pathlib import PurePosixPath

from preflight import crate_of, is_test_path

NAME = "ir-version"
ORDER = 20

VERSION_FILE = "crates/cadmpeg-ir/src/document.rs"
VERSION = re.compile(r'pub const IR_VERSION: &str = "(\d+)";')
IR_CRATE = "cadmpeg-ir"
CORE_CRATE = "cadmpeg-core"
CLI_CRATE = "cadmpeg"
WIRE_CRATES = (IR_CRATE, CORE_CRATE)
SOURCES = tuple(f"crates/{crate}/src/" for crate in (*WIRE_CRATES, CLI_CRATE))
VISIBILITY_PREFIX = r"(?:pub(?:\s*\([^)]*\))?\s+)?"
ITEM = re.compile(rf"^{VISIBILITY_PREFIX}(struct|enum)\s+(\w+)")
ALIAS = re.compile(rf"^{VISIBILITY_PREFIX}type\s+(\w+)")
MODULE = re.compile(rf"^{VISIBILITY_PREFIX}mod\s+(\w+)\s*(;|\{{)")
IMPL = re.compile(r"^(?:unsafe\s+)?impl\b(.*)\bfor\s+(?:[\w:]+::)?(\w+)")
SERDE_TRAIT = re.compile(r"\b(?:Serialize|Deserialize|JsonSchema)\b")
REPORT_TRAIT = re.compile(r"\bReportBody\b")
MACRO_RULES = re.compile(r"^macro_rules!\s*(\w+)")
INVOCATION = re.compile(r"\b(\w+)!\s*[(\[{]")
LOWER_IDENTIFIER = re.compile(r"\b[a-z_]\w*")
VERSION_FIELD = re.compile(r'\bir_version:|rename="ir_version"')
PATH_ATTRIBUTE = re.compile(r'^#\[path\s*=\s*"([^"]+)"\]$')
TEST_CFG = re.compile(r"^#\[cfg\(\s*(?:test|all\(.*\btest\b.*\))\s*\)\]$")
SERDE_DERIVE = re.compile(r"\b(Serialize|Deserialize)\b")
WIRE_ATTRIBUTE = re.compile(r"\bserde\s*\(|\brename|^#\[cfg\(")
VISIBILITY = re.compile(r"\bpub\b(?:\s*\((?:crate|super|self|in\s+[\w:]+)\))?\s*")
IDENTIFIER = re.compile(r"\b[A-Z]\w*")
# Comments, which are dropped, and the literals that may contain comment markers.
LEXEME = re.compile(
    r'//[^\n]*|/\*.*?\*/|r(#*)".*?"\1|b?"(?:\\.|[^"\\])*"'
    r"|b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|.)|[^\\'\n])'",
    re.DOTALL)
STRING = re.compile(r'r(#*)".*?"\1|"(?:\\.|[^"\\])*"', re.DOTALL)
BRACKET = re.compile(r'r(#*)".*?"\1|"(?:\\.|[^"\\])*"|[()\[\]{}]', re.DOTALL)


def version(text: str | None) -> int | None:
    match = VERSION.search(text or "")
    return int(match.group(1)) if match else None


def strip_comments(text: str) -> str:
    """The text without comments; line breaks inside block comments are kept."""
    def replace(match: re.Match) -> str:
        lexeme = match.group(0)
        if lexeme.startswith("//"):
            return ""
        if lexeme.startswith("/*"):
            return "\n" * lexeme.count("\n")
        return lexeme
    return LEXEME.sub(replace, text)


def item_end(lines: list[str], start: int) -> int:
    """The last line of the attribute or item that starts at `start`."""
    attribute = lines[start].lstrip().startswith("#")
    depth = 0
    braced = False
    for index in range(start, len(lines)):
        text = STRING.sub('""', lines[index]).rstrip()
        depth += sum(map(text.count, "([{")) - sum(map(text.count, ")]}"))
        braced = braced or "{" in text
        if depth <= 0 and (text.endswith("]") if attribute else braced or text.endswith(";")):
            return index
    return len(lines) - 1


def normalize(text: str) -> str:
    text = " ".join(text.split())
    text = re.sub(r"\s*([^\w\s\"'])\s*", r"\1", text)
    return re.sub(r",([)\]}>])", r"\1", text)


def wire_attribute(attribute: str) -> bool:
    return bool(WIRE_ATTRIBUTE.search(attribute)) and not attribute.startswith("#[derive")


def drop_attributes(text: str) -> str:
    """The text without the attributes that do not change the wire."""
    out: list[str] = []
    index = 0
    while (start := text.find("#[", index)) >= 0:
        out.append(text[index:start])
        depth = 0
        end = start + 1
        while end < len(text):
            literal = STRING.match(text, end)
            if literal:
                end = literal.end()
                continue
            depth += {"[": 1, "]": -1}.get(text[end], 0)
            if depth == 0:
                break
            end += 1
        if wire_attribute(text[start:end + 1]):
            out.append(text[start:end + 1])
        index = end + 1
    out.append(text[index:])
    return "".join(out)


def wire_form(attributes: list[str], body: str) -> str:
    """What serde writes and reads for a struct or enum, normalized."""
    derives = sorted(set(SERDE_DERIVE.findall(
        " ".join(a for a in attributes if "derive" in a))))
    kept = [a for a in attributes if wire_attribute(a)]
    body = VISIBILITY.sub("", drop_attributes(body))
    return normalize(" ".join([f"derive({','.join(derives)})", *kept, body]))


@dataclass
class Parsed:
    """The serialized items, type aliases, serde impls, and test modules of one file."""

    items: list[tuple[str, str]] = field(default_factory=list)
    aliases: list[tuple[str, str]] = field(default_factory=list)
    impls: list[tuple[str, str]] = field(default_factory=list)
    reports: list[str] = field(default_factory=list)
    macros: list[tuple[str, str, set[str]]] = field(default_factory=list)
    invocations: list[tuple[str, str]] = field(default_factory=list)
    names: Counter = field(default_factory=Counter)
    test_modules: list[tuple[str, str | None]] = field(default_factory=list)


def parse(text: str | None) -> Parsed:
    """Serialized structs and enums outside `#[cfg(test)]`, with their wire forms."""
    parsed = Parsed()
    if not text:
        return parsed
    stripped = strip_comments(text)
    parsed.invocations = invocations(stripped)
    lines = stripped.splitlines()
    attributes: list[str] = []
    macros: list[tuple[int, set[str]]] = []
    index = 0
    while index < len(lines):
        while macros and index > macros[-1][0]:
            macros.pop()
        line = lines[index].strip()
        if not line:
            index += 1
            continue
        if line.startswith("#!["):
            index = item_end(lines, index) + 1
            continue
        if line.startswith("#["):
            end = item_end(lines, index)
            attributes.append(normalize(" ".join(lines[index:end + 1])))
            index = end + 1
            continue
        if any(TEST_CFG.match(a) for a in attributes):
            module = MODULE.match(line)
            if module and module.group(2) == ";":
                path = next((m.group(1) for m in map(PATH_ATTRIBUTE.match, attributes) if m),
                            None)
                parsed.test_modules.append((module.group(1), path))
            index = item_end(lines, index) + 1
            attributes = []
            continue
        rules = MACRO_RULES.match(line)
        if rules:
            end = item_end(lines, index)
            defined: set[str] = set()
            parsed.macros.append((rules.group(1), " ".join(lines[index + 1:end + 1]), defined))
            macros.append((end, defined))
        implementation = IMPL.match(line)
        item = ITEM.match(line)
        if macros and (implementation or item):
            macros[-1][1].add((implementation or item).group(2))
        if implementation and SERDE_TRAIT.search(implementation.group(1)):
            end = item_end(lines, index)
            parsed.impls.append((implementation.group(2), " ".join(lines[index:end + 1])))
        elif implementation and REPORT_TRAIT.search(implementation.group(1)):
            parsed.reports.append(implementation.group(2))
        alias = ALIAS.match(line)
        if item or alias:
            end = item_end(lines, index)
            body = " ".join(lines[index:end + 1])
            if item:
                parsed.names[item.group(2)] += 1
                serialized = " ".join(attributes)
                if SERDE_DERIVE.search(serialized) or "serde(" in serialized:
                    parsed.items.append((f"{item.group(1)} {item.group(2)}",
                                         wire_form(attributes, body)))
            else:
                parsed.names[alias.group(1)] += 1
                parsed.aliases.append((alias.group(1), normalize(body)))
            index = end + 1
        else:
            index += 1
        attributes = []
    return parsed


def invocations(text: str) -> list[tuple[str, str]]:
    """Each macro invocation as (macro name, the tokens between its delimiters)."""
    found: list[tuple[str, str]] = []
    for match in INVOCATION.finditer(text):
        if match.group(1) == "macro_rules":
            continue
        depth = 0
        start = match.end() - 1
        for token in BRACKET.finditer(text, start):
            if token.group(0) in "([{":
                depth += 1
            elif token.group(0) in ")]}":
                depth -= 1
                if depth == 0:
                    found.append((match.group(1), text[start + 1:token.start()]))
                    break
    return found


def serialized_items(text: str | None) -> dict[str, str]:
    """Map each serialized struct or enum of one file to its wire form."""
    items: dict[str, str] = {}
    for key, form in parse(text).items:
        while key in items:
            key += "'"
        items[key] = form
    return items


def test_module_paths(path: str, parsed: Parsed) -> list[str]:
    """Files and directories of the `#[cfg(test)] mod x;` modules a file declares."""
    file = PurePosixPath(path)
    directory = file.parent if file.stem in {"lib", "main", "mod"} else file.parent / file.stem
    found: list[str] = []
    for name, explicit in parsed.test_modules:
        if explicit:
            found.append(str(file.parent / explicit))
        else:
            found += [str(directory / f"{name}.rs"), f"{directory / name}/"]
    return found


def production(files: dict[str, Parsed]) -> dict[str, Parsed]:
    """The files that are not the module file of a `#[cfg(test)] mod x;`."""
    excluded = [entry for path, parsed in files.items()
                for entry in test_module_paths(path, parsed)]
    return {path: parsed for path, parsed in files.items()
            if not any(path == entry or entry.endswith("/") and path.startswith(entry)
                       for entry in excluded)}


class Tree:
    """Parsed Rust sources of revisions, with each distinct text parsed once."""

    def __init__(self, context) -> None:
        self.context = context
        self.cache: dict[str, Parsed] = {}

    def parse(self, text: str) -> Parsed:
        if text not in self.cache:
            self.cache[text] = parse(text)
        return self.cache[text]

    def files(self, revision: str | None, prefixes: tuple[str, ...]) -> dict[str, Parsed]:
        """Production Rust files under the prefixes; `None` reads the working tree."""
        root = self.context.root
        if revision is None:
            listing = self.context.git("ls-files", "-co", "--exclude-standard", "-z", "--",
                                       *prefixes).stdout
            texts = {path: (root / path).read_text(errors="replace")
                     for path in sorted(set(listing.split("\0")))
                     if path.endswith(".rs") and not is_test_path(path)
                     and (root / path).is_file()}
        else:
            listing = self.context.git("ls-tree", "-r", "-z", "--name-only", revision, "--",
                                       *prefixes).stdout
            texts = self.blobs(revision, [path for path in listing.split("\0")
                                          if path.endswith(".rs") and not is_test_path(path)])
        return production({path: self.parse(text) for path, text in texts.items()})

    def blobs(self, revision: str, paths: list[str]) -> dict[str, str]:
        """File texts at a revision, read with one `git cat-file --batch`."""
        if not paths:
            return {}
        request = "".join(f"{revision}:{path}\n" for path in paths).encode()
        output = subprocess.run(["git", "cat-file", "--batch"], cwd=self.context.root,
                                input=request, capture_output=True).stdout
        texts: dict[str, str] = {}
        offset = 0
        for path in paths:
            newline = output.find(b"\n", offset)
            if newline < 0:
                break
            header = output[offset:newline].split()
            offset = newline + 1
            if len(header) == 3 and header[1] == b"blob":
                size = int(header[2])
                texts[path] = output[offset:offset + size].decode(errors="replace")
                offset += size + 1
        return texts


def reached(files: dict[str, Parsed]) -> set[str]:
    """Names that the IR_VERSION documents and the CLI report payloads reach.

    A type defined inside a `macro_rules!` body reaches what the macro's body and
    invocations name, and a macro passed by name into another macro reaches
    what that macro reaches, so that arenas declared through macros are followed.
    """
    edges: dict[str, set[str]] = {}
    frontier: list[str] = []
    defined_macros = {name for parsed in files.values() for name, _, _ in parsed.macros}

    def link(source: str, text: str) -> None:
        targets = edges.setdefault(source, set())
        targets.update(IDENTIFIER.findall(text))
        targets.update(f"{name}!" for name in INVOCATION.findall(text) if name in defined_macros)

    for parsed in files.values():
        for key, form in parsed.items:
            link(key.split()[1], form)
            if VERSION_FIELD.search(form):
                frontier.append(key.split()[1])
        for name, body in [*parsed.aliases, *parsed.impls]:
            link(name, body)
        for name, body, types in parsed.macros:
            link(f"{name}!", body)
            for defined in types:
                edges.setdefault(defined, set()).add(f"{name}!")
        for name, tokens in parsed.invocations:
            if name in defined_macros:
                link(f"{name}!", tokens)
                for argument in set(LOWER_IDENTIFIER.findall(tokens)) & defined_macros:
                    edges.setdefault(f"{argument}!", set()).add(f"{name}!")
        frontier += parsed.reports
    seen: set[str] = set()
    while frontier:
        name = frontier.pop()
        if name not in seen:
            seen.add(name)
            frontier.extend(edges.get(name, ()))
    return seen


def forms(files: dict[str, Parsed], keep=lambda name: True) -> dict[str, Counter]:
    """Each serialized item's wire forms, counted as (path, form)."""
    found: dict[str, Counter] = {}
    for path, parsed in files.items():
        for key, form in parsed.items:
            if keep(key.split()[1]):
                found.setdefault(key, Counter())[(path, form)] += 1
    return found


def differences(before: dict[str, Counter], after: dict[str, Counter]) -> list[str]:
    """Items whose wire forms differ, wherever each form is, as path: item state."""
    changes: list[str] = []
    for key in sorted(set(before) | set(after)):
        old = before.get(key, Counter())
        new = after.get(key, Counter())
        old_forms: Counter = Counter()
        new_forms: Counter = Counter()
        for (_, form), count in old.items():
            old_forms[form] += count
        for (_, form), count in new.items():
            new_forms[form] += count
        if old_forms == new_forms:
            continue
        state = "added" if not old else "removed" if not new else "changed"
        paths = ({path for path, form in new if form not in old_forms}
                 | {path for path, form in old if form not in new_forms})
        paths = paths or {path for path, _ in [*old, *new]}
        changes.extend(f"{path}: {key} {state}" for path in sorted(paths))
    return changes


def wire_changes(context) -> tuple[list[str], list[str], list[str]]:
    """Changed wire types, reached types with a shared name, and unreached IR types."""
    if not any(path.startswith(SOURCES[:2]) and path.endswith(".rs")
               for path in context.changed_paths()):
        return [], [], []
    tree = Tree(context)
    revisions = {label: tree.files(revision, SOURCES)
                 for label, revision in (("before", context.base), ("after", context.rev))}
    names: set[str] = set()
    counts: Counter = Counter()
    for files in revisions.values():
        names |= reached(files)
        defined: Counter = Counter()
        for path, parsed in files.items():
            if crate_of(path) in WIRE_CRATES:
                defined.update(parsed.names)
        counts |= defined

    def compare(crate: str, keep) -> list[str]:
        before, after = ({path: parsed for path, parsed in revisions[label].items()
                          if crate_of(path) == crate} for label in ("before", "after"))
        return differences(forms(before, keep), forms(after, keep))

    wire: list[str] = []
    unclear: list[str] = []
    for crate in WIRE_CRATES:
        wire += compare(crate, lambda name: name in names and counts[name] == 1)
        unclear += compare(crate, lambda name: name in names and counts[name] > 1)
    unreached = compare(IR_CRATE, lambda name: name not in names)
    return wire, unclear, unreached


def other_changes(context) -> list[str]:
    """Changed serialized types in the changed production files of other crates."""
    before: dict[str, dict[str, Parsed]] = {}
    after: dict[str, dict[str, Parsed]] = {}
    for path in context.changed_paths():
        crate = crate_of(path)
        if (not crate or crate in WIRE_CRATES or not path.endswith(".rs")
                or is_test_path(path)):
            continue
        before.setdefault(crate, {})[path] = parse(context.read_base(path))
        after.setdefault(crate, {})[path] = parse(context.read(path))
    changes: list[str] = []
    for crate in sorted(before):
        changes += differences(forms(before[crate]), forms(after[crate]))
    return changes


def stale_fixtures(context, old: int) -> list[str]:
    """Files that pinned the old version at the merge base and still pin it."""
    pattern = rf'"ir_version"\s*:\s*"{old}"'
    pinned = context.git("grep", "-l", "-E", pattern, context.base, "--").stdout.splitlines()
    pinned = {line.split(":", 1)[1] for line in pinned if ":" in line}
    arguments = ["grep", "-l", "-E", pattern]
    if context.rev:
        arguments.append(context.rev)
    still = context.git(*arguments, "--").stdout.splitlines()
    still = {line.split(":", 1)[1] if context.rev else line for line in still}
    return sorted(pinned & still)


def run(context):
    base = version(context.read_base(VERSION_FILE))
    upstream = version(context.read_upstream(VERSION_FILE))
    new = version(context.read(VERSION_FILE))
    if new is None:
        return context.skipped(f"no IR_VERSION in {VERSION_FILE}")
    wire, unclear, unreached = wire_changes(context)
    other = other_changes(context)
    results = []
    expected = (upstream or new) + 1
    if wire and new == upstream:
        summary = (f"CADIR wire types changed; IR_VERSION is still {new}" if new == base else
                   f"CADIR wire types changed; upstream also moved IR_VERSION to {new}, "
                   f"so this change makes it {expected}")
        results.append(context.failed(
            summary,
            *wire[:15],
            f"fix: set IR_VERSION to \"{expected}\" in {VERSION_FILE}, regenerate the "
            "version-pinned fixtures (UPDATE_GOLDEN=1 cargo test-fast golden; fuzz seeds per "
            "crates/cadmpeg-fuzz/README.md), and run "
            "cargo test -p cadmpeg-ir --features schema --lib",
        ))
    elif new != base and upstream is not None and new != upstream + 1:
        results.append(context.failed(
            f"IR_VERSION is {new}; upstream is {upstream}, so one bump makes it {upstream + 1}",
            f"fix: rebase onto {context.upstream} and set IR_VERSION to \"{upstream + 1}\"",
        ))
    if new != base and base is not None:
        stale = stale_fixtures(context, base)
        if stale:
            results.append(context.failed(
                f"IR_VERSION is {new} but {len(stale)} file(s) still pin \"ir_version\": \"{base}\"",
                *stale[:15],
                "fix: regenerate goldens, fuzz seeds, digest pins, and CLI test documents "
                "(UPDATE_GOLDEN=1 cargo test-fast golden, then review the diff)",
            ))
    if (unclear or unreached) and not wire and new == base:
        notes = ([f"{change} (name defined more than once)" for change in unclear]
                 + [f"{change} (no IR_VERSION document reaches it)" for change in unreached])
        results.append(context.warned(
            "serialized IR types changed that the check cannot place on the wire; confirm "
            "whether the CADIR wire changes",
            *notes[:15],
        ))
    if other:
        results.append(context.warned(
            "serialized types outside cadmpeg-ir and cadmpeg-core changed; confirm whether "
            "the CADIR wire changes",
            *other[:15],
        ))
    if not results:
        if new != base:
            results.append(context.passed(f"IR_VERSION {base} -> {new}; pinned fixtures updated"))
        else:
            results.append(context.passed("no CADIR wire type changed"))
    return results
