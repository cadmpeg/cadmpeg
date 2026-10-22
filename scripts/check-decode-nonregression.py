#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Compare decoded JSON facts for #219 using guarded local producer collection.

The importable comparator never infers a field move from its spelling. Its
schema paths, record types and move rows must come from the owning adapters.
The executable collects when real producer proofs are installed; missing
resolver, Appendix B and coverage obligations remain explicit failures.
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


@dataclass(frozen=True)
class Evidence:
    """Typed adapter outputs; no command-line exemption list is accepted."""

    native_types: Mapping[tuple[str, str, str], str] = field(default_factory=dict)
    layout_moves: tuple[LayoutMove, ...] = ()
    fidelity: Mapping[str, Any] = field(default_factory=dict)


REMOVED_NATIVE_ARENAS = frozenset({
    ('f3d', 'design_intent_failures'),
    ('fcstd', 'physical_ledger'), ('fcstd', 'logical_ledger'), ('fcstd', 'byte_coverage'),
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
    if any(prefix(path, companion) for companion in schema.d10_companions):
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

    def walk(value: Any, here: JsonPath) -> None:
        if value is None:
            return
        if here in schema.dimensions and isinstance(value, dict):
            if set(value) != {'kind', 'value'} or value['kind'] not in {'literal', 'parameter'}:
                raise ValueError(f'invalid Dimension wrapper at {here}')
            walk(value['value'], here)
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
                    pointers = evidence.fidelity.get('record_layout', {}).get(identity, {}).get('pointers', {})
                    if move.pointer_name not in pointers or not values_agree(value, pointers[move.pointer_name]):
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


def main() -> int:
    if len(sys.argv) != 1:
        print('check-decode-nonregression.py takes no arguments or exemption switches', file=sys.stderr)
        return 2
    repo = Path(__file__).resolve().parent.parent
    try:
        base = subprocess.check_output(
            ['git', '-C', str(repo), 'merge-base', 'HEAD', 'origin/main'], text=True,
        ).strip()
        _, fixture_failures = pair_fixtures(discover(repo, base), discover(repo, 'HEAD'))
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        print(f'fixture-discovery: {error}', file=sys.stderr)
        return 2
    if fixture_failures:
        print('\n'.join(fixture_failures), file=sys.stderr)
        return 1
    if all(os.environ.get(name) for name in (
        'CADMPEG_BASE_PRODUCER', 'CADMPEG_HEAD_PRODUCER', 'CADMPEG_BASE_SCHEMA_HELPER',
    )):
        from decode_nonregression_collect import BaselineHelper, Producer, collect_fixture
        try:
            verified_base = base_binary(repo, os.environ.get('CADMPEG_BASE_BIN'))
            baseline = Producer.load(repo, Path(os.environ['CADMPEG_BASE_PRODUCER']), 'baseline')
            if verified_base.resolve() != Path(baseline.proof['binary']).resolve():
                raise ValueError('baseline producer is not the verified base-build executable')
            current = Producer.load(repo, Path(os.environ['CADMPEG_HEAD_PRODUCER']), 'head')
            if current.proof['commit'] != subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'HEAD'], text=True).strip():
                raise ValueError('head producer does not match the committed gate input')
            if baseline.proof['commit'] != base:
                raise ValueError('baseline producer is not the actual merge base')
            helper = BaselineHelper.load(baseline, Path(os.environ['CADMPEG_BASE_SCHEMA_HELPER']))
            pairs, failures = pair_fixtures(discover(repo, base), discover(repo, current.proof['commit']))
            if failures:
                raise ValueError('; '.join(failures))
            with tempfile.TemporaryDirectory(prefix='cadmpeg-nonregression-') as directory:
                for index, pair in enumerate(pairs):
                    report = collect_fixture(sys.modules[__name__], pair[1].path, baseline, current, helper,
                                             Path(directory) / str(index))
                    print(json.dumps(report, sort_keys=True))
            # A full run cannot pass while the report has uncovered obligations.
            return 1
        except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
            print(f'collection-proof: {error}', file=sys.stderr)
            return 1
    print(
        'decode non-regression gate is NOT operational: authoritative base/head CLI dumps, '
        'CADMPEG_BASE_BIN verification, schema/typed-record and Appendix B adapters, resolver equality, '
        'Rule B payload/part/read proofs and §3.4 guards are not installed. '
        'The independently tested JSON comparator is not a passing corpus gate.',
        file=sys.stderr,
    )
    return 2


if __name__ == '__main__':
    sys.exit(main())
