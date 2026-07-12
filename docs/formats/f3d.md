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

`+52` is end_vertex and `+79` is curve, not the other way round. `t_start`/`t_end` are stored parameters on the edge's own parameterization: the referenced curve itself when the sense byte is forward (`0x0b`), its reverse `E(t) = C(−t)` when reversed (`0x0a`). A full-circle edge has identical start/end vertex with `t_start = -π`, `t_end = +π`; the shared vertex lies at the `t_start` angle from the major axis, so a full period's phase is significant, not a free normalization. The continuity text is descriptive metadata, **not** a curve-type discriminator.

**Vertex (63 B):** `chunk[3]` @+36 = owning_edge, `chunk[4]` @+45 = index_flag (`0` = this is the owning edge's START vertex, `1` = its END vertex), `chunk[5]` @+54 = point ref. Each vertex has its own point entity; no deduplication.

**Transform (142 B):** 13×f64 (@+18..117): `a[0..8]` 3×3 rotation, `a[9..11]` translation, `a[12]` overall scale; then 3 flag bytes (ROTATION/REFLECTION/SHEAR enums). Column mapping: `a[0..2]`→col0, `a[3..5]`→col1, `a[6..8]`→col2, `a[9..11]`→col3. The body references its transform through `body.chunk[5]`; null denotes no body transform.

### 6.3 Point records and coordinate authority

A `point` record carries a model-space `POSITION`. `vertex.chunk[5]` references the point record. NURBS control grids independently carry their model-space poles.

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

**`cone` (161 B, covers cylinders)**: order: origin (`0x13`), axis (`0x14`), `ref × r_major` (`0x14`, magnitude = base radius), `ratio` (f64, 1.0 = circular), `0x0b 0x0b`, `sin(half_angle)` (f64, 0 ⇒ cylinder), `cos(half_angle)` (f64), `u_scale` u-parameter scale (f64), 5×`0x0b`. **Half-angle rule:** `half_angle = asin(|sine|)`. The angle is the acute branch even when both stored sine and cosine are negative. **Sign rules:** the base radius is the major-axis vector's magnitude; `u_scale` usually equals it but diverges on offset-derived surfaces and is not a radius. The signed slope `sine / cosine` is the radius change per unit axis distance: `r(d) = r_base + d · sine / cosine` at signed distance `d` along the axis from the origin. A negative `cosine` points the surface normal toward the axis; face senses are stored relative to that inward normal.

**`sphere` (134 B)**: center (`0x13`), **signed** radius (f64), dir1 (equator), dir2 (polar axis). **Signed-radius rule:** a negative radius identifies an inward-facing, concave feature; the sign is part of the carrier.

**`torus` (142 B basic / 160 B ranged)**: origin, axis, `major_radius` (f64), **signed** `minor_radius` (f64), `ref_direction`; then a range flag (`0x0b` = full 142-B variant; `0x0a` = 160-B variant with start/end angles). `minor < 0` with `|minor| ≤ |major|` describes an apple/lemon torus. **Inside-out torus rule:** `|minor| > |major|` is self-intersecting. The native frame and minor-radius sign are part of the carrier.

Evaluation formulas for all four carriers follow directly from the frame vectors above.

### 7.3 Analytic curve byte layouts

**`straight` (115 B)**: base point + direction vector. Curve range is unbounded; the owning edge's `t_start`/`t_end` clip it. Endpoints `= base + t·direction` with the stored, unnormalized vector: the direction's magnitude is the line's parameter scale and is not necessarily 1.

**`ellipse` (148 B with angles / 130 B without, covers circles)**: center, axis normal, `ref × r_major` (magnitude = major radius), `ratio = r_minor/r_major`; the 148-B variant adds start/end angles. Circle when `ratio==1`. **Ratio-sign phase convention:** for `ratio > 0` the stored range is axis-aligned and the endpoint phase is +π/2. For `ratio < 0`, the negative sign encodes a flipped parameterization; the stored range is direct and the minor-radius magnitude is `|ratio|`.

**`degenerate_curve`**: collapses to a point (cone apex / sphere pole). An edge may _also_ collapse to a point with no `degenerate_curve` entity: curve ref null and both vertex refs identical. That is valid ACIS, not a malformed edge.

**`helix_int_cur`**: finite angle interval, axis-start position, major-radius position vector, minor-radius position vector, pitch position vector, apex-factor double, and unit axis vector, followed by the solved curve cache. Position-vector components and the cache fit tolerance are lengths. The major and minor vectors have equal magnitude. Their orientation about the axis records handedness; the pitch vector records axial rise per revolution, and the apex factor records linear radial growth per revolution fraction.

**`offset_int_cur`**: one subtype flag, source curve, start/end source-parameter doubles, model-space offset vector, then two `(string label, integer role code)` pairs, followed by the solved curve cache and its fit tolerance. The source curve and solved cache are distinct carriers. Offset-vector components and fit tolerance are lengths; parameters and role codes are unscaled.

**`subset_int_cur`**: parent curve followed by a two-bound native parameter interval, then the solved curve cache and fit tolerance. The parent and solved cache are distinct curve carriers. The interval is unscaled.

**`exact_int_cur`**: the solved `nubs`/`nurbs` curve cache is the authoritative exact construction payload, followed by its fit tolerance. No weaker analytic carrier is implied by the subtype. A zero fit tolerance denotes an exact cache.

**`comp_int_cur`**: a counted leading parameter array, component count, one parameter double per component, one ASM extension flag, then exactly that many ordered child curves. The final curve cache and fit tolerance follow the child curves. Component parameters and the leading parameter array are unscaled; child and solved NURBS control points and fit tolerance use the standard length scaling.

**Surface-related intcurve prefix**: two ordered support surfaces, two ordered BS2 parameter curves paired by side, one native parameter interval, then three counted discontinuity arrays. `null_surface` and `nullbs` are explicit absence sentinels. The interval and discontinuity values are unscaled.

**`off_int_cur`**: the surface-related prefix, one ASM extension flag, then signed left/right offset lengths. The solved curve cache and fit tolerance follow the offsets. The two offsets correspond to the two ordered support sides.

**`int_int_cur`**: the surface-related prefix followed by one ASM extension flag, then the solved curve cache and fit tolerance. The construction is the intersection of the two ordered support surfaces; the paired BS2 curves retain its parameterization on each support.

**`proj_int_cur`**: the surface-related prefix, one ASM extension flag, the source curve, and a second boolean flag. In the ranged form, a source-parameter interval and projection-role string (`surf1` or `surf2`) follow the flag before the solved cache. In the early-close form the subtype closes immediately after the flag and the solved carrier is external to that subtype payload.

**`sss_int_cur`**: the surface-related prefix, an integer selector, then a third support surface and its paired BS2 parameter curve. The solved cache and fit tolerance follow the third support pair. All three support sides retain their serialized order.

**Prefix-only surface curves**: `blend_int_cur`, `surf_int_cur`, `par_int_cur`, and `skin_int_cur` contain the surface-related prefix with no subtype-specific tail, followed by the solved cache and fit tolerance. The subtype name distinguishes blend-edge, surface-constrained, parametric, and skin construction semantics.

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

**`scaled_cloft_spl_sur`**: a singularity enum and singularity-selected shape payload precede six discontinuity arrays, one discontinuity flag, three scale slots, two flags, and an integer. The full shape payload is the solved NURBS surface and fit tolerance. The none shape payload replaces that cache with two intervals and two scalar arrays; the owning face retains an unknown evaluated carrier and the procedural graph supplies its exact construction. The three leading scales form a contiguous prefix under the same zero-token absence rule as `cl_loft_spl_sur`. A false branch flag selects a flag, integer, and selector-zero direction vector or selector-nonzero BS3 curve. A true branch flag selects an optional scale and a second flag. A true second flag requires another scale, integer, and direction vector; a false second flag stores another boolean, singularity enum, and BS3 curve. Every branch rejoins at two flags, an integer, two vectors, a singularity enum, and a BS3 curve. Native generation uses `scaled_cloft_spl_sur`.

**`skin_spl_sur`**: three surface enums, an integer, a scalar, and an inner count precede a structurally selected skin layout. The compact layout begins directly with a curve, loft subdata, integer, second curve, and final integer. The expanded layout contains `inner_count` entries, each comprising a type integer, curve, and loft profile data, followed by a path curve and two integers. Both layouts rejoin at a direction vector, scalar, recursive law formula, parameter curve, solved NURBS surface and fit tolerance, six discontinuity arrays, and a boolean. Native generation retains the selected layout.

**`net_spl_sur`**: two ordered loft-section graphs precede twelve frame scalars, one integer, four direction vectors, and four recursive law formulas. The solved NURBS surface and fit tolerance, six discontinuity arrays, and one boolean complete the payload. Native generation retains every section member, support, pcurve, constraint table, auxiliary path, frame value, and formula.

**`sweep_spl_sur` profile-first layout**: a primary enum precedes the profile curve and spine curve. A secondary enum, five direction vectors, one model-space point, four scalars, and three recursive law formulas follow. The solved NURBS surface and fit tolerance, six discontinuity arrays, and one boolean complete the payload. Native generation retains both curves and the complete construction graph.

**`sweep_spl_sur` explicit formula layout**: a primary enum and integer precede a profile curve, its two-scalar parameter interval, and an optional point-vector profile frame. A frame point and three vectors follow. Branch integer `1` then stores a boolean, path curve, model-length interval, scalar, boolean, recursive formula, and trailing boolean. The common solved-surface cache and discontinuity tail complete the payload. Native generation retains the complete construction graph.

**`sweep_spl_sur` explicit guide layout**: the explicit prefix matches the formula layout. Branch integer `2` stores a boolean, path curve, model-length interval, and scalar, followed by two booleans, an auxiliary guide curve, its two-scalar parameter interval, two integers, six scalars, and three booleans. The common solved-surface cache and discontinuity tail complete the payload. Native generation retains all three curves and the complete construction graph.

**`sweep_spl_sur` explicit support-surface layout**: the explicit prefix matches the other explicit layouts. Branch integer `3` stores a boolean, path curve, model-length interval, scalar, singularity enum, and support surface. A boolean gates an auxiliary curve. A support boolean and an optional legacy boolean precede the common solved-surface cache and discontinuity tail. Native generation retains the support surface, optional curve, and complete construction graph.

**`sweep_spl_sur` law-driven layout**: the explicit profile and frame prefix is followed directly by a recursive law instead of a branch integer. An integer, two-scalar interval, vector, integer, boolean, path curve, two-scalar interval, scalar, and boolean precede a second recursive law. A final integer, recursive formula, and boolean precede the common solved-surface cache and discontinuity tail. Native generation retains both law trees, the formula, both curves, and the complete construction graph.

**`t_spl_sur`**: the solved NURBS surface, fit tolerance, and discontinuity tail precede model-length U and V intervals and a type integer. A nested subtype scope contains either an inline `t_spl_subtrans_object` program with an optional boolean separator and companion values program, or a subtype-table `ref`. A trailing integer follows the nested scope. The inline program is line-oriented. Header tokens and topology, geometry, material, grouping, symmetry, annotation, knot, and grip record tokens select ordered field vectors; comments and unrecognized lines do not contribute typed records. Native generation retains inline subtransform programs byte-for-text, requires the parsed graph to agree with the program, and uses the solved NURBS surface as the face carrier.

**Law formulas**: a text name begins each formula. `null_law` has no following payload. Every other formula carries a variable count followed by that many recursively framed law expressions. Integer, double, model-space point, and vector tags are terminal constants. `SPLINE_LAW` stores an integer, a knot float array, a control float array, and a model-space point. `TRANS` stores thirteen scalars and three enums. `EDGE` stores a curve and two parameters. Algebraic operator tokens are followed directly by their recursively framed operands. Trigonometric, hyperbolic, inverse-trigonometric, inverse-hyperbolic, `ABS`, `EXP`, `LN`, `LOG`, `SIGN`, `SIZE`, `TERM`, `SQRT`, `NORM`, and `NOT` operators are unary. `CROSS`, `DOT`, and `DCUR` are binary. `VEC` and `DSURF` are ternary.

**`g2_blend_spl_sur` / `g2blnsur`**: two ordered side graphs surround the first-side singularity payload. Each side stores a label, support surface, curve, two nullable BS2 pcurves, and a direction. The first side then stores a singularity enum. The full branch carries an optional BS3 support surface and paired tolerance. The none branch carries nine frame scalars, a tolerance, an optional intervening typed token, and a tertiary nullable BS2 pcurve. The second side is followed by an exact spline support, center curve, two center scalars, center integer, U/V intervals, four trailing scalars, the solved NURBS surface and fit tolerance, and three discontinuity arrays. Branch shape is structural; the singularity enum value is retained without assigning undocumented numeric meanings.

**`var_blend_spl_sur` / `srf_srf_v_bl_spl_sur`**: two ordered side graphs precede the primary curve, two signed offsets, and a radius-kind enum (`0` single radius, `1` two radii). Each side stores a label, support surface, curve, primary BS2 pcurve, model-space location, secondary BS2 pcurve, scalar, and tertiary BS2 pcurve. Radius controls use recursive blend-value payloads: `two_ends`, `edge_offset`, `functional`, `const`, or `interp`. Modern ASM blend values carry a boolean, optional discriminator, and calibrated enum. `const` recursively contains another blend value; `functional` stores a `(u,radius)` BS2 pcurve and numeric or symbolic terminal; `interp` stores counted parameter/radius/tangent/location/normal controls and an optional scalar-pair tail. Two-radii blends may append rounded-chamfer enums and a third blend value. Single-radius blends may append selector `1` or `7` and two scalars. U/V intervals, a shape integer/scalar/length/integer prologue, solved NURBS cache and fit tolerance, three ASM integers, secondary curve, convexity and render enums, post interval, BS3 curve, and nullable BS2 pcurve complete the graph. Native generation uses `var_blend_spl_sur`.

**`VBL_SURF` / `vertexblendsur`**: a counted sequence of boundary records followed by a grid-size integer and model-space fit tolerance. Every boundary begins with a type name, cross enum, model-space magic location, U/V smoothing enums, and fullness scalar. `circle` adds a curve, form enum, form-selected twist locations (zero for circle, one for ellipse, two for unknown), two parameters, and sense enum. `deg` adds a location and two normals. `pcurve` adds a support surface, nullable BS2 pcurve, sense enum, and parameter-space fit tolerance. `plane` adds a normal, two parameters, and curve. Unknown boundary names and unsupported circle forms are invalid. Native generation uses `VBL_SURF`.

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
  surface-cache
  DOUBLE cache_fit_tolerance
  0x10
```

`u_start` and `u_end` are directrix parameters. `extrusion_direction` is length-bearing. The final `surface-cache` is the solved NURBS surface, and `cache_fit_tolerance` is a length.

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

---

## 8. Materials and appearance (non-B-rep)

### 8.1 Design metadata

`MetaStream.dat` is a sequence of object records. Each record contains an ASCII type name, a u32 ID count, that many little-endian u64 design-entity IDs, a self GUID, a zero-run delimiter, a secondary GUID, and a trailing u32 record revision. The ID count is a count rather than a flag; a record can carry more than two IDs.

The design `BulkStream` caches each body's axis-aligned bounding box as six f64 values in centimetres, ordered `(xmax, ymax, zmax, xmin, ymin, zmin)`. The cache occurs three times in consecutive sub-entity records following the body's assignment container.

The design BulkStream BREP body map is `u32 count`, followed by `count` pairs of `u64 asm_body_key, u64 entity_suffix`, then `u64 trailing_record_ref`, `u32 pad`, `u32 char_count`, and UTF-16LE `BREP.<uuid>.smbh`. `asm_body_key` is the ASM body `flags` field. `entity_suffix` is the numeric suffix of the design entity ID.

A browser-node record stores a length-prefixed 36-character UTF-16LE node GUID, a one-byte hidden flag, the `0x01 0x01` marker, and the node's `u64` design-entity suffix. Flag `1` hides the entity in the document display; `0` shows it. **Body visibility join:** ASM `asm_body_key` → BREP body map `entity_suffix` → browser-node hidden flag.

A browser body record carries the body's appearance binding. The record head references the body's design entity with the `299` class tag (`u32 3`, ASCII `299`, `u64 entity`). The appearance fields open with the marker GUID pair `D87FBE62-3B12-4CA8-9014-BAD31ABDB101` and `C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C` as consecutive length-prefixed 36-character UTF-16LE strings, then in order: the LP-UTF16 physical-material token (`PrismMaterial-###`) with `0x01 + u64` entity reference, the LP-UTF16 36-character browser-node GUID with `0x01 + u64` node entity, an optional LP-UTF16 display name, zero padding, an f32 opacity, the `0x01 0x01` marker, zero padding, and the LP-UTF16 visual GUID (a 36-character GUID with `_Post2015` repeats). The node entity equals the body's design entity plus one. **Body appearance join:** `299`-tag entity → BREP body map `entity_suffix` → ASM `asm_body_key`; the visual GUID's 36-character prefix selects the appearance asset. ASM body records whose key field is null (`-1`) are sub-bodies of the stream's keyed body and carry no design records of their own.

A sketch entity container follows its self-validating entity header and UTF-16LE entity ID with `u32 record_reference`, `u32 zero`, `0x01`, `u32 reference_count`, then `reference_count` entries of `0x01 + u32 record_index + six zero bytes`. The referenced records contain the sketch's geometry and relations.

An indexed Design record header is `u32 class_tag_length`, a three-digit ASCII dynamic-class tag, then `u32 record_index`. `record_index` is a logical reference value; it is independent of the header's byte offset in the `BulkStream`.

A sketch relation stores counted member references, zero or more auxiliary references, the owning sketch reference, a u32 constraint mask, and a counted return-reference list. References use `0x01 + u32 record_index` with zero padding; direct u32 role fields may occur between references. Constraint bits are `0x1` coincident, `0x2` colinear, `0x4` concentric, `0x10` parallel, `0x20` perpendicular, `0x40` horizontal, `0x80` vertical, `0x100` tangent, `0x200` curvature, `0x400` symmetry, `0x800` equal, `0x1000` midpoint, `0x2000` polygon, `0x10000000` circular pattern, and `0x20000000` rectangular pattern.

A sketch-point record contains one typed property named `pt_tag`: `u32 property_count=1`, LP-ASCII `pt_tag`, LP-ASCII `IntrinsicMetaTypeuint64`, and the persistent u64 point id. The record then stores `0x01`, a u32 paired record reference, a 14-byte flag area whose bytes are each `0x00` or `0x01`, and two f64 sketch coordinates in centimetres at record offsets 89 and 97. The alternate form sets `property_count=2` and prefixes `pt_tag` with an `EntityGenesis` `IntrinsicMetaTypeuint64` property; all subsequent fields shift by 52 bytes.

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
