#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Census tests from merge-base and 8702b20bd missing at HEAD, with citations.

Only #242 §9.1's five exact renames are recognized. Test moves preserve names.
Every missing row reports its deleting commit and section citations; citations
are necessary, not proof that the cited section removes the asserted behavior.
That semantic review remains explicit in the output, never an automatic waiver.
"""

from __future__ import annotations

import argparse
import ast
from collections import Counter, defaultdict
from dataclasses import dataclass
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parent.parent
CHECKPOINT = '8702b20bd'
RENAMES = {
    ('cadmpeg-codec-sldprt', 'unrepresented_source_content_never_reaches_patch_or_replay'): 'unrepresented_classes_content_never_reaches_patch_or_replay',
    ('cadmpeg-codec-sldprt', 'unrepresented_source_synthesizes_without_patch_provenance'): 'unrepresented_classes_synthesizes_without_patch_provenance',
    ('cadmpeg-codec-sldprt', 'unrepresented_source_does_not_redecode_retained_bytes_for_content'): 'unrepresented_classes_does_not_redecode_retained_bytes_for_content',
    ('cadmpeg-codec-iges', 'unrepresented_source_never_reaches_replay'): 'unrepresented_classes_never_reaches_replay',
    ('cadmpeg-codec-freecad', 'neutral_edits_and_unrepresented_source_never_reach_the_patch_path'): 'neutral_edits_and_unrepresented_classes_never_reach_the_patch_path',
}
LEX = re.compile(r'//[^\n]*|/\*[\s\S]*?\*/|(?:br|r)(?P<h>\#*)".*?"(?P=h)|b?"(?:\\.|[^"\\])*"|b?\'(?:\\.|[^\'\\])\'', re.S)
FUNCTION = re.compile(r'(?P<attrs>(?:#\[[^\]]+\]\s*)+)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(?P<name>\w+)\s*\(')
TEST_ATTRIBUTE = re.compile(r'#\[\s*(?:[A-Za-z_]\w*::)*test(?:\s*\([^\]]*\))?\s*\]')


class CensusError(ValueError):
    """Missing history or an unresolvable test declaration is a check failure."""


@dataclass(frozen=True)
class Test:
    owner: str
    name: str
    path: str
    line: int
    macro: str | None = None


def git(root: Path, *args: str, data: bytes | None = None) -> bytes:
    result = subprocess.run(['git', *args], cwd=root, input=data, capture_output=True, check=False)
    if result.returncode:
        raise CensusError(result.stderr.decode(errors='replace').strip())
    return result.stdout


def mask(source: str) -> str:
    result = []
    cursor = 0
    while match := LEX.search(source, cursor):
        result.append(source[cursor:match.start()])
        end = match.end()
        if match[0].startswith('/*'):
            depth = 1
            end = match.start() + 2
            while depth:
                delimiter = re.search(r'/\*|\*/', source[end:])
                if delimiter is None:
                    raise CensusError('unclosed Rust block comment')
                depth += 1 if delimiter[0] == '/*' else -1
                end += delimiter.end()
        result.append(''.join('\n' if char == '\n' else ' ' for char in source[match.start():end]))
        cursor = end
    return ''.join(result) + source[cursor:]


def close(text: str, start: int) -> int:
    opening = text[start]
    closing = {'(': ')', '{': '}', '[': ']'}[opening]
    stack = [closing]
    for index in range(start + 1, len(text)):
        if text[index] in '({[':
            stack.append({'(': ')', '{': '}', '[': ']'}[text[index]])
        elif text[index] in ')}]':
            if not stack or stack.pop() != text[index]:
                raise CensusError('unbalanced Rust token tree')
            if not stack:
                return index
    raise CensusError('unclosed Rust token tree')


def arguments(text: str) -> list[str]:
    result = []
    start = 0
    index = 0
    while index < len(text):
        if text[index] in '({[':
            index = close(text, index)
        elif text[index] == ',':
            result.append(text[start:index].strip())
            start = index + 1
        index += 1
    if text[start:].strip():
        result.append(text[start:].strip())
    return result


def owner(path: str) -> str:
    parts = Path(path).parts
    return parts[1] if parts[0] == 'crates' and len(parts) > 1 else parts[0]


def source_blobs(root: Path, ref: str, scope: str | None = None) -> dict[str, str]:
    entries = git(root, 'ls-tree', '-r', '-z', ref, *([scope] if scope else [])).split(b'\0')
    objects = []
    for entry in entries:
        if not entry:
            continue
        metadata, encoded_path = entry.split(b'\t', 1)
        path = encoded_path.decode()
        if path.endswith('.rs') or (path.endswith('.py') and Path(path).name.startswith('test')):
            objects.append((metadata.split()[2], path))
    if not objects:
        return {}
    raw = git(root, 'cat-file', '--batch', data=b''.join(oid + b'\n' for oid, _ in objects))
    result = {}
    cursor = 0
    for _, path in objects:
        end = raw.index(b'\n', cursor)
        header = raw[cursor:end].split()
        if len(header) != 3 or header[1] != b'blob':
            raise CensusError(f'cannot read source blob {path}')
        size = int(header[2])
        result[path] = raw[end + 1:end + 1 + size].decode()
        cursor = end + 1 + size + 1
    return result


def inventory_files(files: dict[str, str]) -> list[Test]:
    tests = []
    macros = defaultdict(list)
    masked = {}
    spans = defaultdict(list)
    for path, source in files.items():
        if path.endswith('.py'):
            try:
                tree = ast.parse(source, filename=path)
            except SyntaxError as error:
                raise CensusError(str(error)) from error
            for node in ast.walk(tree):
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith('test_'):
                    tests.append(Test(owner(path), node.name, path, node.lineno))
            continue
        text = masked[path] = mask(source)
        for match in re.finditer(r'\bmacro_rules!\s*(\w+)\s*\{', text):
            start = match.end() - 1
            end = close(text, start)
            spans[path].append((match.start(), end + 1))
            body = text[start + 1:end]
            variables = re.findall(r'#\[test\]\s*(?:#\[[^\]]+\]\s*)*fn\s+\$(\w+)', body)
            fixed = [function['name'] for function in FUNCTION.finditer(body) if TEST_ATTRIBUTE.search(function['attrs'])]
            if not variables and not fixed:
                continue
            first = re.search(r'[({\[]', body)
            if first is None:
                raise CensusError(f'{path}: missing macro matcher')
            pattern = body[first.start() + 1:close(body, first.start())]
            bindings = {}
            for index, argument in enumerate(arguments(pattern)):
                binding = re.fullmatch(r'\$(\w+)\s*:\s*ident', argument)
                if binding:
                    bindings[binding[1]] = index
            if any(variable not in bindings for variable in variables):
                raise CensusError(f'{path}: macro {match[1]} needs explicit test-name expansion')
            macros[(owner(path), match[1])].append((path, tuple(bindings[var] for var in variables) + tuple(fixed)))
        for match in FUNCTION.finditer(text):
            if TEST_ATTRIBUTE.search(match['attrs']) and not any(start <= match.start() < end for start, end in spans[path]):
                tests.append(Test(owner(path), match['name'], path, source.count('\n', 0, match.start()) + 1))
    for path, text in masked.items():
        for match in re.finditer(r'\b(\w+)!\s*([({\[])', text):
            definitions = macros.get((owner(path), match[1]), [])
            if not definitions or any(start <= match.start() < end for start, end in spans[path]):
                continue
            local = [definition for definition in definitions if definition[0] == path]
            definitions = local or definitions
            variants = {definition[1] for definition in definitions}
            if len(variants) != 1:
                raise CensusError(f'{path}: ambiguous test macro {match[1]}')
            start = match.end() - 1
            actual = arguments(text[start + 1:close(text, start)])
            for index in next(iter(variants)):
                if isinstance(index, str):
                    name = index
                elif index >= len(actual) or not re.fullmatch(r'[A-Za-z_]\w*', actual[index]):
                    raise CensusError(f'{path}: cannot resolve test name in {match[1]} invocation')
                else:
                    name = actual[index]
                tests.append(Test(owner(path), name, path, text.count('\n', 0, match.start()) + 1, match[1]))
    return tests


def inventory(root: Path, ref: str, scope: str | None = None) -> list[Test]:
    return inventory_files(source_blobs(root, ref, scope))



def citations(message: str) -> list[str]:
    # A vague issue reference alone is not a section citation.
    issue = r'#(?:209|21\d|22\d|23\d|24[0-7])'
    section = r'(?:(?:§\s*|section\s+)\d+(?:\.\d+)*|(?:Rule\s+)?(?:D\d+|E\d+|S-entity)\b|Rule\s+[A-Z]\d*)'
    result = re.findall(issue + r'\s*[:,-]?\s*' + section, message, re.I)
    result += re.findall(section + r'\s*\(' + issue + r'\)', message, re.I)
    return sorted(set(result))


def test_key(test: Test) -> tuple[str, str]:
    for (namespace, old), new in RENAMES.items():
        if (test.owner, test.name) == (namespace, new):
            return namespace, old
    return test.owner, test.name


def history_records(root: Path, start: str, namespaces: set[str]) -> list[dict]:
    """Read changed blob identities once, including each merge's first-parent diff."""
    scopes = [f'crates/{name}' if name.startswith('cadmpeg') else name for name in sorted(namespaces)]
    raw = git(root, 'log', '--topo-order', '--full-history', '--raw', '--no-renames', '--no-abbrev',
              '--diff-merges=first-parent', '--format=%x00%H%x00%P%x00%B%x00',
              f'{start}..HEAD', '--', *scopes)
    fields = raw.split(b'\0')
    if fields[0] or (len(fields) - 1) % 4:
        raise CensusError('invalid NUL-delimited history metadata')
    records = []
    for offset in range(1, len(fields), 4):
        commit, parents, message, changes = fields[offset:offset + 4]
        files = []
        for line in changes.splitlines():
            if not line:
                continue
            if not line.startswith(b':') or b'\t' not in line:
                raise CensusError('unexpected Git raw diff metadata')
            metadata, encoded_path = line.split(b'\t', 1)
            columns = metadata.split()
            path = encoded_path.decode()
            if path.startswith('"'):
                raise CensusError('quoted source paths need an explicit Git-path parser')
            if len(columns) != 5 or columns[4] not in (b'A', b'D', b'M', b'T'):
                raise CensusError('unexpected Git raw change kind')
            if path.endswith('.rs') or (path.endswith('.py') and Path(path).name.startswith('test')):
                files.append((path, columns[2].decode(), columns[3].decode()))
        if files:
            records.append({'commit': commit.decode(), 'parents': parents.decode().split(),
                            'message': message.decode().strip(), 'files': files})
    return records


class BlobReader:
    """One persistent Git process reads each distinct historical source blob."""

    def __init__(self, root: Path):
        self.errors = tempfile.TemporaryFile()
        self.process = subprocess.Popen(['git', 'cat-file', '--batch'], cwd=root,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=self.errors)
        self.reads = 0

    def read(self, oid: str) -> str:
        if not re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}', oid):
            raise CensusError('invalid historical blob identity')
        self.process.stdin.write(oid.encode() + b'\n')
        self.process.stdin.flush()
        header = self.process.stdout.readline().split()
        if len(header) != 3 or header[1] != b'blob':
            raise CensusError(f'cannot read historical source blob {oid}')
        size = int(header[2])
        source = self.process.stdout.read(size)
        if len(source) != size or self.process.stdout.read(1) != b'\n':
            raise CensusError('truncated historical source blob')
        self.reads += 1
        return source.decode()

    def close(self):
        self.process.stdin.close()
        self.process.stdout.close()
        result = self.process.wait()
        self.errors.seek(0)
        error = self.errors.read().decode(errors='replace')
        self.errors.close()
        if result:
            raise CensusError(error or 'historical blob reader failed')


def annotate_deletions(root: Path, start: str, missing: list[dict], locations: dict) -> dict:
    """Index actual per-commit count reductions; reconstruct owners only for macros."""
    started = time.monotonic()
    wanted = {(row['owner'], row['test']): row for row in missing}
    names = defaultdict(set)
    macros = defaultdict(set)
    for key in wanted:
        names[key[0]].add(key[1])
        alias = RENAMES.get(key)
        if alias:
            names[key[0]].add(alias)
        macros[key[0]].update(test.macro for test in locations[key] if test.macro)
    patterns = {namespace: re.compile(r'\b(?:' + '|'.join(map(re.escape, values)) + r')\b')
                for namespace, values in names.items()}
    macro_patterns = {namespace: re.compile(r'(?:\bmacro_rules!\s*(?:' + '|'.join(map(re.escape, values))
                        + r')\b|\b(?:' + '|'.join(map(re.escape, values)) + r')\s*!)')
                      for namespace, values in macros.items() if values}
    records = history_records(root, start, set(names)) if wanted else []
    parsed = {}
    snapshots = {}
    reader = BlobReader(root)

    def blob_counts(path: str, oid: str) -> tuple[Counter, bool]:
        if not oid.strip('0'):
            return Counter(), False
        namespace = owner(path)
        key = namespace, Path(path).suffix, oid
        if key not in parsed:
            source = reader.read(oid)
            macro = macro_patterns.get(namespace)
            ambiguous = bool(macro and macro.search(source))
            if ambiguous or not patterns[namespace].search(source):
                parsed[key] = Counter(), ambiguous
            else:
                parsed[key] = (Counter(test_key(test) for test in inventory_files({path: source})
                                       if test_key(test) in wanted), False)
        return parsed[key]

    def owner_counts(ref: str, namespace: str) -> Counter:
        key = ref, namespace
        if key not in snapshots:
            scope = f'crates/{namespace}' if namespace.startswith('cadmpeg') else namespace
            snapshots[key] = Counter(test_key(test) for test in inventory(root, ref, scope)
                                     if test_key(test) in wanted)
        return snapshots[key]

    try:
        for record in records:
            before, after = Counter(), Counter()
            ambiguous = set()
            for path, old, new in record['files']:
                namespace = owner(path)
                if namespace not in names:
                    continue
                old_counts, old_macro = blob_counts(path, old)
                new_counts, new_macro = blob_counts(path, new)
                before.update(old_counts)
                after.update(new_counts)
                if old_macro or new_macro:
                    ambiguous.add(namespace)
            if ambiguous:
                if not record['parents']:
                    raise CensusError('changed macro context lacks a parent')
                for namespace in ambiguous:
                    for key in wanted:
                        if key[0] == namespace:
                            before[key] = owner_counts(record['parents'][0], namespace)[key]
                            after[key] = owner_counts(record['commit'], namespace)[key]
            for key, amount in (before - after).items():
                row = wanted[key]
                covered = sum(event['removed_count'] for event in row['deletions'])
                if covered < row['missing_count']:
                    row['deletions'].append({'commit': record['commit'], 'removed_count': amount,
                         'section_citations': citations(record['message']),
                         'commit_message': record['message']})
            if all(sum(event['removed_count'] for event in row['deletions']) >= row['missing_count']
                   for row in missing):
                break
    finally:
        reader.close()
    for row in missing:
        covered = sum(event['removed_count'] for event in row['deletions'])
        row['unresolved_deletion_count'] = max(0, row['missing_count'] - covered)
        row['deletion'] = row['deletions'][0] if row['deletions'] else {
            'commit': None, 'section_citations': [], 'commit_message': '',
            'error': 'no actual test-count reduction found in reachable history'}
    return {'history_commands': int(bool(wanted)), 'historical_blob_reader_processes': 1,
            'historical_blobs_read': reader.reads, 'owner_snapshot_reconstructions': len(snapshots),
            'history_records': len(records), 'seconds': round(time.monotonic() - started, 3)}


def census(root: Path) -> dict:
    started = time.monotonic()
    base = git(root, 'merge-base', 'HEAD', 'origin/main').decode().strip()
    checkpoint = git(root, 'rev-parse', '--verify', CHECKPOINT + '^{commit}').decode().strip()
    head = git(root, 'rev-parse', 'HEAD').decode().strip()
    inventories = {ref: inventory(root, ref) for ref in dict.fromkeys([base, checkpoint, head])}
    current = Counter(test_key(test) for test in inventories[head])
    needed = Counter()
    locations = defaultdict(list)
    origins = defaultdict(set)
    for ref in [base, checkpoint]:
        needed |= Counter(test_key(test) for test in inventories[ref])
        for test in inventories[ref]:
            key = test_key(test)
            locations[key].append(test)
            origins[key].add(ref)
    missing = [{'owner': key[0], 'test': key[1], 'missing_count': expected - current[key],
                'baseline_refs': sorted(origins[key]),
                'source_locations': sorted({f'{test.path}:{test.line}' for test in locations[key]}),
                'source_names': sorted({test.name for test in locations[key]}),
                'allowed_rename': RENAMES.get(key), 'deletions': [], 'behavior_review_required': True}
               for key, expected in sorted(needed.items()) if current[key] < expected]
    common = git(root, 'merge-base', base, checkpoint).decode().strip()
    metrics = annotate_deletions(root, common, missing, locations)
    return {'base': base, 'checkpoint': checkpoint, 'head': head, 'history_start': common,
            'inventory_counts': {ref: len(tests) for ref, tests in inventories.items()},
            'missing_tests': missing,
            'uncited_deletions': sum(bool(row['unresolved_deletion_count']) or
                any(not event['section_citations'] for event in row['deletions']) for row in missing),
            'metrics': {**metrics, 'total_seconds': round(time.monotonic() - started, 3)},
            'semantic_review': 'A section citation does not prove the test asserted removed behavior. Review each listed original test against that section.'}

def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args(argv)
    try:
        result = census(ROOT)
    except (CensusError, OSError, UnicodeError) as error:
        print(f'test-deletion census failed: {error}', file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2))
    return int(bool(result['uncited_deletions']))


if __name__ == '__main__':
    sys.exit(main())
