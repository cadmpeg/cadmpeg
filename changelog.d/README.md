# Changelog fragments

A fragment is a file `changelog.d/<topic>.md` that adds lines to the release notes. `scripts/release-notes.py` takes the fragments that a release adds after the previous release tag. A fragment that an earlier release already took stays out of the notes, also when it is edited.

```markdown
### Fixed

- One statement for a user of the previous release.
  An indented line continues the bullet.
```

Each bullet goes to the section that its heading names:

- `Breaking`: the Compatibility section, after the IR version line. When a fragment states `Breaking` lines, the `Breaking change` lines of the pull requests are collected in a list below them.
- `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, and `Security`: one section each, in this order, after Compatibility. Another heading is a section after them.
- A bullet before the first heading goes to `Changed`.

The notes take bullets only: other text and HTML comments stay out. Write what changes for the user and what the user does. Name no pull request, branch, or internal tool.
