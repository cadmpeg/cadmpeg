# Agent guides for contributors

These guides let a contributor point an AI coding agent at one cadmpeg issue and get to a pull request with little hand-holding. Each format-specific guide in this directory applies the general loop below to one format.

Format guides:

- [Solid Edge](solid-edge.md) (`.par`, `.psm`, `.asm`), tracked in #1862.

## Starting an issue

An issue written for this loop starts with a copy-paste block that names the issue's own URL, and it has a repro set attached: a small before/after pair of source files, plus a STEP export of each where the format has one. Paste the block into the agent. Nothing in it needs editing.

## The loop

1. Read the issue, its parent tracking issue, [AGENTS.md](../../AGENTS.md), [CONTRIBUTING.md](../../CONTRIBUTING.md) and [LEGAL.md](../../LEGAL.md).
2. Download the attached repro files into a scratch directory outside the repository. Do not commit them unless the issue says they are corpus fixtures.
3. Inspect the files with `cadmpeg inspect` (`container`, `extract`, `hex`, `find`, `strings`, `struct`, `cmp`). Diff the before and after files to find the records that the change touches.
4. State a hypothesis of the record layout. Check it against every attached file.
5. Implement the read in the owning codec module. Read bytes through `View`. Report a record that does not read as a loss and continue; never fail the whole file for one record.
6. Verify the result as the issue's **How to verify** section says. Where a STEP export is attached, compare body count, face count, volume and bounding box.
7. Write settled byte facts into `docs/formats/<format>.md`, fixed offsets into `docs/layouts/<format>.toml`, and open questions into `docs/formats/<format>-open-items.md`.
8. Add synthetic tests built from the specification. Never paste file bytes into a test.
9. Run the repository checks that [AGENTS.md](../../AGENTS.md) names.
10. Open a pull request that names the issue ("Part of #<tracking>, fixes #<N>"), states the evidence as counts, and carries the sign-off box and the provenance sentence from the pull request template.

## When to stop and ask

- A repro variant that the work needs is not attached. Ask on the issue and describe the exact before/after change.
- Two layouts fit the evidence equally well.
- The change needs a CADIR wire change.
- A repository check fails for a reason outside the change.

## Clean room

Use file bytes and public information only. Do not use vendor SDKs, API documentation, decompiled code, or any source that [LEGAL.md](../../LEGAL.md) forbids.
