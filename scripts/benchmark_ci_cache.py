#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Compare cold and warm workspace compiler caches for the PR test gate."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def run(args: list[str], env: dict[str, str], **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(args, env=env, check=True, **kwargs)


def main() -> int:
    env = dict(os.environ)
    env.pop('RUSTC_WRAPPER', None)
    env['RUSTC_WORKSPACE_WRAPPER'] = 'sccache'
    env['SCCACHE_CACHE_SIZE'] = '400M'
    env['SCCACHE_DIR'] = tempfile.mkdtemp(prefix='ci-sccache-', dir=env['RUNNER_TEMP'])
    metadata = json.loads(run(['cargo', 'metadata', '--no-deps', '--format-version', '1'], env,
                              capture_output=True, text=True).stdout)
    members = set(metadata['workspace_members'])
    packages = sorted(package['name'] for package in metadata['packages'] if package['id'] in members)
    clean = ['cargo', 'clean', '-q'] + [item for name in packages for item in ('-p', name)]
    gate = ['cargo', 'nextest', 'run', '--cargo-quiet', '--status-level', 'fail',
            '-p', 'cadmpeg', '-p', 'cadmpeg-codec-inventor', '-p', 'cadmpeg-registry',
            '--bins', '--tests', '--no-fail-fast']
    target = Path('target/debug/deps')
    samples = []
    complete = False
    try:
        run(['sccache', '--start-server'], env)
        run(gate + ['--no-run'], env)
        run(['sccache', '--stop-server'], env)
        shutil.rmtree(env['SCCACHE_DIR'])
        for iteration in range(5):
            run(['sccache', '--start-server'], env)
            digests = []
            for mode in ('cold', 'warm'):
                run(clean, env)
                run(['sccache', '--zero-stats'], env)
                start = time.monotonic()
                result = subprocess.run(gate, env=env, capture_output=True, text=True, check=False)
                elapsed = time.monotonic() - start
                if result.returncode:
                    print(result.stdout, result.stderr, flush=True)
                    raise subprocess.CalledProcessError(result.returncode, gate)
                stats = json.loads(run(['sccache', '--show-stats', '--stats-format', 'json'], env,
                                       capture_output=True, text=True).stdout)
                libraries = sorted(target.glob('libcadmpeg_codec_inventor-*.rlib'))
                digest = [hashlib.sha256(path.read_bytes()).hexdigest() for path in libraries]
                if len(digest) != 1:
                    raise RuntimeError(f'expected one Inventor library, found {len(digest)}')
                digests.append(digest)
                summary = [line.strip() for line in result.stderr.splitlines() if 'tests run:' in line]
                sample = {'iteration': iteration, 'mode': mode, 'seconds': round(elapsed, 3),
                          'cache_bytes': stats['cache_size'],
                          'hits': stats['stats']['cache_hits']['counts'],
                          'misses': stats['stats']['cache_misses']['counts'],
                          'summary': summary}
                print(json.dumps(sample), flush=True)
                samples.append(sample)
            if digests[0] != digests[1]:
                raise RuntimeError('cached and uncached library artifacts differ')
            run(['sccache', '--stop-server'], env)
            if iteration != 4:
                shutil.rmtree(env['SCCACHE_DIR'])
        if any(not sample['summary'] for sample in samples):
            raise RuntimeError('missing nextest summary')
        if 'GITHUB_OUTPUT' in env:
            with open(env['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
                print(f"cache_path={env['SCCACHE_DIR']}", file=output)
        complete = True
        return 0
    finally:
        subprocess.run(['sccache', '--stop-server'], env=env, capture_output=True, check=False)
        if not complete:
            shutil.rmtree(env['SCCACHE_DIR'], ignore_errors=True)


if __name__ == '__main__':
    raise SystemExit(main())
