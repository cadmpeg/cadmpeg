# SPDX-License-Identifier: Apache-2.0
"""Base/head decode collection and comparison for the §4.4 gate (#219).

Each build is a `cadmpeg` executable. The base build comes from
`scripts/base-build.sh`; the head build is the release binary of the checkout.
Schema paths come from each build's own `query schema types` output.
"""

from __future__ import annotations

from dataclasses import dataclass
import csv
import json
from pathlib import Path, PurePosixPath
import subprocess
from typing import Any, Iterable, Mapping

from decode_nonregression_failures import is_failure_row, loss_code, losses, preservation_failure
from decode_nonregression_resolvers import NO_RESOLVER, Resolvers, check_1b, resolve, section_8_deletions
from decode_nonregression_retained import RetainedEvidenceError, compare_retained
from decode_nonregression_schema import GeneratedArena, GeneratedSchemaError, SchemaPaths
from decode_nonregression_schema_bindings import generated_arena
from decode_nonregression_unknown_reads import discharge_unknown_findings, unknown_read_witnesses
from decode_nonregression_variant_guard import UNCERTAIN_TAGS, compare_uncertain_totals


JsonPath = tuple[str, ...]
APPENDIX_B = Path(__file__).with_name('rule-b-fields.tsv')
COMMAND_TIMEOUT_SECONDS = 120
UNREADABLE_LOSS = 'record.unreadable'
DELETIONS = section_8_deletions()


@dataclass(frozen=True)
class Move:
    """One Appendix B `move-layout` or `move-payload` row at an exact native path."""

    namespace: str
    arena: str
    field_path: JsonPath
    store: str


def appendix_b_moves(path: Path = APPENDIX_B) -> tuple[Move, ...]:
    """Read the moved native paths; rows marked `(unpopulated)` name no golden path."""
    stores = {'move-layout': 'pointers', 'move-payload': 'payloads'}
    result = set()
    with path.open(newline='') as stream:
        for row in csv.DictReader(stream, delimiter='\t'):
            store = stores.get(row['class'])
            if store is None:
                continue
            for spelling in row['json_path'].split(';'):
                parts = tuple(spelling.strip().split('/'))
                if len(parts) < 5 or parts[0] != 'native' or parts[3] != '#':
                    continue
                result.add(Move(parts[1], parts[2], parts[4:], store))
    return tuple(sorted(result, key=lambda move: (move.namespace, move.arena, move.field_path, move.store)))


class BuildCommandError(ValueError):
    """A build's `cadmpeg` command exited with a failure."""


class Build:
    """One `cadmpeg` executable and its compiled IR schema."""

    def __init__(self, role: str, binary: Path):
        self.role = role
        self.binary = binary
        self._queries: dict[str, dict] = {}

    def run(self, arguments: list[str]) -> str:
        result = subprocess.run([str(self.binary), *arguments], capture_output=True, text=True,
                                timeout=COMMAND_TIMEOUT_SECONDS, check=False)
        if result.returncode:
            raise BuildCommandError(f'{self.role} `cadmpeg {" ".join(arguments)}` exited '
                                       f'{result.returncode}: {result.stderr.strip()[:400]}')
        return result.stdout

    def query(self, arena: str) -> dict:
        if arena not in self._queries:
            self._queries[arena] = json.loads(self.run(['query', 'schema', 'types', 'model.' + arena, '--json']))
        return self._queries[arena]

    def arena(self, arena: str) -> GeneratedArena:
        query = self.query(arena)
        # A build without compiler identity metadata has no Dimension<Q> type.
        if 'type_metadata' in query.get('payload', {}):
            return generated_arena(query)
        return GeneratedArena(query)


def dump(build: Build, fixture: Path, output: Path) -> tuple[dict, dict | None]:
    """Decode one fixture; return the document and its decode sidecar."""
    output.unlink(missing_ok=True)
    sidecar = output.with_name(output.name[:-len('.json')] + '.fidelity.json')
    sidecar.unlink(missing_ok=True)
    try:
        result = subprocess.run([str(build.binary), 'dump', str(fixture), '-o', str(output)],
                                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
                                timeout=COMMAND_TIMEOUT_SECONDS, check=False)
    except subprocess.TimeoutExpired:
        return {'decode_error': 'decode timed out'}, None
    if result.returncode:
        return {'decode_error': f'cadmpeg dump exited {result.returncode}: {result.stderr.strip()[:400]}'}, None
    document = json.loads(output.read_text())
    if not isinstance(document, dict) or not isinstance(document.get('ir_version'), str):
        raise ValueError(f'{build.role} dump did not write a CADIR document: {fixture}')
    side = json.loads(sidecar.read_text()) if sidecar.is_file() else None
    return document, side


ELISION_KEY = '__elided'  # cadmpeg_test_support::golden::NATIVE_ELISION_KEY


def effective_values(build: Build, document: Mapping, scratch: Path) -> dict | None:
    """The head resolver values of one document, or None when the view refuses it."""
    if 'decode_error' in document:
        return None
    admitted = dict(document)
    if ELISION_KEY in admitted.get('native', {}):
        admitted['native'] = {}
    source = scratch / 'effective-input.json'
    source.write_text(json.dumps(admitted))
    try:
        return json.loads(build.run(['query', 'effective', str(source), '--json']))['payload']
    except (BuildCommandError, KeyError, ValueError):
        return None


def golden_parts(golden: Mapping) -> tuple[dict, dict | None]:
    """Split a committed decode golden into its document and sidecar-shaped report."""
    decode = golden.get('decode', golden)
    if 'ir' not in decode:
        return {'decode_error': str(decode.get('decode_error', 'golden holds no decode'))}, None
    return decode['ir'], {'report': decode.get('report', {}), 'fidelity': decode.get('source_fidelity', {})}


def native_view(document: Mapping) -> tuple[dict, dict | None]:
    """Native records and, for an elided snapshot, its per-arena record counts."""
    native = document.get('native', {})
    if ELISION_KEY not in native:
        return native, None
    return native.get('__arena_values', {}), native.get('__arena_counts', {})


def elided_count_findings(comparator: Any, before: Mapping | None, after: Mapping | None,
                          base_native: Mapping, head_native: Mapping) -> list:
    """Compare per-arena counts where a snapshot pins only a count."""
    def counts(pinned: Mapping | None, native: Mapping) -> dict:
        if pinned is not None:
            return {(namespace, arena): count for namespace, arenas in pinned.items()
                    for arena, count in arenas.items()}
        return {(namespace, arena): len(rows) for namespace, arenas in native.items()
                for arena, rows in arenas.items() if isinstance(rows, list)}
    if before is None and after is None:
        return []
    old, new = counts(before, base_native), counts(after, head_native)
    findings = []
    for (namespace, arena), count in sorted(old.items()):
        if (namespace, arena) in comparator.REMOVED_NATIVE_ARENAS or arena == 'unknowns':
            continue
        if new.get((namespace, arena), 0) < count:
            findings.append(comparator.Finding('native-count', ('native', namespace, arena),
                            f'{count} records became {new.get((namespace, arena), 0)}'))
    return findings


def is_decode_golden(path: str) -> bool:
    parts = PurePosixPath(path).parts
    if len(parts) < 5 or parts[0] != 'crates' or parts[2:4] != ('tests', 'golden') or not path.endswith('.json'):
        return False
    tail = parts[4:]
    return tail[0] == 'decode' or (len(tail) == 1 and parts[1] == 'cadmpeg-codec-nx')


def golden_key(path: str) -> tuple[str, ...]:
    parts = tuple(part for part in PurePosixPath(path).parts if part != 'damaged')
    return parts[:-1] + (PurePosixPath(parts[-1]).stem,)


def loss_codes(sidecar: Mapping | None) -> set[str]:
    return {loss_code(loss) for loss in losses(sidecar)}


def layout_evidence(comparator: Any, base: Mapping, head_sidecar: Mapping | None, moves: tuple[Move, ...]) -> Any:
    """Bind every base native record of an Appendix B arena to that arena's moved paths."""
    native_types = {}
    layout_moves = []
    arenas = {(move.namespace, move.arena) for move in moves}
    for namespace, rows_by_arena in base.get('native', {}).items():
        for arena, rows in rows_by_arena.items():
            if (namespace, arena) not in arenas or not isinstance(rows, list):
                continue
            for record in rows:
                if isinstance(record, dict) and isinstance(record.get('id'), str):
                    native_types[(namespace, arena, record['id'])] = 'appendix-b'
    for move in moves:
        if '#' in move.field_path:
            continue
        layout_moves.append(comparator.LayoutMove(move.namespace, move.arena, 'appendix-b', move.field_path,
                                                  '.'.join(move.field_path), move.store))
    fidelity = (head_sidecar or {}).get('fidelity', {})
    return comparator.Evidence(native_types=native_types, layout_moves=tuple(layout_moves), fidelity=fidelity)


def classify(arena: GeneratedArena, record: Any) -> Any:
    """Schema paths of one record; a record its build's schema rejects is classified leniently."""
    try:
        return arena.paths([record])
    except GeneratedSchemaError:
        return arena.paths([record], lenient=True)


def uncertain(rows: list, arena: GeneratedArena, name: str) -> set[tuple[str, str]]:
    result = set()
    for record in rows:
        paths = classify(arena, record)

        def walk(value: Any, path: JsonPath) -> bool:
            if path in paths.variant_tags and isinstance(value, str) and value in UNCERTAIN_TAGS:
                return True
            if isinstance(value, dict):
                return any(walk(child, path + (key,)) for key, child in value.items())
            if isinstance(value, list):
                return any(walk(child, path + ('#',)) for child in value)
            return False

        if walk(record, (name, '#')):
            result.add((name, record['id']))
    return result


def resolve_findings(comparator: Any, findings: list, base: Mapping, head: Mapping,
                     effective: Mapping | None) -> list:
    """Replace each exempt-path finding by its resolver comparison."""
    resolvers = Resolvers(base, head, effective) if effective is not None else None
    empty = SchemaPaths(frozenset(), frozenset())
    result = []
    for finding in findings:
        if finding.code != 'resolver-evidence-required':
            result.append(finding)
            continue
        resolver = comparator.exemption(finding.path[1:], empty)
        if resolver in NO_RESOLVER or (resolver is not None and resolve(resolvers, finding.path, resolver)):
            continue
        result.append(comparator.Finding('resolver-mismatch', finding.path,
                                         f'{resolver} does not give the base value'))
    return result


def failure_findings(comparator: Any, findings: list, base: Mapping, head: Mapping,
                     head_sidecar: Mapping | None) -> list:
    """Replace the native findings of each removed base failure record by its preservation proof."""
    removed = set()
    proofs = []
    for namespace, arenas in base.get('native', {}).items():
        if not isinstance(arenas, Mapping):
            continue
        for arena, rows in arenas.items():
            if (namespace, arena) in comparator.GONE_NATIVE_ARENAS:
                removed.add(('native', namespace, arena))
                continue
            whole = (namespace, arena) in comparator.FAILURE_NATIVE_ARENAS
            failures = 0
            for record in rows if isinstance(rows, list) else ():
                if not isinstance(record, Mapping) or not (whole or is_failure_row(namespace, arena, record)):
                    continue
                failures += 1
                removed.add(('native', namespace, arena, record.get('id')))
                failure = preservation_failure(namespace, arena, record, base, head, head_sidecar)
                if failure is not None:
                    proofs.append(comparator.Finding('failure-record-preservation',
                                                     ('native', namespace, arena, str(record.get('id'))), failure))
            head_rows = head.get('native', {}).get(namespace, {}).get(arena, [])
            if whole or (failures and len(head_rows) >= len(rows) - failures):
                removed.add(('native', namespace, arena))
    kept = [finding for finding in findings
            if not (finding.path[:4] in removed or finding.path[:3] in removed)]
    return kept + proofs


def ordinal_order_findings(comparator: Any, base: Mapping, head: Mapping) -> list:
    """Check 7: features present at both keep their base ordinal order."""
    old = {row['id']: row.get('ordinal') for row in base.get('model', {}).get('features', []) if 'id' in row}
    new = {row['id']: row.get('ordinal') for row in head.get('model', {}).get('features', []) if 'id' in row}
    common = sorted((ordinal, identity) for identity, ordinal in old.items()
                    if identity in new and isinstance(ordinal, int) and isinstance(new[identity], int))
    findings = []
    for (_, earlier), (_, later) in zip(common, common[1:]):
        if new[earlier] >= new[later]:
            findings.append(comparator.Finding('ordinal-order', ('model', 'features', later),
                                               f'head orders {later} before or with {earlier}'))
    return findings


def strings(value: Any) -> Iterable[str]:
    if isinstance(value, str):
        yield value
    elif isinstance(value, Mapping):
        for child in value.values():
            yield from strings(child)
    elif isinstance(value, list):
        for child in value:
            yield from strings(child)


def span_identities(document: Mapping, sidecar: Mapping | None) -> set[str]:
    """Identities of B1 spans: unknowns stubs and retained records with untyped classes."""
    result = {identity for identity, record in (sidecar or {}).get('fidelity', {}).get('retained_records', {}).items()
              if isinstance(record, Mapping) and record.get('untyped')}
    for arenas in document.get('native', {}).values():
        if isinstance(arenas, Mapping):
            result |= {row['id'] for row in arenas.get('unknowns', []) if isinstance(row, Mapping) and 'id' in row}
    return result


def span_reference_findings(comparator: Any, base: Mapping, head: Mapping, head_sidecar: Mapping | None) -> list:
    """§3.4: no head reference names a B1 span that no base reference named."""
    spans = span_identities(head, head_sidecar)
    if not spans:
        return []
    def referenced(document: Mapping) -> set[str]:
        found = set()
        for section in ('model', 'native'):
            for arenas in (document.get(section, {}),):
                for value in arenas.values():
                    rows = value.values() if section == 'native' and isinstance(value, Mapping) else [value]
                    for arena_rows in rows:
                        for row in arena_rows if isinstance(arena_rows, list) else ():
                            if isinstance(row, Mapping):
                                found |= {text for key, child in row.items() if key != 'id' for text in strings(child)}
        return found
    named = referenced(head) & spans
    return [comparator.Finding('b1-span-reference', ('native', identity), 'a head record references a B1 span no base record named')
            for identity in sorted(named - referenced(base))]


def model_arenas(build: Build) -> list[str]:
    listing = json.loads(build.run(['query', 'schema', '--json']))['payload']
    return sorted(name.removeprefix('model.') for name in listing if name.startswith('model.'))


def tagged_variants(arena: GeneratedArena) -> dict[tuple[str, str], dict]:
    """(definition, tag value) of every object branch of a tagged union, with its branch schema."""
    result = {}
    for name, definition in arena.defs.items():
        branches = definition.get('oneOf', definition.get('anyOf', []))
        for branch in branches:
            branch = arena._resolve(branch) if isinstance(branch, dict) else branch
            if not isinstance(branch, dict) or branch.get('type') != 'object':
                continue
            for key, child in branch.get('properties', {}).items():
                tag = arena._string_const(child) if isinstance(child, dict) else None
                if tag is not None and key in branch.get('required', []):
                    result[(name, tag)] = {field: value for field, value in branch['properties'].items() if field != key}
                    break
    return result


def weak_field(arena: GeneratedArena, schema: Any) -> bool:
    """A string, a map or a byte field."""
    if not isinstance(schema, dict):
        return False
    schema = arena._resolve(schema)
    choices = [arena._resolve(choice) for choice in schema.get('anyOf', schema.get('oneOf', [schema]))]
    choices = [choice for choice in choices if choice.get('type') != 'null']
    def weak(choice: dict) -> bool:
        kind = choice.get('type')
        if kind == 'string' and not {'const', 'enum', 'pattern', 'format'} & set(choice):
            return True
        if kind == 'object' and 'additionalProperties' in choice and not choice.get('properties'):
            return True
        items = choice.get('items')
        return kind == 'array' and isinstance(items, dict) and items.get('type') == 'integer' and items.get('maximum') == 255
    return bool(choices) and all(weak(choice) for choice in choices)


# #209 §2.1 names `ExpressionToken::Text` as an IR shape of this work.
NAMED_VARIANTS = frozenset({('ExpressionToken', 'text')})


def variant_guard_findings(comparator: Any, base_build: Build, head_build: Build) -> list:
    """§3.4: no variant added since base has only string, map or byte fields."""
    findings = []
    base_arenas = set(model_arenas(base_build))
    seen = set()
    for name in model_arenas(head_build):
        head_arena = head_build.arena(name)
        base_variants = tagged_variants(base_build.arena(name)) if name in base_arenas else {}
        for (definition, tag), fields in sorted(tagged_variants(head_arena).items()):
            if (definition, tag) in base_variants or (definition, tag) in seen or (definition, tag) in NAMED_VARIANTS:
                continue
            seen.add((definition, tag))
            if fields and all(weak_field(head_arena, field) for field in fields.values()):
                findings.append(comparator.Finding('weak-variant', ('schema', definition, tag),
                                                   f'added variant has only string, map or byte fields: {sorted(fields)}'))
    return findings


def compare_pair(comparator: Any, base: Mapping, head: Mapping, base_sidecar: Mapping | None,
                 head_sidecar: Mapping | None, base_build: Build, head_build: Build,
                 moves: tuple[Move, ...], effective: Mapping | None = None) -> list:
    """Every §4.4 comparison of one base decode against its head decode."""
    Finding = comparator.Finding
    if 'decode_error' in base:
        return []
    if 'decode_error' in head:
        return [Finding('head-decode-error', (), str(head['decode_error']))]
    findings: list = []
    before, after = base.get('model', {}), head.get('model', {})
    for name, rows in before.items():
        if not rows:
            continue
        old = comparator.index_records(rows, ('model', name), findings)
        new = comparator.index_records(after.get(name, []), ('model', name), findings)
        if len(new) < len(old):
            findings.append(Finding('neutral-count', ('model', name), f'{len(old)} records became {len(new)}'))
        try:
            old_arena, new_arena = base_build.arena(name), head_build.arena(name)
        except GeneratedSchemaError as error:
            findings.append(Finding('generated-schema-rejection', ('model', name), str(error).splitlines()[0]))
            continue
        for identity, record in old.items():
            matched = new.get(identity)
            if matched is None:
                findings.append(Finding('neutral-record', ('model', name, identity), 'base record has no head identity'))
                continue
            try:
                findings.extend(comparator.compare_neutral({name: [record]}, {name: [matched]},
                                classify(old_arena, record), classify(new_arena, matched)))
            except ValueError as error:
                findings.append(Finding('dimension-wrapper-shape', (name, identity), str(error).splitlines()[0]))
    findings = resolve_findings(comparator, findings, base, head, effective)
    findings.extend(ordinal_order_findings(comparator, base, head))
    findings.extend(comparator.Finding(row.code, row.path, row.message)
                    for row in check_1b(base, dict(head, native=native_view(head)[0]), DELETIONS))
    uncertain_before, uncertain_after = set(), set()
    for name in sorted(set(before) | set(after)):
        try:
            if before.get(name):
                uncertain_before |= uncertain(before[name], base_build.arena(name), name)
            if after.get(name):
                uncertain_after |= uncertain(after[name], head_build.arena(name), name)
        except GeneratedSchemaError as error:
            findings.append(Finding('variant-guard-schema-rejection', ('model', name), str(error).splitlines()[0]))
    findings.extend(Finding(row.code, row.path, row.message)
                    for row in compare_uncertain_totals(uncertain_before, uncertain_after))
    base_native, base_counts = native_view(base)
    head_native, head_counts = native_view(head)
    findings.extend(elided_count_findings(comparator, base_counts, head_counts, base_native, head_native))
    evidence = layout_evidence(comparator, {'native': base_native}, head_sidecar, moves)
    findings.extend(comparator.compare_native(base_native, head_native, evidence))
    findings = failure_findings(comparator, findings, {'native': base_native}, dict(head, native=head_native),
                                head_sidecar)
    findings.extend(span_reference_findings(comparator, dict(base, native=base_native),
                                            dict(head, native=head_native), head_sidecar))
    if base_sidecar is None or head_sidecar is None:
        findings.append(Finding('decode-sidecar-missing', (), 'a successful decode wrote no decode sidecar'))
        return findings
    try:
        retained, _ = compare_retained(base_sidecar, head_sidecar)
        findings.extend(Finding(row.code, row.path, row.message) for row in retained)
        base_view, head_view = dict(base, native=base_native), dict(head, native=head_native)
        witnesses = unknown_read_witnesses(base_view, head_view, base_sidecar, head_sidecar)
        findings = discharge_unknown_findings(findings, base_view, head_view, witnesses)
    except RetainedEvidenceError as error:
        findings.append(Finding('retained-evidence-rejection', ('fidelity',), str(error)))
    return findings


def new_golden_findings(comparator: Any, path: str, head_sidecar: Mapping | None) -> list:
    """A golden absent at base and outside `damaged/` charges no unreadable record."""
    if 'damaged' in PurePosixPath(path).parts:
        return []
    if any(code.endswith(UNREADABLE_LOSS) for code in loss_codes(head_sidecar)):
        return [comparator.Finding('new-golden-unreadable', (path,), f'a new golden charges {UNREADABLE_LOSS}')]
    return []
