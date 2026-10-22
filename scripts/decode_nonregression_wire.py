# SPDX-License-Identifier: Apache-2.0
"""Consume actual baseline Rust admission for historical procedural wire rows.

The guarded helper, compiled against pristine baseline IR, admits the whole
CadIr through its strict private row deserializers, resolves the unique carrier
through typed geometry references, and proves exact row roundtrip. Its direct
public typed payload still passes every unchanged generated schema constraint.
No corrected baseline schema or field-spelling exemption is manufactured.
"""
from dataclasses import dataclass
from typing import Mapping

from decode_nonregression_schema import SchemaPaths
from decode_nonregression_schema_bindings import generated_arena


class WireAdmissionError(ValueError):
    """Historical wire admission has no complete source-backed proof."""


@dataclass(frozen=True)
class WireAdmission:
    paths: SchemaPaths
    carrier_path: tuple[str, ...]
    carrier: str


# These are the helper's actual public generic type instantiations, not field
# allowlists. Private wire schemas are never guessed or replaced.
_TYPES = {
    'procedural_surfaces': ('ProceduralSurface', 'SurfaceId'),
    'procedural_curves': ('ProceduralCurve', 'CurveId'),
}


def verify_wire_rows(report: Mapping, original: Mapping, queries: Mapping) -> dict:
    """Prove every requested exact wire row; return only record-specific paths."""
    if set(report) != {'wire_protocol', 'build', 'ir_version', 'requested', 'rows'}:
        raise WireAdmissionError('malformed wire-admission report')
    if report['wire_protocol'] != 'baseline-procedural-wire-v1' or report['build'] != 'baseline':
        raise WireAdmissionError('unknown wire-admission protocol')
    if report['ir_version'] != original.get('ir_version'):
        raise WireAdmissionError('wire-admission producer has another IR version')
    requested = report['requested']
    rows = report['rows']
    if (not isinstance(requested, list) or not requested or any(not isinstance(x, str) or not x for x in requested)
            or len(set(requested)) != len(requested) or not isinstance(rows, list)
            or [row.get('record_id') for row in rows if isinstance(row, dict)] != requested):
        raise WireAdmissionError('wire admission did not cover unique actual requests')
    result = {}
    for row in rows:
        if set(row) != {'record_id', 'arena', 'owner', 'carrier_identity', 'carrier', 'carrier_path', 'wire_record', 'payload_record'}:
            raise WireAdmissionError('malformed typed wire-row proof')
        arena, identity = row['arena'], row['record_id']
        if arena not in _TYPES or arena not in queries:
            raise WireAdmissionError('unsupported actual typed wire owner')
        owner, carrier_type = _TYPES[arena]
        expected_owner = 'cadmpeg_ir::geometry::' + owner
        expected_carrier = 'cadmpeg_ir::ids::' + carrier_type
        if row['owner'] != {'schema_id': expected_owner, 'rust_type': expected_owner, 'schema_name': owner}:
            raise WireAdmissionError('wire owner differs from the actual typed producer')
        if row['carrier_identity'] != {'schema_id': expected_carrier, 'rust_type': expected_carrier, 'schema_name': carrier_type}:
            raise WireAdmissionError('carrier has no genuine typed identity binding')
        query = queries[arena]
        if query['payload']['arena'] != 'model.' + arena or query['payload']['element'] != owner:
            raise WireAdmissionError('public payload query has another owner')
        source = [record for record in original['model'].get(arena, []) if record.get('id') == identity]
        if len(source) != 1 or source[0] != row['wire_record']:
            raise WireAdmissionError('actual admitted wire row differs from the source row')
        wire, payload = row['wire_record'], row['payload_record']
        if not isinstance(payload, dict) or payload.get('id') != identity:
            raise WireAdmissionError('typed public payload has another identity')
        path = row['carrier_path']
        if (not isinstance(path, list) or len(path) != 1 or not isinstance(path[0], str)
                or not isinstance(row['carrier'], str) or not row['carrier']
                or path[0] in payload or wire.get(path[0]) != row['carrier']):
            raise WireAdmissionError('actual serializer carrier field is not uniquely represented')
        for key, value in wire.items():
            if key != path[0] and (key not in payload or payload[key] != value):
                raise WireAdmissionError('source wire value is missing or changed in typed payload')
        for key, value in payload.items():
            if key not in wire and value is not None:
                raise WireAdmissionError('typed roundtrip introduced an unproved payload field')
        # The historical query is unchanged. Unknown fields, branch constraints,
        # numerical bounds and required fields all continue to apply here.
        paths = generated_arena(query).paths([payload])
        result[(arena, identity)] = WireAdmission(paths, tuple(path), row['carrier'])
    return result
