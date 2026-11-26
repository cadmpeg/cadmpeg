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


# `run-tests` runs each test binary as one process with all its tests, so a
# `LazyLock` fixture is decoded once per binary, not once per test, and runs
# as many binaries at a time as the runner has cores. A binary still running
# after this many seconds is killed as hung.
DEADLINE_SECONDS = 1200
# A test that runs this many seconds or more is listed with its time.
SLOW_SECONDS = 15
# `--report-time` prints each test's time. It is a stable toolchain's
# unstable libtest option, which `RUSTC_BOOTSTRAP` unlocks for the test
# process only; nothing is compiled under it.
REPORT_TIME = ['-Zunstable-options', '--report-time']
RESULT = re.compile(r'^test (\S+) \.\.\. (ok|FAILED|ignored)(?:.* <([0-9.]+)s>)?')
STILL_RUNNING = re.compile(r'^test (\S+) has been running for over 60 seconds$')


@dataclass
class Binary:
    name: str
    argv: list[str]
    cwd: str
    tests: int
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
    found = []
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
        found.append((name, message['executable'], str(Path(message['manifest_path']).parent)))
    return found


def count_tests(executable: str, cwd: str) -> int:
    listed = subprocess.run([executable, '--list', '--format', 'terse'], cwd=cwd,
                            capture_output=True, text=True, check=True).stdout
    return sum(1 for line in listed.splitlines() if line.endswith(': test'))


def plan(built: list[tuple[str, str, str]], threads: int) -> list[Binary]:
    """Return a process per binary with tests, the binaries with the most tests first."""
    binaries = []
    for name, executable, cwd in built:
        if tests := count_tests(executable, cwd):
            binaries.append(Binary(name, [executable, f'--test-threads={threads}', *REPORT_TIME],
                                   cwd, tests))
    binaries.sort(key=lambda binary: -binary.tests)
    return binaries


def finish(binary: Binary, now: float, timed_out: bool) -> None:
    binary.seconds = now - binary.started
    binary.lines = binary.log.read_text(errors='replace').splitlines()
    if timed_out:
        binary.outcome = 'TIMEOUT'
    else:
        binary.outcome = 'PASS' if binary.process.returncode == 0 else 'FAIL'
    status = f'{binary.outcome:>7} [{binary.seconds:7.1f}s] {binary.name} ({binary.tests} tests)'
    print(status, flush=True)
    if binary.outcome == 'PASS':
        return
    for line in binary.lines:
        if not RESULT.match(line) or ' ... FAILED' in line:
            print(f'    {line}')
    if timed_out:
        finished = {match.group(1) for line in binary.lines if (match := RESULT.match(line))}
        hung = [match.group(1) for line in binary.lines
                if (match := STILL_RUNNING.match(line)) and match.group(1) not in finished]
        print(f'    killed after {DEADLINE_SECONDS}s; still running: {", ".join(hung) or "unknown"}')


def run(binaries: list[Binary], jobs: int) -> int:
    """Run the binaries with at most `jobs` at a time; return the exit status."""
    env = {
        **os.environ,
        'RUSTC_BOOTSTRAP': '1',
        'CADMPEG_THREADS': os.environ.get('CADMPEG_THREADS') or '1',
    }
    pending = list(binaries)
    running: list[Binary] = []
    start = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='cadmpeg-tests-') as logs:
        while pending or running:
            while pending and len(running) < jobs:
                binary = pending.pop(0)
                binary.log = Path(logs) / f'{len(pending)}.log'
                with binary.log.open('w') as log:
                    binary.process = subprocess.Popen(binary.argv, cwd=binary.cwd, stdout=log,
                                                      stderr=subprocess.STDOUT, env=env)
                binary.started = time.monotonic()
                running.append(binary)
            time.sleep(0.1)
            now = time.monotonic()
            for binary in list(running):
                timed_out = now - binary.started > DEADLINE_SECONDS
                if timed_out:
                    binary.process.kill()
                    binary.process.wait()
                elif binary.process.poll() is None:
                    continue
                running.remove(binary)
                finish(binary, now, timed_out)
    wall = time.monotonic() - start
    failed = [binary for binary in binaries if binary.outcome != 'PASS']
    print(f'\n{sum(binary.tests for binary in binaries)} tests in {len(binaries)} binaries, '
          f'{wall:.1f}s wall; slowest binaries:')
    for binary in sorted(binaries, key=lambda binary: -binary.seconds)[:5]:
        print(f'  [{binary.seconds:7.1f}s] {binary.name}')
    slow = sorted(((float(match.group(3)), binary.name, match.group(1))
                   for binary in binaries for line in binary.lines
                   if (match := RESULT.match(line)) and match.group(3)
                   and float(match.group(3)) >= SLOW_SECONDS), reverse=True)
    if slow:
        print(f'tests that ran {SLOW_SECONDS}s or more:')
        for seconds, name, test in slow:
            print(f'  [{seconds:7.1f}s] {name} {test}')
    for binary in failed:
        print(f'{binary.outcome}: {binary.name}')
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
    binaries = plan(test_binaries(built.stdout), cores)
    print(f'running {len(binaries)} test binaries, {cores} at a time, {cores} threads each',
          flush=True)
    return run(binaries, cores)


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
