#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Select changed workspace packages and their transitive dependents for CI.

Unknown inputs select the full gate. Python tooling changes still run all static
checks, but do not compile Rust. Main-branch pushes always select the full gate.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import tomllib


# These tools are covered by the complete Python suite and static CI checks.
# New tools remain unclassified until their build inputs have been reviewed.
PYTHON_ONLY = {
    'check-codec-facade.py', 'check-deny-census.py', 'check-dialect-support.py',
    'check-dialects.py', 'check-doc-anchors.py', 'check-fuzz-seeds.py',
    'check-golden-coverage-floors.py', 'check-source-policy.py',
    'census-panic-calls.py', 'dialect_support_data.py',
    'render-format-support.py', 'inventor-evidence.py',
}
PYTHON_ONLY |= {'test_' + name.replace('-', '_') for name in PYTHON_ONLY}


def select(root: Path, paths: list[str], *, full: bool = False) -> dict:
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    members = {}
    for pattern in manifest['workspace']['members']:
        for directory in root.glob(pattern):
            package = tomllib.loads((directory / 'Cargo.toml').read_text())
            members[directory.relative_to(root).as_posix()] = package
    names = {package['package']['name'] for package in members.values()}
    reverse = {name: set() for name in names}
    for package in members.values():
        tables = [package]
        tables.extend(package.get('target', {}).values())
        for table in tables:
            for kind in ('dependencies', 'dev-dependencies', 'build-dependencies'):
                for alias, dependency in table.get(kind, {}).items():
                    if isinstance(dependency, dict) and dependency.get('workspace'):
                        dependency = manifest['workspace']['dependencies'][alias]
                    name = dependency.get('package', alias) if isinstance(dependency, dict) else alias
                    if name in names:
                        reverse[name].add(package['package']['name'])
    affected = set()
    fuzz = False
    for path in paths:
        owner = next((directory for directory in sorted(members, key=len, reverse=True)
                      if path.startswith(directory + '/')), None)
        if owner:
            affected.add(members[owner]['package']['name'])
        elif path.startswith('crates/cadmpeg-fuzz/'):
            fuzz = True
        elif path == 'scripts/verify-iges-bounded.py':
            affected.add('cadmpeg-codec-iges')
        elif path.startswith('scripts/') and Path(path).name in PYTHON_ONLY:
            # These programs have their own tests and the complete static gate.
            continue
        elif path in {'AGENTS.md', 'LICENSE', '.gitignore'}:
            continue
        else:
            # Manifests, configuration, workflows, documentation, fixtures and
            # unclassified generators can affect multiple crates or test inputs.
            full = True
    if full:
        affected = names.copy()
        fuzz = True
    cli_changed = bool(affected & {'cadmpeg', 'cadmpeg-registry'})
    pending = list(affected)
    while pending:
        for dependent in reverse[pending.pop()]:
            if dependent not in affected:
                affected.add(dependent)
                pending.append(dependent)
    return {
        'packages': sorted(affected),
        'rust': bool(affected),
        'features': bool(affected) or fuzz,
        'schema': bool(affected & {'cadmpeg-core', 'cadmpeg-ir', 'cadmpeg-asm'}),
        'iges': 'cadmpeg-codec-iges' in affected or cli_changed,
        'full': full,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base')
    parser.add_argument('--full', action='store_true')
    parser.add_argument('--github-output', type=Path)
    args = parser.parse_args()
    paths = []
    if args.base:
        result = subprocess.run(['git', 'diff', '--no-renames', '--name-only', '-z', args.base, 'HEAD'],
                                check=True, capture_output=True)
        paths = [path.decode() for path in result.stdout.split(b'\0') if path]
    scope = select(Path(__file__).resolve().parent.parent, paths, full=args.full)
    print(json.dumps(scope, sort_keys=True))
    if args.github_output:
        with args.github_output.open('a') as output:
            for key, value in scope.items():
                output.write(f'{key}={json.dumps(value, separators=(",", ":"))}\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
