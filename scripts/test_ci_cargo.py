# SPDX-License-Identifier: Apache-2.0
import unittest

from scripts.ci_cargo import command, low_ram_rustc_wrapper, low_ram_test_command
from scripts.ci_rustc_lock import (
    EXCLUSIVE_CRATES,
    EXCLUSIVE_TEST_CRATES,
    MEDIUM_LIB_CRATES,
    MEDIUM_TEST_CRATES,
    classify,
    crate_name,
)


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

    def test_rustc_lock_wrapper_classifies_heavy_crates(self):
        self.assertTrue(low_ram_rustc_wrapper().is_file())
        self.assertEqual(crate_name(["--crate-name", "cadmpeg_ir", "src/lib.rs"]), "cadmpeg_ir")
        self.assertEqual(crate_name(["--crate-name=cadmpeg_codec_f3d"]), "cadmpeg_codec_f3d")
        self.assertIsNone(crate_name(["-vV"]))
        self.assertEqual(classify(["--crate-name", "cadmpeg_ir", "src/lib.rs"]), "exclusive")
        self.assertEqual(classify(["--crate-name", "cadmpeg_codec_f3d", "--test"]), "exclusive")
        self.assertEqual(classify(["--crate-name", "cadmpeg_codec_sldprt", "src/lib.rs"]), "medium")
        self.assertEqual(classify(["--crate-name", "cadmpeg_codec_sldprt", "--test"]), "exclusive")
        self.assertEqual(classify(["--crate-name", "cadmpeg_codec_catia", "src/lib.rs"]), "medium")
        self.assertEqual(classify(["--crate-name", "cadmpeg_codec_catia", "--test"]), "exclusive")
        self.assertEqual(classify(["--crate-name", "cadmpeg_codec_inventor", "--test"]), "medium")
        self.assertEqual(classify(["--crate-name", "cadmpeg_core", "src/lib.rs"]), "light")
        self.assertTrue(EXCLUSIVE_CRATES.isdisjoint(EXCLUSIVE_TEST_CRATES))
        self.assertTrue(EXCLUSIVE_CRATES.isdisjoint(MEDIUM_LIB_CRATES))
        self.assertTrue(EXCLUSIVE_TEST_CRATES.isdisjoint(MEDIUM_TEST_CRATES))


if __name__ == "__main__":
    unittest.main()
