#!/usr/bin/env python3
"""USAGE (this block is the complete surface — no need to read further):

    cadir-grep.py [--list NAME] [--where PATH=VALUE ...] [--fields F1,F2,...]
                  [--count PATH] [--follow] [--join SPEC ...] [--sort F1,F2,...]
                  [--format table|tsv|json] [--limit N] FILE_OR_DIR...

    FILE_OR_DIR    *.cadir.json file(s) and/or dirs (globs *.cadir.json)
    --list NAME    list to scan: model.<NAME> or native.<codec>.arenas.<NAME>
                   (codec — sldprt, catia, ... — is auto-detected per file);
                   bare NAME ok if unambiguous, else use model.X / arenas.X.
                   OMIT --list to print available lists + lengths per file.
    --where P=V    filter records: dotted PATH equals str(V); repeatable, ANDed.
                   PATH~=SUB matches substring. Missing path never matches.
    --fields F,..  dotted fields to print (default: id kind family native_ref
                   feature_ref offset, absent fields omitted)
    --count PATH   tally values at PATH across matches instead of printing them
    --follow       resolve each match's native_ref/id against the OTHER side
                   (model<->arenas) and print joined records indented
    --join SPEC    CHILD:CHILDFIELD=PARENTFIELD[:F1,F2,...] — for each match,
                   print records of list CHILD (same file) whose CHILDFIELD
                   equals the match's PARENTFIELD, indented. Repeatable.
                   Child fields default to its scalar keys (max 8). Shown in
                   table and json (as _join); ignored with tsv/--count.
    --sort F1,F2   sort matches within each file by these dotted fields
                   (numbers before strings, absent last; never a TypeError)
    --format X     table (default, stem-prefixed) | tsv (<absent> holes) | json
    --limit N      max output lines / rows                 (default: 100)

Examples (tmp dirs churn; glob for a live *.cadir.json dir if these are gone):
    python3 scripts/cadir-grep.py ~/side2/tmp/sldprt-l6/current-native-cadir-v1/
    python3 scripts/cadir-grep.py --list arenas.features --where kind=Extrusion \\
        --fields id,name,input_class ~/side2/tmp/sldprt-l6/current-native-cadir-v1/
    python3 scripts/cadir-grep.py --list sketch_input_entities --count kind \\
        ~/side2/tmp/sldprt-l6/current-native-cadir-v1/
    python3 scripts/cadir-grep.py --list model.features --where ordinal=0 --follow \\
        ~/side2/tmp/sldprt-l6/current-native-cadir-v1/014700f0cce52691.cadir.json
    # features of one class, in ordinal order, with their input-lane names joined
    # by parent id — replaces the usual json.load + manual-filter python one-off:
    python3 scripts/cadir-grep.py --list arenas.features \\
        --where input_class=moMirrorPattern_c --sort ordinal \\
        --join arenas.feature_input_names:parent=id:name,ordinal \\
        ~/side2/tmp/sldprt-l6/sweep-v15/8c6727f13ec417ce.cadir.json
    # works for any codec's arenas (catia shown): records of one class joined
    # to their entity records:
    python3 scripts/cadir-grep.py --list arenas.object_graph_records \\
        --where class_name=2DPoint \\
        --join arenas.entity_records:id=entity_record \\
        ~/side2/tmp/catia/20260815-ancestor-dependencies/

Notes: JSON true/false/null match as both Python (True/False/None) and JSON
spellings. Dotted paths may index lists: lanes[0].names, points[-1] (absent /
out-of-range is simply no match). Unparseable files are skipped
and counted, not fatal. Exit 0 if anything matched (or discovery mode ran),
1 if inputs were readable but nothing matched, stderr + exit 1 for missing
paths, no parseable files, or an ambiguous bare --list name.
"""

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

MISSING = object()
DEFAULT_FIELDS = ["id", "kind", "family", "native_ref", "feature_ref", "offset"]


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


def load_doc(path):
    """Return the parsed dict, or None if unreadable/not a dict."""
    try:
        with open(path) as fh:
            doc = json.load(fh)
    except (OSError, json.JSONDecodeError, UnicodeDecodeError):
        return None
    return doc if isinstance(doc, dict) else None


def stem(path):
    name = path.name
    return name[: -len(".cadir.json")] if name.endswith(".cadir.json") else path.stem


def sides(doc):
    """Return (model_lists, arena_lists) as dicts of name -> list.

    Arenas come from native.<codec>.arenas; the codec key (sldprt, catia,
    ...) is auto-detected: first sorted key whose value has a dict "arenas".
    """
    model = doc.get("model")
    model = model if isinstance(model, dict) else {}
    native = doc.get("native")
    native = native if isinstance(native, dict) else {}
    arenas = {}
    for key in sorted(native):
        val = native[key]
        if isinstance(val, dict) and isinstance(val.get("arenas"), dict):
            arenas = val["arenas"]
            break
    model = {k: v for k, v in model.items() if isinstance(v, list)}
    arenas = {k: v for k, v in arenas.items() if isinstance(v, list)}
    return model, arenas


def resolve_list(doc, name):
    """Return (side, records) for --list NAME in this file, or (None, None).

    side is 'model' or 'arenas'. Exits with an error on ambiguous bare names.
    """
    model, arenas = sides(doc)
    if name.startswith("model."):
        key = name[len("model."):]
        return ("model", model[key]) if key in model else (None, None)
    if name.startswith("arenas."):
        key = name[len("arenas."):]
        return ("arenas", arenas[key]) if key in arenas else (None, None)
    in_m, in_a = name in model, name in arenas
    if in_m and in_a:
        print(f"error: list name {name!r} is ambiguous; use model.{name} or arenas.{name}",
              file=sys.stderr)
        sys.exit(1)
    if in_m:
        return "model", model[name]
    if in_a:
        return "arenas", arenas[name]
    return None, None


INDEXED_PART = re.compile(r"^([^\[\]]*)((?:\[-?\d+\])+)$")


def get_path(record, dotted):
    """Walk a dotted path through nested dicts/lists; MISSING if any step fails.

    A part may carry list indices: `lanes[0]`, `points[-1]`, bare `[2]`.
    A dict key that literally contains brackets still wins over indexing.
    """
    cur = record
    for part in dotted.split("."):
        if isinstance(cur, dict) and part in cur:
            cur = cur[part]
            continue
        m = INDEXED_PART.match(part)
        if not m:
            return MISSING
        name, idxs = m.group(1), m.group(2)
        if name:
            if not isinstance(cur, dict) or name not in cur:
                return MISSING
            cur = cur[name]
        for i in re.findall(r"\[(-?\d+)\]", idxs):
            if not isinstance(cur, list):
                return MISSING
            try:
                cur = cur[int(i)]
            except IndexError:
                return MISSING
    return cur


def render(value):
    """One-line display form of a JSON value."""
    if value is MISSING:
        return "<absent>"
    if isinstance(value, str):
        return value
    if isinstance(value, (dict, list)):
        return json.dumps(value, separators=(",", ":"), ensure_ascii=False)
    return str(value)


def value_matches(value, want, substring):
    if value is MISSING:
        return False
    s = str(value)
    forms = {s}
    if value is True:
        forms.add("true")
    elif value is False:
        forms.add("false")
    elif value is None:
        forms.add("null")
    if substring:
        return any(want in f for f in forms)
    return want in forms


def parse_wheres(raw):
    """Each --where arg -> (path, value, substring?). Malformed args exit 2."""
    out = []
    for arg in raw:
        if "~=" in arg:
            path, val = arg.split("~=", 1)
            out.append((path, val, True))
        elif "=" in arg:
            path, val = arg.split("=", 1)
            out.append((path, val, False))
        else:
            print(f"error: --where expects PATH=VALUE or PATH~=SUBSTR, got {arg!r}",
                  file=sys.stderr)
            sys.exit(2)
    return out


def matches(record, wheres):
    if not isinstance(record, dict):
        return False
    return all(value_matches(get_path(record, p), v, sub) for p, v, sub in wheres)


def sort_key(record, fields):
    """Total order over JSON values: numbers, then strings/other, absent last."""
    key = []
    for f in fields:
        v = get_path(record, f) if isinstance(record, dict) else MISSING
        if v is MISSING:
            key.append((1, 0, 0.0, ""))
        elif isinstance(v, (int, float)) and not isinstance(v, bool):
            key.append((0, 0, float(v), ""))
        else:
            key.append((0, 1, 0.0, render(v)))
    return key


def parse_joins(raw):
    """Each --join arg -> (child_list, child_field, parent_field, fields|None)."""
    out = []
    for arg in raw:
        child, colon, rest = arg.partition(":")
        child_field, eq, tail = rest.partition("=")
        parent_field, _, fieldspec = tail.partition(":")
        if not (child and colon and child_field and eq and parent_field):
            print("error: --join expects CHILD:CHILDFIELD=PARENTFIELD[:F1,F2,...], "
                  f"got {arg!r}", file=sys.stderr)
            sys.exit(2)
        fields = [f for f in fieldspec.split(",") if f] or None
        out.append((child, child_field, parent_field, fields))
    return out


def build_join_indexes(doc, joins, absent):
    """Per file: for each join spec, key CHILD's records by str(CHILDFIELD).

    Returns a list aligned with `joins`; None where CHILD is absent in this
    file (tallied in `absent` for one end-of-run note).
    """
    indexes = []
    for child, child_field, _pf, _fields in joins:
        side, records = resolve_list(doc, child)
        if side is None:
            absent[child] += 1
            indexes.append(None)
            continue
        index = {}
        for rec in records:
            if not isinstance(rec, dict):
                continue
            key = get_path(rec, child_field)
            if key is not MISSING:
                index.setdefault(render(key), []).append(rec)
        indexes.append(index)
    return indexes


def join_hits(rec, joins, indexes):
    """Yield (child_list, parent_value_rendered, child_record, fields|None)."""
    for (child, _cf, parent_field, fields), index in zip(joins, indexes):
        if index is None:
            continue
        pv = get_path(rec, parent_field)
        if pv is MISSING:
            continue
        for hit in index.get(render(pv), ()):
            yield child, render(pv), hit, fields


def child_fields(rec, explicit):
    """Explicit F1,F2 if given; else the child's short scalar keys (max 8)."""
    if explicit:
        return explicit
    return [k for k, v in rec.items()
            if not isinstance(v, (dict, list))
            and not (isinstance(v, str) and len(v) > 160)][:8]


def build_follow_index(doc, scanned_side):
    """Index the OTHER side's lists by each record's string id / native_ref."""
    model, arenas = sides(doc)
    other = arenas if scanned_side == "model" else model
    index = {}
    for list_name, records in other.items():
        for rec in records:
            if not isinstance(rec, dict):
                continue
            for key_field in ("id", "native_ref"):
                key = rec.get(key_field)
                if isinstance(key, str):
                    index.setdefault(key, []).append((list_name, rec))
    return index


def follow_hits(record, index):
    hits, seen = [], set()
    for key_field in ("native_ref", "id"):
        key = record.get(key_field)
        if not isinstance(key, str):
            continue
        for list_name, rec in index.get(key, ()):
            ident = (list_name, id(rec))
            if ident not in seen:
                seen.add(ident)
                hits.append((list_name, rec))
    return hits


def compact_record(record, fields):
    parts = []
    for f in fields:
        v = get_path(record, f)
        if v is not MISSING:
            parts.append(f"{f}={render(v)}")
    return " ".join(parts) or "(no requested fields present)"


def discovery(files, out):
    parsed = 0
    skipped = 0
    for path in files:
        doc = load_doc(path)
        if doc is None:
            skipped += 1
            continue
        parsed += 1
        model, arenas = sides(doc)
        out.emit(f"== {stem(path)}")
        for label, lists in (("model", model), ("arenas", arenas)):
            body = ", ".join(f"{k}={len(v)}" for k, v in sorted(lists.items()) if v)
            out.emit(f"  {label}: " + (body or "(none nonzero)"))
    if skipped:
        print(f"note: {skipped} unparseable file(s) skipped", file=sys.stderr)
    if not parsed:
        print("error: no parseable *.cadir.json inputs", file=sys.stderr)
        sys.exit(1)
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("paths", nargs="+", help="*.cadir.json files and/or directories")
    ap.add_argument("--list", dest="list_name", metavar="NAME",
                    help="list to scan (model.X / arenas.X / bare unambiguous name); "
                         "omit for discovery mode")
    ap.add_argument("--where", action="append", default=[], metavar="PATH=VALUE",
                    help="AND filter; PATH=VALUE exact or PATH~=SUBSTR")
    ap.add_argument("--fields", metavar="F1,F2,...",
                    help="comma-separated dotted fields to print")
    ap.add_argument("--count", metavar="PATH", help="tally values at PATH across matches")
    ap.add_argument("--follow", action="store_true",
                    help="join matches to the other side via native_ref/id")
    ap.add_argument("--join", action="append", default=[], metavar="SPEC",
                    help="CHILD:CHILDFIELD=PARENTFIELD[:F1,F2,...] — print matching "
                         "records of another list in the same file, indented")
    ap.add_argument("--sort", metavar="F1,F2,...",
                    help="sort matches within each file by these dotted fields")
    ap.add_argument("--format", choices=["table", "tsv", "json"], default="table")
    ap.add_argument("--limit", type=int, default=100, help="max output lines (default 100)")
    args = ap.parse_args()

    files = collect_inputs(args.paths)
    out = LineBudget(args.limit)

    if args.list_name is None:
        rc = discovery(files, out)
        out.finish()
        return rc

    wheres = parse_wheres(args.where)
    fields = [f for f in (args.fields or "").split(",") if f] or DEFAULT_FIELDS
    joins = parse_joins(args.join)
    sort_fields = [f for f in (args.sort or "").split(",") if f]
    if args.count and args.follow:
        print("note: --follow is ignored with --count", file=sys.stderr)
    if joins and (args.count or args.format == "tsv"):
        print("note: --join is ignored with --count and tsv format", file=sys.stderr)
        joins = []
    join_absent = Counter()

    parsed = 0
    skipped = 0
    list_found = False
    matched = 0
    rows_emitted = 0  # tsv/json rows
    tally = Counter()
    header_done = False

    for path in files:
        doc = load_doc(path)
        if doc is None:
            skipped += 1
            continue
        parsed += 1
        side, records = resolve_list(doc, args.list_name)
        if side is None:
            continue
        list_found = True
        index = None
        jindexes = None
        hits = [rec for rec in records if matches(rec, wheres)]
        if sort_fields:
            hits.sort(key=lambda r: sort_key(r, sort_fields))
        for rec in hits:
            matched += 1
            if args.count:
                tally[render(get_path(rec, args.count))] += 1
                continue
            if args.format == "table":
                out.emit(f"{stem(path)}  {compact_record(rec, fields)}")
                if args.follow and isinstance(rec, dict):
                    if index is None:
                        index = build_follow_index(doc, side)
                    for list_name, hit in follow_hits(rec, index):
                        out.emit(f"    -> {list_name}: "
                                 f"{compact_record(hit, ['id', 'kind', 'name'] + DEFAULT_FIELDS[2:])}")
                if joins and isinstance(rec, dict):
                    if jindexes is None:
                        jindexes = build_join_indexes(doc, joins, join_absent)
                    for child, pv, hit, jf in join_hits(rec, joins, jindexes):
                        out.emit(f"    -> {child}[{pv}]: "
                                 f"{compact_record(hit, child_fields(hit, jf))}")
            elif rows_emitted < args.limit:
                if args.format == "tsv":
                    if not header_done:
                        print("\t".join(["file"] + fields))
                        header_done = True
                    print("\t".join([stem(path)] + [render(get_path(rec, f)) for f in fields]))
                else:  # json
                    obj = {"file": stem(path)}
                    if args.fields:
                        for f in fields:
                            v = get_path(rec, f)
                            if v is not MISSING:
                                obj[f] = v
                    else:
                        obj.update(rec if isinstance(rec, dict) else {"value": rec})
                    if args.follow and isinstance(rec, dict):
                        if index is None:
                            index = build_follow_index(doc, side)
                        obj["_follow"] = [{"list": ln, "id": h.get("id")}
                                          for ln, h in follow_hits(rec, index)]
                    if joins and isinstance(rec, dict):
                        if jindexes is None:
                            jindexes = build_join_indexes(doc, joins, join_absent)
                        obj["_join"] = [
                            {"list": child, "key": pv,
                             **{f: get_path(hit, f) for f in child_fields(hit, jf)
                                if get_path(hit, f) is not MISSING}}
                            for child, pv, hit, jf in join_hits(rec, joins, jindexes)]
                    print(json.dumps(obj, ensure_ascii=False))
                rows_emitted += 1

    if skipped:
        print(f"note: {skipped} unparseable file(s) skipped", file=sys.stderr)
    if not parsed:
        print("error: no parseable *.cadir.json inputs", file=sys.stderr)
        sys.exit(1)
    if not list_found:
        print(f"note: list {args.list_name!r} not present in any input "
              "(run without --list to see available names)", file=sys.stderr)
    for child, n in join_absent.items():
        print(f"note: --join list {child!r} not present in {n} file(s)", file=sys.stderr)

    if args.count:
        for value, n in tally.most_common():
            out.emit(f"{n:7d}  {value}")
    elif args.format in ("tsv", "json") and matched > rows_emitted:
        print(f"note: {matched - rows_emitted} matching row(s) suppressed by --limit",
              file=sys.stderr)
    out.finish()
    return 0 if matched else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except BrokenPipeError:  # e.g. piped into head; not an error
        import os
        os.dup2(os.open(os.devnull, os.O_WRONLY), sys.stdout.fileno())
        sys.exit(141)
