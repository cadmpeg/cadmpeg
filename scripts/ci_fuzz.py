#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run fuzz checks for the selected codec dependencies."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
FUZZ = ROOT / 'crates/cadmpeg-fuzz'
MANIFEST = FUZZ / 'Cargo.toml'


def target_sources(path: Path, seen: set[Path] | None = None) -> str:
    root = seen is None
    seen = set() if seen is None else seen
    if path in seen:
        return ''
    seen.add(path)
    source = path.read_text()
    if re.search(r'#\s*\[[^]]*\bpath\s*=|\binclude\s*!\s*\(', source):
        raise ValueError(f'unsupported Rust module path in {path}')
    module_pattern = r'^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;'
    modules = re.findall(module_pattern, source, re.MULTILINE)
    if modules and re.search(r'^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{', source, re.MULTILINE):
        raise ValueError(f'unsupported inline module in {path}')
    for module in modules:
        directory = path.parent if root or path.name == 'mod.rs' else path.with_suffix('')
        candidates = [directory / f'{module}.rs', directory / module / 'mod.rs']
        child = next((candidate for candidate in candidates if candidate.is_file()), None)
        if child is None:
            raise ValueError(f'cannot resolve module {module} in {path}')
        source += '\n' + target_sources(child, seen)
    return source


def command(mode: str, codecs: list[str]) -> list[str]:
    if mode not in {'check', 'test'}:
        raise ValueError(f'unknown fuzz gate: {mode}')
    base = ['cargo', mode, '-q', '--manifest-path', str(MANIFEST)]
    if not codecs:
        return base + (['--all-targets', '--keep-going'] if mode == 'check' else [])
    manifest = tomllib.loads(MANIFEST.read_text())
    valid = set(manifest['features']) - {'default'}
    if not set(codecs) <= valid:
        raise ValueError('unknown codec feature')
    patterns = [re.compile(r'\bcadmpeg_codec_' + re.escape(codec.replace('-', '_')) + r'\b')
                for codec in codecs]
    targets = []
    for target in manifest['bin']:
        if mode == 'test' and target.get('test') is False:
            continue
        try:
            source = target_sources(FUZZ / target['path'])
        except ValueError:
            return command(mode, [])
        if any(pattern.search(source) for pattern in patterns):
            targets.extend(['--bin', target['name']])
    return base + ['--no-default-features', '--features', ','.join(codecs), '--lib', *targets] + (
        ['--keep-going'] if mode == 'check' else [])


def main() -> int:
    return subprocess.run(command(sys.argv[1], json.loads(os.environ['CI_FUZZ_CODECS']))).returncode


if __name__ == '__main__':
    raise SystemExit(main())
