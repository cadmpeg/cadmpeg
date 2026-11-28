# SPDX-License-Identifier: Apache-2.0
"""Find old CADIR version pins when the release version changes."""

import re

NAME = "version-pins"
ORDER = 20
VERSION_FILE = "crates/cadmpeg-ir/src/document.rs"
VERSION = re.compile(r'pub const IR_VERSION: &str = "(\d+)";')


def version(text):
    match = VERSION.search(text or "")
    return match.group(1) if match else None


def pinned(context, pattern, rev):
    command = ["grep", "-l", "-z", "-E", pattern]
    if rev:
        command.append(rev)
    result = context.git(*command, "--")
    if result.returncode not in (0, 1):
        raise RuntimeError(f"git grep failed: {result.stderr.strip()}")
    paths = result.stdout.rstrip("\0").split("\0") if result.stdout else []
    return {path.split(":", 1)[1] if rev else path for path in paths}


def run(context):
    old = version(context.read_base(VERSION_FILE))
    new = version(context.read(VERSION_FILE))
    if old is None or new is None:
        return context.skipped("CADIR version is unavailable")
    if old == new:
        return context.passed("CADIR version is unchanged")
    pattern = rf'"ir_version"[[:space:]]*:[[:space:]]*"{old}"'
    stale = sorted(pinned(context, pattern, context.base) & pinned(context, pattern, context.rev))
    if stale:
        return context.failed(f"CADIR version changed to {new}; old version pins remain", *stale)
    return context.passed("CADIR version pins are updated")
