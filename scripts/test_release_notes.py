#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``release-notes.py``."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("release-notes.py")
SPEC = importlib.util.spec_from_file_location("release_notes", SCRIPT)
assert SPEC and SPEC.loader
notes = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = notes
SPEC.loader.exec_module(notes)

SHA = "a" * 40


def record(sha: str, author: str, subject: str) -> str:
    return f"{sha}\x1f{author}\x1f{subject}\x1e\n"


def pull(number: int, sha: str, body: str | None, login: str, typename: str = "User") -> dict:
    return {
        "number": number,
        "body": body,
        "mergeCommit": {"oid": sha},
        "author": {"__typename": typename, "login": login},
    }


class ParseCommits(unittest.TestCase):
    def test_counts_dependency_updates_and_skips_other_bots(self) -> None:
        log = (
            record("1" * 40, "raykholo", "f3d: decode prologues (#479)")
            + record("2" * 40, "dependabot[bot]", "build(deps): bump syn (#358)")
            + record("3" * 40, "cadmpeg-release-bot[bot]", "Bump workspace version to 0.6.7")
            + record("4" * 40, "pcurve", "inventor: pass feature to lookup")
        )
        changes, dependency_updates = notes.parse_commits(log)
        self.assertEqual(dependency_updates, 1)
        self.assertEqual([change.sha for change in changes], ["1" * 40, "4" * 40])
        self.assertEqual([change.number for change in changes], [479, None])

    def test_empty_log_has_no_changes(self) -> None:
        self.assertEqual(notes.parse_commits(""), ([], 0))


class ApplyGithub(unittest.TestCase):
    def test_takes_the_pull_request_merged_as_the_commit(self) -> None:
        change = notes.Change(SHA, "f3d: x")
        commit = {
            "author": {"user": {"__typename": "User", "login": "committer"}},
            "associatedPullRequests": {
                "nodes": [pull(7, "b" * 40, "other", "other"), pull(8, SHA, "body", "author")]
            },
        }
        notes.apply_github(change, commit)
        self.assertEqual((change.number, change.body, change.login), (8, "body", "author"))

    def test_falls_back_to_the_commit_author_without_a_merged_pull_request(self) -> None:
        change = notes.Change(SHA, "f3d: x (#9)", number=9)
        commit = {
            "author": {"user": {"__typename": "User", "login": "committer"}},
            "associatedPullRequests": {"nodes": [pull(7, "b" * 40, "other", "other")]},
        }
        notes.apply_github(change, commit)
        self.assertEqual((change.number, change.body, change.login), (9, "", "committer"))

    def test_null_body_and_bot_author_leave_no_note_or_login(self) -> None:
        change = notes.Change(SHA, "x")
        commit = {
            "author": {"user": None},
            "associatedPullRequests": {"nodes": [pull(8, SHA, None, "copilot", typename="Bot")]},
        }
        notes.apply_github(change, commit)
        self.assertEqual((change.body, change.login), ("", None))

    def test_unknown_commit_keeps_the_subject_number(self) -> None:
        change = notes.Change(SHA, "x (#5)", number=5)
        notes.apply_github(change, None)
        self.assertEqual((change.number, change.login), (5, None))

    def test_bot_suffixed_login_is_not_a_contributor(self) -> None:
        self.assertIsNone(notes.login_of({"__typename": "User", "login": "helper[bot]"}))


class ReleaseNotes(unittest.TestCase):
    def test_reads_lines_up_to_the_next_heading_without_comments(self) -> None:
        body = (
            "## Summary\r\n\r\nText.\r\n\r\n## Release note\r\n\r\n"
            "<!-- One line for users. -->\r\n- Faster STEP export.\r\n"
            "* Breaking: `--allow-empty` is removed.\r\n\r\n## Checklist\r\n- [x] Signed off\r\n"
        )
        self.assertEqual(
            notes.release_notes(body),
            ["Faster STEP export.", "Breaking: `--allow-empty` is removed."],
        )

    def test_none_and_placeholder_only_give_no_notes(self) -> None:
        self.assertEqual(notes.release_notes("## Release note\n\nNone.\n- n/a\n-\n"), [])
        self.assertEqual(notes.release_notes("## Release note\n<!--\n## Release note\nx\n-->\n"), [])

    def test_body_without_the_section_gives_no_notes(self) -> None:
        self.assertEqual(notes.release_notes("## Summary\n\nRelease note: no heading.\n"), [])


class Areas(unittest.TestCase):
    def test_every_named_area_without_the_scope(self) -> None:
        self.assertEqual(
            notes.areas_of("freecad, step: apply placements once (#437)"),
            (["FreeCAD", "STEP"], "apply placements once (#437)"),
        )

    def test_aliases_of_one_area_list_it_once(self) -> None:
        self.assertEqual(notes.areas_of("f3d, f3z: x"), (["Fusion"], "x"))

    def test_unscoped_or_unknown_scope_is_general_and_verbatim(self) -> None:
        for subject in ("Recover usable geometry (#476)", "widget: x", "(fix): inventor: x"):
            self.assertEqual(notes.areas_of(subject), ([notes.GENERAL], subject))

    def test_line_replaces_only_the_merged_pull_request_suffix(self) -> None:
        change = notes.Change(SHA, "", number=521)
        self.assertEqual(
            notes.change_line(change, "give edges a tolerance (#501) (#521)"),
            "Give edges a tolerance (#501) (#521)",
        )

    def test_line_without_a_pull_request_names_the_commit(self) -> None:
        self.assertEqual(notes.change_line(notes.Change(SHA, ""), "x"), f"X ({'a' * notes.SHORT_SHA})")


class Compatibility(unittest.TestCase):
    def test_ir_change_names_both_versions(self) -> None:
        line = notes.ir_change(
            'pub const IR_VERSION: &str = "8";', 'pub const IR_VERSION: &str = "9";'
        )
        self.assertIn("use IR 9", line)
        self.assertIn("does not read IR 8", line)

    def test_unchanged_or_unknown_ir_gives_no_line(self) -> None:
        same = 'pub const IR_VERSION: &str = "9";'
        self.assertIsNone(notes.ir_change(same, same))
        self.assertIsNone(notes.ir_change(None, same))
        self.assertIsNone(notes.ir_change("", same))

    def test_support_changes_list_raised_new_and_removed_levels(self) -> None:
        old = "[format.step]\nlevel = 7\n[format.iges]\nlevel = 9\n[format.sat]\nlevel = 3\n"
        new = "[format.step]\nlevel = 8\n[format.iges]\nlevel = 9\n[format.nx]\nlevel = 1\n"
        self.assertEqual(
            notes.support_changes(old, new),
            ["`nx`: L1 (new)", "`sat`: no longer scored (was L3)", "`step`: L7 → L8"],
        )

    def test_missing_support_file_gives_no_changes(self) -> None:
        self.assertEqual(notes.support_changes(None, "[format.step]\nlevel = 8\n"), [])


class Render(unittest.TestCase):
    def render(self, changes: list, dependency_updates: int = 0, ir_line=None, support=()) -> str:
        return notes.render(
            "v1.1.0", "v1.0.0", "o/r", changes, dependency_updates, ir_line, list(support)
        )

    def test_routes_notes_and_orders_sections(self) -> None:
        changes = [
            notes.Change(
                "1" * 40,
                "f3d: a",
                number=1,
                login="zed",
                body="## Release note\n- Breaking: drop `--x`.\n- Faster joints.\n",
            ),
            notes.Change("2" * 40, "inventor: b", number=2, login="amy"),
            notes.Change("3" * 40, "inventor: c", number=3, login="Bob"),
            notes.Change("4" * 40, "ci: d", number=4, login="amy"),
            notes.Change("5" * 40, "e", number=5),
        ]
        text = self.render(changes, dependency_updates=2, ir_line="IR line.", support=["`step`: L7 → L8"])
        order = [
            "## Compatibility\n\n- IR line.\n- drop `--x`. (#1)",
            "## Highlights\n\n- Faster joints. (#1)",
            "## Support levels\n\n- `step`: L7 → L8",
            "<b>Inventor</b> (2)",
            "<b>Fusion</b> (1)",
            "<b>General</b> (1)",
            "<b>Project</b> (2)",
            "- 2 dependency updates",
            "## Contributors\n\n@amy @Bob @zed",
            "[All changes since v1.0.0](https://github.com/o/r/compare/v1.0.0...v1.1.0)",
        ]
        positions = [text.index(part) for part in order]
        self.assertEqual(positions, sorted(positions))

    def test_empty_release_has_only_the_footer(self) -> None:
        text = self.render([])
        self.assertTrue(text.startswith("[Install instructions](https://github.com/o/r#install)"))
        self.assertNotIn("##", text)

    def test_one_dependency_update_is_singular(self) -> None:
        self.assertIn("- 1 dependency update\n", self.render([], dependency_updates=1))


if __name__ == "__main__":
    unittest.main()
