# SPDX-License-Identifier: Apache-2.0
"""Rule B preservation of the base failure records that §4.5 rule 1 removes (#219).

Each removed base failure record must reappear as the head form its kind
requires: a given-up part, a B1 span, a dropped face or an identity loss.
"""

from __future__ import annotations

import base64
import hashlib
import re
from typing import Mapping


UNREADABLE = 'record.unreadable'
UNDECODED = 'source.bytes-undecoded'
IDENTITY_CONFLICT = 'rse.segment-identity-conflict'
OFFSET_KEYS = ('source_offset', 'offset', 'carrier_offset', 'inflated_offset')
LENGTH_KEYS = ('byte_len', 'carrier_len', 'length')
DIGEST_KEYS = ('sha256', 'carrier_sha256')
FAILURE_ROW_KEYS = {
    ('f3d', 'asm_history_records'): 'framing_error',
    ('inventor', 'active_carrier'): 'detail',
    ('inventor', 'protein'): 'detail',
    ('inventor', 'ufrx'): 'detail',
    ('inventor', 'segment_bulk'): 'record_detail',
    ('fcstd', 'properties'): 'payload_error',
    ('rhino', 'setting_records'): 'parse_error',
}
IDENTITY_ISSUE = re.compile(r'inventor:rse:structural-issue#segment-')
ERROR_SETTING = re.compile(r'rhino:document:setting#error-')


def loss_code(loss: Mapping) -> str:
    code = loss.get('code')
    if isinstance(code, Mapping):
        return str(code.get('code', ''))
    return str(code or '')


def losses(sidecar: Mapping | None) -> list[Mapping]:
    rows = (sidecar or {}).get('report', {}).get('losses', [])
    return [row for row in rows if isinstance(row, Mapping)]


def is_failure_row(namespace: str, arena: str, record: Mapping) -> bool:
    """A base row inside a kept arena that records a decode failure."""
    if namespace == 'rhino' and arena == 'setting_records':
        return bool(ERROR_SETTING.match(str(record.get('id', ''))))
    if namespace == 'f3d' and arena == 'asm_history_records':
        framing = record.get('framing')
        return record.get('framing_error') is not None or (
            isinstance(framing, Mapping) and framing.get('kind') == 'opaque')
    key = FAILURE_ROW_KEYS.get((namespace, arena))
    return key is not None and record.get(key) is not None


def source_extent(record: Mapping) -> tuple[int | None, bytes | None, int | None, str | None]:
    """Offset, bytes, length and digest a base failure record states."""
    offset = next((record[key] for key in OFFSET_KEYS if isinstance(record.get(key), int)), None)
    data = None
    if isinstance(record.get('bytes'), str):
        try:
            data = bytes.fromhex(record['bytes'])
        except ValueError:
            data = None
    length = next((record[key] for key in LENGTH_KEYS if isinstance(record.get(key), int)), None)
    digest = next((record[key] for key in DIGEST_KEYS if isinstance(record.get(key), str)), None)
    if data is not None:
        length, digest = len(data), hashlib.sha256(data).hexdigest()
    return offset, data, length, digest


def retained_image(sidecar: Mapping | None, identity: str) -> tuple[str, int, int, str] | None:
    record = (sidecar or {}).get('fidelity', {}).get('retained_records', {}).get(identity)
    if not isinstance(record, Mapping):
        return None
    source = record.get('bytes', {})
    if source.get('retention') == 'inline' and isinstance(source.get('data'), str):
        data = base64.b64decode(source['data'])
        return record.get('stream'), record.get('offset'), len(data), hashlib.sha256(data).hexdigest()
    if source.get('retention') == 'digest':
        return record.get('stream'), record.get('offset'), source.get('byte_len'), source.get('sha256')
    return None


def unknown_ids(head: Mapping, namespace: str) -> list[str]:
    native = head.get('native', {}).get(namespace, {})
    return [row['id'] for row in native.get('unknowns', []) if isinstance(row, Mapping) and isinstance(row.get('id'), str)]


def names(message: str, offset: int | None, length: int | None) -> bool:
    return ((offset is None or f'offset={offset}' in message)
            and (length is None or f'length={length}' in message))


def given_up(namespace: str, record: Mapping, head: Mapping, sidecar: Mapping | None) -> str | None:
    """None when the head gives the record's source up as one stub and one loss."""
    offset, _, length, digest = source_extent(record)
    stubs = []
    for identity in unknown_ids(head, namespace):
        image = retained_image(sidecar, identity)
        if image is None:
            continue
        _, image_offset, image_length, image_digest = image
        if offset is not None and image_offset != offset:
            continue
        if length is not None and (image_length, image_digest) != (length, digest if digest else image_digest):
            continue
        stubs.append((identity, image))
    if len(stubs) != 1:
        return f'{len(stubs)} unknowns stubs retain the base failure record source (one is required)'
    identity, (stream, image_offset, image_length, _) = stubs[0]
    charged = [loss for loss in losses(sidecar) if loss_code(loss) == UNREADABLE
               and f'stream={stream}' in str(loss.get('message', ''))
               and names(str(loss.get('message', '')), image_offset, image_length)]
    if len(charged) != 1:
        return f'{len(charged)} {UNREADABLE} losses name stub {identity} (one is required)'
    return None


def b1_span(record: Mapping, sidecar: Mapping | None) -> str | None:
    offset, _, length, _ = source_extent(record)
    named = [loss for loss in losses(sidecar) if loss_code(loss) == UNDECODED
             and names(str(loss.get('message', '')), offset, length)]
    if not named:
        return f'no {UNDECODED} loss names offset {offset} and length {length}'
    return None


def identity_conflict(record: Mapping, sidecar: Mapping | None) -> str | None:
    """The head names the segment whose metadata and bulk records the base finding compared."""
    token = str(record.get('scope', '')).removeprefix('segment:')
    for loss in losses(sidecar):
        if loss_code(loss) == IDENTITY_CONFLICT and token and token in str(loss.get('message', '')):
            return None
    return f'no {IDENTITY_CONFLICT} loss names segment {token!r}'


def face_rejection(record: Mapping, head: Mapping, sidecar: Mapping | None) -> str | None:
    face = record.get('face')
    if not isinstance(face, str):
        return 'the base rejection names no face'
    if any(row.get('id') == face for row in head.get('model', {}).get('faces', [])):
        return f'head keeps rejected face {face}'
    if not any(face in str(loss.get('message', '')) for loss in losses(sidecar)):
        return f'no loss names rejected face {face}'
    return None


def preservation_failure(namespace: str, arena: str, record: Mapping, base: Mapping, head: Mapping,
                         sidecar: Mapping | None) -> str | None:
    """None when the head preserves one removed base failure record in its required form."""
    if (namespace, arena) == ('nx', 'parasolid_deltas_residual_spans'):
        return b1_span(record, sidecar)
    if (namespace, arena) == ('creo', 'brep_face_admission_rejections'):
        return face_rejection(record, head, sidecar)
    if (namespace, arena) == ('inventor', 'structural_issues') and IDENTITY_ISSUE.match(str(record.get('id', ''))):
        return identity_conflict(record, sidecar)
    return given_up(namespace, record, head, sidecar)
