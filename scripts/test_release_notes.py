#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unit tests for ``release-notes.py``."""

from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import tempfile
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

    def test_a_rewritten_commit_takes_the_merged_pull_request_its_subject_names(self) -> None:
        change = notes.Change(SHA, "f3d: x (#9)", number=9)
        commit = {
            "author": {"user": {"__typename": "User", "login": "committer"}},
            "associatedPullRequests": {"nodes": [pull(9, "b" * 40, "other", "other")]},
        }
        numbered = {
            "__typename": "PullRequest",
            "number": 9,
            "body": "## Breaking change\n- Drop `--x`.\n",
            "mergedAt": "2026-01-01T00:00:00Z",
            "author": {"__typename": "User", "login": "author"},
        }
        notes.apply_github(change, commit, numbered)
        self.assertEqual((change.number, change.login), (9, "author"))
        self.assertEqual(notes.breaking_notes(change.body), ["Drop `--x`."])

    def test_the_associated_pull_request_wins_over_the_subject_number(self) -> None:
        change = notes.Change(SHA, "f3d: x (#9)", number=9)
        commit = {"author": None, "associatedPullRequests": {"nodes": [pull(8, SHA, "body", "a")]}}
        numbered = {"__typename": "PullRequest", "number": 9, "body": "other", "mergedAt": "t"}
        notes.apply_github(change, commit, numbered)
        self.assertEqual((change.number, change.body), (8, "body"))

    def test_an_issue_or_open_pull_request_the_subject_names_is_not_taken(self) -> None:
        commit = {
            "author": {"user": {"__typename": "User", "login": "committer"}},
            "associatedPullRequests": {"nodes": []},
        }
        for numbered in (
            {"__typename": "Issue"},
            {"__typename": "PullRequest", "number": 9, "body": "b", "mergedAt": None},
            None,
        ):
            change = notes.Change(SHA, "f3d: x (#9)", number=9)
            notes.apply_github(change, commit, numbered)
            self.assertEqual((change.body, change.login), ("", "committer"))

    def test_bot_suffixed_login_is_not_a_contributor(self) -> None:
        self.assertIsNone(notes.login_of({"__typename": "User", "login": "helper[bot]"}))


class GraphqlData(unittest.TestCase):
    def test_a_subject_number_that_names_nothing_is_no_error(self) -> None:
        output = (
            '{"data": {"repository": {"c0": null, "p0": null}}, "errors": '
            '[{"type": "NOT_FOUND", "path": ["repository", "p0"], "message": "m"}]}'
        )
        self.assertIsNone(notes.graphql_data(output, "", "p")["repository"]["p0"])

    def test_every_other_error_refuses(self) -> None:
        for output in (
            '{"data": {"repository": {"c0": null}}, "errors": '
            '[{"type": "NOT_FOUND", "path": ["repository", "c0"], "message": "m"}]}',
            '{"data": null, "errors": [{"type": "RATE_LIMITED", "message": "m"}]}',
            "",
        ):
            with self.assertRaises(RuntimeError):
                notes.graphql_data(output, "HTTP 502", "p")


class Fragments(unittest.TestCase):
    def test_breaking_bullets_with_continuations_ignoring_comments_and_other_headings(self) -> None:
        texts = [
            "### Fixed\n\n- Ignored fix.\n<!-- - hidden -->\n### Breaking\n\n- Drop `--x`.\n  Continued.\n",
            "- Unheaded.\n\n### Breaking changes\n\nProse is no note.\n* Change `y`.\n### Added\n- Ignored add.\n",
        ]
        self.assertEqual(
            notes.fragment_breaking_notes(texts),
            ["Drop `--x`. Continued.", "Change `y`."],
        )

    def test_a_fragment_without_breaking_bullets_gives_no_notes(self) -> None:
        self.assertEqual(notes.fragment_breaking_notes(["### Fixed\n- Fix.\n", "### Breaking\n\nProse only.\n"]), [])

    def test_the_release_takes_only_the_fragments_it_adds(self) -> None:
        cwd = os.getcwd()
        with tempfile.TemporaryDirectory() as directory:
            os.chdir(directory)
            try:

                isolated = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}

                def git(*args: str) -> None:
                    subprocess.run(
                        ["git", "-c", "user.name=Test", "-c", "user.email=test@example.invalid", *args],
                        check=True,
                        stdout=subprocess.DEVNULL,
                        env=isolated,
                    )

                git("init", "-q")
                os.mkdir("changelog.d")
                Path("changelog.d/old.md").write_text("### Breaking\n- Old.\n")
                git("add", ".")
                git("commit", "-q", "-m", "one")
                git("tag", "v1.0.0")
                Path("changelog.d/new.md").write_text("### Breaking\n- New.\n")
                Path("changelog.d/README.md").write_text("- Form.\n")
                Path("changelog.d/old.md").write_text("### Breaking\n- Old, edited.\n")
                git("add", ".")
                git("commit", "-q", "-m", "two")
                git("tag", "v1.1.0")
                self.assertEqual(notes.added_fragments("v1.0.0", "v1.1.0"), ["### Breaking\n- New.\n"])
            finally:
                os.chdir(cwd)


class BreakingNotes(unittest.TestCase):
    def test_reads_lines_up_to_the_next_heading_without_comments(self) -> None:
        body = (
            "## Summary\r\n\r\nText.\r\n\r\n## Breaking change\r\n\r\n"
            "<!-- One line for users. -->\r\n- `--allow-empty` is removed.\r\n"
            "* Rename `--x` to `--y`.\r\n\r\n## Checklist\r\n- [x] Signed off\r\n"
        )
        self.assertEqual(
            notes.breaking_notes(body),
            ["`--allow-empty` is removed.", "Rename `--x` to `--y`."],
        )

    def test_none_and_placeholder_only_give_no_notes(self) -> None:
        self.assertEqual(notes.breaking_notes("## Breaking change\n\nNone.\n- n/a\n-\n"), [])
        self.assertEqual(
            notes.breaking_notes("## Breaking change\n<!--\n## Breaking change\nx\n-->\n"), []
        )

    def test_a_line_that_opens_with_a_placeholder_sentence_gives_no_note(self) -> None:
        body = (
            "## Breaking change\n\nNone. Faces now report area.\n- N/A. Nothing.\n"
            "- No change under default flags: `A::b` is removed.\n- Library: `C` is renamed.\n"
        )
        self.assertEqual(
            notes.breaking_notes(body),
            ["No change under default flags: `A::b` is removed.", "Library: `C` is renamed."],
        )

    def test_body_without_the_section_gives_no_notes(self) -> None:
        self.assertEqual(notes.breaking_notes("## Summary\n\nBreaking change: no heading.\n"), [])

    def test_old_release_note_heading_gives_no_notes(self) -> None:
        self.assertEqual(notes.breaking_notes("## Release note\n- Faster joints.\n"), [])


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
        self.assertIn("does not read IR 8 files: decode the source files again", line)

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
                body="## Breaking change\n- Breaking: drop `--x`.\n- Remove `--y`.\n",
            ),
            notes.Change("2" * 40, "inventor: b", number=2, login="amy"),
            notes.Change("3" * 40, "inventor: c", number=3, login="Bob"),
            notes.Change("4" * 40, "ci: d", number=4, login="amy"),
            notes.Change("5" * 40, "e", number=5),
        ]
        text = self.render(changes, dependency_updates=2, ir_line="IR line.", support=["`step`: L7 → L8"])
        order = [
            "## Compatibility\n\n- IR line.\n- drop `--x`. (#1)\n- Remove `--y`. (#1)",
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
        self.assertNotIn("Highlights", text)

    def test_curated_breaking_follows_the_ir_line_and_collapses_pr_notes(self) -> None:
        text = notes.render(
            "v1.1.0",
            "v1.0.0",
            "o/r",
            [notes.Change("1" * 40, "f3d: a", number=1, body="## Breaking change\n- PR note.\n")],
            0,
            "IR line.",
            ["`step`: L7 → L8"],
            ["Fragment note."],
        )
        order = [
            "## Compatibility\n\n- IR line.\n- Fragment note.\n\n<details>",
            "<b>Breaking changes by pull request</b> (1)</summary>\n\n- PR note. (#1)\n\n</details>",
            "## Support levels",
            "## Changes",
        ]
        positions = [text.index(part) for part in order]
        self.assertEqual(positions, sorted(positions))
        self.assertNotIn("## Breaking", text)

    def test_empty_release_has_only_the_footer(self) -> None:
        text = self.render([])
        self.assertTrue(text.startswith("[Install instructions](https://github.com/o/r#install)"))
        self.assertNotIn("##", text)

    def test_one_dependency_update_is_singular(self) -> None:
        self.assertIn("- 1 dependency update\n", self.render([], dependency_updates=1))


if __name__ == "__main__":
    unittest.main()
