#!/usr/bin/env python3
"""Compact summary/diff of *.report.json sweep-report directories (sldprt sweeps).

One directory: aggregate status / geometry_transferred / loss-severity counts,
top loss codes, and the worst-offender files (most blocking losses, then most
total losses). Two directories: per-file diff keyed by the shared stem
(`<stem>.report.json`), listing regressed and improved files. Output is
hard-capped (default ~100 lines) — prefer this over dumping raw report JSONs.

Examples:
    python3 scripts/report-diff.py ~/side2/tmp/sldprt-l6/sweep-reports-v3/
    python3 scripts/report-diff.py ~/side2/tmp/sldprt-l6/current-reports-v3/ \\
        ~/side2/tmp/sldprt-l6/post-cone-reports-v1/
    python3 scripts/report-diff.py --limit 200 ~/side2/tmp/sldprt-l6/typed-v4/ \\
        ~/side2/tmp/sldprt-l6/typed-v7-subset/

Unparseable files are skipped and counted, not fatal. Exit is non-zero only
when an input path is missing or a directory yields no usable reports.
"""

import argparse
import json
import sys
from collections import Counter
from pathlib import Path


class LineBudget:
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


def loss_code_str(loss):
    """Loss 'code' may be a dict {namespace, code, kind} or a bare string."""
    code = loss.get("code") if isinstance(loss, dict) else None
    if isinstance(code, dict):
        return str(code.get("code") or code.get("kind") or "?")
    return str(code) if code is not None else "?"


def load_dir(dirpath):
    """Return (records: {stem: summary-dict}, skipped: int)."""
    path = Path(dirpath).expanduser()
    if not path.is_dir():
        print(f"error: not a directory: {path}", file=sys.stderr)
        sys.exit(1)
    records, skipped = {}, 0
    for f in sorted(path.glob("*.report.json")):
        try:
            with open(f) as fh:
                d = json.load(fh)
        except (OSError, json.JSONDecodeError, UnicodeDecodeError):
            skipped += 1
            continue
        if not isinstance(d, dict):
            skipped += 1
            continue
        dr = d.get("decode_report") or {}
        losses = dr.get("losses") if isinstance(dr, dict) else None
        losses = losses if isinstance(losses, list) else []
        sev = Counter(str(l.get("severity", "?")) for l in losses if isinstance(l, dict))
        records[f.name[: -len(".report.json")]] = {
            "status": str(d.get("status")),
            "schema_version": d.get("schema_version"),
            "geom": (dr.get("geometry_transferred") if isinstance(dr, dict) else None),
            "losses": len(losses),
            "blocking": sev.get("blocking", 0),
            "severities": sev,
            "codes": Counter(loss_code_str(l) for l in losses if isinstance(l, dict)),
        }
    if not records:
        print(f"error: no parseable *.report.json in {path} ({skipped} skipped)", file=sys.stderr)
        sys.exit(1)
    return records, skipped


def top(counter, n=10):
    items = counter.most_common(n)
    rest = sum(counter.values()) - sum(c for _, c in items)
    s = ", ".join(f"{k}={v}" for k, v in items)
    if rest > 0:
        s += f", (+{rest} more)"
    return s or "(none)"


def score(rec):
    """Badness key: worse status > blocking losses > total losses."""
    return (0 if rec["status"] == "ok" else 1, rec["blocking"], rec["losses"])


def summarize(dirpath, records, skipped, out, offenders):
    out.emit(f"== {dirpath}: {len(records)} reports" + (f" ({skipped} unparseable skipped)" if skipped else ""))
    out.emit("  status: " + top(Counter(r["status"] for r in records.values())))
    out.emit("  schema_version: " + top(Counter(str(r["schema_version"]) for r in records.values())))
    out.emit("  geometry_transferred: " + top(Counter(str(r["geom"]) for r in records.values())))
    sev = Counter()
    codes = Counter()
    for r in records.values():
        sev.update(r["severities"])
        codes.update(r["codes"])
    out.emit(f"  losses: {sum(sev.values())} total across {sum(1 for r in records.values() if r['losses'])} files; "
             "severities: " + top(sev))
    out.emit("  top loss codes: " + top(codes))
    worst = sorted(records.items(), key=lambda kv: score(kv[1]), reverse=True)[:offenders]
    out.emit(f"  worst offenders (status/blocking/losses), top {offenders}:")
    for stem, r in worst:
        if score(r) == (0, 0, 0):
            break
        out.emit(f"    {stem}: status={r['status']} blocking={r['blocking']} losses={r['losses']}")


def diff(dir_a, recs_a, dir_b, recs_b, out, offenders):
    shared = sorted(set(recs_a) & set(recs_b))
    only_a, only_b = set(recs_a) - set(recs_b), set(recs_b) - set(recs_a)
    out.emit(f"== diff  A={dir_a} ({len(recs_a)})  ->  B={dir_b} ({len(recs_b)})")
    out.emit(f"  shared={len(shared)} only-in-A={len(only_a)} only-in-B={len(only_b)}")
    regressed, improved = [], []
    for stem in shared:
        a, b = recs_a[stem], recs_b[stem]
        if score(b) > score(a):
            regressed.append((stem, a, b))
        elif score(b) < score(a):
            improved.append((stem, a, b))
    regressed.sort(key=lambda t: (score(t[2]), t[0]), reverse=True)
    improved.sort(key=lambda t: (score(t[1]), t[0]), reverse=True)
    out.emit(f"  regressed: {len(regressed)}, improved: {len(improved)}, "
             f"unchanged: {len(shared) - len(regressed) - len(improved)}")

    def show(label, rows):
        out.emit(f"  {label} (top {offenders}):")
        for stem, a, b in rows[:offenders]:
            new_codes = set(b["codes"]) - set(a["codes"])
            hint = f" new-codes: {', '.join(sorted(new_codes)[:3])}" if new_codes else ""
            out.emit(f"    {stem}: status {a['status']}->{b['status']}, "
                     f"blocking {a['blocking']}->{b['blocking']}, losses {a['losses']}->{b['losses']}{hint}")
        if len(rows) > offenders:
            out.emit(f"    (+{len(rows) - offenders} more)")

    if regressed:
        show("regressed", regressed)
    if improved:
        show("improved", improved)
    if only_a and len(only_a) <= 10:
        out.emit("  only-in-A: " + ", ".join(sorted(only_a)))
    if only_b and len(only_b) <= 10:
        out.emit("  only-in-B: " + ", ".join(sorted(only_b)))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("dir_a", help="directory of *.report.json files")
    ap.add_argument("dir_b", nargs="?", help="second directory: diff A -> B instead of summarizing")
    ap.add_argument("--limit", type=int, default=100, help="max output lines (default 100)")
    ap.add_argument("--top", type=int, default=10, help="max offenders/diff rows per list (default 10)")
    args = ap.parse_args()

    out = LineBudget(args.limit)
    recs_a, skipped_a = load_dir(args.dir_a)
    if args.dir_b is None:
        summarize(args.dir_a, recs_a, skipped_a, out, args.top)
    else:
        recs_b, skipped_b = load_dir(args.dir_b)
        for d, s in ((args.dir_a, skipped_a), (args.dir_b, skipped_b)):
            if s:
                out.emit(f"  note: {s} unparseable files skipped in {d}")
        diff(args.dir_a, recs_a, args.dir_b, recs_b, out, args.top)
    out.finish()
    return 0


if __name__ == "__main__":
    sys.exit(main())
