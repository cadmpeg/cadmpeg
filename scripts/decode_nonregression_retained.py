# SPDX-License-Identifier: Apache-2.0
"""Rule B retained-image evidence from actual SHA-bound decode sidecars.

This slice compares retained source facts; it gives no semantic-read credit.
A removed image still needs a real consumption/Rule B proof. No stream alias,
layout window, payload copy or retained bytes are treated as typed reads.
"""
from dataclasses import dataclass
import base64
import hashlib
import json
from pathlib import Path
import re
from typing import Mapping


class RetainedEvidenceError(ValueError):
    """A bound sidecar or retained image cannot supply the required evidence."""


@dataclass(frozen=True)
class Image:
    stream: str
    offset: int
    byte_len: int
    sha256: str
    available: bool


@dataclass(frozen=True)
class RetainedFinding:
    code: str
    path: tuple[str, ...]
    message: str


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise RetainedEvidenceError('duplicate JSON object key in source evidence')
        result[key] = value
    return result


def sidecar_path(document: Path) -> Path:
    # Same path rule as cadmpeg_ir::decode_sidecar_path. Collector output has a
    # normal .json filename; no extension heuristic is used for source streams.
    name = document.name[:-5] if document.name.endswith('.json') else document.name
    return document.with_name(name + '.fidelity.json')


def load_bound_sidecar(document: Path, ir: Mapping) -> dict:
    raw = document.read_bytes()
    value = json.loads(sidecar_path(document).read_text(), object_pairs_hook=_object)
    if not isinstance(value, dict) or set(value) != {'ir_version', 'ir_sha256', 'report', 'fidelity'}:
        raise RetainedEvidenceError('decode sidecar has missing or unknown root fields')
    if not isinstance(value['ir_version'], str) or not value['ir_version'] or value['ir_version'] != ir.get('ir_version') or value['ir_sha256'] != hashlib.sha256(raw).hexdigest():
        raise RetainedEvidenceError('decode sidecar is not bound to the exact producer CADIR bytes/version')
    if not isinstance(value['report'], dict) or not isinstance(value['fidelity'], dict):
        raise RetainedEvidenceError('decode sidecar report/fidelity must be objects')
    return value


def _u64(value):
    if type(value) is not int or not 0 <= value <= 2**64 - 1:
        raise RetainedEvidenceError('source extent is not an admitted u64')
    return value


def images(sidecar: Mapping) -> dict[str, Image]:
    records = sidecar['fidelity'].get('retained_records', {})
    if not isinstance(records, dict):
        raise RetainedEvidenceError('retained_records must be an identity-keyed map')
    result = {}
    for identity, record in records.items():
        if not isinstance(identity, str) or not identity or not isinstance(record, dict):
            raise RetainedEvidenceError('retained source record has no actual identity')
        if not {'stream', 'offset', 'bytes'} <= set(record) or set(record) - {'stream', 'offset', 'bytes', 'untyped'}:
            raise RetainedEvidenceError('retained source record has missing or unknown fields')
        if not isinstance(record['stream'], str):
            raise RetainedEvidenceError('retained source stream is not a string')
        if 'untyped' in record and (not isinstance(record['untyped'], list)
                or any(not isinstance(value, str) or value not in {'model', 'design'} for value in record['untyped'])
                or len(set(record['untyped'])) != len(record['untyped'])):
            raise RetainedEvidenceError('retained source classes are malformed')
        source = record['bytes']
        if not isinstance(source, dict):
            raise RetainedEvidenceError('retained bytes are not a typed retention variant')
        if source.get('retention') == 'inline' and set(source) == {'retention', 'data'}:
            if not isinstance(source['data'], str):
                raise RetainedEvidenceError('retained inline bytes are not base64 text')
            try:
                data = base64.b64decode(source['data'], validate=True)
            except (ValueError, TypeError) as error:
                raise RetainedEvidenceError('retained inline bytes are not valid base64') from error
            if base64.b64encode(data).decode() != source['data']:
                raise RetainedEvidenceError('retained inline bytes have a noncanonical encoding')
            length, digest, available = len(data), hashlib.sha256(data).hexdigest(), True
        elif source.get('retention') == 'digest' and set(source) == {'retention', 'byte_len', 'sha256'}:
            length, digest, available = _u64(source['byte_len']), source['sha256'], False
            if not isinstance(digest, str) or re.fullmatch('[0-9a-f]{64}', digest) is None:
                raise RetainedEvidenceError('retained digest is not a lowercase SHA256')
        else:
            raise RetainedEvidenceError('unknown or malformed retained-byte variant')
        offset = _u64(record['offset'])
        _u64(offset + length)
        result[identity] = Image(record['stream'], offset, length, digest, available)
    return result


def compare_retained(before: Mapping, after: Mapping) -> tuple[list[RetainedFinding], int]:
    """Preserve exact source images; missing images require further real proof.

    Digest-only head retention does not preserve available base bytes. Actual
    typed read coverage may justify removing retention, but is deliberately
    not inferred here; no unknown native count/identity check is suppressed.
    """
    old, new = images(before), images(after)
    findings = []
    for identity, source in old.items():
        target = new.get(identity)
        path = ('fidelity', 'retained_records', identity)
        if target is None:
            findings.append(RetainedFinding('retained-consumption-proof-required', path,
                'base source image is absent; actual semantic-read/Rule B preservation proof is required'))
        elif (source.stream, source.offset) != (target.stream, target.offset):
            findings.append(RetainedFinding('retained-source-location', path, 'source image owner/offset changed without an explicit source mapping proof'))
        elif (source.byte_len, source.sha256) != (target.byte_len, target.sha256):
            findings.append(RetainedFinding('retained-source-bytes', path, 'source image length or SHA256 changed'))
        elif source.available and not target.available:
            findings.append(RetainedFinding('retained-source-unavailable', path, 'available base source bytes became digest-only without a semantic-read proof'))
    return findings, len(old)
