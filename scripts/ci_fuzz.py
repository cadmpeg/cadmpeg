#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the fuzz targets that use the selected codecs."""

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


def command(codecs: list[str]) -> list[str]:
    base = ['cargo', 'check', '-q', '--manifest-path', str(MANIFEST), '--keep-going']
    if not codecs:
        return base + ['--all-targets']
    manifest = tomllib.loads(MANIFEST.read_text())
    if not set(codecs) <= set(manifest['features']) - {'default'}:
        return command([])
    patterns = [re.compile(r'\bcadmpeg_codec_' + re.escape(codec.replace('-', '_')) + r'\b')
                for codec in codecs]
    targets = []
    for target in manifest['bin']:
        try:
            source = target_sources(FUZZ / target['path'])
        except ValueError:
            return command([])
        if any(pattern.search(source) for pattern in patterns):
            targets.extend(['--bin', target['name']])
    return base + ['--no-default-features', '--features', ','.join(codecs), '--lib', *targets]


def main() -> int:
    return subprocess.run(command(json.loads(os.environ['CI_FUZZ_CODECS']))).returncode


if __name__ == '__main__':
    raise SystemExit(main())
