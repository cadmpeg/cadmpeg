#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``check-dialect-support.py``.

Every rule the checker enforces is exercised here by synthesizing a pair of
registries (plus, where the rule needs it, a golden fixture and snapshot) that
violate exactly that rule, and asserting the matching failure
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

# `demo:one` has a golden snapshot domain and is scored; `demo:two` does not.
GOOD_SUPPORT = """
[format.demo]
level = 2
scored = ["demo:one", "demo:two"]

[[support]]
dialect = "demo:one"
read = "L2"
write = "none"

[[support]]
dialect = "demo:two"
read = "detected"
write = "none"
reason = "no demo two file exists"
"""

FIXTURE = "crates/cadmpeg-codec-demo/tests/golden/fixtures/one.demo"
SNAPSHOT = "crates/cadmpeg-codec-demo/tests/golden/decode/one.json"
GOOD_EVALUATIONS = "evaluation = []\n"


class SupportCase(unittest.TestCase):
    """Write a synthetic repository into a temporary root and check it."""

    def run_check(
        self,
        support: str | None,
        *,
        identity: str | None = IDENTITY,
        evaluations: str | None = GOOD_EVALUATIONS,
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
            if evaluations is not None:
                (root / "docs" / "evaluations.toml").write_text(evaluations, encoding="utf-8")
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
        for key in ("read", "write"):
            self.assertTrue(
                any(f"missing {key}" in failure for failure in failures),
                f"expected missing {key} in {failures}",
            )

    def test_unknown_key(self):
        self.assertFires(GOOD_SUPPORT + '\n[[support]]\ndialect = "demo:one"\nscore = "L3"\n', "unknown key score")

    def test_dialect_not_a_string(self):
        self.assertFires("[[support]]\ndialect = 7\n", "dialect must be a string")

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
                support = GOOD_SUPPORT.replace("level = 2", "level = 0")
                self.assertClean(support.replace('read = "L2"', f'read = "{rung}"'))

    def test_every_non_score_disposition_is_accepted(self):
        for word in ("detected", "refused", "unclassified-recovered"):
            with self.subTest(word=word):
                support = GOOD_SUPPORT.replace(
                    'scored = ["demo:one", "demo:two"]', 'scored = ["demo:two"]'
                )
                self.assertClean(
                    support.replace(
                        'read = "L2"', f'read = "{word}"\nreason = "`demo:one` has a stated reason"'
                    )
                )


class TestCrossReferences(SupportCase):
    def test_support_row_for_an_unknown_dialect(self):
        self.assertFires(
            GOOD_SUPPORT
            + '\n[[support]]\ndialect = "demo:three"\nread = "detected"\nwrite = "none"\n'
            + 'reason = "why"\n',
            "no identity row for dialect demo:three",
        )

    def test_identity_row_with_no_support_row(self):
        rows = GOOD_SUPPORT.split("[[support]]")
        self.assertFires("[[support]]".join(rows[:2]), "demo:two: identity row has no support row")

    def test_duplicate_support_rows(self):
        self.assertFires(
            GOOD_SUPPORT
            + '\n[[support]]\ndialect = "demo:two"\nread = "detected"\nwrite = "none"\n'
            + 'reason = "again"\n',
            "demo:two: 2 support rows; expected one",
        )


class TestSnapshotDomainGating(SupportCase):
    def test_a_score_with_no_snapshot_domain_is_refused(self):
        self.assertFires(GOOD_SUPPORT, "no golden snapshot domain", snapshot_id=None)

    def test_detected_with_no_snapshot_domain_is_allowed(self):
        self.assertClean(
            GOOD_SUPPORT.replace('read = "L2"', 'read = "detected"'),
            snapshot_id=None,
        )

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

    def test_every_pinned_fixture_joins_the_domain_without_a_registry_list(self):
        summary = self.assertClean(
            GOOD_SUPPORT,
            files={
                FIXTURE: "demo bytes",
                "crates/cadmpeg-codec-demo/tests/golden/fixtures/second.demo": "more bytes",
                "crates/cadmpeg-codec-demo/tests/golden/decode/one.json": json.dumps(
                    {"dialects": [{"dialect": "demo:one"}]}
                ),
                "crates/cadmpeg-codec-demo/tests/golden/decode/second.json": json.dumps(
                    {"dialects": [{"dialect": "demo:one"}]}
                ),
            },
            snapshot_id=None,
        )
        self.assertIn("2 golden fixture-domain confirmations", summary)

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

    def test_refusal_reason_requires_registry_id_or_named_evidence_gap(self):
        support = GOOD_SUPPORT.replace('read = "detected"', 'read = "refused"').replace(
            'reason = "no demo two file exists"', 'reason = "parser unavailable"'
        )
        self.assertFires(support, "must reference a registry id or a named evidence gap")

    def test_refusal_reason_rejects_unknown_registry_id(self):
        support = GOOD_SUPPORT.replace('read = "detected"', 'read = "refused"').replace(
            'reason = "no demo two file exists"', 'reason = "`demo:missing` has no parser"'
        )
        self.assertFires(support, "references unknown registry id demo:missing")

    def test_refusal_reason_accepts_its_registry_id(self):
        support = GOOD_SUPPORT.replace('read = "detected"', 'read = "refused"').replace(
            'reason = "no demo two file exists"', 'reason = "`demo:two` has no parser"'
        ).replace('scored = ["demo:one", "demo:two"]', 'scored = ["demo:one"]')
        self.assertClean(support)

    def test_refusal_reason_accepts_a_named_evidence_gap(self):
        support = GOOD_SUPPORT.replace('read = "detected"', 'read = "refused"').replace(
            'reason = "no demo two file exists"',
            'reason = "evidence gap `missing-container-marker` prevents classification"',
        ).replace('scored = ["demo:one", "demo:two"]', 'scored = ["demo:one"]')
        self.assertClean(support)


class TestFormatBlocks(SupportCase):
    def test_missing_format_table(self):
        self.assertFires(GOOD_SUPPORT.replace("[format.demo]", "[other.demo]"), "format: must be a table")

    def test_missing_format_block(self):
        self.assertFires(GOOD_SUPPORT.replace("[format.demo]", "[format.other]"), "format.demo: missing format block")

    def test_foreign_format_block(self):
        self.assertFires(GOOD_SUPPORT + "\n[format.other]\nlevel = 1\nscored = [\"demo:one\"]\n", "format.other: unknown codec format")

    def test_format_block_must_be_a_table(self):
        self.assertFires(GOOD_SUPPORT.replace("[format.demo]\nlevel = 2\nscored = [\"demo:one\", \"demo:two\"]", 'format = { demo = "bad" }'), "format.demo: must be a table")

    def test_missing_format_keys(self):
        self.assertFires(GOOD_SUPPORT.replace("level = 2", "other = 2", 1), "format.demo: missing level")

    def test_missing_scored_key(self):
        self.assertFires(
            GOOD_SUPPORT.replace('scored = ["demo:one", "demo:two"]\n', "", 1),
            "format.demo: missing scored",
        )

    def test_unknown_format_key(self):
        self.assertFires(
            GOOD_SUPPORT.replace("level = 2", "level = 2\nextra = true", 1),
            "format.demo: unknown key extra",
        )

    def test_invalid_level(self):
        self.assertFires(GOOD_SUPPORT.replace("level = 2", "level = 10", 1), "level must be an integer from 0 through 9")

    def test_scored_must_be_a_non_empty_list(self):
        self.assertFires(GOOD_SUPPORT.replace('scored = ["demo:one", "demo:two"]', "scored = []"), "scored must be a non-empty list")

    def test_scored_entry_must_be_a_string(self):
        self.assertFires(
            GOOD_SUPPORT.replace('scored = ["demo:one", "demo:two"]', 'scored = ["demo:one", 2]'),
            "scored dialect must be a non-empty string",
        )

    def test_duplicate_scored_dialect(self):
        self.assertFires(GOOD_SUPPORT.replace('scored = ["demo:one", "demo:two"]', 'scored = ["demo:one", "demo:one"]'), "duplicate scored dialect demo:one")

    def test_foreign_scored_dialect(self):
        identity = IDENTITY + '\n[[dialect]]\nid = "other:one"\ntitle = "Other"\ndiscriminants = { marker = "x" }\nwitness = "spec:Other"\n'
        self.assertFires(GOOD_SUPPORT.replace('"demo:two"]', '"other:one"]', 1), "belongs to another format", identity=identity)

    def test_missing_scored_identity(self):
        self.assertFires(GOOD_SUPPORT.replace('"demo:two"]', '"demo:gone"]', 1), "has no identity row")


class TestEvaluations(SupportCase):
    @staticmethod
    def record(**changes):
        values = {"dialect": '"demo:two"', "date": "2026-08-28", "level": "2", "files": "1", "result": '"decoded"'}
        values.update(changes)
        return "[[evaluation]]\n" + "\n".join(f"{key} = {value}" for key, value in values.items()) + "\n"

    def test_missing_evaluations_registry(self):
        self.assertFires(GOOD_SUPPORT, "evaluations.toml: not found", evaluations=None)

    def test_malformed_evaluations_registry(self):
        self.assertFires(GOOD_SUPPORT, "evaluations.toml: parse error", evaluations="bad = =\n")

    def test_evaluation_rows_are_required(self):
        self.assertFires(GOOD_SUPPORT, "must be an array of tables", evaluations="other = 1\n")

    def test_evaluation_row_must_be_a_table(self):
        self.assertFires(GOOD_SUPPORT, "evaluation #0: not a table", evaluations='evaluation = ["bad"]\n')

    def test_evaluation_dialect_must_be_a_string(self):
        self.assertFires(GOOD_SUPPORT, "dialect must be a non-empty string", evaluations=self.record(dialect="7"))

    def test_missing_evaluation_key(self):
        self.assertFires(GOOD_SUPPORT, "missing result", evaluations=self.record(result=None).replace("result = None\n", ""))

    def test_unknown_evaluation_key(self):
        self.assertFires(GOOD_SUPPORT, "unknown key extra", evaluations=self.record() + "extra = 1\n")

    def test_foreign_evaluation_dialect(self):
        self.assertFires(GOOD_SUPPORT, "no identity row for dialect demo:gone", evaluations=self.record(dialect='"demo:gone"'))

    def test_invalid_evaluation_date(self):
        self.assertFires(GOOD_SUPPORT, "date must be a TOML local date", evaluations=self.record(date='"2026-08-28"'))

    def test_invalid_evaluation_level(self):
        self.assertFires(GOOD_SUPPORT, "level must be an integer from 0 through 9", evaluations=self.record(level="10"))

    def test_invalid_evaluation_file_count(self):
        self.assertFires(GOOD_SUPPORT, "files must be a positive integer", evaluations=self.record(files="0"))

    def test_invalid_evaluation_result(self):
        self.assertFires(GOOD_SUPPORT, "result must be a non-empty string", evaluations=self.record(result='""'))

    def test_path_like_evaluation_string(self):
        self.assertFires(GOOD_SUPPORT, "must not contain a path-like string", evaluations=self.record(result='"decoded fixtures/one.demo"'))

    def test_detected_without_evaluation_is_allowed(self):
        self.assertClean(GOOD_SUPPORT)

    def test_detected_with_equal_evaluation_is_allowed(self):
        self.assertClean(GOOD_SUPPORT, evaluations=self.record())

    def test_detected_with_higher_evaluation_is_allowed(self):
        self.assertClean(GOOD_SUPPORT, evaluations=self.record(level="9"))

    def test_detected_with_lower_evaluation_fails(self):
        self.assertFires(GOOD_SUPPORT, "evaluation L1", evaluations=self.record(level="1"))

    def test_every_evaluation_record_is_compared(self):
        self.assertFires(GOOD_SUPPORT, "evaluation L1", evaluations=self.record() + self.record(level="1"))


class TestDeclaredLevelComparisons(SupportCase):
    def test_fixture_level_below_declared_level_fails(self):
        self.assertFires(GOOD_SUPPORT.replace('read = "L2"', 'read = "L1"'), "read L1 contradicts L2")

    def test_fixture_level_equal_to_declared_level_passes(self):
        self.assertClean(GOOD_SUPPORT)

    def test_fixture_level_above_declared_level_passes(self):
        self.assertClean(GOOD_SUPPORT.replace('read = "L2"', 'read = "L9"'))

    def test_refused_scored_row_fails(self):
        support = GOOD_SUPPORT.replace('read = "detected"', 'read = "refused"')
        self.assertFires(support, "scored dialect demo:two is refused")

    def test_unclassified_scored_row_without_evaluation_fails(self):
        support = GOOD_SUPPORT.replace('read = "detected"', 'read = "unclassified-recovered"')
        self.assertFires(support, "requires an evaluation")

    def test_unclassified_scored_row_with_evaluation_passes(self):
        support = GOOD_SUPPORT.replace('read = "detected"', 'read = "unclassified-recovered"')
        self.assertClean(support, evaluations=TestEvaluations.record())

    def test_a_refused_row_still_requires_a_reason(self):
        self.assertFires(
            GOOD_SUPPORT.replace('read = "L2"', 'read = "refused"'),
            "read refused requires a reason",
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
