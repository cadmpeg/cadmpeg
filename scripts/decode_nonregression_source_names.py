# SPDX-License-Identifier: Apache-2.0
"""Record-specific equality for the existing styled STEP carrier-name home.

STEP reader::geometry::associate_presentation_carrier reads the referenced
geometric item's name; attach_geometry_source preserves styled-item ownership;
retain_geometry_name projects that fact to a SourceAttribute String targeted to
the same Curve. This is that concrete decoder rule, not a generic name-key
exemption. Native record-retention/P-closure is a separate, still-open proof.
"""
from dataclasses import dataclass
from typing import Mapping

from decode_nonregression_schema_bindings import generated_arena


class SourceNameError(ValueError):
    """An actual effective name or owner/referent witness is unavailable."""


@dataclass(frozen=True)
class SourceNameEquality:
    record_id: str
    value_path: tuple[str, ...]
    value: str
    attribute_id: str
    binding_id: str
    appearance_id: str


def _identity(nominal: str) -> dict:
    return {'schema_id': nominal, 'rust_type': nominal, 'schema_name': nominal.rsplit('::', 1)[-1]}


def _index(records: list, label: str) -> dict:
    if not isinstance(records, list):
        raise SourceNameError(label + ' is not a typed record array')
    result = {}
    for record in records:
        if not isinstance(record, dict) or not isinstance(record.get('id'), str) or not record['id']:
            raise SourceNameError(label + ' has no genuine record identity')
        if record['id'] in result:
            raise SourceNameError(label + ' has duplicate identities')
        result[record['id']] = record
    return result


def verify_source_names(report: Mapping, original: Mapping, current: Mapping,
                        base_queries: Mapping, head_queries: Mapping) -> tuple[SourceNameEquality, ...]:
    """Require equal value AND identical existing carrier/owner/referent binding."""
    if set(report) != {'source_name_protocol', 'build', 'ir_version', 'requested', 'rows'}:
        raise SourceNameError('malformed baseline source-name report')
    if report['source_name_protocol'] != 'baseline-styled-step-curve-name-v1' or report['build'] != 'baseline':
        raise SourceNameError('unsupported source-name proof protocol')
    if report['ir_version'] != original.get('ir_version'):
        raise SourceNameError('baseline source-name proof has another IR version')
    requested, rows = report['requested'], report['rows']
    if (not isinstance(requested, list) or not requested or any(not isinstance(x, str) or not x for x in requested)
            or len(set(requested)) != len(requested) or not isinstance(rows, list)
            or [row.get('record_id') for row in rows if isinstance(row, dict)] != requested):
        raise SourceNameError('baseline source-name proof missed actual unique requests')
    expected_types = {'curves': 'Curve', 'appearance_bindings': 'AppearanceBinding',
                      'appearances': 'Appearance', 'attributes': 'SourceAttribute'}
    for queries in [base_queries, head_queries]:
        for arena, query in queries.items():
            if arena not in expected_types or query['payload']['arena'] != 'model.' + arena or query['payload']['element'] != expected_types[arena]:
                raise SourceNameError('name proof query has another actual typed owner')
    for arena in ['curves', 'appearance_bindings', 'appearances']:
        if arena not in base_queries or arena not in head_queries:
            raise SourceNameError('typed source-name owner/referent query is missing')
    if 'attributes' not in head_queries:
        raise SourceNameError('canonical attribute query is missing')
    old = {arena: _index(original['model'].get(arena, []), 'baseline ' + arena)
           for arena in ['curves', 'appearance_bindings', 'appearances']}
    new = {arena: _index(current['model'].get(arena, []), 'head ' + arena)
           for arena in ['curves', 'appearance_bindings', 'appearances', 'attributes']}
    results = []
    for row in rows:
        if set(row) != {'record_id', 'arena', 'owner', 'association_owner', 'binding_owner', 'value_path',
                        'value', 'source_object', 'binding', 'appearance'}:
            raise SourceNameError('malformed actual typed source-name witness')
        identity = row['record_id']
        if (row['arena'] != 'curves' or row['value_path'] != ['source_object', 'name']
                or row['owner'] != _identity('cadmpeg_ir::geometry::Curve')
                or row['association_owner'] != _identity('cadmpeg_ir::provenance::SourceObjectAssociation')
                or row['binding_owner'] != _identity('cadmpeg_ir::appearance::AppearanceBinding')):
            raise SourceNameError('source-name field or typed owner is unsupported')
        before, after = old['curves'].get(identity), new['curves'].get(identity)
        if before is None or after is None:
            raise SourceNameError('actual curve referent identity was lost')
        generated_arena(base_queries['curves']).paths([before])
        generated_arena(head_queries['curves']).paths([after])
        association = before.get('source_object')
        if association != row['source_object'] or not isinstance(association, dict) or association.get('format') != 'step':
            raise SourceNameError('baseline typed source association does not match the original')
        if not isinstance(row['value'], str) or association.get('name') != row['value']:
            raise SourceNameError('baseline typed name differs from the actual source field')
        head_association = after.get('source_object')
        if not isinstance(head_association, dict):
            raise SourceNameError('source-object owner identity was lost')
        for field, default in [('format', None), ('object_id', None), ('geometry_role', None), ('instance_path', [])]:
            if association.get(field, default) != head_association.get(field, default):
                raise SourceNameError('source-object owner/instance binding changed: ' + field)
        binding, appearance = row['binding'], row['appearance']
        if not isinstance(binding, dict) or not isinstance(appearance, dict):
            raise SourceNameError('baseline typed binding/referent is malformed')
        original_appearance = old['appearances'].get(appearance.get('id'))
        if old['appearance_bindings'].get(binding.get('id')) != binding or original_appearance is None:
            raise SourceNameError('actual baseline binding or appearance identity differs from its typed witness')
        # Appearance is a typed referent witness for this NAME resolver. Its
        # identity and admission are required; its colour is a separate resolver
        # obligation. In particular, serde_json::to_value widens Rust f32 colour
        # values while the actual JSON writer emits their shortest f32 spelling.
        # Never turn that representational difference into name loss, nor waive
        # any colour obligation: only source_object.name is discharged below.
        generated_arena(base_queries['appearances']).paths([original_appearance])
        target = {'kind': 'curve', 'id': identity}
        witnesses = [value for value in old['appearance_bindings'].values()
                     if value.get('target') == target and value.get('source_entity_id') == association.get('object_id')]
        if len(witnesses) != 1 or witnesses[0] != binding or binding.get('appearance') != appearance['id']:
            raise SourceNameError('baseline owner/target/referent binding is missing or ambiguous')
        current_binding = new['appearance_bindings'].get(binding['id'])
        if current_binding is None or any(current_binding.get(key) != binding.get(key)
                                          for key in ['target', 'source_entity_id', 'appearance']):
            raise SourceNameError('actual styled-item owner/target/appearance binding changed')
        current_witnesses = [value for value in new['appearance_bindings'].values()
                             if value.get('target') == target and value.get('source_entity_id') == association.get('object_id')]
        if len(current_witnesses) != 1 or current_witnesses[0]['id'] != binding['id']:
            raise SourceNameError('head owner/target binding is ambiguous')
        current_appearance = new['appearances'].get(appearance['id'])
        if current_appearance is None:
            raise SourceNameError('actual appearance referent is absent')
        for arena, baseline_record, head_record in [('appearance_bindings', binding, current_binding),
                                                    ('appearances', appearance, current_appearance)]:
            generated_arena(base_queries[arena]).paths([baseline_record])
            generated_arena(head_queries[arena]).paths([head_record])
        # This exact home is emitted by STEP retain_geometry_name, not inferred
        # from arbitrary labels/metadata or an ID's textual spelling.
        attributes = [value for value in new['attributes'].values()
                      if value.get('target') == target and value.get('name') == 'name']
        if len(attributes) != 1:
            raise SourceNameError('canonical geometry-name attribute is absent or ambiguous')
        attribute = attributes[0]
        generated_arena(head_queries['attributes']).paths([attribute])
        if attribute.get('values') != [{'kind': 'string', 'value': row['value']}]:
            raise SourceNameError('canonical geometry-name String value changed')
        results.append(SourceNameEquality(identity, ('source_object', 'name'), row['value'],
                                          attribute['id'], binding['id'], appearance['id']))
    return tuple(results)
