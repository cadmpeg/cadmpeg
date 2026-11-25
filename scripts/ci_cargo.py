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


# Crates whose single-process rustc RSS exceeds ~3 GiB and therefore swap
# heavily when compiled concurrently on low-RAM runners (e.g. 7 GiB macOS).
HEAVY_CODECS = (
    'cadmpeg-codec-f3d',
    'cadmpeg-codec-sldprt',
    'cadmpeg-codec-nx',
    'cadmpeg-codec-catia',
    'cadmpeg-codec-creo',
)

# Lightweight and medium workspace crates that fit two concurrent rustc
# processes comfortably inside 7 GiB RAM once cadmpeg-ir is built.
MEDIUM_CRATES = (
    'cadmpeg-asm',
    'cadmpeg-parasolid',
    'cadmpeg-measure',
    'cadmpeg-tessellate',
    'cadmpeg-protein',
    'cadmpeg-test-support',
    'cadmpeg-codec-step',
    'cadmpeg-codec-iges',
    'cadmpeg-codec-freecad',
    'cadmpeg-codec-inventor',
    'cadmpeg-codec-rhino',
    'cadmpeg-codec-sat',
    'cadmpeg-codec-usd',
    'cadmpeg-codec-pvz',
    'cadmpeg-codec-solidedge',
    'cadmpeg-codec-edrawings',
    'cadmpeg-codec-onshape',
)

HEAVY_CRATES = ('cadmpeg-ir', *HEAVY_CODECS)

# Runners at or below this GiB ceiling pre-build heavy crates one at a time.
LOW_RAM_GIB = 10
# Runners at or below this GiB ceiling cap full builds at two parallel jobs.
BROAD_BUILD_CAP_RAM_GIB = 18


def low_ram_prebuild_commands(arguments: list[str]) -> list[tuple[int | None, list[str]]]:
    """Return `(jobs_override, argv)` pre-build stages for a full low-RAM nextest run.

    Each stage enables the workspace test-support features so Cargo reuses
    every compiled artifact without rebuilding during the final `--workspace`
    invocation.
    """
    extra = [arg for arg in arguments[2:] if arg != '--no-fail-fast']
    base = ['cargo', 'nextest', 'run', '--cargo-quiet', '--no-run']
    stage_core = [
        *base,
        '-p', 'cadmpeg-core',
        '-p', 'cadmpeg-container',
        '--features', 'cadmpeg-core/test-support',
        *extra,
    ]
    stage_ir = [*base, '-p', 'cadmpeg-ir', *extra]
    stage_medium = [
        *base,
        *(item for crate in MEDIUM_CRATES for item in ('-p', crate)),
        '--features',
        'cadmpeg-asm/test-support,cadmpeg-measure/test-support,cadmpeg-parasolid/test-support',
        *extra,
    ]
    stage_heavy = [
        *base,
        *(item for crate in HEAVY_CODECS for item in ('-p', crate)),
        *extra,
    ]
    return [(None, stage_core), (1, stage_ir), (2, stage_medium), (1, stage_heavy)]


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
    if full and arguments[:2] == ['nextest', 'run'] and ram_gib is not None:
        env = os.environ.copy()
        if ram_gib <= LOW_RAM_GIB:
            for jobs, stage_cmd in low_ram_prebuild_commands(arguments):
                stage_env = env.copy()
                if jobs is not None:
                    stage_env['CARGO_BUILD_JOBS'] = str(jobs)
                rc = subprocess.run(stage_cmd, env=stage_env).returncode
                if rc != 0:
                    return rc
            current_jobs = int(env.get('CARGO_BUILD_JOBS', '2'))
            env['CARGO_BUILD_JOBS'] = str(min(current_jobs, 2))
        elif ram_gib <= BROAD_BUILD_CAP_RAM_GIB:
            current_jobs = int(env.get('CARGO_BUILD_JOBS', '2'))
            env['CARGO_BUILD_JOBS'] = str(min(current_jobs, 2))
    argv = command(arguments, packages, full=full, codecs=codecs)
    if env is not None:
        return subprocess.run(argv, env=env).returncode
    return subprocess.run(argv).returncode


if __name__ == '__main__':
    raise SystemExit(main())
