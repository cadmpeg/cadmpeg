#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Test issue claims with an in-memory REST API."""

from __future__ import annotations

import copy
import io
import json
import os
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch
from urllib.error import HTTPError, URLError

from scripts import issue_claim as bot

START = datetime(2026, 1, 1, tzinfo=timezone.utc)


def comment(user="alice", body="", at=START, kind="User", number=100):
    return {"id": number, "user": {"login": user, "type": kind}, "body": body,
            "created_at": bot.iso(at), "updated_at": bot.iso(at)}


def bot_comment(body):
    return comment(bot.BOT, body, kind="Bot")


def issue(number=1, assignees=(), state="open", labelled=None):
    return {"number": number, "state": state, "created_at": bot.iso(START), "locked": False,
            "assignees": [{"login": name} for name in assignees],
            "labels": [{"name": "claimed"}] if (assignees if labelled is None else labelled) else []}


def pull(number=10, user="alice", state="open", draft=True, body="Fixes #1", at=START):
    return {"number": number, "user": {"login": user, "type": "User"}, "state": state,
            "draft": draft, "title": "A fix", "body": body, "created_at": bot.iso(at),
            "updated_at": bot.iso(at), "closed_at": None, "merged_at": None}


def review(at=START, state="CHANGES_REQUESTED", user="maintainer"):
    return {"user": {"login": user, "type": "User"}, "state": state, "submitted_at": bot.iso(at)}


def commit(sha="abc", author="alice", committer="alice", at=START):
    return {"sha": sha, "author": {"login": author}, "committer": {"login": committer},
            "commit": {"committer": {"date": bot.iso(at)}}}


class FakeAPI:
    repository = "cadmpeg/cadmpeg"
    repo_url = "https://api.github.com/repos/cadmpeg/cadmpeg"

    def __init__(self):
        self.issues = {1: issue()}
        self.pulls = {}
        self.rows = {}
        self.external = {}
        self.failures = {}
        self.calls = []
        self.label = False
        self.refuse_assignment = False
        self.permissions = {"maintainer": {"permission": "write"}}
        self.reactions = []

    def add_pull(self, item, target=1, external=False):
        self.pulls[item["number"]] = item
        url = ("https://api.github.com/repos/other/repo" if external else self.repo_url)
        self.rows.setdefault(f"/issues/{target}/timeline", []).append({
            "event": "cross-referenced", "source": {"issue": {
                "number": item["number"], "url": f"{url}/issues/{item['number']}", "pull_request": {}}}})

    def items(self, path, **params):
        self.calls.append(("LIST", path, params))
        if ("LIST", path) in self.failures:
            return self.failures[("LIST", path)]
        if path == "/issues":
            values = [item for item in self.issues.values() if item["state"] == "open"
                      and any(label["name"] == "claimed" for label in item.get("labels", []))]
            return copy.deepcopy(values)
        if path == "/pulls":
            raise AssertionError("sweep and events use issue-scoped timelines")
        return copy.deepcopy(self.rows.get(path, []))

    def request(self, method, path, data=None, expected=(), repository=None):
        self.calls.append((method, path, copy.deepcopy(data)))
        if (method, path) in self.failures:
            result = self.failures[(method, path)]
            if isinstance(result, Exception):
                raise result
            return copy.deepcopy(result)
        if repository and repository != self.repository:
            return copy.deepcopy(self.external.get((repository, path)))
        if path == "/labels/claimed":
            return {"name": "claimed"} if self.label else None
        if path == "/labels":
            self.label = True
            return data
        if path.startswith("/collaborators/"):
            return copy.deepcopy(self.permissions.get(path.split("/")[2], {"permission": "read"}))
        if path.endswith("/reactions"):
            self.reactions.append(data["content"])
            return {"id": len(self.reactions)}
        parts = path.strip("/").split("/")
        if parts[0] == "pulls":
            return copy.deepcopy(self.pulls.get(int(parts[1])))
        if parts[0] != "issues":
            raise AssertionError((method, path))
        number = int(parts[1])
        item = self.issues.get(number)
        if len(parts) == 2:
            return copy.deepcopy(item)
        if parts[2] == "comments" and method == "POST":
            self.rows.setdefault(path, []).append(bot_comment(data["body"]))
            return {"id": len(self.rows[path])}
        if parts[2] == "assignees":
            if method == "POST":
                if self.refuse_assignment:
                    return None
                item["assignees"] = [{"login": name} for name in data["assignees"]]
            elif method == "DELETE":
                item["assignees"] = [entry for entry in item["assignees"] if entry["login"] not in data["assignees"]]
            return copy.deepcopy(item)
        if parts[2] == "labels":
            item["labels"] = [{"name": "claimed"}] if method == "POST" else []
            return copy.deepcopy(item["labels"])
        raise AssertionError((method, path))


class Parsing(unittest.TestCase):
    def test_command_parsing(self):
        for body, expected in [("/claim", "/claim"), (" \n\t /ClAiM \r\nDetails", "/claim"),
                               ("/UNCLAIM\nThanks", "/unclaim"), ("\n /unclaim ", "/unclaim")]:
            with self.subTest(body=body):
                self.assertEqual(bot.parse_command(body), expected)
        for body in (None, "", " \n", "> /claim", "Reply\n/claim", "```\n/claim\n```",
                     "~~~\n/unclaim\n~~~", "`/claim`", "/claim please", "/claim!", "/claimer",
                     "/unclaim @alice", "<!-- /claim -->"):
            with self.subTest(body=body):
                self.assertIsNone(bot.parse_command(body))

    def test_link_phrases_title_body_and_limit(self):
        self.assertEqual(bot.linked_issues("Fixes #1\nCLOSES #2; resolves #3. Part OF #4; fixes #1"), {1, 2, 3, 4})
        self.assertEqual(bot.linked_issues("Fixes other/repo#5; Closes cadmpeg/cadmpeg#6; Resolves https://github.com/other/repo/issues/7; Part of #0; fixes #8abc"), set())
        self.assertEqual(len(bot.linked_issues(" ".join(f"Fixes #{n}" for n in range(1, 100)))), bot.MAX_LINKED_ISSUES)

    def test_code_and_quotes_are_stripped_before_references(self):
        body = "Fixes #1\n```text\nCloses #2\nDepends on #20\n```\n> Resolves #3\n> Stacked on #21\n`Part of #4`\n~~~/text\nFixes #5\n~~~\n``Depends on #22``"
        self.assertEqual(bot.linked_issues(body), {1})
        self.assertEqual(bot.dependencies(body, "cadmpeg/cadmpeg"), set())

    def test_dependency_forms(self):
        values = bot.dependencies("Depends on #20\nSTACKED ON other/repo#21\nDepends on https://github.com/team/project/issues/22\nStacked on https://github.com/team/project/pull/23", "cadmpeg/cadmpeg")
        self.assertEqual(values, {bot.Dependency("cadmpeg/cadmpeg", 20), bot.Dependency("other/repo", 21), bot.Dependency("team/project", 22), bot.Dependency("team/project", 23)})
        self.assertEqual(bot.dependencies("Depends on https://evil.test/a/b/issues/3; Depends on #0", "a/b"), set())

    def test_tracking_label_detection(self):
        for labels, expected in [([], False), ([{"name": "tracking"}], True),
                                 ([{"name": "Tracking"}, {"name": "claimed"}], True),
                                 ([{"name": "tracking-work"}], False)]:
            with self.subTest(labels=labels):
                self.assertEqual(bot.is_tracking({"labels": labels, "body": None}), expected)

    def test_tracking_checklist_detection_with_issue_numbers_and_urls(self):
        for body in ("- [ ] #2\n- [x] #3", "  - [X] First: #2\n\t- [ ] Second: #3",
                     "- [ ] #2\n- [ ] [Next](https://github.com/cadmpeg/cadmpeg/issues/3)",
                     "- [ ] #2\n- [ ] https://github.com/cadmpeg/cadmpeg/issues/3#issuecomment-42",
                     "- [ ] #2\n- [ ] https://github.com/cadmpeg/cadmpeg/issues/3/",
                     "- [x] https://github.com/cadmpeg/cadmpeg/issues/2\n- [x] https://github.com/other/repo/issues/3"):
            with self.subTest(body=body):
                self.assertTrue(bot.is_tracking({"body": body, "labels": []}))

    def test_tracking_needs_two_issue_linked_task_items_in_prose(self):
        for body in (None, "#2 and #3", "- [ ] #2 and #3", "- [ ] #2\n- [x] Write docs",
                     "- [ ] #2\n- [ ] https://github.com/cadmpeg/cadmpeg/pull/3",
                     "- [ ] #0\n- [ ] #3", "- [ ] #2abc\n- [ ] #3",
                     "```\n- [ ] #2\n- [ ] #3\n```", "> - [ ] #2\n> - [ ] #3",
                     "- [ ] `#2`\n- [ ] #3", "- [ ] #2\n`- [ ] #3`"):
            with self.subTest(body=body):
                self.assertFalse(bot.is_tracking({"body": body}))

    def test_marker_round_trip(self):
        for claim in (bot.Claim("some-user", START), bot.Claim("alice", START + timedelta(microseconds=123), 10)):
            self.assertEqual(bot.read_claim_marker("text\n" + bot.claim_marker(claim)), claim)
        for body in ("claimed by alice", "<!-- cadmpeg-claim: user=alice at=bad -->", "<!-- cadmpeg-claim: user=alice at=2026-01-01T00:00:00 -->"):
            self.assertIsNone(bot.read_claim_marker(body))
        with self.assertRaises(ValueError):
            bot.claim_marker(bot.Claim("alice", datetime(2026, 1, 1)))
        with self.assertRaises(ValueError):
            bot.claim_marker(bot.Claim("bad\nlogin", START))

    def test_latest_own_marker_release_and_assignment_change(self):
        item = issue(labelled=True)
        old = bot_comment(bot.claim_marker(bot.Claim("alice", START)))
        new = bot_comment(bot.claim_marker(bot.Claim("bob", START + timedelta(hours=1))))
        forged = comment("mallory", bot.claim_marker(bot.Claim("mallory", START + timedelta(days=1))))
        self.assertEqual(bot.current_claim(item, [old, new, forged], []).user, "bob")
        released = bot_comment("<!-- cadmpeg-claim-release: at=2026-01-01T02:00:00Z -->")
        self.assertIsNone(bot.current_claim(item, [old, new, released, forged], []))
        changed = [{"event": "unassigned", "created_at": bot.iso(START + timedelta(hours=2))}]
        self.assertIsNone(bot.current_claim(item, [old, new], changed))


class Clock(unittest.TestCase):
    def test_draft_reminder_and_lapse_boundaries(self):
        claim = bot.Claim("alice", START)
        for hours, expected in [(11.999, "active"), (12, "warn"), (23.999, "warn"), (24, "lapse")]:
            self.assertEqual(bot.lapse_decision(claim, START + timedelta(hours=hours)).action, expected)
        self.assertEqual(bot.lapse_decision(claim, START + timedelta(days=2), activity=START + timedelta(days=2)).action, "lapse")

    def test_idle_warning_and_lapse_boundaries(self):
        claim = bot.Claim("alice", START)
        for days, expected in [(3.999, "active"), (4, "warn"), (4.999, "warn"), (5, "lapse")]:
            self.assertEqual(bot.lapse_decision(claim, START + timedelta(days=days), True).action, expected)
        self.assertEqual(bot.lapse_decision(claim, START + timedelta(days=10), True, waiting=True).action, "hold")


class Actions(unittest.TestCase):
    def setUp(self):
        self.api = FakeAPI()
        self.now = START + timedelta(hours=24)

    def run_comment(self, body="/claim", user="alice", at=None, number=1, kind="User"):
        event = {"issue": {"number": number}, "comment": comment(user, body, at or self.now, kind)}
        bot.ClaimBot(self.api, at or self.now).on_comment(event)

    def claimed(self, user="alice", at=START, pr=None, assigned=True):
        self.api.issues[1] = issue(assignees=[user] if assigned else [], labelled=True)
        self.api.rows["/issues/1/comments"] = [bot_comment(bot.claim_marker(bot.Claim(user, at, pr)))]

    def sweep(self, at=None):
        bot.ClaimBot(self.api, at or self.now).sweep()

    def messages(self, number=1):
        return [entry["body"] for entry in self.api.rows.get(f"/issues/{number}/comments", [])]

    def held(self):
        return any(label["name"] == "claimed" for label in self.api.issues[1]["labels"])

    def pr_event(self, number=10, action="opened", actor=None):
        value = self.api.pulls[number]
        return {"pull_request": {key: copy.deepcopy(value[key]) for key in ("number", "title", "body", "user")},
                "action": action, "sender": {"login": actor or value["user"]["login"], "type": "User"},
                "repository": {"full_name": self.api.repository}}

    def test_free_claim_assigns_labels_reacts_and_replies(self):
        self.run_comment("\n /CLAIM \nI will start")
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "alice"}])
        self.assertTrue(self.held())
        self.assertTrue(self.api.label)
        self.assertEqual(self.api.reactions, ["+1"])
        self.assertIn("24 hours", self.messages()[0])
        self.assertIn("5 quiet days", self.messages()[0])
        self.assertIn("/unclaim", self.messages()[0])
        self.assertEqual(bot.read_claim_marker(self.messages()[0]), bot.Claim("alice", self.now))

    def test_claim_on_tracking_issue_points_to_sub_issues(self):
        for by_label in (True, False):
            with self.subTest(by_label=by_label):
                self.setUp()
                if by_label:
                    self.api.issues[1]["labels"] = [{"name": "tracking"}]
                else:
                    self.api.issues[1]["body"] = "- [ ] #2\n- [x] #3"
                before = copy.deepcopy(self.api.issues[1])
                self.run_comment()
                self.assertEqual(self.api.issues[1], before)
                self.assertEqual(len(self.messages()), 1)
                self.assertIn("tracking issue", self.messages()[0])
                self.assertIn("sub-issues", self.messages()[0])
                self.assertIn("/claim there", self.messages()[0])
                self.assertIsNone(bot.read_claim_marker(self.messages()[0]))
                self.assertFalse(any(method == "LIST" for method, _, _ in self.api.calls))

    def test_part_of_tracking_issue_claims_nothing_and_posts_no_conflicts(self):
        for by_label in (True, False):
            for phrase in ("Part of", "Fixes", "Closes", "Resolves"):
                with self.subTest(by_label=by_label, phrase=phrase):
                    self.setUp()
                    if by_label:
                        self.api.issues[1]["labels"] = [{"name": "tracking"}]
                    else:
                        self.api.issues[1]["body"] = "- [ ] #2\n- [x] #3"
                    self.api.add_pull(pull(body=f"{phrase} #1"))
                    before = copy.deepcopy(self.api.issues[1])
                    bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
                    self.assertEqual(self.api.issues[1], before)
                    self.assertFalse(self.messages())
                    self.assertFalse(self.messages(10))
                    self.assertEqual(self.api.calls, [("GET", "/issues/1", None)])

    def test_tracking_issue_with_existing_holder_produces_no_pr_conflicts(self):
        self.claimed("bob")
        self.api.issues[1]["labels"].append({"name": "tracking"})
        self.api.add_pull(pull(body="Part of #1"))
        self.api.add_pull(pull(20, "carol", body="Part of #1"))
        before = copy.deepcopy((self.api.issues, self.api.rows))
        for number in (10, 20):
            bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event(number))
        self.assertEqual((self.api.issues, self.api.rows), before)
        self.assertFalse(self.messages(10))
        self.assertFalse(self.messages(20))

    def test_part_of_normal_issue_still_claims_for_pr_author(self):
        self.api.add_pull(pull(body="Part of #1"))
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "alice"}])
        self.assertTrue(self.held())
        self.assertEqual(bot.read_claim_marker(self.messages()[-1]).pr, 10)

    def test_fixes_sub_issue_and_part_of_tracker_claims_only_sub_issue(self):
        for by_label in (True, False):
            with self.subTest(by_label=by_label):
                self.setUp()
                self.api.issues[2] = issue(2)
                if by_label:
                    self.api.issues[2]["labels"] = [{"name": "tracking"}]
                else:
                    self.api.issues[2]["body"] = "- [ ] #1\n- [x] #3"
                tracker = copy.deepcopy(self.api.issues[2])
                self.api.add_pull(pull(body="Fixes #1\nPart of #2"))
                bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
                self.assertEqual(self.api.issues[1]["assignees"], [{"login": "alice"}])
                self.assertTrue(self.held())
                self.assertEqual(self.api.issues[2], tracker)
                self.assertFalse(self.messages(2))
                self.assertFalse(self.messages(10))

    def test_assignment_refusal_keeps_marker_claim_label_and_reply(self):
        self.api.refuse_assignment = True
        self.run_comment()
        self.assertFalse(self.api.issues[1]["assignees"])
        self.assertTrue(self.held())
        self.assertIn("could not assign", self.messages()[0])
        self.assertEqual(bot.read_claim_marker(self.messages()[0]).user, "alice")
        self.sweep(self.now + timedelta(hours=12))
        self.assertIn("cadmpeg-claim-warning", self.messages()[-1])
        self.sweep(self.now + timedelta(hours=24))
        self.assertFalse(self.held())

    def test_current_claimant_reclaims_with_quiet_thumbs_up(self):
        self.claimed()
        self.run_comment(user="ALICE")
        self.assertEqual(len(self.messages()), 1)
        self.assertEqual(self.api.reactions, ["+1"])

    def test_other_claimant_reply_names_holder_and_lapse_date(self):
        self.claimed()
        self.run_comment(user="bob", at=START + timedelta(hours=6))
        self.assertIn("@alice since 2026-01-01", self.messages()[-1])
        self.assertIn("2026-01-02 00:00 UTC", self.messages()[-1])
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "alice"}])

    def test_lapsed_claim_is_taken_over_before_sweep(self):
        self.claimed()
        self.run_comment(user="bob")
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "bob"}])
        self.assertEqual(bot.read_claim_marker(self.messages()[-1]).user, "bob")

    def test_recent_linked_pr_prevents_takeover(self):
        self.claimed(pr=10)
        self.api.add_pull(pull(at=START + timedelta(hours=12)))
        self.run_comment(user="bob")
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "alice"}])
        self.assertIn("Already claimed", self.messages()[-1])

    def test_maintainer_and_owner_claims_are_protected(self):
        for user in ("maintainer", "cadmpeg"):
            with self.subTest(user=user):
                self.claimed(user)
                self.run_comment(user="bob", at=START + timedelta(days=30))
                self.sweep(START + timedelta(days=40))
                self.assertEqual(self.api.issues[1]["assignees"], [{"login": user}])
                self.assertIn("protected", self.messages()[-1])

    def test_unclaimed_issue_with_other_open_pr_points_to_pr(self):
        self.api.add_pull(pull(user="bob"))
        self.run_comment()
        self.assertFalse(self.api.issues[1]["assignees"])
        self.assertIn("@bob already has open PR #10", self.messages()[-1])

    def test_body_cross_reference_does_not_block_comment_claim(self):
        self.api.add_pull(pull(user="bob", body="See #1 for context"))
        self.run_comment()
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "alice"}])

    def test_ignored_actors_issue_states_and_other_comments(self):
        for mode in ("bot", "locked", "closed", "pull", "other", "transferred"):
            with self.subTest(mode=mode):
                self.setUp()
                event = {"issue": {"number": 1}, "comment": comment(body="/claim")}
                if mode == "bot":
                    event["comment"]["user"] = {"login": bot.BOT, "type": "Bot"}
                elif mode == "locked":
                    self.api.issues[1]["locked"] = True
                elif mode == "closed":
                    self.api.issues[1]["state"] = "closed"
                elif mode == "pull":
                    event["issue"]["pull_request"] = {}
                elif mode == "other":
                    event["comment"]["body"] = "Hello\n/claim"
                else:
                    self.api.issues[1]["repository_url"] = "https://api.github.com/repos/other/repo"
                bot.ClaimBot(self.api, self.now).on_comment(event)
                self.assertFalse(self.messages())
                self.assertFalse(self.api.issues[1]["assignees"])

    def test_unclaim_by_claimant_and_maintainer(self):
        for user in ("alice", "maintainer"):
            with self.subTest(user=user):
                self.claimed()
                self.run_comment("/unclaim", user)
                self.assertFalse(self.held())
                self.assertFalse(self.api.issues[1]["assignees"])
                self.assertIn("released", self.messages()[-1])

    def test_unclaim_stranger_and_permission_404_react_eyes(self):
        self.claimed()
        self.api.failures[("GET", "/collaborators/bob/permission")] = None
        self.run_comment("/unclaim", "bob")
        self.assertTrue(self.held())
        self.assertEqual(self.api.reactions, ["eyes"])
        self.assertEqual(len(self.messages()), 1)

    def test_unassignable_claimant_releases_and_stranger_cannot(self):
        self.claimed(assigned=False)
        self.run_comment("/unclaim", "bob")
        self.assertTrue(self.held())
        self.run_comment("/unclaim", "alice")
        self.assertFalse(self.held())
        self.assertEqual(self.api.reactions, ["eyes", "+1"])

    def test_owner_assignment_releases_only_for_owner(self):
        self.api.issues[1] = issue(assignees=["cadmpeg"])
        self.run_comment("/unclaim", "maintainer")
        self.assertTrue(self.held())
        self.run_comment("/unclaim", "cadmpeg")
        self.assertFalse(self.held())

    def test_no_pr_at_24_hours_lapses(self):
        self.claimed()
        self.sweep()
        self.assertFalse(self.held())
        self.assertIn("24-hour draft deadline", self.messages()[-1])

    def test_reminder_at_12_hours_once(self):
        self.claimed()
        self.sweep(START + timedelta(hours=12))
        self.sweep(START + timedelta(hours=13))
        warnings = [text for text in self.messages() if "cadmpeg-claim-warning" in text]
        self.assertEqual(len(warnings), 1)
        self.assertIn("@alice", warnings[0])

    def test_young_claim_and_fresh_pr_use_latest_claim_time(self):
        self.claimed(at=START + timedelta(hours=20))
        self.api.rows["/issues/1/comments"].insert(0, bot_comment(bot.claim_marker(bot.Claim("alice", START))))
        self.sweep()
        self.assertTrue(self.held())
        self.api.add_pull(pull(at=self.now))
        self.sweep(self.now + timedelta(days=1))
        self.assertTrue(self.held())

    def test_pr_opened_before_deadline_switches_to_idle_clock(self):
        self.claimed(pr=10)
        opened = START + timedelta(hours=12)
        self.api.add_pull(pull(at=opened))
        self.sweep(START + timedelta(days=1))
        self.assertTrue(self.held())
        self.sweep(opened + timedelta(days=5))
        self.assertFalse(self.held())

    def test_idle_five_days_lapses_and_preserves_pr_for_continuation(self):
        self.claimed(pr=10)
        self.api.add_pull(pull())
        before = copy.deepcopy(self.api.pulls[10])
        self.sweep(START + timedelta(days=5))
        self.assertFalse(self.held())
        self.assertEqual(self.api.pulls[10], before)
        self.assertIn("5 quiet days", self.messages()[-1])
        self.assertIn("/cadmpeg/cadmpeg/pull/10", self.messages()[-1])
        self.assertIn("@alice as co-author", self.messages()[-1])
        self.assertIn("Co-authored-by:", self.messages()[-1])
        self.assertIn("fork the PR", self.messages()[-1])

    def test_idle_warning_day_four_once_per_quiet_period(self):
        self.claimed(pr=10)
        self.api.add_pull(pull())
        self.sweep(START + timedelta(days=4))
        self.sweep(START + timedelta(days=4, hours=1))
        self.assertEqual(sum("cadmpeg-claim-warning" in text for text in self.messages()), 1)
        update = START + timedelta(days=4, hours=2)
        self.api.rows["/issues/1/comments"].append(comment(body="Progress", at=update))
        self.sweep(update + timedelta(days=4))
        self.assertEqual(sum("cadmpeg-claim-warning" in text for text in self.messages()), 2)

    def test_closed_unmerged_pr_starts_new_draft_clock_at_close(self):
        self.claimed(pr=10)
        value = pull(state="closed")
        closed = START + timedelta(days=2)
        value["closed_at"] = bot.iso(closed)
        self.api.add_pull(value)
        self.sweep(closed + timedelta(hours=12))
        self.assertTrue(self.held())
        self.sweep(closed + timedelta(hours=24))
        self.assertFalse(self.held())

    def test_incidental_stranger_pr_and_dependency_do_not_extend_claim(self):
        self.claimed()
        self.api.add_pull(pull(user="bob", body="See #1\nDepends on #20", at=self.now))
        self.api.issues[20] = issue(20)
        self.sweep()
        self.assertFalse(self.held())

    def test_hourly_sweep_idempotence_and_cheap_selection(self):
        self.claimed()
        self.sweep()
        snapshot = copy.deepcopy((self.api.issues, self.api.rows))
        self.sweep(self.now + timedelta(hours=1))
        self.sweep(self.now + timedelta(hours=2))
        self.assertEqual((self.api.issues, self.api.rows), snapshot)
        selections = [call for call in self.api.calls if call[0] == "LIST" and call[1] == "/issues"]
        self.assertEqual([call[2] for call in selections], [{"state": "open", "labels": "claimed"}] * 3)
        self.assertFalse(any(call[1] == "/pulls" for call in self.api.calls))

    def test_ready_pr_waiting_on_review_never_lapses(self):
        self.claimed(pr=10)
        self.api.add_pull(pull(draft=False))
        self.sweep(START + timedelta(days=100))
        self.assertTrue(self.held())
        self.assertEqual(len(self.messages()), 1)

    def test_dependency_open_issue_pr_qualified_and_url_holds(self):
        for ref, repo, number, is_pr in [("#20", "cadmpeg/cadmpeg", 20, False),
                ("other/repo#21", "other/repo", 21, False),
                ("https://github.com/other/repo/issues/22", "other/repo", 22, False),
                ("https://github.com/other/repo/pull/23", "other/repo", 23, True)]:
            with self.subTest(ref=ref):
                self.setUp()
                self.claimed(pr=10)
                self.api.add_pull(pull(body="Fixes #1\nDepends on " + ref))
                dep = issue(number)
                if is_pr:
                    dep["pull_request"] = {}
                if repo == self.api.repository:
                    self.api.issues[number] = dep
                else:
                    self.api.external[(repo, f"/issues/{number}")] = dep
                self.sweep(START + timedelta(days=20))
                self.assertTrue(self.held())
                dep["state"] = "closed"
                self.sweep(START + timedelta(days=20))
                self.assertFalse(self.held())

    def test_changes_requested_and_five_quiet_days_lapses(self):
        self.claimed(pr=10)
        self.api.add_pull(pull(draft=False))
        returned = START + timedelta(days=1)
        self.api.rows["/pulls/10/reviews"] = [review(at=returned)]
        self.sweep(returned + timedelta(days=4))
        self.assertTrue(self.held())
        self.sweep(returned + timedelta(days=5))
        self.assertFalse(self.held())

    def test_maintainer_pr_and_inline_comments_return_ball_to_claimant(self):
        for path in ("/issues/10/comments", "/pulls/10/comments"):
            with self.subTest(path=path):
                self.setUp()
                self.claimed(pr=10)
                self.api.add_pull(pull(draft=False))
                returned = START + timedelta(days=1)
                self.api.rows[path] = [comment("maintainer", "Please update", returned)]
                self.sweep(returned + timedelta(days=5))
                self.assertFalse(self.held())

    def test_claimant_reply_after_maintainer_comment_waits_on_maintainers(self):
        self.claimed(pr=10)
        self.api.add_pull(pull(draft=False))
        self.api.rows["/issues/10/comments"] = [comment("maintainer", "Please update", START + timedelta(days=1)),
                                                comment("alice", "Done", START + timedelta(days=2))]
        self.sweep(START + timedelta(days=100))
        self.assertTrue(self.held())

    def test_claimant_issue_and_pr_comment_reset_idle_clock(self):
        for path in ("/issues/1/comments", "/issues/10/comments", "/pulls/10/comments"):
            with self.subTest(path=path):
                self.setUp()
                self.claimed(pr=10)
                self.api.add_pull(pull())
                self.api.rows.setdefault(path, []).append(comment(at=START + timedelta(days=4)))
                self.sweep(START + timedelta(days=5))
                self.assertTrue(self.held())

    def test_only_claimant_commit_author_or_committer_resets_clock(self):
        for author, committer, kept in [("maintainer", "robot", False), ("alice", "robot", True), ("robot", "alice", True)]:
            with self.subTest(author=author, committer=committer):
                self.setUp()
                self.claimed(pr=10)
                self.api.add_pull(pull())
                self.api.rows["/pulls/10/commits"] = [commit(author=author, committer=committer, at=START + timedelta(days=4))]
                self.sweep(START + timedelta(days=5))
                self.assertEqual(self.held(), kept)

    def test_pr_events_write_no_bookkeeping_comments(self):
        self.claimed(pr=10)
        self.api.add_pull(pull())
        self.api.rows["/pulls/10/commits"] = [commit()]
        for action in ("opened", "edited", "reopened", "synchronize"):
            with self.subTest(action=action):
                bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event(action=action))
        self.assertFalse(self.messages(10))
        self.assertEqual(len(self.messages()), 1)
        self.assertFalse(any(method == "POST" and path == "/issues/10/comments"
                             for method, path, _ in self.api.calls))

    def test_force_push_events_count_only_claimant_and_use_api_time(self):
        self.claimed(pr=10)
        self.api.add_pull(pull())
        for endpoint in ("timeline", "events"):
            for actor, kept in (("alice", True), ("maintainer", False), (bot.BOT, False)):
                with self.subTest(endpoint=endpoint, actor=actor):
                    self.api.rows[f"/issues/10/{endpoint}"] = [{
                        "event": "head_ref_force_pushed", "actor": {"login": actor},
                        "created_at": bot.iso(START + timedelta(days=4))}]
                    activity, _, _ = bot.ClaimBot(self.api, self.now).pull_activity(self.api.pulls[10], "alice")
                    self.assertEqual(activity, START + timedelta(days=4) if kept else START)
            self.api.rows[f"/issues/10/{endpoint}"] = []

    def test_commit_author_and_committer_dates_are_read_without_markers(self):
        self.api.add_pull(pull())
        for role in ("author", "committer"):
            with self.subTest(role=role):
                entry = commit()
                entry["commit"] = {role: {"date": bot.iso(START + timedelta(days=4))}}
                self.api.rows["/pulls/10/commits"] = [entry]
                activity, _, _ = bot.ClaimBot(self.api, self.now).pull_activity(self.api.pulls[10], "alice")
                self.assertEqual(activity, START + timedelta(days=4))

    def test_timeline_review_and_claimant_reply_change_review_hold(self):
        self.api.add_pull(pull(draft=False))
        self.api.rows["/issues/10/timeline"] = [{
            "event": "reviewed", "user": {"login": "maintainer"}, "state": "changes_requested",
            "submitted_at": bot.iso(START + timedelta(days=1))}]
        activity, waiting, returned = bot.ClaimBot(self.api, self.now).pull_activity(self.api.pulls[10], "alice")
        self.assertEqual(activity, START)
        self.assertFalse(waiting)
        self.assertEqual(returned, START + timedelta(days=1))
        self.api.rows["/issues/10/timeline"].append({
            "event": "commented", "user": {"login": "alice"},
            "created_at": bot.iso(START + timedelta(days=2))})
        activity, waiting, _ = bot.ClaimBot(self.api, self.now).pull_activity(self.api.pulls[10], "alice")
        self.assertEqual(activity, START + timedelta(days=2))
        self.assertTrue(waiting)
        self.api.rows["/issues/10/timeline"].append({
            "event": "commented", "actor": {"login": "maintainer"},
            "created_at": bot.iso(START + timedelta(days=3))})
        _, waiting, returned = bot.ClaimBot(self.api, self.now).pull_activity(self.api.pulls[10], "alice")
        self.assertFalse(waiting)
        self.assertEqual(returned, START + timedelta(days=3))

    def test_ready_for_review_event_restores_review_hold(self):
        self.api.add_pull(pull(draft=False))
        self.api.rows["/pulls/10/reviews"] = [review(START + timedelta(days=1))]
        self.api.rows["/issues/10/events"] = [{
            "event": "ready_for_review", "actor": {"login": "alice"},
            "created_at": bot.iso(START + timedelta(days=2))}]
        activity, waiting, _ = bot.ClaimBot(self.api, self.now).pull_activity(self.api.pulls[10], "alice")
        self.assertEqual(activity, START + timedelta(days=2))
        self.assertTrue(waiting)

    def test_pr_claims_free_issue_for_author(self):
        self.api.add_pull(pull())
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "alice"}])
        self.assertEqual(bot.read_claim_marker(self.messages()[-1]).pr, 10)

    def test_pr_conflict_comments_once_and_changes_nothing(self):
        self.claimed("bob")
        self.api.add_pull(pull())
        before = copy.deepcopy(self.api.issues[1])
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        self.assertEqual(self.api.issues[1], before)
        self.assertEqual(sum("cadmpeg-claim-conflict" in text for text in self.messages(10)), 1)
        self.assertIn("@bob since 2026-01-01", self.messages(10)[-1])

    def test_lowest_open_pr_wins_unrecorded_claim_and_later_pr_gets_one_pointer(self):
        self.api.add_pull(pull(20, "alice"))
        self.api.add_pull(pull(10, "bob"))
        for _ in range(2):
            bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event(20))
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "bob"}])
        conflicts = [text for text in self.messages(20) if "cadmpeg-claim-conflict" in text]
        self.assertEqual(len(conflicts), 1)
        self.assertIn("PR #10", conflicts[0])

    def test_lower_pr_replaces_later_automatic_claim(self):
        self.claimed("alice", pr=20)
        self.api.add_pull(pull(20, "alice"))
        self.api.add_pull(pull(10, "bob"))
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event(10))
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "bob"}])

    def test_incidental_pr_mention_does_not_block_actual_fix(self):
        self.api.add_pull(pull(9, "bob", body="See #1"))
        self.api.add_pull(pull(10, "alice"))
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "alice"}])

    def test_older_pr_body_link_reserves_issue(self):
        self.api.add_pull(pull(9, "bob", body="Part of #1"))
        self.api.add_pull(pull(10, "alice", body="", at=self.now))
        self.api.pulls[10]["title"] = "Fixes #1"
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "bob"}])

    def test_other_repository_same_number_cannot_reserve_issue(self):
        self.api.add_pull(pull(9, "bob"), external=True)
        self.api.add_pull(pull(10, "alice"))
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "alice"}])
        self.setUp()
        self.api.add_pull(pull())
        event = self.pr_event()
        event["repository"]["full_name"] = "other/repo"
        bot.ClaimBot(self.api, self.now).on_pull_request(event)
        self.assertFalse(self.api.calls)

    def test_claimant_own_pr_ensures_label_without_new_claim_reply(self):
        self.claimed()
        self.api.issues[1]["labels"] = []
        self.api.add_pull(pull())
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        self.assertTrue(self.held())
        self.assertEqual(len(self.messages()), 1)

    def test_bot_pr_and_pr_targets_closed_and_missing_issues_are_ignored(self):
        self.api.add_pull(pull(body="Fixes #2; Closes #3; Resolves #404"))
        self.api.issues[2] = {**issue(2), "pull_request": {}}
        self.api.issues[3] = issue(3, state="closed")
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        self.assertFalse(self.api.issues[1]["assignees"])
        self.api.pulls[10]["user"] = {"login": "robot[bot]", "type": "Bot"}
        count = len(self.api.calls)
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        self.assertEqual(len(self.api.calls), count)

    def test_unlinked_pr_event_does_not_scan_claims_or_timelines(self):
        self.api.add_pull(pull(body="Routine work"))
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event())
        self.assertFalse(self.api.calls)

    def test_reopened_pr_counts_again(self):
        self.claimed(pr=10)
        value = pull(state="closed")
        value["closed_at"] = bot.iso(START + timedelta(hours=1))
        self.api.add_pull(value)
        self.sweep(START + timedelta(hours=12))
        self.assertTrue(self.held())
        value["state"] = "open"
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event(action="reopened"))
        self.sweep(self.now + timedelta(days=1))
        self.assertTrue(self.held())

    def test_pr_converted_to_draft_and_back(self):
        self.claimed(pr=10)
        value = pull(draft=False)
        self.api.add_pull(value)
        self.sweep(START + timedelta(days=5))
        self.assertTrue(self.held())
        value["draft"] = True
        self.sweep(START + timedelta(days=4))
        self.assertIn("cadmpeg-claim-warning", self.messages()[-1])
        value["draft"] = False
        self.sweep(START + timedelta(days=6))
        self.assertTrue(self.held())

    def test_edited_body_removing_link_stops_counting(self):
        self.claimed(pr=10)
        value = pull()
        self.api.add_pull(value)
        value["body"] = "The fix moved elsewhere"
        bot.ClaimBot(self.api, self.now).on_pull_request(self.pr_event(action="edited"))
        self.sweep()
        self.assertFalse(self.held())

    def test_claimant_pr_edit_resets_idle_clock(self):
        self.claimed(pr=10)
        self.api.add_pull(pull())
        update = START + timedelta(days=4)
        self.api.pulls[10]["updated_at"] = bot.iso(update)
        self.api.rows["/issues/10/events"] = [{
            "event": "edited", "actor": {"login": "alice"}, "created_at": bot.iso(update)}]
        self.sweep(START + timedelta(days=5))
        self.assertTrue(self.held())

    def test_pr_updated_at_requires_claimant_actor_evidence(self):
        for actor, kept in ((None, False), ("maintainer", False), ("alice", True)):
            with self.subTest(actor=actor):
                self.setUp()
                self.claimed(pr=10)
                self.api.add_pull(pull())
                update = START + timedelta(days=4)
                self.api.pulls[10]["updated_at"] = bot.iso(update)
                if actor:
                    self.api.rows["/issues/10/events"] = [{
                        "event": "renamed", "actor": {"login": actor}, "created_at": bot.iso(update)}]
                self.sweep(START + timedelta(days=5))
                self.assertEqual(self.held(), kept)

    def test_deleted_marker_comment_keeps_unassigned_claim_for_manual_resolution(self):
        self.claimed(assigned=False)
        self.api.rows["/issues/1/comments"] = []
        with patch("sys.stderr", new=io.StringIO()):
            self.sweep(START + timedelta(days=20))
        self.assertTrue(self.held())
        self.assertFalse(self.messages())

    def test_deleted_marker_with_assignment_keeps_manual_assignment(self):
        self.claimed()
        self.api.rows["/issues/1/comments"] = []
        self.sweep(START + timedelta(days=20))
        self.assertTrue(self.held())
        self.assertEqual(self.api.issues[1]["assignees"], [{"login": "alice"}])

    def test_claimant_leaves_repository_marker_claim_still_works(self):
        self.claimed(assigned=False)
        self.api.permissions["alice"] = {"permission": "write"}
        self.sweep(START + timedelta(days=20))
        self.assertTrue(self.held())
        self.api.failures[("GET", "/collaborators/alice/permission")] = None
        self.sweep(START + timedelta(days=21))
        self.assertFalse(self.held())

    def test_closed_issue_clears_label_and_assignment(self):
        self.claimed()
        self.api.issues[1]["state"] = "closed"
        bot.ClaimBot(self.api, self.now).on_closed_issue({"issue": {"number": 1}})
        self.assertFalse(self.held())
        self.assertFalse(self.api.issues[1]["assignees"])

    def test_failed_expected_read_keeps_claim_and_continues_other_issues(self):
        self.claimed()
        self.api.issues[2] = issue(2, ["bob"])
        self.api.rows["/issues/2/comments"] = [bot_comment(bot.claim_marker(bot.Claim("bob", START)))]
        self.api.failures[("LIST", "/issues/1/timeline")] = None
        with patch("sys.stderr", new=io.StringIO()):
            self.sweep()
        self.assertTrue(self.held())
        self.assertFalse(self.api.issues[2]["labels"])


class Transport(unittest.TestCase):
    def test_pr_errors_warn_and_exit_zero(self):
        for failure in (bot.APIError("HTTP 401"), bot.APIError("HTTP 403"), bot.APIError("HTTP 500"),
                        ValueError("unexpected\ninput")):
            with self.subTest(failure=failure), TemporaryDirectory() as folder:
                event = Path(folder) / "event.json"
                event.write_text(json.dumps({"pull_request": pull()}))
                api = FakeAPI()
                api.failures[("GET", "/issues/1")] = failure
                with patch.dict(os.environ, {"GITHUB_EVENT_PATH": str(event), "GITHUB_REPOSITORY": api.repository,
                                           "GITHUB_TOKEN": "secret", "GITHUB_EVENT_NAME": "pull_request_target"}), \
                        patch.object(bot, "GitHub", return_value=api), patch.object(bot.sys, "argv", ["issue_claim.py"]), \
                        patch("sys.stderr", new_callable=io.StringIO) as output:
                    self.assertEqual(bot.main(), 0)
                self.assertIn("::warning::", output.getvalue())
                self.assertNotIn("secret", output.getvalue())
                self.assertNotIn("unexpected\ninput", output.getvalue())

    def test_malformed_pr_event_warns_and_exit_zero(self):
        with TemporaryDirectory() as folder:
            event = Path(folder) / "event.json"
            event.write_text("{")
            with patch.dict(os.environ, {"GITHUB_EVENT_PATH": str(event), "GITHUB_EVENT_NAME": "pull_request_target"}), \
                    patch.object(bot.sys, "argv", ["issue_claim.py"]), patch("sys.stderr", new_callable=io.StringIO) as output:
                self.assertEqual(bot.main(), 0)
            self.assertIn("::warning::", output.getvalue())

    def test_incomplete_pr_read_warns_and_preserves_claim(self):
        api = FakeAPI()
        api.add_pull(pull())
        api.failures[("LIST", "/issues/1/timeline")] = None
        with patch("sys.stderr", new_callable=io.StringIO) as output:
            bot.ClaimBot(api, START).on_pull_request({"pull_request": pull()})
        self.assertIn("::warning::", output.getvalue())
        self.assertFalse(api.issues[1]["assignees"])

    def test_issue_command_errors_remain_fatal(self):
        for failure in (bot.APIError("HTTP 401"), bot.APIError("HTTP 403"), RuntimeError("unexpected")):
            with self.subTest(failure=failure), TemporaryDirectory() as folder:
                event = Path(folder) / "event.json"
                event.write_text(json.dumps({"issue": {"number": 1}, "comment": comment(body="/claim")}))
                api = FakeAPI()
                api.failures[("GET", "/issues/1")] = failure
                with patch.dict(os.environ, {"GITHUB_EVENT_PATH": str(event), "GITHUB_REPOSITORY": api.repository,
                                           "GITHUB_TOKEN": "secret", "GITHUB_EVENT_NAME": "issue_comment"}), \
                        patch.object(bot, "GitHub", return_value=api), patch.object(bot.sys, "argv", ["issue_claim.py"]), \
                        patch("sys.stderr", new_callable=io.StringIO) as output:
                    self.assertEqual(bot.main(), 1)
                self.assertNotIn("::warning::", output.getvalue())

    def test_dry_run_event_reads_and_prints_claim_writes(self):
        api = FakeAPI()
        normal = FakeAPI()
        event_data = {"issue": {"number": 1}, "comment": comment(body="/claim")}
        with TemporaryDirectory() as folder:
            event = Path(folder) / "event.json"
            event.write_text(json.dumps(event_data))
            with patch.dict(os.environ, {"GITHUB_REPOSITORY": api.repository, "GITHUB_TOKEN": "secret"}, clear=True), \
                    patch.object(bot, "GitHub", return_value=api), \
                    patch.object(bot.sys, "argv", ["issue_claim.py", "--dry-run", "--event", str(event)]), \
                    patch("sys.stdout", new_callable=io.StringIO) as output:
                self.assertEqual(bot.main(), 0)
        writes = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertTrue(all(entry["dry_run"] for entry in writes))
        self.assertEqual({entry["path"] for entry in writes}, {
            "/issues/1/assignees", "/labels", "/issues/1/labels", "/issues/1/comments",
            "/issues/comments/100/reactions"})
        reply = next(entry["data"]["body"] for entry in writes if entry["path"] == "/issues/1/comments")
        self.assertEqual(bot.read_claim_marker(reply).user, "alice")
        self.assertFalse(api.issues[1]["assignees"])
        self.assertFalse(api.issues[1]["labels"])
        self.assertFalse(api.rows)
        self.assertFalse(api.label)
        self.assertFalse(api.reactions)
        bot.ClaimBot(normal, START).on_comment(event_data)
        reads = lambda calls: [entry for entry in calls if entry[0] in ("GET", "LIST")]
        self.assertEqual(api.calls, reads(normal.calls))

    def test_dry_run_sweep_reads_activity_and_prints_release_writes(self):
        api = FakeAPI()
        api.issues[1] = issue(assignees=["alice"])
        api.rows["/issues/1/comments"] = [bot_comment(bot.claim_marker(bot.Claim("alice", START, 10)))]
        api.add_pull(pull())
        before = copy.deepcopy((api.issues, api.rows))
        with patch.dict(os.environ, {"GITHUB_REPOSITORY": api.repository, "GITHUB_TOKEN": "secret"}, clear=True), \
                patch.object(bot, "GitHub", return_value=api), \
                patch.object(bot.sys, "argv", ["issue_claim.py", "--dry-run", "--sweep"]), \
                patch("sys.stdout", new_callable=io.StringIO) as output:
            self.assertEqual(bot.main(), 0)
        writes = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual({(entry["method"], entry["path"]) for entry in writes}, {
            ("DELETE", "/issues/1/assignees"), ("DELETE", "/issues/1/labels/claimed"), ("POST", "/issues/1/comments")})
        self.assertEqual((api.issues, api.rows), before)
        self.assertTrue(all(method in ("GET", "LIST") for method, _, _ in api.calls))
        self.assertTrue({"/issues/10/timeline", "/issues/10/events", "/pulls/10/commits",
                         "/issues/10/comments", "/pulls/10/comments", "/pulls/10/reviews"}.issubset(
                             {path for method, path, _ in api.calls if method == "LIST"}))

    def test_dry_run_keeps_read_failures_and_external_dependencies(self):
        api = FakeAPI()
        api.external[("other/repo", "/issues/20")] = {"state": "open"}
        dry = bot.DryRun(api)
        self.assertEqual(dry.request("GET", "/issues/20", repository="other/repo", expected=(404,)), {"state": "open"})
        api.failures[("GET", "/issues/1")] = bot.APIError("HTTP 403")
        with self.assertRaises(bot.APIError):
            dry.request("GET", "/issues/1")

    def test_pagination_and_incomplete_list(self):
        api = bot.GitHub("cadmpeg/cadmpeg", "secret")
        with patch.object(api, "request", side_effect=[[{}] * 100, [{}]]) as request:
            self.assertEqual(len(api.items("/issues", state="open")), 101)
            self.assertIn("page=2", request.call_args.args[1])
        with patch.object(api, "request", side_effect=[[{}] * 100, None]):
            self.assertIsNone(api.items("/issues/1/comments"))

    def test_only_expected_404_422_are_tolerated(self):
        api = bot.GitHub("cadmpeg/cadmpeg", "secret")
        for status in (401, 403, 404, 422, 500):
            error = HTTPError("url", status, "failed", {}, None)
            with self.subTest(status=status), patch.object(bot, "urlopen", side_effect=error), patch("sys.stderr", new_callable=io.StringIO) as output:
                with self.assertRaises(bot.APIError):
                    api.request("GET", "/issues/1")
                self.assertIn(f"HTTP {status}", output.getvalue())
                self.assertNotIn("secret", output.getvalue())
                if status in (404, 422):
                    self.assertIsNone(api.request("GET", "/issues/1", expected=(status,)))
                else:
                    with self.assertRaises(bot.APIError):
                        api.request("GET", "/issues/1", expected=(status,))
        with patch.object(bot, "urlopen", side_effect=URLError("offline")), patch("sys.stderr", new=io.StringIO()):
            with self.assertRaises(bot.APIError):
                api.request("GET", "/issues")

    def test_unexpected_failure_exits_nonzero(self):
        with TemporaryDirectory() as folder:
            event = Path(folder) / "event.json"
            event.write_text("{}")
            api = FakeAPI()
            api.items = lambda *args, **kwargs: (_ for _ in ()).throw(bot.APIError("denied"))
            with patch.dict(os.environ, {"GITHUB_EVENT_PATH": str(event), "GITHUB_REPOSITORY": api.repository,
                                       "GITHUB_TOKEN": "secret", "GITHUB_EVENT_NAME": "schedule"}), \
                    patch.object(bot, "GitHub", return_value=api), patch.object(bot.sys, "argv", ["issue_claim.py"]), \
                    patch("sys.stderr", new=io.StringIO()):
                self.assertEqual(bot.main(), 1)

    def test_command_output_uses_event_file_and_ignores_bots(self):
        with TemporaryDirectory() as folder:
            event = Path(folder) / "event.json"
            output = Path(folder) / "output"
            for kind, expected in [("User", "command=/claim\n"), ("Bot", "command=\n")]:
                event.write_text(json.dumps({"comment": comment(body=" \n /claim", kind=kind)}))
                output.write_text("")
                with patch.dict(os.environ, {"GITHUB_EVENT_PATH": str(event), "GITHUB_OUTPUT": str(output)}), \
                        patch.object(bot.sys, "argv", ["issue_claim.py", "--command-output"]):
                    self.assertEqual(bot.main(), 0)
                self.assertEqual(output.read_text(), expected)

    def test_workflow_default_branch_event_safety_and_per_issue_concurrency(self):
        workflow = (Path(__file__).resolve().parents[1] / ".github/workflows/issue-claim.yml").read_text()
        self.assertEqual(workflow.count("ref: ${{ github.event.repository.default_branch }}"), 5)
        self.assertEqual(workflow.count("persist-credentials: false"), 5)
        self.assertIn("types: [opened, edited, reopened, synchronize]", workflow)
        self.assertIn('cron: "7 * * * *"', workflow)
        self.assertNotIn("pull_request.head", workflow)
        self.assertNotIn("\nconcurrency:", workflow)
        self.assertEqual(workflow.count("group: issue-claim-${{ github.event.issue.number }}"), 2)
        self.assertIn("group: issue-claim-sweep", workflow)
        self.assertNotIn("cancel-in-progress: true", workflow)
        for line in workflow.splitlines():
            if "run:" in line:
                self.assertNotIn("${{", line)
        self.assertIn("contains(github.event.comment.body, needs.command.outputs.command)", workflow)


if __name__ == "__main__":
    unittest.main()
