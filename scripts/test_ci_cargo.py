# SPDX-License-Identifier: Apache-2.0
import unittest

from scripts.ci_cargo import HEAVY_CRATES, command, low_ram_prebuild_commands


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

    def test_low_ram_prebuild_stages_heavy_crates_at_one_job(self):
        stages = low_ram_prebuild_commands(["nextest", "run", "--bins", "--tests", "--no-fail-fast"])
        self.assertEqual(len(stages), 4)
        self.assertEqual([jobs for jobs, _ in stages], [None, 1, 2, 1])
        for _, argv in stages:
            self.assertIn("--no-run", argv)
            self.assertNotIn("--no-fail-fast", argv)
        one_job_args = [arg for jobs, argv in stages if jobs == 1 for arg in argv]
        for crate in HEAVY_CRATES:
            self.assertIn(crate, one_job_args)

    def test_low_ram_prebuild_passes_no_run_once(self):
        stages = low_ram_prebuild_commands(["nextest", "run", "--no-run", "--bins", "--tests"])
        for _, argv in stages:
            self.assertEqual(argv.count("--no-run"), 1)


if __name__ == "__main__":
    unittest.main()
