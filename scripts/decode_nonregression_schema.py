# SPDX-License-Identifier: Apache-2.0
"""Finite-value adapter for generated `query schema types` payloads.

Identity bindings name exact generated `$defs` references from the owning Rust
schema metadata. They are inputs, not field-name or object-shape discoveries.
Schema evaluation requires jsonschema>=4; importing SchemaPaths does not.
This module does not authenticate those bindings, fetch a binary, resolve CAD
parameters, or enable the corpus gate.
"""

from __future__ import annotations

from dataclasses import dataclass
import math
import re
from typing import Any, Mapping, Sequence


JsonPath = tuple[str, ...]


class GeneratedSchemaError(ValueError):
    """Generated schema, type bindings, or supplied decoded value is invalid."""


@dataclass(frozen=True)
class SchemaPaths:
    """Exact collapsed paths supplied by the schema/typed-D10 adapter."""

    variant_tags: frozenset[JsonPath]
    dimensions: frozenset[JsonPath]
    d10_companions: frozenset[JsonPath] = frozenset()


@dataclass(frozen=True)
class Companion:
    """A D10 field bound to its owning generated Rust type and target type."""

    owner: str
    field: str
    target: str
    variant: tuple[str, str] | None = None


@dataclass(frozen=True)
class TypeBindings:
    """Per-build exact type identities; no suffix or duck-type recognition."""

    dimensions: frozenset[str] = frozenset()
    companions: tuple[Companion, ...] = ()


# Schema-bearing keywords only: authored const/default/example objects are data.
_SINGLE = ('items', 'contains', 'additionalProperties', 'propertyNames', 'not',
           'if', 'then', 'else', 'unevaluatedProperties', 'unevaluatedItems')
_LIST = ('oneOf', 'anyOf', 'allOf', 'prefixItems')
_MAP = ('properties', 'patternProperties', '$defs', 'dependentSchemas')


class GeneratedArena:
    """One build's arena schema, evaluated only over supplied finite values."""

    def __init__(self, query: Mapping[str, Any], bindings: TypeBindings = TypeBindings()):
        try:
            from jsonschema import Draft202012Validator
            from jsonschema.exceptions import SchemaError, ValidationError
        except ImportError as error:
            raise GeneratedSchemaError('generated-schema adapter requires jsonschema>=4') from error
        self._validator_class = Draft202012Validator
        self._schema_errors = (SchemaError, ValidationError)
        try:
            if query['status'] != 'ok' or query.get('refusal') is not None:
                raise GeneratedSchemaError('schema query did not succeed')
            payload = query['payload']
            self.arena = payload['arena']
            if not isinstance(self.arena, str) or not self.arena.startswith('model.'):
                raise GeneratedSchemaError('expected a named model arena')
            self.schema, self.defs = payload['schema'], payload['defs']
            self.element = payload['element']
            if not isinstance(self.schema, dict) or not isinstance(self.defs, dict):
                raise GeneratedSchemaError('schema and defs must be objects')
            if not isinstance(self.element, str):
                raise GeneratedSchemaError('element identity must be a string')
            self.bindings = bindings
            self._validators: dict[int, Any] = {}
            Draft202012Validator.check_schema({'$defs': self.defs, **self.schema})
            self._check_graph(self.schema, set())
            for schema in self.defs.values():
                self._check_graph(schema, set())
            for name in bindings.dimensions:
                self._definition(name)
            for row in bindings.companions:
                self._definition(row.owner)
                self._definition(row.target)
                if row.field not in {'parameter', 'distance_parameter', 'angle_parameter', 'count_parameter'}:
                    raise GeneratedSchemaError('field is not a D10 companion')
                if row.target not in {'ParameterId', 'OffsetParameter'}:
                    raise GeneratedSchemaError('D10 companion target is not its Rust reference type')
        except (KeyError, TypeError, SchemaError, RecursionError) as error:
            raise GeneratedSchemaError(f'invalid generated schema query: {error}') from error

    def _definition(self, name: str) -> dict:
        schema = self.defs.get(name)
        if not isinstance(schema, dict):
            raise GeneratedSchemaError(f'unresolved generated type {name!r}')
        return schema

    def _ref(self, reference: str) -> tuple[str, dict]:
        if not isinstance(reference, str) or not reference.startswith('#/$defs/'):
            raise GeneratedSchemaError(f'nonlocal or malformed reference {reference!r}')
        encoded = reference[len('#/$defs/'):]
        if '/' in encoded or '~' in encoded.replace('~0', '').replace('~1', ''):
            raise GeneratedSchemaError(f'malformed reference {reference!r}')
        name = encoded.replace('~1', '/').replace('~0', '~')
        return name, self._definition(name)

    def _validator(self, schema: Any) -> Any:
        key = id(schema)
        if key not in self._validators:
            root = {'$defs': self.defs, **schema} if isinstance(schema, dict) else schema
            self._validators[key] = self._validator_class(root)
        return self._validators[key]

    def _check_graph(self, schema: Any, seen: set[int]) -> None:
        if isinstance(schema, bool):
            return
        if not isinstance(schema, dict):
            raise GeneratedSchemaError('schema node must be an object or boolean')
        if id(schema) in seen:
            return
        seen.add(id(schema))
        if any(key in schema for key in (
            '$id', '$dynamicRef', '$recursiveRef', '$defs', 'if', 'then', 'else', 'dependentSchemas',
        )):
            raise GeneratedSchemaError('unsupported conditional or reference-resolution schema')
        if '$ref' in schema:
            self._ref(schema['$ref'])
            structural = set(schema) & set(_SINGLE + _LIST + _MAP)
            if structural - {'properties'}:
                raise GeneratedSchemaError('unsupported structural reference intersection')
            if 'properties' in schema or 'required' in schema:
                self._resolve(schema)
        for keyword in _SINGLE:
            if keyword in schema:
                self._check_graph(schema[keyword], seen)
        for keyword in _LIST:
            for child in schema.get(keyword, []):
                self._check_graph(child, seen)
        for keyword in _MAP:
            for child in schema.get(keyword, {}).values():
                self._check_graph(child, seen)

    def _resolve(self, schema: dict, seen: frozenset[str] = frozenset()) -> dict:
        if '$ref' not in schema:
            return schema
        name, target = self._ref(schema['$ref'])
        if name in seen:
            raise GeneratedSchemaError('reference cycle without consuming a value')
        target = self._resolve(target, seen | {name})
        if 'properties' not in schema and 'required' not in schema:
            return target
        if name in self.bindings.dimensions:
            raise GeneratedSchemaError('structural Dimension reference intersection is unsupported')
        if target.get('type') != 'object' or schema.get('type', 'object') != 'object':
            raise GeneratedSchemaError('property reference intersection requires explicit object types')
        if any(key in target for key in ('oneOf', 'anyOf', 'allOf')):
            raise GeneratedSchemaError('object reference intersection with an applicator is unsupported')
        # This is a traversal view. The unchanged original schema validates the
        # complete reference/sibling conjunction before any value is traversed.
        shape = dict(target)
        properties = dict(target.get('properties', {}))
        for field, constraint in schema.get('properties', {}).items():
            if field in properties:
                properties[field] = {'allOf': [properties[field], constraint]}
            else:
                properties[field] = constraint
        shape['properties'] = properties
        shape['required'] = sorted(set(target.get('required', [])) | set(schema.get('required', [])))
        return shape

    def _string_const(self, schema: dict) -> str | None:
        schema = self._resolve(schema)
        constants = []
        if isinstance(schema.get('const'), str):
            constants.append(schema['const'])
        for child in schema.get('allOf', []):
            value = self._string_const(child)
            if value is not None:
                constants.append(value)
        return constants[0] if constants and len(set(constants)) == 1 else None

    def _tag(self, schema: dict) -> str | None:
        branches = schema.get('oneOf', schema.get('anyOf'))
        if not branches:
            return None
        def variants(branch: dict, seen: frozenset[int]) -> list[dict] | None:
            branch = self._resolve(branch)
            if id(branch) in seen:
                raise GeneratedSchemaError('recursive union without a value boundary')
            if branch.get('type') == 'null':
                return []
            nested = branch.get('oneOf', branch.get('anyOf'))
            if nested is not None:
                output = []
                for child in nested:
                    expanded = variants(child, seen | {id(branch)})
                    if expanded is None:
                        return None
                    output.extend(expanded)
                return output
            return [branch] if branch.get('type') == 'object' else None

        objects = []
        for branch in branches:
            expanded = variants(branch, frozenset())
            if expanded is None:
                return None
            objects.extend(expanded)
        candidates = None
        for branch in objects:
            fields = {
                key: constant for key, child in branch.get('properties', {}).items()
                if key in branch.get('required', []) and isinstance(child, dict)
                and (constant := self._string_const(child)) is not None
            }
            candidates = set(fields) if candidates is None else candidates & fields.keys()
        tags = [key for key in candidates or () if len({
            self._string_const(branch['properties'][key]) for branch in objects
        }) == len(objects)]
        if len(tags) > 1:
            raise GeneratedSchemaError('ambiguous tagged-union discriminant')
        return tags[0] if tags else None

    def paths(self, records: Sequence[Any]) -> SchemaPaths:
        tags: set[JsonPath] = set()
        dimensions: set[JsonPath] = set()
        companions: set[JsonPath] = set()
        active: set[tuple[int, int]] = set()
        ordinary_leaves: set[JsonPath] = set()
        ordinary_fields: set[JsonPath] = set()
        ordinary_wrappers: set[JsonPath] = set()

        def walk(schema: Any, value: Any, path: JsonPath, owner: str | None = None,
                 tag_paths: frozenset[JsonPath] = frozenset()) -> None:
            if isinstance(schema, bool):
                if schema and isinstance(value, (dict, list)):
                    raise GeneratedSchemaError('structured value has no generated type schema')
                return
            key = (id(schema), id(value))
            if key in active:
                raise GeneratedSchemaError('schema/value cycle without progress')
            active.add(key)
            try:
                if '$ref' in schema:
                    name, target = self._ref(schema['$ref'])
                    if name in self.bindings.dimensions:
                        if not isinstance(value, dict) or set(value) != {'kind', 'value'}:
                            raise GeneratedSchemaError('invalid bound Dimension value')
                        if value['kind'] not in {'literal', 'parameter'}:
                            raise GeneratedSchemaError('invalid bound Dimension variant')
                        dimensions.add(path)
                        selected = [branch for branch in target.get('oneOf', [])
                                    if self._validator(branch).is_valid(value)]
                        if len(selected) != 1:
                            raise GeneratedSchemaError('Dimension type has no unique matching branch')
                        walk(selected[0]['properties']['value'], value['value'], path)
                    else:
                        resolved = self._resolve(schema)
                        walk(resolved, value, path, name, tag_paths)
                    return
                branches = schema.get('oneOf', schema.get('anyOf', []))
                scalar_variants = [self._resolve(branch) for branch in branches]
                scalar_enum = bool(scalar_variants) and all(
                    branch.get('type') == 'string' and isinstance(branch.get('const'), str)
                    for branch in scalar_variants
                )
                if scalar_enum or (schema.get('type') == 'string' and isinstance(schema.get('enum'), list)):
                    tags.add(path)
                    tag_paths = tag_paths | {path}
                tag = self._tag(schema)
                if tag is not None:
                    tags.add(path + (tag,))
                    tag_paths = tag_paths | {path + (tag,)}
                for keyword in ('oneOf', 'anyOf', 'allOf'):
                    if keyword in schema:
                        branches = schema[keyword]
                        selected = branches if keyword == 'allOf' else [
                            branch for branch in branches if self._validator(branch).is_valid(value)
                        ]
                        for branch in selected:
                            walk(branch, value, path, owner, tag_paths)
                if isinstance(value, dict):
                    properties = schema.get('properties', {})
                    if set(value) == {'kind', 'value'}:
                        ordinary_wrappers.add(path)
                    local_companions = set()
                    for row in self.bindings.companions:
                        if row.owner != owner or row.field not in properties:
                            continue
                        if row.variant is not None and value.get(row.variant[0]) != row.variant[1]:
                            continue
                        field = properties[row.field]
                        choices = field.get('anyOf', [field])
                        refs = {self._ref(choice['$ref'])[0] for choice in choices if '$ref' in choice}
                        if refs != {row.target}:
                            raise GeneratedSchemaError('D10 binding does not match generated field type')
                        companions.add(path + (row.field,))
                        local_companions.add(row.field)
                    for field, child in value.items():
                        if field in properties:
                            if field not in local_companions:
                                ordinary_fields.add(path + (field,))
                            walk(properties[field], child, path + (field,), tag_paths=tag_paths)
                        else:
                            patterns = schema.get('patternProperties', {})
                            matched = [node for pattern, node in patterns.items() if re.search(pattern, field)]
                            for node in matched:
                                walk(node, child, path + (field,), tag_paths=tag_paths)
                            if not matched:
                                if 'additionalProperties' in schema:
                                    walk(schema['additionalProperties'], child, path + (field,), tag_paths=tag_paths)
                                elif not any(key in schema for key in ('oneOf', 'anyOf', 'allOf')):
                                    raise GeneratedSchemaError('object field has no generated type schema')
                elif isinstance(value, list) and (
                    schema.get('type') == 'array' or 'items' in schema or 'prefixItems' in schema
                ):
                    prefix = schema.get('prefixItems', [])
                    for index, child in enumerate(value):
                        item = prefix[index] if index < len(prefix) else schema.get('items')
                        if item is None:
                            raise GeneratedSchemaError('array value has no generated item type')
                        walk(item, child, path + ('#',), tag_paths=tag_paths)
                elif value is not None and not isinstance(value, (dict, list)):
                    if isinstance(value, float) and not math.isfinite(value):
                        raise GeneratedSchemaError('decoded value contains a nonfinite number')
                    if path not in tag_paths and not any(
                        key in schema for key in ('oneOf', 'anyOf', 'allOf')
                    ):
                        ordinary_leaves.add(path)
            finally:
                active.remove(key)

        try:
            for record in records:
                self._validator(self.schema).validate(record)
                walk(self.schema, record, (self.arena.removeprefix('model.'), '#'), self.element)
        except self._schema_errors + (RecursionError, TypeError, KeyError) as error:
            raise GeneratedSchemaError(f'generated schema/value mismatch: {error}') from error
        if dimensions & ordinary_wrappers or companions & ordinary_fields:
            raise GeneratedSchemaError('collapsed path mixes typed wrapper/companion roles; classify records separately')
        if tags & ordinary_leaves:
            raise GeneratedSchemaError('collapsed path mixes variant tags and ordinary leaves; classify records separately')
        return SchemaPaths(frozenset(tags), frozenset(dimensions), frozenset(companions))


@dataclass(frozen=True)
class BuildPaths:
    """Keep independently generated base/head classifications separate."""

    base: SchemaPaths
    head: SchemaPaths
