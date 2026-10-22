#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the constants-only removed-name registry and all arena declarations.

The required names come from #216, #246 check 2, the codec §8 deletions and
#215's named failure arenas (#219 enumerates those arenas). No codec-adoption
filter, staging exemption or whole-arena suppression is accepted.
"""

from __future__ import annotations

import argparse
import csv
import importlib.util
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
REGISTRY = Path('crates/cadmpeg-test-support/src/removed_names.rs')
REQUIRED_NAMES = frozenset((
    'active_attribute_use',
    'active_brep',
    'alignment',
    'assembly_record_issues',
    'backgroundColor',
    'block_dimension.N',
    'block_dimension_expression.N',
    'body_write.N',
    'brep_face_admission_rejections',
    'brep_local_sha256',
    'byte_coverage',
    'consumer.N',
    'database_issues',
    'datum_csys_block_use.N',
    'declaration',
    'declaration_object_id',
    'definition_id',
    'design_intent_failures',
    'design_record_issues',
    'displayUnit',
    'document_local_sha256',
    'evaluated_scalar',
    'feature_record_issues',
    'foregroundColor',
    'input_block.N',
    'input_block_record.N',
    'input_parameter_expression.S.O',
    'line_width',
    'logical_ledger',
    'operation_record',
    'owner_record_index',
    'parameter_use.N',
    'parasolid_deltas_residual_spans',
    'physical_ledger',
    'point_size',
    'presentation_record_issues',
    'primary_body_reference',
    'property_set_issues',
    'protein_rejections',
    'quarantined_directory_records',
    'quarantined_parameter_records',
    'segment_bulk_issues',
    'segment_meta_issues',
    'sketch_record.N',
    'sketch_record_issues',
    'sldprt_active_partition_unresolved',
    'sldprt_configuration_snapshot_synthesized',
    'sldprt_native_history_sha256',
    'sldprt_native_parameter_sha256',
    'sldprt_neutral_feature_local_sha256',
    'sldprt_neutral_parameter_local_sha256',
    'source_kind',
    'structural_issues',
    'style',
    'surface_construction_reference_record.N',
    'thumbnail_bytes',
    'unit',
    'unreadable_entries',
    'view_bytes',
    'view_sha256',
))
TOKEN = re.compile(r'//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"|[A-Za-z_]\w*|\S')


class RegistryError(ValueError):
    """The registry or an authoritative arena inventory is invalid."""


def parse_registry(text: str) -> dict[str, str]:
    tokens = [t for t in TOKEN.findall(text) if not t.startswith(('//', '/*'))]
    values = {}
    for start in range(0, len(tokens), 9):
        row = tokens[start:start + 9]
        if (len(row) != 9 or row[:2] != ['pub', 'const'] or row[3:7] != [':', '&', 'str', '=']
                or row[8] != ';' or not re.fullmatch('[A-Z][A-Z0-9_]*', row[2])):
            raise RegistryError('only pub const NAME: &str = "literal"; Rust items are allowed')
        try:
            value = json.loads(row[7])
        except (ValueError, TypeError) as error:
            raise RegistryError('registry constants require ordinary string literals') from error
        if not isinstance(value, str):
            raise RegistryError('registry constants require string values')
        if row[2] in values or value in values.values():
            raise RegistryError('duplicate removed-name identifier or value')
        values[row[2]] = value
    if not values:
        raise RegistryError('removed-name registry is empty')
    missing, extra = REQUIRED_NAMES - set(values.values()), set(values.values()) - REQUIRED_NAMES
    if missing or extra:
        raise RegistryError(f'removed-name inventory differs: missing={sorted(missing)}, extra={sorted(extra)}')
    return values


def native_arenas(root: Path) -> dict[str, list[str]]:
    path = root / 'scripts/check-native-arena-disposition.py'
    spec = importlib.util.spec_from_file_location('removed_names_arena_source', path)
    if spec is None or spec.loader is None:
        raise RegistryError(f'cannot load authoritative arena parser {path}')
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    try:
        return {registry.namespace: module.read_registry(root, registry) for registry in module.REGISTRIES}
    except (OSError, ValueError) as error:
        raise RegistryError(str(error)) from error


def absence_findings(names: set[str], arenas: dict[str, list[str]], appendix: Path) -> list[str]:
    failures = [f'native registry {namespace}: removed arena {name}'
                for namespace, values in arenas.items() for name in values if name in names]
    try:
        with appendix.open(newline='') as handle:
            rows = csv.DictReader(handle, delimiter='\t')
            if rows.fieldnames is None or 'arenas' not in rows.fieldnames:
                raise RegistryError('Appendix C has no arenas column')
            for line, row in enumerate(rows, 2):
                # The column includes only names and two documented unused-row
                # annotations. Inspect every identifier, including annotations.
                for name in set(re.findall(r'[A-Za-z_][A-Za-z0-9_]*', row['arenas'])) & names:
                    failures.append(f'Appendix C line {line}: removed arena {name}')
    except OSError as error:
        failures.append(f'Appendix C is required: {error}')
    return failures


def check(root: Path) -> list[str]:
    names = set(parse_registry((root / REGISTRY).read_text()).values())
    return absence_findings(names, native_arenas(root), root / 'scripts/part-framing.tsv')


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args(argv)
    try:
        failures = check(ROOT)
    except (RegistryError, OSError) as error:
        print(f'removed-name check: {error}', file=sys.stderr)
        return 1
    for failure in failures:
        print(failure, file=sys.stderr)
    return int(bool(failures))


if __name__ == '__main__':
    sys.exit(main())
