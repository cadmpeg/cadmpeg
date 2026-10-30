<!-- SPDX-License-Identifier: CC-BY-4.0 -->

# OpenUSD layers: Open Items

This document lists the parts of the OpenUSD layer encodings and schemas that we do not know. The specification `usd.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Attribute names and token values are technical names.

## 1. Schemas

### USD-01. Trim-loop region rule

**Question.** Which part of the `(u, v)` domain of a `NurbsPatch` do the trim loops keep: the region inside an odd number of loops, or the region that the loop orientations select?

**Known.** `usd.md` §14.1 gives the trim-curve encoding. The UsdGeom schema states that the encoding follows the RenderMan `RiTrimCurve` call, and gives no region rule or loop orientation.

**Need.** We must know the rule to map trim loops to outer and inner face loops, and to write face loops as trim loops that other applications read with the same region.
