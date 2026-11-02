# Parasolid transmit stream: Open Items

This document lists the parts of the Parasolid transmit-stream format that we do not know. The specification `parasolid.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

A reference to the specification gives the section number and the start of the paragraph. Do not use line numbers. The `scripts/check-doc-anchors.py` command makes sure that each reference finds exactly one paragraph.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Encoding

### XT-01. Large XMT index value

**Question.** Which formula gives the value of a four-byte XMT index?

**Known.** `parasolid.md` §1 "XMT index encoding:" gives a negative `int16` remainder and a nonzero `uint16` quotient, with `raw = quotient*32767 + abs(remainder)`. `records::positive` in cadmpeg-parasolid applies this formula to node indices, pointer fields, and field extents of the node sequence. `pointer::read` in cadmpeg-parasolid applies the formula of `sldprt.md` §4 "The `00 0c`, `00 0d`, `00 0e`, and `00 13` tags are Parasolid XT node types", and the SLDPRT record readers use it. Across 1179 SolidWorks partition and plain streams, the node indices of each stream form the range `2` through the node count plus one under the `parasolid.md` formula; the `sldprt.md` formula leaves gaps in that range.

**Need.** A pointer past `0x7fff` resolves to a different node under each formula, and both readers live in one crate. The SLDPRT readers and `pointer::read` must apply one formula before SLDPRT reads its topology through the shared node sequence.

**Conflict.** `sldprt.md` §4 "The `00 0c`, `00 0d`, `00 0e`, and `00 13` tags are Parasolid XT node types" decodes `(first & 0x7fff) | (second << 15)`. For the cells `f2 7e 00 01` that formula gives 62078, and the `parasolid.md` formula gives 36225.
