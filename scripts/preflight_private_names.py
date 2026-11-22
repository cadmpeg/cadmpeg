# SPDX-License-Identifier: Apache-2.0
"""The tree names no file of the maintainers' private test corpus.

What a private corpus file teaches goes into code and format documents; the
file's name does not. Where a private corpus checkout exists
(``$CADMPEG_PRIVATE``, default ``~/dev/cadmpeg-private``), its
``corpus/scan_names.py`` lists each tracked line that names a private corpus
file, by path, line and stem id, never by name. Elsewhere the check is
skipped.
"""

from __future__ import annotations

import os
from pathlib import Path

NAME = "private-names"
ORDER = 90

SCANNER = Path("corpus") / "scan_names.py"
SHOWN = 40


def scanner() -> Path:
    root = os.environ.get("CADMPEG_PRIVATE") or Path.home() / "dev" / "cadmpeg-private"
    return Path(root).expanduser() / SCANNER


def run(context):
    script = scanner()
    if not script.is_file():
        return context.skipped(f"no private corpus scanner at {script}")
    command = ["python3", "-I", str(script), "--repo", str(context.root)]
    if context.rev:
        command += ["--rev", context.rev]
    status, output = context.run(command)
    lines = output.splitlines()
    notes = [line for line in lines if line.startswith("scan_names:")]
    summary = notes[-1].removeprefix("scan_names:").strip() if notes else "no summary"
    if status == 0:
        return context.passed(summary)
    if status != 1:
        return context.failed(f"the private corpus scanner failed with exit status {status}",
                              *lines[-10:])
    hits = [line for line in lines if not line.startswith("scan_names:")]
    more = [f"... and {len(hits) - SHOWN} more"] if len(hits) > SHOWN else []
    return context.failed(
        summary, *hits[:SHOWN], *more,
        "fix: replace each name with a neutral description of the shape or feature it "
        f"illustrates; python3 {script} --names prints the names on this host only",
    )
