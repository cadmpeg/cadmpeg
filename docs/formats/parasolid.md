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
| Large index | negative `int16` remainder + positive `int16` quotient, 4 bytes; `raw = quotient*32767 + abs(remainder)` |

A small index holds `0` through `0x7fff`. For a logical index `i` of at least
32767, the remainder is `-(i mod 32767 + 1)` and the quotient is `i div 32767`.
Both words are `int16`: a remainder word is `0x8001` through `0xffff`, and a
quotient word is `0x0001` through `0x7fff`. The largest raw value is
`32768*32767`. The node indices of one stream are `2` through the node count
plus one.

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

The result contains exactly the declared number of effective fields. `Z` ends
the program once it has produced that count, and the base fields after the base
cursor are deleted. The letters name edit operations.

A type absent from the base schema declares its field count as a byte, then
its name and description as short strings, followed by its fields. A short
string is a byte count and that many printable ASCII bytes. Each field
declares its name as a short string, pointer class as a big-endian u16, and extent as a positive
integer using the biased index encoding. Extent zero is scalar, one is
variable, and a greater extent is a fixed array. A zero pointer class adds the
field type as a short string; a nonzero class identifies a pointer. Variable
fields add their transmit flag as a byte. Fixed fields imply transmit flag
one.

Field presence follows the established list. LIMIT's base fields are `type`
and `hvec`; inserting `term_use` adds the second character before the Hvec
array. INTERSECTION's base construction fields are two `surface` references,
`chart`, `start`, and `end`. Appending `intersection_data` adds a sixth
construction reference. The first-use declaration selects these field edits.

The node-sequence cursor begins after the transmit header. Each declared node
consumes its type, first-use information when required, variable count when
required, index, and transmitted fields. Its endpoint is the next cursor. The
terminator is type `0001` followed by encoded null index `0001`. It closes the
node sequence. Deltas sections continue with the enclosing grammar in
`siemens_nx.md` §4.2.

A node starts at the cursor that the preceding node's endpoint gives. A
type-looking run inside a consumed field is payload. The complete u32
`node_id` domain participates in framing. A BODY's `highest_node_id` checks the
semantic consistency of a framed node. Node ownership requires the preceding
cursor and schema state.

Encoded widths accumulate per field, and each field takes its width from its
own encoding. An extended node index and an extended owner pointer each add two
bytes relative to their short forms. A first-use schema-match byte adds one
byte. Geometric doubles start after every preceding declared field has consumed
its full width.

The wrapper `00 ce` instance owns the stream BODY (`child`), attribute-definition list (`attdef_list`), preview-mesh references (`mesh`/`polyline`/`lattice`), and index-map arrays (`index_map`, `node_id_index_map`, `schema_embedding_map`).

## 3. Mesh face support

Type `201` (`MESH`, description `Mesh`) has this effective scalar field list:

| Field | Encoding |
| --- | --- |
| `node_id` | u32 BE |
| `attributes_features` | pointer, class 1019 |
| `owner` | pointer, class 1007 |
| `next` | pointer, class 1006 |
| `previous` | pointer, class 1006 |
| `geometric_owner` | pointer, class 141 |
| `sense` | character |
| `mesh_box` | box: six f64 BE values |
| `transform` | pointer, class 100 |
| `rcv_key` | pointer, class 1041 |
| `rcv_index` | u32 BE |
| `psm_imesh` | pointer, class 189 |
| `pff_imesh` | pointer, class 188 |

A mesh-support FACE names its MESH node in `surface`. The MESH node's `owner`
names that FACE. The FACE's `shell` names a SHELL whose `body` names the owning
BODY. Several mesh-support faces can belong to one body. A BODY with `state`
value `1` is active. Node indices identify these links within one stream.
The `mesh_box` values are the minimum and maximum for x, y, and z, in that order.

A stream description beginning `: MESH FILE created ` identifies a mesh node
sequence. Its header carries the same schema token, maximum node type, and
user-field size as a transmit node sequence. The auxiliary mesh records have
these effective field lists:

| Type | Name | Fields in wire order |
| --- | --- | --- |
| 189 | `PSM_MESH` | `precision` byte; `owner` pointer class 201; `position_pool` and `normal_pool` pointers class 193; `position_indices` pointer class 191; `normal_type` byte; `normal_indices` pointer class 191; `combs` variable pointer array class 1042 |
| 190 | `INTEGER_TOOTH` | `values` variable u32 BE array |
| 191 | `INTEGER_COMB` | `encoding` byte; `n_integers`, `n_max_integers`, `n_bits_per_integer`, and `shift` u32 BE; `teeth` variable pointer array class 190 |
| 192 | `VECTOR_TOOTH` | `values` variable f64 BE array |
| 193 | `VECTOR_COMB` | `encoding` byte; `n_vectors`, `n_max_vectors`, and `shift` u32 BE; `teeth` variable pointer array class 192 |

The variable fields carry their element count in the node header before the
node index. The `teeth` arrays can contain null pointers after populated teeth.
