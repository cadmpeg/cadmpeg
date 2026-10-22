# SPDX-License-Identifier: Apache-2.0
"""Verify compiler-produced, source-bound neutral/native projection links.

Target existence is a necessary P-closure witness. Codec-specific named homes,
consumed-record reads, injectivity and B1/B1u proofs remain separate obligations.
"""
from collections import defaultdict
from dataclasses import dataclass


class NativeLinkError(ValueError):
    """A typed projection inventory does not bind the actual document."""


@dataclass(frozen=True)
class NativeLinkFinding:
    code: str
    path: tuple[str, ...]
    message: str


def verify_native_links(report, document, document_sha256):
    if report.get('status') != 'ok' or report.get('refusal') is not None:
        raise NativeLinkError('typed native-link query did not succeed')
    payload = report.get('payload')
    if (not isinstance(payload, dict) or set(payload) != {'protocol', 'ir_sha256', 'rows'}
            or payload['protocol'] != 'cadmpeg-typed-native-links-v1'
            or payload['ir_sha256'] != document_sha256):
        raise NativeLinkError('typed native-link report is not bound to the exact source bytes')
    expected = set()
    for arena, records in document.get('model', {}).items():
        for record in records:
            key = (arena, record['id'])
            if key in expected:
                raise NativeLinkError('neutral projection owner identity is ambiguous')
            expected.add(key)
    rows = payload['rows']
    if not isinstance(rows, list):
        raise NativeLinkError('typed native links must be a row array')
    seen, owners = set(), {}
    targets = defaultdict(list)
    for namespace, arenas in document.get('native', {}).items():
        for arena, records in arenas.items():
            for record in records:
                targets[record['id']].append((namespace, arena))
    findings, witnesses = [], []
    for row in rows:
        if not isinstance(row, dict) or set(row) != {'arena', 'rust_type', 'id', 'native_ref'}:
            raise NativeLinkError('malformed typed projection row')
        if any(not isinstance(row[key], str) or not row[key] for key in ('arena', 'rust_type', 'id')):
            raise NativeLinkError('typed projection owner has no compiled identity')
        key = (row['arena'], row['id'])
        if key not in expected or key in seen:
            raise NativeLinkError('typed projection owner is absent or ambiguous')
        if row['arena'] in owners and owners[row['arena']] != row['rust_type']:
            raise NativeLinkError('one neutral arena has conflicting compiled owners')
        owners[row['arena']] = row['rust_type']
        seen.add(key)
        native = row['native_ref']
        if native is None:
            continue
        if not isinstance(native, str) or not native:
            raise NativeLinkError('typed native reference is not a nonempty identity')
        locations = targets.get(native, [])
        path = ('model', row['arena'], row['id'])
        if len(locations) != 1:
            findings.append(NativeLinkFinding('native-projection-target', path,
                'typed native reference has no unique actual native target'))
        else:
            namespace, arena = locations[0]
            witnesses.append(dict(row, native_namespace=namespace, native_arena=arena))
    if seen != expected:
        raise NativeLinkError('typed projection inventory omitted actual neutral records')
    return findings, witnesses


def verify_bound_native_links(report, document_path, document, expected_sha256):
    """Bind the typed query and target inventory to one unchanged source file."""
    import hashlib
    import json

    def distinct(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise NativeLinkError('projection source has duplicate JSON object keys')
            result[key] = value
        return result

    raw = document_path.read_bytes()
    actual_sha256 = hashlib.sha256(raw).hexdigest()
    if actual_sha256 != expected_sha256:
        raise NativeLinkError('projection source bytes changed during the actual query')
    try:
        parsed = json.loads(raw, object_pairs_hook=distinct)
        if json.dumps(parsed, sort_keys=True, allow_nan=False) != json.dumps(document, sort_keys=True, allow_nan=False):
            raise NativeLinkError('typed query source differs from the decoded target inventory')
    except (ValueError, TypeError) as error:
        raise NativeLinkError('projection source JSON is not unambiguous and finite') from error
    if report.get('command') != 'query' or report.get('view') != 'native_links':
        raise NativeLinkError('projection evidence is not the actual typed native-links query')
    result = verify_native_links(report, document, actual_sha256)
    if document_path.read_bytes() != raw:
        raise NativeLinkError('projection source bytes changed during link verification')
    return result
