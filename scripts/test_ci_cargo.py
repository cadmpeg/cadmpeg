# SPDX-License-Identifier: Apache-2.0
import unittest

from scripts.ci_cargo import command


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


if __name__ == "__main__":
    unittest.main()
