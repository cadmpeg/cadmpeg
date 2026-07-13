# Dassault Systèmes CATIA V5 `.CATPart`: Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

All multi-byte integers are little-endian unless explicitly marked **BE**. Float coordinates are in millimetres.

---

## 1. Variant families

A file stores its geometry in one of six families; the family determines the record grammar.

| Variant                          | Detection                                                                                                            | Geometry source                                                                               |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| **Standard nested `V5_CFV2`**    | Outer file contains a nested `V5_CFV2` container, no coherent overriding E5 stream                                   | Inner-body BREP spine, trim mesh records, `00 33 <kind>` surface markers, `05 08 01` vertices |
| **FBB-only partial spine**       | Nested `V5_CFV2` with contiguous FBB face rows + `05 08 01` vertices but no standard edge-row table                  | FBB face group + vertex records; post-FBB edge rows are `u24be`, trim `H` handles are `u24be` |
| **E5 `0D 03` stream**            | Coherent walked E5 record stream in the preamble or a FINJPL segment                                                 | Native E5 records: faces, loops, edge-uses, p-curves, curve supports, surface carriers        |
| **Zero-entity `a9 03`**          | No nested inner `V5_CFV2`; outer preamble carries `a9 03 XX YY` record families                                      | Outer-preamble `a9 03` records                                                                |
| **Float-packed inner-no-FBB**    | Nested `V5_CFV2` with no `30 04 04 ff` FBB spine, large vertex/object-graph/float populations                        | Object-stream (`b5 03` / `a8 03`) records; surface-kind markers only for the pure marker case |
| **Inner body without directory** | Nested `V5_CFV2` whose directory contains no BREP body; the body occupies the contiguous region before the directory | Contiguous inner records and freeform carrier families                                        |

Detection invariants: a standard file has one nested inner `V5_CFV2` past byte 8; the standard BREP spine contains the largest FBB run followed by parseable edge tables and a `kind=0x06` table of 15-byte `05 08 01` vertex records. E5 classification requires a coherent record walk. Zero-entity classification requires no inner `V5_CFV2` and at least one recognized `a9 03` family.

Freeform NURBS geometry uses a **three-way storage-class split** cutting across the variants: the consolidated `a5 03 34/32/20` class, the common object-stream `a8 03 34/32/20` class, and the zero-entity `a9 03 34` family. There is no single universal freeform marker.

---

## 2. Identity layers (allocation / occurrence / geometry)

The persisted CGM graph keeps three identity classes separate. Conflating them mislabels allocation metadata as a topology gap.

- **Allocation identity**: the persistent object tag (`op1`; inline `b5 03` object id) and stored table rows (`05 08 01` vertex row, `54`-roster row). This is creation and registration order.
- **Occurrence identity**: the face-local objects (loop members, support occurrences, pcurve occurrences, coedge uses); this layer holds the topology.
- **Geometric definition**: the exact curve/surface/endpoint an occurrence evaluates to.

The occurrence graph and geometry determine an analytic BREP up to isomorphism. Graphs with the same edge-endpoint incidence and coordinates represent the same BREP even when their vertex and edge tables differ in order. The persistent allocation index identifies serialized rows and tags.

---

## 3. Container layout

### 3.1 Outer `V5_CFV2`

```text
0x00..0x07  magic            = "V5_CFV2\0"
0x08..0x0B  directory_offset = u32 BE   # file offset where the trailing stream directory begins
0x0C..0x0F  directory_length = u32 BE   # that directory's byte length
0x10..0x17  fill_ff          = ff * 8
0x18..0x37  fill_00          = 00 * 32
0x38..0x3F  hdr_flags        = 8 raw bytes (not constant)
```

The two u32 fields form a big-endian directory offset and length pair: `directory_offset + directory_length == file_size`. The trailing region `[directory_offset, EOF]` is a `CATIA_V5 CB0001` stream directory with a `CB__END` sentinel. It uses the inner-directory descriptor grammar (§3.4), except that outer-directory physical offsets are measured from file offset 0.

### 3.2 Inner `V5_CFV2`

The standard path uses the first nested `V5_CFV2` magic after byte 8:

```text
inner  = first "V5_CFV2\0" after outer byte 8
A      = u32be(inner + 8)    # directory offset-delta
B      = u32be(inner + 12)   # directory length
diroff = inner + A
```

The inner sub-container stores fragmented named streams and a stream directory. `A` and `B` specify the directory offset delta and length. Reconstruct each logical stream from its extents; the contiguous range `[inner+B, inner+B+A)` can contain stream and directory bytes.

### 3.3 FINJPL segments

`FINJPL  ` (two trailing spaces) marks named stream blocks after the outer preamble. An E5 stream candidate is coherent when at least ten records walk by their declared strides. A coherent preamble wins; otherwise the segment with the largest valid walk wins, with storage type `0x0000008e` breaking equal-count ties.

### 3.4 Nested-container stream directory

```text
file[diroff : diroff+16] == "CATIA_V5 CB0001\0"
directory region = file[diroff : diroff+B]      # ends with "CB__END"
```

Per descriptor header at offset `ds` (found by a self-consistency scan):

```text
ds+0x0c : logical_stream_length  (u32be)
ds+0x50 : extent_count k         (u32be)
ds+0x54 : k extent structs, 20 bytes each:
            phys_off u32be   # measured from the inner magic
            phys_len u32be
            log_len  u32be
            log_off  u32be
            flags    u32be
```

A candidate is a descriptor when every extent validates: `inner + phys_off + phys_len <= filesize`, `phys_len != 0`, `log_off` cumulative from 0, `log_len == phys_len`, `sum(log_len) == logical_stream_length`. A logical stream is the concatenation of `file[inner+phys_off : inner+phys_off+phys_len]` over the descriptor's extents in `log_off` order. The stream name is a UTF-16LE ASCII run in the descriptor header.

The descriptor names include `MAIN`, `MainDataStream`, `Header`, `SceneGraph`, `Describe`, and `SurfacicReps`. `MainDataStream` contains the topology spine, including FBB groups, `05 08 01` vertices, and `30 04 04` loops. `SurfacicReps` contains analytic surface and curve kind markers and trim records. Extents for these streams can interleave physically.

### 3.5 `SurfacicReps` parallel F/E/V roster

A parallel representation of faces/edges/vertices indexed by the same topological tags as the BREP spine.

- **Vertex roster:** trailing run of 7-byte records `54 <tag_u24le> 00 00 00`, with unique, strictly increasing tags.
- **Freeform surface cores:** `<tag_u24le> 00 00 00 <10×f32le> <sign_i8>` (47 bytes), `sign ∈ {+1=0x01, −1=0xff}` (per-face orientation). The ten f32 are `f[0:3]` AABB center, `f[3:6]` AABB half-extents, `f[6:9]` bounding-sphere center, `f[9]` bounding-sphere radius of the **trimmed** face; the per-coordinate containment `|f[i]−f[6+i]| + f[3+i] ≤ f[9]` holds (the AABB-corner-containment reading does not). These cores interleave in face order with analytic surface records (`00 33 <kind>`) at non-uniform stride; a fixed 47-byte stride spans only the freeform prefix before the first analytic record.
- **Freeform curve rows:** `60 <tag_u24le> 00 00 00 <face_ref> <face_ref>` (count tracks the freeform-curve subset).
- **`01 00 04 00 <tag_u32le>` alias:** each freeform surface tag has exactly one matching `01 00 04 00 <tag>` link record elsewhere, bridging the tag to its outer-region native geometry (pole net / knots). Vertex tags carry no such alias.

---

## 4. Marker inventory

| Marker                                | Region                     | Meaning                                                                  |
| ------------------------------------- | -------------------------- | ------------------------------------------------------------------------ |
| `FINJPL  `                            | after outer preamble       | starts named stream blocks                                               |
| `7C 02`                               | outer preamble             | total-length-framed source-schema string catalog                         |
| `7C 05` / `7C 08` / `7C 09` / `7C 0A` | outer preamble             | entity table / object-graph root / object records / tagged-atom payloads |
| `7C D9`                               | outer preamble             | literal float-data bytes (not a framed record family)                    |
| `30 04 04 ff`                         | inner body                 | face outer-bound (FBB) spine row marker                                  |
| `10 24 04 ff ff 00 00 00`             | inner body                 | standard edge-table delimiter                                            |
| `05 08 01`                            | inner body / E5 areas      | 15-byte vertex XYZ record (3×f32le)                                      |
| `00 33 30`                            | inner body                 | surface-of-revolution kind tag (geometry in a `b2 03 2d` record)         |
| `00 33 32/33/34/35/38`                | inner body                 | plane / cylinder / cone / sphere / torus kind markers                    |
| `00 33 36/37`                         | inner body                 | line / circle curve kind markers                                         |
| `0x60`                                | inner body                 | per-edge curve-support / edge-incidence row prefix                       |
| `a9 03`                               | outer preamble             | zero-entity native record family                                         |
| `E5 0D 03`                            | preamble or FINJPL segment | E5 native record family                                                  |

---

## 5. Standard nested `V5_CFV2`: topology spine

### 5.1 Positional binding

```text
trim_record[i] -> face_outer_bound_row[i] -> face i
edge_row[i]    -> edge i
vertex_row[i]  -> vertex i
```

Face identity is the native ordinal within the FBB row sequence.

### 5.2 Spine grammar

```text
face_outer_bound_group := (30 04 04 ff <4 raw bytes>){N}  at stride 8
count_header := 01 <kind> <count_u8>  |  01 <kind> ff <count_u32le>
edge_table   := count_header(kind∈{0x01,0x02}) edge_row{count} [delimiter]
edge_row     := 02 <arity_u8> <payload[arity*2]>
delimiter    := 10 24 04 ff ff 00 00 00
vertex_table := count_header(kind=0x06) vertex_record{count}
vertex_record:= 05 08 01 <x_f32le> <y_f32le> <z_f32le>
```

Spine invariants: the face population is the largest contiguous stride-8 `30 04 04 ff` run; edge-row payloads are `u16` handles read **big-endian**; the first and last BE handles of a row are its graph endpoints; the `05 08 01` table is the vertex coordinate source; body count = number of contiguous FBB runs. The FBB row payload is constant across the run, such as `ffffd2d2`, and carries no per-face tag.

### 5.3 Trim records (indexed triangle-mesh packets)

Each `0x4x` trim record precedes the FBB group and corresponds positionally to one face. **It is an indexed triangle mesh**; the face boundary is recovered by triangle-edge incidence cancellation, not from any stored coedge list. `kind = 0x40 | mask`:

| Bit    | Block                           |
| ------ | ------------------------------- |
| `0x01` | `A` independent triangles       |
| `0x02` | `B` triangle strips             |
| `0x04` | `C` triangle fans               |
| `0x08` | a 12-byte `3×f32le` unit vector |

```text
count(x) := <x:u8> | ff <x:u32le>
01 <kind>
[count(A)] [count(B)] [count(C)]
ff <N:u32le>
[vector 3×f32le  if mask & 0x08]
K[0 : B+C]  each count()
H[0 : N]    handles, BIG-ENDIAN, width family-dependent
```

Invariant `N == 3*A + sum(K)`. Handle **width is family-dependent** and is the only varying part: standard meshing uses `u16be`; FBB-spline meshing uses `u24be`. Under the correct width, packets chain end-to-end with zero leftover and land exactly on the FBB spine offset, one packet per face; a wrong width desyncs at the first packet.

The trim handle lane is ordered as the `A` independent-triangle triples, then the `B` triangle-strip lists in `K[0:B]` order, then the `C` triangle-fan lists in `K[B:B+C]` order.

For kinds `49`, `4a`, `4b`, `4c`, `4e`, and `4f`, the optional vector is the plane normal. The planar trim packets and `00 33 32` plane bounds records bind positionally in their respective record orders. The plane origin is the bounds record's bounding-sphere center.

Triangle expansion: independent `(H[3i],H[3i+1],H[3i+2])`; strips alternate winding by parity; fans pivot on `q[0]`. Boundary extraction emits directed edges per oriented triangle. A directed edge whose reverse is absent is a boundary segment; multiplicity-one segments form the exact ordered closed boundary cycles. **Loop count = boundary-cycle count**, with one outer cycle and one cycle per hole. Inner-hole loops require edge-row endpoint ports at their native width: FBB-spline `u24be`, standard `u16be`.

**`0x42 B=2` packed strip lengths:** a `0x42` packet with plain `B`=2 packs its two strip lengths as two `u8` bytes `K0,K1` (`K0+K1==N`) in place of the usual `count()` list. At `u24be` a naive read of those two bytes as a handle over-consumes one byte and desyncs; read them as two `u8`.

### 5.4 Physical-edge identity and port→vertex collapse

Each spine edge row is a handle sequence `E = [p0, interior…, p1]` with endpoint ports `p0,p1`. Match `E` (forward or reversed) against the boundary cycle; if the interior matches a contiguous run, the two flanking cycle handles are corner tokens `c0,c1`. Logical vertices are the connected components of a **union-find** over (edge ports ∪ face-local corner tokens): `union(Port(edge,0),c0); union(Port(edge,1),c1)` on a forward match (swapped on reverse). Edge orientation comes from the recovered boundary path, not from a stored sense bit, and **not** from the order of the two `0x60` face refs.

The FBB-spline edge-table handles share the mesh-boundary handle namespace used by the trim packets. Endpoint ports form a larger namespace than the vertex table; a port handle is not a vertex index.

Resolved physical-edge endpoint pairs constrain the port namespace. For every
port handle, intersect the unordered vertex pairs of all resolved edge rows
carrying that handle; a singleton intersection binds the port to that vertex.
On a resolved row, either bound port binds the other port to the other member
of the pair. A row with two distinctly bound ports acquires their unordered
endpoint pair. Repeat these two operations to a fixpoint. The collapse is valid
only when every row having two bound ports agrees with its independently
resolved endpoint pair; any contradiction invalidates the entire propagation.

#### 5.4.1 Regular trim-motif vertex allocation

Regular-motif bodies serialize vertex allocation as a walk over the ordered trim packets. A column is `(H[0],H[1])` or `(H[N-2],H[N-1])`. Emitting a handle assigns the next `05 08 01` row on its first occurrence and reuses that row thereafter.

- Three opening `4a` packets emit `(packet2.first, packet0.last, packet0.first, packet2.last)` by column.
- The first `42,4a,42` group emits `(strip0.last, strip0.first, quad.first, strip1.first)` by column.
- Each following `4a` transition packet emits its first and last columns.
- A steady-state `42,4a,42` group with `quad.last == strip0.first` and `strip1.last == quad.first` names columns `strip0=(a0,b0),(a1,b1)`, `quad=(c,d),(a0,b0)`, and `strip1=(e,g),(c,d)` and emits handles `(a1,b1,b0,d,g,a0,c,e)`.
- A steady-state `4a,4a` connector emits `(packet1.H0, packet0.H2, packet0.H3, packet1.H1)`.

The walk is complete when its first-occurrence population equals the vertex-table count. Each edge-row endpoint port maps through this allocation. The mapping is valid only when every circle row having exactly two on-circle vertex rows maps to that same unordered pair. Face-local edge incidence then comes from the two `0x60` face references; each face decomposes into closed degree-two endpoint cycles.

Each closed face cycle initially has a whole-cycle reversal gauge. Across a shell, the gauge is fixed by requiring the two coedge uses of every shared physical edge to have opposite traversal senses. The resulting Boolean parity system must be consistent across every connected face component; reversing a face reverses its coedge order, toggles every edge-use sense, and swaps each use's endpoints.

### 5.5 `0x60` curve-support / edge-incidence table

```text
edge_support_row := 60 <tag:u24le> <curve_body> <face_ref> <face_ref>
line:    60 <tag> 00 02 00 33 36 <face_ref> <face_ref>
circle:  60 <tag> 00 12 00 33 37 <cx f32BE> <cy f32BE> <cz f32BE> <r f32BE> <face_ref> <face_ref>
spline:  60 <tag> 00 00 00 <face_ref> <face_ref>
face_ref := <u8>  |  ff <u32le>   (widened when the ordinal needs it)
```

The table has one row per spine edge. Circle center and radius use BE f32. The two trailing references are adjacent face ordinals and form an edge-to-face incidence graph. The `u24` `tag` is a local allocation identifier. The byte sequence `ff 46` encodes a widened face ordinal as `ff <u32le>`.

### 5.6 Curve carrier and endpoint semantics

**Edge endpoints by surface-intersection binding:** an edge lies on both adjacent analytic carriers. Two vertices on both carriers (`on(P,surf)` := signed distance within 1e-3 mm) define the endpoint pair. For a line edge whose faces share a carrier, use the two common vertices collinear with the surface-intersection direction `d` (`plane∩plane`: `n0×n1`; `plane∩cylinder`: axis; `cylinder∩cylinder`: shared ruling).

When more than two vertex rows lie on the same analytic intersection, group unresolved line rows by their unordered adjacent-face pair. Enumerate unordered pairs of common vertex rows whose chord is parallel to the intersection direction. The group resolves when the candidate-pair count equals the line-row count; the rows are interchangeable within the group because their curve kind and adjacent faces are identical, so the assignment has one B-rep up to edge relabeling. Any count mismatch leaves the group unresolved.

**Circle/arc endpoints by support intersection:** intersect the decoded circle (center `c`, radius `r`) with the vertex table (`|dist(v,c)−r| ≤ 1e-3`). Two candidates define the endpoint pair. Coaxial arcs can share a circle and require connectivity or cycle closure. A full circle has antipodal on-circle candidates and uses `start==end`. **Line edges** derive from their endpoints (`origin=start`, `direction=end−start`). Use the mesh-derived port-to-vertex collapse rather than sorted handle rank.

### 5.7 Surface carrier semantics

- **Cylinder axis-frame** from its two parallel equal-radius rim circles: `origin=circle0.center`, `axis=normalize(circle1.center−circle0.center)`, `radius=circle.radius`.
- **Circle plane normal from the adjacent carrier** under per-kind exact on-carrier identity gates (plane: center in plane; cylinder: center on axis and `r==R` ⇒ normal=axis; torus meridional/latitude, cone latitude, sphere section each with an exact identity gate). The gates matter: a center-on-axis circle not on the torus correctly declines.
- **Plane normal** from three non-collinear incident circle centers (cross product) or two non-parallel line directions. A cap closing a cylinder uses the cylinder axis.
- Standard-family geometry uses single-precision storage (`05 08 01` = 3×f32le, `0x60` circles = BE f32). Incidence gates are no tighter than approximately 1e-5 mm.

### 5.8 Analytic surface records in `SurfacicReps`

Interleaved in face order with the 47-byte freeform cores. Grammar: `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>`, record start = `marker_pos − 5`.

| Surface  | kind   | prebyte | length | sign byte |
| -------- | ------ | ------- | -----: | --------- |
| plane    | `0x32` | `0x02`  |     49 | start+48  |
| cylinder | `0x33` | `0x1a`  |     73 | start+72  |
| cone     | `0x34` | `0x1a`  |     73 | start+72  |
| sphere   | `0x35` | `0x12`  |     65 | start+64  |
| torus    | `0x38` | `0x1e`  |     77 | start+76  |

Cylinder and cone share prebyte and length; the kind byte distinguishes them. The last byte stores a per-face orientation sign: `+1=0x01`, `−1=0xff`. For curved surface kinds, the sign defines face sense relative to the canonical normal.

A sequential walker over the SURFACE section terminates on the `0x60` marker. Parse a freeform 47-byte core before an analytic record.

**Standard analytic parameter records** (BE f32 unless noted): sphere `00 12 00 33 35 [cx cy cz radius]`; torus `00 1e 00 33 38 [cx cy cz ax ay major minor]` (`az` from unit norm); cone `00 1a 00 33 34 [apex_x apex_y apex_z ax ay semi_angle]` (`apex` is where radius=0, `az = sign(semi_angle)·sqrt(1−ax²−ay²)`, half-angle `|semi_angle|`); cylinder `00 1a 00 33 33 [px py pz ax ay radius]` (`az` sign carried by the sign of `radius`, radius `|radius|`). Sphere, torus, and cone parameters are inline in the kind record. Plane parameters use a tag-bridged record. Cylinder and torus records carry an LE-f32 witness point at cylinder `+24..+35` and torus `+28..+39`. The witness selects the angular interval containing that point.

**Two-step param→face binding:** param→surface by shared tag (`plane.tag_u24 == prefix.target_u24`), then surface→face positionally (`surface_prefix[i]` → FBB row `i` by ascending offset). `0x60` `curve_support_row[i]` → spine `edge_row[i]` positionally.

### 5.9 Surface-of-revolution record `b2 03 2d`

The `00 33 30` byte is only the kind tag; geometry is a dedicated 174-byte `b2 03 2d` record: `+6` profile-curve ref (u16le), `+8` 12×f64le (axis origin XYZ + three basis vectors), `+104` 4×f64le angular/profile bounds, then scale/flag tail. Three normalized relations hold to f64 bit-equality (`angular_lo/scale==0.5`, `(angular_hi−angular_lo)/scale==2π`, `mean/scale==π+0.5`).

---

## 6. Object-stream record framing (`a5 03` / `a8 03` / `b5 03`)

The object stream is a contiguous run of length-framed records in two width-prefixed families.

```text
A-family (u32 length):  byte0 = 0xA4 + W  (a5/a6/a7 for W=1/2/3), flag 0x03/0x13/0x83
  +2 class  +3 payload_len:u32le  +7 header_token (W bytes)  payload @ +7+W  next @ +7+W+payload_len
B-family (u8 length):   byte0 = 0xB1 + W  (b2/b3/b4), flag bytes 0x03/0x13/0x83
  +2 class  +3 payload_len:u8  +4 header_token (W bytes)  payload @ +4+W  next @ +4+W+payload_len
```

Header width and flag are independent; all three flags occur with width-1 records and wide records retain flag `0x03`. The header token is a small repeating type code, **not** a per-record object id. The frame is length-closed (walking lands exactly on each next record and on the cluster end). A literal marker scan (e.g. `find b"\xa5\x03\x20"`) is both lossy (misses wide-header `a6`/`a7` records, real geometry) and noisy (in-payload coincidences); census by the frame walk, not by marker hits. The compact integer code (shared by `4n+1` header ints and leading operands): read byte `F`: if `F ≡ 1 (mod 4)` value is `(F−1)/4`; if `F = 4·w`, value is the next `w` bytes LE.

### 6.1 `a5 03 34` freeform surface (consolidated class)

Payload: `degU` and `K_U` (`4n+1` codes), array marker (`0x0c` or `0x08 0x09`), `K_U` distinct U knots (f64le), then `degV`/`K_V`/marker/V-knots, a mode byte (`0x01` non-rational / `0x05` rational), the pole grid (nu×nv×3 f64le, 24-byte stride), an optional weight program, and a limit/parameterization tail. **Only distinct knots are stored; multiplicities are an implicit clamped quintic-C2 policy** (`[6,3,…,3,6]` for degree 5), so `n_control = Σmult − degree − 1 = 3·K` (degree 5) or 2 (degree-1). Poles and `05 08 01` vertex coordinates share the identity world frame. Rational weights (mode `0x05`) are a separate compact `02`-run program after the poles (`02` = copy-previous-row; expands a palindromic seed to the full grid). The tail carries current-limits + original parameterization (`param_after = coef·param_before + shift`) and extrapolation flags/data.

The 6-byte `b2 03 2e 01 05 05` record following an `a5 03 34` core is a standalone object.

### 6.2 `a5 03 32` freeform 3D curve / rolling-ball fillet

Frames an explicit 3D spline support (a swept band around a freeform curve). Header (`K`, degree 5, K-repeat, array marker), K distinct knots, then three K×80-byte jet blocks: block A = position `P(t_i)`, B = `C'`, C = `C''`. Each 80-byte site is ten f64: Limit1 `[0:3]`, Limit2 `[3:6]`, Center `[6:9]`, θ `[9]`, satisfying `|Center−Limit1|=|Center−Limit2|` and `θ = 2·arcsin(|Limit2−Limit1|/(2·|Limit1−Center|))`. Reconstruction is fit-free (per-span quintic Hermite→Bezier). The same jet reconstructs the **rolling-ball fillet surface** `P(t,φ) = Center(t) + Rot(Limit1(t)−Center(t), axis, φ·θ(t))`. Per-site radius classifies constant vs variable-radius. Not universal: a part with freeform edges can carry zero `a5 03 32` records.

### 6.3 `a5 03 20` pcurve-on-surface (`CATPSpline`)

A 2D spline in a support surface's parameter space. Its payload stores `degU`, `K` definition points, one global curve parameter, one `(U,V)` pair per point, and 2D tangent and second derivative values. The leading operand `op1` is an absolute persistent CGM object tag. The `mode/offset` byte encodes the leading-extrapolation count `q = (mode−1)/4`.

`b2 03 20` is the B-family form with the same support reference, degree, knot-site parameters, U/V values, first derivatives, second derivatives, and native range. Wide `a6/a7` and `b3/b4` records use identical payload grammars.

### 6.4 Consolidated guide and topology metadata

- **`a5 03 39`** stores `K`, degree, repeated `K`, distinct knots, three `K × 6 × f64le` blocks, and a 48-byte tail. Each position-site pair is two triples `(P,Q)` satisfying `|Q−P|=1`; `P` is a guide-curve point and `Q−P` is its unit reference direction. The next two blocks contain first and second derivatives of all six channels.
- **`b2 03 18`** stores surface-chart data after a two-byte payload prefix. Length `0x12` is `(u,v)`, `0x1a` is `(station,u,v)`, and `0x2a` is five unsplit f64 values.
- **`b2 03 37`** stores a compact-int persistent-tag reference list followed by f64 `1.0`; its payload length is `0x22`, `0x24`, or `0x26`.
- **`b2 03 3b`** has payload length `0x20`: compact references followed by f64 angular scale and cone half-angle.
- **`b2 03 23`** stores `[lo,hi,eps, lo,hi,1.0, lo,hi,eps]` as nine f64 values. The repeated range is the native parameter interval shared by the two preceding pcurves.

### 6.5 `b2 03 19/28/29/31/30/60` support and construction records

- **`b2 03 19`** stores a compact record id and five f64 values `(c1,c2,radius,arc_lo,arc_hi)`. The interval is arc length; a full circle satisfies `arc_hi−arc_lo=2πr`.
- **`b2 03 28`** analytic cylinder support: layout `0x5a` stores origin, a frame token (`0x19` = stored 2-vector is the axis; `0x1c` = it is the ref direction and `axis=(vy,−vx,0)`), radius, u/v ranges, and terminator. Layout `0x52` implies `axis=+x`, `ref=+y`. Layout `0x62` adds a phase tail but does not determine a complete carrier frame. Resolved cylinders use the arc-length chart `u = radius·angle(P about axis from ref), v = (P−origin)·axis`.
- **`b2 03 29`** stores apex, two transverse unit vectors, axis, half-angle, angular offset, slant range, and angular scale. Its chart is `P(U,V)=apex+V·(cos(half)·axis+sin(half)·(cos(U/scale)·T1+sin(U/scale)·T2))`.
- **`b2 03 31`** constant-offset support: `(carrier ref, signed offset d, carrier UV sub-domain box)`, defining `O(u,v) = S(u,v) + d·n(u,v)` reconstructed from the carrier's analytic NURBS partials; the offset is part-characteristic (a constant wall thickness).
- **`b2 03 30`** construction-use wrapper with a `kind` discriminant: `0x01` offset (byte-equal to `b2 03 31`), `0x19` offset-of-fillet (`R_eff = R_support + |d|`), `{0x05,0x15}` variable-radius domain wrappers.
- **`b2 03 65`** is the constant group separator `81 03 05 0d`.
- **`b2 03 60`** is a two-compact-integer typed group opener. The first integer is the group id; type `3` opens a cylinder chain. Following `<pre> 03 28 5a <compact id>` frames carry the same 90-byte cylinder payload as standalone layout `0x5a` and belong to the type-3 group until the next opener.

### 6.6 `a8 03` common object-stream freeform class

Frame: `a8 03 <cls> <payload_len:u32le @+3> <object_id:u32le @+7> <payload @+11>`. The family stores an inline `object_id` at `+7`, explicit multiplicity vectors, mixed degrees, and an inline rational weight grid after the poles. `a8 03 32` stores a 3D curve and `a8 03 20` stores a pcurve.

**In-stream object-id resolver:** `a8 03` and `b5 03` records hold an inline `object_id`; references are compact tokens selecting an id width (`18`→u16, `38`→u24). Binding is an **in-stream walk** (index `object_id → record` while walking; resolve each ref), not a byte-offset directory. The `object_id` is a dense creation-order ordinal (monotonic with offset, with clean segment resets), so ids can equivalently be assigned by counting objects.

### 6.7 Object-stream topology (`b5 03`)

Object records occur in length-closed runs containing both common-form A8 and
B5 frames. Starting at a frame boundary, advance by the declared frame length;
the next byte is either another A8/B5 frame or the run terminator. Marker bytes
inside an accepted frame payload do not start records. Repeated byte-identical
typed records with the same object id are one object.

- `b5 03 5f` (per-face node): first ref token names the surface (`b5 03 27` plane / `28` cylinder / `2d` revolution / `a8 03 34` bspline), resolved through the object-id map. The bspline subset binds injectively to `a8 03 34`. The `5f` stream rank is the native face ordinal.
- `b5 03 62` (loop node): payload `<0x80 + n_refs> (pcurve_ref edge_ref)* surface_ref <tail>`, `n_refs` odd. Member ref tokens use a positional-byte mask: `08 <lo>`, `10 <mid>`, `18 <lo><mid>`, `20 <page>`, `28 <lo><page>`, `30 <mid><page>`, and `38 <lo><mid><page>`. Omitted id bytes are zero; present bytes occupy bits 0–7, 8–15, and 16–23 respectively. The tail begins with `<0x80 + edge_count>`; its remaining topology metadata does not alter member identity or order.
- `b5 03 21` (pcurve): `catia_support_ref` is the owning surface's `object_id` directly. The 3D edge is the pcurve lifted through the surface (plane / cylinder arc-length `θ=u/r` / surface-of-revolution / bspline), and the clamped end poles land on `05 08 01` vertices to f32 round-trip.
- `b5 03 18` (analytic line pcurve): mode `01` payload `81 surface_ref 01 <u0:f64le> <v0:f64le> <du:f64le> <dv:f64le> <t0:f64le> <t1:f64le>` defines `P(t) = (u0,v0) + t(du,dv)`. Mode `05` payload `81 surface_ref 05 <u:f64le> <v0:f64le> <v1:f64le>` defines the isoparametric line from `(u,v0)` to `(u,v1)`. Mode `09` payload `81 surface_ref 09 <v:f64le> <u0:f64le> <u1:f64le>` defines the transverse isoparametric line from `(u0,v)` to `(u1,v)`. Intervals are increasing. The equivalent clamped B-spline has degree 1, endpoint knots with multiplicities `[2,2]`, and endpoint poles equal to the interval endpoints.
- `b5 03 5f` (face node): dominant payload `<0x80 + n_refs> surface_ref loop_ref... <05-tail>`; the first reference is the carrier and the remaining references are owned loops.
- `b5 03 27/28/2d` analytic surfaces: plane origin+normal, cylinder origin+axis+radius, revolution axis origin+direction.

For `b5 03 2d`, surface U is the referenced `0e` line or `0f` arc profile parameter and surface V is stored revolution arc length. The revolution angle is `V/gauge_radius`. Reversing a negative gauge together with the axis leaves the surface invariant and gives an increasing angular interval. An exact rational NURBS representation uses degree-2 angular spans of at most π/2 with middle weight `cos(Δθ/2)`; its U knots are the profile knots and its V knots remain in the stored arc-length coordinate.

**Object-stream topology:** `b5 03 5f` nodes are faces in native ordinal order. Distinct `b5 03 5e` identifiers referenced by loop nodes are physical edges. Each `b5 03 62` node defines one loop and stores its ordered edge occurrences. A paired pcurve lifted through its carrier defines the edge curve, and its endpoints coincide with `05 08 01` vertex loci. The fixed serialized loop sequence has exactly one head-to-tail endpoint-sense assignment when it represents a closed boundary. Pcurve degree and carrier identify lines, circles, and B-splines. An object-stream file contains one body. The 3D edge geometry uses f32 endpoint coordinates, and native pcurves have degree 1 or 2.

Plane lifting is affine: every pcurve pole `(u,v)` maps to `origin + u·direction_u + v·direction_v`, preserving degree, knots, and weights. On a cylinder, constant U with varying V is an axis-parallel line; constant V with varying U is a circle centered at `origin + V·axis`, with cylinder axis, reference direction, and radius. Other pcurve/carrier compositions retain the pcurve-on-surface construction until an exact solved 3D carrier is available.

A degree-1 cylinder pcurve with both U and V varying is a circular helix. Its angular interval is `[U0/r,U1/r]`; its axial rise per radian is `(V1−V0)/(U1/r−U0/r)`, so the pitch vector per full turn is `2π·rise_per_radian·axis`. Reversing the two endpoints to order the angular interval does not change the curve. The helix radius vectors are `r·reference_x` and `r·(axis×reference_x)` and its radial growth is zero.

A constant-U or constant-V pcurve on a tensor-product NURBS surface, including an exact revolution cache in the native chart, is the corresponding exact surface isocurve. Fixing U contracts each V-column in homogeneous coordinates by the U basis values; fixing V contracts each U-row by the V basis values. The varying direction retains its degree, knot vector, and periodic flag. Each resulting control point is the contracted homogeneous numerator divided by its contracted weight.

Coincident `05 08 01` rows share an endpoint locus. For topology subsets whose
allocation identity is otherwise unresolved, the locus binds to the lowest
serialized matching row; a loop is emitted only when the resulting ordered edge
sequence has exactly one closed head-to-tail sense assignment. Faces or loops
with unresolved references, endpoint lifts, or chain sense remain outside the
connected graph.

---

## 7. Outer schema and object records

### 7.1 `7C02` source-schema catalogs

```text
catalog := 7C 02 <total_len:u32le> <count:atom> entry{count-1}
entry   := <inclusive_len:u8> <ascii[inclusive_len-1]>
```

`total_len` includes the marker and length field. The entries consume the frame exactly. The first four entries are `CATCatalogManager`, `catalogManager`, `catalogLinks`, and the empty string. The catalog names source classes and fields available to the serialized object schema; a name does not declare an object instance.

### 7.2 `7C08` object graph

A nested total-length tree rooted at `7C 08`; each `7C09` object holds a lead-coded head and a `7C0A` tagged-atom payload. It is the **feature/object-ownership layer**, not the expanded face→loop→coedge table or the port→vertex collapse. `7C09` head: `<lead> 01 <owner_ref> <class_ref> [storage_ref]`; references are compact record ordinals, class refs are per-file prototype ordinals rather than global type codes. `7C0A` atoms: compact `0x80..0xD0` (value = byte−0x80), raw `0x51..0x7F`, **paged** `0xD1..0xE4 <byte>` (value = `(prefix−0xD1)·256 + byte + 1`, consumes 2 bytes, **not** little-endian-widened), escaped `0x80 <u32le>`. E5 blobs inside `7C0A` are templated descriptor records (≈59 or 46 bytes), not NURBS payloads. The class30 pair records and `76 ac 7f`-delimited handle table are coedge/half-edge sub-tables, not the port→vertex relation.

### 7.3 `7C0B` value blocks

```text
value_block := 7C 0B <declared_len:u32le> <payload[declared_len-6]> FE 7C 02 ...
```

`declared_len` measures from the `7C0B` marker through the byte before the terminator. The complete block occupies `declared_len + 1` bytes. The trailing `FE` is followed immediately by the associated `7C02` source-schema catalog.

---

## 8. Zero-entity `a9 03` variant

Record framing `a9 03 XX YY <payload[YY+8]>`, `record_length = YY + 12`; records reference each other by **global record ordinal** into the `a9 03` stream.

Record families: `5f 0c` face (24 B), `5e 1a` edge-stride (38 B), `62 xx` edge-loop, `06 38` coedge (68 B, two per edge), `5d 06` vertex marker, `25 69` edge side-pair header, `21 71` curve-support-on-surface, `27 6a` plane, `28 8a` cylinder-family, `29 b8` cone-family, `2b c8` circle/arc/torus, `34 c8`/`34 5e` bspline carriers, `05 0b/10/15` vertex-incidence.

- **`62xx` loop** is an alternating even/odd lane; `edge_count = (flag_at_+12 − 0x81)/2`. The even lane satisfies `A[j] = T − g − j`. **Loop-class byte = location:** `0x50` = inner (hole) loop, `0x41`/`0xc1` = non-inner; the `0x50` count equals the hole count. The outer loop is first, followed by inner loops in ascending terminal-id order.
- A face-family record with counted references `[R0, R1, ..., Rm]` defines ordered loop terminals `T[j] = R0 - R[j+1]`. Concatenate loops in face-record order and each loop's members in serialized order. For an owned `21xx` support occurrence with local slot `s` at `+12`, its first-lane loop member is `A = T - s`. `A` identifies a face-local support occurrence rather than a global `0638` identifier.
- **Coedge sense** is a packed 3-bit-per-coedge stream after the reference lane: code 7 = forward and code 2 = reversed relative to the stored edge direction. The `0638` `(1,2)` byte identifies the positional twin; the `62xx` stream stores orientation.
- A `2569` side-pair header supplies base columns `[B0, B1]`. Its two following `0638` records carry side numbers `1` and `2`; the side-slot pair is `(B0 + side, B1 + side)`. This pair identifies an oriented use in the `2569`/`0638` topology namespace and does not directly address a `21xx` support or `05xx` vertex item.
- **Carrier run = per-face surface:** a carrier (`276a`/`288a`/`29b8`/`2bc8`) followed by a maximal run of `21xx` supports; face order aligns 1:1. Surface kind is in the payload f64, not the tag.
- **Zero-entity edge carrier:** a `21xx` coedge's f64 tail is `(u0,v0,u1,v1)` on its owner-run carrier. Lift per kind: plane direct-UV; cylinder `θ=u/radius`; cone `u` is the angle directly; torus `θ=u/R` and `φ=v/r`. The two lifted UV pairs define the edge endpoint coordinates. Bspline carriers `34c8`/`345e` store the **full NURBS pole grid inline** (`34c8` 7×7 @+167, `345e` 5×7 @+141).
- **`2bc8` carrier kind:** `major≠minor` is a torus. `major==minor` is a degenerate horn torus.
- Edge curve kind: two coaxial surfaces of revolution intersect in circles (exact theorem); a plane cuts a cylinder in a circle (⊥), lines (∥), or ellipse (oblique): classify per `|cos∠(plane_normal, cyl_axis)|`.

The `5e1a` edge-stride, `0638` coedge-twin, `2569` side-pair header, and `2171` support head have the layouts described above. The `2171` f64 tail stores `(u0,v0,u1,v1)` at `+93`, `+101`, `+109`, and `+117`.

---

## 9. E5 `0D 03` stream variant

Framing `E5 0D 03 <cls> <sub> <payload_size_u16le> 00 00 00 <record_id_u32le> <payload>`, stride `payload_size + 13`, from the preamble or the strongest FINJPL walk. Reference tokens: hi-bit byte `b`→`b−0x80`; `08 <lo>`→lo; `10 <hi>`→hi<<8; `18 <lo><hi>`→u16le.

Classes: `0x01` body, `0x00` advanced face, `0x08` datum/template face, `0x09` edge loop, `0x0d` reference bundle, `0x0e` parameter-bound, `0x96`/`0x97` UV line/circle pcurve, `0xa0` complex/spline pcurve, `0xc0`/`0xc1` boundary/intersection curve support, `0xc8`/`0xc9`/`0xca`/`0xcc` plane/cylinder/cone/torus carrier, `0xfe` vertex, `0xff` trimmed edge-use.

**Topology:** a class-`0x01` body references one class-`0x08` root whose counted face roster names the body's class-`0x00` faces. A face is `<0x81 + loop_count> <surface_ref> <loop_ref>* <01 00>`: loop location is structural (`loop_count==1` simply bounded, `>1` = 1 outer + `loop_count−1` holes; `Σ(loop_count−1)` = part hole count). A loop is `<0x81 + 2*edge_count> (pcurve_ref edge_use_ref)* surface_ref`. An edge-use (`0xff`) is `85 <curve_support_ref> <start_vertex> <end_vertex> <param_start> <param_end>`. The paired pcurve must occur in the referenced `0xc0`/`0xc1` support. **Vertex ref → index** is sorted-ref-rank. The binding is valid only when each edge endpoint identity closes against decoded bytes: evaluating the paired pcurve through its referenced surface, or evaluating an explicit 3D curve carrier, yields the two mapped `05 08 01` coordinates within f32 precision.

**E5 orientation** for non-digon loops is `absolute_sense = g_loop × relative_chain_sense`, where `relative_chain_sense` is the unique head-to-tail vertex-chain closure of the fixed cyclic member list. The per-loop sign `g_loop` follows manifold coherence: the `0xff` edge-sharing graph is a closed 2-manifold, every edge is referenced by exactly two loop members, and shared edges satisfy `g_A·g_B = −r_A·r_B`. One global sign per `0xff`-coherent component follows majority `face_trailer_sign` alignment. `ref_aligned_signs[1]` stores loop role: `+1` is outer and `−1` is inner.

**E5 surface carriers** use these byte layouts: plane `0xc8` 90 B, cylinder/circle `0xc9` 137 B, cone `0xca` 185 B, and torus `0xcc` 201 B. **Edge curve descriptors** evaluate pcurves on their carriers: cylinder isoparametric curves yield circles or lines, torus isoparametric curves yield circles, and cone isoparametric curves yield circles. Torus and cone boundary UV pcurves are co-parameterized to the 3D edge angle parameter. The `0xa0` UV jet encodes a constant-speed circular arc with degree-5 C2 grammar; a square Hermite solve recovers `P/D/DD`.

E5 `0x96` p-curves store `<surface_ref>, origin_u, origin_v, dir_u, dir_v, param_lo, param_hi` as f64 values. E5 `0x97` p-curves store `<surface_ref>, center_u, center_v, radius, param_lo, param_hi` with two intervening u32 fields. Cylinder `0x96` U is arc length (`U_angle=U_native/radius`); torus U and V are arc lengths (`U_angle=U_native/major_radius`, `V_angle=V_native/minor_radius`). `0xc0` is a one-pcurve boundary support and `0xc1` is a two-pcurve intersection support. Edge type follows `0xff -> 0xc0/0xc1 -> pcurve -> carrier`.

E5 carrier frames use f64 fields. `0xc9` stores origin, `frame_u`, `frame_v`, radius, and angular/arc data, with `axis = frame_u × frame_v`. `0xca` stores origin, `frame_u`, `frame_v`, axis, angle, reference radius, UV bounds, and the native-U scale at `+158`. `0xcc` stores origin, `frame_u`, `frame_v`, axis, major radius, minor radius, and UV bounds.

An E5 `0xa0` UV jet is a nonperiodic degree-5 C2 B-spline p-curve. Its knot-site position, first derivative, and second derivative determine the B-spline poles through a square Hermite system. Duplicating each interior knot yields the local quintic Bezier controls.

The E5 root `0x08` sign tape contains one face-aligned sign for each class-`00` face, followed by two additional signs. The two trailing signs have no assigned semantic role.

For plane-carrier `0xa0` cases, evaluating the UV jet on its `0xc8` plane produces the same 3D point set as the primitive circle supplied by the paired `0x96` view. The native `0xa0` parameter is not an affine primitive-circle angle parameter.

---

## 10. FBB-only partial-spine variant

A nested-`V5_CFV2` file with a valid FBB face group and `05 08 01` vertices whose post-FBB edge tables use `u24be` handles (vs the standard family's `u16be`) across **two** edge tables (`kind=0x01` then `kind=0x02`) separated by the delimiter.

```text
edge_table := 01 <kind∈{0x01,0x02}> count(row_count) edge_row{row_count}
edge_row   := 02 count(arity) <arity × handle:u24be>
post_fbb   := edge_table(kind=0x01) delimiter edge_table(kind=0x02) delimiter vertex_table
```

The two tables end at the vertex table. Their combined row count equals the `0x60` curve-support count. The `u24be` width preserves record boundaries. The concatenation binds in row order to the `0x60` table: where `0x60_row[i]` is a line, `FBB edge_row[i].arity == 2`. The table split carries no line-versus-curve meaning. The bound `0x60` row provides edge kind, adjacent faces, circle center, and radius. Surface intersection resolves endpoints between analytic faces.

Each FBB-only `u24be` edge-row handle is a trim-mesh boundary-vertex handle. Each row matches a contiguous forward or reversed recovered trim-boundary run, and its analytic curve comes from the positionally bound `0x60` row. Endpoint-port to logical-vertex collapse remains separate.

---

## 11. Float-packed inner-no-FBB variant

A nested-`V5_CFV2` file without a `30 04 04 ff` spine uses the object-stream `b5 03` grammar (§6.6) for topology. A pure-marker path stores `00 33 3X` surface-kind markers and identifies one face per marker without loop or edge topology.

---

## 12. Units & tolerances

- **Length unit is millimetres.** No unit word or scale slot is stored. `05 08 01` vertices, pole grids, and surface origins use world-frame coordinates. Do not scale coordinates.
- **Angles are radians**; cone/torus half-angle is `|semi_angle|` (the stored sine/semi-angle sign is a frame bit, not magnitude). Knot parameters, pcurve `(U,V)` parameters, and surface-parameter tails are dimensionless and are never scaled.
- **Storage precision is variant-dependent, and it sets the incidence gate:**

  | Family                                      | Coordinate storage                                                                              | Effective precision   |
  | ------------------------------------------- | ----------------------------------------------------------------------------------------------- | --------------------- |
  | Standard nested / FBB-only / float-packed   | `05 08 01` = 3×f32le; `0x60` circle center/radius = BE f32; `SurfacicReps` freeform cores = f32 | ~1e-5 mm              |
  | Object-stream (`a5 03` / `a8 03` / `b5 03`) | poles/knots/jet sites = f64le (24-byte pole stride); revolution `b2 03 2d` = f64le              | full f64              |
  | Zero-entity (`a9 03`)                       | surface + pcurve parameter tails = f64le                                                        | full f64              |
  | E5 (`0D 03`)                                | carrier layouts exact; edge endpoints round-trip through f32 `05 08 01`                         | ~1e-5 mm at endpoints |

  A single-precision family stores geometry to ~1e-5 mm; incidence gates cannot be tighter than the source storage precision.

- **On-carrier incidence tolerance is 1e-3 mm.** Endpoint-by-surface-intersection binding uses `on(P,surf)` := signed distance ≤ 1e-3 mm (§5.6); circle/arc vertex matching uses `|dist(v,c) − r| ≤ 1e-3` (§5.6); per-coordinate cylinder rim/axis identity gates and the plane/cylinder/torus/cone/sphere on-carrier gates (§5.7) run at the same order.
- **Normalized-relation checks are exact (f64 bit-equality)** where the source is f64: the surface-of-revolution angular relations (`angular_lo/scale == 0.5`, `(angular_hi−angular_lo)/scale == 2π`; §5.9) and the `SurfacicReps` bounding-sphere containment `|f[i]−f[6+i]| + f[3+i] ≤ f[9]` (§3.5) hold to bit-equality, not within a tolerance band.

---
