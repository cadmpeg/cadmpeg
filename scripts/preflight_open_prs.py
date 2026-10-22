# SPDX-License-Identifier: Apache-2.0
"""No other open pull request changes the same files, or the overlap is known.

Two open PRs that change the same files either duplicate work or must keep
each other's changes when the second one rebases. The check asks GitHub,
through the `gh` CLI, for the upstream repository's open PRs and warns on
each one that shares files with this branch. It needs network access and
an authenticated `gh`; `--offline` skips it.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess

NAME = "open-prs"
ORDER = 90

REPOSITORY = re.compile(r"github\.com[:/]([\w.-]+/[\w.-]+?)(?:\.git)?/?$")


def repository(context) -> str | None:
    remote = context.upstream.split("/", 1)[0]
    url = context.git("remote", "get-url", remote).stdout.strip()
    match = REPOSITORY.search(url)
    return match.group(1) if match else None


def branch_name(context) -> str | None:
    """The branch under test: `HEAD`'s, or the branch that `--rev` names or points at."""
    if context.rev is None:
        name = context.git("rev-parse", "--abbrev-ref", "HEAD").stdout.strip()
        return name if name and name != "HEAD" else None
    full = context.git("rev-parse", "--symbolic-full-name", context.rev).stdout.strip()
    if full.startswith("refs/heads/"):
        return full.removeprefix("refs/heads/")
    if full.startswith("refs/remotes/"):
        return full.removeprefix("refs/remotes/").split("/", 1)[-1]
    pointing = context.git("branch", "--points-at", context.rev,
                           "--format=%(refname:short)").stdout.split()
    return pointing[0] if len(pointing) == 1 else None


def open_pulls(repo: str) -> list[dict] | None:
    process = subprocess.run(
        ["gh", "pr", "list", "-R", repo, "--state", "open", "--limit", "100",
         "--json", "number,title,author,headRefName,files"],
        capture_output=True, text=True, timeout=60)
    return json.loads(process.stdout) if process.returncode == 0 else None


def overlaps(pulls: list[dict], paths: set[str], branch: str | None) -> list[str]:
    found = []
    for pull in pulls:
        if pull.get("headRefName") == branch:
            continue
        shared = sorted(paths & {file["path"] for file in pull.get("files") or []})
        if shared:
            author = (pull.get("author") or {}).get("login", "?")
            found.append(f"#{pull['number']} {pull['title']} ({author}): "
                         + ", ".join(shared[:6]) + (" ..." if len(shared) > 6 else ""))
    return found


def run(context):
    if context.options.offline:
        return context.skipped("--offline")
    if shutil.which("gh") is None:
        return context.skipped("the gh CLI is not installed")
    repo = repository(context)
    if repo is None:
        return context.skipped(f"cannot tell the GitHub repository of {context.upstream}")
    try:
        pulls = open_pulls(repo)
    except (subprocess.TimeoutExpired, json.JSONDecodeError):
        pulls = None
    if pulls is None:
        return context.skipped(f"gh could not list the open PRs of {repo}")
    branch = branch_name(context)
    found = overlaps(pulls, set(context.changed_paths()), branch)
    if not found:
        return context.passed(f"no open PR in {repo} changes the same files")
    return context.warned(
        f"{len(found)} open PR(s) change the same files",
        *found[:15],
        "check: read each one; drop duplicate work, and agree which lands first and what the "
        "second must keep",
    )
