# SPDX-License-Identifier: Apache-2.0
"""Count uncertain neutral records using authenticated generated tag paths.

This bounded guard covers the actual Unknown/Unresolved/Other wire tag spellings
below. Native schema identities and differently renamed Rust variants still need
an owning typed adapter; neither arbitrary text nor a field called kind is proof.
"""
from dataclasses import dataclass

from decode_nonregression_schema_bindings import generated_arena
from decode_nonregression_schema import GeneratedSchemaError


UNCERTAIN_TAGS = frozenset({'unknown', 'unresolved', 'other', 'Unknown', 'Unresolved', 'Other'})


@dataclass(frozen=True)
class VariantGuardFinding:
    code: str
    path: tuple[str, ...]
    message: str


def uncertain_records(rows, query, wire_admissions=None):
    """Count a record once even when several nested variant nodes are uncertain."""
    if not isinstance(rows, list):
        raise GeneratedSchemaError('variant guard needs an actual record array')
    schema = generated_arena(query)
    arena = schema.arena.removeprefix('model.')
    result, identities = set(), set()
    for record in rows:
        if not isinstance(record, dict) or not isinstance(record.get('id'), str) or not record['id']:
            raise GeneratedSchemaError('variant guard needs actual record identities')
        identity = record['id']
        if identity in identities:
            raise GeneratedSchemaError('variant guard record identity is ambiguous')
        identities.add(identity)
        admission = (wire_admissions or {}).get((arena, identity))
        paths = schema.paths([record]) if admission is None else admission.paths

        def walk(value, path):
            if path in paths.variant_tags and isinstance(value, str) and value in UNCERTAIN_TAGS:
                return True
            if isinstance(value, dict):
                return any(walk(child, path + (key,)) for key, child in value.items())
            if isinstance(value, list):
                return any(walk(child, path + ('#',)) for child in value)
            return False

        if walk(record, (arena, '#')):
            result.add(identity)
    return result


def compare_uncertain_records(arena, old_rows, new_rows, old_query, new_query):
    """Fail an increase, including records in an arena absent from the baseline."""
    if new_query['payload']['arena'] != 'model.' + arena:
        raise GeneratedSchemaError('variant guard head schema belongs to another arena')
    if old_query is None:
        if old_rows:
            raise GeneratedSchemaError('variant guard baseline schema is missing')
        before = set()
    else:
        if old_query['payload']['arena'] != 'model.' + arena:
            raise GeneratedSchemaError('variant guard baseline schema belongs to another arena')
        before = uncertain_records(old_rows, old_query)
    after = uncertain_records(new_rows, new_query)
    return compare_uncertain_totals({(arena, identity) for identity in before},
                                    {(arena, identity) for identity in after})


def compare_uncertain_totals(before, after):
    if len(after) > len(before):
        return [VariantGuardFinding('uncertain-variant-increase', ('model',),
            f'{len(before)} neutral records containing a typed Unknown/Unresolved/Other wire variant became {len(after)}')]
    return []
