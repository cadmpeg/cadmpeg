#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run a Cargo gate with CI's selected packages without shell interpolation."""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from dataclasses import dataclass, field
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

# Tests that run for most of the test step. A binary with any of them starts
# first, so they run beside the short tests instead of after them.
SLOW_TESTS = (
    'corpus_manifest::corpus_specimens_pass_check',
    'corpus_manifest::corpus_manifest_dialects_are_pinned_from_classification',
    'integration_tests::cc0_controller::',
    'integration_tests::measured_geometry::',
    'integration_tests::unreadable_shapes::',
    'golden_tests::golden_snapshots_hold',
    'integration_tests::csg_solids::a_sphere_and_a_torus_',
    'every_read_optional_field_refuses_a_null_spelling',
    'writer::tests::leaf_sweep::every_leaf_edit_of_',
    'writer::tests::edit_refusals::',
)
# A binary still running after this many seconds is killed as hung.
DEADLINE_SECONDS = 900
RESULT = re.compile(r'^test (\S+) \.\.\. (ok|FAILED|ignored)')
STILL_RUNNING = re.compile(r'^test (\S+) has been running for over 60 seconds$')


@dataclass
class Unit:
    name: str
    argv: list[str]
    cwd: str
    tests: int
    slow: bool
    log: Path = Path()
    process: subprocess.Popen | None = None
    started: float = 0.0
    seconds: float = 0.0
    outcome: str = ''
    lines: list[str] = field(default_factory=list)


def package_name(package_id: str) -> str:
    """Return the package name from a Cargo package ID specification."""
    fragment = package_id.rpartition('#')[2]
    if '@' in fragment:
        return fragment.partition('@')[0]
    return package_id.rpartition('#')[0].rstrip('/').rpartition('/')[2]


def test_binaries(messages: str) -> list[tuple[str, str, str]]:
    """Return (name, executable, package directory) for each built test binary."""
    binaries = []
    for line in messages.splitlines():
        if not line.startswith('{'):
            continue
        message = json.loads(line)
        if (message.get('reason') != 'compiler-artifact' or not message['profile']['test']
                or not message.get('executable')):
            continue
        package = package_name(message['package_id'])
        target = message['target']
        kind = target['kind'][0]
        name = (f"{package}::bin/{target['name']}" if kind == 'bin'
                else f"{package}::{target['name']}" if kind == 'test' else package)
        binaries.append((name, message['executable'], str(Path(message['manifest_path']).parent)))
    return binaries


def list_tests(executable: str, cwd: str) -> list[str]:
    listed = subprocess.run([executable, '--list', '--format', 'terse'], cwd=cwd,
                            capture_output=True, text=True, check=True).stdout
    return [line.removesuffix(': test') for line in listed.splitlines() if line.endswith(': test')]


def plan(binaries: list[tuple[str, str, str]], threads: int, split: bool) -> list[Unit]:
    """Return the processes to run, those holding a slow test first."""
    units = []
    for name, executable, cwd in binaries:
        tests = list_tests(executable, cwd)
        slow = sum(1 for test in tests if any(filter_ in test for filter_ in SLOW_TESTS))
        base = [executable, f'--test-threads={threads}']
        if split and slow:
            units.append(Unit(f'{name} (slow)', [*base, *SLOW_TESTS], cwd, slow, True))
            skips = [argument for filter_ in SLOW_TESTS for argument in ('--skip', filter_)]
            if len(tests) > slow:
                units.append(Unit(name, [*base, *skips], cwd, len(tests) - slow, False))
        elif tests:
            units.append(Unit(name, base, cwd, len(tests), slow > 0))
    units.sort(key=lambda unit: (not unit.slow, -unit.tests))
    return units


def finish(unit: Unit, now: float, timed_out: bool) -> None:
    unit.seconds = now - unit.started
    unit.lines = unit.log.read_text(errors='replace').splitlines()
    if timed_out:
        unit.outcome = 'TIMEOUT'
    else:
        unit.outcome = 'PASS' if unit.process.returncode == 0 else 'FAIL'
    status = f'{unit.outcome:>7} [{unit.seconds:7.1f}s] {unit.name} ({unit.tests} tests)'
    print(status, flush=True)
    if unit.outcome == 'PASS':
        return
    finished = {match.group(1) for line in unit.lines if (match := RESULT.match(line))}
    for line in unit.lines:
        if not RESULT.match(line) or ' ... FAILED' in line:
            print(f'    {line}')
    if timed_out:
        hung = [match.group(1) for line in unit.lines
                if (match := STILL_RUNNING.match(line)) and match.group(1) not in finished]
        print(f'    killed after {DEADLINE_SECONDS}s; still running: {", ".join(hung) or "unknown"}')


def run(units: list[Unit], jobs: int) -> int:
    """Run the units with at most `jobs` at a time; return the exit status."""
    pending = list(units)
    running: list[Unit] = []
    start = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='cadmpeg-tests-') as logs:
        while pending or running:
            while pending and len(running) < jobs:
                unit = pending.pop(0)
                unit.log = Path(logs) / f'{len(pending)}.log'
                with unit.log.open('w') as log:
                    unit.process = subprocess.Popen(unit.argv, cwd=unit.cwd, stdout=log,
                                                    stderr=subprocess.STDOUT)
                unit.started = time.monotonic()
                running.append(unit)
            time.sleep(0.1)
            now = time.monotonic()
            for unit in list(running):
                timed_out = now - unit.started > DEADLINE_SECONDS
                if timed_out:
                    unit.process.kill()
                    unit.process.wait()
                elif unit.process.poll() is None:
                    continue
                running.remove(unit)
                finish(unit, now, timed_out)
    wall = time.monotonic() - start
    failed = [unit for unit in units if unit.outcome != 'PASS']
    print(f'\n{sum(unit.tests for unit in units)} tests in {len(units)} processes, '
          f'{wall:.1f}s wall; slowest processes:')
    for unit in sorted(units, key=lambda unit: -unit.seconds)[:5]:
        print(f'  [{unit.seconds:7.1f}s] {unit.name}')
    slow = [f'{match.group(1)} ({unit.name})' for unit in units for line in unit.lines
            if (match := STILL_RUNNING.match(line))]
    if slow:
        print('tests running past 60 seconds:\n  ' + '\n  '.join(slow))
    for unit in failed:
        print(f'{unit.outcome}: {unit.name}')
    return 1 if failed else 0


def run_tests(build: list[str], env: dict[str, str] | None) -> int:
    """Build the test binaries with `build` (a `cargo test --no-run` command), then run them."""
    built = subprocess.run([*build, '--message-format=json-render-diagnostics'], env=env,
                           stdout=subprocess.PIPE, text=True)
    if built.returncode != 0:
        return built.returncode
    # Proc-macro test binaries link the standard library dynamically; Cargo
    # puts the toolchain's library directory on the loader path for them.
    variable = {'win32': 'PATH', 'darwin': 'DYLD_FALLBACK_LIBRARY_PATH'}.get(
        sys.platform, 'LD_LIBRARY_PATH')
    libdir = subprocess.run(['rustc', '--print', 'target-libdir'], capture_output=True,
                            text=True, check=True).stdout.strip()
    os.environ[variable] = os.pathsep.join(filter(None, [libdir, os.environ.get(variable)]))
    cores = os.cpu_count() or 1
    jobs = int(os.environ.get('CI_TEST_JOBS', cores))
    threads = int(os.environ.get('CI_TEST_THREADS', cores))
    split = os.environ.get('CI_TEST_SPLIT', '1') == '1'
    units = plan(test_binaries(built.stdout), threads, split)
    print(f'running {len(units)} test processes, {jobs} at a time, {threads} threads each',
          flush=True)
    return run(units, jobs)


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
    tests = arguments == ['run-tests']
    if tests:
        arguments = ['test', '--no-run', '--bins', '--tests']
    argv = command(arguments, packages, full=full, codecs=codecs)
    if full and arguments[:2] == ['test', '--no-run'] and ram_gib is not None:
        env = os.environ.copy()
        if ram_gib <= LOW_RAM_GIB:
            current_jobs = int(env.get('CARGO_BUILD_JOBS', '2'))
            env['CARGO_BUILD_JOBS'] = str(min(current_jobs, 2))
            if sys.platform != 'win32':
                lock_dir = tempfile.mkdtemp(prefix='cadmpeg-rustc-lock-')
                env['RUSTC_WRAPPER'] = str(low_ram_rustc_wrapper())
                env['CI_RUSTC_LOCK_DIR'] = lock_dir
        elif ram_gib <= BROAD_BUILD_CAP_RAM_GIB:
            current_jobs = int(env.get('CARGO_BUILD_JOBS', '2'))
            env['CARGO_BUILD_JOBS'] = str(min(current_jobs, 2))
    try:
        if tests:
            return run_tests(argv, env)
        if env is not None:
            return subprocess.run(argv, env=env).returncode
        return subprocess.run(argv).returncode
    finally:
        if lock_dir is not None:
            shutil.rmtree(lock_dir, ignore_errors=True)


if __name__ == '__main__':
    raise SystemExit(main())
