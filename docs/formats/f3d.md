# Autodesk Fusion 360 `.f3d`: Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

## 1. Container layer

`.f3d` is a **ZIP archive**. Entries may be stored, DEFLATE-compressed, or compressed with **zstd (ZIP method 93)**.

### 1.1 Payload families

| Path pattern                                                                     | Role                                                    |
| -------------------------------------------------------------------------------- | ------------------------------------------------------- |
| `<folder>/Breps.BlobParts/*.smb`, `*.smbh`                                       | ASM/ACIS exact B-rep streams                            |
| `<folder>/ProteinAssets.BlobParts/*.protein`                                     | nested ZIP archives with appearance/material assets     |
| `<folder>/Design1/BulkStream.dat` (or `FusionDesignSegmentType1/BulkStream.dat`) | design recipes, material assignments, body map          |
| `<folder>/*/MetaStream.dat`                                                      | per-segment object tables (GUID → entity-type registry) |
| `<folder>/FusionACTSegmentType1/BulkStream.dat`                                  | Active Component Tree entity/appearance tables          |
| `<folder>/FusionBrowserSegmentType1/BulkStream.dat`                              | Fusion UI browser tree                                  |
| `<folder>/Previews/*`, `<folder>/Images.BlobParts/*`                             | thumbnails / appearance images; never geometry          |
| `ParaMeshGeometry.BlobParts/*.paramesh`                                          | secondary mesh; not the exact source                    |
| `Manifest.dat` (top-level and per-asset)                                         | document, asset, and segment registry (see §1.3)        |

`<folder>` is an asset-folder path component.

### 1.2 Stored property and configuration entries

The following small entries are STORED:

| Entry                                           | Bytes                   | Meaning                                      |
| ----------------------------------------------- | ----------------------- | -------------------------------------------- |
| `Properties.dat`                                | `00 00 00 00` (u32 `0`) | empty document-properties slot               |
| `.../DesignConfigurationTable.<uuid>.dsgcfg`    | JSON object             | configuration table, including parameter and suppression overrides |
| `.../DesignConfigurationRule.<uuid>.dsgcfgrule` | JSON object             | configuration activation rules                 |

Configuration tables and rules are complete JSON objects. A table's `configurations` member is an object keyed by variant name. Each variant value is an object; its `parameters` member is an object, `suppressed` is an array of strings, and `material` is a string. The table's `active` string equals one key in `configurations`. Rule objects carry activation conditions and targets. Unknown object members remain part of the configuration document. ZIP entry name and extension select table versus rule; duplicate entry names are invalid.

Each table variant has a stable neutral identity formed from the complete table-entry name and the variant name as two byte-length-prefixed UTF-8 strings. Table order and variants in other tables do not affect that identity. The table's `active` member selects the active neutral variant. Parameter overrides, suppressed-feature names, and material names transfer as variant properties; the native table remains the full-fidelity source for unrecognized members.

### 1.3 `Manifest.dat` grammar

Both manifests are flat sequences of `u32`-length-prefixed strings. An ASCII field stores a byte count followed by that many bytes. A UTF-16LE field stores a code-unit count followed by twice that many bytes.

The **top-level manifest** carries a document version tag (`3-2-0-0`), the `FusionDocType` marker, the `.f3d` extension, a display name and description, document and asset UUIDs, capability tokens, and an asset-folder UUID.

The **per-asset manifest** carries two asset GUIDs, `FusionAssetType`, the asset type `Neutron3DAssetType`, a `physicalChangeGuid`, and the segment-type registry (`FusionDesignSegmentType`, `FusionACTSegmentType`, `FusionBrowserSegmentType`).

---

## 2. B-rep streams and history partition

### 2.1 Stream forms

Both `.smb` and `.smbh` entries carry an ASM `BinaryFile` token stream. A history-bearing stream contains solved-model records followed by a `history_stream` record and linked `delta_state` records. The `history_stream` record, rather than the first `delta_state` name token, is the byte boundary between the solved-model record sequence and construction history.

### 2.2 History preamble

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

### 2.3 `delta_state` records

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

The BulletinBoard chain closes with its tagged-zero terminator. A second `04 0 11` sequence separates the state body from the following record sequence. On a non-tail state, that `0x11` is the next `delta_state` record delimiter and the state owns no intervening entity records. The tail state is followed by `End-of-ASM-History-Section`, the retained history entity snapshot, and `End-of-ASM-data`. These records use the ordinary SAB name-chain and payload grammar. Their ordered `0x0c` entity-reference tokens, including `-1` null references, use the construction-history revision namespace and are retained independently of the snapshot's local record ordinal. The old references across the BulletinBoard changes form a unique contiguous interval immediately after the active RecordTable. Snapshot local ordinal zero has the interval's first revision identity, and each following snapshot record increments that identity by one. `End-of-ASM-data` is excluded from the interval.

The linked `delta_state` chain runs from the current model toward the initial model. Historical entity membership is reconstructed from the identity map of the active RecordTable. Reversing an update `old -> new` keeps stable entity slot `new` and changes its record revision to `old`. Reversing an insertion `null -> new` removes slot `new`. Reversing a deletion `old -> null` adds slot `old` with record revision `old`. A complete chain accepts every transition without a missing slot or record revision and terminates at the singleton map `0 -> 0` for `asmheader`. Each state retains the complete sorted entity-slot-to-record-revision map before its own changes are reversed. Incomplete chains retain no partial state maps.

Every old revision belongs to stable entity slot `new` for an update and slot `old` for a deletion. Materializing a state selects its recorded revision for each live entity slot, frames that active or archived record, replaces every non-null entity reference with the referenced revision's stable entity slot, and sorts the records by stable slot. A materialized table is complete only when every selected record frames, every revision has one stable slot, and every normalized reference names a live entity in the state. Completeness is assigned atomically across the linked state chain. The resulting sparse RecordTable uses the ordinary active-model topology and carrier grammars. Each complete state has stable entity-slot membership for its bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves, and pcurves. Body-to-region, region-to-shell, shell-to-face, face-to-loop, and loop-to-coedge relations are ordered. Shell wire edges and free vertices are ordered independently. Each coedge retains its owning loop, edge, next, previous, radial-next, and optional pcurve slots. Each edge retains its ordered start and end vertex slots and optional curve slot. Each face retains its surface slot, and each vertex retains its point slot. A state's `next` state is its preceding modeling state. The forward transition from `next` to the current state partitions normalized record slots and every topology and geometry family into inserted, deleted, and updated sets; an updated slot exists in both states and selects different record revisions. The final `End-of-ASM-data` record ends at the enclosing stream boundary without a trailing `0x11`; EOF terminates only that final history record.

---

## 3. ASM binary header

Streams begin with the 15-byte magic `ASM BinaryFile8` or `ASM BinaryFile4`; byte 15 is the low byte of the release word, not part of the magic. The digit selects the width of integer/ref tags (§4): `4` → tag + 4-byte LE signed; `8` → tag + low 32 bits + high 32 bits (consume the full 9-byte field). Fusion writes both widths; ASM-227/228/229-era streams are `BinaryFile4` and ASM-230+ streams are `BinaryFile8`.

`BinaryFile8` header layout (little-endian, mirroring `BinaryFile4` with wider words):

| Bytes    | Meaning                                                                                |
| -------- | -------------------------------------------------------------------------------------- |
| `0..15`  | magic `ASM BinaryFile8`                                                                |
| `15..19` | little-endian u32 ASM release word (`23000` on ASM 230, `23100` on ASM 231 streams)    |
| `19..31` | zero                                                                                    |
| `31..39` | little-endian u64 entity-count word                                                     |
| `39..47` | little-endian u64 flags; bit 0 is set iff the stream carries a history partition (§4a) |

The string region begins at byte 47.

`BinaryFile4` header layout (the classic ACIS save header, little-endian):

| Bytes    | Meaning                                                                                |
| -------- | -------------------------------------------------------------------------------------- |
| `0..15`  | magic `ASM BinaryFile4`                                                                |
| `15..19` | little-endian u32 ASM release word (`22700` on ASM 227, `22900` on ASM 229 streams)    |
| `19..23` | little-endian u32 record count (`0` when unwritten)                                    |
| `23..27` | little-endian u32 entity count                                                         |
| `27..31` | little-endian u32 flags; bit 0 is set iff the stream carries a history partition (§4a) |

The string region begins at byte 31.

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
- The entity-count and flags words carry stream metadata, not model-space quantities. The flags word's bit 0 marks a history partition on `.smbh` streams in both widths.
- `scale`, `resabs`, and `resnor` are kernel metadata. `scale` is not a coordinate transform. Fusion `BinaryFile8` streams use `scale = 60.0`, `resabs = 1e-6`, and `resnor = 1e-10`; ASM-227 `BinaryFile4` streams use `scale = 50.0` with the same tolerances; an ASM-229 `BinaryFile4` stream uses `scale = 90.0`.

---

## 4. Tag encoding and record framing

The stream is a tag-typed SAB (ACIS binary) token stream.

### 4.1 Tag table

| Tag                         | Symbol               | Payload     | Meaning                                         |
| --------------------------- | -------------------- | ----------- | ----------------------------------------------- |
| `0x02`                      | CHAR                 | 1 B         | unsigned 8-bit                                  |
| `0x03`                      | SHORT                | 2 B         | signed 16-bit                                   |
| `0x04`                      | LONG                 | ref_size    | signed int (32 or 64-bit per header)            |
| `0x05`                      | FLOAT                | 4 B         | IEEE float32                                    |
| `0x06`                      | DOUBLE               | 8 B         | IEEE float64                                    |
| `0x07`/`0x08`/`0x09`/`0x12` | UTF-8 string         | 1/2/4/4 + N | length-prefixed string (8/16/32/32-bit length)  |
| `0x0A`                      | TRUE                 | 0 B         | logical true (data token, **not** a terminator) |
| `0x0B`                      | FALSE                | 0 B         | logical false / sentinel                        |
| `0x0C`                      | ENTITY_REF           | ref_size    | RecordTable index                               |
| `0x0D`                      | IDENT                | 1 + N       | record/class name token (leaf)                  |
| `0x0E`                      | SUBIDENT             | 1 + N       | base-class name token                           |
| `0x0F` / `0x10`             | SUBTYPE_OPEN / CLOSE | 0 B         | brace-balanced subtype delimiters               |
| `0x11`                      | TERMINATOR           | 0 B         | end of current record                           |
| `0x13`                      | POSITION             | 24 B        | 3D point (3×f64)                                |
| `0x14`                      | VECTOR_3D            | 24 B        | 3D vector (3×f64)                               |
| `0x15`                      | ENUM_VALUE           | ref_size    | enumeration / secondary integer                 |
| `0x16`                      | VECTOR_2D            | 16 B        | 2D `(u,v)`                                      |
| `0x17`                      | INT64                | 8 B         | AutoCAD int64 attribute value                   |

- `0x11` terminates the current top-level record; the next record's name-token chain begins at the following byte.
- `0x0A`/`0x0B` inside a record are booleans (often `reversed`/`forward`), **never** record boundaries.
- Positions (`0x13`) and length-bearing vectors are centimetres; see §5.

### 4.2 Record names and the RecordTable

A record name is the `-`-joined chain of all `0x0E` tokens terminated by one `0x0D` leaf token (e.g. `persubent-acadSolidHistory-attrib`). In assembled record names, the class token `ASM` is represented as `ACIS`.

**RecordTable indexing:** the stream begins with an `asmheader` record (not preceded by `0x11`) at **index 0**. `RecordTable[1]` is the first record after it, and so on. Positive `0x0C` refs index this table directly; `-1` is null.

The `asmheader` row participates in RecordTable indexing; the first following entity therefore has index 1.

### 4.3 Version/product gates

Non-ASM (pure ACIS) and SpaceClaim SAB streams use version-gated padding absent from Fusion ASM streams: attribute records skip 18 bytes when `ver > 15.0 && !ASM`; topology records skip bytes when `ver > 10.0 && !ASM` and `ver > 6.0`; SpaceClaim uses a `%`-delimited string interning scheme. The byte layouts in §§6–7 apply to Fusion ASM streams.

---

## 5. Unit rules

- Fusion model-space lengths are stored in centimetres in both widths.
- Model-space points, radii, length-bearing vectors, 3D control points, and length tolerances convert to millimetres by ×10.
- Unit vectors, ratios, angles, knot parameters, non-length enums, homogeneous weights, and UV pcurve coordinates are dimensionless.
- The header `scale` field is metadata, not a coordinate multiplier (§3).

An analytic surface is untrimmed; its extent is independent of the face's vertex hull.

---

## 6. Topology records

### 6.1 Ownership graph

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

### 6.2 Fusion-ASM byte layouts (`BinaryFile8`, fixed sizes)

All records of a given class are fixed-size on Fusion files. Offsets are record-relative from the leading `0x11`; ref/int chunks are 9 bytes. On `BinaryFile4` streams ref/int chunks are 5 bytes and the offsets scale accordingly.

**Body (61 B):** `chunk[1]` (@+16, i64) is `history / body flags`, the **`asm_body_key`** joined to the design-side body map (§8). A value of `-1` denotes a sub-body without its own Design record. The key field is retained for every body independently of whether a Design join resolves, and native writing preserves or patches it directly. `chunk[3]` @+34 = first_lump, `chunk[4]` @+43 = first_wire or `-1`, `chunk[5]` @+52 = transform or `-1`.

**Lump (61 B):** `chunk[4]` @+43 = first_shell, `chunk[5]` @+52 = owner_body. (The @+27 slot is reserved `-1`, not the first shell.)

**Shell (80 B):** `chunk[5]` @+53 = first_face, `chunk[6]` = wire, `chunk[7]` = owner.

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
+34 chunk[3] next_face
+43 chunk[4] first_loop
+52 chunk[5] owner_shell
+70 chunk[7] surface REF        ← the ONLY authoritative face→surface binding
+79 chunk[8] sense  (0x0a=reversed, 0x0b=forward)
+80 chunk[9] sides  (0x0b=single)
+81 chunk[10] containment       ← PRESENT ONLY IF chunk[9]=double
```

`sides` and `containment` are separate enum chunks. Single-sided faces end after `sides`; double-sided faces carry `containment`.
The sense token is relative to the native surface carrier. Decoding a reversed spline carrier or an inward-normal cone carrier reverses the sense in the normalized B-rep while retaining the native token; writing applies the same reversal back to the token.

**Loop (61 B):** `chunk[3]` @+34 = next_loop (`-1` terminates the chain), `chunk[4]` @+43 = first_coedge, `chunk[5]` @+52 = owner_face. Loop order is defined by the `next_loop` references, not stream position; the first loop is not an outer-loop marker.

**CoEdge (100 B):**

```
+35 chunk[3] next_coedge   +44 chunk[4] prev_coedge   +53 chunk[5] partner_coedge
+62 chunk[6] edge          +71 chunk[7] sense byte
+72 chunk[8] owner_loop    +81 chunk[9] reserved int (const 0)
+90 chunk[10] pcurve ref (or -1)
```

The `{+35,+44,+53}` triad is next/prev/partner. `+72` is the owner loop. **Partner symmetry** is a manifold invariant: every coedge's partner's partner is itself, and every shell edge is shared by exactly two mutually-referencing coedges of opposite sense.

`tcoedge` inherits this complete base field sequence. `chunk[11]` and `chunk[12]` are its native start and end parameters. Releases below 215 have no fixed extension fields. Releases from 215 through 219 store a nullable reference in `chunk[13]`. In modern selector-one records, an embedded curve is followed by either two false Booleans or two true Boolean-and-double pairs. The paired doubles are the embedded curve's parameter interval and override `chunk[11]` and `chunk[12]` for the neutral coedge use curve; the false pair leaves that use curve on the outer tolerant interval. A cache-local selector-one extension has a null leading reference and owns its embedded NURBS use curve. Native generation writes that curve inside one balanced subtype. The subtype has no serialized token count; its matching close delimiter bounds the curve payload.

Modern releases store a nullable reference in `chunk[13]` and a LONG selector in `chunk[14]`. Selector zero is followed by LONG zero and terminates the record. Selector one is followed by a boolean and one balanced subtype scope containing a 3D NURBS coedge-use curve. `chunk[11..=12]` bound this curve in loop-traversal order: a forward coedge runs from the edge start vertex to the edge end vertex, while a reversed coedge runs from the edge end vertex to the edge start vertex. The fields after the matching outer `SUBTYPE_CLOSE` are either `FALSE, FALSE, LONG 0`, denoting no trailing interval, or `TRUE, f64 start, TRUE, f64 end, LONG 0`, denoting an explicit trailing interval. Nested subtype scopes do not terminate the outer payload. These extension fields do not change the offsets or meanings of the base topology links.

**Edge (98 B):**

```
+34 chunk[3] start_vertex   +43 chunk[4] t_start (f64)
+52 chunk[5] end_vertex     +61 chunk[6] t_end (f64)
+70 chunk[7] owner_coedge   +79 chunk[8] curve ref
+89 chunk[9] sense byte     +90 0x07 'tangent'|'unknown' continuity text
```

`+52` is end_vertex and `+79` is curve, not the other way round. `owner_coedge` is a nullable back-reference selecting one use of the edge; it is retained independently of the radial-ring topology, validated against the selected coedge's edge, and written in both retained and source-less output. `t_start`/`t_end` are stored parameters on the edge's own parameterization: the referenced curve itself when the sense byte is forward (`0x0b`), its reverse `E(t) = C(−t)` when reversed (`0x0a`). A full-circle edge has identical start/end vertex with `t_start = -π`, `t_end = +π`; the shared vertex lies at the `t_start` angle from the major axis, so a full period's phase is significant, not a free normalization. The continuity text is descriptive metadata, **not** a curve-type discriminator.

A closed cylindrical band may use two loops, each containing one self-linked coedge on a full-circle edge. The two circular edges retain their distinct repeated vertices and full-period parameter phases. No seam edge or seam coedge occurs in this native topology.

`tedge` carries this complete base field sequence followed by `chunk[11]` as an f64 model-space tolerance, `chunk[12]` as an opaque LONG producer-version value, and `chunk[13]` as LONG zero. The tolerance converts from native centimetres to document millimetres. The two LONG fields are retained verbatim. The extension does not change the base endpoint, curve, sense, or continuity fields.

**Vertex (63 B):** `chunk[3]` @+36 = owning_edge, `chunk[4]` @+45 = index_flag (`0` = this is the owning edge's START vertex, `1` = its END vertex), `chunk[5]` @+54 = point ref. Each vertex has its own point entity; no deduplication.

**Tolerant vertex:** `tvertex` carries the complete vertex field sequence followed by `chunk[6]` as an f64 model-space tolerance and `chunk[7..=8]` as two f32 tail slots. The tolerance converts from native centimetres to document millimetres; the f32 slots are retained verbatim.

**Transform (142 B):** 13×f64 (@+18..117): `a[0..8]` 3×3 rotation, `a[9..11]` translation, `a[12]` overall scale; then ROTATION, REFLECTION, and SHEAR boolean-enum bytes in that order (`0x0a` selects the named property, `0x0b` selects `no_*`). Column mapping: `a[0..2]`→col0, `a[3..5]`→col1, `a[6..8]`→col2, `a[9..11]`→col3. The body references its transform through `body.chunk[5]`; null denotes no body transform. Native writing retains the three classifications independently of the matrix and emits all three fields.

### 6.3 Point records and coordinate authority

A BinaryFile8 `point` record is 60 bytes: the 8-byte record head, three 9-byte entity-base fields, and one 25-byte model-space `POSITION`. The record terminates immediately after the position and carries no trailing reference-count integer. `vertex.chunk[5]` references the point record. NURBS control grids independently carry their model-space poles.

### 6.4 Sense semantics

Three sense bits compose into the winding:

- **face.sense**: forward = surface's natural normal, reversed = flipped.
- **coedge.sense**: loop-traversal direction relative to the edge curve parameterization.
- **edge.sense**: the edge's own curve-parameterization sense. A reversed edge parameterizes as the negation of its curve (`E(t) = C(−t)`); its `t_start`/`t_end` and vertex order are on that reversed parameterization.

**Winding rule:** `effective_curve_reversed = edge.sense_reversed XOR coedge.sense_reversed`. Each edge has two coedges with opposite `effective_curve_reversed`.

### 6.5 Ownership reachability

Topology membership is defined by references from `body → lump → shell → face → loop → coedge → edge → vertex`. Surface, curve, and point membership follows the authoritative binding references in §6.1.

An edge with `owner_coedge_ref == -1` and no reference from a reachable coedge is outside that ownership graph.

### 6.6 Attributes on the topology graph

Every entity carries an `attrib` ref-chain. `Entity.attrib` is the chain head, each record carries `next` and `previous` references, and `-1` terminates the chain. Color and feature-tag attributes can coexist on one chain. `ATTRIB_CUSTOM-attrib` records carry an owner ref at record-relative `+60..68` and a family name (`generic_tag_attrib_def`, `sketch_attrib_def`, `Timestamp_attrib_def`, `FPM_tracked_attrib_def`). Attribute records are variable-width.

`string_attrib-name_attrib-gen-attrib` stores the four ASM keep/copy/ignore/copy integer flags, a tagged attribute-name string, and a tagged value string. Attribute name `name` assigns the value as the owning body or face display name. The record participates in the ordinary attribute-ref chain between direct-color attributes and persistent-design attributes.

`generic_tag_attrib_def` begins with the family string, three tagged integers `3, 3, -1`, the string `"generic_tag_attrib_def "` including its trailing space, and a tagged integer group count. The group count determines the complete remaining payload. The record terminator follows the final group's final zero.

A body-owned group has five fields:

```text
04 i64 = 3
07 string persistent_design_id
04 i64 design_reference
04 i64 = 0
04 i64 = 0
```

The persistent design ID string contains ASCII decimal digits. Groups are ordered from older assignments to the current final assignment.

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

`sketch_attrib_def` is coedge-owned source-link metadata. After its three-integer attribute header, a tagged UTF-8 field stores the six-integer ASCII tuple `(sketch_curve_id, 0, signed_ref, 0, enum_a, enum_b)`, where `signed_ref` uses `-1` as null. It links a B-rep coedge to a sketch curve and does not define analytic geometry.

`Timestamp_attrib_def` stores an integer marker `1` followed by one tagged f64. The f64 is the original authoring time in microseconds since the Unix epoch. It is distinct from the ASM header save time and participates in the owning entity's ordinary attribute-ref chain.

---

## 7. Geometry carriers

All model-space lengths are cm→mm ×10; unit vectors/ratios/angles/knots are not scaled (§5).

### 7.1 Surface vocabulary

`plane`, `cone` (covers circular and elliptical cylinders when `sin(half_angle)==0`), `sphere`, `torus`, `spline` (procedural/NURBS, dispatched by nested subtype), `mesh` (not the exact carrier when analytic/spline carriers exist). Curve vocabulary: `straight`, `ellipse` (covers circles: `ratio==1` ⇒ circle), `intcurve`, `pcurve`, plus `null_*` sentinels.

### 7.2 Analytic surface byte layouts

Each layout is fixed-size. Offsets are record-relative from the `0x11` byte.

**`plane`**: origin (`0x13`) + unit normal (`0x14`) + unit UV-reference direction (`0x14`). Evaluation `S(u,v) = origin + u·u_dir + v·v_dir`, `v_dir = normal × u_dir`.

**`cone` (161 B, covers cylinders)**: order: origin (`0x13`), axis (`0x14`), `ref × r_major` (`0x14`, magnitude = base major radius), `ratio = r_minor/r_major` (f64, 1.0 = circular), `0x0b 0x0b`, `sin(half_angle)` (f64, 0 ⇒ cylinder), `cos(half_angle)` (f64), `u_scale` u-parameter scale (f64), 5×`0x0b`. A non-unit ratio defines an elliptical cone whose minor radius is `r_major · ratio`; zero sine with a non-unit ratio is an elliptical cylinder. **Half-angle rule:** `half_angle = asin(|sine|)`. The angle is the acute branch even when both stored sine and cosine are negative. **Sign rules:** the base major radius is the major-axis vector's magnitude; `u_scale` usually equals it but diverges on offset-derived surfaces and is not a radius. The signed major-radius slope `sine / cosine` is the radius change per unit axis distance: `r_major(d) = r_base + d · sine / cosine` at signed distance `d` along the axis from the origin. A negative `cosine` points the surface normal toward the axis; face senses are stored relative to that inward normal.

**`sphere` (134 B)**: center (`0x13`), **signed** radius (f64), dir1 (equator), dir2 (polar axis). **Signed-radius rule:** a negative radius identifies an inward-facing, concave feature; the sign is part of the carrier.

**`torus` (142 B basic / 160 B ranged)**: origin, axis, `major_radius` (f64), **signed** `minor_radius` (f64), `ref_direction`; then a range flag (`0x0b` = full 142-B variant; `0x0a` = 160-B variant with start/end angles). `minor < 0` with `|minor| ≤ |major|` describes an apple/lemon torus. **Inside-out torus rule:** `|minor| > |major|` is self-intersecting. The native frame and minor-radius sign are part of the carrier.

Evaluation formulas for all four carriers follow directly from the frame vectors above.

### 7.3 Analytic curve byte layouts

**`straight` (115 B)**: base point + direction vector. Curve range is unbounded; the owning edge's `t_start`/`t_end` clip it. Endpoints `= base + t·direction` with the stored, unnormalized vector: the direction's magnitude is the line's parameter scale and is not necessarily 1.

**`ellipse` (148 B with angles / 130 B without, covers circles)**: center, axis normal, `ref × r_major` (magnitude = major radius), `ratio = r_minor/r_major`; the 148-B variant adds start/end angles. Circle when `ratio==1`. **Ratio-sign phase convention:** for `ratio > 0` the stored range is axis-aligned and the endpoint phase is +π/2. For `ratio < 0`, the negative sign encodes a flipped parameterization; the stored range is direct and the minor-radius magnitude is `|ratio|`.

**`degenerate_curve`**: collapses to a point (cone apex / sphere pole). An edge may _also_ collapse to a point with no `degenerate_curve` entity: curve ref null and both vertex refs identical. That is valid ACIS, not a malformed edge.

**`helix_int_cur`**: finite angle interval, axis-start position, major-radius position vector, minor-radius position vector, pitch position vector, apex-factor double, and unit axis vector, optionally followed by the solved curve cache and its fit tolerance. Position-vector components and the cache fit tolerance are lengths. The major and minor vectors have equal magnitude. Their orientation about the axis records handedness; the pitch vector records axial rise per revolution, and the apex factor records linear radial growth per revolution fraction. Without the solved cache, this complete construction is the exact curve carrier. A reversed record negates and swaps the angle bounds and negates the minor, pitch, and apex-factor fields, producing the parameterization `C'(t) = C(-t)`.

**`offset_int_cur`**: one subtype flag, source curve, start/end source-parameter doubles, model-space offset vector, then two `(string label, integer role code)` pairs, followed by the solved curve cache and its fit tolerance. The source curve and solved cache are distinct carriers. Offset-vector components and fit tolerance are lengths; parameters and role codes are unscaled.

**`subset_int_cur`**: parent curve followed by a two-bound native parameter interval, then the solved curve cache and fit tolerance. The parent and solved cache are distinct curve carriers. The interval is unscaled.

**`exact_int_cur`**: the solved `nubs`/`nurbs` curve cache is the authoritative exact construction payload, followed by its fit tolerance. No weaker analytic carrier is implied by the subtype. A zero fit tolerance denotes an exact cache.

**`comp_int_cur`**: a counted leading parameter array, component count, one parameter double per component, one ASM extension flag, then exactly that many ordered child curves. The final curve cache and fit tolerance follow the child curves. Component parameters and the leading parameter array are unscaled; child and solved NURBS control points and fit tolerance use the standard length scaling.

**Surface-related intcurve prefix**: two ordered support surfaces, two ordered BS2 parameter curves paired by side, one native parameter interval, then three counted discontinuity arrays. `null_surface` and `nullbs` are explicit absence sentinels. The interval and discontinuity values are unscaled.

**`off_int_cur`**: the surface-related prefix, one ASM extension flag, then signed left/right offset lengths. The solved curve cache and fit tolerance follow the offsets. The two offsets correspond to the two ordered support sides.

**`int_int_cur`** has context-first and cache-first forms. The context-first form is the surface-related prefix followed by one boolean ASM extension flag, then the solved curve cache and fit tolerance. The cache-first form starts with a positive serializer-revision integer and enum zero, followed by the solved curve cache and fit tolerance. Two ordered support surfaces follow. A referenced `spline` support carries one boolean before its subtype-table reference and four optional U/V bound fields after it; each optional bound is false when absent or true followed by one double when present. The two ordered parameter curves follow and may independently be `nullbs`. Two optional solved-curve interval endpoints follow; absent endpoints inherit the corresponding bound of the solved NURBS domain. Three counted discontinuity arrays and one integer ASM extension flag terminate the cache-first subtype. The construction is the intersection of the two ordered support surfaces; each non-null BS2 curve retains its parameterization on the corresponding support.

**`proj_int_cur`**: the surface-related prefix, one ASM extension flag, the source curve, and a second boolean flag. In the ranged form, a source-parameter interval and projection-role string (`surf1` or `surf2`) follow the flag before the solved cache. In the early-close form the subtype closes immediately after the flag and the solved carrier is external to that subtype payload.

**`sss_int_cur`**: the surface-related prefix, an integer selector, then a third support surface and its paired BS2 parameter curve. The solved cache and fit tolerance follow the third support pair. All three support sides retain their serialized order.

**Surface curves**: `blend_int_cur`, `surf_int_cur`, `par_int_cur`, and `skin_int_cur` have a context-first form containing the surface-related prefix with no subtype-specific tail, followed by the solved cache and fit tolerance. The subtype name distinguishes blend-edge, surface-constrained, parametric, and skin construction semantics. `blend_int_cur` also has a cache-first form: positive serializer-revision integer, enum zero, solved cache and fit tolerance, two ordered support surfaces with the same optional bound fields as cache-first `int_int_cur`, two nullable ordered parameter curves, two optional solved-curve interval endpoints, three discontinuity arrays, one integer extension, and one terminating boolean flag.

**Silhouette curves**: `silh_int_cur` and `para_silh_int_cur` append a cast surface and light vector to the surface-related prefix. `taper_silh_int_cur` adds one unscaled draft-factor double after the light vector. The solved cache and fit tolerance follow the silhouette tail.

**`off_surf_int_cur`**: the surface-related prefix, one ASM extension flag, base-surface U and V intervals, an embedded base curve and its interval, then distance, shift, and scale doubles. Distance is a signed length; all intervals, shift, and scale are unscaled. The solved cache and fit tolerance follow the tail.

**`spring_int_cur`**: two ordered support surfaces followed by two ordered BS2 curves, the native curve interval, three discontinuity arrays, one ASM extension flag, and a `CURV_DIR` enum. A `null_surface` is followed immediately by its U and V intervals. A `nullbs` in the first BS2 position is followed immediately by its parameter interval; a `nullbs` in the second position has no conditional interval. The solved cache and fit tolerance follow.

**`defm_int_cur`**: one ASM extension integer, an embedded bend curve, and an integer discriminator. Discriminator 8 is followed by four ordered vectors, a pair count, and two doubles per pair. Discriminator 5 is followed by one embedded support surface. The solved cache and fit tolerance follow either branch.

An embedded freeform support surface is encoded as the `spline` surface discriminator followed by its `nubs`/`nurbs` surface block. Its paired BS2 curve is a direct `nubs`/`nurbs` curve block. Surface control points use length scaling; UV poles, knots, weights, intervals, and discontinuities are unscaled.

Embedded analytic supports use the standard `plane`, `cone`, `sphere`, or `torus` discriminator followed by the same position, orientation, radius, angle, and flag payload used by the corresponding top-level carrier. A zero cone sine denotes a cylinder. Signed sphere and torus radii retain their signs.

**`exact_spl_sur` / `exactsur`**: the exact NURBS surface and its fit tolerance, followed by ordered U and V intervals and one ASM extension integer. The NURBS cache is the constructed surface. Native generation uses `exact_spl_sur`.

**`rule_sur` / `rulesur`**: two ordered profile curves followed by the solved NURBS surface and fit tolerance. The surface evaluates as the linear interpolation of the two profiles over its second parameter. Native generation uses `rule_sur`.

**`sum_spl_sur` / `sumsur`**: two ordered curves and a model-space origin followed by the solved NURBS surface and fit tolerance. The surface evaluates as the sum of the two curve positions minus the stored origin. Native generation uses `sum_spl_sur`.

**`rot_spl_sur` / `rotsur`**: one profile curve, a model-space axis origin, and an axis direction followed by the solved NURBS surface and fit tolerance. The profile knot domain is the construction's profile interval; the solved surface V domain is its angular interval. The native layout is not transposed. Native generation uses `rot_spl_sur`.

**`off_spl_sur` / `offsur`**: one support surface, signed offset distance, and U/V sense enums followed by the solved NURBS surface and fit tolerance. The modern name additionally carries a conditional one-to-three-boolean ASM tail: a false first flag ends the tail; a true first flag requires a second flag and permits a third. The legacy name has no ASM boolean tail. Native generation retains the form selected by the stored tail.

**`comp_spl_sur`**: the solved NURBS surface and fit tolerance occur first, followed by a float array and one component surface per array element. Each float is paired positionally with its component surface. The leading surface block is the face cache; trailing NURBS component surfaces do not replace it during cache selection.

**Rolling-ball aliases**: `rb_blend_spl_sur` and `rbblnsur` select the two-support rolling-ball layout. `sss_blend_spl_sur` and `sssblndsur` select the same prefix followed by a third-side graph. `pipe_spl_sur` and `pipesur` denote the surface-surface specialization. Native generation uses the modern spelling.

**Taper spline surfaces**: `taper_spl_sur`, `ortho_spl_sur`/`orthosur`, `edge_tpr_spl_sur`, `shadow_tpr_spl_sur`/`shadowtapersur`, `ruled_tpr_spl_sur`/`ruledtapersur`, and `swept_tpr_spl_sur`/`swepttapersur` share a support surface, reference curve, nullable BS2 pcurve, taper parameter, solved NURBS surface, and fit tolerance. Standard taper has no tail; orthogonal adds a sense boolean; edge adds a draft vector; shadow and swept each add a draft vector plus stored sine/cosine values; ruled adds the same fields plus a factor. Shadow and swept are distinguished by subtype name, not tail shape. Native generation uses the modern subtype corresponding to the retained variant.

**`loft_spl_sur` / `loftsur`**: two ordered loft sections precede two parameter intervals, two closure enums, two singularity enums, and a mode integer. Each section contains parameterized entries; each entry contains a counted profile and one path. Every profile member carries a type integer, curve, support surface, nullable BS2 pcurve, first flag, ASM integer, constraint subdata, and an optional direction selected by a second flag. Each path carries a curve, counted auxiliary BS3 curves, and a tail integer. Constraint subdata stores its type, row/column counts, leading scalar pairs, and per-column scalar pairs; type 211 stores exactly one leading pair and no column pairs. A variable sequence of boolean, integer, double, text, or enum tokens bridges the mode to the solved NURBS surface and fit tolerance.

**`cl_loft_spl_sur`**: the solved NURBS surface and fit tolerance precede four scale slots, an optional fifth scale, two flags, and a tail-kind integer. Present scale slots contain counted members, a path curve, counted auxiliary BS3 curves, and two tail integers. Each member contains a type integer, curve, and the same support, nullable BS2 pcurve, flags, constraint subdata, and optional direction used by a loft profile member. An absent scale consumes no token; the boolean beginning the next field remains at the cursor. Consequently the four leading scales form a contiguous prefix, the fifth scale requires all four leading scales, the kind-6 scale is required, and the second kind-7 scale is required. Kind 6 stores two flags, its scale, an integer, direction vector, interval, and BS3 curve. Kind 7 stores a flag, optional first scale, second flag, required second scale, integer, direction vector, and two trailing flags. Kind 0 stores two flags, a selector, selector-zero direction vector or selector-nonzero BS3 direction curve, and two trailing flags. Native generation uses `cl_loft_spl_sur`.

**`scaled_cloft_spl_sur`**: a singularity enum and singularity-selected shape payload precede six discontinuity arrays, one discontinuity flag, three scale slots, two flags, and an integer. The full shape payload is the solved NURBS surface and fit tolerance. The none shape payload replaces that cache with two intervals and two scalar arrays; its complete procedural graph is the exact face carrier. The three leading scales form a contiguous prefix under the same zero-token absence rule as `cl_loft_spl_sur`. A false branch flag selects a flag, integer, and selector-zero direction vector or selector-nonzero BS3 curve. A true branch flag selects an optional scale and a second flag. A true second flag requires another scale, integer, and direction vector; a false second flag stores another boolean, singularity enum, and BS3 curve. Every branch rejoins at two flags, an integer, two vectors, a singularity enum, and a BS3 curve. Native generation uses `scaled_cloft_spl_sur`.

**`skin_spl_sur`**: three surface enums, an integer, a scalar, and an inner count precede a structurally selected skin layout. The compact layout begins directly with a curve, loft subdata, integer, second curve, and final integer. The expanded layout contains `inner_count` entries, each comprising a type integer, curve, and loft profile data, followed by a path curve and two integers. Both layouts rejoin at a direction vector, scalar, recursive law formula, parameter curve, solved NURBS surface and fit tolerance, six discontinuity arrays, and a boolean. Native generation retains the selected layout.

**`net_spl_sur`**: two ordered loft-section graphs precede twelve frame scalars, one integer, four direction vectors, and four recursive law formulas. The solved NURBS surface and fit tolerance, six discontinuity arrays, and one boolean complete the payload. Native generation retains every section member, support, pcurve, constraint table, auxiliary path, frame value, and formula.

**`sweep_spl_sur` profile-first layout**: a primary enum precedes the profile curve and spine curve. A secondary enum, five direction vectors, one model-space point, four scalars, and three recursive law formulas follow. The solved NURBS surface and fit tolerance, six discontinuity arrays, and one boolean complete the payload. Native generation retains both curves and the complete construction graph.

**`sweep_spl_sur` explicit formula layout**: a primary enum and integer precede a profile curve, its two-scalar parameter interval, and an optional point-vector profile frame. A frame point and three vectors follow. Branch integer `1` then stores a boolean, path curve, model-length interval, scalar, boolean, recursive formula, and trailing boolean. The common solved-surface cache and discontinuity tail complete the payload. Native generation retains the complete construction graph.

**`sweep_spl_sur` explicit guide layout**: the explicit prefix matches the formula layout. Branch integer `2` stores a boolean, path curve, model-length interval, and scalar, followed by two booleans, an auxiliary guide curve, its two-scalar parameter interval, two integers, six scalars, and three booleans. The common solved-surface cache and discontinuity tail complete the payload. Native generation retains all three curves and the complete construction graph.

**`sweep_spl_sur` explicit support-surface layout**: the explicit prefix matches the other explicit layouts. Branch integer `3` stores a boolean, path curve, model-length interval, scalar, singularity enum, and support surface. A boolean gates an auxiliary curve. A support boolean and an optional legacy boolean precede the common solved-surface cache and discontinuity tail. Native generation retains the support surface, optional curve, and complete construction graph.

**`sweep_spl_sur` law-driven layout**: the explicit profile and frame prefix is followed directly by a recursive law instead of a branch integer. An integer, two-scalar interval, vector, integer, boolean, path curve, two-scalar interval, scalar, and boolean precede a second recursive law. A final integer, recursive formula, and boolean precede the common solved-surface cache and discontinuity tail. Native generation retains both law trees, the formula, both curves, and the complete construction graph.

**`t_spl_sur`**: the solved NURBS surface, fit tolerance, and discontinuity tail precede model-length U and V intervals and a type integer. A nested subtype scope contains either an inline `t_spl_subtrans_object` program with an optional boolean separator and companion values program, or a subtype-table `ref`. A trailing integer follows the nested scope. Both inline strings are line-oriented. Header tokens and topology, geometry, material, grouping, symmetry, annotation, knot, and grip record tokens select ordered field vectors; comments and unrecognized lines do not contribute typed records. A referenced subtransform resolves through the per-stream subtype table with cycle rejection. Native generation retains both programs byte-for-text, requires both parsed graphs to agree with their programs, inlines resolved shared programs into self-contained output, and uses the solved NURBS surface as the face carrier.

**Law formulas**: a text name begins each formula. `null_law` has no following payload. Every other formula carries a variable count followed by that many recursively framed law expressions. Integer, double, model-space point, and vector tags are terminal constants. `SPLINE_LAW` stores an integer, a knot float array, a control float array, and a model-space point. `TRANS` stores thirteen scalars and three enums. `EDGE` stores a curve and two parameters. Algebraic operator tokens are followed directly by their recursively framed operands. Trigonometric, hyperbolic, inverse-trigonometric, inverse-hyperbolic, `ABS`, `EXP`, `LN`, `LOG`, `SIGN`, `SIZE`, `TERM`, `SQRT`, `NORM`, and `NOT` operators are unary. `CROSS`, `DOT`, and `DCUR` are binary. `VEC` and `DSURF` are ternary. Native generation requires the exact fixed arity and rejects operators without a defined recursive boundary.

**`law_int_cur` / `lawintcur`**: the solved NURBS curve and fit tolerance precede the shared two-surface/two-pcurve support prefix, parameter interval, and three discontinuity arrays. The modern layout then stores an extension integer, one primary recursive formula, a formula count, and that many additional recursive formulas. Native generation uses `law_int_cur` and retains every support carrier and recursively referenced EDGE curve.

**`helix_spl_circ` / `helix_spl_line`**: an angular interval and secondary interval precede an inline helix path. The circular form length-scales the secondary interval and stores a length before the path and a circle radius after it. The linear form leaves the secondary interval unscaled and stores a model-space origin after the path. The inline path stores an angular interval, axis origin, length-bearing major, minor, and pitch vectors, apex factor, unit axis, two null surfaces, and two null pcurves. Native generation reconstructs the exact cacheless procedural surface.

**`defm_spl_sur` / `defmsur`, mode 8**: a support surface and discriminator `8` precede four deformation vectors and one selector integer. The solved NURBS surface, fit tolerance, and discontinuity tail complete the payload. Native generation retains the support and minimal deformation scaffold.

**`defm_spl_sur` / `defmsur`, modes 1 and 3**: both modes store four vectors, a scalar, three booleans, three vectors, a scalar, two booleans, a model-space point, and five booleans after the support surface and discriminator. Mode 1 appends a count and that many scalar triples. Mode 3 appends an integer and one guide scalar. The solved NURBS surface, fit tolerance, and discontinuity tail complete both payloads.

**`defm_spl_sur` / `defmsur`, mode 5**: a secondary surface, native long, boolean, scalar, integer, scalar, and deformation intcurve follow the initial support and discriminator. Four vectors, a scalar, three booleans, and a counted table of scalar triples precede the solved NURBS surface, fit tolerance, and discontinuity tail. Native generation retains both surfaces and the deformation curve.

**`defm_spl_sur` / `defmsur`, mode 6**: four vectors, a scalar, three booleans, an integer selector, a secondary surface, a native long, a boolean, and a scalar follow the initial support and discriminator. ASM versions above 225 then store one version-gated long. A second scalar, a deformation intcurve, two frames of four vectors plus a scalar and three booleans, and a trailing long precede the solved NURBS surface, fit tolerance, and discontinuity tail. Native generation retains both surfaces, the deformation curve, both vector frames, and the version-gated field.

**`g2_blend_spl_sur` / `g2blnsur`**: two ordered side graphs surround the first-side singularity payload. Each side stores a label, support surface, curve, two nullable BS2 pcurves, and a direction. The first side then stores a singularity enum. The full branch carries an optional BS3 support surface and paired tolerance. The none branch carries nine frame scalars, a tolerance, an optional intervening typed token, and a tertiary nullable BS2 pcurve. The second side is followed by an exact spline support, center curve, two center scalars, center integer, U/V intervals, four trailing scalars, the solved NURBS surface and fit tolerance, and three discontinuity arrays. Branch shape is structural; the singularity enum value is retained without assigning undocumented numeric meanings.

**`var_blend_spl_sur` / `srf_srf_v_bl_spl_sur`**: two ordered side graphs precede the primary curve, two signed offsets, and a radius-kind enum (`0` single radius, `1` two radii). Each side stores a label, support surface, curve, primary BS2 pcurve, model-space location, secondary BS2 pcurve, scalar, and tertiary BS2 pcurve. Radius controls use recursive blend-value payloads: `two_ends`, `edge_offset`, `functional`, `const`, or `interp`. Modern ASM blend values carry a boolean, optional discriminator, and calibrated enum. `const` recursively contains another blend value; `functional` stores a `(u,radius)` BS2 pcurve and numeric or symbolic terminal; `interp` stores counted parameter/radius/tangent/location/normal controls and an optional scalar-pair tail. Two-radii blends may append rounded-chamfer enums and a third blend value. Single-radius blends may append selector `1` or `7` and two scalars. U/V intervals, a shape integer/scalar/length/integer prologue, solved NURBS cache and fit tolerance, three ASM integers, secondary curve, convexity and render enums, post interval, BS3 curve, and nullable BS2 pcurve complete the graph. Native generation uses `var_blend_spl_sur`.

**`VBL_SURF` / `vertexblendsur`**: a counted sequence of boundary records followed by a grid-size integer and model-space fit tolerance. Every boundary begins with a type name, cross enum, model-space magic location, U/V smoothing enums, and fullness scalar. `circle` adds a curve, form enum, form-selected twist locations (zero for circle, one for ellipse, two for unknown), two parameters, and sense enum. `deg` adds a location and two normals. `pcurve` adds a support surface, nullable BS2 pcurve, sense enum, and parameter-space fit tolerance. `plane` adds a normal, two parameters, and curve. The complete boundary graph is the exact face carrier. Unknown boundary names and unsupported circle forms are invalid. Native generation uses `VBL_SURF`.

**`mesh_surface`**: the record has no payload tokens. It is a sentinel stating that no exact surface carrier is stored in the B-rep record. Display triangles belong to tessellation attributes on the owning face or body and do not become exact face geometry. A face referencing this record therefore retains an unknown exact surface and a typed native sentinel; it does not infer a surface from the display mesh.

### 7.4 Pcurves (2D UV trimming curves)

A `pcurve` record has two byte-level forms, discriminated by the `0x04` int at record-relative **+37**:

- **discriminator == 0 → inline form**: a `0x0a`/`0x0b` `wrapper_reversed` boolean, then a `0x0f 0d 0b exp_par_cur` subtype opening a 2D `nubs` or rational `nurbs` block. 2D poles are stored as `(u,v)` pairs (8+8 B each, **not** 24); `nurbs` stores one homogeneous weight after each pole.
- **discriminator != 0 (1, 2, −1) → ref form (72 B)**: a `0x0c` ref to the intcurve carrying the UV curve, then two parameter doubles. No wrapper boolean (its absence is structural).

UV poles are dimensionless surface parameters. `wrapper_reversed` is the inline curve's fit-convention bit, independent of coedge sense and of the parameter-interval sign.

The inline control polygon is followed by a `DOUBLE` parameter-space fit tolerance. After the nested support-surface scope, four ordered trailing booleans precede two final `DOUBLE` values storing the pcurve parameter interval `(t_start, t_end)`. The four booleans are retained and regenerated independently. The balanced `exp_par_cur` scope contains exactly one BS2 carrier; that structurally owned block is the inline pcurve even when an evaluated face cache does not reproduce its endpoints within modeling tolerance. Ref-form pcurves store the same interval immediately after their intcurve reference and have no wrapper, boolean tail, or inline fit-tolerance carrier. A referenced intcurve can contain a lifted BS3 carrier, one or more genuine BS2 carriers, and BS3 blocks whose bytes also admit a BS2 parse. A sole genuine BS2 carrier is the referenced pcurve by serialized dimensional role. Surface-image agreement selects among multiple genuine BS2 carriers; byte-compatible BS3 interpretations rank after them.

Pcurve UV coordinates use the owning surface's exact parameterization. A procedural surface's solved NURBS block is an evaluated model-space cache and does not redefine that parameterization. Candidate validation against the solved cache therefore applies only when the NURBS surface is itself the exact carrier; a pcurve on a procedural surface selects the first unambiguous BS2 carrier in native traversal order.

One pcurve carrier may span a longer parameter domain than an edge using it. Candidate validation evaluates the edge's native stored `t_start` and `t_end` under both parameter signs rather than requiring the pcurve's full knot endpoints to equal the edge vertices. Only intervals whose endpoints lie in the pcurve knot domain are eligible; the full knot domain is the fallback when neither sign lies in that domain. The selected signed interval belongs to the coedge's pcurve use. It remains in the pcurve's native parameterization when the neutral 3D edge parameter is length-normalized. Edge sense selects the first eligible sign tested; pcurve-wrapper and coedge orientations are independent, so model-space endpoint agreement selects between the eligible signs.

Coedge sense is the edge-use orientation for a pcurve inherited from its surface: `effective_pcurve = flip_pcurve(surface_pcurve, coedge.sense)`. The stored 2D B-spline poles and knots retain their native order. `wrapper_reversed` is separate from coedge sense.

An explicit pcurve reference belongs to a free-form B-spline face. Analytic plane, cylinder, cone, sphere, and torus faces store `-1` in the coedge pcurve field; their UV boundary is not serialized as a pcurve record.

### 7.5 `nubs`/`nurbs` blocks (B-spline curves and surfaces)

Surface block grammar: name (`nubs`|`nurbs`), degree_u, degree_v, u/v periodicity + singularity enums, unique-knot counts, (knot, multiplicity) pairs for each direction, then the control grid (3D for `nubs`, 4D homogeneous for `nurbs`). Control grids are **row-major with v in the outer loop, u in the inner loop.**

**Pole-count rule:** the block stores endpoint multiplicities as `degree` (not `degree+1`). With stored multiplicities: `n_poles = sum(stored_mults) − (degree − 1)`. With expanded (clamped) multiplicities: `n_poles = sum(expanded_mults) − (degree + 1)`. Both expressions produce the same pole count.

Native ASM NURBS control grids are the per-face cache. `surface_fit_tolerance == 0.0` indicates fidelity to the procedural surface, rather than identity with a primitive.

### 7.6 `intcurve` and `spline` subtypes

Procedural intcurve subtypes (`exact_int_cur`, `off_int_cur`, `proj_int_cur`, `int_int_cur`, `sss_int_cur`, …) and spline-surface subtypes (`rb_blend_spl_sur`, `sss_blend_spl_sur`, `var_blend_spl_sur`, `loft_spl_sur`, `sweep_spl_sur`, `net_spl_sur`, VBL/taper families, …) each carry per-subtype field tails and version/`asm_major` gates. A `ref N` nested inside a surface, curve, or pcurve body indexes a per-file subtype table, not a byte offset. Each subtype definition — a `0x0F` opening followed by a `0x0d`/`0x0e` name token other than `ref` — contributes one table entry in stream order. Definitions are recognized at token boundaries only: the same byte pattern inside a token payload (an `f64`, a string body) is data, not a table entry.

Legacy intcurve subtype names select the same layouts as their modern names: `bldcur`→`blend_int_cur`, `blndsprngcur`→`spring_int_cur`, `exactcur`→`exact_int_cur`, `lawintcur`→`law_int_cur`, `offintcur`→`off_int_cur`, `offsetintcur`→`offset_int_cur`, `offsurfintcur`→`off_surf_int_cur`, `parasil`→`para_silh_int_cur`, `parcur`→`par_int_cur`, `projcur`→`proj_int_cur`, `surfcur`→`surf_int_cur`, `surfintcur`→`int_int_cur`, `d5c2_cur`→`skin_int_cur`, and `subsetintcur`→`subset_int_cur`. Native generation uses the modern spelling.

Legacy spline-surface subtype names select the same layouts as their modern names. This includes `cylsur`→`cyl_spl_sur`, `skinsur`→`skin_spl_sur`, `netsur`→`net_spl_sur`, `sweepsur`→`sweep_spl_sur`, `sclclftsur`→`scaled_cloft_spl_sur`, `varblendsplsur`→`var_blend_spl_sur`, and `srfsrfblndsur`→`srf_srf_v_bl_spl_sur`. Native generation uses the modern spelling.

An `intcurve` or `spline` record carries a record-level sense boolean immediately before its subtype scope (`0x0a` reversed, `0x0b` forward). A reversed record's geometry is the reverse of its subtype definition: a reversed intcurve parameterizes as the negation of its cache (`C(t) = cache(−t)`; the owning edge's `t_start`/`t_end` are on the reversed parameterization), and a reversed spline surface's normal is the reverse of the cache normal (the face's sense field composes on the reversed surface).

A `spline` subtype can contain several top-level surface-bearing `nubs` or `nurbs` blocks. The final surface block is the face-surface cache; earlier blocks can be 2D support pcurves. A nested `ref` denotes another carrier through the subtype table.

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

`u_start` and `u_end` are directrix parameters. `extrusion_direction` is length-bearing. `POSITION` is stored in model-space length units and is retained independently of the directrix. The optional final `surface-cache` is the solved NURBS surface, and `cache_fit_tolerance` is a length. Without that cache, the directrix, parameter interval, direction, and position still define and retain the exact translational-extrusion construction. Native generation writes the stored interval and position without deriving or replacing either field.

A translational extrusion is an analytic cylinder when its directrix is a closed nonperiodic rational NURBS comprising four ordered quarter-circle Bézier spans and `extrusion_direction` is parallel to the circle normal. For degree `p >= 2`, the carrier has `4p + 1` poles, endpoint knot multiplicity `p + 1`, interior knot multiplicity `p`, and four positive parameter spans. Repeated homogeneous Bézier degree reduction of every span produces a rational quadratic with one common nonzero endpoint weight `w` and middle weight `w sqrt(1/2)`; multiplying every homogeneous weight by the same nonzero scalar does not change the carrier. In Euclidean coordinates, each reduced middle pole is the sum of its two endpoint poles minus the common center. Consecutive endpoint radial vectors are perpendicular, have the same positive length, and have consistently oriented cross products. The cylinder origin is the common center, its axis follows the normalized extrusion direction, its reference direction follows the first radial vector, and its radius is the shared radial length. The analytic carrier takes precedence over an optional solved NURBS cache. A nonclosed, noncircular, degenerate, or obliquely extruded directrix remains a procedural extrusion or retains its solved NURBS carrier.

```
rb_blend_spl_sur :=
  0x0f 0x0d "rb_blend_spl_sur"
  rolling-ball-side
  rolling-ball-side
  curve slice
  LENGTH offset_left
  LENGTH offset_right
  (ENUM_VALUE -1 | DOUBLE radius_selector)
  INTERVAL u_range
  INTERVAL v_range
  DOUBLE parameter[3]
  LONG tail
  surface-cache
  DOUBLE cache_fit_tolerance
  FLOAT_ARRAY discontinuity[3]
  [rolling-ball-third-side]
  0x10

rolling-ball-side :=
  TEXT label
  surface
  curve
  nullable-bs2-pcurve
  POSITION location
  nullable-bs2-pcurve
  nullable-spline-surface

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

The two offsets and fit tolerance are lengths. `ENUM_VALUE -1` selects the absent-radius branch; a `DOUBLE` carries an explicit selector value. Each side retains its support surface, side curve, primary and secondary pcurves, model-space location, and optional exact spline support. `sss_blend_spl_sur` appends the third-side graph after the three discontinuity arrays. The final surface cache is the solved face surface.

A circular rolling-ball construction with equal nonzero signed offsets has a constant radius equal to the offset magnitude. Two nonparallel plane supports and a nonperiodic collinear NURBS slice define an analytic cylinder when the slice direction is parallel to the planes' intersection and every slice pole lies on a line whose perpendicular distance from each plane equals the constant radius. The cylinder axis is that line, the radius is the offset magnitude, and its reference direction is the canonical direction derived from the axis.

One plane support, one circular-cylinder support, and a four-quarter rational-circle slice satisfying the homogeneous degree-reduction invariant above define an analytic torus when the plane normal, cylinder axis, and slice normal are parallel; the slice center lies on the cylinder axis; the center-to-plane distance equals the constant radius; and the absolute difference between the slice radius and cylinder radius equals the constant radius. The torus center, axis, reference direction, and major radius are the slice circle's frame and radius. Its signed minor radius is the common signed offset. The analytic carrier takes precedence over the solved NURBS cache. A variable-radius construction, noncircular cross-section, nontangent support, noncollinear slice, or noncircular slice retains the solved NURBS carrier.

---

## 8. Materials and appearance (non-B-rep)

### 8.1 Design metadata

`MetaStream.dat` is a sequence of object records. Each record contains an ASCII type name, a u32 ID count, that many little-endian u64 design-entity IDs, a self GUID, a zero-run delimiter, a secondary GUID, and a trailing u32 record revision. The ID count is a count rather than a flag; a record can carry more than two IDs.

The type name is a nonempty printable ASCII string and is not itself a GUID. It is an open discriminator: known names select typed Design object classes; every other name retains its exact bytes together with the same entity-ID, GUID-chain, delimiter, and revision fields.

The design `BulkStream` caches each body's axis-aligned bounding box in the three indexed records whose identities are the body entity suffix plus one, two, and three. Each record contains a `u8 1` marker followed by the same six contiguous f64 values in centimetres, ordered `(xmax, ymax, zmax, xmin, ymin, zmin)`. Every maximum is greater than or equal to its corresponding minimum, and a body has positive extent on at least one axis. The three marker-and-sextuple frames are byte-identical despite the records' different dynamic class tags and prefix lengths. Model-space bounds convert each coordinate to millimetres. The body entity suffix joins the cache to every BREP body-map pair carrying that suffix in the same Design stream.

The design BulkStream BREP body map is `u32 count`, followed by `count` ordered pairs of `u64 asm_body_key, u64 entity_suffix`, then `u64 trailing_record_ref`, `u32 pad`, `u32 char_count`, and UTF-16LE `BREP.<uuid>.smbh`. `asm_body_key` is the ASM body `flags` field. `entity_suffix` is the numeric suffix of the design entity ID. The BREP basename qualifies the key namespace; a pair resolves to a solved body only when its basename names the selected active BREP and exactly one solved body carries its key. Retained body-key edits update the ASM field and every joined Design body-map occurrence atomically.

A construction-recipe record places its i32 record index at 16 bytes before the recipe-family name, eight zero bytes at `name−12`, and the u32 byte length of the ASCII family name at `name−4`. The family name selects `body_recipe_data`, `face_recipe_data`, `bounded_face_recipe_data`, `edge_recipe_data`, or `vertex_recipe_data`; the preceding record index is not a family discriminator. The common payload begins with an i64 `−1` null sentinel followed by the five i32 values `[2, 0, −1, 1, −1]`.

An unresolved edge-selection record consists of its eleven-byte indexed header, fourteen zero bytes, `u32 19`, and ASCII `EDGE_REFERENCE_LOST`. The indexed header immediately following the marker terminates the record. Consecutive unresolved selections share that boundary: the following header of one record is the owning header of the next. An unresolved run belongs to the counted edge group whose first identity-wrapper header is the run's final following header. The run cardinality equals the group's operand count. Such a group has an unresolved neutral edge selection; its native records retain the ordered unresolved operands.

Standalone persistent-reference properties named `pt_tag`, `crv_primary_id`, and `crv_secondary_id` store the u32 pair `(2, 14)`, a 14-byte property slot, LP-ASCII `IntrinsicMetaTypeuint64`, and the referenced u64. The compact properties embedded in sketch point and curve records omit the `(2, 14)` pair and property slot.

A browser-node record stores a length-prefixed 36-character UTF-16LE node GUID, a one-byte hidden flag, the `0x01 0x01` marker, and the node's `u64` design-entity suffix. Flag `1` hides the entity in the document display; `0` shows it. **Body visibility join:** ASM `asm_body_key` → BREP body map `entity_suffix` → browser-node hidden flag. Native writing emits this join for every body with explicit visibility and retained writing patches the hidden flag in place.

A browser body record carries the body's appearance binding. The record head references the body's design entity with the `299` class tag (`u32 3`, ASCII `299`, `u64 entity`). The appearance fields open with the marker GUID pair `D87FBE62-3B12-4CA8-9014-BAD31ABDB101` and `C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C` as consecutive length-prefixed 36-character UTF-16LE strings, then in order: the LP-UTF16 physical-material token (`PrismMaterial-###`) with `0x01 + u64` entity reference, the LP-UTF16 36-character browser-node GUID with `0x01 + u64` node entity, an optional LP-UTF16 display name, zero padding, an f32 opacity, the `0x01 0x01` marker, zero padding, and the LP-UTF16 visual GUID (a 36-character GUID with `_Post2015` repeats). The node entity equals the body's design entity plus one. **Body appearance join:** `299`-tag entity → BREP body map `entity_suffix` → ASM `asm_body_key`; the visual GUID's 36-character prefix selects the appearance asset. ASM body records whose key field is null (`-1`) are sub-bodies of the stream's keyed body and carry no design records of their own.

A sketch entity container follows its self-validating entity header and UTF-16LE entity ID with `u32 record_reference`, `u32 zero`, `0x01`, `u32 reference_count`, then `reference_count` entries of `0x01 + u32 record_index + six zero bytes`. The referenced records contain the sketch's geometry and relations.

Typed non-construction sketch curves form neutral profile loops through endpoint incidence at the document linear tolerance. A full circle or full ellipse is a one-curve loop. A bounded line uses its stored endpoints, an arc uses the endpoints evaluated from its center, radius, and angles, a bounded ellipse uses its rotated parametric endpoints, and a clamped nonperiodic NURBS curve uses its first and last control points. An unbranched connected component is a profile exactly when every endpoint vertex has degree two. A branched component consisting entirely of lines uses the counterclockwise angular order of outgoing half-edges at each vertex; every positive-area bounded face is one profile and the negative-area exterior walk is excluded. Profile order starts with the lowest neutral sketch-entity identity, and each subsequent use records whether traversal opposes the entity's stored direction. Open chains, zero-area walks, and branched components containing nonlinear curves do not form neutral profiles. Strictly nested closed line loops form atomic planar regions. A region's exterior is the smallest-area loop containing every projected selection point, and its holes are the exterior's immediate children in the strict-containment tree. Touching or intersecting loops do not form a containment relation. Projected points on any loop boundary do not select an adjacent region. A point carried by an edge perpendicular to the sketch plane selects the region containing the edge's coincident endpoint projections.

An indexed Design record header is `u32 class_tag_length`, a three-digit ASCII dynamic-class tag, then `u32 record_index`. `record_index` is a logical reference value; it is independent of the header's byte offset in the `BulkStream`. Record indices and Design entity suffixes are local to one Design `BulkStream`; indexed-record, sketch-owner, parameter-owner, and geometry joins never cross `BulkStream` boundaries.

An indexed parameter record extends that header with eleven zero bytes, a u64 family discriminator, one zero byte, `u32 source_ordinal`, and one of two owner forms. The discriminator is `6` for exact `source_kind` `TangencyWeight` and `0` for every other source kind. A document parameter stores `u8 0`. A dimension or feature parameter stores `u8 1`, `u32 owner_record_index`, and six zero bytes; the owner resolves through the indexed Design record graph. Both forms then store an LP-UTF16 source expression. The document form follows the expression with eight zero bytes and `u8 1`; the owned form follows it with nine zero bytes. The remainder is LP-UTF16 `source_kind`, `u32 0`, an optional LP-UTF16 unit token, LP-UTF16 parameter name, f64 evaluated scalar, and the fixed twelve-byte tail `00 01 13 00 00 00 00 00 00 00 00 00`. A dimensionless parameter may encode the absent unit as an explicit zero-length LP-UTF16 field. Exact `source_kind` `User Parameter` denotes a document parameter, a value containing `Dimension` denotes a dimensional constraint, and every other value denotes a feature input. The declared `mm` scalar is stored in centimetres and the declared `deg` scalar is stored in radians. Dimensionless and Boolean parameters omit the unit token. `source_ordinal` is unique within one Design `BulkStream`. A neutral parameter identity is the byte-length-prefixed Design stream name followed by `source_ordinal`; indexed-record position and owner-record position do not participate in parameter identity.

Every dimension or feature-input parameter has a 104-byte indexed owner frame at `owner_record_index`. After the eleven-byte indexed header it stores eight zero bytes, `01 + u32 1`, `01 + u32 scope_record_index`, six zero bytes, `u32 local_ordinal`, `u8 0`, f64 evaluated scalar, `01 + u32 parameter_record_index`, six zero bytes, `u32 owned_ordinal`, four zero bytes, a second `01 + u32 scope_record_index`, six zero bytes, `u8 1`, a u8 variant flag, `u8 0`, `01 + u32 companion_record_index`, seven zero bytes, a third `01 + u32 scope_record_index`, and six zero bytes. The three records are consecutive in either owner-parameter-companion or parameter-owner-companion order. The evaluated scalar bitwise equals the parameter record's scalar. `(scope_record_index, local_ordinal)` and `owned_ordinal` are unique. The scope record is the owning sketch or construction operation.

The indexed companion record begins with a 58-byte common prefix. Its eleven-byte indexed header is followed by 20 zero bytes, `01 + u32 owner_record_index`, six zero bytes, a nonzero u64 Unix-epoch timestamp in microseconds, and eight zero bytes. Its `record_index` equals the owner frame's `companion_record_index`. The companion-owned payload interval begins after this prefix and ends at the next indexed parameter record, parameter-owner header, parameter-scope primary header, or primary header referenced by a parameter scope other than the companion owner's scope in the same Design `BulkStream`. A terminating record immediately after the prefix denotes an empty companion payload. A primary header referenced by the owning scope remains inside the payload interval. Another indexed-record header can begin a nested companion record or the following sibling record; byte adjacency alone does not select between those roles. Construction-recipe records whose name fields begin within this interval belong to the companion and retain interval order.

Within a dimensional companion, every contained construction-recipe family name belongs to the immediately preceding indexed record header. The containing record ends at the next indexed record header or at the end of the companion-owned payload. A nonempty recipe-specific prefix extends from the end of the eleven-byte indexed header to the u32 length field of the recipe-family name. A selector-bearing prefix stores ten zero bytes, `u32 1`, `u32 3`, a nonzero u32 value, one or more persistent Design operands, and a terminating `u32 0`. An operand stores a nonzero u32 value, then its decimal selector as either LP-ASCII followed by `u32 0` or unframed ASCII followed by four zero bytes, then `u32 1`, a nonzero u32 Design reference, and `u32 0`. The first operand uses LP-ASCII. Every selector/reference pair and its byte offsets are retained in prefix order. Each pair joins to every active solved face or edge whose persistent-subentity tag contains the same token and Design reference; the candidate face and edge sets are independently ordered by entity identity and deduplicated. The bytes after the family name through the containing-record boundary form a nonempty contiguous sequence of little-endian i32 values with no padding. The containing header's byte offset, dynamic class tag, record index, frame length, companion identity, recipe ordinal, complete prefix bytes and offset, program offset, and complete program are retained with the recipe identity. The indexed header, prefix, length-prefixed family name, and program partition the containing record without gaps.

An edge recipe's words after its seven-word common prologue may occur as a contiguous subsequence of a dimension recipe program. Every such byte-identical containment joins the dimension recipe record to that edge operand. Matching operand identities are ordered and deduplicated; repeated equivalent edge recipes remain distinct operands.

A recipe-backed linear dimension without a locus frame measures solved locus pairs in its owning Sketch at the parameter's evaluated separation. Candidates are axis-aligned point pairs and parallel line pairs. Point pairs produce horizontal or vertical point-locus distance according to their shared sketch coordinate. A parallel line pair produces an entity distance. One candidate is a single distance constraint. Multiple candidates form one repeated-distance constraint when they partition their participating entities into disjoint pairs; the pair order is point-pair discovery order followed by line-pair discovery order. A candidate graph that shares an entity between pairs is ambiguous and retains the ordered recipe operands as a native parameter-backed constraint. Zero candidates also retain the ordered recipe operands.

A paired-locus dimension frame occurs within its parameter companion's owned record interval and is delimited by two indexed headers with the same `record_index`. It may begin immediately after the 58-byte companion prefix or follow intermediate indexed records. Its primary header is followed by eight zero bytes, `01 + u32 3`, `01 + u32 0`, six zero bytes, an opaque u32, `01 + u32 first_geometry_record_index`, six zero bytes, a u32 first-role code, `01 + u32 second_geometry_record_index`, six zero bytes, and a u32 second-role code. Both geometry indices resolve to typed sketch-point or sketch-curve records.

A parameter companion may own one or more non-overlapping counted-locus dimension frames in stream order. Each indexed header is followed by eight zero bytes, `01 + u32 locus_count`, and `locus_count` entries of `01 + u32 geometry_record_index + six zero bytes + u32 role`. The entries are followed by `00 01 + u32 sketch_entity_suffix + six zero bytes + u32 owner_role`, a u32 constraint-state mask, a second u32 equal to `locus_count`, and that many `01 + u32 geometry_record_index + six zero bytes` return entries. One zero byte ends the frame immediately before the next indexed header. Every geometry index resolves to a typed sketch-point or sketch-curve record. The return run is a permutation of the locus geometry indices. The sketch suffix resolves to a Sketch-typed Design entity.

The counted-locus frame's counted references also satisfy the generic sketch-relation prefix grammar. A frame reached through a Dimension parameter companion is a dimension frame only; a generic sketch relation at the same Design `BulkStream` byte offset is not a second constraint.

A null-locus dimension frame has the same companion-owned interval rule and is delimited by primary and paired indexed headers with the same `record_index`. The primary header is followed by eight zero bytes, `01 + u32 2`, `01 + u32 0`, six zero bytes, a u32 null-role code, `01 + u32 geometry_record_index`, six zero bytes, and a u32 geometry-role code. A variable annotation payload extends from that fixed operand prefix to the paired header. The nonzero geometry index resolves to a typed sketch-point or sketch-curve record owned by the parameter scope's sketch. In an `Angular Dimension-2` companion, null role `14` followed by line role `3` measures the line from the positive sketch-u axis and is a parameter-backed axis-angle constraint. Paired-locus and counted-locus frames may coexist in one companion-owned interval; the counted form may repeat.

A null-locus frame or counted-locus frame resolving to exactly one circular sketch entity is a radial dimension when the parameter `source_kind` begins with `Radius Dimension` or `Diameter Dimension`. The entity is a full circle or circular arc. A radius parameter's centimetre scalar multiplied by ten equals the solved millimetre radius; a diameter parameter's converted scalar equals twice that radius. Converted scalar `s` and measured value `m` agree when `|s - m| <= 1e-9 * (1 + max(|s|, |m|))`. The parameter drives a neutral radius or diameter constraint respectively. A noncircular entity, multiple loci, nonfinite scalar, or scalar/geometry mismatch retains the native dimensional relation.

When a paired-locus frame's two typed loci completely determine a linear or angular constraint, that frame is the companion's governing dimensional relation. Counted-locus frames in the same companion are auxiliary graph records for that relation, not additional dimensions; they retain their native roles and order without duplicating the parameter-backed constraint.

For a linear companion governed only by counted-locus frames, a two-point group whose points share one sketch coordinate and whose other-coordinate separation equals the evaluated parameter is the governing directional-distance relation. The evaluated parameter is stored in centimeters and the sketch coordinates are millimeters. State-zero groups in the companion are first classified as auxiliary graph relations: coincident point pairs, coincident point-on-curve loci, perpendicular or parallel line pairs, collinear line pairs, and three-locus reflection symmetry are determined by their typed sketch geometry. Point-on-curve membership includes bounded lines, circular arcs, elliptic arcs, and the endpoint loci of clamped nonperiodic NURBS curves; a full circle or ellipse uses its entire parameter domain. Auxiliary relations do not carry the dimensional parameter. A remaining state-zero group with exactly two typed loci is the parameter-backed distance relation.

A counted linear frame with state mask `0x20` stores an offset graph. Its locus list is an equal-length nonzero-role source prefix followed by a zero-role result suffix. Every locus resolves to a line. The return run alternates one source index and one result index, uses every locus exactly once, and supplies the source/result bijection. Every pair is parallel and has the same nonzero signed separation along the source line's stored left normal. The shared signed separation is the graph's offset distance; the companion parameter is not duplicated onto the auxiliary offset relation.

An angular paired- or counted-locus frame may encode one line and one point instead of two lines. The point is an endpoint indirection. Among lines owned by the same sketch and incident to that point at a stored endpoint, exactly one forms either the stored angle or its supplement with the explicit line. That unique incident line replaces the point in the parameter-backed angular constraint. No line is selected when endpoint incidence and the evaluated radian value do not determine one unique candidate.

A parameter scope is one logical indexed record delimited by two headers carrying the same `record_index`. Both three-digit class tags are per-file dynamic values. The primary header begins the scope payload. An LP-UTF16 feature-family name ends exactly 78 bytes before the paired header. Immediately before the LP-UTF16 field are a u32 history-state identity and an ordered reference table. The table is `u32 reference_count` followed by `reference_count` entries of `01 + u32 record_index + six zero bytes`; it ends at the history-state identity. The 78-byte tail starts with a nonzero u32 ordinal among scopes of the same feature family. Its u32 at tail offset 31 is the preceding history-state identity. A history-state identity is either `0xffffffff` for null or the `state_id` of an ASM delta-state record; the current and preceding identities are null together. When one scope's preceding identity equals another scope's current identity, the former follows and depends on the latter in the modeling history. Every member resolves to the primary header of an indexed Design record in the same stream. `Sketch`, `Extrude`, `Fillet`, and `Chamfer` select the corresponding sketch or construction-operation scope. A `Sketch` scope contains exactly one `01 + u32 entity_suffix + six zero bytes` reference to its Sketch-typed Design entity header; this joins the parameter scope to the sketch container and its relation graph. A neutral feature identity is the byte-length-prefixed Design stream name, byte-length-prefixed feature-family name, and family-local scope ordinal. Indexed-record positions do not participate in feature identity.

An `Extrude` scope stores its result-operation u32 at primary-header offset 28. When an indexed reference occupies offsets 25 through 35 as `01 + u32 record_index + six zero bytes`, the operation follows at offset 38. Operation values are `1 = join`, `2 = cut`, `3 = intersect`, and `4 = new body`. The two immediately following u32 values select the extent form: `(1, 1) = one-sided to face`, `(1, 2) = one-sided distance`, and `(2, 0) = two-sided distance`. Primary-header offset `operation + 12` is the direction-reversal Boolean, offset `operation + 13` is byte `1`, and offset `operation + 14` is the start-support u8: `0 = profile plane`, `1 = offset profile plane`, and `2 = selected face`. The direction-reversal Boolean selects travel opposite the profile normal for a one-sided to-face extent and is zero for distance extents. A one-sided distance has exactly one `AlongDistance` parameter and no `AgainstDistance` or `Side1Offset`; a one-sided to-face extent has neither distance parameter and exactly one `Side1Offset`; a two-sided distance has exactly one of each distance parameter and no `Side1Offset`. A profile-plane start has no `ProfileOffset`; offset-profile-plane and selected-face starts have exactly one `ProfileOffset`. The number of face operand groups equals one for a one-sided to-face termination plus one for a selected-face start. Join, cut, and intersect require an existing-body operand group; new body has no existing-body operand group.

Within a `Fillet` scope, an owned parameter whose `source_kind` is `Radius` carries a fillet radius. One positive radius selects the constant-radius form. Within a `Chamfer` scope, `Distance` selects an equal-distance chamfer, `Distance 1` and `Distance 2` select the two-distance form, and `Distance` together with `Angle` selects the distance-angle form. The distance scalars use the parameter record's centimetre storage and the angle scalar uses its radian storage.

Every `Fillet` and `Chamfer` scope has at least one edge operand in its ordered reference table. An edge operand's primary indexed header has record index `N`. It is followed by a same-index paired header and three consecutive nested indexed headers with record indices `N+1`, `N+2`, and `N+3`. Exactly one `edge_recipe_data` construction recipe occurs after the `N+3` header and before the following indexed header. The bytes after the `N+3` header and before the family-name length field are retained as the recipe prefix; selector-bearing prefixes use the persistent Design operand grammar defined above, and every decoded selector/reference pair carries independently bound face and edge candidates. From the end of the recipe-family name to the following indexed header, the payload is a nonempty, contiguous sequence of little-endian i32 values with no padding. Its first seven words are `[-1, -1, 2, 0, -1, 1, -1]`, comprising the common i64 null sentinel and five-i32 prologue. The complete sequence and its first-byte offset are retained as the recipe program. The recipe's nonnegative record-index field is its persistent Design reference and joins to every active face tag carrying that reference; the candidate face set is ordered by face identity and deduplicated. Each operand retains the candidate faces and their stable boundary-edge slots present in the scope's result ASM topology. It also retains the candidate faces present in the preceding topology, the subset deleted or assigned a different revision by the scope transition, the stable edge slots on those preceding face boundaries, and the subset of boundary edges changed by the transition. Changed boundary edges are partitioned into deleted stable slots and stable slots assigned a different record revision; their ordered union is the changed boundary set. Every changed boundary edge retains each incident coedge in stable coedge-slot order with its owner loop and face, loop boundary count, zero-based coedge ordinal, and preceding and following edge slots. A topology-entry selector is in `0..=2`, occurs at most once in each of the two ordered clauses, and increases strictly within a clause. Equal selector values group topology-context entries across clauses; a selector absent from one clause is one-sided. Each topology triplet is `[v, e, v]`, where `v` is a one-based loop-vertex ordinal not exceeding the entry's boundary-edge count. `e = v - 1` names the edge preceding that vertex, and `e = v` names the following edge. The decoded zero-based edge ordinal is `(v + boundary_edge_count - 2) mod boundary_edge_count` for the preceding form and `v - 1` for the following form. Each clause retains the changed historical edge slots occurring at the loop position named by each triplet. For each selector context, the decoder also retains every changed boundary edge whose distinct incident-loop boundary counts satisfy all present clause entries. Selector contexts are ordered by selector value. An edge operand resolves when every selector context has at least one incidence-compatible changed edge, at least one prefix reference shares a changed boundary edge with the primary candidate faces, and the intersection of every selector incidence set and every nonempty changed shared-edge set contains exactly one stable edge slot. An edge group resolves when its complete ordered member run resolves; the projected edge list follows member order, removes repeated edge identities after their first occurrence, and retains the native group identity. A missing operand, duplicate operand identity, empty selector run, empty selector candidate set, absent persistent-reference adjacency, empty or nonsingleton final intersection, or lost edge reference prevents resolution. For every prefix reference in source order, the operand retains its referenced faces in both the result and preceding topologies. Each side retains the stable edge slots shared by the referenced-face boundaries and the primary candidate-face boundaries; the preceding side additionally retains the subset deleted or updated by the transition. The recipe order follows the scope reference-table order.

The structured edge-recipe program tail is a `-1`-delimited sequence of nine through eleven nonempty runs. Run 0 contains one root word. Each of the two following side clauses contains a two-word header, two or three one-word scalar runs, and a final run of length `2 + 8n`. The header's first word is `3` for two scalar runs and `4` for three scalar runs; it counts the scalar runs plus the payload field. The final run starts with i32 zero and i32 `n`, followed by exactly `n` ordered eight-word entries. An entry is an i32 side-local selector, a positive i32 count of edges in the referenced face loop, and two topology triplets. Each triplet retains its three words, zero-based vertex and incident-edge ordinals, and preceding/following side. When both triplets name the same incident-edge ordinal, the entry also retains that common ordinal. The root, header count and value, every scalar word, payload entry count, and every entry field are retained. A tail outside this grammar remains an exact unstructured program.

An `Extrude` face-group member has the same indexed-record envelope: primary record `N`, a same-index paired header, and consecutive nested records `N+1`, `N+2`, and `N+3`. Exactly one `face_recipe_data` or `bounded_face_recipe_data` construction recipe occurs after the `N+3` header and before the following indexed header. The exact recipe family is retained per face operand. The bytes after the `N+3` header and before the family-name length field are retained as the recipe prefix; selector-bearing prefixes use the persistent Design operand grammar defined above, and every decoded selector/reference pair carries independently bound face and edge candidates. From the end of the framed recipe-family name to the following indexed header, the payload is a nonempty, contiguous sequence of little-endian i32 values with no padding. The complete sequence and its first-byte offset are retained as the recipe program. The program starts with `i32 0`, `i32 -1`, and a positive i32 node count. Exactly that many nodes follow. Each node starts with `i32 -1`, `i32 -1`, `i32 2`; the next node opener or the following indexed header terminates it. The node payload is a `-1`-delimited root scalar, two one-word prelude runs, and two topology side clauses. Each side clause has the same counted header, two or three one-word scalars, zero-tagged counted payload, and ordered eight-word entry grammar as an edge recipe side. The root, first prelude scalar, and every side scalar are either zero or a positive local topology reference not exceeding the declared node count. Their nonzero references are retained in source-field order with repetitions. These local topology references do not identify the containing node's table ordinal. Each node retains this structure, its complete word sequence, its opener byte offset, and its exclusive terminating byte offset. The recipe's persistent Design reference joins to every active B-rep face whose persistent-subentity tag contains that reference; the resulting candidate set is ordered by B-rep face identity and deduplicated. Candidate faces explicitly named by a prefix selector carrying the recipe's own Design reference are topology context. Subtracting those faces produces the ordered unreferenced candidate set. When this set is nonempty, face resolution uses it; an empty set retains the broad candidates because the selected historical face has no active unreferenced candidate. A face operand resolves to the sole effective candidate whose stable face slot is present in the scope's preceding ASM topology. If several candidates are present, it resolves to the sole candidate whose slot is deleted or assigned a different record revision by the scope's ASM transition. Inserted slots are result geometry and do not select an input face. A selected-face start with several remaining candidates resolves to the sole planar candidate coincident with the selected Sketch plane within the document linear and angular tolerances. A start or termination face group resolves when every ordered member resolves. The resolved selection is the member-ordered, deduplicated face list and retains the native group identity.

Every `Extrude` scope has exactly one sketch-profile operand in its ordered reference table. Its primary indexed header has record index `N` and is followed by ten zero bytes, `01 + u32 N+3`, six zero bytes, `u32 1`, the LP-UTF16 asset UUID, and an LP-UTF16 decimal Sketch entity suffix. A 94-byte tail ends at the same-index paired header. The suffix resolves to a Sketch-typed Design entity in the same stream and therefore to that sketch's placement and relation graph.

Within an `Extrude` scope, `AlongDistance` is the signed first-side extent. A positive value follows the selected sketch placement's normal and a negative value reverses that normal. `AgainstDistance` supplies the opposite-side extent; its magnitude is the second-side length. `TaperAngle` is the first-side draft angle and `Side2TaperAngle` is the opposite-side draft angle. Extent scalars use the parameter record's centimetre storage and draft scalars use its radian storage.

Every `Extrude` scope also names exactly one counted selection group in its ordered reference table. The group's primary indexed header has record index `N` and is followed by ten zero bytes, `01 + u32 scope_record_index`, six zero bytes, `u32 member_count`, and `member_count` entries of `01 + u32 member_record_index + six zero bytes`. The member run is followed by a nonzero u32, a finite f64, a second copy of the u32, `01 + u32 N+2 + six zero bytes`, `01 + u8 variant + u8 0`, `01 + u32 N+1 + seven zero bytes`, and `01 + u32 scope_record_index + six zero bytes`. `variant` is zero or one. The same-index paired header begins at primary-record offset `89 + 11 * member_count`.

Every `Extrude`, `Fillet`, and `Chamfer` scope names one or more counted construction-operand groups in its ordered reference table. A group's primary indexed header has record index `N` and is followed by ten zero bytes, `u32 member_count`, and `member_count` entries of `01 + u32 member_record_index + six zero bytes`. Every member is also present in the owning scope's ordered reference table. `Fillet` and `Chamfer` group members are edge-operand records owned by the same scope. Within an `Extrude` scope, u64 role `0x0000000800000000` identifies existing-body operands, `0x0000004100000000` identifies the sketch-profile operand, and `0x0000001100000000` identifies face operands. The profile group contains exactly the scope's sketch-profile record. Face groups retain scope-reference order: a selected-face start contributes the first face group, and a one-sided to-face termination contributes the following face group. Absence of an existing-body group means the operation has no existing-body Boolean operand. The member run is followed by two zero bytes, `u32 1`, `01 + u32 identity_record_index + six zero bytes`, the u64 role, ten zero bytes, a nonzero u32, a finite f64, a second copy of the u32, `01 + u32 N+2 + six zero bytes`, `01 + u8 variant + u8 0`, `01 + u32 N+1 + seven zero bytes`, and `01 + u32 scope_record_index + six zero bytes`. `variant` is zero or one. The same-index paired header begins at primary-record offset `113 + 11 * member_count`.

Within a `Fillet` scope, construction-operand groups in scope-reference order pair one-to-one with `Radius` parameters in increasing owner-local order. `TangencyWeight` parameters, when present, pair with the same groups in increasing owner-local order. Each radius therefore applies to the ordered edge operands carried by its paired group.

Within a `Chamfer` scope, construction-operand groups in scope-reference order pair one-to-one with each dimensional parameter lane in increasing owner-local order. An equal-distance chamfer has one `Distance` parameter per group. A two-distance chamfer has one `Distance 1` and one `Distance 2` parameter per group. A distance-angle chamfer has one `Distance` and one `Angle` parameter per group. Each independently dimensioned specification applies to the ordered edge operands carried by its paired group.

The construction-operand group's `identity_record_index` starts a nested identity chain. Every wrapper is an indexed header followed by ten zero bytes and `01 01 00`; the next indexed header begins exactly 24 bytes after the wrapper header. Wrappers repeat while physically adjacent records carry the same wrapper grammar, and chains may share wrapper suffixes. Physical adjacency to the first non-wrapper record does not by itself assign that record to the operand. When the following record is a fixed persistent identity, it is exactly 190 bytes from its indexed header to the next indexed header. Its header is followed by ten zero bytes, a u64 local identity, an LP-UTF16 asset UUID, an LP-UTF16 context UUID, `u32 2`, and five zero bytes. For an `Extrude`, the asset UUID equals the owning scope's sketch-profile asset UUID.

Each selection member is a 190-byte indexed record ending immediately before the next indexed header. Its eleven-byte header is followed by ten zero bytes, a local persistent identity, the LP-UTF16 asset UUID, an LP-UTF16 context UUID, `u32 2`, and five zero bytes. Every member's asset UUID equals the scope's sketch-profile asset UUID, and all members in one group carry the same context UUID. Selection-group member order is the encoded run order. A construction-operand identity chain belongs to the member when its following indexed header is the member header and its complete local, asset, and context identity equals the member identity; multiple construction groups may share one member through separate chains. The local identity names either the equal stable ASM entity slot or a record revision that the containing state's complete entity-version map assigns to one stable slot. A value valid in both namespaces resolves only when both name the same stable slot. The normalized stable slot, its topology or carrier family, and the ordered containing state identities form the historical selection identity. Historical topology retains each vertex-to-point binding and each point carrier's model-space position. A loop supplies its coedges, a pcurve supplies every coedge using that carrier, a coedge supplies its edge, an edge supplies its two vertices, a curve supplies every edge using that carrier, a vertex supplies itself, and a point supplies its stored position. When the local identity uniquely equals a persistent point id or a primary or nonzero secondary persistent curve id owned by the selected Sketch, it selects that point or curve.

An Extrude selection resolves to neutral sketch-loop indices when every ordered member resolves to a curve and each curve occurs in exactly one neutral profile loop. Repeated members of one loop produce one loop index in first-member order. Otherwise a member in the historical loop, coedge, edge, vertex, point, curve, or pcurve family supplies its historical vertex positions. A historical state contributes positions only when the complete entity-to-edge-to-vertex-to-point chain required by that family resolves; at least one containing state must contribute. Each position is orthogonally projected into the selected Sketch frame. The collective positions select one atomic closed-line region when they lie strictly inside the same smallest containing loop. If collective positions do not select one region, every member must individually select one region; the result is their ordered deduplicated union. Each region records its exterior-loop index and immediate child-loop indices. Model-space displacement normal to the Sketch plane does not affect the result. When member identities do not resolve, the Extrude scope's current and previous history-state identities select their unique linked transition. Every inserted face with a complete face-loop-coedge-edge-vertex-point boundary projects its point positions into the Sketch frame. A face contributes only when its positions select one atomic region, and the transition resolves only when all contributing faces select the same region. An incomplete member run, a member without positions, an incomplete inserted-face boundary, conflicting inserted faces, a boundary point, an unsupported profile geometry family, or intersecting candidate loops retains the group as a native selection within the resolved Sketch. Multiple selection groups retain their ordered native selection identities within the resolved Sketch. A scope without a selection group consumes the whole resolved Sketch.

Each parameter-owning Sketch scope references exactly one paired placement record. The compact placement frame is 201 bytes from its primary indexed header to its same-index paired header and denotes the identity local-to-model transform. The explicit placement frame is 329 bytes and inserts a row-major 4×4 f64 local-to-model matrix at primary-record offset 55. Its bottom row is `(0, 0, 0, 1)` and its three basis columns are orthonormal. Matrix column 0 is the sketch u-axis, column 1 is the sketch v-axis, column 2 is the sketch normal, and column 3 is the model-space origin.

A sketch relation is variable-width. It stores a counted member-reference run, zero or more auxiliary references, the owning sketch reference, a u32 constraint mask, and a counted return-reference run. References use `0x01 + u32 record_index` with zero padding; direct u32 role fields may occur between references. The record ends after the return-reference run and its zero padding, at the next indexed-record header. A zero mask denotes the default coincident relation. Constraint bits are `0x1` coincident, `0x2` colinear, `0x4` concentric or operand-discriminated symmetry, `0x8` equal length, `0x10` parallel or operand-discriminated midpoint, `0x20` perpendicular or operand-discriminated offset, `0x40` horizontal, `0x80` vertical, `0x100` tangent, `0x200` curvature, `0x400` symmetry, `0x800` equal, `0x1000` midpoint, `0x2000` polygon, `0x10000000` circular pattern, and `0x20000000` rectangular pattern. A single-bit `0x200` relation with two resolved members transfers as equal tangent-direction and curvature continuity at contact. A `0x4` relation between two circular or elliptical entities is concentric. A three-member `0x4` or `0x400` relation is symmetry when exactly one line member is a reflection axis that maps the other two point or line entities onto each other; the unique geometric relation assigns the axis independently of member order. A `0x10` relation with two line members is parallel. A `0x10` or `0x1000` relation with one bounded line and one point whose coordinates equal the arithmetic mean of the line's stored endpoints constrains that point to the line midpoint; member order does not change the roles.

A two-line `0x20` relation is perpendicular. An aggregate `0x20` offset relation has an even return-member run of at least four curve records. Consecutive return members form ordered `(source, result)` pairs. Each source curve has a null secondary persistent identity and each result curve has a nonzero secondary identity. Every pair resolves to parallel bounded lines, and every result lies at the same nonzero signed distance from its source. The sign is measured along the source line's stored left normal. The relation constrains the complete ordered pair set at that shared signed offset.

Each member and return-member index resolves through the indexed Design record graph. A sketch-point record contributes its persistent point identity, and a sketch-curve record contributes its primary and nullable secondary persistent curve identities. Other indices retain their generic indexed-record identity. Resolution preserves the encoded run order and cardinality. Persistent point identities and persistent curve-identity pairs are unique within one Design `BulkStream`. Neutral sketch-entity identities combine the byte-length-prefixed stream name, a point-or-curve discriminator, and the corresponding persistent identity values; indexed-record position does not participate.

A relation whose sole constraint bit is coincident and whose complete member run resolves to typed sketch points or curves constrains those entity loci to coincide. A point and curve pair is a point-on-curve constraint. The encoded member run supplies the neutral coincident-locus order without an additional endpoint selector.

The owning sketch reference is the numeric suffix of the owning sketch container's full Design entity id. It resolves through the sketch entity-header table. The reference applies to every typed sketch-point and sketch-curve record named by the relation's member and return-member runs. All relations using one typed geometry record carry the same owning sketch reference. Conflicting owner references are invalid.

A sketch-point record contains one typed property named `pt_tag`: `u32 property_count=1`, LP-ASCII `pt_tag`, LP-ASCII `IntrinsicMetaTypeuint64`, and the persistent u64 point id. The record then stores `0x01`, a u32 paired record reference, a 14-byte flag area whose bytes are each `0x00` or `0x01`, and two f64 sketch coordinates in centimetres at record offsets 89 and 97. The alternate form sets `property_count=2` and prefixes `pt_tag` with an `EntityGenesis` `IntrinsicMetaTypeuint64` property whose u64 value is the persistent genesis identity; all subsequent fields shift by 52 bytes.

A sketch-curve record contains two typed properties in order: `crv_primary_id` and `crv_secondary_id`, both `IntrinsicMetaTypeuint64`. The primary id is the curve's persistent identity; zero in the secondary slot is null. The alternate form sets `property_count=3` and prefixes these properties with an `EntityGenesis` `IntrinsicMetaTypeuint64` property whose u64 value is the persistent genesis identity, shifting the curve identity and geometry fields by 52 bytes. The analytic payload following the identity properties is twelve f64 values. A line stores `(start point xyz, displacement xyz, unit direction xyz, unit sketch normal xyz)`. A circular arc stores `(center xyz, unit normal xyz, in-plane unit reference direction xyz, radius, start angle, end angle)`. Points, displacements, and radii are in centimetres; angles are radians. A referenced analytic wrapper prefixes this payload with `0x01 + u32 record_ref + six zero bytes`.

A sketch NURBS payload begins with either an eight-byte all-`0xff` null sentinel or a non-null u64 carrier reference, then a nested dynamic-class record header, the degree marker, f64 fit tolerance, and three arrays. Each array header is `(u32 count, u32 duplicate_count, u32 scalar_width=8)`. The arrays are the nondecreasing f64 knot vector, positive f64 weights, and xyz f64 control points. A non-rational curve stores a zero-length weight array; otherwise weight and control-point counts are equal. In both forms, `knot_count = control_point_count + degree + 1`. Fit tolerance and control points are in centimetres.

The ACT BulkStream begins with records whose headers contain a per-file dynamic three-digit ASCII class tag and a u32 record index. `ACTTable` entries are `0x01`, u32 index, six zero bytes, and a UTF-16LE entity ID. The entries are followed by an independent ordered pool of UTF-16LE GUID records; pool position does not assign one GUID to each table entry. Per-entity channel-group records store named channel/GUID pairs followed by the entity ID. Their GUIDs are change-version handles, not visibility or suppression flags.

The ACT root-component link follows its class tag and record index with ten zero bytes, `0x01 + u32 instance_root_index + six zero bytes`, the UTF-16LE root entity ID, `0x01 + u32 3 + five zero bytes`, `0x01 + u32 registry_flag`, the UTF-16LE document display name, one or more zero bytes, and `0x01 + u32 components_root_index`. `registry_flag` is 0 or 1.

On a body, `generic_tag_attrib_def` supplies a design/construction ID distinct from the material-assignment suffix. This ID keys the design BulkStream body construction-recipe records. A body can have no body-keyed recipe.

#### Edge-recipe incidence matching

For each selector, an edge is an incidence match when, for every present clause entry, its incident loops contain both encoded boundary-count and coedge-ordinal incidences. Each selector retains all matching changed historical edge slots in stable slot order and separately retains the edge slot when that set is a singleton. One selector's incidence set does not resolve the operand independently; resolution uses the complete selector/reference intersection.

### 8.2 Materials

Visual and physical materials are distinct serialized channels.

Color attribute records include `rgb_color-st-attrib` (float r,g,b in 0..1), `truecolor-adesk-attrib` (packed ARGB integer), `color-adesk-attrib` (palette index), and `material-adesk-attrib` (library lookup pair). `Timestamp_attrib_def` carries an f64 Unix-epoch timestamp in microseconds for the original feature or body creation time. The ASM header `save_date` stores the file save-time string.

`.protein` assets are **nested ZIP archives** carrying per-asset `AssetData/*.bin` value streams plus XML schemas (`CommonSchema`, `GenericSchema`, `PhysMatSchema`, `PrismOpaqueSchema`, …). `InstanceProperties.bin` and `DefinitionIteratorProperties.bin` have a 16-byte prefix followed by 136-byte pages. Each page is a record-start page, continuation page, or `0xffffffff` terminal page with a u16 used length. A logical record is the concatenation of its start-page and continuation-page payloads.

A design BulkStream material assignment targets the nearest preceding component-prefixed entity ID. Its physical-material token joins to the `.protein` `PhysMatSchema` asset. Its visual appearance GUID is the GUID immediately before the fixed visual-appearance marker GUID. A physical-material default-appearance clause stores associated GenericSchema and Prism appearance asset references.

A per-face appearance assignment ends with the visual-appearance marker GUID `BA5EE55E-9982-449B-9D66-9F036540E140`. The two length-prefixed UTF-16LE strings before the marker are the 36-character face GUID and the visual GUID. The face GUID also appears as the string payload of the owning face's `NEUTRON_Material_attrib_def` `ATTRIB_CUSTOM` attribute in the B-rep stream. **Face appearance join:** face `NEUTRON_Material_attrib_def` GUID → design BulkStream face assignment → visual GUID → appearance asset. A face assignment overrides the owning body's appearance for that face.

A `PhysMatSchema` value block contains a count followed by 36-character GUID references to its constituent aspect assets. The physical-material join is `BulkStream` `PrismMaterial` token → `PhysMatSchema` asset → referenced Structural, Thermal, and Prism aspect assets.

**Design-entity join backbone:** body identity resolves across five tables via the numeric design-entity namespace:

```
ASM body.flags (asm_body_key)
  ↔ design BulkStream BREP body map (asm_body_key → entity_suffix)
  ↔ material-assignment record entity-id suffix ("0_985" → 985)
  ↔ metastream Body object_id
  ↔ ACT fusion_entity_id
```

The material-bearing bodies are the ACT PhysicalMaterial-channel entities minus the document/component roots.

The `id_count` field after a MetaStream type name is a count, not a flag with fixed `id1`/`id2` slots. BulkStream design body IDs use the numeric design-entity namespace and do not index the ACIS RecordTable.

Material records store a visual preset (`Prism-###`), visual GUID, protein phrase, physical-material token and category, and shader parameters. They do not store Autodesk material-library display names.

---
