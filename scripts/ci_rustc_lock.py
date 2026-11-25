#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Serialize memory-heavy rustc invocations on low-RAM runners via flock.

Cargo invokes this wrapper as `RUSTC_WRAPPER <rustc> <args...>`. Heavy crates
that peak above ~4 GiB RSS acquire an exclusive lock so they run alone without
swapping on 7 GiB runners. Medium-heavy crates (~2.5-3.5 GiB RSS) acquire a
shared heavy lock plus an exclusive medium lock so at most one runs alongside
lightweight crates. All other crates acquire only the shared heavy lock and
saturate every core.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

# Crates whose single-process rustc RSS peaks near or above 4 GiB.
EXCLUSIVE_CRATES = frozenset({
    'cadmpeg_ir',
    'cadmpeg_codec_f3d',
    'cadmpeg_codec_sldprt',
})

# Crates whose single-process rustc RSS peaks around 2.5-3.5 GiB.
MEDIUM_HEAVY_CRATES = frozenset({
    'cadmpeg_codec_nx',
    'cadmpeg_codec_catia',
    'cadmpeg_codec_creo',
})


def crate_name(argv: list[str]) -> str | None:
    """Return the `--crate-name` argument from a rustc command line, if any."""
    for index, arg in enumerate(argv):
        if arg == '--crate-name' and index + 1 < len(argv):
            return argv[index + 1]
        if arg.startswith('--crate-name='):
            return arg.split('=', 1)[1]
    return None


def main() -> int:
    if len(sys.argv) < 2:
        return 2
    rustc_argv = sys.argv[1:]
    name = crate_name(rustc_argv[1:])
    if name is None or sys.platform == 'win32':
        os.execvp(rustc_argv[0], rustc_argv)
    import fcntl

    lock_dir = Path(os.environ.get('CI_RUSTC_LOCK_DIR') or tempfile.gettempdir())
    lock_dir.mkdir(parents=True, exist_ok=True)
    heavy_path = lock_dir / 'cadmpeg-rustc-heavy.lock'
    medium_path = lock_dir / 'cadmpeg-rustc-medium.lock'

    if name in EXCLUSIVE_CRATES:
        # While holding LOCK_EX no other rustc runs, so clear Cargo's jobserver
        # env (whose tokens are held by blocked peer wrappers) and let LLVM
        # codegen saturate every runner core directly.
        env = os.environ.copy()
        env.pop('CARGO_MAKEFLAGS', None)
        env.pop('MAKEFLAGS', None)
        with heavy_path.open('a+') as heavy:
            fcntl.fcntl(heavy.fileno(), fcntl.F_SETFD, fcntl.FD_CLOEXEC)
            fcntl.flock(heavy.fileno(), fcntl.LOCK_EX)
            return subprocess.run(rustc_argv, env=env).returncode
    if name in MEDIUM_HEAVY_CRATES:
        env = os.environ.copy()
        env.pop('CARGO_MAKEFLAGS', None)
        env.pop('MAKEFLAGS', None)
        with heavy_path.open('a+') as heavy, medium_path.open('a+') as medium:
            fcntl.fcntl(heavy.fileno(), fcntl.F_SETFD, fcntl.FD_CLOEXEC)
            fcntl.fcntl(medium.fileno(), fcntl.F_SETFD, fcntl.FD_CLOEXEC)
            fcntl.flock(heavy.fileno(), fcntl.LOCK_SH)
            fcntl.flock(medium.fileno(), fcntl.LOCK_EX)
            return subprocess.run(rustc_argv, env=env).returncode
    with heavy_path.open('a+') as heavy:
        fcntl.fcntl(heavy.fileno(), fcntl.F_SETFD, fcntl.FD_CLOEXEC)
        fcntl.flock(heavy.fileno(), fcntl.LOCK_SH)
        return subprocess.run(rustc_argv, close_fds=False).returncode


if __name__ == '__main__':
    raise SystemExit(main())
