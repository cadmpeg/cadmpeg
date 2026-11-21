---
name: solid-edge-issue
description: Follow the Solid Edge issue workflow when asked to "work on a Solid Edge issue", "#186x", or "solid edge reader".
---
0. Claim the issue: comment /claim on it, then open a draft pull request linked to the issue within 24 hours, even a rough one, and keep it moving; CONTRIBUTING.md gives the claim rules.
1. Read [docs/agents/solid-edge.md](../../../docs/agents/solid-edge.md), its general loop, the issue, #1862 and skeleton PR #1875.
2. Read the repository policies and start from the skeleton base that the guide names.
3. Download the CC0 before/after pair and STEP exports into external scratch storage.
4. List containers, extract streams and compare changed bytes with `cadmpeg inspect`.
5. Check a record-layout hypothesis against every attached variant; ask for missing or distinguishing evidence.
6. Implement the owning module with `View` reads and `SolidEdgeLossCode` notes; recover bad records and preserve resource refusals.
7. Decode native and STEP pairs; compare counts, volume and boxes, and run the issue's How to verify procedure.
8. Update settled semantics, fixed-offset tables and open items; add synthetic tests with the invariant's owner.
9. Run the guide's applicable checks; ask about CADIR wire changes and failures outside the change.
10. Prepare the PR with issue links, counts, check results, the DCO box and the truthful LEGAL.md declaration.
