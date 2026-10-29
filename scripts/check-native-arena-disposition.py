#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the native arena contract against production registries; --write renders it.

No test fixtures, generated goldens, or trial decoder outputs define this inventory.
The check compares the whole generated document byte for byte.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import difflib
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
DOCUMENT = Path('docs/native-arena-disposition.md')
TOKEN = re.compile(r'//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"|[A-Za-z_]\w*|\S')
NAME = re.compile(r'[a-z][a-z0-9_]*\Z')


class RegistryError(ValueError):
    """A source manifest is missing, ambiguous, or internally inconsistent."""


@dataclass(frozen=True)
class Registry:
    namespace: str
    source: str
    selector: str
    kind: str = 'constant'
    writer: bool = True


REGISTRIES = (
    Registry('catia', 'crates/cadmpeg-codec-catia/src/native.rs', 'define_catia_arenas', 'macro', False),
    Registry('creo', 'crates/cadmpeg-codec-creo/src/decode/native.rs', 'CreoArena', 'enum', False),
    Registry('f3d', 'crates/cadmpeg-codec-f3d/src/native.rs', 'F3D_ARENA_NAMES'),
    Registry('fcstd', 'crates/cadmpeg-codec-freecad/src/native.rs', 'ARENAS'),
    Registry('iges', 'crates/cadmpeg-codec-iges/src/native.rs', 'ARENAS'),
    Registry('inventor', 'crates/cadmpeg-codec-inventor/src/validate.rs', 'ARENAS', writer=False),
    Registry('nx', 'crates/cadmpeg-codec-nx/src/native/catalogue.rs', 'CATALOGUE', 'catalogue', False),
    Registry('rhino', 'crates/cadmpeg-codec-rhino/src/native_registry.rs', 'ARENAS'),
    Registry('sat', 'crates/cadmpeg-asm/src/brep/transfer.rs', 'ASM_NATIVE_ARENAS', writer=False),
    Registry('sldprt', 'crates/cadmpeg-codec-sldprt/src/native.rs', 'SLDPRT_ARENA_NAMES'),
    Registry('step', 'crates/cadmpeg-codec-step/src/reader/mod.rs', 'NATIVE_ARENAS'),
)


def tokens(source: str) -> list[str]:
    return [token for token in TOKEN.findall(source) if not token.startswith(('//', '/*'))]


def block(stream: list[str], start: int, opening: str) -> tuple[list[str], int]:
    closing = {'[': ']', '{': '}', '(': ')'}[opening]
    if stream[start] != opening:
        raise RegistryError(f'expected {opening!r}')
    depth = 1
    for end in range(start + 1, len(stream)):
        if stream[end] == opening:
            depth += 1
        elif stream[end] == closing:
            depth -= 1
            if depth == 0:
                return stream[start + 1:end], end + 1
    raise RegistryError(f'unclosed {opening!r} in production registry')


def constant(stream: list[str], name: str) -> list[str]:
    starts = [i for i in range(len(stream) - 1) if stream[i:i + 2] == ['const', name]]
    if len(starts) != 1:
        raise RegistryError(f'expected one production constant {name}, found {len(starts)}')
    cursor = starts[0] + 2
    while cursor < len(stream) and stream[cursor] != '=':
        cursor += 1
    cursor += 1
    if cursor < len(stream) and stream[cursor] == '&':
        cursor += 1
    return block(stream, cursor, '[')[0]


def quoted_name(token: str) -> str:
    if len(token) < 2 or token[0] != '"' or token[-1] != '"':
        raise RegistryError(f'arena must be a literal string, got {token!r}')
    name = token[1:-1]
    if not NAME.fullmatch(name):
        raise RegistryError(f'invalid arena name {name!r}')
    return name


def read_registry(root: Path, registry: Registry) -> list[str]:
    stream = tokens((root / registry.source).read_text())
    if registry.kind in ('constant', 'catalogue'):
        content = constant(stream, registry.selector)
        if registry.kind == 'constant':
            names = [quoted_name(token) for token in content if token != ',']
        else:
            names = [quoted_name(content[i + 2]) for i in range(len(content) - 2)
                     if content[i:i + 2] == ['arena', ':']]
    elif registry.kind == 'macro':
        # Only the declaration invocation has `name ! {`; recursive macro rules
        # use parentheses and never define additional production families.
        starts = [i for i in range(len(stream) - 2)
                  if stream[i:i + 3] == [registry.selector, '!', '{']]
        if len(starts) != 1:
            raise RegistryError(f'expected one {registry.selector}! declaration')
        content = block(stream, starts[0] + 2, '{')[0]
        names = []
        cursor = 0
        while cursor < len(content):
            if content[cursor] == ',':
                cursor += 1
                continue
            name = content[cursor]
            if not NAME.fullmatch(name) or content[cursor + 1] != ':':
                raise RegistryError(f'invalid family declaration near {name!r}')
            cursor += 2
            while content[cursor] != '{':
                cursor += 1
            body, cursor = block(content, cursor, '{')
            # CATIA_FAMILIES emits stored and flattened children; a schema-only
            # declaration with no storage/owner rule does not install an arena.
            if 'stored' in body or any(body[i:i + 2] == ['=', '>'] for i in range(len(body) - 1)):
                names.append(name)
    elif registry.kind == 'enum':
        starts = [i for i in range(len(stream) - 1)
                  if stream[i:i + 2] == ['impl', registry.selector]]
        if len(starts) != 1:
            raise RegistryError(f'expected one {registry.selector} implementation')
        body = block(stream, starts[0] + 2, '{')[0]
        matches = [i for i in range(len(body) - 1) if body[i:i + 2] == ['fn', 'as_str']]
        if len(matches) != 1:
            raise RegistryError(f'expected one {registry.selector}::as_str')
        cursor = matches[0]
        while body[cursor] != '{':
            cursor += 1
        arms = block(body, cursor, '{')[0]
        names = [quoted_name(arms[i + 2]) for i in range(len(arms) - 2)
                 if arms[i:i + 2] == ['=', '>']]
    else:
        raise RegistryError(f'unknown registry kind {registry.kind}')
    if not names:
        raise RegistryError(f'{registry.namespace}: empty production arena registry')
    if len(set(names)) != len(names):
        raise RegistryError(f'{registry.namespace}: duplicate production arena names')
    return sorted(names)


def render(root: Path) -> str:
    lines = [
        '# Native arena content and output contract', '',
        '<!-- Generated by scripts/check-native-arena-disposition.py --write. -->', '',
        'The inventories below come from the production storage and validation registries.',
        'They enumerate typed document records, including diagnostic records about retained',
        'native records. They do not establish that a decoder projected every source record',
        'or that an encoder writes every arena. Those are tested separately.', '',
        'For each native writer, **W1** re-emits its own typed records that have no neutral',
        'counterpart when their references resolve in the current document. **W2** regenerates',
        'records that have neutral counterparts from the current neutral arenas. Deleting a',
        'neutral record does not authorize replaying its obsolete native counterpart.',
        'A record whose current references cannot be resolved is dropped and reported.', '',
        '**W3** reports `Written`, `Regenerated`, or `Dropped` for each native arena, with',
        'record overrides where an arena contains different cases. The export report is the',
        'authority for the particular document and output. Foreign or undeclared populated',
        'arenas are dropped with `export:native.records-omitted` and per-arena counts.',
        'Diagnostic arenas never decide whether a neutral class is held.', '',
        'This table states the content and output contract. Inventory agreement alone is',
        'not proof of output preservation or native application acceptance; the wire and',
        'writer gates provide that evidence.', '',
        '| Namespace | Production arenas | Native output |',
        '| --- | --- | --- |',
    ]
    counts = []
    for registry in REGISTRIES:
        names = read_registry(root, registry)
        counts.append((registry.namespace, len(names)))
        arenas = ', '.join(f'`{name}`' for name in names)
        output = 'W1 for records without neutral counterparts; W2 for records with neutral counterparts; W3 reports the result.' if registry.writer else 'Decode only; no native encoder is registered.'
        lines.append(f'| `{registry.namespace}` | {arenas} | {output} |')
    lines += ['', '## Registry sources', '',
              '| Namespace | Arena count | Authoritative production declaration |',
              '| --- | ---: | --- |']
    for registry, (_, count) in zip(REGISTRIES, counts):
        lines.append(f'| `{registry.namespace}` | {count} | [`{registry.selector}`](../{registry.source}) |')
    lines += ['', 'The `sat` inventory is installed by the shared ASM transfer registry into the',
              'embedding format namespace. F3Z uses the F3D typed families for its component',
              'documents. CADIR carries registered namespaces but has no separate native arena',
              'inventory of its own.', '',
              'Run `python3 scripts/check-native-arena-disposition.py` to check this document.',
              'Run the same command with `--write` after changing a production registry.', '']
    return '\n'.join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args(argv)
    try:
        expected = render(args.root)
        destination = args.root / DOCUMENT
        if args.write:
            destination.write_text(expected)
            return 0
        actual = destination.read_text()
        if actual == expected:
            return 0
        sys.stderr.writelines(difflib.unified_diff(actual.splitlines(True), expected.splitlines(True),
                                                 fromfile=str(DOCUMENT), tofile='production registries'))
        return 1
    except (OSError, RegistryError, IndexError) as error:
        print(f'native arena registry check: {error}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
