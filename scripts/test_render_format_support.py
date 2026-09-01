#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``render-format-support.py``.

Each fault the renderer refuses is synthesized here and the matching message
asserted. The last case renders the committed repository and requires
``--check`` to pass, so a registry edit without a re-render fails this suite.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import re
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("render-format-support.py")
SPEC = importlib.util.spec_from_file_location("render_format_support", SCRIPT)
assert SPEC and SPEC.loader
renderer = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = renderer
SPEC.loader.exec_module(renderer)

REPO = SCRIPT.resolve().parent.parent

IDENTITY = """\
[format.demo]
complete = true

[[dialect]]
id = "demo:one"
title = "Demo one"
discriminants = { marker = "1" }
witness = "spec:Demo 1"

[[dialect]]
id = "demo:two"
title = "Demo two"
discriminants = { marker = "2" }
witness = "code:demo.rs:1"

[[dialect]]
id = "demo:unknown"
title = "Demo residue"
discriminants = { marker = "other" }
witness = "corpus:demo/other.bin"
"""

SUPPORT = """\
[format.demo]
level = 3
scored = ["demo:one", "demo:two"]

[[support]]
dialect = "demo:one"
read = "L3"
write = "verified"

[[support]]
dialect = "demo:two"
read = "refused"
write = "none"
reason = "not decoded"

[[support]]
dialect = "demo:unknown"
read = "unclassified-recovered"
write = "preserved"
reason = "residue"
"""
EVALUATIONS = "evaluation = []\n"


class HeadlineCase(unittest.TestCase):
    """The owner-declared digit is printed without arithmetic."""

    def _format(self, *, level: int = 3):
        row = renderer.Row("demo:one", "detected", "none")
        return renderer.Format("demo", level, (row,))

    def test_headline_is_the_declared_digit(self):
        self.assertEqual(self._format(level=7).headline, "L7")

    def test_format_section_prints_the_registry_read_cell(self):
        fmt = self._format()
        rendered = renderer.format_section("demo", {"demo": fmt})
        self.assertIn("detected", rendered)
        self.assertNotIn("pending", rendered)


class SpliceCase(unittest.TestCase):
    """A region the generator cannot own unambiguously is a hard failure."""

    def test_a_region_is_replaced_between_its_markers(self):
        text = "head\n<!-- generated: x -->\nstale\n<!-- /generated: x -->\ntail\n"
        out = renderer.splice(text, "x", "\nfresh\n", rel=Path("f.md"))
        self.assertEqual(
            out, "head\n<!-- generated: x -->\n\nfresh\n\n<!-- /generated: x -->\ntail\n"
        )

    def test_a_missing_begin_marker_fails(self):
        with self.assertRaisesRegex(renderer.RenderError, "expected one .* found 0"):
            renderer.splice("nothing\n", "x", "body", rel=Path("f.md"))

    def test_a_duplicated_begin_marker_fails(self):
        text = "<!-- generated: x -->\n<!-- generated: x -->\n<!-- /generated: x -->\n"
        with self.assertRaisesRegex(renderer.RenderError, "found 2"):
            renderer.splice(text, "x", "body", rel=Path("f.md"))

    def test_an_end_marker_before_its_begin_fails(self):
        text = "<!-- /generated: x -->\n<!-- generated: x -->\n"
        with self.assertRaisesRegex(renderer.RenderError, "precedes"):
            renderer.splice(text, "x", "body", rel=Path("f.md"))

    def test_a_lib_rs_region_carries_the_doc_comment_prefix(self):
        text = "//! <!-- generated: x -->\n//! <!-- /generated: x -->\n"
        out = renderer.splice(text, "x", "//! line", rel=Path("l.rs"), prefix="//! ")
        self.assertEqual(out, "//! <!-- generated: x -->\n//! line\n//! <!-- /generated: x -->\n")


class RegistryCase(unittest.TestCase):
    """Structural faults in the two registries. All are exit code 2."""

    @contextlib.contextmanager
    def _root(
        self,
        identity: str = IDENTITY,
        support: str = SUPPORT,
        evaluations: str = EVALUATIONS,
    ):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            (root / "docs" / "dialects.toml").write_text(identity, encoding="utf-8")
            (root / "docs" / "dialect-support.toml").write_text(support, encoding="utf-8")
            (root / "docs" / "evaluations.toml").write_text(evaluations, encoding="utf-8")
            yield root

    def test_a_well_formed_pair_loads(self):
        with self._root() as root:
            formats = renderer.load_formats(root)
            self.assertEqual(formats["demo"].headline, "L3")

    def test_evaluations_are_not_a_renderer_input(self):
        with self._root() as root:
            (root / "docs" / "evaluations.toml").unlink()
            formats = renderer.load_formats(root)
            self.assertEqual(formats["demo"].headline, "L3")

    def test_a_missing_registry_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(renderer.RenderError, "not found"):
                renderer.load_formats(Path(tmp))

    def test_malformed_toml_fails(self):
        with self._root(identity="[format.demo\n") as root:
            with self.assertRaises(renderer.RenderError):
                renderer.load_formats(root)

    def test_an_identity_row_with_no_support_row_fails(self):
        trimmed = SUPPORT.split("[[support]]\ndialect = \"demo:two\"")[0]
        with self._root(support=trimmed) as root:
            with self.assertRaisesRegex(renderer.RenderError, "no support row for demo:two"):
                renderer.load_formats(root)

    def test_a_support_row_naming_no_identity_row_fails(self):
        extra = SUPPORT + '\n[[support]]\ndialect = "demo:ghost"\nread = "detected"\nwrite = "none"\nreason = "x"\n'
        with self._root(support=extra) as root:
            with self.assertRaisesRegex(renderer.RenderError, "demo:ghost"):
                renderer.load_formats(root)

    def test_a_duplicate_support_row_fails(self):
        doubled = SUPPORT + '\n[[support]]\ndialect = "demo:one"\nread = "L1"\nwrite = "none"\n'
        with self._root(support=doubled) as root:
            with self.assertRaisesRegex(renderer.RenderError, "duplicate support row"):
                renderer.load_formats(root)


class AnchorCase(unittest.TestCase):
    """Anchors come from the document's own headings, never from a table."""

    def test_the_enclosing_heading_supplies_the_anchor(self):
        ladder = "## CATIA V5 `.CATPart`\n\n<!-- generated: dialects catia -->\n"
        self.assertEqual(
            renderer.section_profiles(ladder),
            {
                "catia": renderer.Profile(
                    name="CATIA V5 `.CATPart`", anchor="catia-v5-catpart"
                )
            },
        )

    def test_a_region_before_every_heading_fails(self):
        with self.assertRaisesRegex(renderer.RenderError, "precedes every"):
            renderer.section_profiles("<!-- generated: dialects catia -->\n")

    def test_a_format_cannot_own_several_profile_regions(self):
        ladder = (
            "## First\n<!-- generated: dialects demo -->\n"
            "## Second\n<!-- generated: dialects demo -->\n"
        )
        with self.assertRaisesRegex(renderer.RenderError, "several profile regions"):
            renderer.section_profiles(ladder)


class CapabilityTargetCase(unittest.TestCase):
    """Each codec names its own format at both generated surfaces."""

    @contextlib.contextmanager
    def _root(self, readme_format: str, lib_format: str):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            crate = root / "crates" / "cadmpeg-codec-demo"
            (crate / "src").mkdir(parents=True)
            (crate / "README.md").write_text(
                f"<!-- generated: capability {readme_format} -->\n"
                f"<!-- /generated: capability {readme_format} -->\n",
                encoding="utf-8",
            )
            (crate / "src" / "lib.rs").write_text(
                f"//! <!-- generated: capability {lib_format} -->\n"
                f"//! <!-- /generated: capability {lib_format} -->\n",
                encoding="utf-8",
            )
            yield root

    def test_matching_markers_name_the_target(self):
        with self._root("demo", "demo") as root:
            self.assertEqual(
                renderer.capability_targets(root),
                {
                    "demo": (
                        Path("crates/cadmpeg-codec-demo/README.md"),
                        Path("crates/cadmpeg-codec-demo/src/lib.rs"),
                    )
                },
            )

    def test_mismatched_markers_fail(self):
        with self._root("demo", "other") as root:
            with self.assertRaisesRegex(renderer.RenderError, "must name the same"):
                renderer.capability_targets(root)


class CommittedCase(unittest.TestCase):
    """The committed repository is a fresh render, byte for byte."""

    def test_check_passes_against_the_committed_tree(self):
        stale = renderer.check(REPO)
        self.assertEqual(stale, [], "".join(stale))

    def test_check_reports_a_hand_edited_table(self):
        rendered = renderer.render(REPO)
        rel = renderer.LADDER_REL
        text = rendered[rel].replace("L9", "L2", 1)
        self.assertNotEqual(text, rendered[rel])
        diff = "".join(
            __import__("difflib").unified_diff(
                text.splitlines(keepends=True), rendered[rel].splitlines(keepends=True)
            )
        )
        self.assertIn("L2", diff)

    def test_every_generated_headline_is_one_ladder_digit(self):
        rendered = renderer.render(REPO)
        headlines = []
        for rel, text in rendered.items():
            for line in text.splitlines():
                if "Support: " in line:
                    headlines.append(line.split("Support: ", 1)[1].split(" (", 1)[0])
                elif line.startswith("- **") and " — L" in line:
                    headlines.append(line.split(" — ", 1)[1].split(" (", 1)[0])
                elif line.startswith("**Ladder: "):
                    headlines.append(line.removeprefix("**Ladder: ").removesuffix(".**"))
                elif rel == renderer.LADDER_REL and line.startswith("| "):
                    cells = [cell.strip() for cell in line.strip("|").split("|")]
                    if len(cells) == 2 and cells[1] != "Level" and cells[1].startswith("L"):
                        headlines.append(cells[1])
        self.assertTrue(headlines)
        self.assertTrue(all(re.fullmatch(r"L[0-9]", headline) for headline in headlines), headlines)

    def test_forbidden_tokens_are_absent_from_every_generated_region(self):
        forbidden = ("depth", "breadth", ">=", "n/a")
        regions = []
        for rel, text in renderer.render(REPO).items():
            lines = text.splitlines()
            starts = [i for i, line in enumerate(lines) if "<!-- generated: " in line]
            for start in starts:
                prefix, marker = lines[start].split("<!-- generated: ", 1)
                end_line = f"{prefix}<!-- /generated: {marker}"
                end = lines.index(end_line, start + 1)
                regions.append((rel, "\n".join(lines[start + 1 : end]).lower()))
        self.assertTrue(regions)
        for token in forbidden:
            with self.subTest(token=token):
                self.assertEqual(
                    [(rel, token) for rel, body in regions if token in body],
                    [],
                )

    def test_the_committed_tree_needs_no_write(self):
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            code = renderer.main([str(REPO), "--check"])
        self.assertEqual(code, 0, buffer.getvalue())


if __name__ == "__main__":
    unittest.main()
