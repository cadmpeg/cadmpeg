# Agent guides for contributors

These guides define the work from one cadmpeg issue to a pull request. Each format guide applies the general loop below to one format.

Format guides:

- [Solid Edge](solid-edge.md) (`.par`, `.psm`, `.asm`), tracked in [#1862](https://github.com/cadmpeg/cadmpeg/issues/1862).

## Starting an issue

An issue for this loop starts with an instruction block that includes its URL and the guide path. Its attached repro set contains a small before/after pair of source files and an available reference export of each. Give the block to the agent as written. This example uses the Solid Edge guide:

```
Work on https://github.com/cadmpeg/cadmpeg/issues/1871
Clone https://github.com/cadmpeg/cadmpeg
Follow docs/agents/solid-edge.md
Use the repro files attached to the issue
Open a pull request when the issue's "How to verify" passes
```

## The loop

0. Claim the issue under [CONTRIBUTING.md's claim rules](../../CONTRIBUTING.md#claiming-an-issue).
1. Read the issue, its parent tracking issue, [AGENTS.md](../../AGENTS.md), [CONTRIBUTING.md](../../CONTRIBUTING.md), [LEGAL.md](../../LEGAL.md), [corpus/README.md](../../corpus/README.md) and [docs/source-policy.md](../source-policy.md).
2. Download the attached repro files into a scratch directory outside the repository. Keep downloads there. An issue that requests corpus fixtures must follow the corpus donation rules.
3. Inspect the files with `cadmpeg inspect` (`container`, `extract`, `hex`, `find`, `strings`, `struct`, `cmp`). Diff the before and after files to find the records that the change touches.
4. State a hypothesis of the record layout. Check it against every attached file.
5. Implement the settled reading in the owning codec module under [AGENTS.md's code rules](../../AGENTS.md).
6. Verify the result against the issue's How to verify section. Decode an attached reference export with cadmpeg. Compare body count, face count, volume and bounding box with `cadmpeg query`, `cadmpeg measure` and `cadmpeg diff --geometry`.
7. Write settled byte facts into `docs/formats/<format>.md`, fixed offsets into `docs/layouts/<format>.toml`, and open questions into `docs/formats/<format>-open-items.md`. Follow [AGENTS.md's writing rules](../../AGENTS.md) and [the layout workflow](../layouts/README.md).
8. Add synthetic tests for the settled reading under [AGENTS.md's test rules](../../AGENTS.md).
9. Run the applicable checks from AGENTS.md and CONTRIBUTING.md. Fix failures caused by the change and repeat the affected checks.
10. Complete the draft pull request with a title that states the change, "Part of #<tracking>, fixes #<N>", evidence as counts and check results. Use [the PR template](../../.github/PULL_REQUEST_TEMPLATE.md), [CONTRIBUTING.md's checklist](../../CONTRIBUTING.md#pull-request-checklist) and [LEGAL.md](../../LEGAL.md).

## When to stop and ask

- A required repro variant is missing. Ask on the issue and describe the exact before/after change.
- Two layouts fit the evidence equally well.
- The change needs a CADIR wire change.
- A repository check fails for a reason outside the change.
