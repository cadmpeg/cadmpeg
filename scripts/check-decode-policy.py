#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run the pinned compiler's decode allocation and work checks."""
import argparse
from collections import deque
from functools import lru_cache
import os
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
TOOL = ROOT / "crates/cadmpeg-decode-policy"


@lru_cache(maxsize=None)
def signature_pattern(source):
    """Parse the compiler's length-prefixed structural signature."""
    def read(offset):
        kind = source[offset]
        separator = source.index(":", offset + 1)
        length = int(source[offset + 1:separator])
        offset = separator + 1
        name = source[offset:offset + length]
        offset += length
        children = []
        if kind == "r":
            separator = source.index(":", offset)
            count = int(source[offset:separator])
            offset = separator + 1
            for _ in range(count):
                child, offset = read(offset)
                children.append(child)
        elif kind != "v":
            raise ValueError("invalid signature pattern")
        return (kind, name, tuple(children)), offset
    pattern, end = read(0)
    if end != len(source):
        raise ValueError("trailing signature data")
    return pattern


@lru_cache(maxsize=None)
def compatible(left, right):
    """Unify signatures with separate, consistent substitutions for each side."""
    bindings = {}
    def resolve(term):
        while term[1][0] == "v" and (term[0], term[1][1]) in bindings:
            term = bindings[(term[0], term[1][1])]
        return term
    def occurs(variable, term):
        side, pattern = resolve(term)
        if pattern[0] == "v":
            return variable == (side, pattern[1])
        return any(occurs(variable, (side, child)) for child in pattern[2])
    pending = [((0, left), (1, right))]
    while pending:
        left, right = (resolve(term) for term in pending.pop())
        a, b = left[1], right[1]
        if a[0] == "v" and b[0] == "v" and left[0] == right[0] and a[1] == b[1]:
            continue
        if a[0] == "v":
            variable = (left[0], a[1])
            if occurs(variable, right):
                return False
            bindings[variable] = right
        elif b[0] == "v":
            variable = (right[0], b[1])
            if occurs(variable, left):
                return False
            bindings[variable] = left
        elif a[:2] != b[:2] or len(a[2]) != len(b[2]):
            return False
        else:
            pending.extend(((left[0], x), (right[0], y)) for x, y in zip(a[2], b[2]))
    return True


class DecodeGraph:
    """Resolve guarded object edges and typed indirect edges in one joined graph."""

    def __init__(self, source):
        self.roots = {}
        self.bodies = {}
        self.ends = {}
        self.nodes = {}
        self.edges = {}
        self.addresses = {}
        self.pointer_calls = []
        self.trait_calls = []
        self.symbolic_calls = []
        self.method_impls = {}
        self.symbolic_roots = set()
        self.symbolic_instances = set()
        self.symbolic_candidates = set()
        self.symbolic_edges = {}
        self.objects = []
        self.object_calls = []
        for row in source.splitlines():
            fields = row.split("\t")
            tag = fields[0]
            if len(fields) == 7 and tag == "decode_body":
                self.bodies[fields[1]] = fields[2:]
            elif len(fields) == 3 and tag == "decode_body_span":
                self.ends[fields[1]] = int(fields[2])
            elif len(fields) == 3 and tag == "decode_node":
                self.nodes[fields[1]] = fields[2]
            elif len(fields) == 3 and tag == "decode_root":
                self.roots[fields[1]] = fields[2]
            elif len(fields) == 4 and tag == "decode_edge":
                self.edges.setdefault(fields[1], set()).add((fields[2], fields[3]))
            elif len(fields) == 2 and tag == "decode_symbolic_root":
                self.symbolic_roots.add(fields[1])
            elif len(fields) == 2 and tag == "decode_symbolic_instance":
                self.symbolic_instances.add(fields[1])
            elif len(fields) == 2 and tag == "decode_symbolic_candidate":
                self.symbolic_candidates.add(fields[1])
            elif len(fields) == 3 and tag == "decode_symbolic_edge":
                self.symbolic_edges.setdefault(fields[1], set()).add(fields[2])
            elif len(fields) == 3 and tag == "decode_address":
                self.addresses.setdefault(signature_pattern(fields[1]), set()).add(fields[2])
            elif len(fields) == 3 and tag == "decode_pointer_call":
                self.pointer_calls.append((fields[1], signature_pattern(fields[2])))
            elif len(fields) == 4 and tag == "decode_trait_call":
                self.trait_calls.append((fields[1], fields[2], signature_pattern(fields[3])))
            elif len(fields) == 4 and tag == "decode_symbolic_call":
                self.symbolic_calls.append((fields[1], fields[2], signature_pattern(fields[3])))
            elif len(fields) == 4 and tag == "decode_method_impl":
                self.method_impls.setdefault(fields[1], set()).add((signature_pattern(fields[2]), fields[3]))
            elif len(fields) == 5 and tag == "decode_object":
                self.objects.append((fields[1], fields[2], signature_pattern(fields[3]), fields[4]))
            elif len(fields) == 4 and tag == "decode_object_call":
                self.object_calls.append((fields[1], fields[2], signature_pattern(fields[3])))

    def resolve(self):
        reached = set(self.roots)
        symbolic = self.symbolic_roots.copy()
        edges = {caller: targets.copy() for caller, targets in self.edges.items()}
        while True:
            before = (len(reached), len(symbolic))
            symbolic.update(self.symbolic_instances & reached)
            for caller, targets in self.symbolic_edges.items():
                if caller in symbolic:
                    symbolic.update(targets)
                    reached.update(targets)
                    edges.setdefault(caller, set()).update((target, "generic instantiation") for target in targets)
            called = {}
            for caller, method, signature in self.object_calls:
                if caller in reached:
                    called.setdefault(method, set()).add((caller, signature))
            for caller, method, signature, target in self.objects:
                if caller in reached:
                    calls = [call for call, call_signature in called.get(method, ()) if compatible(signature, call_signature)]
                    if calls:
                        reached.add(target)
                        for call in calls:
                            edges.setdefault(call, set()).add((target, "trait-object call"))
                        if caller in symbolic:
                            symbolic.add(target)
            for caller, signature in self.pointer_calls:
                if caller in reached:
                    targets = {target for candidate, bodies in self.addresses.items() if compatible(signature, candidate) for target in bodies}
                    reached.update(targets)
                    symbolic.update(targets & self.symbolic_candidates)
                    edges.setdefault(caller, set()).update((target, "unresolved-indirect candidate") for target in targets)
            for caller, method, signature in self.trait_calls:
                if caller in reached:
                    targets = {target for candidate, target in self.method_impls.get(method, ()) if compatible(signature, candidate)}
                    reached.update(targets)
                    symbolic.update(targets & self.symbolic_candidates)
                    edges.setdefault(caller, set()).update((target, "unresolved-indirect candidate") for target in targets)
            for caller, method, signature in self.symbolic_calls:
                if caller in symbolic:
                    targets = {target for candidate, target in self.method_impls.get(method, ()) if compatible(signature, candidate)}
                    reached.update(targets)
                    symbolic.update(targets)
                    edges.setdefault(caller, set()).update((target, "generic instantiation") for target in targets)
            for caller, targets in edges.items():
                if caller in reached:
                    reached.update(target for target, _ in targets)
            if before == (len(reached), len(symbolic)):
                return reached, edges

    def select(self, selector):
        path, separator, line = selector.rpartition(":")
        if separator and line.isdecimal():
            line = int(line)
            candidates = [key for key, body in self.bodies.items() if body[0] == path and int(body[1]) <= line <= self.ends.get(key, int(body[1]))]
            if candidates:
                shortest = min(self.ends.get(key, int(self.bodies[key][1])) - int(self.bodies[key][1]) for key in candidates)
                candidates = [key for key in candidates if self.ends.get(key, int(self.bodies[key][1])) - int(self.bodies[key][1]) == shortest]
        else:
            candidates = [key for key, body in self.bodies.items() if body[2] == selector or body[2].endswith("::" + selector)]
        if not candidates:
            raise ValueError(f"no body named {selector}")
        if len(candidates) != 1:
            raise ValueError(f"ambiguous body {selector}: " + "; ".join(self.location(key) for key in candidates))
        return candidates[0]

    def location(self, key):
        if key in self.bodies:
            body = self.bodies[key]
            return f"{body[2]} ({body[0]}:{body[1]})"
        return self.nodes.get(key, self.roots.get(key, key))

    def explain(self, selector):
        target = self.select(selector)
        reached, edges = self.resolve()
        if target not in reached:
            return f"decode_path\t{self.location(target)}\tunreachable\n"
        predecessors = {key: None for key in sorted(self.roots)}
        pending = deque(predecessors)
        order = {kind: index for index, kind in enumerate(("direct call", "function address", "trait-object call", "generic instantiation", "unresolved-indirect candidate"))}
        while pending and target not in predecessors:
            caller = pending.popleft()
            for callee, kind in sorted(edges.get(caller, ()), key=lambda edge: (order[edge[1]], edge[0])):
                if callee not in predecessors:
                    predecessors[callee] = (caller, kind)
                    pending.append(callee)
        path = []
        current = target
        while predecessors[current] is not None:
            caller, kind = predecessors[current]
            path.append((caller, current, kind))
            current = caller
        rows = [f"decode_path_root\t{self.location(current)}"]
        rows.extend(f"decode_path_edge\t{kind}\t{self.location(caller)}\t{self.location(callee)}" for caller, callee, kind in reversed(path))
        return "\n".join(rows) + "\n"


def resolve_graph(source):
    graph = DecodeGraph(source)
    reached, _ = graph.resolve()
    return reached, graph.roots


def unreachable_bodies(source, reached):
    rows = set()
    for line in source.splitlines():
        fields = line.split("\t")
        if len(fields) == 7 and fields[0] == "decode_body":
            if fields[6] == "false" or fields[1] not in reached:
                rows.add("\t".join(("unreachable_decode_body", *fields[2:6])))
    return sorted(rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crate", action="append", default=[], help="select a decode crate")
    parser.add_argument("--output", type=Path, help="write sorted TSV findings or external operation inventory")
    parser.add_argument("--external-output", type=Path, help="also save the resolved operation inventory during a findings run")
    parser.add_argument("--list-externals", action="store_true", help="list resolved external operations with allocation and named work costs")
    parser.add_argument("--list-unreachable", action="store_true", help="list excluded production bodies with path, line, name and reason")
    parser.add_argument("--unreachable-output", type=Path, help="also save excluded production bodies during a findings run")
    parser.add_argument("--explain-body", help="print one shortest decode path to a definition name or PATH:LINE")
    parser.add_argument("--graph-output", type=Path, help="save compiler graph rows")
    parser.add_argument("--graph-input", type=Path, help="explain a body using saved compiler graph rows")
    args = parser.parse_args()
    if args.graph_input:
        if not args.explain_body:
            parser.error("--graph-input requires --explain-body")
        try:
            sys.stdout.write(DecodeGraph(args.graph_input.read_text()).explain(args.explain_body))
        except ValueError as error:
            parser.error(str(error))
        return 0
    toolchain = tomllib.loads((TOOL / "rust-toolchain.toml").read_text())["toolchain"]
    pin = toolchain["channel"]
    installed = subprocess.run(["rustup", "component", "list", "--toolchain", pin, "--installed"], cwd=ROOT, text=True, capture_output=True)
    components = toolchain["components"]
    names = {line.split()[0] for line in installed.stdout.splitlines()}
    if installed.returncode or any(not any(name == component or name.startswith(component.removesuffix("-preview") + "-") for name in names) for component in components):
        setup = subprocess.run(["rustup", "toolchain", "install", pin, "--profile", "minimal", *[argument for component in components for argument in ("--component", component)]], cwd=ROOT)
        if setup.returncode:
            return setup.returncode
    env = os.environ.copy()
    sysroot = subprocess.run(["rustc", f"+{pin}", "--print", "sysroot"], cwd=ROOT, text=True, capture_output=True, check=True).stdout.strip()
    env["LD_LIBRARY_PATH"] = str(Path(sysroot) / "lib") + os.pathsep + env.get("LD_LIBRARY_PATH", "")
    built = subprocess.run(["cargo", f"+{pin}", "build", "-q", "--manifest-path", str(TOOL / "Cargo.toml")], cwd=ROOT, env=env)
    if built.returncode:
        return built.returncode
    packages = []
    for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        name = tomllib.loads(manifest.read_text())["package"]["name"]
        if name.startswith("cadmpeg-codec-") or name in {"cadmpeg-core", "cadmpeg-ir", "cadmpeg-container", "cadmpeg-asm", "cadmpeg-parasolid", "cadmpeg-protein"}:
            packages.append(name)
    decode_packages = packages.copy()
    if args.crate:
        if set(args.crate) - set(packages):
            parser.error("--crate must name a decode crate")
        packages = args.crate
    target = ROOT / "target/decode-policy"
    # This target belongs to the checker. Remove its decode package
    # artifacts so cargo runs the driver on unchanged source too.
    def clean_packages():
        return subprocess.run(["cargo", f"+{pin}", "clean", "-q", "--target-dir", str(target), *[argument for name in decode_packages for argument in ("-p", name)]], cwd=ROOT, env=env).returncode

    def compile_packages(names):
        return subprocess.run(["cargo", f"+{pin}", "check", "-q", "--keep-going", "--lib", "--target-dir", str(target), *[argument for name in names for argument in ("-p", name)]], cwd=ROOT, env=env, text=True, capture_output=True)

    clean = clean_packages()
    if clean:
        return clean
    env["RUSTC_WORKSPACE_WRAPPER"] = str(TOOL / "target/debug/cadmpeg-decode-policy")
    env["CADMPEG_POLICY_COLLECT"] = "1"
    env["CADMPEG_POLICY_GRAPH"] = "1"
    graph = compile_packages(decode_packages)
    sys.stderr.write(graph.stderr)
    if graph.returncode:
        return graph.returncode
    if args.graph_output:
        args.graph_output.write_text(graph.stdout)
    if args.explain_body:
        try:
            sys.stdout.write(DecodeGraph(graph.stdout).explain(args.explain_body))
        except ValueError as error:
            parser.error(str(error))
        return 0
    reached, roots = resolve_graph(graph.stdout)
    scope = target / "decode-scope.txt"
    scope.write_text("".join(name + "\n" for name in sorted(reached)))
    (target / "decode-roots.txt").write_text("".join(name + "\n" for name in sorted(roots.values())))
    unreachable = unreachable_bodies(graph.stdout, reached)
    if args.crate:
        selected = {"crates/" + name + "/" for name in packages}
        unreachable = [line for line in unreachable if any(line.split("\t")[1].startswith(path) for path in selected)]
    excluded = "".join(line + "\n" for line in unreachable)
    if args.unreachable_output:
        args.unreachable_output.write_text(excluded)
    if args.list_unreachable:
        sys.stdout.write(excluded)
        if args.output:
            args.output.write_text(excluded)
        return 0
    del env["CADMPEG_POLICY_GRAPH"]
    env["CADMPEG_POLICY_SCOPE"] = str(scope)
    clean = clean_packages()
    if clean:
        return clean
    if args.list_externals or args.external_output:
        env["CADMPEG_POLICY_EXTERNALS"] = "1"
    result = compile_packages(packages)
    sys.stderr.write(result.stderr)
    if args.external_output:
        externals = sorted(set(line for line in result.stdout.splitlines() if line.startswith("external_operation\t")))
        args.external_output.write_text("".join(line + "\n" for line in externals))
    prefixes = ("external_operation\t",) if args.list_externals else ("uncharged_decode_allocation\t", "uncharged_decode_work\t", "unproven_decode_charge\t")
    findings = sorted(set(line for line in result.stdout.splitlines() if line.startswith(prefixes)))
    content = "".join(line + "\n" for line in findings)
    sys.stdout.write(content)
    if args.output:
        args.output.write_text(content)
    return result.returncode or int(any("\tMISSING\t" in line for line in findings) if args.list_externals else bool(findings))


if __name__ == "__main__":
    sys.exit(main())
