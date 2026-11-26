#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""The history-body admission route lists the rules of the checks that give up
a history body.

The route in `crates/cadmpeg-ir/src/validate/admit.rs` names its rules by hand.
This test reads the validator source, follows calls from the history-body
check and from the nest checks of identities, references, typed references,
graphs, edge ends, and tolerances, and collects every `rule!` and `rule_name!`
it reaches. The route must list exactly the reached rules that `DRAFT_CORE`
lists. A rule added to those checks fails this test until the route takes it
or a check is moved out of the list below.

Calls resolve by path: a bare name to the function of the same file or to its
`use` import, a `path::name` through `crate`, `self`, `super`, imports, and
child modules, a `Type::name` to the functions of that name beside the type,
and a `.name(` method call to the methods of that name in the same file.
"""

from __future__ import annotations

from dataclasses import dataclass
import importlib.util
from pathlib import Path
import re
import sys
import unittest


SPEC = importlib.util.spec_from_file_location(
    "name_validation_rules", Path(__file__).with_name("name-validation-rules.py")
)
assert SPEC and SPEC.loader
namer = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = namer
SPEC.loader.exec_module(namer)

SRC = "crates/cadmpeg-ir/src/"
VALIDATE = SRC + "validate/"
ADMIT = VALIDATE + "admit.rs"
# The checks whose rules give up a history body: its links, and the nest
# checks of identities, references, typed references, coedge pairing, shell
# connectivity, wire topology, edge ends, and topology tolerances.
ROOTS = (
    ("history_bodies.rs", "check_history_bodies"),
    ("identity_order.rs", "check_identity_and_order"),
    ("topology.rs", "check_references"),
    ("referential_integrity.rs", "check_typed_references"),
    ("topology/graphs.rs", "check_coedge_pairing"),
    ("topology/graphs.rs", "check_shell_connectivity"),
    ("topology/graphs.rs", "check_wire_topology"),
    ("geometry_consistency.rs", "check_edge_endpoint_consistency"),
    ("topology.rs", "check_topology_tolerances"),
)
# The nest check runs every topology rule. A nest it reaches from the roots
# (a nest's datum witness body) gives up its body by the roots' rules.
NEST_CHECK = ("topology/nested.rs", "check")


@dataclass(frozen=True)
class Fn:
    path: str
    body: tuple[int, int]
    method: bool


def module_of(path: str) -> tuple[str, ...]:
    parts = Path(path[len(SRC):]).with_suffix("").parts
    return parts[:-1] if parts and parts[-1] in ("mod", "lib") else parts


def rule_at(toks: list, k: int) -> str | None:
    """The rule name of a `rule!("…"` or `rule_name!("…"` at `k`."""
    if (
        toks[k].kind == "ident"
        and toks[k].text in ("rule", "rule_name")
        and k + 3 < len(toks)
        and toks[k + 1].text == "!"
        and toks[k + 2].text == "("
        and toks[k + 3].kind == "str"
    ):
        return namer.string_value(toks[k + 3].text)
    return None


class Source:
    """The production items of one file: its functions and `use` imports."""

    def __init__(self, path: str, src: str) -> None:
        self.path = path
        self.module = module_of(path)
        self.toks = namer.tokenize(src[: namer.test_start(src)])
        self.fns: dict[str, list[Fn]] = {}
        self.imports: dict[str, tuple[str, ...]] = {}
        toks = self.toks
        for i, tok in enumerate(toks):
            if tok.kind != "ident" or i + 2 >= len(toks):
                continue
            if tok.text == "fn" and toks[i + 1].kind == "ident":
                self._function(i)
            elif tok.text == "use" and (i == 0 or toks[i - 1].text not in (".", ":")):
                end = i + 1
                while end < len(toks) and toks[end].text != ";":
                    end += 1
                self._use(toks[i + 1:end], ())

    def _function(self, i: int) -> None:
        toks = self.toks
        q = i + 2
        if toks[q].text == "<":
            depth = 0
            while q < len(toks):
                if toks[q].text == "<":
                    depth += 1
                elif toks[q].text == ">" and toks[q - 1].text != "-":
                    depth -= 1
                q += 1
                if depth == 0:
                    break
        if q >= len(toks) or toks[q].text != "(":
            return
        close = namer.matching(toks, q)
        method = False
        for k in range(q + 1, close):
            if toks[k].text == ",":
                break
            method = method or toks[k].text == "self"
        k = close + 1
        while k < len(toks) and toks[k].text not in ("{", ";"):
            k += 1
        if k < len(toks) and toks[k].text == "{":
            fn = Fn(self.path, (k, namer.matching(toks, k)), method)
            self.fns.setdefault(toks[i + 1].text, []).append(fn)

    def _use(self, toks: list, prefix: tuple[str, ...]) -> None:
        segments = list(prefix)
        for i, tok in enumerate(toks):
            if tok.text == "{":
                part: list = []
                depth = 0
                for inner in toks[i + 1:-1]:
                    depth += {"{": 1, "}": -1}.get(inner.text, 0)
                    if inner.text == "," and depth == 0:
                        self._use(part, tuple(segments))
                        part = []
                    else:
                        part.append(inner)
                if part:
                    self._use(part, tuple(segments))
                return
            if tok.text == "*":
                return
            if tok.text == "as" and i + 1 < len(toks):
                self.imports[toks[i + 1].text] = tuple(segments)
                return
            if tok.kind == "ident":
                segments.append(tok.text)
        if segments and segments[-1] == "self":
            segments.pop()
        if segments:
            self.imports[segments[-1]] = tuple(segments)


class Crate:
    def __init__(self) -> None:
        self.files: dict[str, Source] = {}
        self.modules: dict[tuple[str, ...], list[Source]] = {}
        for path in namer.rust_files():
            if not path.startswith(SRC) or namer.is_test_path(path):
                continue
            if not (namer.ROOT / path).exists():
                continue
            source = Source(path, (namer.ROOT / path).read_text())
            self.files[path] = source
            self.modules.setdefault(source.module, []).append(source)

    def module(self, source: Source, segments: list[str]) -> tuple[str, ...] | None:
        """The module a path prefix written in `source` names."""
        if not segments:
            return source.module
        first, rest = segments[0], segments[1:]
        if first == "crate":
            return tuple(rest)
        if first in ("self", "super"):
            base = source.module
            while segments and segments[0] in ("self", "super"):
                if segments[0] == "super":
                    base = base[:-1]
                segments = segments[1:]
            return base + tuple(segments)
        imported = source.imports.get(first)
        if imported and imported[0] != first:
            return self.module(source, list(imported) + rest)
        if source.module + (first,) in self.modules:
            return source.module + tuple(segments)
        if (first,) in self.modules:
            return tuple(segments)
        return None

    def named(self, module: tuple[str, ...] | None, name: str) -> list[Fn]:
        if module is None:
            return []
        return [fn for source in self.modules.get(module, []) for fn in source.fns.get(name, [])]

    def callees(self, source: Source, k: int) -> list[Fn]:
        """The functions the identifier at `k` calls or names."""
        toks = source.toks
        name = toks[k].text
        before = toks[k - 1].text if k else ""
        after = toks[k + 1].text if k + 1 < len(toks) else ""
        if before == "fn":
            return []
        if before == ".":
            if after not in ("(", ":"):
                return []
            return [fn for fn in source.fns.get(name, []) if fn.method]
        segments: list[str] = []
        j = k - 1
        while j >= 2 and toks[j].text == ":" and toks[j - 1].text == ":" \
                and toks[j - 2].kind == "ident":
            segments.insert(0, toks[j - 2].text)
            j -= 3
        if segments and (segments[-1] == "Self" or segments[-1][:1].isupper()):
            if after != "(":
                return []
            found = list(source.fns.get(name, []))
            owner = segments[-1]
            if len(segments) > 1:
                found += self.named(self.module(source, segments[:-1]), name)
            elif owner in source.imports:
                found += self.named(self.module(source, list(source.imports[owner][:-1])), name)
            return found
        if segments:
            return self.named(self.module(source, segments), name)
        if name in source.fns:
            return source.fns[name]
        if name in source.imports:
            imported = source.imports[name]
            return self.named(self.module(source, list(imported[:-1])), name)
        return []

    def function(self, path: str, name: str) -> list[Fn]:
        found = self.files[VALIDATE + path].fns.get(name, [])
        assert found, f"{VALIDATE}{path} defines no `{name}`"
        return found

    def reached_rules(self, roots: list[Fn], cut: set[Fn]) -> set[str]:
        """The rules named in `roots` and in every function they reach,
        without entering a function of `cut`."""
        rules: set[str] = set()
        seen: set[Fn] = set()
        stack = list(roots)
        while stack:
            fn = stack.pop()
            if fn in seen or fn in cut:
                continue
            seen.add(fn)
            source = self.files[fn.path]
            for k in range(*fn.body):
                rule = rule_at(source.toks, k)
                if rule is not None:
                    rules.add(rule)
                elif source.toks[k].kind == "ident":
                    stack.extend(self.callees(source, k))
        return rules


def listed_rules(src: str, start: str, end: str) -> list[str]:
    """The `rule_name!` names between the line `start` and the next line `end`."""
    begin = src.index(start)
    return re.findall(r'rule_name!\("([^"]+)"\)', src[begin:src.index(end, begin)])


class HistoryBodyAdmissionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.crate = Crate()

    def test_every_validator_rule_lies_in_a_function(self) -> None:
        for path, source in self.crate.files.items():
            if path == ADMIT:
                continue
            bodies = [fn.body for fns in source.fns.values() for fn in fns]
            for k in range(len(source.toks)):
                rule = rule_at(source.toks, k)
                if rule is not None:
                    self.assertTrue(
                        any(start < k < end for start, end in bodies),
                        f"{path}: `{rule}` lies outside every function body",
                    )

    def test_the_route_lists_the_core_rules_of_its_checks(self) -> None:
        src = (namer.ROOT / ADMIT).read_text()
        core = set(listed_rules(src, "macro_rules! draft_core_rules", "\n}\n"))
        route = listed_rules(src, "const HISTORY_BODY_RULES", "];")
        self.assertEqual(route, sorted(set(route)), "the route lists each rule once, in order")
        roots = [fn for path, name in ROOTS for fn in self.crate.function(path, name)]
        cut = set(self.crate.function(*NEST_CHECK))
        reached = self.crate.reached_rules(roots, cut)
        self.assertEqual(set(route), reached & core)


if __name__ == "__main__":
    unittest.main()
