# Changelog fragments

A fragment is a file `changelog.d/<topic>.md` that adds curated breaking-change lines to a release's `## Compatibility` section. Use one when a release has enough breaking changes (such as an IR wire transition) that users need a concise summary above the per-pull-request list; ordinary per-PR breaking changes belong in the pull request description's `## Breaking change` section.

`scripts/release-notes.py` takes the fragments that a release adds after the previous release tag. A fragment that an earlier release already took stays out of the notes, also when it is edited.

```markdown
### Breaking

- One statement for a user of the previous release.
  An indented line continues the bullet.
```

Bullets under `### Breaking` (or `### Breaking change(s)`) go to `## Compatibility`, after the IR version line, and the pull requests' `## Breaking change` lines are collected in a collapsed list below them. Other headings, prose, and HTML comments stay out of the notes. Write what changes for the user and what the user does. Name no pull request, branch, or internal tool.
