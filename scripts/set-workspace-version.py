#!/usr/bin/env python3
"""Set the cadmpeg workspace release version.

Rewrites the `[workspace.package]` version and every internal
`[workspace.dependencies]` cadmpeg-* requirement in the root `Cargo.toml` to
the given version, keeping them in lockstep (the cargo-dist release derives its
release set by matching the pushed git tag against these package versions, so
they must equal the tag).

This edits `Cargo.toml` only. Refresh `Cargo.lock` afterwards with
`cargo update --workspace`.

Usage:
    scripts/set-workspace-version.py 0.1.4
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# cargo-dist requires at least major.minor.patch; allow an optional
# pre-release / build suffix (e.g. 0.2.0-rc.1) for completeness.
VERSION_RE = re.compile(
    r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)

# A `[workspace.dependencies]` line for an internal path crate, e.g.
#   cadmpeg-ir = { path = "crates/cadmpeg-ir", version = "0.1.1" }
INTERNAL_DEP_RE = re.compile(
    r'^(?P<head>cadmpeg[\w-]*\s*=\s*\{[^}]*path\s*=\s*"crates/[^"]+"[^}]*version\s*=\s*")'
    r'(?P<ver>[^"]+)'
    r'(?P<tail>".*)$'
)


def bump(cargo_toml: Path, version: str) -> list[str]:
    """Rewrite versions in-place, returning a list of human-readable changes.

    Matching is done on line content with the newline held aside so it is never
    consumed or dropped by the version regex.
    """
    lines = cargo_toml.read_text().splitlines(keepends=True)
    section = None
    package_version_set = False
    changes: list[str] = []

    for i, raw in enumerate(lines):
        newline = raw[len(raw.rstrip("\r\n")):]
        line = raw[: len(raw) - len(newline)]
        stripped = line.strip()

        if stripped.startswith("[") and stripped.endswith("]"):
            section = stripped
            continue

        if section == "[workspace.package]" and not package_version_set:
            m = re.match(r'^(?P<head>version\s*=\s*")(?P<ver>[^"]+)(?P<tail>".*)$', line)
            if m:
                if m.group("ver") != version:
                    changes.append(
                        f"[workspace.package] version {m.group('ver')} -> {version}"
                    )
                lines[i] = f"{m.group('head')}{version}{m.group('tail')}{newline}"
                package_version_set = True
                continue

        if section == "[workspace.dependencies]":
            m = INTERNAL_DEP_RE.match(line)
            if m:
                crate = line.split("=", 1)[0].strip()
                if m.group("ver") != version:
                    changes.append(f"{crate} {m.group('ver')} -> {version}")
                lines[i] = f"{m.group('head')}{version}{m.group('tail')}{newline}"

    if not package_version_set:
        raise SystemExit(
            f"error: no version found in [workspace.package] of {cargo_toml}"
        )

    cargo_toml.write_text("".join(lines))
    return changes


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__)
        print("error: expected exactly one argument: the target version", file=sys.stderr)
        return 2

    version = argv[1].lstrip("v")
    if not VERSION_RE.match(version):
        print(
            f"error: '{argv[1]}' is not a major.minor.patch version (e.g. 0.1.4)",
            file=sys.stderr,
        )
        return 2

    repo_root = Path(__file__).resolve().parent.parent
    cargo_toml = repo_root / "Cargo.toml"
    if not cargo_toml.is_file():
        print(f"error: {cargo_toml} not found", file=sys.stderr)
        return 1

    changes = bump(cargo_toml, version)
    if changes:
        print(f"Set workspace version to {version}:")
        for change in changes:
            print(f"  {change}")
    else:
        print(f"Workspace already at {version}; no changes.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
