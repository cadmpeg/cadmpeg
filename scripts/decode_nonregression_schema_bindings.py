# SPDX-License-Identifier: Apache-2.0
"""Consume compiler-produced schema reference identities without caller exemptions.

The command/source guard owns authenticity of query JSON. These checks reject
missing, conflicting, or structurally unbound metadata; JSON cannot authenticate
its own producer. D10 owner identity is not proof of a value/companion field pair.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Mapping

from decode_nonregression_schema import GeneratedArena, GeneratedSchemaError, TypeBindings


_DIMENSION = 'cadmpeg_ir::features::dimensions::Dimension'
_PARAMETER = 'cadmpeg_ir::features::ParameterId'
_OFFSET = 'cadmpeg_ir::sketches::OffsetParameter'
_OWNERS = frozenset({
    'cadmpeg_ir::sketches::SketchConstraintDefinition',
    'cadmpeg_ir::sketches::SketchConstraintDefinitionInput',
    'cadmpeg_ir::sketches::SpatialSketchConstraintDefinition',
    'cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput',
    'cadmpeg_ir::sketches::SketchPatternDirection',
    'cadmpeg_ir::sketches::SketchCircularPattern',
})


@dataclass(frozen=True)
class AuthenticatedTypes:
    """Validated bindings from one guarded compiler query, never merged builds."""

    bindings: TypeBindings
    d10_owners: frozenset[str]
    parameter_targets: frozenset[str]
    offset_targets: frozenset[str]


def _family(identity: str, nominal: str) -> bool:
    return identity == nominal or (
        identity.startswith(nominal + '<') and identity.endswith('>')
        and bool(identity[len(nominal) + 1:-1])
    )


def from_query(query: Mapping[str, Any]) -> AuthenticatedTypes:
    """Validate same-query nominal references; never infer D10 field pairs."""
    arena = GeneratedArena(query)
    try:
        references = query['payload']['type_metadata']['references']
    except (KeyError, TypeError) as error:
        raise GeneratedSchemaError('compiler schema identity metadata is required') from error
    if not isinstance(references, dict):
        raise GeneratedSchemaError('compiler reference metadata must be an object')
    dimensions, owners, parameters, offsets = set(), set(), set(), set()
    identities = {}
    for reference, aliases in references.items():
        if not isinstance(aliases, list) or len(aliases) != 1 or not isinstance(aliases[0], dict):
            raise GeneratedSchemaError('reference has missing or nonexclusive Rust identity bindings')
        binding = aliases[0]
        if set(binding) != {'schema_id', 'rust_type', 'role'}:
            raise GeneratedSchemaError('malformed compiler identity binding')
        schema_id, rust_type, role = (binding[key] for key in ('schema_id', 'rust_type', 'role'))
        if not all(isinstance(value, str) and value for value in (schema_id, rust_type, role)):
            raise GeneratedSchemaError('compiler identity fields must be nonempty strings')
        # The element schema is resolved out of defs by the CLI itself.
        if isinstance(reference, str) and reference.startswith('#/$defs/'):
            encoded = reference[len('#/$defs/'):]
            if '/' in encoded or '~' in encoded.replace('~0', '').replace('~1', ''):
                raise GeneratedSchemaError('malformed compiler reference')
            name = encoded.replace('~1', '/').replace('~0', '~')
        else:
            raise GeneratedSchemaError('compiler binding must name a local generated reference')
        if name not in arena.defs and name != arena.element:
            raise GeneratedSchemaError('compiler reference is absent from the same arena query')
        if (schema_id, rust_type) in identities and identities[(schema_id, rust_type)] != reference:
            raise GeneratedSchemaError('one compiler identity names multiple references')
        # A shared schema_id is also ambiguous even if a caller changes rust_type.
        if any(previous[0] == schema_id and previous[1] != rust_type for previous in identities):
            raise GeneratedSchemaError('schema_id is shared by distinct Rust types')
        if any(previous[1] == rust_type and previous[0] != schema_id for previous in identities):
            raise GeneratedSchemaError('one Rust type has conflicting schema_id bindings')
        identities[(schema_id, rust_type)] = reference
        if role == 'dimension' and _family(schema_id, _DIMENSION) and _family(rust_type, _DIMENSION):
            if schema_id == _DIMENSION or rust_type == _DIMENSION:
                raise GeneratedSchemaError('dimension metadata lacks concrete generic arguments')
            dimensions.add(name)
        elif role == 'd10_owner' and any(
            _family(schema_id, nominal) and _family(rust_type, nominal) for nominal in _OWNERS
        ):
            owners.add(name)
        elif role == 'parameter_id' and schema_id == rust_type == _PARAMETER:
            parameters.add(name)
        elif role == 'offset_parameter' and schema_id == rust_type == _OFFSET:
            offsets.add(name)
        else:
            raise GeneratedSchemaError('role does not match its genuine nominal Rust identity')
    # Shape validation is delegated to the existing adapter after nominal binding.
    bindings = TypeBindings(frozenset(dimensions))
    GeneratedArena(query, bindings)
    return AuthenticatedTypes(bindings, frozenset(owners), frozenset(parameters), frozenset(offsets))


def generated_arena(query: Mapping[str, Any]) -> GeneratedArena:
    """Bind dimensions only through the guarded query's compiler metadata."""
    return GeneratedArena(query, from_query(query).bindings)


def attach_external(query: Mapping[str, Any], external: Mapping[str, Any]) -> dict:
    """Pair an old query with guarded helper output only after exact root agreement.

    The source/build guard must establish the external helper's baseline identity.
    This equality check does not replace that guard or a binary provenance check.
    """
    try:
        payload = query['payload']
        if 'type_metadata' in payload:
            raise GeneratedSchemaError('existing compiler metadata cannot be replaced by external bindings')
        root = external['schema']
        defs = root['$defs']
        model = root['properties']['model']
        if '$ref' in model:
            model = _resolve_root(root, model['$ref'])
        arena = payload['arena']
        if not isinstance(arena, str) or not arena.startswith('model.'):
            raise GeneratedSchemaError('external metadata requires a model arena query')
        items = model['properties'][arena[len('model.'):]]['items']
        element = _resolve_root(root, items['$ref']) if '$ref' in items else items
        expected_element = items['$ref'][len('#/$defs/'):].replace('~1', '/').replace('~0', '~') if '$ref' in items else '(inline)'
        if payload['element'] != expected_element:
            raise GeneratedSchemaError('external helper element identity differs from baseline query')
        reached = set()
        queue = [element]
        while queue:
            node = queue.pop()
            if isinstance(node, dict):
                if '$ref' in node:
                    target = _resolve_root(root, node['$ref'])
                    name = node['$ref'][len('#/$defs/'):].replace('~1', '/').replace('~0', '~')
                    if name not in reached:
                        reached.add(name)
                        queue.append(target)
                queue.extend(node.values())
            elif isinstance(node, list):
                queue.extend(node)
        if reached != set(payload['defs']):
            raise GeneratedSchemaError('external helper closure differs from baseline query')
        if element != payload['schema']:
            raise GeneratedSchemaError('external helper element differs from actual baseline query')
        for name, schema in payload['defs'].items():
            if name not in defs or schema != defs[name]:
                raise GeneratedSchemaError('external helper defs differ from actual baseline query')
        result = dict(query)
        result['payload'] = dict(payload)
        names = set(payload['defs']) | {payload['element']}
        references = external['type_metadata']['references']
        if not isinstance(references, dict):
            raise GeneratedSchemaError('external reference metadata must be an object')
        selected = {}
        for reference, aliases in references.items():
            if not isinstance(reference, str) or not reference.startswith('#/$defs/'):
                raise GeneratedSchemaError('external metadata contains a nonlocal reference')
            encoded = reference[len('#/$defs/'):]
            if '/' in encoded or '~' in encoded.replace('~0', '').replace('~1', ''):
                raise GeneratedSchemaError('external metadata contains an invalid reference')
            name = encoded.replace('~1', '/').replace('~0', '~')
            if name not in defs:
                raise GeneratedSchemaError('external metadata names no original-root definition')
            if name in names:
                selected[reference] = aliases
        result['payload']['type_metadata'] = {'references': selected}
        from_query(result)
        return result
    except (KeyError, TypeError) as error:
        raise GeneratedSchemaError('invalid baseline helper/query envelope') from error


def _resolve_root(root: dict, reference: Any) -> Any:
    if not isinstance(reference, str) or not reference.startswith('#/$defs/'):
        raise GeneratedSchemaError('external root requires local generated references')
    encoded = reference[len('#/$defs/'):]
    if '/' in encoded or '~' in encoded.replace('~0', '').replace('~1', ''):
        raise GeneratedSchemaError('external root has a malformed reference')
    name = encoded.replace('~1', '/').replace('~0', '~')
    try:
        return root['$defs'][name]
    except KeyError as error:
        raise GeneratedSchemaError('external root reference is unresolved') from error
