# Siemens NX `.prt` (SPLMSSTR + Parasolid): Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

## 1. Format overview

`.prt` is the native part format of Siemens NX. NX uses the **SPLMSSTR** (Siemens PLM Software Master Storage) hierarchical name-to-stream container. Geometry uses zlib-compressed Parasolid neutral-binary streams.

**Part and assembly storage.** A part stores geometry as Parasolid partition and deltas stream pairs. An assembly stores child part names and paths in `EXTREFSTREAM`.

**Byte order and units (global):**

- SPLMSSTR and UG_PART table fields are **little-endian**.
- Parasolid neutral-binary payload fields are **big-endian**.
- Parasolid geometric doubles are in **meters**; model geometry is conventionally millimeters (×1000).
- The Parasolid null reference value is `1`.
- Parasolid xmt indices are **stream-scoped**; the cross-stream merge key is `(stream_index, node_type, xmt_index)`.

---

## 2. SPLMSSTR container

```text
0x00..0x07   ASCII "SPLMSSTR"
0x08         version tag, constant 0x06
0x09..0x0b   file-specific uint24 LE (correlates with file complexity, not footer offset)
0x0c..0x0f   constant 0x00000000
0x10         constant 0x00
0x11..0x16   FOOTER offset, 48-bit LE (points into the FOOTER region near EOF)
0x19..0x1e   ASCII "HEADER"
0x1f..       variable-length directory entries
```

Directory entry grammar (HEADER and FOOTER identical): `name_len:u32 LE` + ASCII path (`/Root/...`) + payload. File entries carry `file_offset:u64 LE, size:u64 LE`; directory/non-file entries carry 16 opaque bytes.

FOOTER region at the 48-bit offset: ASCII `FOOTER`, then `entry_count:u32 LE`, then directory entries, then a 4-byte per-save fingerprint (unique per file version). The `/Root/` sentinel node carries UUID `611ec9b3-fa60-d111-8ad9-0800362fb302` across files.

`/Root/part/arrangements` is a UTF-8 XML document with an `Arrangements` root. Each `Arrangement` child has a nonempty `Name` and a `Default` value of `YES` or `NO`. At most one child is default. Child order is configuration order.

The canonical `/Root/UG_PART/UG_PART` payload begins with a segment index of
12-byte little-endian rows:

```text
type_code:u32  subtype_code:u32  value:u32
```

Row ordinal 1 has `type_code = 1`, `subtype_code = 1`, and a `value` equal to
the payload-relative byte offset immediately after the index. Complete rows
occupy the declared region from offset zero; zero to eleven trailing bytes fill
the remainder when that offset is not divisible by twelve. Row order and all
three words are significant.

A segment-index word can point to a compressed-stream wrapper. Its first
`u32 LE` is `kind | extension_length`, where `extension_length` is the low 30
bits. Kind `0x80000000` places the zlib header at
`8 + extension_length`; kind `0xc0000000` places it at
`33 + extension_length`. The extension may contain a Parasolid text header.
The pointed stream is valid only when that exact computed position begins a
complete zlib payload accepted by the stream grammar. The containing row
ordinal and word position preserve the wrapper's segment order.

A partition or plain cached-body wrapper word begins a five-word segment tuple.
The following word is zero, the next two words are object-index aliases naming
the same body image, and the final word is the stream role. Either body alias
may occur in feature-history primary-body and Boolean operand fields. The tuple
can cross a 12-byte row boundary. The body-image binding is valid only when the
wrapper word resolves to the exact compressed stream position and both aliases
are non-zero.

A deltas stream applies to the nearest preceding partition stream in segment
order with the same Parasolid schema token. Non-history compressed streams do
not break this relation. A later partition begins a distinct body-history unit;
a deltas stream does not cross an intervening equal-schema partition.

A segment-index word can also point directly to an OM section signature, or to
`c0 d1 f1 ed` followed immediately by that signature. The latter form has a
four-byte separator. The row ordinal and word position order the pointed OM
section relative to the compressed stream wrappers in the same segment index.

Linked OM registries define their schema role by exact declarations:
`UGS::Solid::Topol` marks the model store, `UGS::FEATURE_RECORD` marks feature
history, `UGS::EXP_expression` marks expressions, and
`UGS::OM::SaveAuditTrail` marks audit data when no preceding specialized marker
applies.

A size-framed OM section's schema trailer can contain a little-endian
section-relative record-area offset. The target begins with three `u32 LE`
control words followed by `04|05 01 text_length:u8 "NX " product_text 00`.
The pointed record area extends to the size-framed section boundary.
Within a feature-history record area, an operation header is encoded as the
marker `80 cd 01 04 01 2f a4 7a e1 47 ae 14 7b ff ff`, four object-index
slots, then `03 length:u8 name 00`. An index below 128 is one byte. Values
through 4095 use `80..8f low:u8` and decode as `(prefix - 80) * 256 + low`.
Larger values use `90 value:u16 BE`; `ff` is null. `name` contains printable
ASCII bytes and `length = name_length + 2`.
Each non-null header slot addresses the zero-based entity-record ordinal in the
offset-only OM store. The addressed record retains its external index boundary
as the operation's ordered input block. A slot binds only when exactly one
offset-only store contains that ordinal.
Input bindings from two or more distinct operation headers form an identity
group when they resolve to the same bounded data block. Group members retain
their input-binding identity, operation-label identity, header slot, and
object-index token offset in ascending token-offset order. Repeated slots from
only one operation do not form a group. The group assigns no direction or
semantic role between its operations.
All resolved bindings from one operation to one exact numeric expression form
one parameter-use relation. Binding identities and source offsets remain in
ascending source-offset order. Multiple input slots may witness the same use;
they do not create multiple operation-expression relations.
The fixed marker begins an operation record. A record extends through the byte
before the next validated operation marker; the final record extends through
the feature-history record-area boundary.

`UNITE`, `SUBTRACT`, and `INTERSECT` labels are followed by the fixed Boolean
header `31 00 00 01 00 14 2f a4 7a e1 47 ae 14 7b 03 00 00 e0 7f ff ff ff 01 01`,
then a target list and a tool list separated and terminated by `00`. Each list
is encoded as `01 count:u8 refs`, contains `count - 1` object indices using the
operation-header index encoding, and contains no null indices. The target list
contains exactly one reference. The tool list contains at least one reference
and preserves tool order.

A body-affecting operation record contains exactly one primary-body field
`01 02 10 body_object_index ff`. The object index uses the operation-header
encoding. Operations sharing the index form one ordered body lineage. An
operation depends on the preceding operation in its primary-body lineage. A
Boolean additionally depends on the preceding operation in each tool-body
lineage, preserving tool order and omitting duplicate dependencies. When the
primary body object has a segment body-image binding, every surviving neutral
body from that image is an output of the operation. An unbound primary body
retains its object index but has no neutral output.

An operation label equal to `SKETCH` denotes a planar sketch history node. Its
position in the operation sequence is the sketch's history position. The
sketch record consists of that label, the operation record beginning at the
same header, and its uniquely resolved non-null input blocks in header-slot
order. A missing operation boundary prevents formation of the sketch record;
an unresolved input slot remains absent without reordering the other slots.

### 2.1 Stream inventory

| Stream                       | Role                                                                           |
| ---------------------------- | ------------------------------------------------------------------------------ |
| `/Root/UG_PART/UG_PART`      | canonical part payload: OM sections + Parasolid partition/deltas/plain streams |
| `/Root/FastLoad/RMFastLoad`  | fast-load object-id table → active-body membership (NX OM per-class form)      |
| `/Root/FastLoad/JT`          | preview/JT mesh and metadata                                                   |
| `/Root/*/ExternalReferences` | `EXTREFSTREAM`; child-part names, filesystem paths, occurrence handles         |
| `/Root/part/attrs`           | `<UgAttributes>` UTF-8 XML key/value part metadata                             |
| `/Root/qafmetadata`          | UTF-8 XML preview-folder metadata                                              |
| `/Root/part/arrangements`    | (assemblies) UTF-8 XML arrangement config                                      |

`part/attrs` has an `UgAttributes` root. Each `Attribute` supplies `owner`,
`pdmBased`, `title`/`utf8title`, `value`/`utf8value`, `version`, and an XML schema
type. UTF-8 title and value fields take precedence over their compatibility
duplicates. JT and LWPA payloads are preview meshes.

`EXTREFSTREAM` contains `EXTREFSTREAM` magic, `version:u32 LE (3)`, `payload_size:u32 LE`, a record region, and a trailing string table: `01` + `count:u32 LE` + `count × (len:u16 LE + control-free UTF-8)`. The string table contains child `.prt` names and paths.

Assembly `.prt` files contain no inline Parasolid partition, deltas, or plain cached-body streams. Their component geometry resides in the external child `.prt` files named by `EXTREFSTREAM`. Occurrence placement binds each external component instance.

---

## 3. Parasolid stream extraction

Text-wrapped envelope:

```text
**PARASOLID ... **END_OF_HEADER <zlib payload>
```

The partition zlib stream is preceded by `c0 d1 f1 ed`. Small zlib streams use repeating `<u32 BE count> 0x02000002` marker pairs. The wrapper-header counts are segment or record counts.

Inflated prologue text classifies each stream:

| Prologue bytes                                      | Stream kind         |
| --------------------------------------------------- | ------------------- |
| contains `(partition)`                              | partition           |
| contains `(deltas)`                                 | deltas              |
| contains `TRANSMIT FILE created by` without subtype | plain (cached body) |
| otherwise                                           | stream              |

### 3.1 Neutral-binary encoding

Inflated streams begin `PS 00 00`; the prologue contains a schema token `SCH_<version>` (for example, `SCH_3501171_35102_13006`). The third component (`13006`) is an NX-embedding constant.

XMT index encoding:

| Form        | Encoding                                                                                         |
| ----------- | ------------------------------------------------------------------------------------------------ |
| Small index | `uint16` BE, 2 bytes                                                                             |
| Large index | negative `int16` remainder + `uint16` quotient, 4 bytes; `raw = quotient*32767 + abs(remainder)` |

**Record shift rules.** At logical offset `+2`, `0xff` can encode an envelope escape or begin a large-index xmt with a remainder beginning `ff`. Any xmt pointer slot can consume four bytes instead of two and shifts later fixed fields in the record. Effective record length is `fixed_length + escape_shift + record_start_large_index_shift`. Pointer-field large-index shifts change field positions without changing the record start length, except in families with a compact tail.

### 3.2 Schema self-description

The neutral-binary streams are partially self-describing. After `SCH_` the head carries a field dictionary for the stream-root wrapper class (the `00 ce` record). Node types absent from the base schema carry an inline class definition at first use:

```text
<type:u16 BE> <sig_len:u8> <signature> <name_len:u8> <name>
```

Signature alphabet: `C` = component/pointer (xmt ref), `I` = int, `D` = double, `A` = array ref, `Z` = terminator/compound. Inline definitions include type 38 `intersection_data` (`CCCCCCCCCCCA`), type 80 `legal_owners` (`CCCCCDI`), and type 100 `precision` (`CCCCCCCCCA`).

The wrapper `00 ce` instance owns the stream BODY (`child`), attribute-definition list (`attdef_list`), preview-mesh references (`mesh`/`polyline`/`lattice`), and index-map arrays (`index_map`, `node_id_index_map`, `schema_embedding_map`).

### 3.3 NX object-model framing

An indexed object-model section carries an entity-boundary array followed by an object count and object-ID array. Boundary slot zero is zero. Subsequent values are monotonic offsets relative to the section base. Object IDs in slots `1..count` pair with entity spans bounded by adjacent boundary values. The first entity begins with `04 01 0e "NX "`.

An offset-only object-model store instead carries an absolute boundary array,
then a record count. Boundary slot zero bounds the store root/control block;
slots `1..count+1` bound column-storage blocks. These blocks have no individual
object identity. A block may split a string, fixed array, or field lane across
adjacent boundaries, so marker-shaped bytes inside one block do not define an
entity string or reference. Concatenating the column-storage blocks in boundary
order reconstructs the exact logical storage region; block boundaries add no
separator or padding.

Each indexed store contains one self-framed product/version header:
`04 01 text_length:u8 "NX " version_text 00`. `text_length` equals the
printable text length plus two. Store metadata may precede this
header inside its bounded control or first data block.

Class definitions before the boundary array use `declared_length:u8 + "UGS::" name bytes + trailing_code:u8`, where `declared_length` includes the trailing code. Bytes between the trailing code and the next class declaration form that declaration's registry suffix; an empty suffix is valid. An 11–14-byte suffix consists of a 2–5-byte layout prefix, an eight-byte schema fingerprint, and one terminal layout byte. Member definitions in the same indexed schema use the same framing with an `m_` name. Declaration order supplies section-local class and member identity.

Class and member declaration ordinals are local to one OM section. The containing
section base plus the declaration ordinal forms their identity; equal ordinals in
distinct sections do not identify the same class or member. Entity-record
ordinals are likewise local to the indexed section whose base governs the
external boundary array.

A compact-index lane is a concatenation of entries. Bytes `00..7f` encode their
unsigned value directly. A byte in `80..fe` followed by `low:u8` encodes
`(prefix - 0x80) * 256 + low`. Byte `ff` encodes a null entry and consumes no
following byte. A two-byte prefix without its low byte does not form a complete
lane.

A numeric expression table contains a `hostglobalvariables` root entity. Each expression entity contains:

```text
<handle:u8> 04 text_length:u8
"(Number [" unit "]) " name ": " expression "; "
00
```

`text_length` includes the leading marker byte and trailing zero, so it equals the ASCII text length plus two. Defined units are `mm` and `degrees`. Parameter names use `p<decimal-index>` or `p<decimal-index>_<qualifier>`. The qualifier remains part of the parameter name; equal decimal indices with distinct qualifiers are distinct parameters. A context-free arithmetic expression over finite decimal scalars, parentheses, unary signs, `^`, `*`, `/`, `+`, and `-` supplies its evaluated value. Powers associate right; multiplication and division precede addition and subtraction. Formula text retains ordered exact parameter-name dependencies; repeated references denote one dependency at its first occurrence. A dependency resolves only when its exact name identifies one parameter in the same expression table. Acyclic formulas evaluate after same-unit dependencies have values. Ambiguous names, cycles, cross-unit references, unknown names, and calls remain unevaluated.

---

## 4. Record framing

### 4.1 Fixed record families

Lengths are logical, before escape/large-index shifts. Each code is a Parasolid XT node type.

| Type | Name    | Length | Type | Name          | Length     |
| ---: | ------- | -----: | ---: | ------------- | ---------- |
|   12 | BODY    |     24 |   50 | PLANE         | 91         |
|   13 | SHELL   |     24 |   51 | CYLINDER      | 99         |
|   14 | FACE    |     39 |   52 | CONE          | 115        |
|   15 | LOOP    |     16 |   53 | SPHERE        | 99         |
|   16 | EDGE    |     32 |   54 | TORUS         | 107        |
|   17 | FIN     |     23 |   56 | BLEND_SURF    | 66 + shift |
|   18 | VERTEX  |     28 |   60 | OFFSET_SURF   | 39         |
|   19 | REGION  |     16 |  124 | B_SURFACE     | 23         |
|   29 | POINT   |     40 |  133 | TRIMMED_CURVE | 85 + shift |
|   30 | LINE    |     67 |  134 | B_CURVE       | 23         |
|   31 | CIRCLE  |     99 |  137 | SP_CURVE      | 33 + shift |
|   32 | ELLIPSE |    107 |      |               |            |

Types carrying `node_id:u32` place it at record offset `+4` (after shifts). FIN has no `node_id`. EDGE candidates with denormal tolerance (`abs(tol) < 1e-100`) are payload coincidences, not records.

Type 38 is the XT `INTERSECTION` node. Delta-stream `0x5a` records use the `intersection_data` layout.

### 4.2 Deltas-stream framing

A deltas stream is a schema-framed incremental edit log paired with a partition. Both declare the same schema token. Records are not length-prefixed; they self-delimit by typed decode (valid record ends on a plausible next-record tag). Two record forms:

**Full record:**

```text
type:u16 BE
xmt:encoded_index
node_id:u32 BE                   0-based delta-stream ordinal
<type signature fields>          reference slot = encoded_xmt + status:u8
```

FIN omits `node_id` and begins its nine signature references immediately after `xmt`. The status byte is `0x01` and frames each reference. The record form carries the merge operation.

**Tombstone:** a compact 6-byte deletion `type:u16 BE  xmt:u16  00 01`. A whole-record tombstone has this complete form. In a full record, `xmt 01` is a reference and status byte. Tombstone xmts are plain high-range `u16` values (48300+).

Tombstones form descending contiguous xmt runs that can span topology, geometry, and attribute record types. Partition topology remains authoritative. A tombstone does not remove a point, curve, or surface carrier still referenced by a surviving vertex, fin, edge, or face unless a later full deltas record replaces that carrier. Unreferenced exact-key records follow the last full-record or tombstone event.

---

## 5. Topology

### 5.1 Ownership graph

```text
body → shell → [region] → face → loop → fin → edge → vertex → point
                                    ↑ face → surface, edge → curve
```

**Common header** for analytic curve/surface types 30–32, 50–54: `attributes +8`, `owner +10`, `next +12`, `previous +14`, `group +16`, `sense +18`.

Any fixed record may place an envelope escape byte `ff` between its type and xmt fields. The xmt begins one byte later and all logical payload offsets shift by one. When the first xmt byte is also `ff`, both the escaped and unescaped large-index forms are structurally possible; the complete family field grammar disambiguates them.

Topology node layouts (logical offsets, pre-shift):

| Type        | Fields                                                                                                                                                                                                                           |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| BODY (12)   | `node_id +4`; owner of shells/faces/edges/vertices                                                                                                                                                                               |
| SHELL (13)  | `node_id +4`, `attributes +8` (=1), `body_ref +10`, `next_shell +12` (=1), `first_face +14`, sentinels `+16/+18` (=1), `region_ref +20`, `face_anchor +22` (`1` or `first_face`)                                                         |
| FACE (14)   | `attributes +8`, `tolerance:f64 +10`, `next_face +18`, `prev_face +20`, `loop +22`, `shell +24`, `surface +26`, `sense +28`, `next_on_surface +29`, `prev_on_surface +31`, `next_front +33`, `prev_front +35`, `front_shell +37` |
| LOOP (15)   | `attributes +8`, `fin +10`, `face +12`, `next_loop +14`                                                                                                                                                                          |
| EDGE (16)   | `attributes +8`, `tolerance:f64 +10`, `fin +18`, `prev_edge +20`, `next_edge +22`, `curve +24`, `next_on_curve +26`, `prev_on_curve +28`, `owner +30`                                                                            |
| FIN (17)    | `attributes +4`, `loop +6`, `forward_fin +8`, `backward_fin +10`, `vertex +12`, `other_fin +14`, `edge +16`, `curve +18`, `next_at_vertex +20`, `sense +22`                                                                      |
| VERTEX (18) | `attributes +8`, `fin +10`, `prev_vertex +12`, `next_vertex +14`, `point +16`, `tolerance:f64 +18`, `owner +26`                                                                                                                  |
| POINT (29)  | `attributes +8`, `owner +10`, `next +12`, `prev +14`, `xyz:3×f64 +16` (meters)                                                                                                                                                   |
| REGION (19) | `node_id +4`; referenced by SHELL                                                                                                                                                                                                |

A **body-shape SHELL** requires the invariant fields `attributes`, `next_shell`, and `+16/+18` to equal `1`, non-null `body_ref` and `region_ref`, and a resolvable `first_face`. With null `face_anchor`, `FACE.next_face` defines a finite ownership chain whose members back-reference the SHELL. With non-null `face_anchor == first_face`, every FACE that back-references the SHELL belongs to it. The body and region references remain ownership identities when the stream omits the corresponding BODY or REGION record. FACE and EDGE `tolerance` decode as the sentinel `-3.14158e13` (`c2 bc 92 8f 99 6e 00 00`) or a finite model-scale value, giving an 8-byte alignment check. `FIN.curve` is non-null only on tolerant edges (tolerant-edge trims use TRIMMED_CURVE→SP_CURVE).

For SHELL, FACE, LOOP, FIN, EDGE, and VERTEX, a non-null `attributes`
reference identifies the stream-local attribute list owned by that exact topology
record. The topology type and xmt together identify the owner. Attribute-list
identity does not assign a class, value, or presentation meaning until the
referenced list and its instances resolve.

### 5.2 Reference domains

- Ordinary BREP references (`FACE.surface`, `EDGE.curve`, `FIN.curve`, `VERTEX.point`, BLEND_SURF/INTERSECTION support refs) resolve within the same stream.
- SHELL ownership records may resolve in `{partition, paired_deltas}`. A SHELL's non-null BODY and REGION references remain ownership identities when either referenced record is not serialized.

### 5.3 Topology assembly

| Entity   | Rule                                                                  |
| -------- | --------------------------------------------------------------------- |
| vertices | FIN-referenced VERTEX nodes; coordinates from same-stream POINT nodes |
| edges    | one per EDGE node; native endpoint incidence is `EDGE.fin → FIN.vertex` and `FIN.other_fin → FIN.vertex`, with null `other_fin` falling back to `FIN.forward_fin → FIN.vertex`; canonical start/end order follows increasing curve parameter; the carrier resolves through non-null `EDGE.curve`, otherwise through the owning `FIN.curve` |
| loops    | walked from `FACE.loop` through the null-terminated LOOP chain; each FIN ring closes at its first FIN with reciprocal forward/backward links; non-null partner FINs reciprocally reference one another and carry the same EDGE |
| faces    | one per FACE node, with resolved surface when available               |
| bodies   | one per validated body-shape SHELL                                    |

POINT is a geometric carrier. It becomes a topological vertex only through a validated `FIN.vertex → VERTEX.point` path. An unreferenced POINT is not a free vertex of an existing body.
An EDGE belongs to the assembled B-rep only when a FIN in a fully resolved owned LOOP references it.
An unresolved carrier placeholder belongs to the transferred model only when an
emitted FACE or EDGE references it. Fixed-record scanner candidates outside the
resolved body closure do not create free unknown carriers.
An edge's two serialized trim limits are an unordered interval. Canonical start/end order follows evaluation at the ascending limits. A periodic interval is then normalized by reducing its start modulo `2π` and preserving its nonnegative sweep; a seam-crossing interval therefore ends above `2π`.
The interval binds to the referenced typed curve only when evaluating its two limits reaches the edge vertices within the edge and vertex tolerances. A failed interval binding omits the parameter range but does not replace or discard the referenced curve carrier.
For a procedural carrier without a solved evaluator, the ascending native trim
interval remains authoritative and FIN incidence supplies endpoint order. Lack
of an evaluator does not replace an exact procedural construction with an
unknown carrier.

An EDGE may carry null curve reference `1` with a finite tolerance. With a null
owning `FIN.curve`, this is a tolerant intersection edge: its carrier is the
intersection relation between the two distinct surfaces reached through its
radial FIN pair, bounded by the EDGE vertices, within the serialized edge
tolerance. Transfer represents the relation as a procedural intersection
carrier with the two face surfaces; it does not synthesize a line between the
vertices. A null EDGE and FIN curve without exactly two distinct adjacent
support surfaces remains carrierless.
A null `EDGE.curve` may instead have a non-null owning `FIN.curve`. The FIN
reference is the carrier path. When it resolves through
`TRIMMED_CURVE → SP_CURVE` whose original 3D curve is null, the SP_CURVE's
surface and pcurve define a procedural parametric surface curve. Its finite
domain is the trim interval, or the solved NURBS pcurve knot domain when the FIN
references the SP_CURVE directly.
A FIN pcurve attaches to a coedge only when evaluation through that face's
surface reaches both edge vertices within the larger of the edge, vertex, and
pcurve fit tolerances. A pcurve carried on a different support remains part of
the procedural curve construction but is not attached to that face.
A body is solid when every assembled EDGE has exactly two FIN uses in that body. A body with faces and any edge-use count other than two is a sheet body.

BODY, REGION, and SHELL records contain no placement reference. POINT coordinates and the origins and axes stored by curve and surface carriers are part-model coordinates. An inline Parasolid body's part placement is therefore the identity transform.

Body-shape SHELL validation: invariant/ref predicate passes; `body_ref` and `region_ref` are non-null; `first_face`→FACE in the SHELL's stream. A null `face_anchor` requires the `FACE.next` walk to close at null with visited faces back-referencing the SHELL. A non-null `face_anchor` equals `first_face` and selects all FACE records that back-reference the SHELL.

**Periodic faces / closed edges.** Parasolid stores a periodic surface as one face. A full-circle/ellipse edge stores no trim interval or wrap-count field and references the bare CIRCLE/ELLIPSE. Its one-FIN loop has `forward_fin == backward_fin == self`. The FIN vertex is either a VERTEX shared by both edge ends or the null reference; the null form's canonical topological point is the analytic curve point at parameter zero. The full revolution has parameter identity `[0, 2π]`. An EDGE with `curve == 1` has no curve record and is the surface-intersection locus of its incident faces.

---

## 6. Geometry carriers

All geometric doubles are meters → ×1000 for mm. Directions and axes are unit vectors (not scaled); angular parameters are radians; linear curve parameters are meters of arc length.

### 6.1 Analytic curves and surfaces

Payload offsets are relative to the record's type tag, after the common header (§5.1).

| Type          | Payload                                                                              |
| ------------- | ------------------------------------------------------------------------------------ |
| LINE (30)     | point `+19`, direction `+43`                                                         |
| CIRCLE (31)   | center `+19`, normal `+43`, x_axis `+67`, radius `+91`                               |
| ELLIPSE (32)  | center `+19`, normal `+43`, x_axis `+67`, major `+91`, minor `+99`                   |
| PLANE (50)    | origin `+19`, normal `+43`, x_axis `+67`                                             |
| CYLINDER (51) | origin `+19`, axis `+43`, radius `+67`, x_axis `+75`                                 |
| CONE (52)     | origin `+19`, axis `+43`, radius `+67`, sin_half `+75`, cos_half `+83`, x_axis `+91` |
| SPHERE (53)   | center `+19`, radius `+43`, axis `+51`, x_axis `+75`                                 |
| TORUS (54)    | center `+19`, axis `+43`, major `+67`, minor `+75`, x_axis `+83`                     |

Validity gates: CONE satisfies `sin_half² + cos_half² ≈ 1`; SPHERE has `radius > 0` and unit axis; a horn torus has `major == minor`.

**OFFSET_SURF (60):** check byte `+19` (`V`/`I`/`U`), base surface ref `+21`, `offset_distance:f64 +23` (meters). Surface `P = base(u,v) + offset_distance · unit_normal(u,v)`. There is no scale field at `+31` (that position lands in the next record). For a B_SURFACE base, the unit normal comes from the rational quotient rule:

```text
Pu = (Au·W − A·Wu)/W²,  Pv = (Av·W − A·Wv)/W²,  normal = normalize(Pu × Pv)
```

An OFFSET_SURF used by a FACE transfers as a procedural surface carrier. The carrier and offset construction reference each other; the base surface and signed millimeter offset remain in the construction. Model evaluation follows the base reference recursively, computes the normalized parameter-tangent cross product, and applies the signed offset; cyclic base graphs do not evaluate.

### 6.2 B-spline carriers (B_SURFACE 124 / B_CURVE 134)

B_SURFACE / B_CURVE are compact: header through sense `+18`, then `nurbs` ref `+19` and `data` ref `+21` (both large-index capable). The full NURBS resolves through support records:

| Type | Tag    | Role                                                                                                                                                        |
| ---: | ------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
|  125 | `007d` | B-surface control-grid payload (`double_count` near `+91`, then values)                                                                                     |
|  126 | `007e` | B-surface descriptor: `u_degree +6`, `v_degree +8`, `u_pole_count +12`, `v_pole_count +16`, forms `+18/+19`, distinct-knot counts `+20/+24`, mult/knot refs |
|  127 | `007f` | multiplicity arrays (`alloc`, ref, `alloc × u16`)                                                                                                           |
|  128 | `0080` | knot arrays (`alloc`, ref, `alloc × f64`)                                                                                                                   |
|  135 | `0087` | B-curve control payload                                                                                                                                     |
|  136 | `0088` | B-curve descriptor: `degree +4`, `pole_count +8`, `dimension +10` (2=UV, 3=XYZ), distinct-knot `+14`, form `+16`, mult/knot refs `+23/+25`                  |

Types 135 and 136 may place an `ff` envelope escape before their xmt. This
shifts every subsequent logical field by one byte. Type 135 may place a second
`ff` escape before its control-value count; the count and control-reference tail
shift by one additional byte. Multiplicity and knot references in type 136 are
sequential encoded xmts, so an extended multiplicity reference shifts the knot
reference.

Control-grid stride = `double_count / (u_pole_count · v_pole_count)`; `3` = non-rational xyz and `4` = rational xyzw. Canonical multiplicities satisfy `sum(mults) = n_poles + degree + 1` in each direction. Pole-grid ordering is u-major.

### 6.3 Procedural intersection curves (type 38 / `0x5a`)

NX stores freeform edges and blend rails as construction relations with branch witnesses. A type-38 record has a compact header through sense `+18` and six support xmt references at `+19,+21,+23,+25,+27,+29`.

The chart/start-term/end-term witness slots `ref[2:5]` are atomic: all three are null reference `1`, or all three are non-null. Mixed null and non-null witness slots do not form a type-38 or `intersection_data` construction. Type-38 common-header `attributes` is null reference `1`. Deltas type-38 records append status byte `01` to every reference; transfer removes those status bytes before applying the partition-style construction grammar.

| Ref | Role                                                                                       |
| --- | ------------------------------------------------------------------------------------------ |
| 0/1 | primary support surface + type-59 second-support bridge (order set by the `0x00cc` marker) |
| 2   | `0x28` CHART_s seed/control polyline                                                       |
| 3/4 | `0x29` term_use start / end endpoint                                                       |
| 5   | `0x00cc` values-array (support UV parameters)                                              |

For the `0x5a` delta twin the layout is fixed (primary = ref[0], bridge = ref[1]); for type-38 the primary/bridge assignment follows the `0x00cc` marker (marker-2 → primary ref[0]; marker-3 → primary ref[1]).

**CHART_s (`0x28`):** branch selector and native-parameter certificate:

```text
00 28 [ff] count:u32 BE  xmt
base_parameter:f64  base_scale:f64  chart_count:u32  chordal_error:f64  angular_error:f64
parameter_error[2]:f64   (sentinel pair -31415800000000.0)
count × Hvec              (Hvec block always starts at pre+52, pre = end of count+xmt)
```

Hvec form depends on the stream: partition streams use **`xyz3`** (`x,y,z` meters); deltas streams use **`ext11`** (`x,y,z, p3,p4,p5,p6, tx,ty,tz, t`), with a unit tangent and strictly increasing native `t`. The chart parameter is meter-scale: `t_{k+1} = t_k + chord_k · f_k`, with `t_0 = base_parameter` and chords in meters. `chordal_error` defines the verification tolerance for chart-hosted carriers. Intersection charts use `(base_parameter, base_scale) = (0.0, 1.0)`. Procedural-spine charts have `chart_count == count`, sentinel `parameter_error`, and finite non-zero `base_scale`. When `xyz3` and `ext11` records have the same xmt, count, and point sequence within the larger chordal error, the `ext11` native `t` sequence governs the shared chart carrier.

**term_use (`0x29`)** records are hard trim endpoints (`ref[3]` = start vertex point, `ref[4]` = end vertex point, meters). Each endpoint lies within the CHART_s `chordal_error` of the corresponding first or last chart point. Three record forms occur for `0x28`/`0x29`/`0x00cc`: direct tagged, `0xff`-escaped, and descriptor-inline (payload follows the ASCII schema keyword + a fixed field-schema tail).

**`0x00cc` values-array** packs support UV samples by marker byte:

| Marker | Packing | Meaning                                                       |
| -----: | ------- | ------------------------------------------------------------- |
|    2/3 | `2·n`   | `(u,v)` on support 0                                          |
|      4 | `4·n`   | `(u0,v0,u1,v1)`: first pair on support 0, second on support 1 |

The value `-31415800000000.0` is a missing-parameter sentinel. Preserve the tuple position. Support-0 `(u,v)` values evaluate on the analytic surface to the curve's 3D points.

CHART_s and its two term_use endpoints define the bounded 3D carrier independently of the values-array. A null, sentinel-bearing, or count-mismatched values-array omits the corresponding pcurve; it does not invalidate the 3D chart carrier.

When a FIN carries the intersection and its owning FACE uses one of the intersection supports, that support's UV chart is the FIN pcurve. Transfer requires both pcurve endpoints, mapped through the FACE surface, to coincide with the EDGE vertices within the stored edge, vertex, face, or chart-fit tolerance. A chart that fails this incidence relation remains construction data and is not attached to the coedge.

```text
cylinder: P = O_mm + (v·1000)·A + r_mm·(cos u · X + sin u · (A×X))
plane:    P = O_mm + (u·1000)·X + (v·1000)·(N×X)
torus:    Y = A×X;  P = C_mm + (R + r·cos v)·(cos u · X + sin u · Y) + r·sin v · A
```

**UV validation.** The first and last evaluated UV samples reproduce the term endpoints within `1e-6` mm.

**Type-59 BLEND_BOUND (`0x003b`)** contains `boundary_index` (0/1) and `blend_surface_ref` to a BLEND_SURF construction surface. The `0xff` after the tag is an envelope escape. For participating support `A`, `B.support_refs[1 - boundary_index] == A`. `B.support_refs[boundary_index]` identifies the support that closes the blend rolling-ball law at the cap.

### 6.4 TRIMMED_CURVE (133) and SP_CURVE (137)

**TRIMMED_CURVE (133):** basis_curve ref `+19` (large-index capable → shifts later fields +2), `point_1 +21`, `point_2 +45`, `parm_1:f64 +69`, `parm_2:f64 +77`. The curve is `basis(t)` restricted to `[parm_1, parm_2]`; parameters are in the basis's native units: LINE uses meters of arc length from the stored point (×1000 for mm), CIRCLE uses radians, and B_CURVE uses knot units. Unscaled meter spans on a LINE basis place the trim interval 1000× too small.

TRIMMED_CURVE and SP_CURVE references form an XMT graph, not a record-order stack. A wrapper may reference another wrapper serialized later; resolve wrapper chains to a terminal curve carrier independent of record order.

**SP_CURVE (137):** surface ref `+19`, b_curve ref `+21`, original ref `+23`, `tolerance_to_original:f64 +25` (after ref shifts). It represents a curve-on-surface: a 2D B-curve in the surface parameter space.

A B_CURVE descriptor with `dimension = 2` stores `(u,v)` control points rather than model-space coordinates. Rational payloads store homogeneous `(u·w,v·w,w)` triples. The coordinates use the supporting surface's native parameter units. Transfer to canonical IR multiplies both plane parameters by 1000 and multiplies the axial parameter of cylinders and cones by 1000; angular parameters and NURBS knot-space parameters remain unchanged. The SP_CURVE tolerance is a model-space distance in meters and transfers to millimeters.

### 6.5 Rolling-ball blend surface (BLEND_SURF 56)

A BLEND_SURF FACE is a procedural canal or envelope surface. Record layout:

```text
compact header through sense +18
subtype byte +19            (`0x52` / `R` = rolling-ball)
support refs +20,+22,+24    (large-index capable): support 0, support 1, spine
4 × f64                     values = (range[0], range[1], thumb_weight[0], thumb_weight[1])
4 × xmt tail refs           `1` (null references)
```

A BLEND_SURF used by a FACE transfers as a procedural surface carrier. The carrier and blend construction reference each other; oriented supports, spine, radius law, and cross-section remain in the construction.

`values[0:2]` are signed support offsets `range[2]` in meters. Their magnitude gives the rolling-ball radius `r = |range|`. `values[2:4]` are dimensionless `thumb_weight[2]`. Support reference 2 identifies the ball-centre spine. Spine families include:

- **Offset-intersection spine:** a type-38 whose two supports are both OFFSET_SURF, with base refs and offsets mirroring the blend's supports and `range` (`O_i = base_i + range_i · oriented_normal_i`). Freeform (NURBS-offset) bases.
- **Direct-supports spine:** a type-38 on the original analytic supports directly.
- **Fixed-curve spine:** an ELLIPSE (type 32); ellipse non-circularity encodes plane draft angle (`major/minor = 1/cos(draft)`).
- **Tool-body delta spine:** a `0x5a` INTERSECTION_DATA record with a real (non-sentinel) `geometric_owner`.

**Canal law** `B(t,s)` uses the two supports, signed range, and the spine marker-4 UV chart:

```text
B(t,s) = C(t) + r · Rot_about_T(t)( s·α(t) ) · E0(t)
  C(t)   = ball-centre spine = S0(u0,v0) + σ0·r·N0 = S1(u1,v1) + σ1·r·N1
  Q_i(t) = contact rail on support i = S_i(u_i(t), v_i(t))
  E_i(t) = (Q_i(t) − C(t)) / r        (unit; |Q_i − C| = r exactly)
  T(t)   = unit spine tangent C'(t)/|C'(t)|
  α(t)   = atan2((E0×E1)·T, E0·E1)    signed ball-arc angle, varying along the spine
  rails:  B(t,0) = Q0(t),  B(t,1) = Q1(t)
  normal: n(t,s) = (B(t,s) − C(t)) / r   (radial from ball centre; envelope-of-spheres, no differentiation)
```

`σ0, σ1 ∈ {+1,−1}` are the `range` signs, with `|range| = r`. The spine identity is `S0+σ0·r·N0 == S1+σ1·r·N1`. Rail incidence is `B(t,0)=Q0(t)`. At each rail, the canal normal equals the support surface normal.

**Chained blend-on-blend** recurses into the support blend canal. Offsetting a constant-radius canal along its normal gives a canal with radius `r+δ`: `B(t,s; r+δ) = B(t,s; r) + δ·n(t,s)`. A spine uses one branch pair `(i0,i1)` for each polyline point.

**Primitive reduction.** A constant-radius blend with a circular spine has torus parameters `major = circle radius`, `minor = r`. A line spine has cylinder radius `r`. Reduction requires `|range[0]| == |range[1]|` and a circular or linear spine with at least five points.

---

## 7. Metadata, history, and body composition

### 7.1 NX object model (OM)

UG_PART begins with a 12-byte row table of LE u32 triples pointing at OM sections and Parasolid wrapper headers. An OM section starts at signature `ff ff ff ff`, optionally preceded by `c0 d1 f1 ed`, and stores `payload_size:u32 BE` at `+8` with `section_end = signature_offset + 16 + payload_size`. Bytes `+12..+14` are `OM`. The section decomposes into preamble, type registry, field registry, object-id table, and entity records.

**Externalized record boundaries.** Every OM section with an id-table carries, immediately before its `object_id_table`, a `(count+1)`-entry monotone `u32 LE` **entity_index** with `index[0] == 0`. OM entity records have no inline length prefix; lengths live in the entity_index:

```text
oid_end = object_id_table_off + 4 + count*4       # first entity record start
base    = oid_end − entity_index[1]               # self-anchoring
record i = bytes[base + index[i], base + index[i+1])
object_id(i) = object_id_table[i]
```

The first record at `oid_end` begins `04 01, declared_len:u8, version_text[declared_len-2], 00`. `version_text` is printable ASCII beginning with `NX ` and may end in a space. A **type registry** declaration is `declared_len:u8, name[declared_len-1], trailing_code:u8`; `name` is printable ASCII beginning with `UGS::`. The zero-based declaration ordinal is the class identity. A **field registry** declaration has the same core framing with a printable name beginning `m_`. The bytes from its trailing code through the next length-framed `m_` declaration form that field's registry suffix. The final declaration has no next-declaration boundary and therefore no bounded suffix.

The primary UG_PART section uses an offset-only index. A trailing `record_count:u32 LE` follows `record_count+2` monotone offsets. Offsets are relative to the UG_PART payload start. `index[0]` starts identity metadata, `index[1]` starts the first entity, and the remaining entries bound `record_count` entities:

```text
identity_metadata = bytes[index[0], index[1])
record i = bytes[index[i+1], index[i+2])   # 0 <= i < record_count
```

The offset-only form does not assign one fixed-width object ID to every record. Entity identity remains unspecified unless a persistent handle is present in the bounded record.

A zero-prefixed offset-only store control-array form is an atomic array of four-byte words. Each word is `00, value:u24 LE`; the array is nonempty and its byte length is divisible by four. Values retain their zero-based word order and byte offsets. A nonzero prefix byte or incomplete final word means the control block uses another form and does not produce this array.

A product-terminated control-array form has zero to three leading zero bytes, followed by a nonempty aligned array of `value:u32 LE`, followed immediately by the unique self-framed `04|05 01 ... "NX " ... 00` product record in the control block. The leading-zero count aligns the value array to its own four-byte boundary. A value smaller than the same section's total control-plus-column block count addresses the block at that ordinal; other values remain unbound. Multiple product records, a nonzero alignment prefix, or a partial value invalidates the complete array.

Independently of the control-block form, complete `e0, handle:u32 BE` and four-byte high-nibble-`c` tagged-reference tokens are retained in byte order within the bounded control block. Record-ordinal tokens are not defined for offset-only control storage and are excluded.

A maximal run of exactly two adjacent persistent-handle tokens forms a control handle pair: `e0, first:u32 BE, e0, second:u32 BE`. The pair retains both reference occurrences and values. A single token or a maximal run of three or more tokens does not form a pair.

An offset-store block may carry a counted block-index lane `01, declared_count:u8, anchor, member[declared_count-2], 01 11`, with `declared_count >= 3`. The anchor and members are non-null compact indices: `00..7f` are direct, `80..fe, low:u8` decode as `(marker-80)*256+low`, and `ff` is null. Every index addresses the same offset-only store's control-plus-column block ordinal. The lane is retained only when its count is complete, its terminator is exact, and every addressed block exists. Anchor and member order remain distinct; no semantic role is assigned by the lane framing.

Contiguous offset-store column storage may carry an `ABR` reference lane `11, slot[16], 02 11 41 42 52 ff 03`. Each ordered slot is a nullable compact block index: `ff` is null and non-null values use the direct and extended forms. Every non-null value addresses the same offset-only store's control-plus-column block ordinal. The lane is retained only when all sixteen slots and the complete literal terminator are present and every non-null target exists. Physical data-block boundaries do not constrain the lane.

An offset-store object frame is `object_id:compact_index, 00 72 01 c0 20 02 01 c0 45 04 00 80 86 02 01 02 80 a4`. The compact index is non-null and uses the same direct and extended forms. Its value is a persistent object ID. The frame and discriminator lie within one bounded data block; non-overlapping frame order and the compact-index byte offset are retained.

A zero-prefixed offset-store control block begins with an ordered class-selection lane. Each word is `00, class_ordinal:u24 LE`; every ordinal indexes the store-local class registry and occurs once. The lane ends at the first out-of-range word, and every remaining control word is out of range. An empty lane, duplicate ordinal, or later in-range word rejects the class-selection lane atomically. Each retained ordinal resolves to its exact registered class definition and name.

A printable OM string value is framed as `66 32 03, declared_len:u8, text[declared_len-2], 00`. The text is non-empty printable ASCII. The marker, declared length, text, and null terminator lie within one externally bounded record.

A feature-history operation record begins at the fixed operation-header marker and ends at the next validated operation header or the record-area boundary. Its label is `03, declared_len:u8, printable_name[declared_len-2], 00`. The operation payload begins immediately after that null terminator and extends through the operation-record boundary. Payload strings use `04, declared_len:u8, utf8_text[declared_len-2], 00`; the text is non-empty valid UTF-8 and contains no control characters.

A `SKETCH` operation carries one ordered counted-reference field beginning `01 00, nonempty:u8`. When `nonempty` is one, `declared_count:u8` follows and is nonzero, followed by `declared_count - 1` contiguous indices. When `nonempty` is zero, the declared count is zero and no leading indices follow. The field then contains `00 00`, one terminal index, and `01 00 00 00`. Each index uses a canonical width marker: `f0, value:u8` represents `0..255`, while `f1, value:u16 BE` represents `256..65535`. The indices address offset-only OM data blocks; resolution is retained only when one indexed store contains the addressed block.

A complete sketch construction-input record requires one joined sketch record, a consistent declared count, contiguous reference ordinals, exactly `max(declared_count-1, 0)` leading member references, one final terminal reference, and unique data-block resolution for every reference. It retains the leading member lane and separated terminal reference as distinct ordered fields. Any missing, inconsistent, noncontiguous, multiply terminal, or unresolved field is rejected atomically.

The logical sketch construction payload is the bytewise concatenation of the resolved leading member blocks followed by the resolved terminal block. Block boundaries do not delimit values or named-record boundaries. The payload retains its exact concatenated byte length and hash, ordered source-block identities, each block's payload offset and byte length, and each block's absolute source offset.

A sketch payload scalar field is `50 59 66, field_code:u8, 00, shifted_f64`. The shifted binary64 uses the extrusion shifted-IEEE transform. Each complete finite field retains its discriminator, decoded value, payload-relative marker offset, and absolute source offset. The field frame does not assign a geometric or constraint role to the value.

A sketch payload name field is `66, compact_type, 03, declared_len:u8, text[declared_len-2], 00`. The compact type is non-null. At reconstructed payload offset zero, the type-free form is `03, declared_len:u8, text[declared_len-2], 00`; it has no compact type. In both forms text is nonempty printable ASCII. A complete name field opens a named payload interval ending exclusively at the next complete name field or the reconstructed payload boundary. Framed scalar fields within that interval are retained in payload order. Bytes preceding the first complete name field remain outside named intervals.

A named payload interval whose name is exactly `Point` followed by a positive decimal ordinal is a sketch point when the interval contains exactly two framed scalar fields. The scalar order is the point's native two-dimensional coordinate order. The coordinate unit and model-space frame are not assigned by this record. A zero ordinal, nondecimal suffix, missing scalar, or additional scalar rejects the typed point atomically.

An offset-store named point object begins at a bounded data block whose offset zero carries the type-free `Point<positive decimal>` name frame. Its extent is the minimal consecutive-block span containing exactly two complete framed scalars and no second complete name. Zero or one scalar extends the span; a second name or a third scalar rejects the object. The object retains every block identity in the span, scalar order and values, and exact source offsets. The record assigns no sketch ownership, coordinate unit, or model-space frame.

A sketch named-point block use exists when one resolved reference in the sketch's counted field addresses a block in a typed named-point span. It retains the sketch reference and ordinal, named-point identity, shared block, and block position within the point span. The relation assigns no ownership when the reference field does not address the point span.

An `EXTRUDE` operation carries an ordered profile-reference field `01 02 16 01, count:u8, reference[count-1], 01 03 79`, with `count >= 2`. The payload may repeat the identical ordered encoded references as `01, count, reference[count-1], 00 00`; an exact unique repetition is retained as an independent witness of the list. Profile indices use the same canonical `f0` and `f1` widths and resolve against offset-only OM data blocks under the same uniqueness rule.

The extrusion payload begins `0f 00 00 01 00` followed by two shifted-IEEE scalars. A shifted-IEEE scalar occupies eight bytes: adding `0x10` to its first byte and retaining the following seven bytes verbatim produces one big-endian IEEE-754 binary64 value. Overflow of the first-byte addition and non-finite reconstructed values invalidate the scalar header atomically.

The three-scalar extrusion branch places `11` and three self-delimiting scalar atoms after its unique body-reference field. `00` is exact zero. Markers `20..3f` and `a0..bf` begin eight-byte binary64 atoms decoded by adding `0x10` to the marker. Markers `40..5f` and `c0..df` begin four-byte binary32 atoms decoded by subtracting `0x10` from the marker; the finite binary32 value is widened exactly to binary64. The three atoms retain their ordered values, width forms, and source offsets.

The same three-scalar clause framing applies independently to every complete body-reference occurrence in any operation record: the body-reference terminator is followed by a one-byte branch discriminator and three self-delimiting scalar atoms. Each complete clause retains its body-reference occurrence order, body object index, discriminator, scalar values, width forms, and source offsets. A body occurrence without three complete scalar atoms does not produce a scalar clause.

A branch-`11` body clause may continue with a wrapped member lane `01, count:u8, (2e, compact_index, 00)[count-1]`, where `count >= 2` and compact indices use the non-null compact-index form. The lane is atomic and retains body-reference occurrence order, member order, decoded index, and source offset.

For `TRIM BODY`, the branch-`11` member lane is followed by `01, 02, compact_index, 00, 00, 01, object_index, 00, 00`. The compact index and terminal object index are non-null. The continuation is atomic and retains the anchoring body index, continuation index, terminal object index, and their source offsets.

A branch-`11` or branch-`1c` body clause may continue after its three scalars with an unwrapped reference lane `01, count:u8, reference[count-1], 00, 00, 0b, 00`, where `count >= 2`. Every reference in one lane uses either non-null compact-index encoding or `f0`/`f1` payload object-index encoding; encodings are not mixed. The indices address offset-only OM data blocks under the unique-resolution rule used by construction references. The lane is atomic and retains the body-reference occurrence, branch discriminator, encoding, ordered decoded indices, ordered resolved targets, and source offsets. A wrapped branch-`11` member lane begins with `2e` after its count and is disjoint from this form.

An `EXTRUDE` construction profile is complete when its witnessed profile-reference field and one branch-`11` payload-object reference lane contain the same non-empty ordered object-index sequence and independently resolve to the same ordered offset-bounded data blocks. The construction profile retains the anchoring body index, ordered object indices, resolved blocks, and source offsets from both encodings. Missing, ambiguous, differently ordered, differently resolved, or unresolved inputs reject the complete profile atomically.

A wrapped operation-body member is a body operand when its compact index differs from the anchoring body index and equals an object index present in an operation body-reference field or validated segment body-binding tuple. The operand retains its body clause, member order, serialized identity, matching segment bindings, and source offset. Other wrapped members retain only their native member representation.

Bodies named by validated segment binding tuples exist at the start of retained feature history. A `SEW` or `TRIM BODY` body operand consumes that body image when the body's latest decoded writer precedes the operation. Boolean tool operands follow the same ordering rule. A later writer supersedes earlier consumption. Terminal body selection is applied only when every emitted partition has one unambiguous terminal status and at least one, but not every, emitted body remains terminal.

The structured extrusion branch begins `32 00 00` after its unique body-reference field, followed by one shifted-IEEE binary64 scalar. A counted fixed-width lane follows as `01, count:u8, (3d, extended_compact_index, 00)[count-1]`, where `count >= 2`. Each wrapped index uses exactly `80..fe, low:u8` and decodes as `(marker-80)*256+low`; direct and null forms are invalid in this lane. Two counted compact-index lanes follow, each framed `01, count:u8, index[count-1]` with `count >= 2`. Compact indices use `00..7f` as direct values, `80..fe, low:u8` as `(marker-80)*256+low`, and `ff` as null; null is invalid in these lanes. Indices in all three lanes address offset-only OM data blocks under the unique-resolution rule used by profile references. The branch ends `00 01, object_index, 00 00`, using the feature object-index form. The terminal object index equals the body object index anchoring the branch.

A complete structured-`32` extrusion construction requires one self-witnessed structured branch, one non-empty profile-reference field with contiguous ordinals, and unique data-block resolution for every profile reference and every member of the branch's three index lanes. It retains the branch, body identity, ordered profile references, and the four resolved block lanes without assigning unresolved semantic roles to the three branch lanes.

A `BLOCK` payload begins `control:u8, 00 00 01 00 00`, eighteen contiguous canonical payload references, `01`, one terminal canonical payload reference, eleven `ff` bytes, and four zero bytes. A canonical payload reference is `f0, value:u8` for `0..255` or `f1, value:u16 BE` for `256..65535`; noncanonical widths invalidate the complete field. The nineteen ordered references address offset-only OM data blocks under the uniqueness rule used by sketch and extrusion profile references. The control byte is retained independently of the ordered reference lane.

The logical `BLOCK` construction payload is the bytewise concatenation of all eighteen resolved member blocks followed by the resolved terminal block. Fields may cross source-block boundaries. The reconstructed payload retains its exact length and hash, ordered block identities, payload-relative block starts, exact block lengths, and absolute source offsets.

A `BLOCK` operation parameter binding selects the first declaration of its dimension run. The run consists of exactly three consecutive, unqualified declarations `pN`, `p(N+1)`, and `p(N+2)` in expression-record order. Each declaration resolves uniquely to one finite millimeter expression. The typed dimension set retains every anchor binding, the three ordered declarations and expression records, and the three values in model millimeters. A nonconsecutive name or index, ambiguity, non-length unit, or unevaluated value rejects the complete dimension set.

The owning `BLOCK` feature links the complete typed dimension set and construction independently. Dimension order is native parameter order; placement and axis roles remain separate from the three scalar dimensions.

A `BLOCK` feature with a complete typed dimension set projects as a neutral rectangular block with ordered local x, y, and z dimensions. Its placement remains absent until the native local-to-model frame is complete; absent placement does not imply the identity transform.

A complete block construction requires nineteen contiguous reference ordinals, one uniform control byte, exactly eighteen nonterminal members, one final terminal reference, and unique data-block resolution for every reference. It retains the member lane and terminal reference as distinct fields. Missing, reordered, differently controlled, incorrectly terminated, or unresolved inputs reject the construction atomically.

A body-reference field is `01 02 10, object_index, ff`. `object_index` uses the feature object-index form: `00..7f` is direct, `80..8f` contributes the high index byte and is followed by one low byte, `90` is followed by a big-endian `u16`, and `ff` is null. Every complete non-null field in a bounded operation record is retained in byte order. Exactly one field identifies an unambiguous primary-body writer; records containing zero or multiple fields do not establish that writer role.

An object-ID-bounded record in a section declaring `UGS::EXP_expression` declares a parameter name as `04, declared_len:u8, name[declared_len-2], 00`. `name` is `p`, one or more decimal digits, and an optional underscore-prefixed qualifier composed of ASCII letters, digits, and underscores. A declaration record contains exactly one such name frame. The parameter index is the decimal integer after `p`. The record may contain one additional frame with the same framing whose text is a context-free constant numeric expression; this is the declaration-local literal. Multiple numeric-expression frames make the declaration literal ambiguous without invalidating the parameter declaration. An exact unique name match binds the declaration to the value record carrying `(Number [mm|degrees]) name: expression; `.

An offset-only OM data block references a persistent OM object as `04 00, object_index, 02 0b`, using the same object-index form as feature operation headers. Complete fields are retained in block byte order. An object ID resolves to a target record or parameter declaration only when exactly one record with that ID occurs in the same directory entry.

An operation input slot depends on every uniquely resolved parameter declaration referenced by its target data block. Binding order is operation-header slot order followed by reference byte order within each block. When exactly one numeric-expression record names the declaration, the consumption edge also identifies that expression record. The binding establishes parameter consumption but does not assign a dimensional role to the parameter.

The `SIMPLE HOLE` payload template is underscore-delimited. `Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer` identifies a general simple hole extending through all material, with chamfer treatments at its entry and exit. The six tokens form one atomic template; missing, reordered, or unknown tokens do not produce a typed hole template.

Before its unique `Hole_` template string, a `SIMPLE HOLE` payload may carry exactly four marker-`30` shifted-binary64 scalars. When the first scalar is bitwise equal to the third and the second is bitwise equal to the fourth, the payload retains one ordered pair with two byte-identical witnesses. Any other scalar count or unequal pair rejects the repeated pair atomically. No unit, coordinate frame, or geometric role is assigned to these values.

Each complete scalar-pair witness is followed immediately by two tagged object indices. `f0,lo` encodes an ordinal below 256. `f1,hi,lo` encodes a big-endian ordinal of at least 256. Both pairs address blocks by direct ordinal in the offset store that owns the operation-header input blocks. The four indices resolve atomically: the operation inputs must select one store and every addressed ordinal must exist in that store. The first and repeated pairs retain their order independently.

A `DATUM_CSYS` payload begins `control:u8, 00 00 01 00 00 01 01 00 01 00 00 00 00`, followed by exactly eight canonical `f0`/`f1` object indices and `01 01 00 01 00 00 00 00`. The control byte is retained independently. The eight indices resolve atomically to blocks in the single offset store selected by the operation-header inputs. Their serialized order is retained. A missing, noncanonical, unresolved, differently stored, or incorrectly terminated reference rejects the complete coordinate-system construction lane.

The first two resolved datum-coordinate-system blocks form one logical object payload in serialized lane order. Their bytewise concatenation is authoritative: fields may cross the source-block boundary. The reconstructed payload retains its exact length and hash, both block identities, payload-relative block starts, exact block lengths, and absolute source offsets. The other six construction lanes remain independently bounded records.

An object-payload scalar-pair frame is `08 02 03 01, branch, c0 45 04 00 80 86 02 00 03, shifted-f64, 00, shifted-f64`, where `branch` is `03 01` or `81 02 01`. Each complete occurrence in a reconstructed datum-coordinate-system or sketch payload is retained in payload order. Both values are finite. The typed frame preserves its owning logical payload, exact discriminator including the branch, payload-relative discriminator and scalar offsets, and their exact absolute source offsets across source-block boundaries. A preceding `6d 00 f0` prefix belongs to the containing record and does not create a second pair.

Each sketch feature links its ordered typed coordinate-pair records by payload ordinal. Source-block boundaries do not delimit sketch entities and cannot assign coordinate ownership; a coordinate frame crossing a block boundary remains one field in the owning logical sketch payload.

Each of datum-coordinate-system construction lanes 5–7 is an independently bounded descriptor block. A typed block contains exactly one maximal run of 30–32 lowercase hexadecimal digits. Bytes before and after the identity remain exact prefix and suffix fields. The descriptor retains its construction lane, resolved block, identity, exact prefix and suffix, block offset, and identity offset. A block with no qualifying run or multiple qualifying runs remains untyped.

Equal typed descriptor identities join datum-plane and datum-coordinate-system constructions. The relation retains both typed descriptors, both operations, the shared identity, and the coordinate-system lane ordinal. Feature dependency follows serialized operation order: the later operation depends on the earlier operation. Identity equality does not impose a fixed plane-to-coordinate-system ownership direction.

Each resolved coordinate-system block is joined to every operation-header input addressing the identical store block. The relation retains the coordinate-system construction, reference ordinal, shared block, input binding, consuming operation, and input slot. Equal numeric indices in different stores do not join. No origin, axis, input, or output role follows from block equality alone.

A `DATUM_PLANE` payload begins `control:u8, 00 00 01 00 01, declared_count:u8, branch_tag:u8, 01 02`, with `declared_count >= 2`. The control, count, and branch tag are retained independently. The branch tag selects the following construction grammar; the common header assigns no reference, plane-kind, origin, or normal role to branch bytes.

For branch tag `1b` or `23` with declared count two, the header is followed by one non-null compact descriptor index, `01`, one canonical `f0`/`f1` object index, and `00 14 02 00 01 00 00 00 00 ff ff 00`. The descriptor and object indices remain separate ordered fields. Both indices resolve atomically in the single offset store selected by the operation-header inputs; a missing, ambiguous, or differently stored target leaves both unresolved. The branch does not assign a plane-kind, origin, normal, or dependency role to either index.

Branch tag `29` carries two canonical object indices. With declared count two they are separated by `01 01 18 03 00 01 00 00 00 00 ff` and followed by `01`, nine `ff` bytes, twelve zero bytes, and `0d`. With declared count three they are separated by `01 01 3a 01 02` and followed by `01 17 02 00 01 00 00 00 00 ff ff 00`, nine `ff` bytes, twelve zero bytes, and `0d`. Both indices resolve atomically under the same operation-selected-store rule. Their serialized order is retained without assigning plane-frame or dependency roles.

Branch tag `28` with declared count three carries one non-null compact descriptor index, `01 29 01 02`, one canonical object index, `01 01 07 02 00 00 00 00 00 00 ff ff 00`, nine `ff` bytes, twelve zero bytes, and `0d`. Its two indices use the same separate ordered lanes and atomic same-store resolution as the tag-`1b`/`23` form.

Each resolved datum-plane descriptor or object block is joined to every operation-header input addressing the identical store block. The relation retains the construction operation, lane kind, lane ordinal, shared block, consuming operation, and input slot. Equality across different offset stores does not join and the relation alone assigns no plane-frame role. When the datum-plane construction precedes the consuming operation in the same ordered feature area, the consuming feature depends on the datum-plane feature.

The logical datum-plane object payload is the bytewise concatenation of its resolved object blocks in serialized lane order. Block boundaries do not delimit fields. The payload retains its exact length and hash, ordered block identities, each block's payload offset and byte length, and each block's absolute source offset.

A terminal datum-plane object-index lane is `01, declared_count:u8, compact_index[declared_count-1], 00, trailer:u32 BE`, with `declared_count >= 2`. Every compact index is non-null and the trailer ends at the reconstructed payload boundary. A unique complete lane retains its payload offset, count, ordered values and value offsets, and trailer word. Truncation, null indices, trailing bytes, or multiple complete candidates leave the typed lane absent.

A datum-plane object scalar-pair frame is `6d 00 f0 08 02 03 01 03 01 c0 45 04 00 80 86 02 00 03, shifted-f64, 00, shifted-f64`. Each occurrence in the reconstructed logical payload is independent and ordered by payload offset. Both scalars are finite. The native record retains the frame offset, both scalar offsets and values, and their exact absolute source offsets across source-block boundaries.

A datum-plane descriptor block is exactly 40 bytes: `lowercase_hex_identity, 3f 41, compact_schema_index, ff 02 01, printable_label`, where the identity and label are nonempty and the compact index is non-null. Descriptor references resolve within the operation-selected offset store. The typed descriptor retains its owning plane header, descriptor-lane ordinal, resolved block, identity, exact delimiter-prefixed suffix, schema index, label, and absolute block offset. Malformed framing or a non-40-byte block leaves the descriptor untyped.

**Persistent-handle identity.** `e0 + handle:u32 BE` values are persistent handles forming a cross-stream bridge (RMFastLoad ↔ UG_PART OM ↔ EXTREFSTREAM). Equal handle values group their ordered distinct bounded OM records, offset-store control blocks, and indexed EXTREFSTREAM records under one native handle identity. A second family is a four-byte big-endian word whose high nibble is `0xC` and low 28 bits are the reference value. Both tokens remain within one externally bounded record and occur as `(e0-handle, c-ref)` pairs.

**Same-section record references.** A counted reference run is `01, count:u8, (count - 1) × (90, record_ordinal:u16 BE)`, with `count >= 2`. Every ordinal addresses an entity record in the same external entity-index directory. The containing record depends on the addressed records; the addressed records have the containing record as a dependent. The complete run lies within one bounded record; any out-of-range ordinal invalidates the run atomically. Token order is operand order, and inverse dependent order follows containing-record ordinal.

### 7.2 Partition and deltas merge

A complex part contains current body images and historical or tool bodies, each with its own partition/deltas pair and stream-local xmt namespace, plus optional plain cached tool bodies. `RMFastLoad` object-ID membership identifies the current body images. Multiple decisively represented images are distinct current bodies. When membership does not distinguish current images from historical or tool bodies, the final body set requires the operand bindings and order encoded by NX OM feature-history records.

`/Root/part/attrs` is a versioned XML attribute table. Each `Attr` element
contains its owner token, UTF-8 title and value, schema type, PDM-ownership flag,
and record version. These part-level values transfer as document attributes;
the native record retains the remaining ownership and schema fields.

```text
live = partition ∪ delta_full − tombstones
```

- A full record with `xmt ∈ partition` replaces that partition record. Paired streams share one xmt namespace.
- A full record with `xmt ∉ partition` (high range) adds a new entity.
- The deltas stream adds entities through explicit high-range records.

BODY (`00 0c`, xmt=3) records delimit body revisions. `node_id` is a monotonic per-body revision counter. A partition containing a validated body-shape SHELL is the authoritative current topology image. BODY through REGION records in its paired deltas stream are revision history and do not replace or delete that topology image.

`RMFastLoad` stores the active object-id set alongside the partition and deltas body records. FACE, EDGE, and VERTEX `node_id` values share this identity space. Membership assigns each represented body image independently; the set may select more than one body. A body image without active membership is retained unless another image has a decisive membership assignment.

Within one ordered feature-history area, the last operation carrying a primary-body field is that body object's latest writer. A segment-bound image exists before the retained operations when it has no decoded writer. The two body-object indices in a segment tuple are aliases for one body image and are interchangeable in writer and operand fields. A later Boolean consumes each tool image; a later `SEW` or `TRIM BODY` consumes each typed body operand. Consumption applies only when the image's latest writer precedes the consuming operation, and a still later writer supersedes it. Every segment binding receives one terminal or consumed lineage status only when alias pairs are nonconflicting and the complete ordered history resolves atomically. Terminal selection requires one status for every emitted partition image and retains at least one but fewer than all images; otherwise every emitted image remains retained.

A compact deltas tombstone is `type:u16 BE, xmt:u16 BE, 00 01`. Outside the authoritative partition topology families, a matching key deletes the partition record and a full record replaces it. Repeated events are chronological; the last full record or tombstone for one key is current. A deltas topology image is assembled only when its partition has no validated body-shape SHELL.

---

## 8. Units and tolerances

- Geometric doubles are meters; multiply by 1000 for mm. Applies to point coordinates, radii, offsets, tolerances, chart chords, TRIMMED_CURVE LINE parameters.
- Do not scale unit axes/directions/normals, `thumb_weight`, angular parameters (radians), UV surface parameters, knot values, or ratios.
- `chordal_error` defines the verification tolerance for chart-hosted procedural carriers.
- Exactness certificates for procedural geometry are floor bounds `max(1e-12, 128·eps·scale)` mm; the relations are zero in exact arithmetic (S0==S1 spine identity, envelope-of-spheres normal).

## 9. Additional record semantics

### 9.1 `EXTREFSTREAM`

An `EXTREFSTREAM` record region begins with `0x00`, followed by little-endian `(record_id, record_offset)` pairs terminated by a single `record_id == 0`. A handle-set record at `record_offset` begins `01 00 00 00`, then `n:u16 BE`, `01`, four `u32 LE` ID slots, `01`, `count:u8`, `count - 1` occurrences of `e0 + handle:u32 BE`, and a closing byte equal to `count`. Handles are strictly ascending except that the final occurrence may repeat the preceding handle; transfer records whether that closing duplicate is present and omits it from the normalized handle list. Other indexed record layouts remain opaque. The trailing string table is `01 + count:u32 LE + count × (len:u16 LE + nonempty control-free UTF-8)`. The final string ends at the stream boundary. The nominal `16 + payload_size` boundary can fall inside a string record. Each string transfers with its table ordinal and absolute byte offset.

### 9.2 Stream and deltas framing

The `00 ce` stream-root schema declares `index_map`, `node_id_index_map`, and `schema_embedding_map`; each serializes as a null or empty array and supplies no tombstone bridge.

A deltas-stream BODY record with type `00 0c` and xmt `3` delimits a body snapshot. Its `node_id` is a monotonic revision counter within that body sequence, and a reset begins another interleaved body sequence. Deltas streams encode null-node deletions as descending contiguous xmt runs that can span topology, geometry, and attribute record types.

### 9.3 B-spline payloads

A type-125 B-surface control payload stores a parameter-range block, a marker byte, a sense byte, `double_count:u32`, a large-index-capable `first_index`, and `double_count` doubles. An optional envelope escape before `double_count` shifts the remaining fields by one byte.

A type-126 B-surface descriptor stores U and V degrees, pole counts, form codes, distinct-knot counts, multiplicity references, knot references, and a control-payload reference. It has short and large-index layouts.

A type-135 B-curve control payload stores `double_count:u32`, `first_index`, and `double_count` doubles. Type 136 stores degree, pole count, dimension, distinct-knot count, form, control-data index, multiplicity reference, and knot reference.

The B-spline form code does not determine whether a control grid is rational. The control-grid stride determines the representation: stride 3 stores xyz and stride 4 stores xyzw.

### 9.4 Attributes and expressions

Parasolid attribute definitions use a two-record catalog entry. `00 4f [ff] name_len:u32 BE, class_xmt:u16 BE, name[name_len]` declares a non-empty printable ASCII class name; `ff` is the optional record-envelope escape. The field record follows immediately as `00 50, field_count:u32 BE, field_xmt:u16 BE, reference[2]:u16 BE, header_word[2]:u16 BE, payload`. Both XMT identities and the ordered references are stream-local. The header words are retained verbatim; their second value includes `2328`, `1f67`, and `1f44`. A truncated header invalidates the declaration pair atomically. Type code `0x05` in the field payload denotes a component/reference or string field, `0x06` a double field, and `0x00` a void or flag field.

A type-81 entity/attribute-list record is `00 51 [ff], flags:u32 BE,
xmt, sequence:u32 BE, discriminator:u16 BE, references`. XMT fields use the
compact or extended XMT encoding. `xmt` is non-null, `sequence` is nonzero, and
the low flags byte is in `1..=0x20`. The reference count is seven for
`(discriminator, low_flags) = (001d|001e, 02)`, nine for
`(0020|0024|0027, 04)`, and six otherwise, including
`(0018|0020|0025, 01)`. References are either consecutive XMT values or
individually `01`-prefixed XMT values followed by `00`; the two forms are
atomic. A topology attribute-list identity resolves only when exactly one
type-81 record in the same stream has that xmt.

`hostglobalvariables` stores numeric expressions as independently length-framed ASCII records:

```text
handle:u8  04  length:u8  "(Number [units]) name: expression; "  00
```

`length - 2` is the ASCII text length. `units` is `mm` or `degrees`; `name` contains ASCII alphanumerics and underscores. `expression` is a finite decimal scalar or formula. Context-free arithmetic uses parentheses, unary signs, `^`, `*`, `/`, `+`, and `-`. Formula parameter references use `p<decimal-index>` tokens. The record framing is independent of the OM entity-index and object-ID arrays. An enclosing indexed entity supplies persistent object identity when present; otherwise the record's entry-relative byte offset supplies identity.
