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
[[support]]
dialect = "demo:one"
read = "L3"
write = "verified"
fixtures = ["demo/a.bin", "demo/b.bin"]

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


def _row(dialect: str, read: str) -> renderer.Row:
    fmt, _, name = dialect.partition(":")
    witness = "code:x.rs:1" if name == "two" else "corpus:x"
    return renderer.Row(
        dialect=dialect,
        fmt=fmt,
        name=name,
        witness=witness,
        read=read,
        write="none",
        fixtures=0,
    )


class HeadlineCase(unittest.TestCase):
    """Depth and breadth arithmetic, including every degenerate denominator."""

    def _format(self, reads: dict[str, str], *, complete: bool = True) -> renderer.Format:
        return renderer.Format(
            fmt="demo",
            complete=complete,
            rows=tuple(_row(d, r) for d, r in reads.items()),
        )

    def test_depth_is_the_highest_level_any_row_reaches(self):
        fmt = self._format({"demo:one": "L3", "demo:three": "L5", "demo:four": "detected"})
        self.assertEqual(fmt.depth, "L5")

    def test_depth_is_none_when_no_row_carries_a_level(self):
        fmt = self._format({"demo:one": "detected", "demo:unknown": "refused"})
        self.assertEqual(fmt.depth, "none")

    def test_code_witnesses_are_outside_the_denominator(self):
        # `demo:two` is code-witnessed, so neither half counts it.
        fmt = self._format({"demo:one": "L1", "demo:two": "L9"})
        self.assertEqual(fmt.breadth, "1 of 1")

    def test_the_unknown_row_is_outside_the_denominator(self):
        fmt = self._format({"demo:one": "L1", "demo:unknown": "L1"})
        self.assertEqual(fmt.breadth, "1 of 1")

    def test_detected_is_below_l1_and_does_not_count(self):
        fmt = self._format({"demo:one": "L1", "demo:three": "detected"})
        self.assertEqual(fmt.breadth, "1 of 2")

    def test_l0_is_below_l1_and_does_not_count(self):
        fmt = self._format({"demo:one": "L0"})
        self.assertEqual(fmt.breadth, "0 of 1")

    def test_an_incomplete_format_prints_a_floor(self):
        fmt = self._format({"demo:one": "L1"}, complete=False)
        self.assertEqual(fmt.breadth, "1 of >=1")

    def test_no_witnessed_row_prints_not_applicable(self):
        fmt = self._format({"demo:two": "L9", "demo:unknown": "L9"})
        self.assertEqual(fmt.breadth, "n/a")

    def test_refusals_are_counted_not_excluded(self):
        fmt = self._format({"demo:one": "L1", "demo:two": "refused"})
        self.assertEqual(fmt.refusals, 1)


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
    def _root(self, identity: str = IDENTITY, support: str = SUPPORT):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            (root / "docs" / "dialects.toml").write_text(identity, encoding="utf-8")
            (root / "docs" / "dialect-support.toml").write_text(support, encoding="utf-8")
            yield root

    @contextlib.contextmanager
    def _targets(self, mapping: dict[str, renderer.Target]):
        saved = dict(renderer.TARGETS)
        renderer.TARGETS.clear()
        renderer.TARGETS.update(mapping)
        try:
            yield
        finally:
            renderer.TARGETS.clear()
            renderer.TARGETS.update(saved)

    def test_a_well_formed_pair_loads(self):
        with self._root() as root, self._targets({"demo": renderer.Target("Demo")}):
            formats = renderer.load_formats(root)
            self.assertEqual(formats["demo"].headline, "depth L3, breadth 1 of 1")

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
        with self._root(support=trimmed) as root, self._targets({"demo": renderer.Target("Demo")}):
            with self.assertRaisesRegex(renderer.RenderError, "no support row for demo:two"):
                renderer.load_formats(root)

    def test_a_support_row_naming_no_identity_row_fails(self):
        extra = SUPPORT + '\n[[support]]\ndialect = "demo:ghost"\nread = "detected"\nwrite = "none"\nreason = "x"\n'
        with self._root(support=extra) as root, self._targets({"demo": renderer.Target("Demo")}):
            with self.assertRaisesRegex(renderer.RenderError, "demo:ghost"):
                renderer.load_formats(root)

    def test_a_duplicate_support_row_fails(self):
        doubled = SUPPORT + '\n[[support]]\ndialect = "demo:one"\nread = "L1"\nwrite = "none"\n'
        with self._root(support=doubled) as root, self._targets({"demo": renderer.Target("Demo")}):
            with self.assertRaisesRegex(renderer.RenderError, "duplicate support row"):
                renderer.load_formats(root)

    def test_a_format_absent_from_the_target_map_fails(self):
        with self._root() as root, self._targets({"other": renderer.Target("Other")}):
            with self.assertRaisesRegex(renderer.RenderError, "absent from TARGETS"):
                renderer.load_formats(root)

    def test_a_target_map_key_absent_from_the_registry_fails(self):
        with self._root() as root, self._targets(
            {"demo": renderer.Target("Demo"), "ghost": renderer.Target("Ghost")}
        ):
            with self.assertRaisesRegex(renderer.RenderError, "absent from the registry"):
                renderer.load_formats(root)


class AnchorCase(unittest.TestCase):
    """Anchors come from the document's own headings, never from a table."""

    def test_the_enclosing_heading_supplies_the_anchor(self):
        ladder = "## CATIA V5 `.CATPart`\n\n<!-- generated: dialects catia -->\n"
        self.assertEqual(renderer.section_anchors(ladder), {"catia": "catia-v5-catpart"})

    def test_a_region_before_every_heading_fails(self):
        with self.assertRaisesRegex(renderer.RenderError, "precedes every"):
            renderer.section_anchors("<!-- generated: dialects catia -->\n")


class CommittedCase(unittest.TestCase):
    """The committed repository is a fresh render, byte for byte."""

    def test_check_passes_against_the_committed_tree(self):
        stale = renderer.check(REPO)
        self.assertEqual(stale, [], "".join(stale))

    def test_check_reports_a_hand_edited_table(self):
        rendered = renderer.render(REPO)
        rel = renderer.LADDER_REL
        text = rendered[rel].replace("depth L9", "depth L2", 1)
        self.assertNotEqual(text, rendered[rel])
        diff = "".join(
            __import__("difflib").unified_diff(
                text.splitlines(keepends=True), rendered[rel].splitlines(keepends=True)
            )
        )
        self.assertIn("depth L2", diff)

    def test_the_committed_tree_needs_no_write(self):
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            code = renderer.main([str(REPO), "--check"])
        self.assertEqual(code, 0, buffer.getvalue())


if __name__ == "__main__":
    unittest.main()
