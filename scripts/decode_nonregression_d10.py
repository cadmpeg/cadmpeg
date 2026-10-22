# SPDX-License-Identifier: Apache-2.0
"""Record-specific D10 length equality using actual typed baseline pair output.

The guarded baseline producer reads admitted Rust fields. Head records and
Dimension identities come from guarded generated queries. Length resolution
matches Dimension<Length>::value_from and DimensionValue for Length: a literal
or ParameterValue::Length cache, never expression evaluation or quantity casts.
This bounded pair verifier does not enumerate all D10 cases or enable the gate.
"""

from dataclasses import dataclass
import math
from typing import Any, Mapping

from decode_nonregression_schema import GeneratedArena, GeneratedSchemaError
from decode_nonregression_schema_bindings import from_query


OWNER = 'cadmpeg_ir::sketches::SketchConstraintDefinition'
LENGTH = 'cadmpeg_ir::scalar::Length'
DIMENSION_LENGTH = f'cadmpeg_ir::features::dimensions::Dimension<{LENGTH}>'
FLOAT_TOLERANCE = 1.0e-12  # cadmpeg_ir::compare::FLOAT_TOLERANCE
# These variants are read by the baseline producer's actual typed match arms.
PAIRS = {'distance_loci_value': 'parameter', 'polar_distance': 'distance_parameter'}


class D10ResolutionError(ValueError):
    """A typed pair or its current effective value lacks the required proof."""


@dataclass(frozen=True)
class D10Equality:
    """One exact record obligation discharged after value AND binding equality."""

    record_id: str
    value_path: tuple[str, ...]
    companion_path: tuple[str, ...]
    value: float
    parameter: str | None


def _number(value: Any) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise D10ResolutionError('D10 length is not a finite numeric value')
    return float(value)


def _index(records: Any, label: str) -> dict:
    if not isinstance(records, list):
        raise D10ResolutionError(f'{label} must be a record array')
    result = {}
    for record in records:
        if not isinstance(record, dict) or not isinstance(record.get('id'), str) or not record['id']:
            raise D10ResolutionError(f'{label} has no actual record identity')
        if record['id'] in result:
            raise D10ResolutionError(f'{label} has duplicate record identities')
        result[record['id']] = record
    return result


def _field(arena: GeneratedArena, record: dict, path: list[str]) -> tuple[dict, Any, list[str]]:
    schema, value, references = arena.schema, record, []
    for field in path:
        while True:
            if '$ref' in schema:
                references.append(schema['$ref'])
                schema = arena._resolve(schema)
            unions = [key for key in ('oneOf', 'anyOf') if key in schema]
            if not unions:
                break
            if len(unions) != 1:
                raise D10ResolutionError('D10 field has multiple unresolved schema applicators')
            selected = [branch for branch in schema[unions[0]] if arena._validator(branch).is_valid(value)]
            if len(selected) != 1:
                raise D10ResolutionError('D10 field has no unique actual schema branch')
            schema = selected[0]
        if 'allOf' in schema:
            raise D10ResolutionError('D10 field intersection traversal is not implemented')
        if not isinstance(value, dict) or field not in value or field not in schema.get('properties', {}):
            raise D10ResolutionError('typed D10 value field is missing')
        schema, value = schema['properties'][field], value[field]
    return schema, value, references


def _owner(bindings: Mapping, owner: Mapping, references: list[str] | None = None) -> bool:
    return any(
        (references is None or reference in references)
        and row['role'] == 'd10_owner'
        and {key: row[key] for key in ('schema_id', 'rust_type')} == owner
        for reference, aliases in bindings.items() for row in aliases
    )


def verify_length_pairs(report: Mapping, head_ir: Mapping, base_query: Mapping,
                        head_query: Mapping, parameter_query: Mapping) -> tuple[D10Equality, ...]:
    """Reject the entire requested batch unless every typed pair proves equality.

    Producers and input/output hashes must be verified by the owning collector.
    Returned obligations are record-specific; they are not SchemaPaths exemptions.
    """
    try:
        if set(report) != {'resolver_protocol', 'build', 'ir_version', 'requested', 'rows'}:
            raise D10ResolutionError('malformed typed baseline resolver report')
        if report['resolver_protocol'] != 'd10-length-pairs-v1' or report['build'] != 'baseline':
            raise D10ResolutionError('unknown typed baseline resolver protocol')
        if not isinstance(report['ir_version'], str) or not report['ir_version']:
            raise D10ResolutionError('baseline resolver has no compiled IR version')
        requested, rows = report['requested'], report['rows']
        if not isinstance(requested, list) or not requested or any(not isinstance(x, str) or not x for x in requested):
            raise D10ResolutionError('actual nonempty requested record IDs are required')
        if len(set(requested)) != len(requested) or not isinstance(rows, list):
            raise D10ResolutionError('duplicate requests or malformed typed pair rows')
        if [row.get('record_id') for row in rows if isinstance(row, dict)] != requested or len(rows) != len(requested):
            raise D10ResolutionError('typed baseline resolver did not cover every exact requested record')
        from_query(base_query)
        head_types = from_query(head_query)
        from_query(parameter_query)
        if base_query['payload']['arena'] != 'model.sketch_constraints' or head_query['payload']['arena'] != 'model.sketch_constraints':
            raise D10ResolutionError('D10 pair schema is not its actual constraint arena')
        if parameter_query['payload']['arena'] != 'model.parameters':
            raise D10ResolutionError('parameter schema is not its actual arena')
        base_bindings = base_query['payload']['type_metadata']['references']
        head_bindings = head_query['payload']['type_metadata']['references']
        arena = GeneratedArena(head_query, head_types.bindings)
        parameter_arena = GeneratedArena(parameter_query)
        model = head_ir['model']
        constraints = _index(model.get('sketch_constraints', []), 'head constraints')
        parameters = _index(model.get('parameters', []), 'head parameters')
        proofs = []
        for row in rows:
            if set(row) != {'record_id', 'arena', 'owner', 'variant', 'value_path', 'companion_path', 'quantity', 'value', 'parameter'}:
                raise D10ResolutionError('malformed typed pair row')
            companion = PAIRS.get(row['variant'])
            if companion is None or row['arena'] != 'sketch_constraints':
                raise D10ResolutionError('typed D10 variant is not implemented')
            if row['value_path'] != ['definition', 'distance'] or row['companion_path'] != ['definition', companion]:
                raise D10ResolutionError('typed D10 field pair differs from its actual source accessor')
            if row['owner'] != {'schema_id': OWNER, 'rust_type': OWNER} or not _owner(base_bindings, row['owner']):
                raise D10ResolutionError('baseline pair owner has no authentic generated type binding')
            if row['quantity'] != {'schema_id': LENGTH, 'rust_type': LENGTH}:
                raise D10ResolutionError('baseline pair quantity is not the actual Length type')
            expected = _number(row['value'])
            parameter = row['parameter']
            if parameter is not None and (not isinstance(parameter, str) or not parameter):
                raise D10ResolutionError('baseline companion is not a parameter identity')
            record = constraints.get(row['record_id'])
            if record is None:
                raise D10ResolutionError('baseline constraint has no exact head identity')
            arena.paths([record])
            if record['definition'].get('kind') != row['variant']:
                raise D10ResolutionError('head D10 variant changed')
            field_schema, dimension, references = _field(arena, record, row['value_path'])
            if not _owner(head_bindings, row['owner'], references):
                raise D10ResolutionError('head field is not inside the same authentic typed owner')
            reference = field_schema.get('$ref')
            if reference not in head_bindings:
                raise D10ResolutionError('head value field has no authentic Dimension identity')
            identity = head_bindings[reference][0]
            if identity['role'] != 'dimension' or identity['rust_type'] != DIMENSION_LENGTH:
                raise D10ResolutionError('head value field is not actual Dimension<Length>')
            if not isinstance(dimension, dict) or set(dimension) != {'kind', 'value'}:
                raise D10ResolutionError('head dimension wrapper is malformed')
            if dimension['kind'] == 'literal':
                value, bound = _number(dimension['value']), None
            elif dimension['kind'] == 'parameter':
                bound = dimension['value']
                if not isinstance(bound, str) or bound not in parameters:
                    raise D10ResolutionError('head bound dimension has no canonical parameter')
                canonical = parameters[bound]
                parameter_arena.paths([canonical])
                cache = canonical.get('value')
                if not isinstance(cache, dict) or set(cache) != {'kind', 'value'} or cache['kind'] != 'length':
                    raise D10ResolutionError('head canonical parameter has no evaluated Length cache')
                value = _number(cache['value'])
            else:
                raise D10ResolutionError('unknown head Dimension variant')
            if bound != parameter:
                raise D10ResolutionError('D10 canonical parameter binding differs from baseline companion')
            if abs(value - expected) > FLOAT_TOLERANCE * max(abs(value), abs(expected), 1.0):
                raise D10ResolutionError('D10 effective value differs from baseline value')
            proofs.append(D10Equality(row['record_id'], tuple(row['value_path']), tuple(row['companion_path']), value, bound))
        return tuple(proofs)
    except (GeneratedSchemaError, KeyError, TypeError, AttributeError) as error:
        raise D10ResolutionError(f'D10 typed proof is incomplete: {error}') from error
