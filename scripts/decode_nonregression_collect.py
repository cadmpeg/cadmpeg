# SPDX-License-Identifier: Apache-2.0
"""Bounded real-producer collection; incomplete obligations remain failures.

Producer manifests are local build attestations, not self-authenticating JSON or
CI evidence. Their immutable Git tree, retained build evidence and executable
hash are checked here. The baseline helper must additionally bind its compiled
sources and pristine baseline worktree. No caller-supplied schema exemptions
are accepted. Collection does not establish Appendix B or §3.4 completeness.
"""
from __future__ import annotations

from dataclasses import dataclass, field
import hashlib
import json
from pathlib import Path
import subprocess
from typing import Any

from decode_nonregression_d10 import PAIRS, verify_length_pairs
from decode_nonregression_inputs import dump_fixture
from decode_nonregression_schema_bindings import attach_external, generated_arena
from decode_nonregression_wire import verify_wire_rows


class CollectionError(ValueError):
    """A required producer, source or semantic proof is unavailable."""


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()


@dataclass
class Producer:
    repo: Path
    proof: dict
    commands: list[dict] = field(default_factory=list)
    schemas: dict[str, dict] = field(default_factory=dict)

    @classmethod
    def load(cls, repo: Path, path: Path, role: str) -> 'Producer':
        proof = json.loads(path.read_text())
        if proof.get('protocol') != 'cadmpeg-local-producer-v1' or proof.get('role') != role or proof.get('CI_evidence') is not False:
            raise CollectionError('expected a local source-verified producer attestation')
        if git(repo, 'rev-parse', proof['commit'] + '^{tree}') != proof['tree']:
            raise CollectionError('producer commit/tree disagree')
        evidence = json.loads(Path(proof['build_evidence']).read_text())
        if role == 'baseline' and (evidence.get('exit_code') != 0 or evidence.get('clean_stable_inputs') is not True):
            raise CollectionError('baseline build evidence is not a successful stable local build')
        if role == 'head' and (evidence.get('tree') != proof['tree'] or evidence.get('commit') != proof['commit']):
            raise CollectionError('head build evidence does not bind this committed source tree')
        # The retained schema proof also binds the exact executable; never use
        # metadata from a similarly named or subsequently rebuilt binary.
        schema_proof = json.loads(Path(proof['schema_proofs']).read_text())
        if schema_proof.get('binary_sha256') != proof['binary_sha256']:
            raise CollectionError('schema evidence belongs to another executable')
        result = cls(repo, proof)
        result.verify()
        return result

    def verify(self) -> None:
        if digest(Path(self.proof['binary'])) != self.proof['binary_sha256']:
            raise CollectionError('producer executable changed')
        if git(self.repo, 'rev-parse', self.proof['commit'] + '^{tree}') != self.proof['tree']:
            raise CollectionError('producer source identity changed')

    def run_json(self, arguments: list[str], input_value: Any = None) -> dict:
        self.verify()
        command = [self.proof['binary'], *arguments]
        result = subprocess.run(command, input=None if input_value is None else json.dumps(input_value),
                                text=True, capture_output=True, timeout=60, check=False)
        self.verify()
        self.commands.append({'command': command, 'commit': self.proof['commit'],
                              'binary_sha256': self.proof['binary_sha256'], 'exit_code': result.returncode,
                              'stdout_sha256': hashlib.sha256(result.stdout.encode()).hexdigest()})
        if result.returncode:
            raise CollectionError(f'producer command failed: {command}: {result.stderr}')
        value = json.loads(result.stdout)
        if not isinstance(value, dict):
            raise CollectionError('producer returned a non-object JSON value')
        return value

    def schema(self, arena: str, helper: 'BaselineHelper') -> dict:
        if arena not in self.schemas:
            query = self.run_json(['query', 'schema', 'types', 'model.' + arena, '--json'])
            if self.proof['role'] == 'baseline':
                query = attach_external(query, helper.metadata())
            generated_arena(query)  # Missing or ambiguous nominal identities fail.
            self.schemas[arena] = query
        return self.schemas[arena]


@dataclass
class BaselineHelper:
    producer: Producer
    manifest: dict
    commands: list[dict] = field(default_factory=list)
    _metadata: dict | None = None

    @classmethod
    def load(cls, producer: Producer, manifest_path: Path) -> 'BaselineHelper':
        result = cls(producer, json.loads(manifest_path.read_text()))
        result.verify()
        return result

    def verify(self) -> None:
        manifest = self.manifest
        if digest(Path(manifest['guard'])) != manifest['guard_sha256']:
            raise CollectionError('baseline helper source attestation changed')
        guard = json.loads(Path(manifest['guard']).read_text())
        proof = self.producer.proof
        if guard['base_commit'] != proof['commit'] or guard['base_tree'] != proof['tree']:
            raise CollectionError('baseline helper targets a different source tree')
        source = Path(guard['base_source'])
        if git(source, 'rev-parse', 'HEAD') != proof['commit'] or git(source, 'status', '--porcelain', '--untracked-files=all'):
            raise CollectionError('baseline helper source is not pristine')
        if git(source, 'rev-parse', '--path-format=absolute', '--git-common-dir') != git(self.producer.repo, 'rev-parse', '--path-format=absolute', '--git-common-dir'):
            raise CollectionError('baseline helper source belongs to another repository')
        if digest(source / 'Cargo.lock') != guard['base_lock_sha256']:
            raise CollectionError('baseline dependency lock changed')
        helper_source = Path(manifest['source'])
        for name, expected in guard['helper_sources_sha256'].items():
            if digest(helper_source / name) != expected:
                raise CollectionError('baseline helper source changed after build')
        if digest(helper_source / 'Cargo.toml') != guard['helper_manifest_sha256']:
            raise CollectionError('baseline helper manifest changed')
        if digest(Path(manifest['binary'])) != manifest['binary_sha256']:
            raise CollectionError('baseline helper executable changed')
        evidence = json.loads(Path(manifest['build_evidence']).read_text())
        if evidence.get('exit_code') != 0 or evidence.get('before') != evidence.get('after'):
            raise CollectionError('baseline helper successful stable local build evidence is absent')
        self.producer.verify()

    def run(self, arguments: list[str], document: dict | None = None) -> dict:
        self.verify()
        command = [self.manifest['binary'], *arguments]
        result = subprocess.run(command, input=None if document is None else json.dumps(document),
                                text=True, capture_output=True, timeout=60, check=False)
        self.verify()
        self.commands.append({'command': command, 'commit': self.producer.proof['commit'],
                              'binary_sha256': self.manifest['binary_sha256'], 'exit_code': result.returncode,
                              'stdout_sha256': hashlib.sha256(result.stdout.encode()).hexdigest(),
                              'input_sha256': None if document is None else hashlib.sha256(json.dumps(document).encode()).hexdigest()})
        if result.returncode:
            raise CollectionError(f'typed baseline helper failed: {result.stderr}')
        return json.loads(result.stdout)

    def metadata(self) -> dict:
        if self._metadata is None:
            self._metadata = self.run([])
        return self._metadata


def compare_collected(comparator: Any, base: dict, head: dict, baseline: Producer,
                      current: Producer, helper: BaselineHelper) -> list:
    """Compare actual dumps using build-specific, record-specific schema paths."""
    if 'decode_error' in base:
        # Base failures are retained in collection output, never counted as
        # successful decoder comparisons or resolver witnesses.
        return [comparator.Finding('baseline-decode-error', (), str(base['decode_error']))]
    if 'decode_error' in head:
        return [comparator.Finding('head-decode-error', (), str(head['decode_error']))]
    before, after = base.get('ir', base), head.get('ir', head)
    findings = []
    admitted = {}
    wire_ids = [record['id'] for arena in ('procedural_surfaces', 'procedural_curves')
                for record in before.get('model', {}).get(arena, [])]
    if wire_ids:
        queries = {arena: baseline.schema(arena, helper) for arena in ('procedural_surfaces', 'procedural_curves')
                   if before.get('model', {}).get(arena)}
        wire_report = helper.run(['--wire-rows', *wire_ids], before)
        if wire_report.get('requested') != wire_ids:
            raise CollectionError('typed wire helper did not cover the actual requested IDs')
        admitted = verify_wire_rows(wire_report, before, queries)
    for arena, rows in before.get('model', {}).items():
        if not rows:
            continue
        old_query, new_query = baseline.schema(arena, helper), current.schema(arena, helper)
        old_schema, new_schema = generated_arena(old_query), generated_arena(new_query)
        old_index = comparator.index_records(rows, ('model', arena), findings)
        new_rows = after.get('model', {}).get(arena, [])
        new_index = comparator.index_records(new_rows, ('model', arena), findings)
        if len(new_index) < len(old_index):
            findings.append(comparator.Finding('neutral-count', ('model', arena), 'record count fell without typed pairing proof'))
        for identity, record in old_index.items():
            matched = new_index.get(identity)
            if matched is None:
                findings.append(comparator.Finding('neutral-record', ('model', arena, identity), 'base record has no head identity'))
                continue
            try:
                admission = admitted.get((arena, identity))
                old_paths = old_schema.paths([record]) if admission is None else admission.paths
                if admission is not None and matched.get(admission.carrier_path[0]) != admission.carrier:
                    findings.append(comparator.Finding('carrier-reference-preservation',
                                    (arena, identity, *admission.carrier_path), 'actual typed carrier reference changed'))
                findings.extend(comparator.compare_neutral({arena: [record]}, {arena: [matched]},
                                old_paths, new_schema.paths([matched])))
            except ValueError as error:
                findings.append(comparator.Finding('generated-schema-rejection', (arena, identity),
                                                  str(error).splitlines()[0]))
    constraints = before.get('model', {}).get('sketch_constraints', [])
    # Selection does not itself prove a pair: the source-verified typed helper
    # admits and reads each requested actual baseline definition below.
    selected = [row['id'] for row in constraints if row.get('definition', {}).get('kind') in PAIRS]
    if selected:
        report = helper.run(['--d10', *selected], before)
        if report.get('requested') != selected:
            raise CollectionError('typed D10 helper did not cover the actual requested IDs')
        equalities = verify_length_pairs(report, after, baseline.schema('sketch_constraints', helper),
                                        current.schema('sketch_constraints', helper), current.schema('parameters', helper))
        witnessed = {(row.record_id, 'sketch_constraints', '#', *row.companion_path) for row in equalities}
        findings = [finding for finding in findings if not (
            finding.code in {'neutral-leaf', 'resolver-evidence-required'} and finding.path in witnessed)]
    findings.extend(comparator.compare_native(before.get('native', {}), after.get('native', {}), comparator.Evidence()))
    # No blanket exemptions: these are still missing whole-gate mechanisms even
    # when a tiny unchanged fixture happens to have no structural differences.
    findings.append(comparator.Finding('coverage-incomplete', (), 'Appendix B typed native moves, Rule B preservation, remaining effective-value resolvers and §3.4 guard/code-built corpus coverage are not yet implemented'))
    return findings


def collect_fixture(comparator: Any, fixture: str, baseline: Producer, current: Producer,
                    helper: BaselineHelper, output: Path) -> dict:
    """Run one immutable real fixture against both verified local executables."""
    output.mkdir(parents=True, exist_ok=True)
    starts = [len(owner.commands) for owner in (baseline, current, helper)]
    source = current.repo / fixture
    blob = git(current.repo, 'hash-object', '--', str(source))
    expected = [git(current.repo, 'rev-parse', producer.proof['commit'] + ':' + fixture)
                for producer in (baseline, current)]
    if expected != [blob, blob]:
        raise CollectionError('fixture source bytes differ from an authenticated producer snapshot')
    dumps = []
    for producer in (baseline, current):
        producer.verify()
        target = output / (producer.proof['role'] + '.json')
        command = [producer.proof['binary'], 'dump', str(source), '-o', str(target)]
        value = dump_fixture(Path(producer.proof['binary']), source, target)
        producer.verify()
        producer.commands.append({'command': command, 'commit': producer.proof['commit'],
                                  'binary_sha256': producer.proof['binary_sha256'],
                                  'result': 'decode_error' if 'decode_error' in value else 'decoded'})
        dumps.append(value)
    if git(current.repo, 'hash-object', '--', str(source)) != blob:
        raise CollectionError('fixture changed during collection')
    findings = compare_collected(comparator, *dumps, baseline, current, helper)
    return {'fixture': fixture, 'blob': blob,
            'commands': [command for owner, start in zip((baseline, current, helper), starts) for command in owner.commands[start:]],
            'schema_sha256': {producer.proof['role']: {arena: hashlib.sha256(json.dumps(query, sort_keys=True).encode()).hexdigest() for arena, query in producer.schemas.items()} for producer in (baseline, current)},
            'findings': [vars(finding) for finding in findings],
            'dump_sha256': {role: digest(output / (role + '.json')) for role in ('baseline', 'head')
                            if (output / (role + '.json')).is_file()}}
