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

**Known.** `parasolid.md` §1 "XMT index encoding:" gives a negative `int16` remainder and a `uint16` quotient, with `raw = quotient*32767 + abs(remainder)`. The shared reader applies this formula to node indices, pointer fields, and field extents. The SLDPRT ownership-node and compact-record readers apply the formula in `sldprt.md` §4 "The `00 0c`, `00 0d`, `00 0e`, and `00 13` tags are Parasolid XT node types". The SLDPRT first-use schema reader applies the `parasolid.md` formula.

**Need.** A pointer past `0x7ffe` resolves to a different node under each formula. One formula must hold for both hosts before the SLDPRT topology readers use the shared pointer read.

**Conflict.** `sldprt.md` §4 "The `00 0c`, `00 0d`, `00 0e`, and `00 13` tags are Parasolid XT node types" decodes `(first & 0x7fff) | (second << 15)`. For the cells `f2 7e 00 01` that formula gives 62078, and the `parasolid.md` formula gives 36225.

### XT-02. Edit program that ends before the base list

**Question.** Can a first-use edit program reach `Z` before it consumes every base field?

**Known.** `parasolid.md` §2 "The result contains exactly the declared number of effective fields." states that the remaining base fields need no explicit `D` operations. The shared reader accepts such a program and reports whether it consumed every base field. The SLDPRT ownership-node scanner accepts only a program that consumes every base field.

**Need.** The SLDPRT scanner tests every byte offset, and the complete-consumption rule rejects short type-tag runs inside payload bytes. A sequential SLDPRT node reader can use the `parasolid.md` rule only when that rule holds for SLDPRT streams.

**Conflict.** `sldprt.md` §4.1 "The first node of an XT type carries its schema-reconciliation data" states that `Z` ends a complete edit program when all base fields are consumed.

### XT-03. Field-name bytes in a schema declaration

**Question.** Which byte values can a field name, node name, or node description hold?

**Known.** `parasolid.md` §2 "A type absent from the base schema declares its field count as a byte" defines a short string as a byte count and that many bytes. The shared reader admits nonempty printable ASCII.

**Need.** A declaration with another byte in a name stops the node sequence at that declaration.

**Conflict.** The shared reader is narrower than `parasolid.md` §2.
