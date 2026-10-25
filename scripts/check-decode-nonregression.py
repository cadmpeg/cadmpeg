#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""The §4.4 decode non-regression gate (#219).

Decodes every fixture with the base build (`CADMPEG_BASE_BIN`, from
`scripts/base-build.sh`) and with the head build, compares the two decodes and
the base and head decode goldens, and prints one JSON line per finding. The
comparison functions here are importable without a build.
"""

from __future__ import annotations

from collections import Counter
from dataclasses import dataclass, field
import math
import json
import os
from pathlib import Path, PurePosixPath
import tempfile
import subprocess
import sys
from typing import Any, Mapping

from decode_nonregression_inputs import base_binary, discover, pair_fixtures
from decode_nonregression_schema import SchemaPaths


JsonPath = tuple[str, ...]
FLOAT_TOLERANCE = 1.0e-12  # cadmpeg_ir::compare::FLOAT_TOLERANCE
MISSING = object()
D10_COMPANIONS = frozenset({'parameter', 'distance_parameter', 'angle_parameter', 'count_parameter'})
NATIVE_TAGS = frozenset({'native', 'native_set', 'Native', 'NativeSet'})


@dataclass(frozen=True)
class Finding:
    code: str
    path: JsonPath
    message: str


@dataclass(frozen=True)
class LayoutMove:
    """An Appendix B move-layout row, already resolved to its owning type."""

    namespace: str
    arena: str
    native_type: str
    field_path: JsonPath
    pointer_name: str
    store: str = 'pointers'


@dataclass(frozen=True)
class Evidence:
    """Typed adapter outputs; no command-line exemption list is accepted."""

    native_types: Mapping[tuple[str, str, str], str] = field(default_factory=dict)
    layout_moves: tuple[LayoutMove, ...] = ()
    fidelity: Mapping[str, Any] = field(default_factory=dict)


# Arenas §4.4 removes outright; their records need no preservation proof.
GONE_NATIVE_ARENAS = frozenset({
    ('f3d', 'design_intent_failures'),
    ('fcstd', 'physical_ledger'), ('fcstd', 'logical_ledger'), ('fcstd', 'byte_coverage'),
})
# The failure arenas of §4.5 rule 1; each record needs its Rule B preservation proof.
FAILURE_NATIVE_ARENAS = frozenset({
    ('fcstd', 'unreadable_entries'),
    ('iges', 'quarantined_directory_records'), ('iges', 'quarantined_parameter_records'),
    ('inventor', 'assembly_record_issues'), ('inventor', 'database_issues'),
    ('inventor', 'design_record_issues'), ('inventor', 'feature_record_issues'),
    ('inventor', 'presentation_record_issues'), ('inventor', 'property_set_issues'),
    ('inventor', 'protein_rejections'), ('inventor', 'segment_bulk_issues'),
    ('inventor', 'segment_meta_issues'), ('inventor', 'sketch_record_issues'),
    ('inventor', 'structural_issues'), ('creo', 'brep_face_admission_rejections'),
    ('nx', 'parasolid_deltas_residual_spans'),
})
REMOVED_NATIVE_ARENAS = GONE_NATIVE_ARENAS | FAILURE_NATIVE_ARENAS

# These are record-root paths in their named owning arenas, never a key-name
# blacklist. Deletion still requires rule-1 preservation evidence below.
FAILURE_FIELD_PATHS = {
    ('inventor', 'active_carrier'): frozenset({('detail',)}),
    ('inventor', 'protein'): frozenset({('detail',)}),
    ('inventor', 'ufrx'): frozenset({('detail',)}),
    ('inventor', 'segment_bulk'): frozenset({('record_detail',), ('record_state',)}),
    ('f3d', 'asm_history_records'): frozenset({('framing_error',)}),
    ('fcstd', 'properties'): frozenset({('payload_error',)}),
    ('rhino', 'setting_records'): frozenset({('parse_error',)}),
}


def values_agree(before: Any, after: Any) -> bool:
    """Structural native equality, allowing added object keys and float drift."""
    if type(before) is not type(after):
        return False
    if isinstance(before, float):
        return math.isfinite(before) and math.isfinite(after) and (
            before == after or abs(before - after) <= FLOAT_TOLERANCE * max(abs(before), abs(after), 1.0)
        )
    if isinstance(before, dict):
        return all(key in after and values_agree(value, after[key]) for key, value in before.items())
    if isinstance(before, list):
        return len(before) == len(after) and all(values_agree(a, b) for a, b in zip(before, after))
    return before == after


def at(value: Any, path: JsonPath) -> Any:
    for key in path:
        if not isinstance(value, dict) or key not in value:
            return MISSING
        value = value[key]
    return value


def prefix(path: JsonPath, ancestor: JsonPath) -> bool:
    return path[:len(ancestor)] == ancestor


def exemption(path: JsonPath, schema: SchemaPaths) -> str | None:
    """Name the resolver obligation at an exact declared neutral path."""
    if len(path) < 3 or path[1] != '#':
        return None
    arena, _, key, *tail = path
    if arena in {'bodies', 'faces'} and key == 'color':
        return 'effective_display_color'
    if key in {'properties', 'source_properties'}:
        return 'check-1b'
    if arena == 'parameters' and key in {'expression', 'dependencies', 'pmi'}:
        return 'parameter-resolver'
    if arena == 'configurations':
        return 'configuration-resolver'
    if arena == 'features' and key == 'dependencies':
        return 'feature-dependencies'
    if key == 'source_content':
        return 'source-content-projection'
    if key == 'source_object' and tail and tail[0] in {'name', 'color', 'visible', 'layer'}:
        return 'effective-source-object'
    if arena == 'occurrences' and key == 'scale':
        return 'effective_transform'
    if arena == 'spreadsheets' and key == 'cells':
        return 'cell-resolver'
    if arena == 'sketch_constraints' and key == 'driving':
        return 'dimension-driving'
    if any(prefix(path, companion) for companion in schema.d10_companions) or (
        arena.startswith(('sketch', 'spatial_sketch')) and path[-1] in D10_COMPANIONS
    ):
        return 'dimension-resolver'
    return None


def tag_exempt(path: JsonPath) -> bool:
    if len(path) < 3:
        return False
    arena, _, key = path[:3]
    return (
        (arena == 'parameters' and key in {'expression', 'pmi'})
        or (arena == 'configurations' and key in {'parameter_overrides', 'parameter_values', 'feature_states'})
        or (arena == 'features' and key == 'dependencies')
        or key == 'source_content'
        or (arena == 'spreadsheets' and key == 'cells')
    )


def record_facts(record: Any, path: JsonPath, schema: SchemaPaths) -> tuple[Counter, Counter]:
    """Count non-null leaves and variant tags for ONE record, collapsing indices."""
    leaves: Counter = Counter()
    tags: Counter = Counter()

    def walk(value: Any, here: JsonPath, unwrapped: bool = False) -> None:
        if value is None:
            return
        if here in schema.dimensions and isinstance(value, dict) and not unwrapped:
            if set(value) != {'kind', 'value'} or value['kind'] not in {'literal', 'parameter'}:
                raise ValueError(f'invalid Dimension wrapper at {here}')
            walk(value['value'], here, True)
        elif isinstance(value, dict):
            for key, child in value.items():
                walk(child, here + (key,))
        elif isinstance(value, list):
            for child in value:
                walk(child, here + ('#',))
        else:
            leaves[here] += 1
            if here in schema.variant_tags and isinstance(value, str) and value not in NATIVE_TAGS and not tag_exempt(here):
                tags[(here, value)] += 1

    walk(record, path)
    return leaves, tags


def index_records(rows: Any, path: JsonPath, findings: list[Finding]) -> dict[str, dict]:
    result = {}
    if not isinstance(rows, list):
        findings.append(Finding('arena-shape', path, 'arena must be an array'))
        return result
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get('id'), str):
            findings.append(Finding('record-identity', path, 'record must have a string id'))
            continue
        if row['id'] in result:
            findings.append(Finding('record-identity', path, f'duplicate id {row["id"]}'))
        result[row['id']] = row
    return result


def compare_neutral(before: Mapping, after: Mapping, schema: SchemaPaths, head_schema: SchemaPaths | None = None) -> list[Finding]:
    findings: list[Finding] = []
    for arena, rows in before.items():
        old = index_records(rows, ('model', arena), findings)
        new = index_records(after.get(arena, []), ('model', arena), findings)
        if len(new) < len(old):
            findings.append(Finding('neutral-count', ('model', arena), f'{len(old)} records became {len(new)}; no typed count-change proof supplied'))
        for identity, record in old.items():
            head = new.get(identity)
            if head is None:
                findings.append(Finding('neutral-record', ('model', arena, identity), 'base record has no head identity/pairing witness'))
                continue
            old_leaves, old_tags = record_facts(record, (arena, '#'), schema)
            new_leaves, new_tags = record_facts(head, (arena, '#'), schema if head_schema is None else head_schema)
            for (path, tag), count in old_tags.items():
                if new_tags[(path, tag)] < count:
                    findings.append(Finding('neutral-tag', (identity,) + path, f'typed variant {tag!r} lost'))
            for path, count in old_leaves.items():
                if new_leaves[path] >= count:
                    continue
                resolver = exemption(path, schema)
                code = 'resolver-evidence-required' if resolver else 'neutral-leaf'
                message = f'{count} non-null leaves became {new_leaves[path]}'
                if resolver:
                    message += f'; {resolver} equality proof is required, not an exemption'
                findings.append(Finding(code, (identity,) + path, message))
    return findings


def native_differences(before: Any, after: Any, path: JsonPath = ()) -> list[tuple[JsonPath, str]]:
    if isinstance(before, dict) and isinstance(after, dict):
        result = []
        for key, value in before.items():
            if key not in after:
                result.append((path + (key,), 'base key is absent'))
            else:
                result.extend(native_differences(value, after[key], path + (key,)))
        return result
    if isinstance(before, list) and isinstance(after, list) and len(before) == len(after):
        return [finding for i, (a, b) in enumerate(zip(before, after)) for finding in native_differences(a, b, path + (str(i),))]
    return [] if values_agree(before, after) else [(path, 'native structure or value changed')]


def compare_native(before: Mapping, after: Mapping, evidence: Evidence) -> list[Finding]:
    findings: list[Finding] = []
    for namespace, arenas in before.items():
        for arena, rows in arenas.items():
            root = ('native', namespace, arena)
            old = index_records(rows, root, findings)
            new = index_records(after.get(namespace, {}).get(arena, []), root, findings)
            removed_arena = (namespace, arena) in REMOVED_NATIVE_ARENAS
            if len(new) < len(old):
                code = 'rule-b-evidence-required' if removed_arena or arena == 'unknowns' else 'native-count'
                findings.append(Finding(code, root, 'native count fell; removed arenas/unknowns require actual part/read preservation proofs'))
            for identity, record in old.items():
                if identity not in new:
                    findings.append(Finding('native-identity', root + (identity,), 'base native id is absent or changed form'))
                    continue
                native_type = evidence.native_types.get((namespace, arena, identity))
                moves = [move for move in evidence.layout_moves if (move.namespace, move.arena, move.native_type) == (namespace, arena, native_type)]
                witnessed = set()
                for move in moves:
                    value = at(record, move.field_path)
                    if value is MISSING:
                        continue
                    stored = evidence.fidelity.get('record_layout', {}).get(identity, {}).get(move.store, {})
                    if move.pointer_name not in stored or not values_agree(value, stored[move.pointer_name]):
                        findings.append(Finding('layout-value-preservation', root + (identity,) + move.field_path, 'base moved value is absent or unequal in record_layout pointers'))
                    else:
                        witnessed.add(move.field_path)
                for path, message in native_differences(record, new[identity]):
                    if any(prefix(path, moved) for moved in witnessed):
                        continue
                    failure = path in FAILURE_FIELD_PATHS.get((namespace, arena), ())
                    code = 'rule-b-evidence-required' if failure else 'native-record'
                    findings.append(Finding(code, root + (identity,) + path, message))
    return findings


def compare_decodes(before: Mapping, after: Mapping, schema: SchemaPaths, evidence: Evidence | None = None) -> list[Finding]:
    if 'decode_error' in before:
        return []
    if 'decode_error' in after:
        return [Finding('head-decode-error', (), str(after['decode_error']))]
    base = before.get('ir', before)
    head = after.get('ir', after)
    proof = evidence if evidence is not None else Evidence()
    return compare_neutral(base.get('model', {}), head.get('model', {}), schema) + compare_native(base.get('native', {}), head.get('native', {}), proof)


def fixture_key(path: str) -> tuple[str, ...]:
    parts = tuple(part for part in PurePosixPath(path).parts if part != 'damaged')
    return parts[:-1] + (PurePosixPath(parts[-1]).stem,)


def compare_fixture_sets(before: Mapping[str, Any], after: Mapping[str, Any], schema: SchemaPaths) -> list[Finding]:
    def indexed(fixtures: Mapping[str, Any]) -> dict:
        result = {}
        for name, value in fixtures.items():
            key = fixture_key(name)
            if key in result:
                raise ValueError(f'ambiguous fixture stem {key}')
            result[key] = value
        return result
    old, new = indexed(before), indexed(after)
    findings = []
    for key, value in old.items():
        if key not in new:
            findings.append(Finding('fixture-missing', key, 'base fixture is absent; moving into damaged/ does not delete the base row'))
        else:
            findings.extend(compare_decodes(value, new[key], schema))
    return findings


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()


def committed_goldens(repo: Path, revision: str) -> dict[tuple[str, ...], str]:
    from decode_nonregression_run import golden_key, is_decode_golden
    listing = subprocess.run(['git', '-C', str(repo), 'ls-tree', '-rz', '--name-only', '--full-tree', revision],
                             check=True, stdout=subprocess.PIPE).stdout
    result = {}
    for raw in listing.split(b'\0'):
        path = raw.decode('utf-8')
        if path and is_decode_golden(path):
            key = golden_key(path)
            if key in result:
                raise ValueError(f'ambiguous golden stem: {path} and {result[key]}')
            result[key] = path
    return result


def committed_json(repo: Path, revision: str, path: str) -> Any:
    return json.loads(subprocess.check_output(['git', '-C', str(repo), 'show', f'{revision}:{path}']))


def build_base(repo: Path) -> Path:
    """The verified scripts/base-build.sh output that CADMPEG_BASE_BIN names."""
    configured = os.environ.get('CADMPEG_BASE_BIN')
    if not configured:
        raise ValueError('CADMPEG_BASE_BIN is unset; run `eval "$(scripts/base-build.sh)"` first')
    return base_binary(repo, configured)


def build_head(repo: Path) -> Path:
    subprocess.run(['cargo', 'build', '-q', '--release', '-p', 'cadmpeg', '--bin', 'cadmpeg'], cwd=repo, check=True)
    binary = repo / 'target' / 'release' / 'cadmpeg'
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError('head build wrote no executable')
    return binary


def run_gate(repo: Path) -> list[tuple[str, Finding]]:
    from decode_nonregression_run import (
        Build, appendix_b_moves, compare_pair, dump, effective_values, golden_parts, new_golden_findings,
        variant_guard_findings,
    )
    base = git(repo, 'merge-base', 'HEAD', 'origin/main')
    old_fixtures, new_fixtures = discover(repo, base), discover(repo, 'HEAD')
    pairs, failures = pair_fixtures(old_fixtures, new_fixtures)
    report: list[tuple[str, Finding]] = [
        (failure.split(': ', 1)[1], Finding(failure.split(':', 1)[0], (), failure)) for failure in failures
    ]
    base_build, head_build = Build('base', build_base(repo)), Build('head', build_head(repo))
    moves = appendix_b_moves()
    report.extend(('schema', finding) for finding in variant_guard_findings(sys.modules[__name__], base_build, head_build))
    paired = {new.key for _, new in pairs}
    with tempfile.TemporaryDirectory(prefix='cadmpeg-nonregression-') as directory:
        scratch = Path(directory)
        for index, (old, new) in enumerate(pairs):
            print(f'fixture {index + 1}/{len(pairs)}: {new.path}', file=sys.stderr, flush=True)
            source = repo / new.path
            base_doc, base_side = dump(base_build, source, scratch / 'base.json')
            head_doc, head_side = dump(head_build, source, scratch / 'head.json')
            report.extend((new.path, finding) for finding in compare_pair(
                sys.modules[__name__], base_doc, head_doc, base_side, head_side, base_build, head_build, moves,
                effective_values(head_build, head_doc, scratch)))
        for key, fixture in sorted(new_fixtures.items()):
            if key in paired or key in old_fixtures:
                continue
            head_doc, head_side = dump(head_build, repo / fixture.path, scratch / 'head.json')
            if 'decode_error' in head_doc:
                report.append((fixture.path, Finding('head-decode-error', (), head_doc['decode_error'])))
            report.extend((fixture.path, finding) for finding in new_golden_findings(
                sys.modules[__name__], fixture.path, head_side))
    old_goldens, new_goldens = committed_goldens(repo, base), committed_goldens(repo, 'HEAD')
    for index, (key, path) in enumerate(sorted(old_goldens.items())):
        print(f'golden {index + 1}/{len(old_goldens)}: {path}', file=sys.stderr, flush=True)
        if key not in new_goldens:
            report.append((path, Finding('golden-missing', (), 'base decode golden is absent at head')))
            continue
        base_doc, base_side = golden_parts(committed_json(repo, base, path))
        head_doc, head_side = golden_parts(committed_json(repo, 'HEAD', new_goldens[key]))
        with tempfile.TemporaryDirectory(prefix='cadmpeg-nonregression-') as directory:
            values = effective_values(head_build, head_doc, Path(directory))
        report.extend((new_goldens[key], finding) for finding in compare_pair(
            sys.modules[__name__], base_doc, head_doc, base_side, head_side, base_build, head_build, moves, values))
    for key, path in sorted(new_goldens.items()):
        if key not in old_goldens:
            _, head_side = golden_parts(committed_json(repo, 'HEAD', path))
            report.extend((path, finding) for finding in new_golden_findings(sys.modules[__name__], path, head_side))
    return report


def main() -> int:
    if len(sys.argv) != 1:
        print('check-decode-nonregression.py takes no arguments', file=sys.stderr)
        return 2
    repo = Path(__file__).resolve().parent.parent
    try:
        report = run_gate(repo)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f'decode non-regression gate failed to run: {error}', file=sys.stderr)
        return 1
    for source, finding in report:
        print(json.dumps({'input': source, 'code': finding.code, 'path': list(finding.path),
                          'message': finding.message}, sort_keys=True))
    codes = Counter(finding.code for _, finding in report)
    for code, count in sorted(codes.items()):
        print(f'{code}: {count}', file=sys.stderr)
    print(f'{len(report)} finding(s) over {len({source for source, _ in report})} input(s)', file=sys.stderr)
    return 1 if report else 0


if __name__ == '__main__':
    sys.exit(main())
