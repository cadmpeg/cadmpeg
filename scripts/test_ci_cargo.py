# SPDX-License-Identifier: Apache-2.0
import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts import ci_cargo
from scripts.ci_cargo import command, low_ram_rustc_wrapper, package_name, plan, run, test_binaries
from scripts.ci_rustc_lock import WORKSPACE, crate_name, peak_gib, slots

# Answers `--list` like a libtest binary and runs the tests its arguments
# select: `passes` passes, `fails` fails, `hangs` never finishes, and `slow`
# reports 20 seconds.
FAKE_LIBTEST = textwrap.dedent('''\
    import sys, time
    TESTS = {TESTS}
    args = sys.argv[1:]
    if '--list' in args:
        print('\\n'.join(name + ': test' for name in TESTS))
        raise SystemExit(0)
    skips = [args[i + 1] for i, a in enumerate(args) if a == '--skip']
    filters = [a for i, a in enumerate(args)
               if not a.startswith('-') and (i == 0 or args[i - 1] != '--skip')]
    failed = False
    for name in TESTS:
        if any(s in name for s in skips) or (filters and not any(f in name for f in filters)):
            continue
        if name.endswith('hangs'):
            print(f'test {{name}} has been running for over 60 seconds', flush=True)
            time.sleep(60)
        seconds = 20 if name.endswith('slow') else 0.001
        if name.endswith('fails'):
            print(f'test {{name}} ... FAILED <{{seconds:.3f}}s>')
            failed = True
        else:
            print(f'test {{name}} ... ok <{{seconds:.3f}}s>')
    raise SystemExit(101 if failed else 0)
''')


def fake_binary(path: Path, tests: list[str]) -> str:
    path.write_text(f'#!{sys.executable}\n' + FAKE_LIBTEST.format(TESTS=tests))
    path.chmod(0o755)
    return str(path)


def artifact(package_id: str, kind: str, target: str, executable: str | None,
             test: bool = True) -> str:
    return json.dumps({
        'reason': 'compiler-artifact', 'package_id': package_id,
        'manifest_path': '/w/crates/x/Cargo.toml',
        'target': {'kind': [kind], 'name': target},
        'profile': {'test': test}, 'executable': executable,
    })


class CommandTests(unittest.TestCase):
    def test_scoped_features_reach_selected_packages(self):
        for packages, expected in [
            (["cadmpeg", "cadmpeg-registry", "cadmpeg-codec-freecad"], "cadmpeg/fcstd,cadmpeg-registry/fcstd"),
            (["cadmpeg"], "cadmpeg/fcstd"),
            (["cadmpeg-registry"], "cadmpeg-registry/fcstd"),
        ]:
            with self.subTest(packages=packages):
                args = command(["test", "--no-run", "--bins", "--tests"], packages, codecs=["freecad"])
                self.assertEqual(args[args.index("--features") + 1], expected)
                self.assertIn("--bins", args)
                self.assertNotIn("--lib", args)

    def test_rustc_lock_wrapper_estimates_peak_memory(self):
        self.assertTrue(low_ram_rustc_wrapper().is_file())
        self.assertEqual(crate_name(["--crate-name", "cadmpeg_ir", "src/lib.rs"]), "cadmpeg_ir")
        self.assertEqual(crate_name(["--crate-name=cadmpeg_codec_f3d"]), "cadmpeg_codec_f3d")
        self.assertIsNone(crate_name(["-vV"]))
        crate = str(WORKSPACE / "crates/cadmpeg-asm")
        self.assertGreater(peak_gib(["--crate-name", "cadmpeg_codec_f3d", "--test"], crate),
                           peak_gib(["--crate-name", "cadmpeg_codec_usd", "--test"], crate))
        self.assertGreater(peak_gib(["--crate-name", "cadmpeg_asm", "--test"], crate),
                           peak_gib(["--crate-name", "cadmpeg_asm"], crate))
        self.assertLess(peak_gib(["--crate-name", "serde"], "/registry/serde-1.0.0"),
                        peak_gib(["--crate-name", "cadmpeg_asm"], crate))
        self.assertEqual(slots(5.0, 4.5), 18)
        self.assertEqual(slots(0.1, 4.5), 1)

    def test_rustc_lock_wrapper_runs_claims_beyond_the_budget_one_at_a_time(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = dict(os.environ, CI_RUSTC_LOCK_FILE=f"{tmp}/budget", CI_RUSTC_MEMORY_GIB="10",
                       CI_RUSTC_LOG=f"{tmp}/units.tsv")
            env.pop("CARGO_MANIFEST_DIR", None)
            # Each stand-in rustc records when it ran; the two largest claims
            # cannot share a 10 GiB budget, a 0.5 GiB dependency fits beside one.
            script = "import sys, time; open(sys.argv[-1], 'w').write(f'{time.time()} '); time.sleep(0.6); open(sys.argv[-1], 'a').write(f'{time.time()}')"
            runs = [(name, Path(tmp) / name) for name in ("cadmpeg_codec_f3d", "cadmpeg_ir", "serde")]
            procs = [subprocess.Popen([sys.executable, str(low_ram_rustc_wrapper()), sys.executable, "-c", script,
                                       "--crate-name", name, *(["--test"] if name != "serde" else []), str(out)],
                                      env=env)
                     for name, out in runs]
            self.assertEqual([proc.wait() for proc in procs], [0, 0, 0])
            spans = {name: tuple(map(float, out.read_text().split())) for name, out in runs}
            overlap = lambda a, b: spans[a][0] < spans[b][1] and spans[b][0] < spans[a][1]
            self.assertFalse(overlap("cadmpeg_codec_f3d", "cadmpeg_ir"))
            self.assertTrue(overlap("serde", "cadmpeg_codec_f3d") or overlap("serde", "cadmpeg_ir"))
            self.assertEqual(len(Path(f"{tmp}/units.tsv").read_text().splitlines()), 3)


class TestBinaryTests(unittest.TestCase):
    def test_package_names_come_from_both_id_forms(self):
        self.assertEqual(package_name('path+file:///w/crates/cadmpeg-ir#0.6.12'), 'cadmpeg-ir')
        self.assertEqual(package_name('path+file:///w/crates/cli#cadmpeg@0.6.12'), 'cadmpeg')

    def test_only_test_executables_are_run_and_named_by_target(self):
        messages = '\n'.join([
            artifact('path+file:///w/crates/cadmpeg-ir#0.6.12', 'lib', 'cadmpeg_ir', '/t/ir'),
            artifact('path+file:///w/crates/cli#cadmpeg@0.6.12', 'test', 'integration', '/t/int'),
            artifact('path+file:///w/crates/cli#cadmpeg@0.6.12', 'bin', 'cadmpeg', '/t/bin'),
            artifact('path+file:///w/crates/cli#cadmpeg@0.6.12', 'bin', 'cadmpeg', '/t/cli', test=False),
            artifact('path+file:///w/crates/cadmpeg-ir#0.6.12', 'lib', 'cadmpeg_ir', None, test=False),
            '   Compiling cadmpeg-ir',
        ])
        self.assertEqual(test_binaries(messages), [
            ('cadmpeg-ir', '/t/ir', '/w/crates/x'),
            ('cadmpeg::integration', '/t/int', '/w/crates/x'),
            ('cadmpeg::bin/cadmpeg', '/t/bin', '/w/crates/x'),
        ])


class RunTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)

    def plan_and_run(self, binaries: dict[str, list[str]]) -> tuple[int, str, list]:
        executables = [(name, fake_binary(self.directory / name, tests), str(self.directory))
                       for name, tests in binaries.items()]
        planned = plan(executables, threads=2)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            status = run(planned, jobs=2)
        return status, output.getvalue(), planned

    def test_binaries_with_the_most_tests_start_first(self):
        status, _, planned = self.plan_and_run({
            'small': ['a::one'],
            'empty': [],
            'big': ['a::one', 'a::two', 'b::three'],
        })
        self.assertEqual(status, 0)
        self.assertEqual([(binary.name, binary.tests) for binary in planned], [('big', 3), ('small', 1)])

    def test_slow_tests_are_listed_with_their_times(self):
        status, output, _ = self.plan_and_run({'crate': ['a::passes', 'a::slow']})
        self.assertEqual(status, 0)
        self.assertIn('[   20.0s] crate a::slow', output)
        self.assertNotIn('a::passes', output.split('or more:')[1])

    def test_a_failing_test_is_named_and_fails_the_run(self):
        status, output, _ = self.plan_and_run({'crate': ['a::passes', 'a::fails']})
        self.assertEqual(status, 1)
        self.assertIn('test a::fails ... FAILED', output)
        self.assertIn('FAIL: crate', output)

    def test_a_hung_test_is_killed_and_named(self):
        with patch.object(ci_cargo, 'DEADLINE_SECONDS', 1):
            status, output, _ = self.plan_and_run({'crate': ['a::passes', 'a::hangs']})
        self.assertEqual(status, 1)
        self.assertIn('still running: a::hangs', output)
        self.assertIn('TIMEOUT: crate', output)

    def test_run_defaults_cadmpeg_threads_to_one(self):
        script = self.directory / 'env_check'
        script.write_text(textwrap.dedent(f'''\
            #!{sys.executable}
            import os, sys
            if '--list' in sys.argv:
                print('a::env: test')
                raise SystemExit(0)
            assert os.environ.get('CADMPEG_THREADS') == os.environ['EXPECT_THREADS']
            assert os.environ.get('RUSTC_BOOTSTRAP') == '1'
            print('test a::env ... ok <0.001s>')
        '''))
        script.chmod(0o755)
        with patch.dict(os.environ, {'EXPECT_THREADS': '1'}, clear=False):
            os.environ.pop('CADMPEG_THREADS', None)
            planned = plan([('env_check', str(script), str(self.directory))], threads=2)
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(run(planned, jobs=1), 0)
        with patch.dict(os.environ, {'CADMPEG_THREADS': '3', 'EXPECT_THREADS': '3'}, clear=False):
            planned = plan([('env_check', str(script), str(self.directory))], threads=2)
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(run(planned, jobs=1), 0)


if __name__ == "__main__":
    unittest.main()
