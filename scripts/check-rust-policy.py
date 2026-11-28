#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run Rust source policy, resource-cap, and panic checks with shared analysis."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path


def load(name: str, filename: str):
    path = Path(__file__).with_name(filename)
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise SystemExit(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


policy = load("shared_rust_source_policy", "check-source-policy.py")
caps = load("shared_resource_caps", "check-resource-caps.py")
panic = load("shared_panic_macros", "census-panic-calls.py")


@policy.analysis_session
def scan():
    """Return the findings from the checks over one source snapshot."""
    sources = policy.read_sources()
    source_findings = policy.check_source(sources, workers=1)
    _, cap_findings = caps.census(caps.ROOT, sources)
    panic_findings = panic.panic_findings(sources)
    return source_findings, cap_findings, panic_findings


def main() -> int:
    source_findings, cap_findings, panic_findings = scan()
    for finding in source_findings:
        print(f"{finding.path}:{finding.line}: {finding.rule}: {finding.message}")
    for finding in cap_findings:
        print(finding, file=sys.stderr)
    for finding in panic_findings:
        print(finding)
    if source_findings or cap_findings or panic_findings:
        return 1
    print("rust policy: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
