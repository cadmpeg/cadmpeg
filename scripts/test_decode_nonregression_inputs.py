#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Exercise fixture discovery against real tracked Git trees."""

from pathlib import Path
import subprocess
import tempfile
import unittest

from decode_nonregression_inputs import base_binary, discover, dump_fixture, is_fixture, pair_fixtures


class FixtureInputs(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.git('init', '-q')
        self.git('config', 'user.name', 'pcurve')
        self.git('config', 'user.email', '301495646+pcurve@users.noreply.github.com')

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.repo), *args], text=True).strip()

    def write(self, path, data):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(data)

    def commit(self):
        self.git('add', '.')
        self.git('commit', '-qm', 'fixture control')
        return self.git('rev-parse', 'HEAD')

    def test_tracked_discovery_keeps_raw_sources_and_excludes_outputs(self):
        fixture = 'crates/cadmpeg-codec-rhino/tests/golden/fixtures/space name.3dm'
        self.write(fixture, 'source')
        self.write('crates/cadmpeg-codec-rhino/tests/golden/decoded/space name.json', '{}')
        revision = self.commit()
        self.write('crates/cadmpeg-codec-rhino/tests/golden/fixtures/untracked.3dm', 'not committed')
        self.assertEqual([f.path for f in discover(self.repo, revision).values()], [fixture])

    def test_damaged_move_matches_bytes_and_keeps_base_row(self):
        old = 'crates/cadmpeg-codec-step/tests/golden/fixtures/one.stp'
        new = 'crates/cadmpeg-codec-step/tests/golden/fixtures/damaged/one.stp'
        self.write(old, 'source')
        base = self.commit()
        self.write(new, 'source')
        (self.repo / old).unlink()
        head = self.commit()
        pairs, failures = pair_fixtures(discover(self.repo, base), discover(self.repo, head))
        self.assertEqual(failures, [])
        self.assertEqual([(a.path, b.path) for a, b in pairs], [(old, new)])

    def test_deleted_and_changed_sources_cannot_disappear(self):
        a = 'corpus/freecad_fcstd/fixtures/a.FCStd'
        b = 'crates/cadmpeg-codec-step/tests/fixtures/b.p21'
        self.write(a, 'original a')
        self.write(b, 'original b')
        base = self.commit()
        (self.repo / a).unlink()
        self.write(b, 'replacement')
        head = self.commit()
        pairs, failures = pair_fixtures(discover(self.repo, base), discover(self.repo, head))
        self.assertEqual(pairs, [])
        self.assertEqual(len(failures), 2)
        self.assertTrue(any(f.startswith('fixture-missing:') for f in failures))
        self.assertTrue(any(f.startswith('fixture-bytes-changed:') for f in failures))

    def test_duplicate_stem_is_ambiguous(self):
        self.write('crates/cadmpeg-codec-step/tests/golden/fixtures/one.stp', 'first')
        self.write('crates/cadmpeg-codec-step/tests/golden/fixtures/damaged/one.step', 'second')
        revision = self.commit()
        with self.assertRaisesRegex(ValueError, 'ambiguous fixture stem'):
            discover(self.repo, revision)

    def test_exact_fixture_locations(self):
        self.assertTrue(is_fixture('crates/cadmpeg-codec-step/tests/fixtures/a.p21'))
        self.assertTrue(is_fixture('corpus/freecad_fcstd/fixtures/damaged/a.FCStd'))
        self.assertFalse(is_fixture('private-corpus/a.FCStd'))
        self.assertFalse(is_fixture('crates/cadmpeg-codec-step/tests/fixtures/a.json'))


    def executable(self, body):
        path = self.repo / 'decoder'
        path.write_text('#!/usr/bin/env python3\n' + body)
        path.chmod(0o755)
        return path

    def test_dump_consumes_real_input_and_never_accepts_stale_output(self):
        source = self.repo / 'input'
        source.write_text('fixture payload')
        output = self.repo / 'result.json'
        binary = self.executable(
            'import json,sys\nfrom pathlib import Path\n'
            'assert sys.argv[1] == "dump" and sys.argv[3] == "-o"\n'
            'Path(sys.argv[4]).write_text(json.dumps({"ir_version":"8","payload":Path(sys.argv[2]).read_text()}))\n'
        )
        self.assertEqual(dump_fixture(binary, source, output)['payload'], 'fixture payload')
        binary = self.executable('raise SystemExit(7)\n')
        self.assertIn('decode_error', dump_fixture(binary, source, output))
        self.assertFalse(output.exists())

    def test_success_without_valid_output_is_fatal(self):
        source, output = self.repo / 'input', self.repo / 'result.json'
        source.write_text('input')
        with self.assertRaises(FileNotFoundError):
            dump_fixture(self.executable('pass\n'), source, output)
        with self.assertRaisesRegex(ValueError, 'CADIR document'):
            dump_fixture(self.executable(
                'import sys\nfrom pathlib import Path\nPath(sys.argv[4]).write_text("{}")\n'
            ), source, output)

    def test_each_decode_has_its_own_timeout(self):
        source, output = self.repo / 'input', self.repo / 'result.json'
        source.write_text('input')
        binary = self.executable('import time\ntime.sleep(10)\n')
        self.assertEqual(dump_fixture(binary, source, output, timeout_seconds=0.02),
                         {'decode_error': 'decode timed out'})

    def test_base_binary_requires_current_build_and_clean_source(self):
        self.write('tracked', 'base')
        commit = self.commit()
        self.git('update-ref', 'refs/remotes/origin/main', commit)
        source = self.repo / 'target/base-src'
        self.git('worktree', 'add', '--detach', str(source), commit)
        binary = self.repo / 'target/base/cadmpeg'
        binary.parent.mkdir(parents=True)
        binary.write_text('#!/bin/sh\nexit 0\n')
        binary.chmod(0o755)
        marker = binary.parent / 'base.commit'
        marker.write_text(commit + '\n')
        self.assertEqual(base_binary(self.repo, str(binary)), binary)
        with self.assertRaisesRegex(ValueError, 'must name'):
            base_binary(self.repo, str(self.repo / 'other'))
        marker.write_text('0' * 40 + '\n')
        with self.assertRaisesRegex(ValueError, 'current merge base'):
            base_binary(self.repo, str(binary))
        marker.write_text(commit + '\n')
        (source / 'tracked').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'source changed'):
            base_binary(self.repo, str(binary))


if __name__ == '__main__':
    unittest.main()
