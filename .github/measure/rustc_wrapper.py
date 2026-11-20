#!/usr/bin/env python3
"""Run rustc and append its wall time and peak RSS to $RUSTC_LOG."""
import os, platform, resource, subprocess, sys, time

args = sys.argv[1:]
start = time.time()
code = subprocess.run(args).returncode
wall = time.time() - start
rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
kib = rss // 1024 if platform.system() == "Darwin" else rss
name = args[args.index("--crate-name") + 1] if "--crate-name" in args else "-"
kind = "test" if "--test" in args else "lib/bin"
with open(os.environ["RUSTC_LOG"], "a") as log:
    log.write(f"{wall:.1f}\t{kib}\t{name}\t{kind}\n")
sys.exit(code)
