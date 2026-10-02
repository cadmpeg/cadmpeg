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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crate", action="append", default=[], help="select a decode crate")
    parser.add_argument("--output", type=Path, help="write sorted TSV findings")
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
    selection = [argument for name in packages for argument in ("-p", name)]
    # This target belongs to the checker. Remove its decode package
    # artifacts so cargo runs the driver on unchanged source too.
    clean = subprocess.run(["cargo", f"+{pin}", "clean", "-q", "--target-dir", str(target), *[argument for name in decode_packages for argument in ("-p", name)]], cwd=ROOT, env=env)
    if clean.returncode:
        return clean.returncode
    env["RUSTC_WORKSPACE_WRAPPER"] = str(TOOL / "target/debug/cadmpeg-decode-policy")
    env["CADMPEG_POLICY_COLLECT"] = "1"
    result = subprocess.run(["cargo", f"+{pin}", "check", "-q", "--keep-going", "--lib", "--target-dir", str(target), *selection], cwd=ROOT, env=env, text=True, capture_output=True)
    sys.stderr.write(result.stderr)
    findings = sorted(set(line for line in result.stdout.splitlines() if line.startswith(("uncharged_decode_allocation\t", "uncharged_decode_work\t", "unproven_decode_charge\t"))))
    content = "".join(line + "\n" for line in findings)
    sys.stdout.write(content)
    if args.output:
        args.output.write_text(content)
    return result.returncode or int(bool(findings))


if __name__ == "__main__":
    sys.exit(main())
