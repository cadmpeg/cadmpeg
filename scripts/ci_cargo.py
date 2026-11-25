#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run a Cargo gate with CI's selected packages without shell interpolation."""

import json
import os
import subprocess
import sys
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


# Runners at or below this many GiB build with at most two parallel jobs.
TWO_JOB_RAM_GIB = 18


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
    if (full and arguments[:2] == ['nextest', 'run'] and ram_gib is not None
            and ram_gib <= TWO_JOB_RAM_GIB):
        env = os.environ.copy()
        env['CARGO_BUILD_JOBS'] = str(min(int(env.get('CARGO_BUILD_JOBS', '2')), 2))
    argv = command(arguments, packages, full=full, codecs=codecs)
    if env is not None:
        return subprocess.run(argv, env=env).returncode
    return subprocess.run(argv).returncode


if __name__ == '__main__':
    raise SystemExit(main())
