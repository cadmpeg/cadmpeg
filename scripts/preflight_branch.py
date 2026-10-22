# SPDX-License-Identifier: Apache-2.0
"""The branch merges cleanly with the upstream tip and includes related merges.

GitHub runs no pull-request workflow while a branch conflicts with its base,
so a conflict leaves the PR without CI. A merge on the base branch that
touched the same files can also change what the branch must do, so a branch
behind such a merge is rebased and checked again.
"""

from __future__ import annotations

NAME = "branch"
ORDER = 10


def run(context):
    head = context.rev or "HEAD"
    behind = int(context.git("rev-list", "--count", f"{head}..{context.upstream}").stdout or 0)
    results = []
    merge = context.git("merge-tree", "--write-tree", "--name-only", "--no-messages",
                        context.upstream, head)
    if merge.returncode == 1:
        files = merge.stdout.splitlines()[1:]
        results.append(context.failed(
            f"{head} conflicts with {context.upstream}; GitHub runs no CI on a conflicted PR",
            *(f"conflict: {path}" for path in files[:20]),
            f"fix: git rebase {context.upstream}, keep both sides' changes, and rerun preflight",
        ))
    elif merge.returncode not in (0, 1):
        results.append(context.skipped("git merge-tree --write-tree is unavailable "
                                       "(Git 2.38 or later)"))
    if behind:
        upstream_paths = set(filter(None, context.git(
            "diff", "--no-renames", "--name-only", context.base, context.upstream).stdout.splitlines()))
        shared = sorted(upstream_paths & set(context.changed_paths()))
        if shared:
            results.append(context.warned(
                f"{behind} commit(s) behind {context.upstream}; upstream merges touched "
                f"{len(shared)} file(s) this branch changes",
                *(f"shared: {path}" for path in shared[:20]),
                f"fix: git rebase {context.upstream}, then recheck the shared files",
            ))
        elif not results:
            results.append(context.passed(
                f"{behind} commit(s) behind {context.upstream}; no shared files"))
    if not results:
        results.append(context.passed(f"up to date with {context.upstream}"))
    if context.worktree:
        dirty = context.git("status", "--porcelain", "--untracked-files=no").stdout.strip()
        if dirty:
            results[-1].details.append("note: the merge check covers commits only, "
                                       "not uncommitted changes")
    return results
