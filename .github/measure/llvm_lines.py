#!/usr/bin/env python3
"""Summarise an LLVM IR file by function: lines and copies, grouped by generic base."""
import collections, re, sys

per_fn = collections.Counter()
copies = collections.Counter()
current = None
count = 0
for line in open(sys.argv[1], errors="replace"):
    if line.startswith("define "):
        match = re.search(r'@"?([^"(\s]+)"?\(', line)
        current = match.group(1) if match else "?"
        count = 0
    elif current is not None:
        count += 1
        if line.startswith("}"):
            name = current
            try:
                import subprocess
            except ImportError:
                pass
            per_fn[name] += count
            current = None
names = list(per_fn)
demangled = {}
try:
    import subprocess
    out = subprocess.run(["rustfilt"], input="\n".join(names), capture_output=True, text=True).stdout.split("\n")
    demangled = dict(zip(names, out))
except Exception:
    pass
grouped = collections.Counter()
grouped_copies = collections.Counter()
for name, lines in per_fn.items():
    text = demangled.get(name, name)
    base = re.sub(r"::h[0-9a-f]{16}$", "", text)
    base = re.sub(r"<[^<>]*>", "<_>", base)
    base = re.sub(r"<[^<>]*>", "<_>", base)
    grouped[base] += lines
    grouped_copies[base] += 1
total = sum(per_fn.values())
print(f"total lines {total} functions {len(per_fn)}")
for base, lines in grouped.most_common(60):
    print(f"{lines:9} {100*lines/total:5.1f}% {grouped_copies[base]:6}x  {base[:200]}")
