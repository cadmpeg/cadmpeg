#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Manage issue claims through the GitHub REST API."""

from __future__ import annotations

import json
import os
import re
import sys
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen

DRAFT_HOURS = 24
IDLE_DAYS = 5
MAX_LINKED_ISSUES = 50
BOT = "github-actions[bot]"
LOGIN = r"[A-Za-z0-9][A-Za-z0-9-]{0,38}(?:\[bot\])?"
CLAIM = re.compile(rf"<!-- cadmpeg-claim: user=({LOGIN}) at=(\S+?)(?: pr=([1-9]\d*))? -->")
ACTIVITY = re.compile(rf"<!-- cadmpeg-claim-activity: user=({LOGIN}) at=(\S+) kind=(push|update) -->")
RELEASE = re.compile(r"<!-- cadmpeg-claim-release: at=(\S+) -->")
LINK = re.compile(r"\b(?:fixes|closes|resolves|part\s+of)\s+#([1-9]\d*)(?![\w/-])", re.I)
REFERENCE = r"(?:https://github\.com/[\w.-]+/[\w.-]+/(?:issues|pull)/[1-9]\d*|[\w.-]+/[\w.-]+#[1-9]\d*|#[1-9]\d*)(?![\w/#-])"
DEPENDENCY = re.compile(rf"\b(?:depends\s+on|stacked\s+on)\s+({REFERENCE})", re.I)
SNAPSHOT = re.compile(r"<!-- cadmpeg-claim-commits: shas=([a-zA-Z0-9,]*) -->")


def parse_command(body: str | None) -> str | None:
    first = next((line.strip().lower() for line in (body or "").splitlines() if line.strip()), "")
    return first if first in ("/claim", "/unclaim") else None


def prose(text: str | None) -> str:
    lines = []
    fence = None
    for line in (text or "").splitlines():
        match = re.match(r"^ {0,3}(`{3,}|~{3,})", line)
        if fence:
            if match and match[1][0] == fence[0] and len(match[1]) >= len(fence):
                fence = None
            continue
        if match:
            fence = match[1]
        elif not re.match(r"^\s*>", line):
            lines.append(line)
    return re.sub(r"(`+).*?\1", "", "\n".join(lines), flags=re.S)


def linked_issues(text: str | None) -> set[int]:
    result = set()
    for match in LINK.finditer(prose(text)):
        result.add(int(match[1]))
        if len(result) == MAX_LINKED_ISSUES:
            break
    return result


@dataclass(frozen=True)
class Dependency:
    repository: str
    number: int


def dependencies(text: str | None, repository: str) -> set[Dependency]:
    result = set()
    for match in DEPENDENCY.finditer(prose(text)):
        ref = match[1]
        if ref.lower().startswith("https://github.com/"):
            parts = ref.split("/")
            dep = Dependency("/".join(parts[3:5]), int(parts[6]))
        elif ref.startswith("#"):
            dep = Dependency(repository, int(ref[1:]))
        else:
            repo, number = ref.split("#")
            dep = Dependency(repo, int(number))
        result.add(dep)
        if len(result) == MAX_LINKED_ISSUES:
            break
    return result


def timestamp(value: str | None) -> datetime | None:
    try:
        at = datetime.fromisoformat((value or "").replace("Z", "+00:00"))
        return at.astimezone(timezone.utc) if at.tzinfo else None
    except ValueError:
        return None


def iso(at: datetime) -> str:
    if at.tzinfo is None:
        raise ValueError("timestamp needs a timezone")
    return at.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


@dataclass(frozen=True)
class Claim:
    user: str
    at: datetime
    pr: int | None = None
    recorded: bool = True


def claim_marker(claim: Claim) -> str:
    if not re.fullmatch(LOGIN, claim.user):
        raise ValueError("invalid GitHub login")
    source = f" pr={claim.pr}" if claim.pr is not None else ""
    return f"<!-- cadmpeg-claim: user={claim.user} at={iso(claim.at)}{source} -->"


def read_claim_marker(body: str | None) -> Claim | None:
    match = CLAIM.search(body or "")
    at = timestamp(match[2]) if match else None
    return Claim(match[1], at, int(match[3]) if match[3] else None) if match and at else None


def activity_marker(user: str, at: datetime, kind: str) -> str:
    if not re.fullmatch(LOGIN, user) or kind not in ("push", "update"):
        raise ValueError("invalid activity marker")
    return f"<!-- cadmpeg-claim-activity: user={user} at={iso(at)} kind={kind} -->"


def own_comment(comment: dict) -> bool:
    return (comment.get("user") or {}).get("login") == BOT and (comment.get("user") or {}).get("type") == "Bot"


def is_bot(user: dict) -> bool:
    return user.get("type") == "Bot" or user.get("login", "").endswith("[bot]")


def login(record: dict) -> str:
    return (record.get("user") or {}).get("login", "")


def text(pull: dict) -> str:
    return (pull.get("title") or "") + "\n" + (pull.get("body") or "")


def comment_time(comment: dict) -> datetime | None:
    return timestamp(comment.get("updated_at")) or timestamp(comment.get("created_at"))


def current_claim(issue: dict, comments: list[dict], timeline: list[dict]) -> Claim | None:
    claim = None
    for comment in comments:
        if own_comment(comment):
            if RELEASE.search(comment.get("body") or ""):
                claim = None
            elif found := read_claim_marker(comment.get("body")):
                claim = found
    assignees = [entry["login"] for entry in issue.get("assignees", [])]
    labelled = any(label.get("name") == "claimed" for label in issue.get("labels", []))
    changes = [entry for entry in timeline if entry.get("event") in ("assigned", "unassigned")]
    if claim and any((timestamp(entry.get("created_at")) or claim.at) > claim.at for entry in changes):
        claim = None
    if claim and (labelled or any(name.lower() == claim.user.lower() for name in assignees)):
        if not assignees or any(name.lower() == claim.user.lower() for name in assignees):
            return claim
    if assignees:
        user = assignees[0]
        at = max((at for entry in changes if entry.get("event") == "assigned"
                  and (entry.get("assignee") or {}).get("login", "").lower() == user.lower()
                  and (at := timestamp(entry.get("created_at")))), default=timestamp(issue.get("created_at")))
        return Claim(user, at, recorded=False) if at else None
    return None


@dataclass(frozen=True)
class Decision:
    action: str
    phase: str
    since: datetime
    deadline: datetime | None


def lapse_decision(claim: Claim, now: datetime, has_pr: bool = False,
                   activity: datetime | None = None, waiting: bool = False,
                   returned: datetime | None = None, closed: datetime | None = None) -> Decision:
    phase = "idle" if has_pr else "draft"
    since = max(claim.at, activity or claim.at, returned or claim.at) if has_pr else max(claim.at, closed or claim.at)
    duration = timedelta(days=IDLE_DAYS) if has_pr else timedelta(hours=DRAFT_HOURS)
    warning = duration - timedelta(days=1) if has_pr else duration / 2
    deadline = since + duration
    action = "hold" if waiting else "lapse" if now >= deadline else "warn" if now >= since + warning else "active"
    return Decision(action, phase, since, deadline)


def log(message: str) -> None:
    print(f"issue-claim: {message}", file=sys.stderr)


class APIError(RuntimeError):
    pass


class Incomplete(RuntimeError):
    pass


class GitHub:
    def __init__(self, repository: str, token: str, server: str = "https://api.github.com"):
        self.repository = repository
        self.server = server.rstrip("/")
        self.repo_url = f"{self.server}/repos/{repository}"
        self.token = token

    def request(self, method: str, path: str, data: dict | None = None,
                expected: tuple[int, ...] = (), repository: str | None = None):
        repo = repository or self.repository
        if not re.fullmatch(r"[\w.-]+/[\w.-]+", repo):
            raise APIError("invalid repository")
        request = Request(f"{self.server}/repos/{repo}{path}", method=method,
                          data=json.dumps(data).encode() if data is not None else None,
                          headers={"Authorization": f"Bearer {self.token}",
                                   "Accept": "application/vnd.github+json",
                                   "X-GitHub-Api-Version": "2022-11-28", "Content-Type": "application/json"})
        try:
            with urlopen(request, timeout=30) as response:
                body = response.read()
                return json.loads(body) if body else {}
        except HTTPError as error:
            log(f"{method} {repo}{path}: HTTP {error.code}")
            error.close()
            if error.code in expected and error.code in (404, 422):
                return None
            raise APIError(f"REST request failed: HTTP {error.code}") from error
        except (URLError, TimeoutError, OSError, ValueError) as error:
            log(f"{method} {repo}{path}: {type(error).__name__}")
            raise APIError("REST request failed") from error

    def items(self, path: str, **params) -> list[dict] | None:
        result = []
        page = 1
        while True:
            query = urlencode({**params, "per_page": 100, "page": page})
            expected = (404,) if re.match(r"/(?:issues|pulls)/\d+/", path) else ()
            batch = self.request("GET", f"{path}?{query}", expected=expected)
            if batch is None:
                return None
            if not isinstance(batch, list):
                raise APIError("REST list response has an invalid shape")
            result.extend(batch)
            if len(batch) < 100:
                return result
            page += 1


class ClaimBot:
    def __init__(self, api, now: datetime):
        self.api = api
        self.now = now
        self.permissions = {}
        self.pulls = {}
        self.lists = {}

    def rows(self, path: str) -> list[dict]:
        if path not in self.lists:
            rows = self.api.items(path)
            if rows is None:
                raise Incomplete(path)
            self.lists[path] = rows
        return self.lists[path]

    def post(self, number: int, body: str) -> bool:
        result = self.api.request("POST", f"/issues/{number}/comments", {"body": body}, expected=(404, 422))
        self.lists.pop(f"/issues/{number}/comments", None)
        return result is not None

    def react(self, event: dict, content: str) -> None:
        self.api.request("POST", f"/issues/comments/{event['comment']['id']}/reactions",
                         {"content": content}, expected=(404, 422))

    def issue(self, number: int, allow_closed: bool = False) -> dict | None:
        issue = self.api.request("GET", f"/issues/{number}", expected=(404,))
        if not issue or "pull_request" in issue or issue.get("locked"):
            return None
        if issue.get("repository_url", self.api.repo_url).lower() != self.api.repo_url.lower():
            return None
        return issue if allow_closed or issue.get("state") == "open" else None

    def maintainer(self, user: str) -> bool:
        if user.lower() == self.api.repository.split("/")[0].lower():
            return True
        if user.lower() not in self.permissions:
            data = self.api.request("GET", f"/collaborators/{quote(user, safe='')}/permission", expected=(404,)) or {}
            self.permissions[user.lower()] = (data.get("permission") in ("admin", "maintain", "write")
                or (data.get("user") or {}).get("permissions", {}).get("push", False))
        return self.permissions[user.lower()]

    def load(self, issue: dict) -> Claim | None:
        number = issue["number"]
        return current_claim(issue, self.rows(f"/issues/{number}/comments"), self.rows(f"/issues/{number}/timeline"))

    def ensure_label(self, number: int) -> bool:
        if self.api.request("GET", "/labels/claimed", expected=(404,)) is None:
            created = self.api.request("POST", "/labels", {"name": "claimed", "color": "1d76db",
                                      "description": "An active contributor claim"}, expected=(422,))
            if created is None and self.api.request("GET", "/labels/claimed", expected=(404,)) is None:
                return False
        return self.api.request("POST", f"/issues/{number}/labels", {"labels": ["claimed"]}, expected=(404, 422)) is not None

    def record(self, issue: dict, user: str, pr: int | None = None) -> None:
        number = issue["number"]
        assigned = self.api.request("POST", f"/issues/{number}/assignees", {"assignees": [user]}, expected=(422,))
        assigned_ok = assigned is not None and any(
            entry["login"].lower() == user.lower() for entry in assigned.get("assignees", []))
        if not self.ensure_label(number):
            return
        explanation = "" if assigned_ok else " GitHub could not assign you; the recorded claim remains active."
        self.post(number, f"@{user} claimed this issue.{explanation} Open a linked draft PR within "
                  f"{DRAFT_HOURS} hours, even a rough one. A reminder is posted halfway through that period. "
                  f"Once a linked PR exists, {IDLE_DAYS} quiet days releases the claim, with a warning one day "
                  "before release. Claimant pushes, PR edits, and comments reset the idle clock. Claims stay "
                  "active while a ready PR waits for maintainers or an open dependency. Maintainer claims "
                  "stay active. Use /unclaim to release early. On lapse, anyone may continue from the PR "
                  "and keep its author as co-author with a `Co-authored-by: Name <email>` trailer.\n\n"
                  + claim_marker(Claim(user, self.now, pr)))

    def release(self, issue: dict, body: str) -> None:
        number = issue["number"]
        if issue.get("assignees"):
            if self.api.request("DELETE", f"/issues/{number}/assignees",
                    {"assignees": [entry["login"] for entry in issue["assignees"]]}, expected=(404, 422)) is None:
                return
        self.api.request("DELETE", f"/issues/{number}/labels/claimed", expected=(404,))
        self.post(number, body + f"\n\n<!-- cadmpeg-claim-release: at={iso(self.now)} -->")

    def reference_number(self, entry: dict) -> int | None:
        source = (entry.get("source") or {}).get("issue") or {}
        number = source.get("number")
        if (entry.get("event") == "cross-referenced" and "pull_request" in source
                and isinstance(number, int) and source.get("url", "").lower() == f"{self.api.repo_url}/issues/{number}".lower()):
            return number
        return None

    def get_pull(self, number: int) -> dict | None:
        if number not in self.pulls:
            self.pulls[number] = self.api.request("GET", f"/pulls/{number}", expected=(404,))
        pull = self.pulls[number]
        repository = (((pull or {}).get("base") or {}).get("repo") or {}).get("full_name", self.api.repository)
        return pull if repository.lower() == self.api.repository.lower() else None

    def linked_pulls(self, issue: dict, claim: Claim | None, extra: int | None = None) -> list[dict]:
        numbers = {number for entry in self.rows(f"/issues/{issue['number']}/timeline")
                   if (number := self.reference_number(entry)) is not None}
        if claim and claim.pr:
            numbers.add(claim.pr)
        if extra:
            numbers.add(extra)
        result = []
        for number in sorted(numbers):
            pull = self.get_pull(number)
            if pull is None or is_bot(pull.get("user") or {}):
                continue
            explicit = issue["number"] in linked_issues(text(pull))
            if not explicit and (claim is None or login(pull).lower() != claim.user.lower()):
                continue
            if not explicit and claim and claim.pr == number:
                continue
            result.append(pull)
        return result

    def pull_activity(self, pull: dict, claimant: str) -> tuple[datetime | None, bool, datetime | None]:
        number = pull["number"]
        comments = self.rows(f"/issues/{number}/comments") + self.rows(f"/pulls/{number}/comments")
        reviews = self.rows(f"/pulls/{number}/reviews")
        commits = self.rows(f"/pulls/{number}/commits")
        actions = []
        responses = []
        returned = []
        if login(pull).lower() == claimant.lower() and (at := timestamp(pull.get("created_at"))):
            actions.append(at)
            responses.append(at)
        for entry in comments:
            at = comment_time(entry)
            if login(entry).lower() == claimant.lower() and at:
                actions.append(at)
                responses.append(at)
            elif at and login(entry) and not is_bot(entry.get("user") or {}) and self.maintainer(login(entry)):
                returned.append(at)
            if own_comment(entry):
                for match in ACTIVITY.finditer(entry.get("body") or ""):
                    if match[1].lower() == claimant.lower() and (at := timestamp(match[2])):
                        actions.append(at)
                        if match[3] == "push":
                            responses.append(at)
        for entry in reviews:
            at = timestamp(entry.get("submitted_at"))
            if at and entry.get("state") not in ("PENDING", "DISMISSED"):
                if login(entry).lower() == claimant.lower():
                    actions.append(at)
                    responses.append(at)
                elif not is_bot(entry.get("user") or {}):
                    returned.append(at)
        for commit in commits:
            if any((commit.get(role) or {}).get("login", "").lower() == claimant.lower() for role in ("author", "committer")):
                if at := timestamp(((commit.get("commit") or {}).get("committer") or {}).get("date")):
                    actions.append(at)
                    responses.append(at)
        last_return = max(returned, default=None)
        last_response = max(responses, default=None)
        waiting = (pull.get("state") == "open" and not pull.get("draft", False)
                   and login(pull).lower() == claimant.lower() and last_response is not None
                   and (last_return is None or last_response > last_return))
        if pull.get("state") == "open":
            for dep in dependencies(pull.get("body"), self.api.repository):
                if dep.repository.lower() == self.api.repository.lower() and dep.number == number:
                    continue
                item = self.api.request("GET", f"/issues/{dep.number}", repository=dep.repository, expected=(404,))
                if item is None:
                    raise Incomplete(f"dependency {dep.repository}#{dep.number}")
                waiting |= item.get("state") == "open"
        return max(actions, default=None), waiting, last_return

    def assess(self, issue: dict, claim: Claim) -> tuple[Decision, list[dict]]:
        if self.maintainer(claim.user):
            return Decision("hold", "protected", claim.at, None), []
        if not claim.recorded:
            return Decision("hold", "manual", claim.at, None), []
        pulls = self.linked_pulls(issue, claim)
        owned = [pull for pull in pulls if login(pull).lower() == claim.user.lower()
                 and issue["number"] in linked_issues(text(pull))]
        live = [pull for pull in owned if pull.get("state") == "open" or pull.get("merged_at")]
        closed = max((at for pull in owned if pull.get("state") == "closed" and not pull.get("merged_at")
                      and (at := timestamp(pull.get("closed_at")))), default=None)
        activity = claim.at
        returned = None
        waiting = False
        if live:
            for entry in self.rows(f"/issues/{issue['number']}/comments"):
                if login(entry).lower() == claim.user.lower() and (at := comment_time(entry)):
                    activity = max(activity, at)
            for pull in pulls:
                if pull.get("state") == "closed" and not pull.get("merged_at"):
                    continue
                at, hold, response = self.pull_activity(pull, claim.user)
                activity = max(activity, at or claim.at)
                waiting |= hold
                if response:
                    returned = max(returned or claim.at, response)
        return lapse_decision(claim, self.now, bool(live), activity, waiting, returned, closed), owned

    def lapse_body(self, claim: Claim, decision: Decision, pulls: list[dict]) -> str:
        reason = (f"the {DRAFT_HOURS}-hour draft deadline passed" if decision.phase == "draft"
                  else f"{IDLE_DAYS} quiet days passed")
        body = f"@{claim.user}, this claim lapsed because {reason}. This issue is available."
        if pulls:
            server = os.environ.get("GITHUB_SERVER_URL", "https://github.com")
            links = ", ".join(f"[PR #{pull['number']}]({server}/{self.api.repository}/pull/{pull['number']})" for pull in pulls)
            body += (f" Anyone may continue from {links}: fork the PR, or push to it if maintainers allow. "
                     f"Keep @{claim.user} as co-author with a `Co-authored-by: Name <email>` trailer in the continuation.")
        return body

    def holder_reply(self, number: int, claim: Claim, decision: Decision) -> None:
        expiry = (f"lapses at {decision.deadline:%Y-%m-%d %H:%M UTC}" if decision.action != "hold"
                  else "is protected by maintainer access" if decision.phase == "protected"
                  else "stays active until a maintainer releases it" if decision.phase == "manual"
                  else "stays active while waiting on maintainers or a dependency")
        self.post(number, f"Already claimed by @{claim.user} since {claim.at.date()}; the claim {expiry}. "
                  + ("Please ask them." if decision.action == "hold" else "Please ask them or wait for it to lapse."))

    def on_comment(self, event: dict) -> None:
        entry = event["comment"]
        command = parse_command(entry.get("body"))
        if command is None or is_bot(entry.get("user") or {}) or "pull_request" in event["issue"]:
            return
        issue = self.issue(event["issue"]["number"])
        if issue is None:
            return
        try:
            claim = self.load(issue)
            user = login(entry)
            owner = self.api.repository.split("/")[0]
            if command == "/unclaim":
                if claim is None:
                    self.react(event, "eyes")
                elif claim.user.lower() == owner.lower() and user.lower() != owner.lower():
                    self.react(event, "eyes")
                elif user.lower() == claim.user.lower() or self.maintainer(user):
                    self.release(issue, f"@{user} released this claim. This issue is available.")
                    self.react(event, "+1")
                else:
                    self.react(event, "eyes")
                return
            takeover = False
            if claim:
                if claim.user.lower() == user.lower():
                    self.react(event, "+1")
                    return
                decision, pulls = self.assess(issue, claim)
                if decision.action != "lapse":
                    self.holder_reply(issue["number"], claim, decision)
                    return
                self.release(issue, self.lapse_body(claim, decision, pulls))
                issue = self.issue(issue["number"])
                takeover = True
            if issue is None:
                return
            if not takeover:
                reserved = [pull for pull in self.linked_pulls(issue, None) if pull.get("state") == "open"]
                if reserved and login(reserved[0]).lower() != user.lower():
                    self.post(issue["number"], f"@{login(reserved[0])} already has open PR #{reserved[0]['number']} "
                              "for this issue. Please coordinate with them.")
                    return
            self.record(issue, user)
            self.react(event, "+1")
        except Incomplete as error:
            log(f"skip issue #{issue['number']}: incomplete {error}")

    def conflict(self, pr: int, target: int, message: str) -> None:
        marker = f"<!-- cadmpeg-claim-conflict: issue={target} -->"
        if not any(own_comment(entry) and marker in (entry.get("body") or "") for entry in self.rows(f"/issues/{pr}/comments")):
            self.post(pr, message + "\n\n" + marker)

    def record_activity(self, event: dict, number: int) -> None:
        entries = self.rows(f"/issues/{number}/comments")
        delivery = f"<!-- cadmpeg-claim-delivery: run={os.environ.get('GITHUB_RUN_ID', iso(self.now))} -->"
        if any(own_comment(entry) and delivery in (entry.get("body") or "") for entry in entries):
            return
        commits = self.rows(f"/pulls/{number}/commits")
        previous = None
        for entry in entries:
            if own_comment(entry) and (match := SNAPSHOT.search(entry.get("body") or "")):
                previous = set(match[1].split(","))
        markers = []
        if event.get("action") == "synchronize":
            added = [commit for commit in commits if commit.get("sha") not in previous] if previous is not None else commits[-1:]
            users = {commit[role]["login"] for commit in added for role in ("author", "committer")
                     if (commit.get(role) or {}).get("login") and not is_bot(commit[role])}
            markers.extend(activity_marker(user, self.now, "push") for user in sorted(users))
        else:
            actor = login({"user": event.get("sender") or event["pull_request"]["user"]})
            markers.append(activity_marker(actor, self.now, "update"))
        shas = ",".join(commit["sha"] for commit in commits if re.fullmatch(r"[a-zA-Z0-9]+", commit.get("sha", "")))
        self.post(number, "Claim activity recorded.\n\n" + "\n".join(markers + [delivery, f"<!-- cadmpeg-claim-commits: shas={shas} -->"]))

    def on_pull_request(self, event: dict) -> None:
        pull = event["pull_request"]
        if (is_bot(pull.get("user") or {}) or is_bot(event.get("sender") or {})
                or (event.get("repository") or {}).get("full_name", self.api.repository).lower() != self.api.repository.lower()):
            return
        targets = linked_issues(text(pull))
        if not targets:
            return
        number = pull["number"]
        try:
            self.record_activity(event, number)
        except Incomplete as error:
            log(f"skip PR #{number} activity: incomplete {error}")
        for target in sorted(targets):
            issue = self.issue(target)
            if issue is None:
                continue
            try:
                claim = self.load(issue)
                candidates = [item for item in self.linked_pulls(issue, claim, number)
                              if item.get("state") == "open" and target in linked_issues(text(item))]
                if not candidates:
                    continue
                winner = min(candidates, key=lambda item: item["number"])
                if number != winner["number"]:
                    self.conflict(number, target, f"PR #{winner['number']} by @{login(winner)} has the earlier claim on issue #{target}. Please coordinate there.")
                user = login(winner)
                if claim and claim.user.lower() == user.lower():
                    self.ensure_label(target)
                elif claim and claim.pr and claim.pr > winner["number"] and not self.maintainer(claim.user):
                    self.release(issue, f"The claim moves to earlier PR #{winner['number']} by @{user}.")
                    self.record(self.issue(target), user, winner["number"])
                elif claim:
                    self.conflict(number, target, f"Issue #{target} is already claimed by @{claim.user} since {claim.at.date()}. Please coordinate with them.")
                else:
                    self.record(issue, user, winner["number"])
            except Incomplete as error:
                log(f"skip issue #{target}: incomplete {error}")

    def sweep_issue(self, issue: dict) -> None:
        claim = self.load(issue)
        if claim is None:
            log(f"skip issue #{issue['number']}: claim marker or assignment is unavailable")
            return
        decision, pulls = self.assess(issue, claim)
        if decision.action == "lapse":
            self.release(issue, self.lapse_body(claim, decision, pulls))
        elif decision.action == "warn":
            marker = f"<!-- cadmpeg-claim-warning: user={claim.user} phase={decision.phase} since={iso(decision.since)} -->"
            if not any(own_comment(entry) and marker in (entry.get("body") or "") for entry in self.rows(f"/issues/{issue['number']}/comments")):
                detail = (f"Please open a linked draft PR within {DRAFT_HOURS} hours of claiming, even a rough one."
                          if decision.phase == "draft" else f"The idle limit is {IDLE_DAYS} days. Please post an update or push your work.")
                self.post(issue["number"], f"@{claim.user}, this claim lapses at {decision.deadline:%Y-%m-%d %H:%M UTC}. {detail} Use /unclaim to release early.\n\n{marker}")

    def sweep(self) -> None:
        issues = self.api.items("/issues", state="open", labels="claimed")
        if issues is None:
            raise APIError("claimed issue list is unavailable")
        for listed in issues:
            if "pull_request" not in listed and (issue := self.issue(listed["number"])):
                if any(label.get("name") == "claimed" for label in issue.get("labels", [])):
                    try:
                        self.sweep_issue(issue)
                    except Incomplete as error:
                        log(f"skip issue #{issue['number']}: incomplete {error}")

    def on_closed_issue(self, event: dict) -> None:
        number = event["issue"]["number"]
        issue = self.api.request("GET", f"/issues/{number}", expected=(404,))
        if issue and issue.get("state") == "closed" and "pull_request" not in issue:
            if issue.get("assignees"):
                self.api.request("DELETE", f"/issues/{number}/assignees",
                                 {"assignees": [entry["login"] for entry in issue["assignees"]]}, expected=(404, 422))
            self.api.request("DELETE", f"/issues/{number}/labels/claimed", expected=(404,))


def main() -> int:
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    if sys.argv[1:] == ["--command-output"]:
        command = parse_command((event.get("comment") or {}).get("body")) or ""
        if is_bot((event.get("comment") or {}).get("user") or {}):
            command = ""
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
            output.write(f"command={command}\n")
        return 0
    api = GitHub(os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_TOKEN"], os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    bot = ClaimBot(api, datetime.now(timezone.utc))
    try:
        name = os.environ["GITHUB_EVENT_NAME"]
        if name == "issue_comment":
            bot.on_comment(event)
        elif name == "pull_request_target":
            bot.on_pull_request(event)
        elif name == "issues" and event.get("action") == "closed":
            bot.on_closed_issue(event)
        elif name in ("schedule", "workflow_dispatch"):
            bot.sweep()
    except APIError as error:
        log(str(error))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
