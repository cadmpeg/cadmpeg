#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run the pinned compiler's decode allocation and work checks."""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
TOOL = ROOT / "crates/cadmpeg-decode-policy"


def resolve_graph(source):
    reached = set()
    roots = {}
    edges = {}
    uncertain = set()
    addresses = set()
    for row in source.splitlines():
        fields = row.split("\t")
        if len(fields) == 3 and fields[0] == "decode_root":
            reached.add(fields[1])
            roots[fields[1]] = fields[2]
        elif len(fields) == 3 and fields[0] == "decode_edge":
            edges.setdefault(fields[1], set()).add(fields[2])
        elif len(fields) == 2 and fields[0] == "decode_uncertain":
            uncertain.add(fields[1])
        elif len(fields) == 2 and fields[0] == "decode_address":
            addresses.add(fields[1])
    for caller in uncertain:
        edges.setdefault(caller, set()).update(addresses)
    pending = list(reached)
    while pending:
        for callee in edges.get(pending.pop(), ()):
            if callee not in reached:
                reached.add(callee)
                pending.append(callee)
    return reached, roots


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crate", action="append", default=[], help="select a decode crate")
    parser.add_argument("--output", type=Path, help="write sorted TSV findings or external operation inventory")
    parser.add_argument("--external-output", type=Path, help="also save the resolved operation inventory during a findings run")
    parser.add_argument("--list-externals", action="store_true", help="list resolved external operations with allocation and named work costs")
    args = parser.parse_args()
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
    reached, roots = resolve_graph(graph.stdout)
    scope = target / "decode-scope.txt"
    scope.write_text("".join(name + "\n" for name in sorted(reached)))
    (target / "decode-roots.txt").write_text("".join(name + "\n" for name in sorted(roots.values())))
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
