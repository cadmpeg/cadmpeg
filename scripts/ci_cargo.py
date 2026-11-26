#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run a Cargo gate with CI's selected packages without shell interpolation."""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path


def command(arguments: list[str], packages: list[str], *, full: bool = False,
            codecs: list[str] | None = None) -> list[str]:
    if not arguments or (not full and not packages):
        raise ValueError('Cargo gate requires a subcommand and selected packages')
    scope = ['--workspace'] if full else [argument for package in packages
                                         for argument in ('-p', package)]
    if codecs and not full:
        registry = Path(__file__).resolve().parent.parent / 'crates/cadmpeg-registry/Cargo.toml'
        features = tomllib.loads(registry.read_text())['features']
        codec_features = {
            dependency.removeprefix('dep:cadmpeg-codec-'): feature
            for feature, dependencies in features.items()
            for dependency in dependencies
            if dependency.startswith('dep:cadmpeg-codec-')
        }
        scope += ['--no-default-features', '--features', ','.join(
            f'{package}/{codec_features[codec]}' for codec in codecs
            for package in ('cadmpeg', 'cadmpeg-registry') if package in packages)]
    if arguments[0] == 'nextest':
        return ['cargo', 'nextest', *arguments[1:2], '--cargo-quiet', '--status-level', 'fail',
                '--final-status-level', 'slow', *scope, *arguments[2:]]
    return ['cargo', arguments[0], '-q', *scope, *arguments[1:]]


# Runners at or below this GiB ceiling run the test binaries through
# `cargo test` (see `low_ram_test_command`).
LOW_RAM_GIB = 10
# Windows runners at or below this GiB ceiling cap broad builds at two
# parallel jobs. Elsewhere `ci_rustc_lock.py` keeps the build inside RAM.
BROAD_BUILD_CAP_RAM_GIB = 18
# RAM left to the OS and the runner when sizing the rustc memory budget.
RESERVED_RAM_GIB = 2.5 if sys.platform == 'darwin' else 4.5


def low_ram_rustc_wrapper() -> Path:
    """Return the path to the memory-budget `RUSTC_WRAPPER` script."""
    return Path(__file__).resolve().parent / 'ci_rustc_lock.py'


def low_ram_test_command(arguments: list[str], *, platform: str) -> list[str]:
    """Return the full-workspace test command for a low-RAM runner.

    On macOS (`darwin`), `cargo test` runs each crate's tests in-process (34
    processes instead of 30,000+ Mach-O spawns of 300-500 MB binaries) and
    shares `LazyLock` fixture decode caches across tests in each binary.
    """
    if platform == 'darwin' and arguments[:2] == ['nextest', 'run']:
        return ['cargo', 'test', '-q', '--workspace', *arguments[2:]]
    return command(arguments, [], full=True)


def report_memory_budget(log: Path) -> None:
    """Print the budget's wait and each heavy unit's claimed and measured peak."""
    try:
        rows = [line.split('\t') for line in log.read_text().splitlines()]
    except OSError:
        return
    waited = sum(float(row[4]) for row in rows)
    print(f'rustc memory budget: {len(rows)} units, {waited:.0f} s waiting in total')
    print('crate\tkind\tclaimed GiB\tpeak GiB\twait s\trun s')
    for row in sorted(rows, key=lambda row: -max(float(row[2]), float(row[3])))[:30]:
        print('\t'.join(row))


def main() -> int:
    arguments = sys.argv[1:]
    packages = json.loads(os.environ['CI_PACKAGES'])
    full = os.environ.get('CI_FULL') == 'true'
    # These packages have no library target. Cargo accepts them alongside
    # libraries, preserving any features they enable.
    if (not full and arguments[:1] == ['test'] and '--doc' in arguments
            and set(packages) <= {'cadmpeg', 'cadmpeg-layout-tables'}):
        print('No selected package has library documentation tests.')
        return 0
    codecs = json.loads(os.environ.get('CI_CODEC_FEATURES', '[]'))
    ram_text = os.environ.get('CI_RUNNER_RAM_GIB', '').strip()
    ram_gib = int(ram_text) if ram_text.isdigit() else None
    env: dict[str, str] | None = None
    lock_dir: str | None = None
    argv = command(arguments, packages, full=full, codecs=codecs)
    if arguments[:2] == ['nextest', 'run'] and ram_gib is not None:
        env = os.environ.copy()
        if full and ram_gib <= LOW_RAM_GIB:
            argv = low_ram_test_command(arguments, platform=sys.platform)
        if sys.platform == 'win32':
            if full and ram_gib <= BROAD_BUILD_CAP_RAM_GIB:
                current_jobs = int(env.get('CARGO_BUILD_JOBS', '2'))
                env['CARGO_BUILD_JOBS'] = str(min(current_jobs, 2))
        else:
            lock_dir = tempfile.mkdtemp(prefix='cadmpeg-rustc-lock-')
            env['RUSTC_WRAPPER'] = str(low_ram_rustc_wrapper())
            env['CI_RUSTC_LOCK_FILE'] = str(Path(lock_dir) / 'budget')
            env['CI_RUSTC_MEMORY_GIB'] = str(ram_gib - RESERVED_RAM_GIB)
            env['CI_RUSTC_LOG'] = str(Path(lock_dir) / 'units.tsv')
    try:
        if env is not None:
            return subprocess.run(argv, env=env).returncode
        return subprocess.run(argv).returncode
    finally:
        if lock_dir is not None:
            report_memory_budget(Path(lock_dir) / 'units.tsv')
            shutil.rmtree(lock_dir, ignore_errors=True)


if __name__ == '__main__':
    raise SystemExit(main())
