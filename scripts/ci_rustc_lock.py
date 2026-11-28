#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Keep a CI test build's concurrent rustc memory inside the runner's RAM.

Cargo invokes this wrapper as `RUSTC_WRAPPER <rustc> <args...>`. Each rustc
invocation first claims its expected peak memory from a budget shared by every
wrapper of the build, waits until that much is free, and only then runs. The
budget is a run of byte-range locks in one file, one byte per quarter GiB, so a
rustc that exits or crashes returns its share with its process.

Cargo's job count only bounds how many rustc run at once. The heaviest crates
peak above 4 GiB and most others below 1 GiB, so no single job count both fits
the 7 GiB macOS runner and keeps the 16 GiB runners' cores busy.
"""

from __future__ import annotations

import math
import os
import subprocess
import sys
import time
from pathlib import Path

if sys.platform == 'win32':
    import msvcrt
else:
    import fcntl
    import resource

SLOT_GIB = 0.25
WORKSPACE = Path(__file__).resolve().parent.parent

# Peak RSS in GiB of rustc and the linker it waits for, keyed by crate name
# and whether the unit is a test harness. Measured with `debug = 0` on the
# workspace's heaviest units; CI logs each unit's measured peak.
PEAK_GIB = {
    ('cadmpeg_codec_f3d', True): 9.78,
    ('cadmpeg_ir', True): 9.75,
    ('cadmpeg_ir', False): 8.10,
    ('cadmpeg_codec_f3d', False): 5.50,
    ('cadmpeg_codec_sldprt', True): 5.04,
    ('cadmpeg', True): 4.76,
    ('cadmpeg', False): 4.75,
    ('cadmpeg_codec_nx', True): 4.28,
    ('cadmpeg_registry', True): 3.75,
    ('integration', True): 3.75,
    ('cadmpeg_codec_inventor', True): 3.30,
    ('cadmpeg_codec_catia', True): 3.25,
    ('cadmpeg_codec_catia', False): 3.00,
    ('cadmpeg_codec_iges', True): 2.77,
    ('cadmpeg_codec_creo', True): 2.75,
    ('cadmpeg_codec_freecad', True): 2.52,
    ('cadmpeg_codec_sldprt', False): 2.5,
    ('cadmpeg_codec_ironcad', True): 2.29,
    ('cadmpeg_codec_onshape', True): 2.28,
    ('cadmpeg_codec_nx', False): 2.25,
    ('cadmpeg_codec_creo', False): 1.75,
    ('cadmpeg_codec_rhino', True): 1.5,
    ('cadmpeg_codec_step', True): 1.5,
    ('cadmpeg_codec_inventor', False): 1.5,
}
WORKSPACE_TEST_GIB = 1.25
WORKSPACE_LIB_GIB = 1.0
DEPENDENCY_GIB = 0.5


def crate_name(argv: list[str]) -> str | None:
    """Return the `--crate-name` argument from a rustc command line, if any."""
    for index, arg in enumerate(argv):
        if arg == '--crate-name' and index + 1 < len(argv):
            return argv[index + 1]
        if arg.startswith('--crate-name='):
            return arg.split('=', 1)[1]
    return None


def peak_gib(argv: list[str], manifest_dir: str) -> float:
    """Return the expected peak GiB of one rustc invocation."""
    name = crate_name(argv)
    test = '--test' in argv
    if (name, test) in PEAK_GIB:
        return PEAK_GIB[(name, test)]
    if Path(manifest_dir or '/').resolve().is_relative_to(WORKSPACE):
        return WORKSPACE_TEST_GIB if test else WORKSPACE_LIB_GIB
    return DEPENDENCY_GIB


def slots(gib: float, budget_gib: float) -> int:
    """Return the quarter-GiB slots a unit claims, at most the whole budget."""
    return max(1, min(math.ceil(gib / SLOT_GIB), int(budget_gib / SLOT_GIB)))


def try_lock(handle, offset: int) -> bool:
    if sys.platform == 'win32':
        handle.seek(offset)
        try:
            msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
        except OSError:
            return False
        return True
    try:
        fcntl.lockf(handle, fcntl.LOCK_EX | fcntl.LOCK_NB, 1, offset)
    except OSError:
        return False
    return True


def unlock(handle, offset: int) -> None:
    if sys.platform == 'win32':
        handle.seek(offset)
        msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
    else:
        fcntl.lockf(handle, fcntl.LOCK_UN, 1, offset)


def claim(handle, needed: int, total: int) -> None:
    """Hold `needed` of the `total` slot bytes (offsets 1..total).

    Byte 0 orders the claimants: the wrapper that holds it waits until enough
    slots are free, so a large claim is not starved by a stream of small ones.
    """
    while not try_lock(handle, 0):
        time.sleep(0.1)
    held: list[int] = []
    while True:
        for offset in range(1, total + 1):
            if len(held) == needed:
                break
            if offset not in held and try_lock(handle, offset):
                held.append(offset)
        if len(held) == needed:
            break
        time.sleep(0.1)
    unlock(handle, 0)


def main() -> int:
    if len(sys.argv) < 2:
        return 2
    rustc_argv = sys.argv[1:]
    lock_path = os.environ.get('CI_RUSTC_LOCK_FILE')
    budget_gib = float(os.environ.get('CI_RUSTC_MEMORY_GIB') or 0)
    if crate_name(rustc_argv[1:]) is None or not lock_path or budget_gib <= 0:
        return subprocess.run(rustc_argv, close_fds=False).returncode
    gib = peak_gib(rustc_argv[1:], os.environ.get('CARGO_MANIFEST_DIR', ''))
    needed = slots(gib, budget_gib)
    env = None
    if gib * 2 > budget_gib:
        # A unit that claims over half the budget runs beside small ones at
        # most. Without Cargo's jobserver its codegen uses every core instead
        # of only the tokens the waiting wrappers leave free.
        env = os.environ.copy()
        env.pop('CARGO_MAKEFLAGS', None)
        env.pop('MAKEFLAGS', None)
    with open(lock_path, 'a+b') as handle:
        started = time.monotonic()
        claim(handle, needed, int(budget_gib / SLOT_GIB))
        waited = time.monotonic() - started
        code = subprocess.run(rustc_argv, env=env, close_fds=False).returncode
    ran = time.monotonic() - started - waited
    log = os.environ.get('CI_RUSTC_LOG')
    if log:
        usage = 0 if sys.platform == 'win32' else resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        peak = usage / (1 << 30) if sys.platform == 'darwin' else usage / (1 << 20)
        kind = 'test' if '--test' in rustc_argv else 'lib'
        with open(log, 'a') as out:
            out.write(f'{crate_name(rustc_argv[1:])}\t{kind}\t{gib:.2f}\t{peak:.2f}\t'
                      f'{waited:.1f}\t{ran:.1f}\n')
    return code


if __name__ == '__main__':
    raise SystemExit(main())
