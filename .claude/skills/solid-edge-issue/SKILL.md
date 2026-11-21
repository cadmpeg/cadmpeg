---
name: solid-edge-issue
description: Follow the Solid Edge issue workflow when asked to "work on a Solid Edge issue", "#186x", or "solid edge reader".
---
0. Claim the issue under [CONTRIBUTING.md's claim rules](../../../CONTRIBUTING.md#claiming-an-issue).
1. Read [docs/agents/solid-edge.md](../../../docs/agents/solid-edge.md), its general loop, the issue, #1862 and skeleton PR #1875.
2. Read the repository policies and start from the skeleton base that the guide names.
3. Download the CC0 before/after pair and STEP exports into external scratch storage.
4. List containers, extract streams and compare changed bytes with `cadmpeg inspect`.
5. Check a record-layout hypothesis against every attached variant; ask for missing or distinguishing evidence.
6. Implement the owning module under [AGENTS.md's code rules](../../../AGENTS.md) and recover bad Solid Edge records.
7. Decode native and STEP pairs; compare counts, volume and boxes, and run the issue's How to verify procedure.
8. Update the format model and add synthetic tests for the settled reading.
9. Run the guide's applicable checks; ask about CADIR wire changes and failures outside the change.
10. Complete the draft PR with issue links, counts and check results under [CONTRIBUTING.md's checklist](../../../CONTRIBUTING.md#pull-request-checklist), [LEGAL.md](../../../LEGAL.md) and [the PR template](../../../.github/PULL_REQUEST_TEMPLATE.md).
