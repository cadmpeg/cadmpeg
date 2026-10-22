# SPDX-License-Identifier: Apache-2.0
"""Removed unknown records need preserved bytes and actual surviving reads."""
from dataclasses import dataclass
from collections import Counter

from decode_nonregression_retained import images, RetainedEvidenceError, _u64


@dataclass(frozen=True)
class UnknownReadWitness:
    namespace: str
    record_id: str
    stream: str
    start: int
    end: int
    readers: tuple[str, ...]


def _records(document):
    result = Counter()
    unknown_ids = set()
    for rows in document.get('model', {}).values():
        for record in rows:
            result[record['id']] += 1
    for arenas in document.get('native', {}).values():
        for arena, rows in arenas.items():
            for record in rows:
                result[record['id']] += 1
                if arena == 'unknowns':
                    unknown_ids.add(record['id'])
    return {identity: count for identity, count in result.items() if identity not in unknown_ids}


def _unknowns(document):
    result = {}
    for namespace, arenas in document.get('native', {}).items():
        rows = {}
        for record in arenas.get('unknowns', []):
            identity = record.get('id')
            if not isinstance(identity, str) or not identity or identity in rows:
                raise RetainedEvidenceError('unknown records need unique actual identities')
            rows[identity] = record
        result[namespace] = rows
    return result


def _reads(layout):
    if not isinstance(layout, dict) or set(layout) != {'stream', 'offset', 'byte_len', 'reads', 'pointers', 'payloads'}:
        raise RetainedEvidenceError('read witness requires a complete closed RecordLayout')
    if not isinstance(layout['stream'], str) or not layout['stream']:
        raise RetainedEvidenceError('record layout requires an actual named stream')
    _u64(_u64(layout['offset']) + _u64(layout['byte_len']))
    if not isinstance(layout['pointers'], dict) or not isinstance(layout['payloads'], dict):
        raise RetainedEvidenceError('record pointers/payloads must be maps')
    for name, value in layout['pointers'].items():
        if not isinstance(name, str):
            raise RetainedEvidenceError('record pointer key is not a string')
        _u64(value)
    for name, value in layout['payloads'].items():
        if not isinstance(name, str) or not isinstance(value, str):
            raise RetainedEvidenceError('record payload is not hexadecimal bytes')
        if len(value) % 2 or any(character not in '0123456789abcdefABCDEF' for character in value):
            raise RetainedEvidenceError('record payload is not hexadecimal bytes')
    if not isinstance(layout['reads'], dict):
        raise RetainedEvidenceError('record semantic reads must be stream-keyed')
    for stream, ranges in layout['reads'].items():
        if not isinstance(stream, str) or not stream or not isinstance(ranges, list) or not ranges:
            raise RetainedEvidenceError('record semantic reads require named nonempty ranges')
        previous_end = None
        for interval in ranges:
            if not isinstance(interval, dict) or set(interval) != {'start', 'end'}:
                raise RetainedEvidenceError('record semantic read has unknown or missing fields')
            start, end = _u64(interval['start']), _u64(interval['end'])
            if start >= end or previous_end is not None and start <= previous_end:
                raise RetainedEvidenceError('record semantic reads must be sorted and coalesced')
            previous_end = end
            yield stream, start, end


def unknown_read_witnesses(base, head, before_sidecar, after_sidecar):
    """Prove base unknowns with unchanged retained images and full reads.

    Same-image retention binds the source stream and bytes across builds. This
    bounded proof cannot authenticate renamed streams or images absent at head.
    Read owners must be unique, surviving non-unknown native or neutral records.
    """
    before, after = images(before_sidecar), images(after_sidecar)
    old_unknowns = _unknowns(base)
    owner_counts = _records(head)
    layouts = after_sidecar['fidelity'].get('record_layout', {})
    if not isinstance(layouts, dict):
        raise RetainedEvidenceError('record_layout must be an identity-keyed map')
    intervals = {}
    for identity, layout in layouts.items():
        ranges = list(_reads(layout))
        if owner_counts.get(identity) == 1:
            for stream, start, end in ranges:
                intervals.setdefault(stream, []).append((start, end, identity))
    for ranges in intervals.values():
        ranges.sort()
    witnesses = []
    for namespace, records in old_unknowns.items():
        for identity in records:
            source, target = before.get(identity), after.get(identity)
            if source is None or source != target or not source.available or not source.stream or source.byte_len == 0:
                continue
            # An identity reused across namespaces has no unambiguous image owner.
            if sum(identity in rows for rows in old_unknowns.values()) != 1:
                continue
            cursor = source.offset
            stop = cursor + source.byte_len
            readers = set()
            for start, end, owner in intervals.get(source.stream, []):
                if end <= cursor:
                    continue
                if start > cursor or cursor >= stop:
                    break
                readers.add(owner)
                cursor = max(cursor, end)
            if cursor >= stop:
                witnesses.append(UnknownReadWitness(namespace, identity, source.stream,
                                                   source.offset, stop, tuple(sorted(readers))))
    return witnesses


def discharge_unknown_findings(findings, base, head, witnesses):
    """Discharge only exact removed identities and count loss only when every base unknown is covered."""
    proven = {(row.namespace, row.record_id) for row in witnesses}
    old, new = _unknowns(base), _unknowns(head)
    complete = {namespace for namespace, records in old.items()
                if records.keys() - new.get(namespace, {}).keys()
                and all((namespace, identity) in proven for identity in records)}
    result = []
    for finding in findings:
        path = finding.path
        if (finding.code == 'native-identity' and len(path) == 4 and path[0] == 'native'
                and path[2] == 'unknowns' and (path[1], path[3]) in proven):
            continue
        if (finding.code == 'rule-b-evidence-required' and len(path) == 3 and path[0] == 'native'
                and path[2] == 'unknowns' and path[1] in complete):
            continue
        result.append(finding)
    return result
