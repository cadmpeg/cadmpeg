# SPDX-License-Identifier: Apache-2.0
"""Resolver equality for the §4.4 exempt neutral paths, and check 1b (#219).

A leaf count that falls at an exempt path is a finding until the head
resolver gives the base value. Head resolver values come from the head build's
`cadmpeg query effective` view.
"""

from __future__ import annotations

from dataclasses import dataclass
import math
from pathlib import Path
import re
from typing import Any, Iterable, Mapping


JsonPath = tuple[str, ...]
FLOAT_TOLERANCE = 1.0e-12  # cadmpeg_ir::compare::FLOAT_TOLERANCE
REMOVED_NAMES = Path(__file__).resolve().parent.parent / 'crates/cadmpeg-test-support/src/removed_names.rs'
CODEC_ISSUES = {
    '220': 'sat', '221': 'step', '222': 'iges', '223': 'rhino', '224': 'fcstd', '225': 'f3d',
    '226': 'sldprt', '227': 'inventor', '228': 'catia', '229': 'creo', '230': 'nx',
}
NAME_ATTRIBUTE = 'name'

# The neutral arena owned by each id-bearing AttributeTarget tuple variant.
ATTRIBUTE_TARGET_ARENAS = {
    'body': 'bodies', 'face': 'faces', 'shell': 'shells', 'loop': 'loops',
    'coedge': 'coedges', 'edge': 'edges', 'vertex': 'vertices', 'region': 'regions',
    'surface': 'surfaces', 'curve': 'curves', 'point': 'points', 'feature': 'features',
    'sketch': 'sketches', 'parameter': 'parameters', 'product_definition': 'product_definitions',
    'occurrence': 'occurrences', 'tessellation': 'tessellations', 'pmi': 'pmi',
    'semantic_annotation': 'semantic_annotations', 'drawing': 'drawings',
    'view_presentation': 'view_presentations',
}


@dataclass(frozen=True)
class Finding:
    code: str
    path: JsonPath
    message: str


def section_8_deletions(path: Path = REMOVED_NAMES) -> dict[str, tuple[re.Pattern, ...]]:
    """Per-codec key patterns that a codec issue deletes under §8; `.N` is an index."""
    result: dict[str, list[re.Pattern]] = {}
    issues: list[str] = []
    for line in path.read_text().splitlines():
        if line.strip().startswith('///'):
            issues = [number for number in re.findall(r'#(\d+) §8', line) if number in CODEC_ISSUES]
            continue
        constant = re.match(r'\s*pub const \w+: &str = "(.*)";', line)
        if constant:
            pattern = re.escape(constant.group(1)).replace(r'\.N', r'\.\d+')
            for number in issues:
                result.setdefault(CODEC_ISSUES[number], []).append(re.compile(pattern + r'\Z'))
            issues = []
    return {codec: tuple(patterns) for codec, patterns in result.items()}


def agree(before: Any, after: Any) -> bool:
    """Value equality with the comparator's float tolerance and int/float identity."""
    if isinstance(before, bool) or isinstance(after, bool):
        return before is after
    if isinstance(before, (int, float)) and isinstance(after, (int, float)):
        if isinstance(before, int) and isinstance(after, int):
            return before == after
        return math.isfinite(before) and math.isfinite(after) and (
            before == after or abs(before - after) <= FLOAT_TOLERANCE * max(abs(before), abs(after), 1.0))
    if isinstance(before, dict) and isinstance(after, dict):
        return before.keys() == after.keys() and all(agree(value, after[key]) for key, value in before.items())
    if isinstance(before, list) and isinstance(after, list):
        return len(before) == len(after) and all(agree(a, b) for a, b in zip(before, after))
    return before == after


def records(document: Mapping, arena: str) -> dict[str, dict]:
    return {row['id']: row for row in document.get('model', {}).get(arena, [])
            if isinstance(row, dict) and isinstance(row.get('id'), str)}


def text(value: Any) -> str:
    if isinstance(value, bool):
        return 'true' if value else 'false'
    return str(value)


def snake(name: str) -> str:
    return re.sub(r'(?<=[a-z0-9])(?=[A-Z])', '_', name).replace('-', '_').replace(' ', '_').lower()


def same_text(value: Any, expected: str) -> bool:
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        try:
            return agree(float(value), float(expected))
        except ValueError:
            return False
    return isinstance(value, (str, bool)) and text(value) == expected


def attribute_owner(target: Any) -> tuple[str, str] | None:
    """Read the complete neutral owner from AttributeTarget's adjacent wire form."""
    if not isinstance(target, dict) or set(target) != {'kind', 'id'}:
        return None
    kind, identity = target['kind'], target['id']
    if kind == 'record':
        if not isinstance(identity, dict) or set(identity) != {'arena', 'id'}:
            return None
        arena, identity = identity['arena'], identity['id']
    else:
        arena = ATTRIBUTE_TARGET_ARENAS.get(kind) if isinstance(kind, str) else None
    if not isinstance(arena, str) or not isinstance(identity, str):
        return None
    return arena, identity


class Resolvers:
    """Head resolver values and their comparison with base field values."""

    def __init__(self, base: Mapping, head: Mapping, effective: Mapping):
        self.base, self.head = base, head
        self.effective = effective
        self.values = {(row['arena'], row['id']): row for row in effective.get('records', [])}
        self.head_parameters = records(head, 'parameters')
        self.base_parameters = records(base, 'parameters')

    def record_value(self, arena: str, identity: str, key: str) -> Any:
        return self.values.get((arena, identity), {}).get(key)

    def source_name(self, arena: str, identity: str) -> Any:
        record = records(self.head, arena).get(identity)
        if record is None:
            return None
        if isinstance(record.get('name'), str):
            return record['name']
        names = [value.get('value') for attribute in self.head.get('model', {}).get('attributes', [])
                 if attribute.get('name') == NAME_ATTRIBUTE
                 and attribute_owner(attribute.get('target')) == (arena, identity)
                 for value in attribute.get('values', []) if value.get('kind') == 'string']
        return names[0] if len(names) == 1 else None

    def source_object(self, arena: str, identity: str, key: str, value: Any) -> bool:
        if key == 'color':
            return agree(value, self.record_value(arena, identity, 'display_color'))
        if key == 'visible':
            return value is self.record_value(arena, identity, 'visible')
        if key == 'layer':
            return value in (self.record_value(arena, identity, 'layers') or [])
        if key == 'name':
            return value == self.source_name(arena, identity)
        return False

    def transform(self, identity: str, record: Mapping) -> bool:
        rows, scale = record.get('transform'), record.get('scale')
        if not (isinstance(rows, list) and isinstance(scale, list) and len(scale) == 3):
            return False
        scaled = [[row[column] * (scale[column] if column < 3 else 1.0) for column in range(4)] for row in rows]
        linked = record.get('linked_prototype')
        if isinstance(linked, list):
            scaled = compose(scaled, linked)
        return agree(scaled, self.record_value('occurrences', identity, 'transform'))

    def expression(self, identity: str, record: Mapping) -> bool:
        return record.get('expression') == self.effective.get('expressions', {}).get(identity, MISSING)

    def dependencies(self, identity: str, record: Mapping) -> bool:
        head = self.head_parameters.get(identity, {})
        tokens = {token.get('value') for token in head.get('expression', {}).get('tokens', [])
                  if token.get('kind') == 'parameter'}
        return set(record.get('dependencies') or []) == tokens

    def pmi(self, identity: str) -> bool:
        reference = self.head_parameters.get(identity, {}).get('pmi')
        return isinstance(reference, str) and reference in records(self.head, 'pmi')

    def configuration(self, identity: str, record: Mapping, key: str, member: str) -> bool:
        resolved = self.effective.get('configurations', {}).get(identity)
        if resolved is None:
            return False
        value = record.get(key, {}).get(member, MISSING)
        if key == 'parameter_overrides':
            return value == resolved['parameters'].get(member, {}).get('expression', MISSING)
        if key == 'parameter_values':
            return agree(value, resolved['parameters'].get(member, {}).get('value'))
        if key == 'feature_states':
            suppressed = isinstance(value, dict) and value.get('evaluation', {}).get('kind') == 'suppressed'
            return resolved['suppressed'].get(member) is suppressed
        return False

    def cells(self, identity: str, record: Mapping) -> bool:
        head = records(self.head, 'spreadsheets').get(identity)
        if head is None:
            return False
        by_address = {json_key(cell.get('address')): cell for cell in head.get('cells', [])}
        for cell in record.get('cells', []):
            target = by_address.get(json_key(cell.get('address')))
            if target is None:
                return False
            old = self.base_parameters.get(cell.get('parameter'), {})
            new = self.head_parameters.get(target.get('parameter'), {})
            rendered = self.effective.get('expressions', {}).get(target.get('parameter'), MISSING)
            if old.get('expression') != rendered or not agree(old.get('value'), new.get('value')):
                return False
        return True

    def companion(self, arena: str, identity: str, path: JsonPath, base_record: Mapping) -> bool:
        """A D10 companion: its sibling Dimension binds it and resolves to the base sibling value."""
        concrete = path[2:]
        parent = at(base_record, concrete[:-1])
        bound = at(base_record, concrete)
        head_parent = at(records(self.head, arena).get(identity, {}), concrete[:-1])
        resolved = at(self.effective.get('definitions', {}).get(arena, {}).get(identity), concrete[1:-1])
        if not (isinstance(parent, dict) and isinstance(head_parent, dict) and isinstance(resolved, dict)):
            return False
        siblings = [key for key, value in head_parent.items()
                    if isinstance(value, dict) and value == {'kind': 'parameter', 'value': bound}]
        return len(siblings) == 1 and agree(parent.get(siblings[0]), resolved.get(siblings[0]))

    def driving(self, identity: str, record: Mapping) -> bool:
        head = records(self.head, 'sketch_constraints').get(identity, {})
        if isinstance(head.get('driving'), bool):
            return head['driving'] is record.get('driving')
        bound = set(parameter_references(head.get('definition')))
        states = {self.head_parameters[parameter].get('driving') for parameter in bound
                  if parameter in self.head_parameters}
        return states == {record.get('driving')}


MISSING = object()


def json_key(value: Any) -> str:
    return repr(value)


def at(value: Any, path: Iterable[str]) -> Any:
    for key in path:
        if isinstance(value, dict):
            value = value.get(key, MISSING)
        elif isinstance(value, list) and key.isdigit() and int(key) < len(value):
            value = value[int(key)]
        else:
            return MISSING
    return value


def parameter_references(value: Any) -> Iterable[str]:
    if isinstance(value, dict):
        if value.get('kind') == 'parameter' and isinstance(value.get('value'), str) and set(value) == {'kind', 'value'}:
            yield value['value']
        for child in value.values():
            yield from parameter_references(child)
    elif isinstance(value, list):
        for child in value:
            yield from parameter_references(child)


def compose(left: list, right: list) -> list:
    """Row-major 3x4 affine product `left * right` for column vectors."""
    result = []
    for row in range(3):
        values = [sum(left[row][k] * right[k][column] for k in range(3)) for column in range(4)]
        values[3] += left[row][3]
        result.append(values)
    return result


NO_RESOLVER = frozenset({'check-1b', 'feature-dependencies', 'source-content-projection'})


def resolve(resolvers: Resolvers | None, finding_path: JsonPath, resolver: str) -> bool:
    """Whether the head resolver gives the base value at one finding path."""
    if resolvers is None:
        return False
    identity, path = finding_path[0], finding_path[1:]
    arena = path[0]
    record = records(resolvers.base, arena).get(identity)
    if record is None or len(path) < 3:
        return False
    key = path[2]
    if resolver == 'effective_display_color':
        return agree(record.get('color'), resolvers.record_value(arena, identity, 'display_color'))
    if resolver == 'effective-source-object':
        field = path[3]
        return resolvers.source_object(arena, identity, field, record.get('source_object', {}).get(field))
    if resolver == 'effective_transform':
        return resolvers.transform(identity, record)
    if resolver == 'parameter-resolver':
        if key == 'expression':
            return resolvers.expression(identity, record)
        if key == 'dependencies':
            return resolvers.dependencies(identity, record)
        return resolvers.pmi(identity)
    if resolver == 'configuration-resolver':
        return len(path) > 3 and resolvers.configuration(identity, record, key, path[3])
    if resolver == 'cell-resolver':
        return resolvers.cells(identity, record)
    if resolver == 'dimension-resolver':
        return resolvers.companion(arena, identity, concrete_path(record, path), record)
    if resolver == 'dimension-driving':
        return resolvers.driving(identity, record)
    return False


def concrete_path(record: Mapping, collapsed: JsonPath) -> JsonPath:
    """The single concrete path of a collapsed record path, or the collapsed path itself."""
    concrete = list(collapsed[:2])
    value: Any = record
    for key in collapsed[2:]:
        if key == '#':
            if not isinstance(value, list) or len(value) != 1:
                return collapsed
            concrete.append('0')
            value = value[0]
        else:
            concrete.append(key)
            value = value.get(key) if isinstance(value, dict) else None
    return tuple(concrete)


def check_1b(base: Mapping, head: Mapping, deletions: Mapping[str, tuple[re.Pattern, ...]]) -> list[Finding]:
    """Every base (key, value) of a neutral `properties`/`source_properties` map survives."""
    native = {}
    owned: dict[str, list[dict]] = {}
    for arenas in head.get('native', {}).values():
        if not isinstance(arenas, dict):
            continue
        for rows in arenas.values():
            for row in rows if isinstance(rows, list) else ():
                if isinstance(row, dict) and isinstance(row.get('id'), str):
                    native[row['id']] = row
                    if isinstance(row.get('owner'), str):
                        owned.setdefault(row['owner'], []).append(row)
    findings = []
    for arena, rows in base.get('model', {}).items():
        new = records(head, arena)
        for record in rows:
            identity = record.get('id')
            target = new.get(identity)
            codec = identity.split(':', 1)[0] if isinstance(identity, str) else ''
            for map_key in ('properties', 'source_properties'):
                entries = record.get(map_key)
                if not isinstance(entries, dict):
                    continue
                for key, value in entries.items():
                    if any(pattern.match(key) for pattern in deletions.get(codec, ())):
                        continue
                    if target is not None and kept(key, value, target, native, owned):
                        continue
                    findings.append(Finding('property-lost', (identity, arena, '#', map_key, key),
                                            f'base {map_key} entry {key!r}={value!r} has no head home'))
    return findings


def kept(key: str, value: Any, target: Mapping, native: Mapping, owned: Mapping) -> bool:
    expected = text(value)
    for map_key in ('properties', 'source_properties'):
        if isinstance(target.get(map_key), dict) and same_text(target[map_key].get(key), expected):
            return True
    holders = [target]
    if isinstance(target.get('native_ref'), str) and target['native_ref'] in native:
        holders.append(native[target['native_ref']])
    for holder in holders:
        for field, field_value in holder.items():
            if snake(field) == snake(key) and same_text(field_value, expected):
                return True
            if isinstance(field_value, dict) and same_text(field_value.get(key), expected):
                return True
    for row in owned.get(target.get('id'), []) + owned.get(target.get('native_ref'), []):
        if row.get('name') == key and any(same_text(row.get(field), expected) for field in ('value', 'text')):
            return True
    return False
