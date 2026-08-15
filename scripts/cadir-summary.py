#!/usr/bin/env python3
"""USAGE (this block is the complete surface — no need to read further):

    cadir-summary.py [--section all|model|arenas|features|configs]
                     [--limit N] FILE_OR_DIR...

    FILE_OR_DIR   *.cadir.json file(s) and/or dirs (globs *.cadir.json)
    --section     which section(s) to print         (default: all)
    --limit N     max output lines, hard cap        (default: 100)

Examples (tmp dirs churn; glob for a live *.cadir.json if these are gone):
    python3 scripts/cadir-summary.py ~/side2/tmp/sldprt-l6/current-typed-cadir-v1/01fcc5d98a109241.cadir.json
    python3 scripts/cadir-summary.py ~/side2/tmp/sldprt-l6/typed-v7-subset/
    python3 scripts/cadir-summary.py --section features --limit 200 \\
        ~/side2/tmp/sldprt-l6/current-typed-cadir-v1/01fcc5d98a109241.cadir.json

Prints per-file: model topology counts, native arena sizes, feature kinds,
configurations, unknown tallies. Prefer this over dumping raw JSON (>1 MB);
for single-item queries prefer `target/debug/cadmpeg query item` / `query summary`.
"""

import argparse
import json
import sys
from collections import Counter
from pathlib import Path


class LineBudget:
    """Print lines up to a cap; note how many were suppressed."""

    def __init__(self, limit):
        self.limit = limit
        self.used = 0
        self.suppressed = 0

    def emit(self, line=""):
        if self.used < self.limit:
            print(line)
            self.used += 1
        else:
            self.suppressed += 1

    def finish(self):
        if self.suppressed:
            print(f"... [{self.suppressed} more lines suppressed; re-run with --limit N to see more]")


def collect_inputs(paths):
    files = []
    for p in paths:
        path = Path(p).expanduser()
        if path.is_dir():
            found = sorted(path.glob("*.cadir.json"))
            if not found:
                print(f"error: no *.cadir.json files in directory: {path}", file=sys.stderr)
                sys.exit(1)
            files.extend(found)
        elif path.is_file():
            files.append(path)
        else:
            print(f"error: no such file or directory: {path}", file=sys.stderr)
            sys.exit(1)
    return files


def top_counter(counter, n=8):
    items = counter.most_common(n)
    rest = sum(counter.values()) - sum(c for _, c in items)
    s = ", ".join(f"{k}={v}" for k, v in items)
    if rest > 0:
        s += f", (+{rest} others)"
    return s


def summarize_file(path, out, section):
    try:
        with open(path) as fh:
            doc = json.load(fh)
    except (OSError, json.JSONDecodeError, UnicodeDecodeError) as exc:
        out.emit(f"== {path}  [UNREADABLE: {exc}]")
        return False
    if not isinstance(doc, dict):
        out.emit(f"== {path}  [unexpected top-level {type(doc).__name__}]")
        return False

    src = doc.get("source") or {}
    out.emit(f"== {path}")
    out.emit(f"  ir_version={doc.get('ir_version')} format={src.get('format')} "
             f"units={ (doc.get('units') or {}).get('length') }")

    model = doc.get("model") or {}
    if section in ("all", "model") and isinstance(model, dict):
        counts = {k: len(v) for k, v in model.items() if isinstance(v, list) and v}
        out.emit("  model: " + (", ".join(f"{k}={v}" for k, v in sorted(counts.items())) or "(empty)"))

    native = (doc.get("native") or {}).get("sldprt") or {}
    arenas = native.get("arenas") or {}
    if not isinstance(arenas, dict):
        arenas = {}
    if section in ("all", "arenas"):
        out.emit(f"  native.sldprt version={native.get('version')}")
        sizes = {k: len(v) for k, v in arenas.items() if isinstance(v, list) and v}
        out.emit("  arenas(nonzero): " + (", ".join(f"{k}={v}" for k, v in sorted(sizes.items())) or "(none)"))

    features = arenas.get("features") or []
    if section in ("all", "features") and isinstance(features, list):
        kinds = Counter()
        suppressed = 0
        for f in features:
            if isinstance(f, dict):
                kinds[str(f.get("kind", "?"))] += 1
                if f.get("suppressed"):
                    suppressed += 1
        out.emit(f"  features: {len(features)} total, {suppressed} suppressed; kinds: "
                 + (top_counter(kinds) or "(none)"))
        if section == "features":
            for f in features:
                if isinstance(f, dict):
                    out.emit(f"    [{f.get('ordinal')}] kind={f.get('kind')} name={f.get('name')!r} "
                             f"class={f.get('input_class')} suppressed={f.get('suppressed')}")

    configs = arenas.get("configurations") or []
    if section in ("all", "configs") and isinstance(configs, list):
        names = [c.get("name") for c in configs if isinstance(c, dict)]
        shown = names if section == "configs" else names[:6]
        tail = "" if len(names) <= len(shown) else f", (+{len(names)-len(shown)} more)"
        out.emit(f"  configurations: {len(configs)}: " + ", ".join(repr(n) for n in shown) + tail)

    unknowns = arenas.get("unknowns")
    if section == "all" and isinstance(unknowns, list) and unknowns:
        out.emit(f"  unknowns: {len(unknowns)}")

    interesting = bool(model) or bool(arenas)
    if not interesting:
        out.emit("  [WARNING: no 'model' or native arenas found — schema mismatch?]")
    return interesting


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("paths", nargs="+", help="*.cadir.json files and/or directories containing them")
    ap.add_argument("--section", choices=["all", "model", "arenas", "features", "configs"],
                    default="all", help="restrict output to one section (default: all)")
    ap.add_argument("--limit", type=int, default=100, help="max output lines (default 100)")
    args = ap.parse_args()

    files = collect_inputs(args.paths)
    out = LineBudget(args.limit)
    any_content = False
    for f in files:
        any_content |= summarize_file(f, out, args.section)
    out.finish()
    return 0 if any_content else 1


if __name__ == "__main__":
    sys.exit(main())
