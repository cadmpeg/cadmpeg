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


# Runners at or below this GiB ceiling serialize heavy rustc invocations via
# `ci_rustc_lock.py` instead of capping every crate to one job or splitting
# Cargo into multiple invocations that break workspace feature unification.
LOW_RAM_GIB = 10
# Runners at or below this GiB ceiling cap broad builds at two parallel jobs.
BROAD_BUILD_CAP_RAM_GIB = 18


def low_ram_rustc_wrapper() -> Path:
    """Return the path to the flock-based `RUSTC_WRAPPER` script."""
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
    if full and arguments[:2] == ['nextest', 'run'] and ram_gib is not None:
        env = os.environ.copy()
        if ram_gib <= LOW_RAM_GIB:
            current_jobs = int(env.get('CARGO_BUILD_JOBS', '2'))
            env['CARGO_BUILD_JOBS'] = str(min(current_jobs, 2))
            if sys.platform != 'win32':
                lock_dir = tempfile.mkdtemp(prefix='cadmpeg-rustc-lock-')
                env['RUSTC_WRAPPER'] = str(low_ram_rustc_wrapper())
                env['CI_RUSTC_LOCK_DIR'] = lock_dir
            argv = low_ram_test_command(arguments, platform=sys.platform)
        elif ram_gib <= BROAD_BUILD_CAP_RAM_GIB:
            current_jobs = int(env.get('CARGO_BUILD_JOBS', '2'))
            env['CARGO_BUILD_JOBS'] = str(min(current_jobs, 2))
    try:
        if env is not None:
            return subprocess.run(argv, env=env).returncode
        return subprocess.run(argv).returncode
    finally:
        if lock_dir is not None:
            shutil.rmtree(lock_dir, ignore_errors=True)


if __name__ == '__main__':
    raise SystemExit(main())
