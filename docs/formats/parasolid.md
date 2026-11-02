# Parasolid transmit stream: Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

A Parasolid transmit stream serializes one modelling-kernel partition as a binary transmit header and a node sequence. Siemens NX `.prt` files carry these streams as described in [`siemens_nx.md`](siemens_nx.md) §3. SolidWorks `.sldprt` files carry them as described in [`sldprt.md`](sldprt.md) §3. This document gives the stream grammar that both hosts share.

## 1. Neutral-binary encoding

The binary transmit header begins with `PS`, a big-endian u32 description
length and its ASCII bytes, then a big-endian u32 schema-token length and its
ASCII bytes. `SCH_<modeller>_<schema>` selects that schema directly.
`SCH_<modeller>_<schema>_<base>` selects embedded schema edits relative to
`<base>`; `13006` is a base-schema number. An embedded-schema header adds the
maximum node type as a big-endian u16. The following big-endian u32 is the
user-field size.

XMT index encoding:

| Form        | Encoding                                                                                         |
| ----------- | ------------------------------------------------------------------------------------------------ |
| Small index | `uint16` BE, 2 bytes                                                                             |
| Large index | negative `int16` remainder + `uint16` quotient, 4 bytes; `raw = quotient*32767 + abs(remainder)` |

An encoded index is one greater than the logical node index; encoded `0001`
denotes the null reference. An extended pointer occupies four bytes instead of
two and shifts every later field by two bytes. Embedded schema information
precedes the ordinary variable count and index only at the first use of a node
type. A later `ff` at the index position is part of the index encoding.

## 2. Schema self-description

An embedded-schema stream maintains one field list for each node type. Its
first occurrence inserts schema information between the type and the ordinary
variable count and index. Later occurrences use that established field list
without another schema marker. A direct-schema stream uses its named schema
without embedded first-use information.

An effective base field has transmit flag one or variable extent. For a node
type present in the base schema, first-use byte `ff` declares an identical
effective field list. Any other first-use byte is the current effective-field
count, followed by an edit program:

| Operation | Effect |
| --- | --- |
| `C` | Copy the current base field and advance the base cursor. |
| `D` | Delete the current base field and advance the base cursor. |
| `I` | Read an inserted field without advancing the base cursor. |
| `A` | Read an appended field after the base fields are exhausted. |
| `Z` | End the edit program. |

The result contains exactly the declared number of effective fields. When the
current field list ends before the base list, the remaining base fields need
no explicit `D` operations. The letters name edits, not instance field types.

A type absent from the base schema declares its field count as a byte, then
its name and description as short strings, followed by its fields. A short
string is a byte count and that many bytes. Each field declares its name as a
short string, pointer class as a big-endian u16, and extent as a positive
integer using the biased index encoding. Extent zero is scalar, one is
variable, and a greater extent is a fixed array. A zero pointer class adds the
field type as a short string; a nonzero class identifies a pointer. Variable
fields add their transmit flag as a byte. Fixed fields imply transmit flag
one.

Field presence follows the established list. LIMIT's base fields are `type`
and `hvec`; inserting `term_use` adds the second character before the Hvec
array. INTERSECTION's base construction fields are two `surface` references,
`chart`, `start`, and `end`. Appending `intersection_data` adds a sixth
construction reference. Neither the counted transmit description nor a
following type tag selects these field edits.

The node-sequence cursor begins after the transmit header. Each declared node
consumes its type, first-use information when required, variable count when
required, index, and transmitted fields. Its endpoint is the next cursor. The
terminator is type `0001` followed by encoded null index `0001`. It closes the
node sequence. Deltas sections continue with the enclosing grammar in `siemens_nx.md` §4.2.

A type-looking run inside an already consumed field is payload. Graph
completion, typed-reference joins, type-map membership, a following boundary,
and node-ID magnitude do not establish a record start. The complete u32
`node_id` domain participates in framing. A BODY's `highest_node_id` can check
an already framed node's semantic consistency; it does not select its frame.
An unavailable preceding cursor or schema state leaves ownership unknown.

Encoded widths accumulate per field. An extended node index and an extended
owner pointer each add two bytes relative to their short forms. The node
index does not set the width of any pointer field. A first-use schema-match
byte adds its own byte independently of both extensions. Geometric doubles
start only after every preceding declared field has consumed its full width.

The wrapper `00 ce` instance owns the stream BODY (`child`), attribute-definition list (`attdef_list`), preview-mesh references (`mesh`/`polyline`/`lattice`), and index-map arrays (`index_map`, `node_id_index_map`, `schema_embedding_map`).
