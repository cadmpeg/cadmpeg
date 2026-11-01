#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Select changed workspace packages and their transitive dependents for CI.

Unknown inputs select the full gate. Python tooling changes still run all static
checks, but do not compile Rust.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import tomllib


# These tools are covered by the complete Python suite and static CI checks.
# New tools remain unclassified until their build inputs have been reviewed.
PYTHON_ONLY = {
    'check-codec-facade.py', 'check-deny-census.py', 'check-dialect-support.py',
    'check-dialects.py', 'check-doc-anchors.py', 'check-fuzz-seeds.py',
    'check-golden-coverage-floors.py', 'check-resource-caps.py',
    'check-source-policy.py', 'census-panic-calls.py', 'dialect_support_data.py',
    'render-format-support.py',
}
PYTHON_ONLY |= {'test_' + name.replace('-', '_') for name in PYTHON_ONLY}
# The pre-PR checker, its per-check modules and their tests read no Rust build
# input: scripts/preflight.py, scripts/preflight_*.py and scripts/test_preflight*.py.
PREFLIGHT_TOOL = re.compile(r'scripts/(?:preflight(?:_\w+)?|test_preflight\w*)\.py')

# Files that no Rust target reads.
STATIC_ONLY = {
    'AGENTS.md', 'CLAUDE.md', 'CONTRIBUTING.md', 'LEGAL.md', 'LICENSE',
    'LICENSE-docs', 'README.md', 'SECURITY.md', '.gitignore',
    'docs/codec-facade.toml', 'docs/evaluations.toml',
    'docs/golden-coverage-floors.toml', 'corpus/README.md', 'corpus/manifest.example.toml',
    '.github/PULL_REQUEST_TEMPLATE.md', '.github/dependabot.yml',
}

# Single-codec CLI builds in the feature-gate job, by output name.
SINGLE_CODEC_GATES = {'sldprt_cli': 'cadmpeg-codec-sldprt', 'step_cli': 'cadmpeg-codec-step'}

# Read by the `cadmpeg` package's `layout_tables` test.
LAYOUT_TABLE_INPUTS = ('docs/formats/', 'docs/layouts/')

EMBEDDED_BY = {
    'docs/dialect-id-conformance.toml': 'cadmpeg-core',
    'docs/measurement.md': 'cadmpeg-measure',
}


def is_cache_key_input(path: str) -> bool:
    """Whether `path` is hashed into the Swatinem/rust-cache key."""
    name = Path(path).name
    return (name in {'Cargo.toml', 'Cargo.lock', 'rust-toolchain', 'rust-toolchain.toml'}
            or path.endswith('.cargo/config.toml'))


def ci_calls_workflows(root: Path) -> bool:
    workflow = root / '.github/workflows/ci.yml'
    return not workflow.is_file() or bool(re.search(
        r'\buses:\s*[\'"]?\./\.github/workflows/', workflow.read_text()))


def lock_dependents(old: str, new: str, names: set[str]) -> set[str]:
    """Return the workspace packages that reach a package whose lock entry changed."""
    graphs = []
    entries = []
    for text in (old, new):
        packages = tomllib.loads(text).get('package', [])
        reverse: dict[str, set[str]] = {}
        entry = set()
        for package in packages:
            dependencies = tuple(sorted(package.get('dependencies', [])))
            if 'source' in package:
                key = (package['name'], package['version'], package['source'])
            else:
                key = (package['name'],)
            entry.add((key, package.get('checksum'), dependencies))
            for dependency in dependencies:
                reverse.setdefault(dependency.split(' ')[0], set()).add(package['name'])
        graphs.append(reverse)
        entries.append(entry)
    changed = {key[0] for key, _, _ in entries[0] ^ entries[1]}
    reached = set(changed)
    for reverse in graphs:
        pending = list(changed)
        while pending:
            for dependent in reverse.get(pending.pop(), ()):
                if dependent not in reached:
                    reached.add(dependent)
                    pending.append(dependent)
    return reached & names


def select(root: Path, paths: list[str], *, full: bool = False,
           base_lock: str | None = None, cache_writer: bool = False,
           tested: bool = False) -> dict:
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    excluded = {directory for pattern in manifest['workspace'].get('exclude', [])
                for directory in root.glob(pattern)}
    members = {}
    for pattern in manifest['workspace']['members']:
        for directory in root.glob(pattern):
            if directory in excluded:
                continue
            package = tomllib.loads((directory / 'Cargo.toml').read_text())
            members[directory.relative_to(root).as_posix()] = package
    names = {package['package']['name'] for package in members.values()}
    reverse = {name: set() for name in names}
    required = {name: set() for name in names}
    for package in members.values():
        tables = [package]
        tables.extend(package.get('target', {}).values())
        for table in tables:
            for kind in ('dependencies', 'dev-dependencies', 'build-dependencies'):
                for alias, dependency in table.get(kind, {}).items():
                    optional = isinstance(dependency, dict) and dependency.get('optional', False)
                    if isinstance(dependency, dict) and dependency.get('workspace'):
                        dependency = manifest['workspace']['dependencies'][alias]
                    name = dependency.get('package', alias) if isinstance(dependency, dict) else alias
                    if name in names:
                        reverse[name].add(package['package']['name'])
                        if not optional:
                            required[package['package']['name']].add(name)
    fuzz_manifest = root / 'crates/cadmpeg-fuzz/Cargo.toml'
    fuzz_dependencies = (tomllib.loads(fuzz_manifest.read_text()).get('dependencies', {})
                         if fuzz_manifest.exists() else {})
    fuzz_inputs = {dependency.get('package', alias) if isinstance(dependency, dict) else alias
                   for alias, dependency in fuzz_dependencies.items()} & names
    if (cache_writer or tested) and any(is_cache_key_input(path) for path in paths):
        full = True
    elif tested:
        return {'packages': [], 'rust': False, 'fuzz': False,
                **dict.fromkeys(SINGLE_CODEC_GATES, False), 'schema': False,
                'iges': False, 'inventor_cli': False, 'layout': False, 'codec_scope': [], 'prose_only': False,
                'full': False, 'static': False, 'audit': False}
    affected = set()
    tested_only = set()
    layout = False
    fuzz = False
    audit = full
    for path in paths:
        if path.startswith('crates/cadmpeg-fuzz/'):
            fuzz = True
            continue
        if path == 'Cargo.lock' and base_lock is not None:
            audit = True
            affected |= lock_dependents(base_lock, (root / 'Cargo.lock').read_text(), names)
            continue
        if re.fullmatch(r'crates/cadmpeg-codec-[^/]+/README\.md', path):
            continue
        if Path(path).name in {'Cargo.toml', 'Cargo.lock'}:
            # Feature unification can change crates outside the dependency closure.
            audit = True
            full = True
            continue
        owner = next((directory for directory in sorted(members, key=len, reverse=True)
                      if path.startswith(directory + '/')), None)
        if owner:
            affected.add(members[owner]['package']['name'])
        elif path in EMBEDDED_BY:
            affected.add(EMBEDDED_BY[path])
        elif path.startswith(LAYOUT_TABLE_INPUTS):
            layout = True
        elif path == 'corpus/manifest.toml':
            tested_only.add('cadmpeg')
        elif path in {'scripts/inventor-evidence.py', 'scripts/test_inventor_evidence.py'}:
            affected.add('cadmpeg-codec-inventor')
        elif path == 'scripts/verify-iges-bounded.py':
            affected.add('cadmpeg-codec-iges')
        elif ((path.startswith('scripts/') and Path(path).name in PYTHON_ONLY)
              or PREFLIGHT_TOOL.fullmatch(path)):
            # These programs have their own tests and the complete static gate.
            continue
        elif path in STATIC_ONLY:
            continue
        elif path.startswith('docs/') and path.count('/') == 1 and path.endswith('.md'):
            continue
        elif (path.startswith('.github/workflows/')
              and path != '.github/workflows/ci.yml'
              and not ci_calls_workflows(root)):
            continue
        else:
            # Manifests, configuration, workflows, documentation, fixtures and
            # unclassified generators can affect multiple crates or test inputs.
            full = True
    if full:
        affected = names.copy()
        fuzz = True
    changed = set(affected)
    affected |= tested_only
    cli_changed = bool(changed & {'cadmpeg', 'cadmpeg-registry'})
    pending = list(affected)
    while pending:
        for dependent in reverse[pending.pop()]:
            if dependent not in affected:
                affected.add(dependent)
                pending.append(dependent)
    gates = {}
    for output, codec in SINGLE_CODEC_GATES.items():
        inputs = {'cadmpeg', codec}
        pending = list(inputs)
        while pending:
            for dependency in required.get(pending.pop(), ()):
                if dependency not in inputs:
                    inputs.add(dependency)
                    pending.append(dependency)
        gates[output] = bool(changed & inputs)
    return {
        'packages': sorted(affected),
        'rust': full or bool(affected),
        'fuzz': fuzz or bool(affected & fuzz_inputs),
        'codec_scope': sorted(name.removeprefix('cadmpeg-codec-') for name in affected
                              if name.startswith('cadmpeg-codec-'))
        if (changed and not full and not fuzz
            and all(name.startswith('cadmpeg-codec-') for name in changed)
            and all(name.startswith('cadmpeg-codec-') for name in affected & fuzz_inputs)) else [],
        **gates,
        'schema': full or bool(affected & {'cadmpeg-core', 'cadmpeg-ir', 'cadmpeg-asm'}),
        'iges': full or 'cadmpeg-codec-iges' in affected or cli_changed,
        'inventor_cli': full or 'cadmpeg-codec-inventor' in affected or cli_changed,
        'layout': layout and 'cadmpeg' not in affected and not full,
        'prose_only': bool(paths) and not full and all(
            path == 'README.md' or re.fullmatch(r'crates/cadmpeg-codec-[^/]+/README\.md', path)
            or path.startswith(LAYOUT_TABLE_INPUTS) for path in paths),
        'full': full,
        'static': True,
        'audit': audit or full,
    }


def base_lock_text(base: str) -> str | None:
    result = subprocess.run(['git', 'show', f'{base}:Cargo.lock'], capture_output=True)
    return result.stdout.decode() if result.returncode == 0 else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base')
    parser.add_argument('--full', action='store_true')
    parser.add_argument('--cache-writer', action='store_true')
    parser.add_argument('--tested', action='store_true')
    parser.add_argument('--github-output', type=Path)
    args = parser.parse_args()
    paths = []
    base_lock = None
    if args.base:
        result = subprocess.run(['git', 'diff', '--no-renames', '--name-only', '-z', args.base, 'HEAD'],
                                check=True, capture_output=True)
        paths = [path.decode() for path in result.stdout.split(b'\0') if path]
        base_lock = base_lock_text(args.base)
    scope = select(Path(__file__).resolve().parent.parent, paths, full=args.full,
                   base_lock=base_lock, cache_writer=args.cache_writer, tested=args.tested)
    print(json.dumps(scope, sort_keys=True))
    if args.github_output:
        with args.github_output.open('a') as output:
            for key, value in scope.items():
                output.write(f'{key}={json.dumps(value, separators=(",", ":"))}\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
