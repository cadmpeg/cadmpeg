#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Tracked fixture discovery for the decode non-regression gate."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path, PurePosixPath
import json
import os
import subprocess


@dataclass(frozen=True)
class Fixture:
    path: str
    blob: str

    @property
    def key(self) -> tuple[str, ...]:
        parts = tuple(p for p in PurePosixPath(self.path).parts if p != 'damaged')
        return parts[:-1] + (PurePosixPath(parts[-1]).stem,)


def is_fixture(path: str) -> bool:
    parts = PurePosixPath(path).parts
    return (
        len(parts) >= 6 and parts[0] == 'crates'
        and parts[2:5] == ('tests', 'golden', 'fixtures')
    ) or (
        len(parts) == 5 and parts[:4] == ('crates', 'cadmpeg-codec-step', 'tests', 'fixtures')
        and parts[-1].endswith('.p21')
    ) or (
        len(parts) >= 4 and parts[:3] == ('corpus', 'freecad_fcstd', 'fixtures')
        and parts[-1].endswith('.FCStd')
    )


def discover(repo: Path, revision: str) -> dict[tuple[str, ...], Fixture]:
    listing = subprocess.run(
        ['git', '-C', str(repo), 'ls-tree', '-rz', '--full-tree', revision],
        check=True, stdout=subprocess.PIPE,
    ).stdout
    result = {}
    for entry in listing.split(b'\0'):
        if not entry:
            continue
        header, raw_path = entry.split(b'\t', 1)
        mode, kind, blob = header.decode('ascii').split()
        path = raw_path.decode('utf-8')
        if not is_fixture(path):
            continue
        if kind != 'blob' or mode not in {'100644', '100755'}:
            raise ValueError(f'fixture must be a regular tracked file: {path}')
        fixture = Fixture(path, blob)
        if fixture.key in result:
            raise ValueError(f'ambiguous fixture stem: {path} and {result[fixture.key].path}')
        result[fixture.key] = fixture
    return result


def pair_fixtures(base: dict, head: dict) -> tuple[list[tuple[Fixture, Fixture]], list[str]]:
    pairs = []
    failures = []
    for key, old in sorted(base.items()):
        new = head.get(key)
        if new is None:
            failures.append(f'fixture-missing: {old.path}')
        elif old.blob != new.blob:
            failures.append(f'fixture-bytes-changed: {old.path} -> {new.path}')
        else:
            pairs.append((old, new))
    return pairs, failures


def base_binary(repo: Path, configured: str | None) -> Path:
    """Require the executable published by this checkout's merge-base build."""
    expected = repo / 'target' / 'base' / 'cadmpeg'
    if configured is None or Path(configured).resolve() != expected.resolve():
        raise ValueError('CADMPEG_BASE_BIN must name scripts/base-build.sh output')
    commit = subprocess.check_output(
        ['git', '-C', str(repo), 'merge-base', 'HEAD', 'origin/main'], text=True,
    ).strip()
    marker = expected.parent / 'base.commit'
    if marker.read_text().strip() != commit:
        raise ValueError('base executable is not from the current merge base')
    source = repo / 'target' / 'base-src'
    built = subprocess.check_output(
        ['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True,
    ).strip()
    dirty = subprocess.check_output(
        ['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=normal'], text=True,
    )
    if built != commit or dirty:
        raise ValueError('base source changed after its build')
    if not expected.is_file() or not os.access(expected, os.X_OK):
        raise ValueError('base executable is missing or not executable')
    return expected


def dump_fixture(binary: Path, fixture: Path, output: Path, timeout_seconds: float = 60) -> dict:
    """Decode one input; an unavailable CLI or invalid successful output is fatal."""
    output.unlink(missing_ok=True)
    try:
        result = subprocess.run(
            [str(binary), 'dump', str(fixture), '-o', str(output)],
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            timeout=timeout_seconds, check=False,
        )
    except subprocess.TimeoutExpired:
        return {'decode_error': 'decode timed out'}
    if result.returncode:
        return {'decode_error': f'cadmpeg dump exited {result.returncode}'}
    value = json.loads(output.read_text())
    if not isinstance(value, dict) or not isinstance(value.get('ir_version'), str):
        raise ValueError(f'successful dump did not write a CADIR document: {output}')
    return value
