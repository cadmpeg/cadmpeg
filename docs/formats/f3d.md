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

### 1.2 Small stored placeholder entries

The following small entries are STORED:

| Entry                                           | Bytes                   | Meaning                                      |
| ----------------------------------------------- | ----------------------- | -------------------------------------------- |
| `Properties.dat`                                | `00 00 00 00` (u32 `0`) | empty document-properties slot               |
| `.../DesignConfigurationTable.<uuid>.dsgcfg`    | `7B 7D` (`{}`)          | single-configuration model (no config table) |
| `.../DesignConfigurationRule.<uuid>.dsgcfgrule` | `7B 7D` (`{}`)          | no configuration rules                       |

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
04 i64 high_water_mark
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

---

## 3. ASM binary header

Streams begin with `ASM BinaryFile8<` (16-byte magic) or `ASM BinaryFile4` (15-byte magic; there is no `<`). The digit selects the width of integer/ref tags (§4): `4` → tag + 4-byte LE signed; `8` → tag + low 32 bits + high 32 bits (consume the full 9-byte field). Fusion writes both widths; ASM-227-era streams are `BinaryFile4` and ASM-231-era streams are `BinaryFile8`.

`BinaryFile8` header layout:

| Bytes    | Meaning                                                            |
| -------- | ------------------------------------------------------------------ |
| `0..15`  | magic `ASM BinaryFile8<`                                           |
| `16..23` | zero                                                               |
| `24..31` | **big-endian** u64 version/save word: per-file-varying (see below) |
| `32..39` | big-endian u64 = `3` (constant: ASM binary format version)         |
| `40..47` | big-endian u64 = `7` (ASM binary schema version)                   |

Byte 47 is both the low byte of the schema-version word and the `0x07` tag of the first string.

`BinaryFile4` header layout (the classic ACIS save header, little-endian):

| Bytes    | Meaning                                                                                |
| -------- | -------------------------------------------------------------------------------------- |
| `0..15`  | magic `ASM BinaryFile4`                                                                |
| `15..19` | little-endian u32 ASM release word (`22700` on ASM 227 streams)                        |
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

- The `BinaryFile8` words at 24/32/40 are **big-endian**; everything else in either width is little-endian.
- Word @24 is a header version/save word, not a model-space quantity.
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

**Body (61 B):** `chunk[1]` (@+16, i64) is `history / body flags`, the **`asm_body_key`** joined to the design-side body map (§8). `chunk[3]` @+34 = first_lump, `chunk[4]` @+43 = first_wire or `-1`, `chunk[5]` @+52 = transform or `-1`.

**Lump (61 B):** `chunk[4]` @+43 = first_shell, `chunk[5]` @+52 = owner_body. (The @+27 slot is reserved `-1`, not the first shell.)

**Shell (80 B):** `chunk[5]` @+53 = first_face, `chunk[6]` = wire, `chunk[7]` = owner.

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

**Loop (61 B):** `chunk[3]` @+34 = next_loop (`-1` terminates the chain), `chunk[4]` @+43 = first_coedge, `chunk[5]` @+52 = owner_face. Loop order is defined by the `next_loop` references, not stream position; the first loop is not an outer-loop marker.

**CoEdge (100 B):**

```
+35 chunk[3] next_coedge   +44 chunk[4] prev_coedge   +53 chunk[5] partner_coedge
+62 chunk[6] edge          +71 chunk[7] sense byte
+72 chunk[8] owner_loop    +81 chunk[9] reserved int (const 0)
+90 chunk[10] pcurve ref (or -1)
```

The `{+35,+44,+53}` triad is next/prev/partner. `+72` is the owner loop. **Partner symmetry** is a manifold invariant: every coedge's partner's partner is itself, and every shell edge is shared by exactly two mutually-referencing coedges of opposite sense.

**Edge (98 B):**

```
+34 chunk[3] start_vertex   +43 chunk[4] t_start (f64)
+52 chunk[5] end_vertex     +61 chunk[6] t_end (f64)
+70 chunk[7] owner_coedge   +79 chunk[8] curve ref
+89 chunk[9] sense byte     +90 0x07 'tangent'|'unknown' continuity text
```

`+52` is end_vertex and `+79` is curve, not the other way round. `t_start`/`t_end` are stored parameters on the referenced curve. A full-circle edge has identical start/end vertex with `t_start = -π`, `t_end = +π`. The continuity text is descriptive metadata, **not** a curve-type discriminator.

**Vertex (63 B):** `chunk[3]` @+36 = owning_edge, `chunk[4]` @+45 = index_flag (`0` = this is the owning edge's START vertex, `1` = its END vertex), `chunk[5]` @+54 = point ref. Each vertex has its own point entity; no deduplication.

**Transform (142 B):** 13×f64 (@+18..117): `a[0..8]` 3×3 rotation, `a[9..11]` translation, `a[12]` overall scale; then 3 flag bytes (ROTATION/REFLECTION/SHEAR enums). Column mapping: `a[0..2]`→col0, `a[3..5]`→col1, `a[6..8]`→col2, `a[9..11]`→col3. The body references its transform through `body.chunk[5]`; null denotes no body transform.

### 6.3 Point records and coordinate authority

A `point` record carries a model-space `POSITION`. `vertex.chunk[5]` references the point record. NURBS control grids independently carry their model-space poles.

### 6.4 Sense semantics

Three sense bits compose into the winding:

- **face.sense**: forward = surface's natural normal, reversed = flipped.
- **coedge.sense**: loop-traversal direction relative to the edge curve parameterization.
- **edge.sense**: the edge's own curve-parameterization sense.

**Winding rule:** `effective_curve_reversed = edge.sense_reversed XOR coedge.sense_reversed`. Each edge has two coedges with opposite `effective_curve_reversed`.

### 6.5 Ownership reachability

Topology membership is defined by references from `body → lump → shell → face → loop → coedge → edge → vertex`. Surface, curve, and point membership follows the authoritative binding references in §6.1.

An edge with `owner_coedge_ref == -1` and no reference from a reachable coedge is outside that ownership graph.

### 6.6 Attributes on the topology graph

Every entity carries an `attrib` ref-chain. `Entity.attrib` is the chain head, each record carries `next` and `previous` references, and `-1` terminates the chain. Color and feature-tag attributes can coexist on one chain. `ATTRIB_CUSTOM-attrib` records carry an owner ref at record-relative `+60..68` and a family name (`generic_tag_attrib_def`, `sketch_attrib_def`, `Timestamp_attrib_def`, `FPM_tracked_attrib_def`). Attribute records are variable-width.

`generic_tag_attrib_def` stores a count followed by repeated `(kind, token string, design reference, 0, 0)` groups. `kind` identifies the labelled entity class: `3` for body, `2` for face, and `1` for edge. Each token/reference pair binds a persistent Fusion design ID to an ASM entity reference.

`sketch_attrib_def` is coedge-owned source-link metadata. After its three-integer attribute header, a tagged UTF-8 field stores the six-integer ASCII tuple `(sketch_curve_id, 0, signed_ref, 0, enum_a, enum_b)`, where `signed_ref` uses `-1` as null. It links a B-rep coedge to a sketch curve and does not define analytic geometry.

---

## 7. Geometry carriers

All model-space lengths are cm→mm ×10; unit vectors/ratios/angles/knots are not scaled (§5).

### 7.1 Surface vocabulary

`plane`, `cone` (covers cylinders: `sin(half_angle)==0` ⇒ cylinder), `sphere`, `torus`, `spline` (procedural/NURBS, dispatched by nested subtype), `mesh` (not the exact carrier when analytic/spline carriers exist). Curve vocabulary: `straight`, `ellipse` (covers circles: `ratio==1` ⇒ circle), `intcurve`, `pcurve`, plus `null_*` sentinels.

### 7.2 Analytic surface byte layouts

Each layout is fixed-size. Offsets are record-relative from the `0x11` byte.

**`plane`**: origin (`0x13`) + unit normal (`0x14`) + unit UV-reference direction (`0x14`). Evaluation `S(u,v) = origin + u·u_dir + v·v_dir`, `v_dir = normal × u_dir`.

**`cone` (161 B, covers cylinders)**: order: origin (`0x13`), axis (`0x14`), `ref × r_major` (`0x14`, magnitude = major radius), `ratio` (f64, 1.0 = circular), `0x0b 0x0b`, `sin(half_angle)` (f64, 0 ⇒ cylinder), `cos(half_angle)` (f64), `r1` explicit base radius (f64), 5×`0x0b`. **Half-angle rule:** `half_angle = asin(|sine|)`. The angle is the acute branch even when both stored sine and cosine are negative.

**`sphere` (134 B)**: center (`0x13`), **signed** radius (f64), dir1 (equator), dir2 (polar axis). **Signed-radius rule:** a negative radius identifies an inward-facing, concave feature; the sign is part of the carrier.

**`torus` (142 B basic / 160 B ranged)**: origin, axis, `major_radius` (f64), **signed** `minor_radius` (f64), `ref_direction`; then a range flag (`0x0b` = full 142-B variant; `0x0a` = 160-B variant with start/end angles). `minor < 0` with `|minor| ≤ |major|` describes an apple/lemon torus. **Inside-out torus rule:** `|minor| > |major|` is self-intersecting. The native frame and minor-radius sign are part of the carrier.

Evaluation formulas for all four carriers follow directly from the frame vectors above.

### 7.3 Analytic curve byte layouts

**`straight` (115 B)**: base point + unit direction. Curve range is unbounded; the owning edge's `t_start`/`t_end` clip it. Endpoints `= base + t·direction`. Line directions are unit vectors.

**`ellipse` (148 B with angles / 130 B without, covers circles)**: center, axis normal, `ref × r_major` (magnitude = major radius), `ratio = r_minor/r_major`; the 148-B variant adds start/end angles. Circle when `ratio==1`. **Ratio-sign phase convention:** for `ratio > 0` the stored range is axis-aligned and the endpoint phase is +π/2. For `ratio < 0`, the negative sign encodes a flipped parameterization; the stored range is direct and the minor-radius magnitude is `|ratio|`.

**`degenerate_curve`**: collapses to a point (cone apex / sphere pole). An edge may _also_ collapse to a point with no `degenerate_curve` entity: curve ref null and both vertex refs identical. That is valid ACIS, not a malformed edge.

**`helix_int_cur`**: finite angle interval, axis-start position, major-radius position vector, minor-radius position vector, pitch position vector, apex-factor double, and unit axis vector, followed by the solved curve cache. Position-vector components and the cache fit tolerance are lengths. The major and minor vectors have equal magnitude. Their orientation about the axis records handedness; the pitch vector records axial rise per revolution, and the apex factor records linear radial growth per revolution fraction.

**`offset_int_cur`**: one subtype flag, source curve, start/end source-parameter doubles, model-space offset vector, then two `(string label, integer role code)` pairs, followed by the solved curve cache and its fit tolerance. The source curve and solved cache are distinct carriers. Offset-vector components and fit tolerance are lengths; parameters and role codes are unscaled.

**`subset_int_cur`**: parent curve followed by a two-bound native parameter interval, then the solved curve cache and fit tolerance. The parent and solved cache are distinct curve carriers. The interval is unscaled.

**`exact_int_cur`**: the solved `nubs`/`nurbs` curve cache is the authoritative exact construction payload, followed by its fit tolerance. No weaker analytic carrier is implied by the subtype. A zero fit tolerance denotes an exact cache.

### 7.4 Pcurves (2D UV trimming curves)

A `pcurve` record has two byte-level forms, discriminated by the `0x04` int at record-relative **+37**:

- **discriminator == 0 → inline form**: a `0x0a`/`0x0b` `wrapper_reversed` boolean, then a `0x0f 0d 0b exp_par_cur` subtype opening a 2D `nubs` or rational `nurbs` block. 2D poles are stored as `(u,v)` pairs (8+8 B each, **not** 24); `nurbs` stores one homogeneous weight after each pole.
- **discriminator != 0 (1, 2, −1) → ref form (72 B)**: a `0x0c` ref to the intcurve carrying the UV curve, then two parameter doubles. No wrapper boolean (its absence is structural).

UV poles are dimensionless surface parameters. `wrapper_reversed` is the inline curve's fit-convention bit, independent of coedge sense and of the parameter-interval sign.

The inline control polygon is followed by a `DOUBLE` parameter-space fit tolerance. After the nested support-surface scope and four trailing booleans, two final `DOUBLE` values store the pcurve parameter interval `(t_start, t_end)`. Ref-form pcurves store the same interval immediately after their intcurve reference and have no wrapper or inline fit-tolerance carrier.

Coedge sense is the edge-use orientation for a pcurve inherited from its surface: `effective_pcurve = flip_pcurve(surface_pcurve, coedge.sense)`. The stored 2D B-spline poles and knots retain their native order. `wrapper_reversed` is separate from coedge sense.

An explicit pcurve reference belongs to a free-form B-spline face. Analytic plane, cylinder, cone, sphere, and torus faces store `-1` in the coedge pcurve field; their UV boundary is not serialized as a pcurve record.

### 7.5 `nubs`/`nurbs` blocks (B-spline curves and surfaces)

Surface block grammar: name (`nubs`|`nurbs`), degree_u, degree_v, u/v periodicity + singularity enums, unique-knot counts, (knot, multiplicity) pairs for each direction, then the control grid (3D for `nubs`, 4D homogeneous for `nurbs`). Control grids are **row-major with v in the outer loop, u in the inner loop.**

**Pole-count rule:** the block stores endpoint multiplicities as `degree` (not `degree+1`). With stored multiplicities: `n_poles = sum(stored_mults) − (degree − 1)`. With expanded (clamped) multiplicities: `n_poles = sum(expanded_mults) − (degree + 1)`. Both expressions produce the same pole count.

Native ASM NURBS control grids are the per-face cache. `surface_fit_tolerance == 0.0` indicates fidelity to the procedural surface, rather than identity with a primitive.

### 7.6 `intcurve` and `spline` subtypes

Procedural intcurve subtypes (`exact_int_cur`, `off_int_cur`, `proj_int_cur`, `int_int_cur`, `sss_int_cur`, …) and spline-surface subtypes (`rb_blend_spl_sur`, `sss_blend_spl_sur`, `var_blend_spl_sur`, `loft_spl_sur`, `sweep_spl_sur`, `net_spl_sur`, VBL/taper families, …) each carry per-subtype field tails and version/`asm_major` gates. A `ref N` nested inside a surface, curve, or pcurve body indexes a per-file subtype table, not a byte offset. Each `0x0F` Surface/Curve/Pcurve block contributes one table entry in stream order.

An `intcurve` or `spline` record carries a record-level sense boolean immediately before its subtype scope (`0x0a` reversed, `0x0b` forward). A reversed record's geometry is the reverse of its subtype definition: a reversed intcurve parameterizes as the negation of its cache (`C(t) = cache(−t)`; the owning edge's `t_start`/`t_end` are on the reversed parameterization), and a reversed spline surface's normal is the reverse of the cache normal (the face's sense field composes on the reversed surface).

A `spline` subtype can contain several top-level surface-bearing `nubs` or `nurbs` blocks. The final surface block is the face-surface cache; earlier blocks can be 2D support pcurves. A nested `ref` denotes another carrier through the subtype table.

The `cyl_spl_sur` and `rb_blend_spl_sur` field sequences are:

```
cyl_spl_sur :=
  0x0f 0x0d "cyl_spl_sur"
  DOUBLE u_start
  DOUBLE u_end
  VECTOR_3D extrusion_direction
  POSITION
  curve-cache
  surface-cache
  DOUBLE cache_fit_tolerance
  0x10
```

`u_start` and `u_end` are directrix parameters. `extrusion_direction` is length-bearing. The final `surface-cache` is the solved NURBS surface, and `cache_fit_tolerance` is a length.

```
rb_blend_spl_sur :=
  0x0f 0x0d "rb_blend_spl_sur"
  support-name support-kind
  support-name support-kind
  curve-cache
  DOUBLE radius_start
  DOUBLE radius_end
  ENUM_VALUE -1
  surface-cache
  DOUBLE cache_fit_tolerance
  0x10
```

Each `support-name` is the string `blend_support_surface`; `support-kind` is a surface class token. The curve cache is the blend center curve. The signed radii and fit tolerance are lengths. Equal radius values define a constant-radius rolling-ball blend; unequal values define a linear radius law.

---

## 8. Materials and appearance (non-B-rep)

### 8.1 Design metadata

`MetaStream.dat` is a sequence of object records. Each record contains an ASCII type name, a u32 ID count, that many little-endian u64 design-entity IDs, a self GUID, a zero-run delimiter, a secondary GUID, and a trailing u32 record revision. The ID count is a count rather than a flag; a record can carry more than two IDs.

The design `BulkStream` caches each body's axis-aligned bounding box as six f64 values in centimetres, ordered `(xmax, ymax, zmax, xmin, ymin, zmin)`. The cache occurs three times in consecutive sub-entity records following the body's assignment container.

The design BulkStream BREP body map is `u32 count`, followed by `count` pairs of `u64 asm_body_key, u64 entity_suffix`, then `u64 trailing_record_ref`, `u32 pad`, `u32 char_count`, and UTF-16LE `BREP.<uuid>.smbh`. `asm_body_key` is the ASM body `flags` field. `entity_suffix` is the numeric suffix of the design entity ID.

A sketch entity container follows its self-validating entity header and UTF-16LE entity ID with `u32 record_reference`, `u32 zero`, `0x01`, `u32 reference_count`, then `reference_count` entries of `0x01 + u32 record_index + six zero bytes`. The referenced records contain the sketch's geometry and relations.

An indexed Design record header is `u32 class_tag_length`, a three-digit ASCII dynamic-class tag, then `u32 record_index`. `record_index` is a logical reference value; it is independent of the header's byte offset in the `BulkStream`.

A sketch relation stores counted member references, zero or more auxiliary references, the owning sketch reference, a u32 constraint mask, and a counted return-reference list. References use `0x01 + u32 record_index` with zero padding; direct u32 role fields may occur between references. Constraint bits are `0x1` coincident, `0x2` colinear, `0x4` concentric, `0x10` parallel, `0x20` perpendicular, `0x40` horizontal, `0x80` vertical, `0x100` tangent, `0x200` curvature, `0x400` symmetry, `0x800` equal, `0x1000` midpoint, `0x2000` polygon, `0x10000000` circular pattern, and `0x20000000` rectangular pattern.

A sketch-point record contains one typed property named `pt_tag`: `u32 property_count=1`, LP-ASCII `pt_tag`, LP-ASCII `IntrinsicMetaTypeuint64`, and the persistent u64 point id. The record then stores a paired record reference and two f64 sketch coordinates in centimetres. The alternate form sets `property_count=2` and prefixes `pt_tag` with an `EntityGenesis` `IntrinsicMetaTypeuint64` property; all subsequent fields shift by 52 bytes.

A sketch-curve record contains two typed properties in order: `crv_primary_id` and `crv_secondary_id`, both `IntrinsicMetaTypeuint64`. The primary id is the curve's persistent identity; zero in the secondary slot is null. The alternate form sets `property_count=3` and prefixes these properties with `EntityGenesis`, shifting the curve identity and geometry fields by 52 bytes. The analytic payload following the identity properties is twelve f64 values. A line stores `(start point xyz, displacement xyz, unit direction xyz, unit sketch normal xyz)`. A circular arc stores `(center xyz, unit normal xyz, in-plane unit reference direction xyz, radius, start angle, end angle)`. Points, displacements, and radii are in centimetres; angles are radians. A referenced analytic wrapper prefixes this payload with `0x01 + u32 record_ref + six zero bytes`.

A sketch NURBS payload begins with either an eight-byte all-`0xff` null sentinel or a non-null u64 carrier reference, then a nested dynamic-class record header, the degree marker, f64 fit tolerance, and three arrays. Each array header is `(u32 count, u32 duplicate_count, u32 scalar_width=8)`. The arrays are the nondecreasing f64 knot vector, positive f64 weights, and xyz f64 control points. A non-rational curve stores a zero-length weight array; otherwise weight and control-point counts are equal. In both forms, `knot_count = control_point_count + degree + 1`. Fit tolerance and control points are in centimetres.

The ACT BulkStream begins with records whose headers contain a per-file dynamic three-digit ASCII class tag and a u32 record index. `ACTTable` entries are `0x01`, u32 index, six zero bytes, and a UTF-16LE entity ID. The entries are followed by an independent ordered pool of UTF-16LE GUID records; pool position does not assign one GUID to each table entry. Per-entity channel-group records store named channel/GUID pairs followed by the entity ID. Their GUIDs are change-version handles, not visibility or suppression flags.

The ACT root-component link follows its class tag and record index with ten zero bytes, `0x01 + u32 instance_root_index + six zero bytes`, the UTF-16LE root entity ID, `0x01 + u32 3 + five zero bytes`, `0x01 + u32 registry_flag`, the UTF-16LE document display name, one or more zero bytes, and `0x01 + u32 components_root_index`. `registry_flag` is 0 or 1.

On a body, `generic_tag_attrib_def` supplies a design/construction ID distinct from the material-assignment suffix. This ID keys the design BulkStream body construction-recipe records. A body can have no body-keyed recipe.

### 8.2 Materials

Visual and physical materials are distinct serialized channels.

Color attribute records include `rgb_color-st-attrib` (float r,g,b in 0..1), `truecolor-adesk-attrib` (packed ARGB integer), `color-adesk-attrib` (palette index), and `material-adesk-attrib` (library lookup pair). `Timestamp_attrib_def` carries an f64 Unix-epoch timestamp in microseconds for the original feature or body creation time. The ASM header `save_date` stores the file save-time string.

`.protein` assets are **nested ZIP archives** carrying per-asset `AssetData/*.bin` value streams plus XML schemas (`CommonSchema`, `GenericSchema`, `PhysMatSchema`, `PrismOpaqueSchema`, …). `InstanceProperties.bin` and `DefinitionIteratorProperties.bin` have a 16-byte prefix followed by 136-byte pages. Each page is a record-start page, continuation page, or `0xffffffff` terminal page with a u16 used length. A logical record is the concatenation of its start-page and continuation-page payloads.

A design BulkStream material assignment targets the nearest preceding component-prefixed entity ID. Its physical-material token joins to the `.protein` `PhysMatSchema` asset. Its visual appearance GUID is the GUID immediately before the fixed visual-appearance marker GUID. A physical-material default-appearance clause stores associated GenericSchema and Prism appearance asset references.

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
