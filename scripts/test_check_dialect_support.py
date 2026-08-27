#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-dialect-support.py``.

Every rule the checker enforces is exercised here by synthesizing a pair of
registries (plus, where the rule needs it, a fixture tree and a golden
snapshot) that violate exactly that rule, and asserting the matching failure
fires. The last tests run the checker against the committed registries and
require a clean pass.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-dialect-support.py")
SPEC = importlib.util.spec_from_file_location("check_dialect_support", SCRIPT)
assert SPEC and SPEC.loader
checker = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = checker
SPEC.loader.exec_module(checker)

REPO = SCRIPT.resolve().parent.parent

IDENTITY = """
[format.demo]
complete = true

[[dialect]]
id = "demo:one"
title = "Demo one"
discriminants = { marker = "0x01" }
witness = "spec:Demo specification section 1"

[[dialect]]
id = "demo:two"
title = "Demo two"
discriminants = { marker = "0x02" }
witness = "spec:Demo specification section 2"
"""

# `demo:one` is fixture-backed and scored; `demo:two` is the fixture-less row.
GOOD_SUPPORT = """
[[support]]
dialect = "demo:one"
read = "L2"
write = "none"
fixtures = ["crates/cadmpeg-codec-demo/tests/golden/fixtures/one.demo"]

[[support]]
dialect = "demo:two"
read = "detected"
write = "none"
fixtures = []
reason = "no demo two file exists"
"""

FIXTURE = "crates/cadmpeg-codec-demo/tests/golden/fixtures/one.demo"
SNAPSHOT = "crates/cadmpeg-codec-demo/tests/golden/decode/one.json"


class SupportCase(unittest.TestCase):
    """Write a synthetic repository into a temporary root and check it."""

    def run_check(
        self,
        support: str | None,
        *,
        identity: str | None = IDENTITY,
        files: dict[str, str] | None = None,
        snapshot_id: str | None = "demo:one",
    ):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            if identity is not None:
                (root / "docs" / "dialects.toml").write_text(identity, encoding="utf-8")
            if support is not None:
                (root / "docs" / "dialect-support.toml").write_text(support, encoding="utf-8")
            tree = dict(files) if files else {FIXTURE: "demo bytes"}
            if snapshot_id is not None:
                tree.setdefault(
                    SNAPSHOT,
                    json.dumps({"report": {"dialects": [{"dialect": snapshot_id}]}}),
                )
            for rel, text in tree.items():
                target = root / rel
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(text, encoding="utf-8")
            return checker.check(root)

    def assertFires(self, support: str | None, needle: str, **kwargs):
        failures, _ = self.run_check(support, **kwargs)
        self.assertTrue(
            any(needle in failure for failure in failures),
            f"expected {needle!r} in {failures}",
        )

    def assertClean(self, support: str, **kwargs):
        failures, summary = self.run_check(support, **kwargs)
        self.assertEqual(failures, [], summary)
        return summary


class TestFileLevel(SupportCase):
    def test_baseline_is_clean(self):
        summary = self.assertClean(GOOD_SUPPORT)
        self.assertIn("dialect-support: ok", summary)

    def test_missing_support_registry(self):
        self.assertFires(None, "dialect-support.toml: not found")

    def test_missing_identity_registry(self):
        self.assertFires(GOOD_SUPPORT, "dialects.toml: not found", identity=None)

    def test_support_parse_error(self):
        self.assertFires("this is not = = toml\n", "parse error")

    def test_identity_with_no_rows(self):
        self.assertFires(
            GOOD_SUPPORT, "no identity rows to support", identity="[format.demo]\ncomplete = true\n"
        )

    def test_no_support_rows(self):
        self.assertFires("# empty\n", "no [[support]] rows")

    def test_support_not_an_array_of_tables(self):
        self.assertFires('support = "nope"\n', "non-empty array of tables")

    def test_support_empty_array(self):
        self.assertFires("support = []\n", "non-empty array of tables")

    def test_support_element_not_a_table(self):
        self.assertFires('support = ["nope"]\n', "support #0: not a table")


class TestRowShape(SupportCase):
    def test_missing_required_keys(self):
        failures, _ = self.run_check('[[support]]\ndialect = "demo:one"\n')
        for key in ("read", "write", "fixtures"):
            self.assertTrue(
                any(f"missing {key}" in failure for failure in failures),
                f"expected missing {key} in {failures}",
            )

    def test_unknown_key(self):
        self.assertFires(GOOD_SUPPORT + '\n[[support]]\ndialect = "demo:one"\nscore = "L3"\n', "unknown key score")

    def test_dialect_not_a_string(self):
        self.assertFires("[[support]]\ndialect = 7\n", "dialect must be a string")

    def test_grammar_must_be_a_non_empty_string(self):
        self.assertFires(
            GOOD_SUPPORT.replace('read = "detected"', 'grammar = ""\nread = "detected"'),
            "grammar must be a non-empty string",
        )

    def test_reason_must_be_a_non_empty_string(self):
        self.assertFires(
            GOOD_SUPPORT.replace('reason = "no demo two file exists"', "reason = 7"),
            "reason must be a non-empty string",
        )


class TestVocabulary(SupportCase):
    def test_read_outside_the_vocabulary(self):
        self.assertFires(GOOD_SUPPORT.replace('read = "L2"', 'read = "L10"'), "read must be one of")

    def test_read_rejects_a_bare_score_word(self):
        self.assertFires(GOOD_SUPPORT.replace('read = "L2"', 'read = "partial"'), "read must be one of")

    def test_write_outside_the_vocabulary(self):
        self.assertFires(
            GOOD_SUPPORT.replace('write = "none"', 'write = "partial"', 1), "write must be one of"
        )

    def test_preserved_is_accepted_without_a_catalog(self):
        # Preservation is input-conditioned, so it needs no `targets()` row.
        self.assertClean(GOOD_SUPPORT.replace('write = "none"', 'write = "preserved"', 1))

    def test_every_ladder_rung_is_accepted(self):
        for rung in (f"L{n}" for n in range(10)):
            with self.subTest(rung=rung):
                self.assertClean(GOOD_SUPPORT.replace('read = "L2"', f'read = "{rung}"'))

    def test_every_non_score_disposition_is_accepted(self):
        for word in ("detected", "refused", "unclassified-recovered"):
            with self.subTest(word=word):
                self.assertClean(
                    GOOD_SUPPORT.replace(
                        'read = "L2"', f'read = "{word}"\nreason = "a stated reason"'
                    )
                )


class TestCrossReferences(SupportCase):
    def test_support_row_for_an_unknown_dialect(self):
        self.assertFires(
            GOOD_SUPPORT
            + '\n[[support]]\ndialect = "demo:three"\nread = "detected"\nwrite = "none"\n'
            + 'fixtures = []\nreason = "why"\n',
            "no identity row for dialect demo:three",
        )

    def test_identity_row_with_no_support_row(self):
        rows = GOOD_SUPPORT.split("[[support]]")
        self.assertFires("[[support]]".join(rows[:2]), "demo:two: identity row has no support row")

    def test_duplicate_support_rows(self):
        self.assertFires(
            GOOD_SUPPORT
            + '\n[[support]]\ndialect = "demo:two"\nread = "detected"\nwrite = "none"\n'
            + 'fixtures = []\nreason = "again"\n',
            "demo:two: 2 support rows; expected one",
        )


class TestFixtures(SupportCase):
    def test_fixtures_not_a_list(self):
        self.assertFires(
            GOOD_SUPPORT.replace(f'fixtures = ["{FIXTURE}"]', 'fixtures = "one.demo"'),
            "fixtures must be a list",
        )

    def test_fixture_entry_not_a_string(self):
        self.assertFires(
            GOOD_SUPPORT.replace(f'fixtures = ["{FIXTURE}"]', "fixtures = [7]"),
            "fixture entry must be a non-empty string",
        )

    def test_absolute_fixture_path(self):
        self.assertFires(
            GOOD_SUPPORT.replace(FIXTURE, "/etc/passwd"), "fixture must be a repo-relative path"
        )

    def test_escaping_fixture_path(self):
        self.assertFires(
            GOOD_SUPPORT.replace(FIXTURE, "../outside.demo"),
            "fixture must be a repo-relative path",
        )

    def test_fixture_file_not_found(self):
        self.assertFires(
            GOOD_SUPPORT.replace(FIXTURE, "crates/gone/tests/golden/fixtures/one.demo"),
            "fixture file not found",
        )


class TestFixtureGating(SupportCase):
    def test_a_score_with_no_fixtures_is_refused(self):
        self.assertFires(
            GOOD_SUPPORT.replace(f'fixtures = ["{FIXTURE}"]', "fixtures = []"),
            "cannot claim above detected",
        )

    def test_detected_with_no_fixtures_is_allowed_with_a_reason(self):
        self.assertClean(
            GOOD_SUPPORT.replace('read = "L2"', 'read = "detected"').replace(
                f'fixtures = ["{FIXTURE}"]', 'fixtures = []\nreason = "no file yet"'
            ),
            # No golden pins this dialect, so nothing is owed to the row.
            snapshot_id=None,
        )

    def test_a_score_needs_a_fixture_a_snapshot_confirms(self):
        # The file exists, but no golden snapshot pins any dialect for it.
        self.assertFires(GOOD_SUPPORT, "no fixture confirmed by a golden", snapshot_id=None)

    def test_a_fixture_pinned_to_another_dialect_is_refused(self):
        self.assertFires(GOOD_SUPPORT, "decodes to ['demo:two'], not demo:one", snapshot_id="demo:two")

    def test_an_inspect_branch_snapshot_confirms_a_fixture(self):
        self.assertClean(
            GOOD_SUPPORT,
            files={
                FIXTURE: "demo bytes",
                "crates/cadmpeg-codec-demo/tests/golden/inspect/one.json": json.dumps(
                    {"dialects": [{"dialect": "demo:one"}]}
                ),
            },
            snapshot_id=None,
        )

    def test_a_pinned_fixture_the_row_omits_is_refused(self):
        self.assertFires(
            GOOD_SUPPORT,
            "but the support row does not list it",
            files={
                FIXTURE: "demo bytes",
                "crates/cadmpeg-codec-demo/tests/golden/fixtures/second.demo": "more bytes",
                "crates/cadmpeg-codec-demo/tests/golden/decode/second.json": json.dumps(
                    {"dialects": [{"dialect": "demo:one"}]}
                ),
            },
        )

    def test_a_flat_snapshot_layout_confirms_a_fixture(self):
        self.assertClean(
            GOOD_SUPPORT,
            files={
                FIXTURE: "demo bytes",
                "crates/cadmpeg-codec-demo/tests/golden/one.json": json.dumps(
                    {"decode": {"ir": {"source": {"dialect": "demo:one"}}}}
                ),
            },
            snapshot_id=None,
        )


class TestReasons(SupportCase):
    def test_refused_requires_a_reason(self):
        self.assertFires(
            GOOD_SUPPORT.replace('read = "detected"', 'read = "refused"').replace(
                'reason = "no demo two file exists"', ""
            ),
            "read refused requires a reason",
        )

    def test_empty_fixtures_require_a_reason(self):
        self.assertFires(
            GOOD_SUPPORT.replace('reason = "no demo two file exists"', ""),
            "no fixtures requires a reason",
        )

    def test_a_refused_row_with_fixtures_still_requires_a_reason(self):
        self.assertFires(
            GOOD_SUPPORT.replace('read = "L2"', 'read = "refused"'),
            "read refused requires a reason",
        )


class TestTargetCatalogs(SupportCase):
    """The write-target catalogs, parsed out of ``crates/cadmpeg-codec-*/src``."""

    CATALOG = 'crates/cadmpeg-codec-demo/src/lib.rs'

    def catalog(self, ids: str) -> dict[str, str]:
        return {FIXTURE: "demo bytes", self.CATALOG: ids}

    def test_a_literal_catalog_is_a_subset_of_the_identity_rows(self):
        self.assertClean(
            GOOD_SUPPORT.replace('write = "none"', 'write = "verified"', 1),
            files=self.catalog('TargetDescriptor { id: "demo:one", default: true }'),
        )

    def test_a_target_outside_the_identity_rows(self):
        self.assertFires(
            GOOD_SUPPORT,
            "demo:nine: write target is not an identity row",
            files=self.catalog('TargetDescriptor { id: "demo:nine", default: true }'),
        )

    def test_a_target_the_support_row_says_is_not_written(self):
        self.assertFires(
            GOOD_SUPPORT,
            'exported as a write target but the support row says write = "none"',
            files=self.catalog('TargetDescriptor { id: "demo:one", default: true }'),
        )

    def test_a_preserved_row_exported_as_a_synthesis_target(self):
        self.assertFires(
            GOOD_SUPPORT.replace('write = "none"', 'write = "preserved"', 1),
            "preservation is input-conditioned and is never a targets() row",
            files=self.catalog('TargetDescriptor { id: "demo:one", default: true }'),
        )

    def two_is_the_only_target(self, one_write: str) -> str:
        """`demo:two` is the catalog's sole target; `demo:one` writes some other way."""
        return (
            '[[support]]\ndialect = "demo:one"\nread = "L2"\n'
            f'write = "{one_write}"\nfixtures = ["{FIXTURE}"]\n'
            '\n[[support]]\ndialect = "demo:two"\nread = "detected"\n'
            'write = "verified"\nfixtures = []\nreason = "synthesized, never observed"\n'
        )

    def test_a_synthesis_claim_the_catalog_does_not_export(self):
        # `demo:one` claims synthesis, but the catalog exports only `demo:two`,
        # so the table and the encoder have drifted apart.
        self.assertFires(
            self.two_is_the_only_target("emitted"),
            "the demo targets() catalog does not export it",
            files=self.catalog('TargetDescriptor { id: "demo:two", default: true }'),
        )

    def test_a_preserved_row_beside_a_catalog_that_omits_it(self):
        # The same shape with `preserved` is exactly right: preservation is
        # reachable without a catalog row, synthesis is not.
        self.assertClean(
            self.two_is_the_only_target("preserved"),
            files=self.catalog('TargetDescriptor { id: "demo:two", default: true }'),
        )

    def test_a_catalog_for_a_format_with_no_identity_rows(self):
        self.assertFires(
            GOOD_SUPPORT,
            "other: write-target catalog for a format with no identity rows",
            files=self.catalog('TargetDescriptor { id: "other:one", default: true }'),
        )

    def test_a_target_id_without_a_format_prefix(self):
        self.assertFires(
            GOOD_SUPPORT,
            "is not <format>:<name>",
            files=self.catalog('TargetDescriptor { id: "bare", default: true }'),
        )

    def test_a_pinned_call_resolves_through_the_match_arms(self):
        self.assertClean(
            GOOD_SUPPORT.replace('write = "none"', 'write = "emitted"', 1),
            files=self.catalog(
                'const fn pinned(self) -> &str { match self { Self::One => "demo:one", } }\n'
                "TargetDescriptor { id: Demo::One.pinned(), default: true }"
            ),
        )

    def test_an_unresolvable_id_expression_is_loud(self):
        self.assertFires(
            GOOD_SUPPORT,
            "cannot resolve TargetDescriptor id expression",
            files=self.catalog("TargetDescriptor { id: compute_id(), default: true }"),
        )

    def test_a_pinned_call_with_no_matching_arm_is_loud(self):
        self.assertFires(
            GOOD_SUPPORT,
            "cannot resolve TargetDescriptor id expression",
            files=self.catalog("TargetDescriptor { id: Demo::Absent.pinned(), default: true }"),
        )

    def test_a_literal_without_an_id_field_is_loud(self):
        self.assertFires(
            GOOD_SUPPORT,
            "TargetDescriptor literal has no id field",
            files=self.catalog("TargetDescriptor { label: \"no id here\", default: true }"),
        )


class TestCommittedRegistries(unittest.TestCase):
    def test_the_real_registries_are_clean(self):
        failures, summary = checker.check(REPO)
        self.assertEqual(failures, [], "\n".join(failures))
        self.assertTrue(summary.startswith("dialect-support: ok"))

    def test_every_identity_row_is_covered(self):
        import tomllib

        with (REPO / "docs" / "dialects.toml").open("rb") as handle:
            identity = tomllib.load(handle)
        with (REPO / "docs" / "dialect-support.toml").open("rb") as handle:
            support = tomllib.load(handle)
        self.assertEqual(
            {row["id"] for row in identity["dialect"]},
            {row["dialect"] for row in support["support"]},
        )

    def test_every_committed_target_catalog_parses(self):
        failures: list[str] = []
        catalogs = checker.parse_target_catalogs(REPO, failures)
        self.assertEqual(failures, [])
        self.assertTrue(catalogs, "no write-target catalog was found")

    def test_main_exits_zero_on_the_real_registries(self):
        with contextlib.redirect_stdout(io.StringIO()) as out:
            code = checker.main([str(REPO)])
        self.assertEqual(code, 0)
        self.assertIn("dialect-support: ok", out.getvalue())

    def test_main_exits_one_on_a_broken_registry(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            (root / "docs" / "dialect-support.toml").write_text("nonsense = =\n", encoding="utf-8")
            with contextlib.redirect_stderr(io.StringIO()) as err:
                code = checker.main([str(root)])
        self.assertEqual(code, 1)
        self.assertIn("error: ", err.getvalue())


if __name__ == "__main__":
    unittest.main()
