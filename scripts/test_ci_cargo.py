# SPDX-License-Identifier: Apache-2.0
import unittest

import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from scripts.ci_cargo import command, low_ram_rustc_wrapper, low_ram_test_command
from scripts.ci_rustc_lock import WORKSPACE, crate_name, peak_gib, slots


class CommandTests(unittest.TestCase):
    def test_scoped_features_reach_selected_packages(self):
        for packages, expected in [
            (["cadmpeg", "cadmpeg-registry", "cadmpeg-codec-freecad"], "cadmpeg/fcstd,cadmpeg-registry/fcstd"),
            (["cadmpeg"], "cadmpeg/fcstd"),
            (["cadmpeg-registry"], "cadmpeg-registry/fcstd"),
        ]:
            with self.subTest(packages=packages):
                args = command(["nextest", "run", "--bins", "--tests"], packages, codecs=["freecad"])
                self.assertEqual(args[args.index("--features") + 1], expected)
                self.assertIn("--bins", args)
                self.assertNotIn("--lib", args)

    def test_low_ram_macos_uses_in_process_cargo_test(self):
        args = low_ram_test_command(
            ["nextest", "run", "--bins", "--tests", "--no-fail-fast"],
            platform="darwin",
        )
        self.assertEqual(
            args,
            ["cargo", "test", "-q", "--workspace", "--bins", "--tests", "--no-fail-fast"],
        )

    def test_low_ram_linux_keeps_nextest(self):
        args = low_ram_test_command(
            ["nextest", "run", "--bins", "--tests", "--no-fail-fast"],
            platform="linux",
        )
        self.assertEqual(args[:3], ["cargo", "nextest", "run"])
        self.assertIn("--workspace", args)

    def test_rustc_lock_wrapper_estimates_peak_memory(self):
        self.assertTrue(low_ram_rustc_wrapper().is_file())
        self.assertEqual(crate_name(["--crate-name", "cadmpeg_ir", "src/lib.rs"]), "cadmpeg_ir")
        self.assertEqual(crate_name(["--crate-name=cadmpeg_codec_f3d"]), "cadmpeg_codec_f3d")
        self.assertIsNone(crate_name(["-vV"]))
        crate = str(WORKSPACE / "crates/cadmpeg-asm")
        self.assertEqual(peak_gib(["--crate-name", "cadmpeg_codec_f3d", "--test"], crate), 5.0)
        self.assertGreater(peak_gib(["--crate-name", "cadmpeg_asm", "--test"], crate),
                           peak_gib(["--crate-name", "cadmpeg_asm"], crate))
        self.assertLess(peak_gib(["--crate-name", "serde"], "/registry/serde-1.0.0"),
                        peak_gib(["--crate-name", "cadmpeg_asm"], crate))
        self.assertEqual(slots(5.0, 4.5), 18)
        self.assertEqual(slots(0.1, 4.5), 1)

    def test_rustc_lock_wrapper_runs_claims_beyond_the_budget_one_at_a_time(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = dict(os.environ, CI_RUSTC_LOCK_FILE=f"{tmp}/budget", CI_RUSTC_MEMORY_GIB="6",
                       CI_RUSTC_LOG=f"{tmp}/units.tsv")
            env.pop("CARGO_MANIFEST_DIR", None)
            # Each stand-in rustc records when it ran; two 5 GiB claims cannot
            # share a 6 GiB budget, a 0.5 GiB dependency fits beside one.
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

if __name__ == "__main__":
    unittest.main()
