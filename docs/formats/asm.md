# ASM and ACIS kernel stream: Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/asm.md`](../layouts/asm.md), generated from `docs/layouts/asm.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. `cargo test -p cadmpeg --test layout_tables` proves the two agree.

An ASM or Spatial ACIS stream serializes one B-rep model as a header, a table of framed records, and an optional construction-history partition. The stream identifies its branch through its binary magic or text terminator. Fusion `.f3d` containers carry ASM streams in the binary and text encodings named in [`f3d.md`](f3d.md) §2. Inventor part carriers can contain the ASM binary branch or the ACIS 217/218 binary branch.

## 1. ASM binary header

Streams begin with the 15-byte magic `ASM BinaryFile8` or `ASM BinaryFile4`; byte 15 is the low byte of the ACIS save-format version word, not part of the magic. The digit selects the width of integer/ref tags (§2): `4` → tag + 4-byte LE signed; `8` → tag + low 32 bits + high 32 bits (consume the full 9-byte field). Both widths occur.

`BinaryFile8` header layout (little-endian, mirroring `BinaryFile4` with wider words):

| Bytes    | Meaning                                                                                |
| -------- | -------------------------------------------------------------------------------------- |
| `0..15`  | magic `ASM BinaryFile8`                                                                |
| `15..19` | little-endian u32 ACIS save-format version (`major * 100 + minor`)                     |
| `19..31` | zero                                                                                   |
| `31..39` | little-endian u64 entity-count word                                                    |
| `39..47` | little-endian u64 flags; bit 0 is set iff the stream carries a history partition (§3) |

The string region begins at byte 47.

`BinaryFile4` header layout (the classic ACIS save header, little-endian):

| Bytes    | Meaning                                                                                |
| -------- | -------------------------------------------------------------------------------------- |
| `0..15`  | magic `ASM BinaryFile4`                                                                |
| `15..19` | little-endian u32 ACIS save-format version (`major * 100 + minor`)                     |
| `19..23` | little-endian u32 record count (`0` when unwritten)                                    |
| `23..27` | little-endian u32 entity count                                                         |
| `27..31` | little-endian u32 flags; bit 0 is set iff the stream carries a history partition (§3) |

The string region begins at byte 31.

ACIS 217 and 218 binary streams use this 31-byte fixed prefix:

| Bytes    | Meaning                                                                                |
| -------- | -------------------------------------------------------------------------------------- |
| `0..15`  | magic `ACIS BinaryFile`                                                                |
| `15..19` | little-endian u32 ACIS save-format version (`major * 100 + minor`)                     |
| `19..23` | little-endian u32 record count (`0` when unwritten)                                    |
| `23..27` | little-endian u32 entity count                                                         |
| `27..31` | little-endian u32 flags; bit 0 is set iff the stream carries a history partition (§3) |

The ACIS string region begins at byte 31. Its three strings, three tolerance values, 32-bit SAB tags, RecordTable indexing, and solved/history boundary use the grammar below.

In both widths the remaining header is a sequence rather than a fixed-offset structure:

```
0x07 u8_len UTF8[product_family]
0x07 u8_len UTF8[product_version_string]
0x07 u8_len UTF8[save_date]
0x06 f64_le scale
0x06 f64_le resabs
0x06 f64_le resnor
```

Header invariants:

- Every header word in either width is little-endian.
- The entity-count and flags words carry stream metadata, not model-space quantities. In both widths, the entity-count word is the RecordTable index of the first owned record: `asmheader` occupies index 0, the stream's saved top-level entities (bodies, free faces, and free wire edges) occupy indices `1..entity_count` with an exclusive upper bound, no record in that range is referenced by an earlier record, and the record at index `entity_count` is the first record referenced from that range.
- The flags word's bit 0 marks a history partition in both widths. Bits 1 to 7 hold the save format's revision number: the stream's full format version is `save_format × 100 + ((flags >> 1) & 0x7f)`. Save format 22300 carries revision 2 (version 2230002) and save format 22500 carries revision 3 (version 2250003). Bits 8 and above are zero. All bits above 0 are preserved.
- `scale`, `resabs`, and `resnor` are kernel metadata. `scale` is not a coordinate transform; it varies per stream and is not fixed by width or save format. Both widths use `resabs = 1e-6` and `resnor = 1e-10`.

---

## 2. Tag encoding and record framing

The stream is a tag-typed SAB (ACIS binary) token stream.

### 2.1 Tag table

| Tag                         | Symbol               | Payload                   | Meaning                                                           |
| --------------------------- | -------------------- | ------------------------- | ----------------------------------------------------------------- |
| `0x02`                      | CHAR                 | 1 B                       | unsigned 8-bit                                                    |
| `0x03`                      | SHORT                | 2 B                       | signed 16-bit                                                     |
| `0x04`                      | LONG                 | ref_size                  | signed int (32 or 64-bit per header)                              |
| `0x05`                      | FLOAT                | 4 B                       | IEEE float32                                                      |
| `0x06`                      | DOUBLE               | 8 B                       | IEEE float64                                                      |
| `0x07`/`0x08`/`0x09`/`0x12` | UTF-8 string         | 1/2/ref_size/ref_size + N | length-prefixed string (8-bit, 16-bit, ref_size, ref_size length) |
| `0x0A`                      | TRUE                 | 0 B                       | logical true (data token, **not** a terminator)                   |
| `0x0B`                      | FALSE                | 0 B                       | logical false / sentinel                                          |
| `0x0C`                      | ENTITY_REF           | ref_size                  | RecordTable index                                                 |
| `0x0D`                      | IDENT                | 1 + N                     | record/class name token (leaf)                                    |
| `0x0E`                      | SUBIDENT             | 1 + N                     | base-class name token                                             |
| `0x0F` / `0x10`             | SUBTYPE_OPEN / CLOSE | 0 B                       | brace-balanced subtype delimiters                                 |
| `0x11`                      | TERMINATOR           | 0 B                       | end of current record                                             |
| `0x13`                      | POSITION             | 24 B                      | 3D point (3×f64)                                                  |
| `0x14`                      | VECTOR_3D            | 24 B                      | 3D vector (3×f64)                                                 |
| `0x15`                      | ENUM_VALUE           | ref_size                  | enumeration / secondary integer                                   |
| `0x16`                      | VECTOR_2D            | 16 B                      | 2D `(u,v)`                                                        |
| `0x17`                      | INT64                | 8 B                       | AutoCAD int64 attribute value                                     |

- `0x11` terminates the current top-level record; the next record's name-token chain begins at the following byte.
- `0x0A`/`0x0B` inside a record are booleans (often `reversed`/`forward`), **never** record boundaries.
- Positions (`0x13`) and length-bearing vectors are centimetres; see §4.
- The tag space is closed at `0x17`. A byte outside the table at an item position is not an item: it means framing lost synchronization earlier in the stream. The two length prefixes that are `ref_size` rather than fixed — `0x09` and `0x12` — are the usual cause, because reading either as a fixed four bytes in a `ref_size` 8 stream both starts four bytes early and stops four bytes short, leaving the payload's trailing bytes to be read as tags. `0x17` is the one tag whose payload width does not follow `ref_size`; it is 8 bytes in either width.

### 2.2 Record names and the RecordTable

A record name is the `-`-joined chain of all `0x0E` tokens terminated by one `0x0D` leaf token (e.g. `persubent-acadSolidHistory-attrib`). In assembled record names, the class token `ASM` is represented as `ACIS`.

**RecordTable indexing:** the stream begins with an `asmheader` record (not preceded by `0x11`) at **index 0**. `RecordTable[1]` is the first record after it, and so on. Positive `0x0C` refs index this table directly; `-1` is null.

The `asmheader` row participates in RecordTable indexing; the first following entity therefore has index 1.

### 2.3 Version/product gates

Other pure ACIS and SpaceClaim SAB envelopes use separate version-gated record layouts and string interning. The byte layouts in §§5–6 apply to ASM streams and the ACIS 217/218 Inventor carrier envelope. Other ACIS save-format bands are not admitted by this grammar.

---

## 3. History partition

A history-bearing stream contains solved-model records followed by a
`history_stream` record and linked `delta_state` records. The framed
`Begin-of-ASM-History-Data` record is the byte boundary between the
solved-model record sequence and construction history; identifier bytes
occurring inside another record do not define a boundary. In a history-less
stream the final `End-of-ASM-data` record ends at the end of the stream
without a trailing `0x11` terminator, mirroring the history tail rule (§3.2).

### 3.1 History preamble

The history-container record begins with this tag-segmented name chain:

```
11 0e 05 "Begin" 0e 02 "of" 0e 03 "ASM" 0e 07 "History" 0d 04 "Data"  0d 0e "history_stream"
```

The class lineage is `Begin-of-ASM-History-Data`; `history_stream` is the second leaf token. Its body begins with:

```
04 i64 stream_size
04 i64 stream_size_duplicate
04 i64 = 0
04 i64 history_entry_count
0c ref[4]
11
```

`stream_size == stream_size_duplicate`.

An ACIS 217/218 history partition begins directly with the first exact `delta_state` record. The record framer establishes the preceding solved-record boundary; identifier bytes inside payload strings do not select it.

### 3.2 `delta_state` records

Each history node is a linked construction state:

```
11 0d 0b "delta_state"
04 i64 state_id          (head node's state_id == history_stream preamble field[0])
04 i64 = 1               (constant)
04 i64 = 0               (constant)
0c ref prev_state        (-1 on head)
0c ref next_state        (-1 on tail)
0c ref node_index        (0,1,2,... sequential)
0c ref = -1
0c ref = 0
0b                       (false sentinel)
```

Each `delta_state` body contains a BulletinBoard chain. A bulletin entry stores an old and new entity reference: null→entity is insertion, entity→null is deletion, and entity→entity is update.

The BulletinBoard chain closes with its tagged-zero terminator. A second `04 0 11` sequence separates the state body from the following record sequence. On a non-tail state, that `0x11` is the next `delta_state` record delimiter and the state owns no intervening entity records. The tail state is followed by `End-of-ASM-History-Section`, the retained history entity snapshot, and `End-of-ASM-data`. These records use the ordinary SAB name-chain and payload grammar. An `End-of-ASM-History-Section` name chain immediately followed by an `edge` identifier wraps an edge snapshot record; the fields after that identifier use the ordinary edge payload layout. Their ordered `0x0c` entity-reference tokens, including `-1` null references, use the construction-history revision namespace and are retained independently of the snapshot's local record ordinal. The old references across the BulletinBoard changes form a unique contiguous interval immediately after the active RecordTable. Snapshot local ordinal zero has the interval's first revision identity, and each following snapshot record increments that identity by one. `End-of-ASM-data` is excluded from the interval.

The linked `delta_state` chain runs from the current model toward the initial model. Historical entity membership is reconstructed from the identity map of the active RecordTable. Reversing an update `old -> new` keeps stable entity slot `new` and changes its record revision to `old`. Reversing an insertion `null -> new` removes slot `new`. Reversing a deletion `old -> null` adds slot `old` with record revision `old`. A complete chain accepts every transition without a missing slot or record revision and terminates at the singleton map `0 -> 0` for `asmheader`. Each state retains the complete sorted entity-slot-to-record-revision map before its own changes are reversed. Incomplete chains retain no partial state maps.

Every old revision belongs to stable entity slot `new` for an update and slot `old` for a deletion. Materializing a state selects its recorded revision for each live entity slot, frames that active or archived record, replaces every non-null entity reference with the referenced revision's stable entity slot, and sorts the records by stable slot. A materialized table is complete only when every selected record frames, every revision has one stable slot, and every normalized reference names a live entity in the state. Completeness is assigned atomically across the linked state chain. The resulting sparse RecordTable uses the ordinary active-model topology and carrier grammars. Each complete state has stable entity-slot membership for its bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves, and pcurves. Body-to-region, region-to-shell, shell-to-face, face-to-loop, and loop-to-coedge relations are ordered. Shell wire edges and free vertices are ordered independently. Each coedge retains its owning loop, edge, next, previous, radial-next, and optional pcurve slots. Each edge retains its ordered start and end vertex slots and optional curve slot. Each face retains its surface slot, and each vertex retains its point slot. A state's `next` state is its preceding modeling state. The forward transition from `next` to the current state partitions normalized record slots and every topology and geometry family into inserted, deleted, and updated sets; an updated slot exists in both states and selects different record revisions. The final `End-of-ASM-data` record ends at the enclosing stream boundary without a trailing `0x11`; EOF terminates only that final history record.

---

## 4. Unit rules

- Model-space lengths are stored in centimetres in both widths.
- Model-space points, radii, length-bearing vectors, 3D control points, and length tolerances convert to millimetres by ×10.
- Unit vectors, ratios, angles, knot parameters, non-length enums, and homogeneous weights are dimensionless. Pcurve coordinates use the parameter units of their support carrier.
- The header `scale` field is metadata, not a coordinate multiplier (§1).

An analytic surface is untrimmed; its extent is independent of the face's vertex hull.

---

## 5. Topology records

### 5.1 Ownership graph

```
body → lump → shell → [subshell] → face → loop → coedge → edge → vertex → point
```

Authoritative binding links:

| Link               | Field              |
| ------------------ | ------------------ |
| face → surface     | `face.chunk[7]`    |
| edge → 3D curve    | `edge.chunk[8]`    |
| coedge → UV pcurve | `coedge.chunk[10]` |
| vertex → point     | `vertex.chunk[5]`  |

Every `Entity` record begins with an `attrib` ref (chain head, `-1` if none) and a `history` int (present when `ver > 6.0`). The `Geometry` subclass consumes an extra ref slot before its concrete payload.

### 5.2 ASM byte layouts (`BinaryFile8`, fixed sizes)

All records of a given class are fixed-size in ASM streams. Offsets are record-relative from the leading `0x11`; ref/int chunks are 9 bytes. On `BinaryFile4` streams ref/int chunks are 5 bytes and the offsets scale accordingly.

**Body (61 B):** `chunk[1]` (@+16, i64) is `history / body flags`, the body selector joined to a product-side body map (for Fusion, [`f3d.md` §3.1](f3d.md#31-design-metadata)). A nonnegative value is the body's `asm_body_key`; `-1` is a null key. The field is retained for every body independently of whether a Design join resolves, and native writing preserves or patches it directly. `chunk[3]` @+34 = first_lump, `chunk[4]` @+43 = first_wire or `-1`, `chunk[5]` @+52 = transform or `-1`.

**Lump (61 B):** `chunk[0]` is the attribute-chain head, `chunk[3]` is the next sibling lump, `chunk[4]` @+43 = first_shell, and `chunk[5]` @+52 = owner_body. (The @+27 slot is reserved `-1`, not the first shell.)

**Shell (80 B):** `chunk[0]` is the attribute-chain head, `chunk[3]` is the next sibling shell, `chunk[5]` @+53 = first_face, `chunk[6]` = wire, and `chunk[7]` = owner.

**Subshell:** after the entity base header, `chunk[3]` = owner shell or parent subshell,
`chunk[4]` = next sibling subshell, `chunk[5]` = first child subshell,
`chunk[6]` = first face, and `chunk[7]` = wire. Subshell faces are projected onto
their nearest shell ancestor in the neutral IR; retained-source writing preserves
the native subshell records and ownership references byte-for-byte.

**Wire:** `chunk[3]` = next sibling wire, `chunk[4]` = first coedge,
`chunk[5]` = owner shell/body/subshell, `chunk[6]` = isolated vertex, and
`chunk[7]` = side (`0x0a` in, `0x0b` out). The member references are mutually
exclusive: an edge wire has a non-null first coedge and a null isolated vertex;
a point wire has a null first coedge and a non-null isolated vertex. Coedges form
an ordered closed ring. A point wire's vertex names that wire as its owner and
uses endpoint index `-1`. Wires owned by a subshell project onto the nearest shell
ancestor. Each wire record retains its ordered edges or isolated vertex and side
as typed metadata on the normalized shell. Retained writing patches the side token
in place. Source-less writing emits one edge-ring wire for a shell's ordered wire
edges and one point wire for each ordered free vertex. These records form the
shell's sibling-wire chain. Each record's side is selected from metadata matching
that exact edge list or free vertex and defaults to out when no match exists.

**Face (81 B; +1 chunk if double-sided):**

```
+16 chunk[1] history / face flags
+34 chunk[3] next_face
+43 chunk[4] first_loop
+52 chunk[5] owner_shell
+70 chunk[7] surface REF        ← the ONLY authoritative face→surface binding
+79 chunk[8] sense  (0x0a=reversed, 0x0b=forward)
+80 chunk[9] sides  (0x0b=single)
+81 chunk[10] containment       ← PRESENT ONLY IF chunk[9]=double
```

A nonnegative `chunk[1]` value is the face's `asm_face_key`, used by embedding formats for Design-side joins. `-1` is a null key. The field is retained independently for every emitted face.

`sides` and `containment` are separate enum chunks. Single-sided faces end after `sides`; double-sided faces carry `containment`.
The sense token is relative to the native surface carrier. Decoding a reversed spline carrier or an inward-normal cone carrier reverses the sense in the normalized B-rep while retaining the native token; writing applies the same reversal back to the token.

**Loop (61 B):** `chunk[3]` @+34 = next_loop (`-1` terminates the chain), `chunk[4]` @+43 = first_coedge, `chunk[5]` @+52 = owner_face. Loop order is defined by the `next_loop` references, not stream position; the first loop is not an outer-loop marker.

An untrimmed closed analytic face stores `-1` in `chunk[4]` (first_loop) and owns no loops, coedges, edges, or vertices. Full spheres and tori use this loopless form; the closed surface carries no seam, pole, or periphery loop and no degenerate edge. A `degenerate_curve` never bounds a loopless closed face; it occurs only as an edge curve at a cone apex or sphere pole of a trimmed face, or nested inside a procedural-surface construction.

**CoEdge (100 B):**

```
+35 chunk[3] next_coedge   +44 chunk[4] prev_coedge   +53 chunk[5] partner_coedge
+62 chunk[6] edge          +71 chunk[7] sense byte
+72 chunk[8] owner_loop    +81 chunk[9] reserved int (const 0)
+90 chunk[10] pcurve ref (or -1)
```

The `{+35,+44,+53}` triad is next/prev/partner. `+72` is the owner loop. **Partner symmetry** is a manifold invariant: every coedge's partner's partner is itself, and every shell edge is shared by exactly two mutually-referencing coedges of opposite sense.

`tcoedge` inherits this complete base field sequence. `chunk[11]` and `chunk[12]` are its native start and end parameters. Releases below 215 have no fixed extension fields. Releases from 215 through 219 store a nullable reference in `chunk[13]`. Modern releases store a nullable reference in `chunk[13]` and a LONG selector in `chunk[14]`. Selector zero is followed by LONG zero and terminates the record. Selector one inlines an `intcurve`: `chunk[15]` is its sense Boolean and `chunk[16]` opens one balanced subtype containing a 3D NURBS coedge-use curve. False evaluates the serialized spline forward; true evaluates it with parameter negation as `C(-t)`. The reversed form's outer interval, and its optional trailing interval when present, are the negated endpoint-swapped serialized-curve range. The fields after the matching outer `SUBTYPE_CLOSE` are either `FALSE, FALSE, LONG 0`, denoting no trailing interval, or `TRUE, f64 start, TRUE, f64 end, LONG 0`, denoting an explicit trailing interval. The explicit trailing interval overrides `chunk[11..=12]` for the neutral coedge use curve; the two-false form leaves that use curve on the outer tolerant interval. A cache-local selector-one extension has a null leading reference and owns its embedded curve. Native generation writes that curve inside one balanced subtype. The subtype has no serialized token count; nested subtype scopes do not terminate it, and its matching close delimiter bounds the curve payload. These extension fields do not change the offsets or meanings of the base topology links.

**Edge (98 B):**

```
+34 chunk[3] start_vertex   +43 chunk[4] t_start (f64)
+52 chunk[5] end_vertex     +61 chunk[6] t_end (f64)
+70 chunk[7] owner_coedge   +79 chunk[8] curve ref
+89 chunk[9] sense byte     +90 0x07 'tangent'|'unknown' continuity text
```

`+52` is end_vertex and `+79` is curve, not the other way round. `owner_coedge` is a nullable back-reference selecting one use of the edge; it is retained independently of the radial-ring topology, validated against the selected coedge's edge, and written in both retained and source-less output. `t_start`/`t_end` are stored parameters on the edge's own parameterization: the referenced curve itself when the sense byte is forward (`0x0b`), its reverse `E(t) = C(−t)` when reversed (`0x0a`). A full-circle edge has identical start/end vertex with `t_start = -π`, `t_end = +π`; the shared vertex lies at the `t_start` angle from the major axis, so a full period's phase is significant, not a free normalization. The continuity text is descriptive metadata, **not** a curve-type discriminator.

When the curve reference is null, the edge has no attributed 3D carrier. Its serialized endpoint doubles remain finite optional `Edge.param_range` values, but no canonical carrier-domain ordering is applied to them.

A closed cylindrical band may use two loops, each containing one self-linked coedge on a full-circle edge. The two circular edges retain their distinct repeated vertices and full-period parameter phases. No seam edge or seam coedge occurs in this native topology.

`tedge` carries this complete base field sequence followed by `chunk[11]` as an f64 model-space tolerance, `chunk[12]` as the LONG per-entity serializer revision stamp (the release ×100 family, e.g. `22601`), and `chunk[13]` as a trailing LONG holding a small non-negative per-entity change counter (`0` is the default for a freshly built entity; rewrites preserve the stored value), present when the stream's format version is at least 2250003 and absent below. The tolerance is a model-space length in the stream's length unit and scales with the record's coordinates when that unit changes. The two LONG fields are retained verbatim. The extension does not change the base endpoint, curve, sense, or continuity fields.

**Vertex (63 B):** `chunk[3]` @+36 = owning_edge, `chunk[4]` @+45 = index_flag (`0` = this is the owning edge's START vertex, `1` = its END vertex), `chunk[5]` @+54 = point ref. Each vertex has its own point entity; no deduplication.

**Tolerant vertex:** `tvertex` carries the complete vertex field sequence followed by `chunk[6..=8]` as three f64 model-space tolerance slots and `chunk[9]` as a trailing LONG holding the same per-entity change counter, present when the stream's format version is at least 2250003 and absent below — the same gate as the `tedge` trailing LONG — and retained verbatim. The slots are three tolerance evaluations of successive generations; a `-1` slot denotes an unset evaluation, and any slot can be unset, including all three. When the third slot is unset the second is also unset; the converse does not hold, and a set second slot beside an unset third does not occur. When both the second and third slots are set they are non-decreasing in slot order and differ by at most `1e-6`. These two relations hold of the values an evaluation produces; they are not read constraints, and a pair that breaks either one is read and retained. `-1` is the only negative value a slot takes, and a set slot is never zero: the second and third slots are bounded below by `1e-11`. Every set slot is a model-space length in the stream's length unit and scales with the record's coordinates when that unit changes; the `-1` sentinel is a marker rather than a length and does not scale. The second slot is the largest of two families of terms taken over the vertex's incident edges: the gap between the vertex point and the edge curve's evaluated endpoint plus `1e-11`, and, for each incident tolerant edge, that gap plus the edge's own tolerance plus `1e-10`. The third slot raises the second over the incident edges that are not tolerant to that gap plus `1e-6`. A set slot is stored rather than recomputed on read and survives a read-and-write cycle unchanged apart from length conversion. The first slot carries the vertex's own stored tolerance: a construction that supplies a per-vertex tolerance stores that value in the first slot, converted with the stream's length unit like the other two slots, and a construction that supplies none leaves the slot unset. A set first slot takes no part in the second and third slots' evaluation and does not exceed the second slot. An unset third slot does not survive a read-and-write cycle: the second and third slots are both evaluated and stored. A set third slot survives together with the second slot's stored content, and that content can be the unset sentinel. A set last slot is the vertex tolerance. The trailing LONG counts entity changes and is independent of the three slots.

**Transform (142 B):** 13×f64 (@+18..117): `a[0..8]` 3×3 rotation, `a[9..11]` translation, `a[12]` overall scale; then ROTATION, REFLECTION, and SHEAR boolean-enum bytes in that order (`0x0a` selects the named property, `0x0b` selects `no_*`). Column mapping: `a[0..2]`→col0, `a[3..5]`→col1, `a[6..8]`→col2, `a[9..11]`→col3. The body references its transform through `body.chunk[5]`; null denotes no body transform. Native writing retains the three classifications independently of the matrix and emits all three fields.

### 5.3 Point records and coordinate authority

A BinaryFile8 `point` record is 60 bytes: the 8-byte record head, three 9-byte entity-base fields, and one 25-byte model-space `POSITION`. The record terminates immediately after the position and carries no trailing reference-count integer. `vertex.chunk[5]` references the point record. NURBS control grids independently carry their model-space poles.

### 5.4 Sense semantics

Three sense bits compose into the winding:

- **face.sense**: forward = surface's natural normal, reversed = flipped.
- **coedge.sense**: loop-traversal direction relative to the edge curve parameterization.
- **edge.sense**: the edge's own curve-parameterization sense. A reversed edge parameterizes as the negation of its curve (`E(t) = C(−t)`); its `t_start`/`t_end` and vertex order are on that reversed parameterization.

**Winding rule:** `effective_curve_reversed = edge.sense_reversed XOR coedge.sense_reversed`. Each edge has two coedges with opposite `effective_curve_reversed`.

### 5.5 Ownership reachability

Topology membership is defined by references from `body → lump → shell → face → loop → coedge → edge → vertex`. Surface, curve, and point membership follows the authoritative binding references in §5.1.

An edge with `owner_coedge_ref == -1` and no reference from a reachable coedge is outside that ownership graph.

### 5.6 Attributes on the topology graph

Every entity carries an `attrib` ref-chain. `Entity.attrib` is the chain head. A current attribute starts with `REF reserved`, `INTEGER marker`, `REF next`, `REF previous`, and `REF owner`. Its forward, backward, and owner fields are payload fields 2, 3, and 4. A legacy attribute omits the marker. Its forward, backward, and owner fields are payload fields 1, 2, and 3. The reserved, marker, and absent-link values are `-1`, and `-1` terminates either direction. An attribute owner can be another attribute; following owner fields terminates at the topology entity that owns the complete nested attribute set. A format-231 `ATTRIB_CUSTOM-attrib` record carries its owner ref at record-relative `+60..68` and a family name (`generic_tag_attrib_def`, `sketch_attrib_def`, `Timestamp_attrib_def`, `FPM_tracked_attrib_def`, `NEUTRON_Material_attrib_def`). Attribute records are variable-width.

Color and feature-tag attributes can coexist on one chain. Forward chain order gives color precedence. The first well-formed self-contained direct-color record defines the entity color. `rgb_color-st-attrib` carries three normalized f64 channels; format-231 binary records also carry a terminal f64 `1`. `truecolor-adesk-attrib` carries one Autodesk method-and-color integer: bits 24..31 are the color method, bits 16..23 are red, bits 8..15 are green, and bits 0..7 are blue. It defines direct RGB only when the method is `0xc2` (`ByColor`). `entatt_color-bt-attrib` carries one nonempty decimal-digit string whose integer is at most `0xffffff`, with the same red, green, and blue bit positions. These direct forms define alpha as one. An out-of-range RGB channel, another truecolor method, malformed decimal text, a palette-index record, or a material-library record does not define a neutral color and does not stop the forward search.

`NEUTRON_Material_attrib_def` is face-owned appearance metadata: the family string is followed by one tagged integer `1` and a tagged 36-character GUID string, which ends the payload. The GUID joins the face to its per-face appearance assignment in the Fusion Design stream ([`f3d.md` §3.2](f3d.md#32-materials)).

`no_combine_attribute-st-attrib` is a marker attribute: the standard attribute record followed by one boolean, with no other payload. It marks a face excluded from coplanar-face merging during Boolean and combine operations; carried on sheet-metal flange faces.

`string_attrib-name_attrib-gen-attrib` stores the four ASM keep/copy/ignore/copy integer flags, a tagged attribute-name string, and a tagged value string. Attribute name `name` assigns the value as the owning body or face display name. The record participates in the ordinary attribute-ref chain between direct-color attributes and persistent-design attributes.

`generic_tag_attrib_def` begins with the family string, three tagged integers `3, 3, -1`, the string `"generic_tag_attrib_def "` including its trailing space, and a tagged integer group count. The group count determines the complete remaining payload. The record terminator follows the final group's final zero.

A body persistent-link group has five fields:

```text
04 i64 = 3
07 string persistent_design_id
04 i64 design_reference
04 i64 = 0
04 i64 = 0
```

The persistent design ID string contains ASCII decimal digits. A body-owned generic-tag attribute can interleave groups whose entity-class discriminator is not `3`; those groups remain part of the retained attribute but do not identify the solved body. Discriminator-`3` groups are ordered from older assignments to the current final body assignment.

A face- or edge-owned group is variable-width:

```text
04 i64 selector
07 string token
04 i64 = 0
04 i64 reference_count
reference_count * (04 i64 design_reference)
04 i64 = 0
```

`reference_count` supplies the only boundary for the signed reference vector. The token retains its UTF-8 spelling, including the `"-1"` form. Face and edge groups are distinct from the fixed-width body persistent-ID history and do not use the body group's five-field interpretation.

`sketch_attrib_def` is source-link metadata owned by a coedge, an edge, or a vertex. Its attribute header is the integers `1`, `1`, and a payload-form selector. Every form writes the five members `(sketch_curve_id, ref_b, sense, enum_a, enum_b)` in that order and nothing else varies between the forms. Form `3` writes them in one tagged UTF-8 field as a six-integer ASCII tuple carrying a `0` between `sense` and `enum_a`; form `2` writes six integers with a trailing `0`; form `0` writes the five integers alone. `sketch_curve_id`, `enum_a`, and `enum_b` are signed. `ref_b` spans the full unsigned 64-bit range, so a reader that takes it signed refuses the links above `i64::MAX`; it is `0` in most links. `sense` takes three values: `0` and `1` select one of the sketch curve's two senses, and the all-ones 32-bit pattern leaves the sense unconstrained, spelled `4294967295` in the tagged field and `-1` in the integer forms. It links its owning B-rep entity to a sketch curve and does not define analytic geometry.

`Timestamp_attrib_def` stores an integer marker `1` followed by one tagged f64. The f64 is the original authoring time in microseconds since the Unix epoch. It is distinct from the ASM header save time and participates in the owning entity's ordinary attribute-ref chain.

---

## 6. Geometry carriers

All model-space lengths are cm→mm ×10; unit vectors/ratios/angles/knots are not scaled (§4).

### 6.1 Surface vocabulary

`plane`, `cone` (covers circular and elliptical cylinders when `sin(half_angle)==0`), `sphere`, `torus`, `spline` (procedural/NURBS, dispatched by nested subtype), `mesh` (not the exact carrier when analytic/spline carriers exist). Curve vocabulary: `straight`, `ellipse` (covers circles: `ratio==1` ⇒ circle), `intcurve`, `pcurve`, plus `null_*` sentinels.

### 6.2 Analytic surface byte layouts

Each layout is fixed-size. Offsets are record-relative from the `0x11` byte.

**`plane`**: origin (`0x13`) + unit normal (`0x14`) + unit UV-reference direction (`0x14`). Evaluation `S(u,v) = origin + u·u_dir + v·v_dir`, `v_dir = normal × u_dir`.

An embedded or face-bound pcurve on a plane stores its first coordinate as a native length along `u_dir` and its second coordinate as a native length opposite `v_dir`. An embedded or face-bound pcurve on a `cone` stores normalized generator distance first and azimuth angle second. Let `sine`, `cosine`, and `u_scale` be the three stored cone chart fields after the ratio. The axial distance is `first × direction × cosine × u_scale`, where `direction` is `-1` when `sine × cosine < 0` and `1` otherwise. Neutral projection converts plane coordinates to document length units and converts cone coordinates to `(azimuth, axial distance)`.

**`cone` (161 B, covers cylinders)**: order: origin (`0x13`), axis (`0x14`), `ref × r_major` (`0x14`, magnitude = base major radius), `ratio = r_minor/r_major` (f64, 1.0 = circular), `0x0b 0x0b`, `sin(half_angle)` (f64, 0 ⇒ cylinder), `cos(half_angle)` (f64), `u_scale` u-parameter scale (f64), 5×`0x0b`. A non-unit ratio defines an elliptical cone whose minor radius is `r_major · ratio`; zero sine with a non-unit ratio is an elliptical cylinder. **Half-angle rule:** `half_angle = asin(|sine|)`. The angle is the acute branch even when both stored sine and cosine are negative. **Sign rules:** the base major radius is the major-axis vector's magnitude; `u_scale` usually equals it but diverges on offset-derived surfaces and is not a radius. The signed major-radius slope `sine / cosine` is the radius change per unit axis distance: `r_major(d) = r_base + d · sine / cosine` at signed distance `d` along the axis from the origin. A negative `cosine` points the surface normal toward the axis; face senses are stored relative to that inward normal.

**`sphere` (134 B)**: center (`0x13`), **signed** radius (f64), dir1 (equator), dir2 (polar axis). **Signed-radius rule:** a negative radius identifies an inward-facing, concave feature; the sign is part of the carrier.

**`torus` (142 B basic / 160 B ranged)**: origin, axis, `major_radius` (f64), **signed** `minor_radius` (f64), `ref_direction`; then a range flag (`0x0b` = full 142-B variant; `0x0a` = 160-B variant with start/end angles). `minor < 0` with `|minor| ≤ |major|` describes an apple/lemon torus. **Inside-out torus rule:** `|minor| > |major|` is self-intersecting. The native frame and minor-radius sign are part of the carrier.

Both direction vectors in each analytic-surface frame are required. A `cone` major-axis vector also has nonzero magnitude; its magnitude is the base radius. A record with an absent required vector or a zero cone major-axis vector does not define an analytic surface. `u_scale` does not substitute for the cone radius or reference direction.

Evaluation formulas for all four carriers follow directly from the frame vectors above.

### 6.3 Analytic curve byte layouts

**`straight` (115 B)**: base point + direction vector. Curve range is unbounded; the owning edge's `t_start`/`t_end` clip it. Endpoints `= base + t·direction` with the stored, unnormalized vector: the direction's magnitude is the line's parameter scale and is not necessarily 1.

**`ellipse` (148 B with angles / 130 B without, covers circles)**: center, axis normal, `ref × r_major` (magnitude = major radius), `ratio = r_minor/r_major`; the 148-B variant adds start/end angles. Circle when `ratio==1`. **Ratio-sign phase convention:** for `ratio > 0` the stored range is axis-aligned and the endpoint phase is +π/2. For `ratio < 0`, the negative sign encodes a flipped parameterization; the stored range is direct and the minor-radius magnitude is `|ratio|`.

**`degenerate_curve`**: collapses to a point (cone apex / sphere pole). An edge may _also_ collapse to a point with no `degenerate_curve` entity: curve ref null and both vertex refs identical. That is valid ACIS, not a malformed edge.

**`helix_int_cur`**: the current form starts with a tagged integer ASM release word; the earlier form omits it. The remaining payload is a finite angle interval, axis-start position, major-radius vector, minor-radius vector, pitch vector, apex-factor double, and unit axis vector, optionally followed by the solved curve cache and its fit tolerance. The current form encodes the axis start as a position token and the next three triples as vector tokens; the earlier form encodes all four as position tokens. Position and radius-vector components and the cache fit tolerance are lengths. The major and minor vectors have equal magnitude. Their orientation about the axis records handedness; the pitch vector records axial rise per revolution, and the apex factor records linear radial growth per revolution fraction. Without the solved cache, this complete construction is the exact curve carrier. A reversed record negates and swaps the angle bounds and negates the minor, pitch, and apex-factor fields, producing the parameterization `C'(t) = C(-t)`.

**`offset_int_cur`**: one subtype flag, source curve, start/end source-parameter doubles, model-space offset vector, then two `(string label, integer role code)` pairs, followed by the solved curve cache and its fit tolerance. The source curve and solved cache are distinct carriers. Offset-vector components and fit tolerance are lengths; parameters and role codes are unscaled.

**`subset_int_cur`**: parent curve followed by a two-bound native parameter interval, then the solved curve cache and fit tolerance. The parent and solved cache are distinct curve carriers. The interval is unscaled.

**`exact_int_cur`**: the solved `nubs`/`nurbs` curve cache is the authoritative exact construction payload, followed by its fit tolerance. In the revision-gated cache-first form, the shared cache-first context follows that cache: its two ordered support surfaces and two nullable ordered BS2 curves remain the pcurve slots named by a ref-form `pcurve` selector. Those slots do not replace or reinterpret the exact 3D curve cache. No weaker analytic carrier is implied by the subtype. A zero fit tolerance denotes an exact cache.

**`comp_int_cur`**: a counted leading parameter array, component count, one parameter double per component, one ASM extension flag, then exactly that many ordered child curves. The final curve cache and fit tolerance follow the child curves. Component parameters and the leading parameter array are unscaled; child and solved NURBS control points and fit tolerance use the standard length scaling.

**Surface-related intcurve prefix**: two ordered support surfaces, two ordered BS2 parameter curves paired by side, one native parameter interval, then three counted discontinuity arrays. `null_surface` and `nullbs` are explicit absence sentinels. The interval and discontinuity values are unscaled.

**`off_int_cur`**: the surface-related prefix, one ASM extension flag, then signed left/right offset lengths. The solved curve cache and fit tolerance follow the offsets. The two offsets correspond to the two ordered support sides.

**Cache-first subtype selection**: a positive serializer-revision integer directly after a procedural subtype name selects that subtype's cache-first (revision-gated) layout; its absence selects the context-first layout. The shared cache-first intcurve context is the revision integer, a leading enum, the solved curve cache and fit tolerance, two ordered support surfaces with the optional bound fields of cache-first `int_int_cur`, two nullable ordered parameter curves, two optional solved-curve interval endpoints, three counted discontinuity arrays, and one integer ASM extension flag. A per-subtype tail follows the extension flag. The leading enum selects the approximation-cache form. Zero selects the solved curve cache and fit tolerance as above. Two replaces the cache and tolerance with a bool-gated curve interval followed by a closed-form enum; no `bs3_curve` and no tolerance are stored. Every member from the two ordered support surfaces onward is unchanged, so a form-2 context is the revision integer, the enum, the interval, the closed-form enum, the two supports, the two nullable parameter curves, the two optional solved-curve interval endpoints, the three discontinuity arrays, and the integer extension flag, followed by the per-subtype tail. The stored interval is the parameter interval of the occupied parameter curve. Form 2 has no cache, so its construction is definitive: a form-2 `par_int_cur` is the occupied support surface restricted to the parameter curve in the same slot, and its second boolean flag has no cache to promote. Other values have no defined grammar and select a native branch in which the containing record is retained verbatim.

**`int_int_cur`** has context-first and cache-first forms. The context-first form is the surface-related prefix followed by one boolean ASM extension flag, then the solved curve cache and fit tolerance. The cache-first form starts with a positive serializer-revision integer and the leading enum, followed by the solved curve cache and fit tolerance. Two ordered support surfaces follow. A referenced `spline` support carries one boolean before its subtype-table reference and four optional U/V bound fields after it; each optional bound is false when absent or true followed by one double when present. The two ordered parameter curves follow and may independently be `nullbs`. Two optional solved-curve interval endpoints follow; absent endpoints inherit the corresponding bound of the solved NURBS domain. Three counted discontinuity arrays and one integer ASM extension flag terminate the cache-first subtype. The construction is the intersection of the two ordered support surfaces; each non-null BS2 curve retains its parameterization on the corresponding support.

**`proj_int_cur`**: the surface-related prefix, one ASM extension flag, the source curve, and a second boolean flag. In the ranged form, a source-parameter interval and projection-role string (`surf1` or `surf2`) follow the flag before the solved cache. In the early-close form the subtype closes immediately after the flag and the solved carrier is external to that subtype payload.

**`sss_int_cur`**: the surface-related prefix, an integer selector, then a third support surface and its paired BS2 parameter curve. The solved cache and fit tolerance follow the third support pair. All three support sides retain their serialized order.

**Surface curves**: `blend_int_cur`, `surf_int_cur`, `par_int_cur`, and `skin_int_cur` have a context-first form containing the surface-related prefix with no subtype-specific tail, followed by the solved cache and fit tolerance. The subtype name distinguishes blend-edge, surface-constrained, parametric, and skin construction semantics. `blend_int_cur` also has a cache-first form: positive serializer-revision integer, the leading enum, solved cache and fit tolerance, two ordered support surfaces with the same optional bound fields as cache-first `int_int_cur`, two nullable ordered parameter curves, two optional solved-curve interval endpoints, three discontinuity arrays, one integer extension, and one terminating boolean flag. `par_int_cur` also has a cache-first form: the shared cache-first context followed by two boolean flags. The first flag is the support-slot selector: `true` places the parametric support surface and its bs2 pcurve in serialized slot 1 with slot 2 null; `false` mirrors them onto slot 2 with slot 1 null. The second flag promotes the solved cache to the definitive carrier: when set, point and derivative queries read the cached curve instead of the support surface and its parameter curve. It is set only in records whose cache fit tolerance is zero, and a zero tolerance does not require it. Clearing it is valid in every record.

**Silhouette curves**: `silh_int_cur` and `para_silh_int_cur` append a cast surface and light vector to the surface-related prefix. `taper_silh_int_cur` adds one unscaled draft-factor double after the light vector. The solved cache and fit tolerance follow the silhouette tail.

**`off_surf_int_cur`**: the surface-related prefix, one ASM extension flag, base-surface U and V intervals, an embedded base curve and its interval, then distance, shift, and scale doubles. Distance is a signed length; all intervals, shift, and scale are unscaled. The solved cache and fit tolerance follow the tail. `off_surf_int_cur` also has a cache-first form: the shared cache-first context followed by base-surface U and V intervals, an embedded base curve with two optional parameter endpoints, the base-curve interval, then the distance, shift, and scale doubles. Every cache-first interval endpoint uses the optional bool-gated encoding.

**`spring_int_cur`**: two ordered support surfaces followed by two ordered BS2 curves, the native curve interval, three discontinuity arrays, one ASM extension flag, and a `CURV_DIR` enum. A `null_surface` is followed immediately by its U and V intervals. A `nullbs` in the first BS2 position is followed immediately by its parameter interval; a `nullbs` in the second position has no conditional interval. The solved cache and fit tolerance follow. `spring_int_cur` also has a cache-first form: the shared cache-first context followed by one `CURV_DIR` enum.

**`defm_int_cur`**: the shared cache-first intcurve context is followed by a source curve, two optional source-interval bounds, and an integer discriminator. The source is either an embedded base curve or an `intcurve` slot containing a boolean and a `ref` subtype. Discriminator 8 is followed by four ordered vectors, a pair count, and two doubles per pair. Discriminator 3 is followed by four vectors, one double, three booleans, one position, two vectors, one double, two booleans, three doubles, five booleans, one double, and one integer. The subtype closes immediately after the selected discriminator payload.

An embedded freeform support surface is encoded as the `spline` surface discriminator followed by its `nubs`/`nurbs` surface block. Its paired BS2 curve is a direct `nubs`/`nurbs` curve block. Surface control points use length scaling; UV poles, knots, weights, intervals, and discontinuities are unscaled.

Embedded analytic supports use the standard `plane`, `cone`, `sphere`, or `torus` discriminator followed by the same position, orientation, radius, angle, and flag payload used by the corresponding top-level carrier. A zero cone sine denotes a cylinder. Signed sphere and torus radii retain their signs.

**`exact_spl_sur` / `exactsur`**: the exact NURBS surface and its fit tolerance, followed by ordered U and V intervals and one ASM extension integer. The NURBS cache is the constructed surface. The revision-gated form stores the revision integer, the shared revision-gated surface tail, two parameter intervals in U-then-V order — each an ordered pair of optional bounds carrying the surface's unextended parameter range in that direction; the first interval is the U range and the second the V range — and the extension as an enum. A pair with no recorded range is the descending pair `(1.0, 0.0)` with both bounds present in the bool-gated encoding. Native generation uses `exact_spl_sur`.

**`rule_sur` / `rulesur`**: two ordered profile curves followed by the solved NURBS surface and fit tolerance. The surface evaluates as the linear interpolation of the two profiles over its second parameter. Native generation uses `rule_sur`.

**`sum_spl_sur` / `sumsur`**: two ordered curves and a model-space origin followed by the solved NURBS surface and fit tolerance. The surface evaluates as the sum of the two curve positions minus the stored origin. The revision-gated form stores the revision integer, the two curves each followed by two optional parameter endpoints, the origin, and the shared revision-gated surface tail. Native generation uses `sum_spl_sur`.

**`rot_spl_sur` / `rotsur`**: one profile curve, a model-space axis origin, and an axis direction followed by the solved NURBS surface and fit tolerance. The profile knot domain is the construction's profile interval; the solved surface V domain is its angular interval. The native layout is not transposed. The revision-gated form stores the revision integer, the profile curve with two optional parameter endpoints, the axis origin and direction, and the shared revision-gated surface tail. Native generation uses `rot_spl_sur`.

**Revision-gated spline-surface forms**: a positive serializer-revision integer directly after a spline-surface subtype name selects that subtype's revision-gated layout. In these layouts, interval endpoints use the optional bool-gated encoding, support surfaces carry the optional bound fields of cache-first `int_int_cur`, and every embedded curve is followed by two optional parameter endpoints. The shared revision-gated surface tail opens with an enum selecting the approximation-cache form, and closes with six counted discontinuity arrays and one boolean; the first three arrays carry U-domain values and the last three V-domain values. Zero selects the solved NURBS surface and its fit tolerance. Two stores no cache and no fit tolerance: it stores the U parameter interval and the V parameter interval in the optional bool-gated encoding, followed by four enums holding U closure, V closure, U singularity, and V singularity. Both forms continue into the discontinuity arrays and the trailing boolean unchanged. Other values have no defined grammar and select a native branch in which the containing record is retained verbatim. Cache-first spline-surface layouts store the tail directly after the revision integer, and their subtype fields follow the tail. A revision-era `spline` surface record body is one subtype scope — an inline definition or a subtype-table `ref` — followed by four optional U/V parameter bounds. An inline `spline` support scope inside a revision-gated layout uses the same revision-gated subtype grammars as a record body.

Revision-gated `cyl_spl_sur`, `loft_spl_sur`, `sweep_sur`, `off_spl_sur`, `rot_spl_sur`, and `sum_spl_sur` end at the shared surface tail. The subtype close follows the tail immediately; another token before the close does not belong to these layouts.

Every other admitted shared-tail subtype consumes its complete subtype-owned suffix and then the subtype close. The suffix is part of that subtype's grammar; another token after the suffix does not extend the admitted layout.

**`off_spl_sur` / `offsur`**: one support surface, signed offset distance, and U/V sense enums followed by the solved NURBS surface and fit tolerance. The modern name additionally carries a conditional one-to-three-boolean ASM tail: a false first flag ends the tail; a true first flag requires a second flag and permits a third. The legacy name has no ASM boolean tail. Native generation retains the form selected by the stored tail. The revision-gated form stores the revision integer, the support surface with its optional bound fields, the offset distance, a leading two-boolean pair occupying the U/V sense-enum slots, a two-boolean ASM extension prefix, and the shared revision-gated surface tail. The pair carries record-level progenitor orientation state, not a per-axis decomposition. The offset surface displaces each support point by the stored distance along the support normal, where the support normal is the cross product of the support's stored U and V partial derivatives, negated when the first boolean is set. The first boolean equals the sense flag of the support-surface reference, so the displacement follows the support's declared normal and the sign of the stored distance carries the offset side in every state of the pair. The second boolean leaves the offset surface's point set unchanged. The pair takes the states false/false, true/false, false/true, and true/true; a writer sets the first boolean equal to the support reference's sense flag, and a reader accepts a record in which the two disagree. A support surface whose stored parameterization is reflected relative to the model carries both booleans set, and the true/true state occurs with both signs of the stored distance. The second boolean of the extension prefix gates an extension run between the prefix and the shared tail: a true second boolean requires the run, and a false second boolean has no run slot. The first boolean gates nothing in the record's layout and changes no other field of the record, so the prefix takes all four states and the run's presence follows the second boolean alone. The run stores one boolean, six LONGs, one boolean, an embedded cache-first intcurve with its two optional parameter endpoints, two booleans, a tolerance, and four LONG `-1`. The first of the six LONGs is `0`. The run tolerance is not a length: it does not convert with the stream's length unit.

**`comp_spl_sur`**: the solved NURBS surface and fit tolerance occur first, followed by a float array and one component surface per array element. Each float is paired positionally with its component surface. The leading surface block is the face cache; trailing NURBS component surfaces do not replace it during cache selection.

**Rolling-ball aliases**: `rb_blend_spl_sur` and `rbblnsur` select the two-support rolling-ball layout. `sss_blend_spl_sur` and `sssblndsur` select the same prefix followed by a third-side graph. `pipe_spl_sur` and `pipesur` denote the surface-surface specialization. Native generation uses the modern spelling.

**Taper spline surfaces**: `taper_spl_sur`, `ortho_spl_sur`/`orthosur`, `edge_tpr_spl_sur`, `shadow_tpr_spl_sur`/`shadowtapersur`, `ruled_tpr_spl_sur`/`ruledtapersur`, and `swept_tpr_spl_sur`/`swepttapersur` share a support surface, reference curve, nullable BS2 pcurve, taper parameter, solved NURBS surface, and fit tolerance. Standard taper has no tail; orthogonal adds a sense boolean; edge adds a draft vector; shadow and swept each add a draft vector plus stored sine/cosine values; ruled adds the same fields plus a factor. Shadow and swept are distinguished by subtype name, not tail shape. Native generation uses the modern subtype corresponding to the retained variant. The revision-gated orthogonal form stores the revision integer, the support surface with its optional bound fields, the reference curve with its optional endpoints, the BS2 pcurve, the taper parameter, the shared revision-gated surface tail, and the orthogonal-sense boolean, positionally matching the sense boolean of the pre-revision orthogonal tail.

**`loft_spl_sur` / `loftsur`**: two ordered loft sections precede two parameter intervals, two closure enums, two singularity enums, and a mode integer. Each section contains parameterized entries; each entry contains a counted profile and one path. Every profile member carries a type integer, curve, support surface, nullable BS2 pcurve, first flag, ASM integer, constraint subdata, and an optional direction selected by a second flag. Each path carries a curve, counted auxiliary BS3 curves, and a tail integer. Constraint subdata stores its type, row/column counts, leading scalar pairs, and per-column scalar pairs; type 211 stores exactly one leading pair and no column pairs. A variable sequence of boolean, integer, double, text, or enum tokens bridges the mode to the solved NURBS surface and fit tolerance. The revision-gated form inserts the revision integer after the subtype name, retains the section and bridge grammar, stores two wrap-range intervals in V-then-U order — each an ordered pair of optional bounds, where a reversed pair is the empty no-wrap interval and a non-empty interval corresponds to a direction in which the solved surface is closed; the first interval carries the V direction and the second the U direction — and ends with the shared revision-gated surface tail. Its constraint subdata rows carry one additional trailing scalar pair: a nonzero-type row stores the leading pair followed by `column_count + 1` scalar pairs. A revision-gated profile member's payload is selected by its type integer: a nonzero type stores the support surface, a nullable BS2 pcurve, and the first flag; a zero type stores two nullable BS2 pcurve slots and no first flag. Both forms are followed by the ASM integer, the constraint subdata, and the optional direction selected by a second flag. The ASM integer is present in save format 23200 streams and absent in save format 22300 through 22600 streams; the gate reads the stream's save format version, not the record's own revision stamp, so one revision stamp takes the integer in a later stream and omits it in an earlier one. Auxiliary path curves carry no optional endpoints.

**`cl_loft_spl_sur`**: the solved NURBS surface and fit tolerance precede four scale slots, an optional fifth scale, two flags, and a tail-kind integer. Present scale slots contain counted members, a path curve, counted auxiliary BS3 curves, and two tail integers. Each member contains a type integer, curve, and the same support, nullable BS2 pcurve, flags, constraint subdata, and optional direction used by a loft profile member. An absent scale consumes no token; the boolean beginning the next field remains at the cursor. Consequently the four leading scales form a contiguous prefix, the fifth scale requires all four leading scales, the kind-6 scale is required, and the second kind-7 scale is required. Kind 6 stores two flags, its scale, an integer, direction vector, interval, and BS3 curve. Kind 7 stores a flag, optional first scale, second flag, required second scale, integer, direction vector, and two trailing flags. Kind 0 stores two flags, a selector, selector-zero direction vector or selector-nonzero BS3 direction curve, and two trailing flags. The revision-gated form is cache-first: the revision integer and the shared revision-gated surface tail precede one unparameterized scale block, a counted sequence of parameterized scale blocks, two flags, and the tail-kind integer. A revision scale block is a counted sequence of loft profile members, a nullable path curve with two optional parameter endpoints, counted auxiliary BS3 curves, and one tail integer; a parameterized block appends its parameter after those fields. Revision members use the revision loft profile-member encoding. The kind-zero payload stores two flags, a selector, a selector-zero direction vector or selector-nonzero BS3 direction curve, two optional parameter values, and an optional trailing BS3 curve. The two parameter values take the bool-gated encoding. The trailing curve is present exactly when both parameter values are present: two present values require the curve, and two absent values end the payload with no curve after them. The stored numbers do not select the slot, and the ascending and the descending parameter order both occur with the curve present. Native generation uses `cl_loft_spl_sur`.

**`scaled_cloft_spl_sur`**: a singularity enum and singularity-selected shape payload precede six discontinuity arrays, one discontinuity flag, three scale slots, two flags, and an integer. The full shape payload is the solved NURBS surface and fit tolerance. The none shape payload replaces that cache with two intervals and two scalar arrays; its complete procedural graph is the exact face carrier. The three leading scales form a contiguous prefix under the same zero-token absence rule as `cl_loft_spl_sur`. A false branch flag selects a flag, integer, and selector-zero direction vector or selector-nonzero BS3 curve. A true branch flag selects an optional scale and a second flag. A true second flag requires another scale, integer, and direction vector; a false second flag stores another boolean, singularity enum, and BS3 curve. Every branch rejoins at two flags, an integer, two vectors, a singularity enum, and a BS3 curve. Native generation uses `scaled_cloft_spl_sur`.

**`skin_spl_sur`**: three surface enums, an integer, a scalar, and an inner count precede a structurally selected skin layout. The compact layout begins directly with a curve, loft subdata, integer, second curve, and final integer. The expanded layout contains `inner_count` entries, each comprising a type integer, curve, and loft profile data, followed by a path curve and two integers. Both layouts rejoin at a direction vector, scalar, recursive law formula, parameter curve, solved NURBS surface and fit tolerance, six discontinuity arrays, and a boolean. Native generation retains the selected layout.

**`law_spl_sur`**: legacy layouts begin with the U and V parameter intervals as four doubles; modern layouts begin directly with the primary recursive law formula. A counted sequence of additional law formulas follows. The legacy interval-prefixed layout has an implicit `full` tail: its solved NURBS surface follows the formula sequence directly. Modern layouts serialize a standard-tail enum selecting one of five layouts. Selector `0` (`full`) contains a solved NURBS surface and its fit tolerance. Selector `1` (`summary`) contains counted U and V parameter arrays, a fit tolerance, two closure enums, and two singularity enums. Selector `2` (`none`) contains U and V parameter intervals, two closure enums, and two singularity enums. Selectors `3` (`historical`) and `4` (`optimal`) have no mode-specific fields. Six discontinuity float arrays follow every mode. Only `full` carries a solved cache; the other modes use the recursive law construction as the exact surface carrier. The legacy interval prefix is structurally distinguished from a formula by its leading double tag. Native generation retains the legacy implicit-full or modern explicit-tail layout.

**`sub_spl_sur`**: U and V parameter intervals precede one embedded support surface. The construction is the exact restriction of the support surface to the stored rectangular parameter domain; it has no solved-cache or fit-tolerance field.

**`net_spl_sur`**: two ordered loft-section graphs precede twelve frame scalars, one integer, four direction vectors, and four recursive law formulas. The solved NURBS surface and fit tolerance, six discontinuity arrays, and one boolean complete the payload. Native generation retains every section member, support, pcurve, constraint table, auxiliary path, frame value, and formula.

**`sweep_spl_sur` profile-first layout**: a primary enum precedes the profile curve and spine curve. A secondary enum, five direction vectors, one model-space point, four scalars, and three recursive law formulas follow. The solved NURBS surface and fit tolerance, six discontinuity arrays, and one boolean complete the payload. Native generation retains both curves and the complete construction graph.

**`sweep_spl_sur` explicit formula layout**: a primary enum and integer precede a profile curve, its two-scalar parameter interval, and an optional point-vector profile frame. A frame point and three vectors follow. Branch integer `1` then stores a boolean, path curve, model-length interval, scalar, boolean, recursive formula, and trailing boolean. The common solved-surface cache and discontinuity tail complete the payload. Native generation retains the complete construction graph. The revision-gated form stores the `sweep_sur` spelling and the revision integer, replaces the primary enum with a boolean, and ends with the shared revision-gated surface tail. A recursive formula whose text names `EDGE` references is followed by a binding count and that many bindings, each a label string, an embedded curve, and two parameter doubles. Its revision-gated law-driven form replaces the branch integer with a string law, its mode and two presence-gated range bounds, a direction vector, path mode and flag, path curve, optional path endpoints, two presence-gated path bounds, and a path scalar. A second string law and mode precede a string rail formula, its binding count, and its recursive bindings. The shared tail's form `0` stores the solved cache and fit tolerance; form `2` stores the U/V parameterization and no cache or fit tolerance. Native generation retains the selected tail and complete construction graph.

**`sweep_spl_sur` explicit guide layout**: the explicit prefix matches the formula layout. Branch integer `2` stores a boolean, path curve, model-length interval, and scalar, followed by two booleans, an auxiliary guide curve, its two-scalar parameter interval, two integers, six scalars, and three booleans. The common solved-surface cache and discontinuity tail complete the payload. Native generation retains all three curves and the complete construction graph.

**`sweep_spl_sur` explicit support-surface layout**: the explicit prefix matches the other explicit layouts. Branch integer `3` stores a boolean, path curve, model-length interval, scalar, singularity enum, and support surface. A boolean gates an auxiliary curve. A support boolean and an optional legacy boolean precede the common solved-surface cache and discontinuity tail. Native generation retains the support surface, optional curve, and complete construction graph.

**`sweep_spl_sur` law-driven layout**: the explicit profile and frame prefix is followed directly by a recursive law instead of a branch integer. An integer, two-scalar interval, vector, integer, boolean, path curve, two-scalar interval, scalar, and boolean precede a second recursive law. A final integer, recursive formula, and boolean precede the common solved-surface cache and discontinuity tail. The text-law form stores each of the two law slots as one string token instead of a recursive law tree; the token text is preserved exactly and its internal expression is not re-tokenized. For a straight path with an identity rail and unit scale law, the section evaluates as `S(u,v) = p(u) + spine(v) + law1(v) * (t_hat(u) × d_hat(v))`, where `p` is the profile, `spine` is the path, and `t_hat` and `d_hat` are their unit tangents. Native generation retains both law carriers, the formula, both curves, and the complete construction graph.

**`t_spl_sur`**: the solved NURBS surface, fit tolerance, and discontinuity tail precede model-length U and V intervals and a type integer. The revision-gated form replaces that prefix with the revision integer, the shared revision-gated surface tail, two unextended parameter intervals in the exact-surface encoding, and the type code as an enum; the nested subtype scope and trailing integer are unchanged. A nested subtype scope contains either an inline `t_spl_subtrans_object` program with an optional boolean separator and companion values program, or a subtype-table `ref`. A trailing integer follows the nested scope. Both inline strings are line-oriented. Header tokens and topology, geometry, material, grouping, symmetry, annotation, knot, and grip record tokens select ordered field vectors; comments and unrecognized lines do not contribute typed records. A referenced subtransform resolves through the per-stream subtype table with cycle rejection. Native generation retains both programs byte-for-text, requires both parsed graphs to agree with their programs, inlines resolved shared programs into self-contained output, and uses the solved NURBS surface as the face carrier.

**Law formulas**: a text name begins each formula. `null_law` has no following payload. Every other formula carries a variable count followed by that many recursively framed law expressions. Integer, double, model-space point, and vector tags are terminal constants. A sweep law slot may use a single text terminal instead; that text is the exact serializer expression and is not interpreted as a recursive operator token. `SPLINE_LAW` stores an integer, a knot float array, a control float array, and a model-space point. `TRANS` has two forms selected by the byte following the operator token: a vector form, opening with a `0x14` vector tag, stores four vectors, a scalar, and three booleans; the scalar form stores thirteen scalars and three enums. `EDGE` stores a curve, two optional parameter bounds, and two parameters. Algebraic operator tokens are followed directly by their recursively framed operands. Trigonometric, hyperbolic, inverse-trigonometric, inverse-hyperbolic, `ABS`, `EXP`, `LN`, `LOG`, `SIGN`, `SIZE`, `SET`, `SQRT`, `NORM`, and `NOT` operators are unary. `CROSS`, `DOT`, `DCUR`, `ROTATE`, and `TERM` are binary. `VEC` and `DSURF` are ternary. `O` is an infix binary composition: `(A)O(B)` evaluates to `A(B(x))`, so the right operand runs first and its result is the left operand's input. Both operands are parenthesized, so the operator is always written `)O(`. `MTRAIL` is unary over stored curve law data. Its curve carries the rail direction at the path parameter rather than the swept path: the value is a unit vector to within the curve's fit tolerance and lies in the plane normal to the path tangent, so a consumer normalizes each evaluation. The curve is a non-rational cubic sharing the path's parameterization, and the law stores no initial vector, tolerance, or mode alongside it. `DOMAIN` has odd arity of at least one: the first operand is the wrapped law and each later operand pair is one term's lower and upper domain bound; the wrapper evaluates to its first operand and supplies the term domain. Native generation requires the exact fixed arity and rejects operators without a defined recursive boundary.

**`law_int_cur` / `lawintcur`**: the solved NURBS curve and fit tolerance precede the shared two-surface/two-pcurve support prefix, parameter interval, and three discontinuity arrays. Each support-surface slot and each pcurve slot is nullable: a `null_surface` or `nullbs` sentinel replaces an absent carrier. The stamped serializer form opens the record with `0x04`-tagged version stamp and `0x15`-tagged enum after the subtype name; in that form the parameter interval is stored as two optional bounds, each a bare `0x0b` unbounded sentinel or a `0x0a`-tagged double, and an absent bound inherits the solved-curve domain. The legacy form omits the stamp prefix and stores the interval as two plain range values. The layout then stores an extension integer, one primary recursive formula, a formula count, and that many additional recursive formulas. Native generation uses `law_int_cur` and retains the version stamp, every support carrier, and recursively referenced EDGE curve.

**`helix_spl_circ` / `helix_spl_line`**: the current form starts with a tagged integer ASM release word; the earlier form omits it. An angular interval and secondary interval precede an inline helix path. The circular form length-scales the secondary interval and stores a length before the path and a circle radius after it. The linear form leaves the secondary interval unscaled and stores a model-space profile direction after the path. The inline path stores an angular interval, axis origin, length-bearing major, minor, and pitch vectors, apex factor, unit axis, two null surfaces, and two null pcurves. The current form encodes the axis origin as a position token and the major, minor, pitch, and linear-profile triples as vector tokens; the earlier form encodes those triples as position tokens. Native generation reconstructs the exact cacheless procedural surface.

**`defm_spl_sur` / `defmsur`, mode 8**: a support surface and discriminator `8` precede four deformation vectors and one selector integer. The solved NURBS surface, fit tolerance, and discontinuity tail complete the payload. Native generation retains the support and minimal deformation scaffold.

**`defm_spl_sur` / `defmsur`, modes 1 and 3**: both modes store four vectors, a scalar, three booleans, three vectors, a scalar, two booleans, a model-space point, and five booleans after the support surface and discriminator. Mode 1 appends a count and that many scalar triples. Mode 3 appends an integer and one guide scalar. The solved NURBS surface, fit tolerance, and discontinuity tail complete both payloads.

**`defm_spl_sur` / `defmsur`, mode 5**: a secondary surface, native long, boolean, scalar, integer, scalar, and deformation intcurve follow the initial support and discriminator. Four vectors, a scalar, three booleans, and a counted table of scalar triples precede the solved NURBS surface, fit tolerance, and discontinuity tail. Native generation retains both surfaces and the deformation curve.

**`defm_spl_sur` / `defmsur`, mode 6**: four vectors, a scalar, three booleans, an integer selector, a secondary surface, a native long, a boolean, and a scalar follow the initial support and discriminator. ASM versions above 225 then store one version-gated long. A second scalar, a deformation intcurve, two frames of four vectors plus a scalar and three booleans, and a trailing long precede the solved NURBS surface, fit tolerance, and discontinuity tail. Native generation retains both surfaces, the deformation curve, both vector frames, and the version-gated field.

**`g2_blend_spl_sur` / `g2blnsur`**: two ordered side graphs surround the first-side singularity payload. Each side stores a label, support surface, curve, two nullable BS2 pcurves, and a direction. The first side then stores a singularity enum. The full branch carries an optional BS3 support surface and paired tolerance. The none branch carries nine frame scalars, a tolerance, an optional intervening typed token, and a tertiary nullable BS2 pcurve. The second side is followed by an exact spline support, center curve, two center scalars, center integer, U/V intervals, four trailing scalars, the solved NURBS surface and fit tolerance, and three discontinuity arrays. Branch shape is structural; the singularity enum value is retained without assigning undocumented numeric meanings. The revision-gated form stores the revision integer, two scalars, two support sides in the variable-blend side layout, the center curve with two optional parameter endpoints, two radii, a radius-selector enum (`-1` selects the absent-radius branch), four optional U/V interval endpoints, an integer/scalar/length/integer prologue, the shared revision-gated surface tail, and three trailing integers.

**`var_blend_spl_sur` / `srf_srf_v_bl_spl_sur`**: a serializer-revision integer and two ordered side graphs in the rolling-ball side layout precede the slice curve. Each side begins with one closed support-kind discriminator: `blend_support_cos_curve` / `blendsupcos`, `blend_support_curve` / `blendsupcur`, `blend_support_point_curve` / `blendsuppnt`, `blend_support_surface` / `blendsupsur`, or `blend_support_zero_curve` / `blendsupzro`. It then stores the support surface with four optional parameter bounds (or `null_surface`), the side curve with two optional parameter bounds (or `null_curve`), a nullable primary BS2 pcurve, the model-space location, a nullable secondary BS2 pcurve, one zero integer, and a nullable tertiary BS2 pcurve. The slice curve carries two optional parameter bounds and is followed by two signed offsets and a radius-kind enum (`0` single radius, `1` two radii). Radius controls use recursive blend-value payloads: `two_ends`, `fixed_width`, `edge_offset`, `functional`, `const`, or `interp`. Blend values store the type name, an optional sub-discriminator, the calibrated enum, one Boolean, and the type-specific payload. `two_ends` stores its law-domain parameter range and two radii; `fixed_width` stores its law-domain parameter range and the chamfer width; `edge_offset` without the leading sub-discriminator stores its law-domain parameter range and one offset, so its second field is a parameter and only its third field is a length — the contact offset of the law's own side, in model-space length units; in a two-radii sequence the first radius law is the first side graph's, whose contact lies at the section-parameter start, and the second is the second side graph's, at the section-parameter end; `const` recursively contains another blend value; `functional` stores a `(u,radius)` BS2 pcurve and numeric or symbolic terminal; `interp` stores its law-domain parameter range, a `(u,radius)` BS2 function, an extension enum, a count, and that many radius points — each a parameter, radius, first and second derivative scalars with `9.9999999999999995e+36` as the unset sentinel, a plane point, and a plane normal. The extension enum precedes the count, is present in every payload, and gates nothing. The payload ends at the last radius point and has no trailing scalar pair; the token after that point is the cross-section enum of the enclosing record. The `interp` extension enum is an integer token or an enum token, and one payload uses one encoding throughout. One cross-section enum follows the complete radius-law sequence independently of radius cardinality, and the enum's payload width is the same over a single-radius sequence and over a two-radii sequence. An absent enum is the elided circular default; zero is explicit circular, one is thumbweights followed by two shape scalars, two, four, five, and six are distinct unclassified cross-section values followed by no payload of their own, three is rounded chamfer followed by a radius-presence Boolean and an optional recursive radius law, and seven is G2 round followed by two shape scalars. The two shape scalars of selector one and of selector seven are ordered by side: the first shapes the cross-section at the first side graph's support and the second shapes it at the second side graph's support. Equal scalars give a cross-section symmetric about its midpoint. Neither scalar moves the two contact boundaries, which the radius law alone fixes; the scalars shape the section between them. Selectors zero, one, and seven select three different cross-section laws, so one radius law and one pair of supports give three different surfaces under the three selectors; selector zero's cross-section is the circular arc, and the cross-sections of selectors one and seven are neither circular arcs nor each other. A selector-zero or selector-one cross-section is a rational degree-three span on four control points, and the two share knot structure; a selector-seven cross-section is a non-rational degree-four span on six control points; a selector-three cross-section with zero corner radii is a degree-one span on two control points, a straight chord. The cross-section family does not change with radius cardinality: a two-radii law sets the section's two half-widths and keeps the selector's family. The support-side parameter interval, a second interval storing a lower bound whose upper-bound marker can be the unbounded sentinel, an approximation-current integer, the requested and achieved cache fit tolerances — usually with achieved at or below requested, though a record may store an achieved value above its requested value, and `-1.0` in both slots is the unset pair — a signed handedness marker (`-1` left-handed, `1` not left-handed, `0` reads as not left-handed; writers emit only `-1` or `1`), a cache-selector enum, the solved NURBS cache and fit tolerance, six counted discontinuity arrays, one Boolean, three integers, a nullable secondary curve with optional parameter bounds when present, convex/concave selection, rolling-ball envelope/snapshot selection, two optional post-interval bounds, a nullable post curve, and a nullable BS2 pcurve complete the graph. The approximation-current integer takes `0` and `1`, and a stored value above `1` reads as `1`. `1` marks the stored cache as the current approximation of the surface definition, and the cache is used as it stands. `0` marks the cache as not current: the approximation is rebuilt from the definition, and neither the stored cache nor the stored fit tolerances describe the surface. The handedness marker keeps its stored value across the rebuild; the cache-selector enum and the two fit tolerances do not. A record stored with cache-selector `0` and a cache rebuilds into cache-selector `0` with a new achieved tolerance and new control points, or into cache-selector `2`, which stores no cache and carries the unset `-1.0` tolerance pair. Native generation uses `srf_srf_v_bl_spl_sur` and modern support-kind names.

**`VBL_SURF` / `vertexblendsur`**: a counted sequence of boundary records followed by a grid-size integer and model-space fit tolerance. Every boundary begins with a type name, cross logical, magic direction, U/V smoothing logicals, and fullness scalar. The magic item is a unit direction or the zero vector, not a length-bearing location, so it takes no unit conversion. `circle` adds a curve, form enum, form-selected twist locations (zero for circle, one for ellipse, two for unknown), two parameters, and sense logical. `deg` adds a location and two normals. `pcurve` adds a support surface, nullable BS2 pcurve, sense logical, and parameter-space fit tolerance. `plane` adds a normal, two parameters, and curve. The complete boundary graph is the exact face carrier. Unknown boundary names and unsupported circle forms are invalid. The revision-gated form stores the revision integer before the boundary count; boundary type names are ident tokens, the magic direction is a vector token, support surfaces carry the optional bound fields, embedded curves carry two optional parameter endpoints, and circle form `3` selects two twist vectors. Native generation uses `VBL_SURF`.

**`mesh_surface`**: the record has no payload tokens. It is a sentinel stating that no exact surface carrier is stored in the B-rep record. Display triangles belong to tessellation attributes on the owning face or body and do not become exact face geometry. A face referencing this record therefore retains an unknown exact surface and a typed native sentinel; it does not infer a surface from the display mesh.

### 6.4 Pcurves (2D UV trimming curves)

A `pcurve` record has two byte-level forms, discriminated by the `0x04` int at record-relative **+37**:

- **discriminator == 0 → wrapped form**: a `0x0a`/`0x0b` `wrapper_reversed` boolean, then one balanced subtype payload. An `exp_par_cur` payload owns an inline 2D `nubs` or rational `nurbs` block; a `ref N` payload delegates to subtype-table entry `N`. 2D poles are stored as `(u,v)` pairs (8+8 B each, **not** 24); `nurbs` stores one homogeneous weight after each pole.
- **discriminator != 0 (1, 2, −1, −2) → ref form (72 B)**: a `0x0c` ref to the intcurve carrying the UV curve, then two parameter doubles. No wrapper boolean (its absence is structural).

UV poles are dimensionless surface parameters. `wrapper_reversed` is the inline curve's fit-convention bit, independent of coedge sense and of the parameter-interval sign.

The inline control polygon is followed by a `DOUBLE` parameter-space fit tolerance. After the nested support-surface scope, four ordered trailing booleans precede two final `DOUBLE` values storing the pcurve parameter interval `(t_start, t_end)`. The four booleans are retained and regenerated independently. The balanced `exp_par_cur` scope contains exactly one BS2 carrier; that structurally owned block is the pcurve even when the scope is reached through `ref N` or contains references to other BS2 blocks. A wrapped `ref N` stores the interval immediately after its balanced reference and has no boolean tail or inline fit-tolerance carrier. Nonzero-discriminator ref-form pcurves store the same interval immediately after their intcurve reference and have no wrapper, boolean tail, or inline fit-tolerance carrier. Its selector magnitude names the ordered intcurve slot (`1` for `pcur1`, `2` for `pcur2`); a negative selector reverses the selected pcurve and composes with the intcurve's own reversed sense. The decoder follows that slot and does not enumerate every reachable BS2 block. A lifted BS3 cache, a support carrier, or a BS2-compatible interpretation of a 3D block is not an additional pcurve candidate. The selected slot must contain a BS2 carrier and its paired support slot must be non-null and resolve as an analytic, NURBS, or typed procedural surface. A cacheless procedural support is present even when it has no standalone `SurfaceGeometry` cache. A null support slot, missing pcurve slot, invalid selector, or malformed typed reference leaves the pcurve undecoded.

For a revision-gated `exact_int_cur`, the selected slot is read from the shared cache-first context after the exact 3D cache. The exact cache is not considered a BS2 carrier.

An `intcurve` wrapper whose sole subtype payload is `{ref N}` or the compact `0x0F LONG N 0x10` form delegates pcurve-slot resolution to subtype-table entry `N`. A nested reference inside a named construction does not delegate the owning construction's slot selection.

Pcurve UV coordinates use the owning surface's exact parameterization. A procedural surface's solved NURBS block is an evaluated model-space cache and does not redefine that parameterization. Carrier selection is structural; face-surface evaluation and edge-vertex endpoint matching do not replace the serialized subtype or slot role. If that role is unavailable, the decoder leaves the pcurve undecoded rather than selecting the first parseable block.

One pcurve carrier may span a longer parameter domain than an edge using it. The decoder evaluates the edge's native stored `t_start` and `t_end` under both parameter signs rather than requiring the pcurve's full knot endpoints to equal the edge vertices. Only intervals whose endpoints lie in the pcurve knot domain are eligible; the full knot domain is the fallback when neither sign lies in that domain. The selected signed interval belongs to the coedge's pcurve use. It remains in the pcurve's native parameterization when the neutral 3D edge parameter is length-normalized. Edge sense selects the first eligible sign tested; pcurve-wrapper and coedge orientations are independent.

Coedge sense is the edge-use orientation for a pcurve inherited from its surface: `effective_pcurve = flip_pcurve(surface_pcurve, coedge.sense)`. The stored 2D B-spline poles and knots retain their native order. `wrapper_reversed` is separate from coedge sense.

An explicit pcurve reference belongs to a free-form B-spline face. Analytic plane, cylinder, cone, sphere, and torus faces store `-1` in the coedge pcurve field; their UV boundary is not serialized as a pcurve record.

### 6.5 `nubs`/`nurbs` blocks (B-spline curves and surfaces)

Surface block grammar: name (`nubs`|`nurbs`), degree_u, degree_v, u/v periodicity + singularity enums, unique-knot counts, (knot, multiplicity) pairs for each direction, then the control grid (3D for `nubs`, 4D homogeneous for `nurbs`). Control grids are **row-major with v in the outer loop, u in the inner loop.**

**Pole-count rule:** the block stores endpoint multiplicities as `degree` (not `degree+1`). With stored multiplicities: `n_poles = sum(stored_mults) − (degree − 1)`. With expanded (clamped) multiplicities: `n_poles = sum(expanded_mults) − (degree + 1)`. Both expressions produce the same pole count.

Native ASM NURBS control grids are the per-face cache. `surface_fit_tolerance == 0.0` indicates fidelity to the procedural surface, rather than identity with a primitive.

### 6.6 `intcurve` and `spline` subtypes

Procedural intcurve subtypes (`exact_int_cur`, `off_int_cur`, `proj_int_cur`, `int_int_cur`, `sss_int_cur`, …) and spline-surface subtypes (`rb_blend_spl_sur`, `sss_blend_spl_sur`, `var_blend_spl_sur`, `loft_spl_sur`, `sweep_spl_sur`, `net_spl_sur`, VBL/taper families, …) each carry per-subtype field tails and version/`asm_major` gates. A named `ref N` scope or compact `0x0F LONG N 0x10` scope nested inside a surface, curve, or pcurve body indexes a per-file subtype table, not a byte offset. Each subtype definition — a `0x0F` opening followed by a `0x0d`/`0x0e` name token other than `ref` — contributes one table entry in stream order. Definitions and references are recognized at token boundaries only: the same byte pattern inside a token payload (an `f64`, a string body) is data, not a table entry.

Legacy intcurve subtype names select the same layouts as their modern names: `bldcur`→`blend_int_cur`, `blndsprngcur`→`spring_int_cur`, `exactcur`→`exact_int_cur`, `lawintcur`→`law_int_cur`, `offintcur`→`off_int_cur`, `offsetintcur`→`offset_int_cur`, `offsurfintcur`→`off_surf_int_cur`, `parasil`→`para_silh_int_cur`, `parcur`→`par_int_cur`, `projcur`→`proj_int_cur`, `surfcur`→`surf_int_cur`, `surfintcur`→`int_int_cur`, `d5c2_cur`→`skin_int_cur`, and `subsetintcur`→`subset_int_cur`. Native generation uses the modern spelling.

Legacy spline-surface subtype names select the same layouts as their modern names. This includes `cylsur`→`cyl_spl_sur`, `lawsur`→`law_spl_sur`, `subsur`→`sub_spl_sur`, `skinsur`→`skin_spl_sur`, `netsur`→`net_spl_sur`, `sweepsur`→`sweep_spl_sur`, `sweep_sur`→`sweep_spl_sur` (the spelling stored by revision-gated records), `sclclftsur`→`scaled_cloft_spl_sur`, `varblendsplsur`→`var_blend_spl_sur`, `srfsrfblndsur`→`srf_srf_v_bl_spl_sur`, `crvcrvblndsur`→`crv_crv_v_bl_spl_sur`, `crvsrfblndsur`→`crv_srf_v_bl_spl_sur`, and `sfcvfreeblndsur`→`sfcv_free_bl_spl_sur`. Native generation uses the modern spelling.

`var_blend_spl_sur`, `srf_srf_v_bl_spl_sur`, `crv_crv_v_bl_spl_sur`, `crv_srf_v_bl_spl_sur`, and `sfcv_free_bl_spl_sur` share one payload grammar. The subtype name selects the native variable-blend behavior class and is retained independently of the common construction fields.

An `intcurve` or `spline` record carries a record-level sense boolean immediately before its subtype scope (`0x0a` reversed, `0x0b` forward). A reversed record's geometry is the reverse of its subtype definition: a reversed intcurve parameterizes as the negation of its cache (`C(t) = cache(−t)`; the owning edge's `t_start`/`t_end` are on the reversed parameterization), and a reversed spline surface's normal is the reverse of the cache normal (the face's sense field composes on the reversed surface).

A `spline` subtype can contain several top-level surface-bearing `nubs` or `nurbs` blocks. The final surface block is the face-surface cache; earlier blocks can be 2D support pcurves. A nested `ref` denotes another carrier through the subtype table. A valid final cache defines the exact typed face surface independently of whether the enclosing subtype's construction fields have a neutral interpretation. When the subtype construction grammar is not typed, its complete native record remains linked from an opaque procedural construction while the face uses the solved NURBS carrier.

The compact `rb_blend_spl_sur`, `rbblnsur`, `pipe_spl_sur`, and `pipesur` form omits the native side graph. It stores zero, one, or two consecutive support entries, then the spine curve, two signed radius values, enum `-1`, the solved surface cache, and an optional cache-fit tolerance. Each support entry is string `blend_support_surface`, an outer surface-kind identifier, and either a complete embedded analytic surface or a NURBS support cache. A labelled NURBS block without the outer kind occupies an unresolved support slot. Support entries bind to support slots zero then one. Equal radius values select a constant circular section; unequal values select a linearly varying circular section. The compact form ends after the optional tolerance.

A rolling-ball record outside the complete and compact grammars does not assign supports, spine, radius values, or cross-section by token encounter order. Its complete native record remains linked from an opaque procedural construction, and a valid final surface cache remains the face carrier.

An intcurve subtype opens with the record's own 3D B-spline cache: the first `nubs`/`nurbs` curve block after the subtype scope opens, followed by a `DOUBLE` fit tolerance, safe-range booleans, and the counted discontinuity arrays. Construction machinery — support surfaces, blend spines, progenitor curves — is serialized after the cache in nested subtype scopes, and its curve blocks are not the record's carrier. The owning edge's `t_start`/`t_end` live on the cache parameterization.

The `cyl_spl_sur` and `rb_blend_spl_sur` field sequences are:

```
cyl_spl_sur :=
  0x0f 0x0d "cyl_spl_sur"
  DOUBLE u_start
  DOUBLE u_end
  VECTOR_3D extrusion_direction
  POSITION
  curve-cache
  [ surface-cache
    [ DOUBLE cache_fit_tolerance ] ]
  0x10
```

The compact layout above stores the directrix interval before its carriers. The versioned nested layout stores `LONG schema_version`, then the directrix as either the `intcurve` carrier name, its record-level sense Boolean, and one balanced embedded intcurve subtype, or the sense Boolean and one compact subtype-table reference to that intcurve. Two `OPTIONAL_RANGE_ENDPOINT` directrix parameters, `VECTOR_3D extrusion_direction`, `POSITION`, and the shared revision-gated surface tail follow. The tail closes the subtype scope. Its form `0` carries this record's surface cache and fit tolerance; its form `2` carries neither, and the U parameter interval it stores is the directrix parameter interval except where the tail's U closure enum marks that direction closed. A surface block inside the embedded intcurve subtype belongs to that subtype and is never this record's cache. `u_start` and `u_end` are directrix parameters in either layout. `extrusion_direction` is length-bearing. `POSITION` is stored in model-space length units and is retained independently of the directrix. It is the extrusion axis reference point: the representative of the axis line orthogonal to `extrusion_direction`, satisfying `POSITION · extrusion_direction = 0`. It equals the direction-orthogonal projection of the directrix control-point average at construction time and is retained unchanged when the directrix is re-approximated; the field is redundant for surface evaluation. The optional final `surface-cache` is the solved NURBS surface, and `cache_fit_tolerance` is a length. Without that cache, the directrix, parameter interval, direction, and position still define and retain the exact translational-extrusion construction. Native generation writes the stored interval and position without deriving or replacing either field.

A translational extrusion is an analytic cylinder when its directrix is a closed nonperiodic rational NURBS comprising four ordered quarter-circle Bézier spans and `extrusion_direction` is parallel to the circle normal. For degree `p >= 2`, the carrier has `4p + 1` poles, endpoint knot multiplicity `p + 1`, interior knot multiplicity `p`, and four positive parameter spans. Repeated homogeneous Bézier degree reduction of every span produces a rational quadratic with one common nonzero endpoint weight `w` and middle weight `w sqrt(1/2)`; multiplying every homogeneous weight by the same nonzero scalar does not change the carrier. In Euclidean coordinates, each reduced middle pole is the sum of its two endpoint poles minus the common center. Consecutive endpoint radial vectors are perpendicular, have the same positive length, and have consistently oriented cross products. The cylinder origin is the common center, its axis follows the normalized extrusion direction, its reference direction follows the first radial vector, and its radius is the shared radial length. The analytic carrier takes precedence over an optional solved NURBS cache. A nonclosed, noncircular, degenerate, or obliquely extruded directrix remains a procedural extrusion or retains its solved NURBS carrier.

```
rb_blend_spl_sur :=
  0x0f 0x0d "rb_blend_spl_sur"
  LONG serializer_revision
  rolling-ball-side
  rolling-ball-side
  curve slice
  OPTIONAL_RANGE_ENDPOINT slice_range[2]
  LENGTH offset_left
  LENGTH offset_right
  (ENUM_VALUE -1 | DOUBLE radius_selector)
  OPTIONAL_RANGE_ENDPOINT u_range[2]
  OPTIONAL_RANGE_ENDPOINT v_range[2]
  LONG shape_prefix
  DOUBLE parameter[2]
  LONG tail
  revision-gated-surface-tail
  [rolling-ball-third-side]
  LONG tail_extension[3]
  0x10

revision-gated-surface-tail :=
  ENUM_VALUE tail_form
  ( 0 surface-cache
      DOUBLE cache_fit_tolerance
  | 2 OPTIONAL_RANGE_ENDPOINT u_interval[2]
      OPTIONAL_RANGE_ENDPOINT v_interval[2]
      ENUM_VALUE u_closure
      ENUM_VALUE v_closure
      ENUM_VALUE u_singularity
      ENUM_VALUE v_singularity )
  FLOAT_ARRAY discontinuity[6]
  BOOLEAN discontinuity_flag

rolling-ball-side :=
  TEXT support_kind
  ( null_surface
  | surface
    OPTIONAL_RANGE_ENDPOINT surface_u_range[2]
    OPTIONAL_RANGE_ENDPOINT surface_v_range[2] )
  nullable-curve
  OPTIONAL_RANGE_ENDPOINT curve_range[2]
  nullable-bs2-pcurve
  POSITION location
  nullable-bs2-pcurve
  [ INTEGER extension
    nullable-bs2-pcurve ]

rolling-ball-third-side :=
  TEXT label
  surface
  curve
  nullable-bs2-pcurve
  VECTOR_3D direction
  nullable-bs2-pcurve
  INTEGER extension
  nullable-bs2-pcurve
  BOOLEAN flag
```

`serializer_revision` is the serializer-revision integer in the release ×100 stamp family, the same field that opens `var_blend_spl_sur`. `support_kind` uses the closed blend-support discriminator set defined for variable blends. `null_surface`, `null_curve`, and `nullbs` encode absent support geometry. Every present embedded side or slice curve is followed by two optional parameter-range endpoints. Modern sides append the extension integer and tertiary pcurve; legacy sides end after the secondary pcurve. An optional range endpoint is `BOOLEAN false` when absent and `BOOLEAN true DOUBLE value` when finite. The two offsets and fit tolerance are lengths. `ENUM_VALUE -1` selects the absent-radius branch; a `DOUBLE` carries an explicit selector value. `tail_form` is the shared tail's form enum, not a selector private to this subtype: form `0` stores the solved face surface and its fit tolerance, and form `2` stores no cache and no fit tolerance, so a form-`2` record's construction graph is its own carrier. `sss_blend_spl_sur` appends the third-side graph after the shared tail. The three `tail_extension` integers close the subtype scope in both subtypes.

A circular rolling-ball construction with equal nonzero signed offsets has a constant radius equal to the offset magnitude. Two nonparallel plane supports and a nonperiodic collinear NURBS slice define an analytic cylinder when the slice direction is parallel to the planes' intersection and every slice pole lies on a line whose perpendicular distance from each plane equals the constant radius. The cylinder axis is that line, the radius is the offset magnitude, and its reference direction is the canonical direction derived from the axis.

One plane support, one circular-cylinder support, and a four-quarter rational-circle slice satisfying the homogeneous degree-reduction invariant above define an analytic torus when the plane normal, cylinder axis, and slice normal are parallel; the slice center lies on the cylinder axis; the center-to-plane distance equals the constant radius; and the absolute difference between the slice radius and cylinder radius equals the constant radius. The torus center, axis, reference direction, and major radius are the slice circle's frame and radius. Its signed minor radius is the common signed offset. The analytic carrier takes precedence over the solved NURBS cache. A variable-radius construction, noncircular cross-section, nontangent support, noncollinear slice, or noncircular slice retains the solved NURBS carrier.

---

## 7. Text encoding (SAT/SMT)

A `.sat` or `.smt` stream carries the same entity model as a binary stream in a line-oriented ASCII encoding. The two encodings are alternatives, not layers: a document stores its geometry in one of them.

### 7.1 Header lines

Three header lines precede the records.

The first line holds exactly four binary header words as ASCII integers in the binary order: the save-format version, the record-count word (`0` when unwritten), the entity-count word, and the flags word. The words keep their binary semantics (§1): the entity-count word is the RecordTable index of the first referenced record, flag bit 0 marks a history partition, and flag bits 1 to 7 hold the revision. Trailing spaces after the flags word are padding.

The second line holds exactly three product strings — product family, product version, and save date — as counted strings. A counted string in a header line is a decimal byte count, one whitespace separator byte, and that many bytes; header lines do not use the record encoding's `@` prefix. Only whitespace can follow the third string.

The third line holds exactly three kernel doubles in the binary order: `scale`, `resabs`, and `resnor`. The `resabs` and `resnor` tolerances are finite nonnegative error bounds. A negative value, infinity, or NaN makes the stream malformed.

**Unit rule.** In the text encoding, `scale` is a finite positive number and is the stream's length unit in millimetres per unit. Zero, a negative number, infinity, or NaN makes the stream malformed. A model-space length equals its stored value multiplied by `scale` millimetres. This differs from the binary encoding, whose lengths are centimetres and whose `scale` word is not a coordinate multiplier (§4). Dimensionless values — unit vectors, ratios, angles, knots, parameters, and pcurve coordinates — do not take the unit.

### 7.2 Record grammar

Each record is a record name, its fields, and the terminator field `#`. Whitespace — spaces, tabs, and newlines — separates fields, and a record continues across lines until its terminator. The record name is the `-`-joined chain the binary name tokens assemble (§2.2).

Field forms:

- `$N` is a reference to the record at index `N`; `$-1` is null.
- `@N` followed by one separator byte and exactly `N` raw bytes is a string. The bytes count is exact: the payload can contain spaces and newlines.
- `{` and `}` delimit a balanced, nested subtype scope. A record terminator is valid only after every opened scope closes. The bare words directly after `{` are the scope's identifier chain, including subtype names, `ref`, `nubs`, `nurbs`, and the `null_surface`, `null_curve`, and `nullbs` sentinels. The `{ref N}` form and the serializer version stamps are unchanged from the binary encoding.
- A number field is one serialized value. A field at a `POSITION`, `VECTOR_3D`, or `VECTOR_2D` slot spans three or two consecutive number fields. An integral spelling does not select an integer type: a `DOUBLE` slot whose value is integral is written without a decimal point, so the slot's type comes from the record layout (§5, §6), not from the field's spelling.
- A boolean is a word. A sense slot writes `forward` for `FALSE` and `reversed` for `TRUE`. A face sides slot writes `single` for `FALSE` and `double` for `TRUE`. A surface v-sense slot writes `forward_v` for `FALSE` and `reverse_v` for `TRUE`. A plain logical slot writes `F` for `FALSE` and `T` for `TRUE`. An optional range bound (§6.3) writes `I` for the absent bound (`FALSE`, no value follows) and `F` for the present bound (`TRUE`, one value follows). The word `F` therefore takes its meaning from the slot class.
- An enumeration (`ENUM_VALUE`) is a word from the slot's vocabulary. Closure slots write `open` (0), `closed` (1), and `periodic` (2). Singularity slots write `none` (0). Approximation-cache form slots write the `law_spl_sur` selector names: `full` (0), `summary` (1), `none` (2), `historical` (3), and `optimal` (4). Curve extension slots write `UNEXTENDED` (0).

The `$` and `@` prefixes are reserved for references and counted strings. A field that starts with either prefix is malformed when its decimal operand is absent, invalid, or outside the supported integer range.

### 7.3 Record indexing and stream end

Record indices count records in file order from zero, starting at the first record after the header lines. A stream that begins with an `asmheader` record gives it index 0; a save-format 700 stream stores no `asmheader` record and gives index 0 to its first entity record. `$N` references index this table directly.

The stream ends with a terminator line that identifies the serialization branch: `End-of-ASM-data` on the ASM branch and `End-of-ACIS-data` on the ACIS branch. Only whitespace can follow this terminator. Save-format 700 streams use the ACIS terminator and the legacy subtype spellings (§6.6); later save formats use the ASM terminator and the modern spellings.

### 7.4 Save-format 700 record layouts

A save-format 700 stream stores three topology records with fewer fields than the layouts of §5.2. The `vertex` record stores no endpoint-index integer: the owning edge is followed directly by the point reference. The `tvertex` record stores the vertex fields, then one model-space tolerance and no trailing integer. The `coedge` and `tcoedge` records store no reserved integer between the owner loop and the pcurve reference; the tolerant parameters follow the pcurve reference directly. The other §5.2 records keep their field sequences.
