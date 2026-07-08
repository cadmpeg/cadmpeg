# SolidWorks `.sldprt`: Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

## 1. File container

### 1.1 Outer header and block frame

The file starts with an 8-byte header: `file_id` (u32), then `version` (u32 **big-endian**, value `0x00000004`). The rest is a sequence of compressed blocks.

A block begins with the marker `14 00 06 00 08 00` and uses this frame:

```
marker      bytes[6]   ; 14 00 06 00 08 00
type_id     u32 LE
crc32       u32 LE     ; CRC-32 of the DECOMPRESSED payload
comp_sz     u32 LE
uncomp_sz   u32 LE
pre_sz      u32 LE
preamble    bytes[pre_sz]
payload     bytes[comp_sz]   ; raw DEFLATE, wbits = -15
```

A valid block decompresses to exactly `uncomp_sz` bytes and has CRC-32 `crc32`. Its extent is `block_end = marker_offset + 26 + pre_sz + comp_sz`. Preamble bytes decode to OPC section names by swapping the high and low nibble of each byte, such as `Contents/Config-0-Partition`, `Contents/DisplayLists`, `PreviewPNG`, and `swXmlContents/Features`.

### 1.2 Cache-cell section-index grid

The bytes before the first valid block and the long inter-block gaps hold a **fixed-cell section-index grid** reusing the outer marker. These cells are **not** compressed payloads. The three size-shaped header fields are redundant scalings of one logical value `L`:

```
field@+10 == 2*L
field@+14 == L // 2
field@+18 == L
field@+22 == name_len       ; nibble-swapped section name follows at +26
```

Each file has one cache-cell stride `S`; every `L` is an integer multiple (`L = N*S`, `N in 5..14`).

A valid cache cell satisfies `two_L == 2L`, `half_L == L//2`, `0 < name_len < 500`, and has a printable nibble-swapped name.

### 1.3 Tail section directory

The file tail carries an **OPC package section directory**: a per-section index, not checksum blobs. Fixed-shape frames, each naming one OPC part:

```
+0   marker      bytes[6]   ; 14 00 06 00 08 00
+6   type_id     u32 LE
+10  zero        u32 LE
+14  size        u32 LE     ; section's stored/uncompressed size
+18  zero        u32 LE
+22  name_len    u32 LE
+26  descriptor  bytes[14]
+40  name        bytes[name_len]   ; nibble-swapped section name
     trailer     bytes[6]   ; [4-byte per-file separator][00 00]
```

The 6-byte trailer has one value for all entries in a file, for example `e5 4b 57 5b 00 00`; its first four bytes form the directory separator. Decoded names are OPC parts such as `[Content_Types].xml`, `_rels/.rels`, `docProps/Config-0-Properties.xml`, and `ThirdPty/SWA_Schedules`.

A tail-directory entry with `size == 2` aliases a degenerate empty compressed block. Its payload is raw-DEFLATE `03 00`, its decompressed size and CRC-32 are zero, and its section name occupies the block preamble. This directory/block aliasing is valid.

---

## 2. Block payload families

Payload kind is determined from **decompressed bytes**, not from `type_id`.

| Signature at/near payload start                 | Family                      |
| ----------------------------------------------- | --------------------------- |
| `89 50 4e 47`                                   | PNG preview                 |
| OLE2 compound-file header                       | OLE2                        |
| `PS 00 00`                                      | plain Parasolid stream      |
| Parasolid wrapper magic at offset 4             | wrapped Parasolid           |
| nested compressed member → `PS 00 00`           | nested Parasolid            |
| `uoTempBodyTessData_c` / `uoTempFaceTessData_c` | tessellation / DisplayLists |
| `ff ff 01 00`                                   | SW Objects                  |
| `<?xml` / UTF-16LE BOM+XML / byte0 `86`+XML     | XML                         |
| `unqlite`                                       | UnQLite database            |

The active `Contents/Config-0-Partition` block contains the analytic B-rep in partition and deltas streams. `Contents/Config-0-ResolvedFeatures` contains feature-input sketch profiles. `Config-0-GhostPartition`, `Contents/Definition`, and `Config-0-LWDATA` do not contain active body geometry.

`ResolvedFeatures` sketch entities start with `ff ff 1f 00 03`. A little-endian u32 at marker +17 is the native entity type: `0` point, `1` curve, `2` arc, `3` constrained point; other values remain native codes. IR retains the complete block payload and marker offsets. Semantic writing patches typed codes into that payload and preserves all other bytes.

---

## 3. Parasolid stream

### 3.1 Stream header

```
PS 00 00
desc_len    u16 BE
description ASCII[desc_len]
padding
schema_len  u8
schema      ASCII[schema_len]      ; SCH_<modeller>_<schema>_<format>
```

The schema identifier has the form `SCH_<modeller>_<schema>_<format>`. Partition and deltas streams contain the active body geometry. Class-definition payloads use `C` for class, `I` for instance, `A` for attribute, `D` for data, and `Z` for a Z-block container.

### 3.2 Sites and attribute scope

An attribute id is **not** globally unique. A **site** is one validated outer block (identified by its marker offset). Partition and deltas streams in the same outer block share a site namespace; streams in different outer blocks are distinct sites.

These families **must be keyed by `(site_id, attr)`** because their attrs collide across sites: compact analytic records, `00 11` coedges, `00 12` vertex-uses, `00 1d` points. Bridges (`00 0e`), loop heads (`00 0f`), and edge-uses (`00 10`) are globally unique, but their references to colliding families must still resolve inside the referring record's site. Within a site, partition records are the base set and deltas records are incremental variants; a weak deltas candidate must not overwrite a stronger partition record. Boundary vertices are **coordinate-canonical**: resolve `00 11 → 00 12 → 00 1d` inside the coedge's site, then deduplicate by coordinate.

---

## 4. Typed topology records

Primary ownership chain:

```
face → 00 0e bridge → support surface
face → 00 0f loop head → 00 11 coedge ring → 00 10 edge-use → support curve
00 11 coedge → 00 12 vertex-use → 00 1d world point
```

| Tag     | Role                           | Bare length | Magic       |
| ------- | ------------------------------ | ----------: | ----------- |
| `00 0e` | bridge / face-use→surface link |          37 | at body +8  |
| `00 0f` | loop head                      |         ≥14 | none        |
| `00 10` | edge-use                       |          28 | at body +8  |
| `00 11` | oriented coedge                |          21 | none        |
| `00 12` | vertex-use                     |          24 | at body +16 |
| `00 1d` | world point                    |          38 | none        |

Magic-bearing records use `c2 bc 92 8f 99 6e 00 00`.

- **Bridge `00 0e`:** `refs[2]` = owning loop-head, `refs[4]` = primary surface carrier (compact analytic or `00 7c`), `marker` = face orientation versus the surface natural normal (`0x2b` forward / `0x2d` reversed). `ref0` = owner/use discriminator for face validation.
- **Loop head `00 0f`:** `refs[1]` = first coedge, `refs[2]` = owning bridge, `refs[3]` = next sibling loop head.
- **Edge-use `00 10`:** `refs[0]` = canonical forward coedge (`0x2b`), `refs[3]` = support curve (compact analytic or `00 86`).
- **Coedge `00 11`:** `refs[1]` owning loop, `refs[2]`/`refs[3]` reciprocal ring links (prev/next), `refs[4]` start vertex-use, `refs[5]` twin coedge, `refs[6]` edge-use, `marker` sense vs canonical (`0x2b` forward, `0x2d` reversed).
- **Vertex-use `00 12` / point `00 1d`:** `00 12.refs[4]` = point attr; `00 1d` stores xyz as three f64 BE at body +14, in metres. Attrs `0` and `1` are sentinels, not world points.

A support surface belongs to a face only through `face -> validated bridge -> bridge.refs[4] -> carrier`. Face and carrier attributes do not establish ownership by equality. Resolve the carrier in the bridge's site.

### 4.1 Canonical edge direction

```
canonical coedge = same-site coedge with attr == 00 10.refs[0]   (marker always 0x2b)
edge.start_vertex = canonical.start_vertex_use → 00 12 → 00 1d
edge.end_vertex   = partner coedge (same edge_use_attr).start_vertex_use → …
```

The canonical coedge anchors the stored edge direction.

Deduplicate byte-backed edge uses by order-independent endpoint pair, curve kind, and curve-geometry fingerprint. Retain the lowest-attr representative. A final B-rep boundary edge is an edge used by a loop after this deduplication. A native `00 1d` coordinate is a boundary vertex only when a final edge endpoint reaches it.

For a closed full-circle edge whose paired coedges both use vertex-use sentinel `0x0001`, derive its seam vertex as `circle.origin - circle.radius * owning_cylinder_surface.ref_direction`. A periodic cylinder with two singleton circle loop heads has one derived axial seam-line edge between its circle seam vertices. A spherical patch bounded by three circle arcs has a degenerate meridian seam edge at `center + radius * axis`.

### 4.2 Deltas encodings

Deltas streams re-encode records in prefixed/tripled forms (each ref stored as a `[hi][lo][01]` triple) or as `[disc][attr]` adjacency tables; the magic moves within the record window and must be located inside it.

| Tag                    | Deltas form | Magic    | Anchor                                 |
| ---------------------- | ----------- | -------- | -------------------------------------- |
| `00 10`                | prefixed    | body +9  | decoded ref slot 2 = curve carrier     |
| `00 11`                | tripled     | none     | slot4 vuse, slot5 twin, slot6 edge-use |
| `00 12`                | prefixed    | body +21 | refs-before-magic slot 4 = point attr  |
| `00 1d`                | prefixed    | none     | xyz after `[hi][lo][01]*` run          |
| `00 1e/1f/20/32/33/35` | prefixed    | none     | f64 block after `2b`/`2d` marker       |

Partition and deltas streams in the same outer block share a site namespace. When recovering a deltas-only candidate topology, key its records under a synthetic deltas site so colliding references resolve within the deltas tables. Deltas topology is candidate topology. Prefer partition topology for the final solid unless a byte-backed body-membership path assigns a deltas edge to the active solid boundary.

Recover loop heads and bare coedges with non-advancing byte-by-byte passes. Accept a recovered coedge only when its edge-use and start vertex-use resolve and its twin has the same edge-use, a reciprocal twin reference, and the opposite marker. Apply the loop-head graph filter after coedge recovery.

Use the bridge marker as the initial face orientation. Flip whole faces until each shared edge has opposite composed orientation in its two incident faces, then select the remaining shell sense by positive signed volume.

## 5. Entity records and canonical faces

Top-level entity families: `00 51` entity, `00 52` wrapper/container, `00 53` color/property/helper, `00 54` metadata. Common header: `flags u32 BE`, `attr u16 BE`, `seq u32 BE`, `disc u16 BE`.

`0x52` and `0x51` can occur in u16 slot values. Entity boundaries use fixed-slot windows keyed by `(schema, disc, flo)`.

The canonical face container depends on the SolidWorks generation:

| Family       | Selection rule                                        |
| ------------ | ----------------------------------------------------- |
| disc14       | entity-51 `disc == 0x0014` + structured 6-slot prefix |
| disc15/flo=1 | entity-51 `disc == 0x0015`, low flag byte 1           |
| disc1F/flo=1 | entity-51 `disc == 0x001f`, low flag byte 1           |

All share a 6-slot prefix (`raw_body[12:24]` = six u16 BE slots) with family-specific roles. Support families (disc11/flo=2, disc13/flo=1, disc19/flo=2) are not faces but participate in owner-chain validation: a small set of per-family chains resolves `bridge.ref0` to the face attr, directly or through one/two support records. A structurally valid canonical face outranks any non-face record for the same attr; within a class, highest `seq` wins. A wide scan can catch a **false** `00 54` when a byte pair lands inside another record's payload (garbage high-bit `disc`, absurd `seq`); a plain highest-seq dedup would drop the real face, so the canonical-precedence rule is required.

---

## 6. Bodies

Explicit manifold bodies use entity-51 disc `0x0017` with geometry-body flags. Grouping resolves through two mechanisms: UUID-body face-list membership, and single-shell face-use rings. The body→face relation partitions the face set exactly once per file (0 duplicate, 0 unassigned, 0 dangling). **Bodies must be grouped explicitly**: pooling all faces and re-separating by geometric edge connectivity merges bodies whose faces touch (abutting bodies collapse into one wrong pseudo-solid).

### 6.1 Schema-32001 UUID bodies

Stream-scope `00 51` records carry a disc/flo family: disc17/flo2 = UUID body, disc1b/flo2 = solid region/lump, disc19/flo2 = connector, disc15/flo2 = body face-list head, disc13/flo1 = face owner, disc1f/flo1 = canonical face. The optional `ff` after the `00 51` marker is **load-bearing** (a body-face-list head is invisible without the one-byte shift).

**Solid/sheet gate:**

```
solid iff  0x17.slot1 → 0x1b directly
      or   0x17.slot1 → 0x19 ; 0x19.slot1 → 0x1b
```

A body satisfying neither is an **open/sheet body**. Sheet bodies are positively identified: each reaches `0x19 → slot1 → disc 0x1d/flo1`, so schema 32001 uses `0x1b/flo2` for solid regions and `0x1d/flo1` for sheet regions. Both body kinds retain their faces and boundary topology in IR. The gate is schema-specific; applying the 32001 constants to schema 33103 misclassifies bodies.

**Body → face membership** is a relocated shell/face-use equivalent, read section-ordered by stream offset between face-list heads.

### 6.2 Schema-33103 variant

Schema 33103 encodes the same section-ordered membership with shifted roles:

| role           | 32001       | 33103       |
| -------------- | ----------- | ----------- |
| canonical face | disc1F/flo1 | disc15/flo1 |
| face-list head | disc15/flo2 | disc13/flo2 |
| body           | disc17/flo2 | disc17/flo2 |
| region         | disc1b/flo2 | disc1b/flo1 |

Heads bind to bodies by a **shared slot0 cluster key** (`body.slot0 == head.slot0`), because `head.slot1` is polymorphic. Each disc17 body resolves through its disc1b/flo1 region into a slot1-linked `disc1b → disc1f → disc21 → disc23` hierarchy (the 33103 analog of 32001's region/lump/shell).

**The face partition is the disc15/flo1 face-use _adjacency graph_, not the stream interval:** the head stream-interval is only a section-order proxy, exact when a body's records are contiguous and wrong when interleaved. The adjacency-graph components are the true per-body face sets and build valid closed solids where the interval reading yields a non-closing shell.

### 6.3 Single-shell disc14 bodies

The partition stream of a single-solid disc14 part has exactly one region + one shell, with one face-use per face. Deltas streams add extra shells/face-uses from superseded feature states, so the shell count must come from the partition stream's **undeduplicated** variants. Body/shell/face-use membership walks through an entity web (shell `0x16` → face-use `0x20`.slot3 ring → face-geom `0x18` → face `0x14`). An explicit **class-root vector** anchored by the ASCII token `index_map_offset` followed by `CCZ` names the head entity of each disc family; walking the face-use ring from the root visits every face-use and terminates at sentinel `0x0001`. A `0x52`-safe fixed-slot window is required; a terminator scan loses face-uses whose slot values contain byte `0x52`.

---

## 7. Geometry carriers

All length fields are metres. Directions, normals, axes, reference directions, knots, and weights are dimensionless.

### 7.1 Compact analytic records

Stream-scope support-geometry carriers encoding untrimmed surface/curve placement. Generic layout:

```
00 TT  [ff]?  attr u16 BE  ordinal u32 BE  refs u16 BE[5]  marker u8 (0x2b|0x2d)  values f64 BE[n]
```

| Tag     | Kind     | f64 count | Payload                                                                  |
| ------- | -------- | --------: | ------------------------------------------------------------------------ |
| `00 1e` | line     |         6 | point xyz, direction xyz                                                 |
| `00 1f` | circle   |        10 | center xyz, axis xyz, refdir xyz, radius                                 |
| `00 20` | ellipse  |        11 | center xyz, axis xyz, refdir xyz, major r, minor r                       |
| `00 32` | plane    |         9 | origin xyz, normal xyz, refdir xyz                                       |
| `00 33` | cylinder |        10 | origin xyz, axis xyz, radius, refdir xyz                                 |
| `00 34` | cone     |        12 | origin xyz, axis xyz, radius, sin half-angle, cos half-angle, refdir xyz |
| `00 35` | sphere   |        10 | center xyz, radius, axis xyz, refdir xyz                                 |
| `00 36` | torus    |        11 | center xyz, axis xyz, major r, minor r, refdir xyz                       |

The cone fields satisfy `sin² + cos² = 1`. Torus fields satisfy `major > minor > 0`; the axis has unit length and the reference direction is orthogonal to it.

Compact records omit trim intervals, loop membership, edge orientation, and vertex points. Typed topology records contain those relations.

### 7.2 B-spline and list carriers

A bridge's `refs[4]` can point to a `00 7c` **surface-use wrapper** (a first-class carrier for list/NURBS surfaces) instead of a compact analytic surface. Ownership check: `00 7c.refs[1] == owning 00 0e bridge attr`. The wrapper's `child0` → `00 7e` **surface descriptor** (control/knot counts at fixed u16 BE offsets; final five refs = `[control_grid, u_mult, v_mult, u_knot, v_knot]`); `child1` → `00 7d` **scale node** (for a curved surface, a diagonal parameter-space scale; both `1.0` = identity).

B-spline **array records** are reached by attr reference, not inline:

```
00 2d  marker [ff?]  value_count u32 BE  attr u16 BE  f64[value_count] BE   ; poles / homogeneous control grid
00 7f  marker [ff?]  count       u32 BE  attr u16 BE  u16[count]  BE        ; knot multiplicities
00 80  marker [ff?]  count       u32 BE  attr u16 BE  f64[count]  BE        ; unique knot values
```

For rational arrays (`dimension == 4`) `00 2d` stores `[x*w, y*w, z*w, w]` per pole. Surface control grids use native index `u*n_v + v`. A trailing zero-multiplicity sentinel and its paired `0.0` knot are not part of the knot vector.

Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier, whose body references a `00 88` **curve descriptor** (attr, degree, control_count, dimension, knot_count, subtype, flags, then control/multiplicity/knot array attrs). Adjacent `00 87`/`00 b8`/`00 a3` are 3D prolog/wrapper records, not 2D UV pcurves.

The curved Parasolid partition and deltas stream grammar contains no stored two-dimensional UV pcurve control array. The `00 2d`, `00 7f`, and `00 80` arrays carry 3D or homogeneous control nets and knot data; trimmed NURBS-face pcurves derive from the surface, 3D boundary curve, and trim topology.

## 8. Auxiliary lanes

- **DisplayLists tessellation** uses a 6-descriptor table: List A strip lengths, Positions/Normals f32 metres, and Lists B/C/D. `C = sum(ListA)`, `ListC[i] = 2*ListA[i] - 2`, and `TriCount = C - 2*N`.
- **Materials / metadata** live in SW Objects blocks: `moVisualProperties_c` contains material names and RGB `0x00BBGGRR` values; names use UTF-16LE. `moBBoxCenterData_c` contains the bounding-box center and maximum radius in metres. `moDefaultRefPlnData_c` contains datum planes through the origin.
- **Per-face appearance** is generation-specific and the two routes point in _opposite_ directions: disc14 is face-local (`disc14 face stub → adjacent 00 53 flo=3 → rgb`); disc15 (33103) is `face slot5 → 00 53 flo=3 color attr → rgb`. A real flo=3 color record is ~84 bytes (three RGB f64 BE at body +6 + one inline `00 51` face link); the apparent "2–3 KB nested color bodies" are the `0x52`-terminator artifact. The optional `ff` after `00 53` is load-bearing.
- **XML / UnQLite** carry OPC parts, document/feature metadata, unit metadata (`SW_UnitsLinear=0` = millimetres), and MessagePack UI data: auxiliary, not the exact B-rep.

---

## 9. Units

- **Length fields are metres.** Model-space coordinates, radii, and length-bearing values in compact analytic records, `00 1d` points, and B-spline control grids are stored in metres.
- Coordinates and radii convert from metres to millimetres by a factor of `1000`. Rational B-spline poles require dehomogenization (`[x*w, y*w, z*w, w] -> xyz`) before conversion.
- Directions, normals, axes, reference directions, knot values, and weights are dimensionless.
- `SW_UnitsLinear=0` denotes millimetre document units. It does not change stored coordinate values.
- Model coordinates use the export world frame. Metres-to-millimetres conversion by `1000` is the only coordinate conversion; no per-file rotation, translation, or scale transform applies.

---

## 10. Inline record selection

Inline `00 51` subrecords use a fixed slot count selected by the Parasolid schema, disc, low flag byte, and optional prefixed form. `00 51` and `00 52` byte occurrences do not delimit records.

For a prefixed subrecord, `body[14] == 0x01`, each slot is `[01][hi][lo]`, the final byte is `00`, and `end = pos + 14 + 3*slot_count + 1`. For a bare subrecord, `end = pos + 14 + 2*slot_count`. Stop an inline walk at the next record owned by another entity.

In schema 33103, partition and deltas candidates that share a face attribute and sequence number select the candidate whose slot-5 appearance reference resolves. A remaining tie selects the partition candidate.
