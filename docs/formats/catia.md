# Dassault Systèmes CATIA V5 `.CATPart`: Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

All multi-byte integers are little-endian unless explicitly marked **BE**. Float coordinates are in millimetres.

---

## 1. Variant families

A file stores its geometry in one of six families; the family determines the record grammar.

| Variant                          | Detection                                                                                                            | Geometry source                                                                               |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| **Standard nested `V5_CFV2`**    | Outer file contains a nested `V5_CFV2` container, no coherent overriding E5 stream                                   | Inner-body BREP spine, trim mesh records, `00 33 <kind>` surface markers, `05 08 01` vertices |
| **FBB-only partial spine**       | Nested `V5_CFV2` with contiguous FBB face rows + `05 08 01` vertices but no standard edge-row table                  | FBB face group + vertex records; post-FBB edge rows and trim `H` handles share a selected `u8`, `u16be`, or `u24be` width |
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

`FINJPL  ` (two trailing spaces) marks named stream blocks after the outer preamble. The four bytes after the marker are the big-endian type word. When the following bytes are `<name_length:u32be> 00 <printable-ASCII name>`, they define the segment's primary name. Every segment ends at the next `FINJPL  ` marker or the containing body boundary, and the complete bounded bytes are retained under their offset, type, family, and optional primary name. An E5 stream candidate is coherent when at least ten records walk by their declared strides. A coherent preamble wins; otherwise the segment with the largest valid walk wins, with storage type `0x0000008e` breaking equal-count ties.

A project-flags segment with type word `0x01010003` contains the summary-information fields. Its JPEG preview is the complete marker stream from `ff d8` SOI through `ff d9` EOI. The JPEG start-of-frame segment supplies pixel width, height, and component count. JPEG signatures outside this segment family are not previews.

The `LastSaveVersion` summary field stores ASCII values delimited by `<Version>`/`/<Version>`, `<Release>`/`/<Release>`, `<ServicePack>`/`/<ServicePack>`, `<BuildDate>`/`/<BuildDate>`, and `<HotFix>`/`/<HotFix>`. Repeated identical tuples are one saved-by version; conflicting tuples do not define a governing version.

An external-document storage property in a project-flags segment is the atom sequence `34 12 "CATStorageProperty" 80 01 00 00 00 00 22 0c 00 00 00 34 01 01 00`, `34 10 "CATUnicodeString" a0 02 00 00 00 00`, `34 05 "CATIA" 9f a0 02 00 00 00 00`, then `34 <length:u8> <ASCII target> 9f`. Targets ending in `.CATPart`, `.CATProduct`, `.CATShape`, or `.cgr`, compared case-insensitively, are external CATIA document references. Each reference retains the target-string offset and the identity of its containing project-flags segment.

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

- **Vertex roster:** trailing run of 7-byte records `54 <tag_u24le> 00 00 00`, with unique, strictly increasing tags. Roster row `i` names counted vertex-coordinate row `i`; edge endpoint identities use these tags directly.
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

Spine invariants: the face population is the unique largest contiguous stride-8 `30 04 04 ff` run; shorter marker runs are not members of that population. Equal-largest runs do not identify one governing face population. Edge-row payloads are big-endian handles. A standard-spine row's first and last handles are graph endpoint ports. An FBB-only row uses its family's selected `u8`, `u16`, or `u24` width and its first and last handles are the endpoints of its complete trim-boundary run. The counted `01 06` table is the vertex coordinate source. Only the declared `05 08 01` rows in that table are vertices; identical signatures outside the counted table are payload bytes. The FBB row payload is constant across the run, such as `ffffd2d2`, and carries no per-face tag.

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

Invariant `N == 3*A + sum(K)`. Handle **width is family-dependent** and is the only varying part: standard meshing uses `u16be`; FBB-only meshing uses the width selected by its post-FBB edge tables. Under the correct width, packets chain end-to-end with zero leftover and land exactly on the FBB spine offset, one packet per face; a wrong width desyncs at the first packet.

The trim handle lane is ordered as the `A` independent-triangle triples, then the `B` triangle-strip lists in `K[0:B]` order, then the `C` triangle-fan lists in `K[B:B+C]` order.

For kinds `49`, `4a`, `4b`, `4c`, `4e`, and `4f`, the optional vector is the face frame normal. Frame vectors come only from the unique complete width-selected trim chain; incidental packet signatures outside that chain do not contribute. A plane face takes the vector from its index-aligned trim packet. Its face-roster carrier tag selects the `00 02 00 33 32` bounds record with the same tag. The plane origin is that bounds record's bounding-sphere center. Plane bounds record order does not define frame-vector binding.

Triangle expansion: independent `(H[3i],H[3i+1],H[3i+2])`; strips alternate winding by parity; fans pivot on `q[0]`. Boundary extraction emits directed edges per oriented triangle. A directed edge whose reverse is absent is a boundary segment; multiplicity-one segments form the exact ordered closed boundary cycles. **Loop count = boundary-cycle count**, with one outer cycle and one cycle per hole. Inner-hole loops require edge-row endpoint ports at their family-selected width.

**`0x42 B=2` packed strip lengths:** a `0x42` packet with plain `B`=2 packs its two strip lengths as two `u8` bytes `K0,K1` (`K0+K1==N`) in place of the usual `count()` list. At `u24be` a naive read of those two bytes as a handle over-consumes one byte and desyncs; read them as two `u8`.

### 5.4 Physical-edge identity and port→vertex collapse

Standard `u16be` edge rows are handle sequences `E = [p0, interior…, p1]` whose endpoint ports `p0,p1` are outside the trim-handle namespace. Match the interior forward or reversed against a contiguous boundary run; the two flanking cycle handles are corner tokens `c0,c1`. The interior match fixes boundary coverage but does not order `p0,p1` against `c0,c1`; that endpoint gauge is selected by the closed face-boundary quotient and correlated endpoint-pair domains. FBB-only rows instead contain the complete same-width trim-boundary run, including its endpoint handles; an arity-two row directly covers one boundary segment. Logical vertices are the connected components of a **union-find** over edge ports and face-local corner tokens. For a complete FBB-only run, its first and last boundary handles are its corner tokens. Edge orientation comes from the recovered boundary path and endpoint quotient, not from a stored sense bit or the order of the two `0x60` face refs.

The row arity uses the table count form: values below `0xff` occupy one byte; `ff <count:u32le>` carries a widened cardinality. Long discretized boundary rows use the widened form.

Every matched occurrence is represented by `(edge row, face, boundary cycle, first segment, segment count, reversal)`. A standard row with `m` interior handles covers `m+1` boundary segments beginning at the segment before the interior match. A complete FBB row with `n` handles covers `n−1` boundary segments beginning at its first matched handle.

A standard `u16be` edge row with no interior handles does not match every boundary position. After all non-empty interiors are matched, maximal uncovered boundary-segment runs are retained separately from the incident edge rows having no occurrence on that face. Assigning those rows to an uncovered run requires additional endpoint or carrier constraints; empty-interior matching alone establishes no position, order, or orientation.

The placement domain for unmatched rows contains only assignments that partition every uncovered run end-to-end. Each assignment groups one placement for every missing edge use; its placements remain correlated and cannot be selected independently. Row arity fixes a trim-boundary span only when the stored interior handles match a contiguous boundary run. Every unmatched row may cover any positive remaining span because its stored curve discretization and the face trim tessellation may use different sample counts. Face incidence determines the eligible rows; span partitions do not determine edge orientation.

When every missing row has one endpoint pair, head-to-tail endpoint adjacency partitions the rows into trails. A trail binds to the uncovered run whose start and end corner domains contain its oriented endpoints and whose positive segment spans partition the run length. This endpoint-trail binding is exact for any number of missing rows and precedes exhaustive placement enumeration.

Merging an assignment with the exact interior-handle occurrences produces one ordered physical-edge sequence per trim cycle. Every boundary segment is covered exactly once. A standard occurrence retains its matched coverage span but its endpoint-port direction remains unresolved until endpoint constraints select it. A complete-run FBB occurrence retains the direction of its matched endpoint handles.

Selecting one assignment per face and one direction per unresolved occurrence defines the logical-corner quotient independently of coordinate rows. Consecutive uses in a trim cycle share a face-local corner. Each occurrence maps those two corners to the physical edge row's start and end ports according to its direction. Equal physical ports across the two incident occurrences collapse their face-local corners into one logical vertex.

When a face has one surviving positional assignment, intersect the port-corner equations induced by all of that assignment's surviving direction choices. Every equation in the intersection is independent of the unresolved direction choice and is merged into the initial quotient before global selection.

Boundary orientation is a parity problem over the selected face boundaries. The two uses of every physical edge must traverse it in opposite directions after applying an optional reversal to each complete boundary. A partial selection whose boundary-parity graph contains an inconsistent cycle is invalid. Apply the solved boundary reversals to the completed coedge graph before emitting topology.

Within one FBB face group, physical-edge incidence classifies the body. A face component is closed when every physical edge it uses has two uses. A group containing only closed components is solid, one containing only open manifold components is sheet, and one mixing closed and open components or containing an edge with more than two uses is general. Faces connected through a shared physical-edge row form one region and shell; disconnected face components remain separate regions and shells of the inferred body. Every physical edge belongs to at least one face boundary in exactly one FBB group; an unused edge or an edge shared by separate groups invalidates the grouping.

Initialize the quotient with two distinct endpoint ports per physical edge row. The endpoint integers stored in a standard edge row are occurrence-local names and do not establish equality with an endpoint integer in another row. Exact mesh occurrences and face-boundary corner equations collapse these initial ports.

Assignments with the same ordered physical-edge rows and the same resolved occurrence directions induce the same logical-corner quotient. Differences confined to boundary-segment allocation do not create distinct quotient choices. Positional assignments remain distinct while deriving trim-corner endpoint constraints.

Boundary rotation, reversal with every resolved occurrence direction inverted, and permutation of separate boundaries leave the face's port-corner equations unchanged. These are one quotient choice. Canonicalize each cycle over all rotations and both traversal directions, sort the cycle signatures, and retain the first serialized representative.

Each physical port initially admits every coordinate row present in one of its edge's endpoint-pair candidates. An edge with no local endpoint predicate admits the complete coordinate-row table. Collapsing two ports intersects their coordinate domains. Reject an empty intersection immediately. A complete quotient is valid only when it has one component per coordinate row and the component domains have one bijective coordinate assignment modulo rows storing the same exact coordinate.

Port-domain intersections preserve physical-edge pair correlation. For every edge with an explicit pair domain, retain in each current port component only coordinate rows participating in a pair with a row retained by the opposite component. Repeat this reduction across all edges to a fixpoint. A diagonal pair remains admissible while the two ports occupy distinct partial-quotient components because later corner equations can merge them. Equal endpoint ports encode a closed edge: both endpoint positions bind the same logical vertex, and its admissible domain consists only of diagonal pairs `[v,v]`. Apply the fixpoint after every corner merge and on the complete quotient.

Coordinate binding is one joint constraint problem over the complete quotient. Before the final bijection, assign every provisional component to a coordinate row, require every coordinate row to be used, and require every edge with an explicit unordered endpoint-pair domain to select one stored pair across its assigned components. When this surjective assignment is unique, provisional components assigned to the same row collapse. The resulting logical-vertex components bind bijectively to coordinate rows. A closed edge selects one diagonal pair. Coordinate rows with equal stored XYZ form one coordinate class whose capacity is its row population. The binding is resolved only when the capacity-constrained class assignment has exactly one solution; permutations among rows of the same class do not change vertex placement. Uniqueness of the independent component domains is insufficient.

Every consecutive edge pair in a candidate boundary requires a supported port adjacency. The possible traversal-end ports of the first use and traversal-start ports of the second use depend on their resolved or unresolved directions. At least one such port pair must have intersecting coordinate domains in the current quotient. A face assignment with an unsupported adjacency cannot extend that quotient.

Resolved matched edges constrain trim-cycle corner points by unordered endpoint pairs. Intersect the pair sets at shared corners, then propagate a singleton corner across its edge to the opposite endpoint. An unmatched placement whose start and end corners are bound contributes their unordered point pair to that edge's endpoint domain. A placement-derived domain is complete only when every retained placement has both corners bound; incomplete corner coverage does not narrow the edge.

Endpoint constraints prune complete face assignments atomically. A placement domain intersects the edge's resolved endpoint pair and the complete placement domain on its opposite incident face. Repeat assignment removal and domain intersection to a fixpoint. An unbound placement contributes no endpoint restriction; it does not invalidate its face assignment.

An endpoint-pair candidate is retained only when every incident face has a complete boundary assignment whose ordered edge uses admit a closed head-to-tail traversal containing that pair. Propagate supported pairs through each cycle in both directions, union support across alternative assignments on one face, intersect support across the edge's distinct incident faces, and repeat to a fixpoint.

The standard `u16be` endpoint integers are not vertex indices or reusable port identities. Each row contributes two occurrence-local ports even when another row stores the same endpoint integer. Boundary-run corner unions establish all cross-row port identity. In an FBB complete-run table, the first and last handles occupy the table's mesh-boundary namespace: equal endpoint handles within that counted table identify one port. The delimiter resets the namespace, so equal handles in separate tables remain distinct until trim-cycle corner equations collapse them. Cross-face logical-vertex collapse is applied after complete-run identity is established.

The endpoint components induced by exact face-local run matches are provisional. Distinct occurrence components map to one logical vertex when their physical edge endpoints carry the same native endpoint identity, even when adjacent faces use different trim handles. Native endpoint identities are global within the topology; each edge's ordered identity pair is in physical edge-row direction and replaces its provisional corner pair. Coordinate-row indices therefore cannot be assigned directly from component ordinals; the native-identity quotient precedes coordinate binding.

Native edge records and the vertex roster may cover only a subset of the physical edge table. Each covered edge binds independently: its two endpoint tags select coordinate rows through the positional vertex roster. These exact partial bindings propagate through the trim-mesh endpoint quotient. A complete trim-mesh propagation supersedes equality in the table-local port namespaces because the mesh quotient includes the cross-table corner unions.

After exact local-tag bindings consume their native edge records, a remaining native edge may bind an unmatched standard row by endpoint incidence. Map its two native vertex identities through the positional roster, require both coordinate rows to belong to that standard row's surface-constrained endpoint domain, and compare unordered pairs. The row binds only when all matching unused native edges reduce to one distinct unordered coordinate-row pair. Zero or multiple distinct pairs leave the row unresolved.

The co-stored `b5 03 5d` identity graph supplies a world-space locus for a native endpoint identity when its parameter incidences lift through their carriers. That locus binds a `05 08 01` coordinate row only when exactly one row lies within the identity's stored tolerance, with a `2e-3` mm floor. Bound identities seed the global port-to-coordinate fixpoint; ambiguous or absent locus matches remain unbound.

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

Each closed face cycle initially has an independent whole-cycle reversal gauge, including each outer and inner boundary of a multiply connected face. A closed edge is a one-coedge boundary whose start and end vertex are identical. Across a shell, the gauges are fixed by requiring the two coedge uses of every shared physical edge to have opposite traversal senses. The resulting Boolean parity system must be consistent across every connected boundary component; reversing a boundary reverses its coedge order, toggles every edge-use sense, and swaps each use's endpoints.

### 5.5 `0x60` curve-support / edge-incidence table

```text
edge_support_row := 60 <tag:u24le> <curve_body> <face_ref> <face_ref>
line:    60 <tag> 00 02 00 33 36 <face_ref> <face_ref>
circle:  60 <tag> 00 12 00 33 37 <cx f32BE> <cy f32BE> <cz f32BE> <r f32BE> <face_ref> <face_ref>
spline:  60 <tag> 00 00 00 <face_ref> <face_ref>
face_ref := <u8>  |  ff <u32le>   (widened when the ordinal needs it)
```

The table begins immediately after the complete face-local surface roster and has one row per spine edge. Circle center and radius use BE f32. The two trailing references are adjacent face ordinals and form an edge-to-face incidence graph. The `u24` `tag` is a local allocation identifier. When both `tag+1` and `tag+2` occur in the positional vertex roster, they are the row's two endpoint allocation identities in that order. Checked successor arithmetic and membership of both identities are required; one successor alone establishes no endpoint. A resolved native edge record's second and third references are also the row's native endpoint identities. Shared identities constrain analytic and spline rows to one injective vertex-coordinate assignment. The byte sequence `ff 46` encodes a widened face ordinal as `ff <u32le>`.

The allocation-local curve reference in a consolidated class-`5e` historical edge run is not a standard-row `tag`. Equal numeric values across those namespaces do not bind a historical run to a final standard edge row.

Equal trailing face ordinals retain one incidence with that face and leave the second incidence slot unresolved. When the edge row has exact trim-boundary occurrences on two distinct faces, those occurrences supply the two incidences and the non-repeated face fills the second slot. Assign all remaining unresolved slots simultaneously. The unique assignment is the one in which every used face vertex has degree two, every face decomposes into closed endpoint cycles, every edge has an admissible placement in each incident trim boundary, and at least one stored endpoint pair lies on each candidate face carrier. Trim placement is required only to distinguish multiple endpoint-closed assignments; a unique endpoint-closed assignment already fixes the incidence. Assignments that differ only by permuting rows with the same unordered endpoint identities and the same analytic curve carrier are one edge-identity gauge. Zero or multiple non-equivalent assignments invalidate the incidence graph.

A co-stored `b5 03 5e` edge whose object id equals the standard row `tag` supplies additional face-incidence identity. For each owning B5 face, match its face object id against the standard roster carrier identity: `target` for an analytic record and `tag` for a freeform record. A carrier identity binds a face only when it occurs once in the roster. For a repeated standard face ordinal, exactly one distinct matched owner face fills the unresolved slot before endpoint-cycle completion; absent, repeated, or conflicting matches supply no incidence.

The spline row identifies a required 3D curve carrier even when its spline definition is not resolved. It is not a curveless topological edge: the adjacent faces, native endpoint identities, and trim incidence belong to an opaque curve occurrence retained with the B-rep payload.

### 5.6 Curve carrier and endpoint semantics

**Edge endpoints by surface-intersection binding:** an edge lies on both adjacent analytic carriers. Two vertices on both carriers (`on(P,surf)` := signed distance within 1e-3 mm) define the endpoint pair. For a line edge whose faces share a carrier, use the two common vertices collinear with the surface-intersection direction `d` (`plane∩plane`: nonzero `n0×n1`; `plane∩cylinder`: axis; `cylinder∩cylinder`: shared ruling). Coincident planes provide no intersection direction; incidence closure selects among their common vertex pairs.

A face-local freeform core whose aliased carrier is not typed contributes no surface-membership predicate. Its known adjacent carrier, serialized curve support, endpoint identities, and closed incidence constraints continue to constrain the edge; an unknown carrier never means that a vertex is off the surface.

When more than two vertex rows lie on the same analytic intersection, group unresolved line rows by their unordered adjacent-face pair. Enumerate unordered pairs of common vertex rows whose chord is parallel to the intersection direction and whose midpoint lies on both carriers. When the analytic pair has no single intersection direction, the midpoint-on-both-carriers test selects its straight branches. Candidate pairs use lexicographic order and rows use serialized order; equal ranks fix the first stable edge-identity gauge when equivalent assignments differ only by permuting rows with the same curve kind and adjacent faces. Resolve ambiguous endpoint pairs by selecting the edge with the fewest currently feasible pairs. Reject a partial assignment when a face-vertex degree exceeds two or a degree-one incidence cannot be completed by an unassigned edge. A complete assignment is valid only when every face has degree two at every used vertex, decomposes into closed cycles, and the shell-wide radial orientation equations are consistent.

Endpoint alternatives are resolved as complete face-closing graphs before trim-mesh port orientation. Each graph is then checked against the exact trim quotient. Circular occurrences with the same carrier and owning face use their witness-selected angular intervals: exact coincident intervals are permitted as seam occurrences; otherwise their open interiors are pairwise disjoint. Distinct endpoint graphs must reduce to one topology modulo logical-vertex labels, intrinsic edge direction, and boundary-cycle start.

When a spline row has no decoded 3D carrier, or a line row has no locally decisive analytic branch, its endpoint domain is every unordered pair of distinct vertex rows incident to both adjacent carriers. Native endpoint identity is injective over this domain. The selected pairs must satisfy the same complete face-cycle and radial-orientation constraints; an unconstrained local pair is never emitted directly.

Same-incidence spline rows with the same complete bipartite endpoint relation bind by allocation rank: partition the relation into its two vertex-row sets, order each set by vertex allocation, and pair equal ranks with rows in serialized edge order. Singleton relations already fixed by an adjacent edge on either shared face are excluded before testing completeness. Same-incidence circle rows with one identical carrier and one identical endpoint relation bind the relation's lexicographically ordered pairs to serialized edge rows by equal rank when their cardinalities match. An incomplete relation or cardinality mismatch establishes no binding.

**Circle/arc endpoints by support intersection:** intersect the decoded circle (center `c`, radius `r`) with the vertex table (`|dist(v,c)−r| ≤ 1e-3`). Two candidates define the endpoint pair. Coaxial arcs can share a circle and require connectivity or cycle closure. A full circle has antipodal on-circle candidates and uses `start==end`. **Line edges** derive from their endpoints (`origin=start`, `direction=end−start`). Use the mesh-derived port-to-vertex collapse rather than sorted handle rank.

**Analytic occurrence pcurves** are the inverse image of the bound edge endpoints in the owning face chart. Plane coordinates are orthogonal projections onto `(u_axis, normal×u_axis)`. Cylinder and cone coordinates are azimuth and axial distance; the cone's tangential component is divided by its elliptic ratio before `atan2`. Sphere coordinates are azimuth and latitude. Torus coordinates are major and minor azimuth. Periodic endpoint coordinates unwrap across the shortest congruent interval. A parameter-space segment transfers only when its lifted midpoint lies on the serialized line or circle within `2e-3` mm. Plane-circle images transfer as exact piecewise rational quadratic arcs.

### 5.7 Surface carrier semantics

- **Cylinder axis-frame** from its two parallel equal-radius rim circles: `origin=circle0.center`, `axis=normalize(circle1.center−circle0.center)`, `radius=circle.radius`.
- **Circle plane normal from the adjacent carrier** under per-kind exact on-carrier identity gates (plane: center in plane; cylinder: center on axis and `r==R` ⇒ normal=axis; torus meridional/latitude, cone latitude, sphere section each with an exact identity gate). The gates matter: a center-on-axis circle not on the torus correctly declines.
- **Plane normal** from three non-collinear incident circle centers (cross product) or two non-parallel line directions. A cap closing a cylinder uses the cylinder axis.
- Standard-family geometry uses single-precision storage (`05 08 01` = 3×f32le, `0x60` circles = BE f32). Incidence gates are no tighter than approximately 1e-5 mm.

### 5.8 Analytic surface records in `SurfacicReps`

Interleaved in face order with the 47-byte freeform cores. Grammar: `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>`, record start = `marker_pos − 5`.

Cylinder and torus payloads store a face-side witness point as three little-endian f32 values immediately after their big-endian carrier parameters: cylinder at marker-relative `+27`, torus at `+31`. For a cylinder section circle, exactly one complementary endpoint interval contains the witness azimuth. For a torus isoparametric circle, exactly one complementary interval of its varying periodic chart coordinate contains the corresponding witness coordinate. That interval is the face's angular branch; the witness's constant chart coordinate need not equal the boundary circle's constant coordinate.

A generated standard line carrier begins at its stored start vertex, has unit direction toward its end vertex, and uses the distance interval `[0, |end-start|]`. A standard circle carrier uses radians in its orthonormal reference frame. An incident cylinder or torus witness selects the angular branch when it lies strictly inside exactly one complementary interval. A witness at an interval endpoint supplies no branch constraint and preserves the canonical unwrapped interval. Every incident witness that selects a branch must select the same oriented interval after conversion into the circle frame.

A derived face pcurve exists only when both physical edge endpoints lie on that face's carrier. Analytic inverse parameterization does not project an off-carrier endpoint into UV space. At a cone apex the angular parameter is singular; a generator ending at the apex uses the other endpoint's angular coordinate and the apex's axial coordinate. A trim-mesh endpoint allocation that is topologically valid but fails this carrier-incidence invariant retains its coedge without a pcurve.

A standard spline edge with two distinct adjacent face carriers is their exact intersection construction. The neutral construction uses the ordered adjacent-face pair and an endpoint-normalized `[0,1]` interval. When the row tag is also the unique object identity of a class-`5e` edge, the edge's first reference names a class-`23` wrapper whose two counted references name the ordered class-`20` support pcurves. Each pcurve's support resolves through class-`37` result-carrier constructions and class-`38` aliases. Equal pcurve ranges replace the normalized interval and retain the exact degree-5 UV jets on their resolved neutral support surfaces. A same-face spline row is an exact line when its two endpoints lie on the carrier and their segment is a cylinder generator parallel to the axis or a cone generator passing through the apex; the line uses distance parameterization over the endpoint interval. The serialized 3D spline cache remains attached as an opaque carrier until its pole and knot program resolves; it does not replace an exact construction.

| Surface  | kind   | prebyte | length | sign byte |
| -------- | ------ | ------- | -----: | --------- |
| plane    | `0x32` | `0x02`  |     49 | start+48  |
| cylinder | `0x33` | `0x1a`  |     73 | start+72  |
| cone     | `0x34` | `0x1a`  |     73 | start+72  |
| sphere   | `0x35` | `0x12`  |     65 | start+64  |
| torus    | `0x38` | `0x1e`  |     77 | start+76  |

Cylinder and cone share prebyte and length; the kind byte distinguishes them. The last byte stores a per-face orientation sign: `+1=0x01`, `−1=0xff`. For curved surface kinds, the sign defines face sense relative to the canonical normal.

A sequential walker over the SURFACE section consumes exactly one fixed-length analytic record or 47-byte freeform core per face and terminates on the first `0x60` curve-support row. The unique contiguous chain fixes face-carrier order; raw `00 33` signature order does not, because identical signatures also occur inside parameter payloads.

**Standard analytic parameter records** (BE f32 unless noted): sphere `00 12 00 33 35 [cx cy cz radius]`; torus `00 1e 00 33 38 [cx cy cz ax ay major minor]` (`az = sign(major)·sqrt(1−ax²−ay²)`, major radius `|major|`); cone `00 1a 00 33 34 [apex_x apex_y apex_z ax ay semi_angle]` (`apex` is where radius=0, `az = sign(semi_angle)·sqrt(1−ax²−ay²)`, half-angle `|semi_angle|`); cylinder `00 1a 00 33 33 [px py pz ax ay radius]` (`az` sign carried by the sign of `radius`, radius `|radius|`). Sphere, torus, and cone parameters are inline in the kind record. Plane parameters use a tag-bridged record. Cylinder and torus records carry an LE-f32 witness point at cylinder `+24..+35` and torus `+28..+39`. The witness selects the angular interval containing that point.

The reconstructed analytic axis is normalized after recovering `az`; this removes f32 unit-sphere roundoff while preserving the stored axis direction and hemisphere. A tag-bridged plane normal is normalized under the same rule. A zero or non-finite reconstructed direction does not define a carrier.

**Two-step param→face binding:** param→surface by shared tag (`plane.tag_u24 == prefix.target_u24`), then surface→face positionally (`surface_prefix[i]` → FBB row `i` by ascending offset). `0x60` `curve_support_row[i]` → spine `edge_row[i]` positionally.

### 5.9 Surface-of-revolution record `b2 03 2d`

The `00 33 30` byte is only the kind tag; geometry is a dedicated 174-byte `b2 03 2d` record: `+5` reference token (`08` or `0a`), `+6` profile-curve ref (u16le), `+8` 12×f64le (axis origin XYZ + three basis vectors), `+104` 4×f64le angular/profile bounds, then scale/flag tail. Three normalized relations hold to f64 bit-equality (`angular_lo/scale==0.5`, `(angular_hi−angular_lo)/scale==2π`, `mean/scale==π+0.5`).

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

Object-stream `05 08 01` coordinate rows are unframed allocations outside the complete A/B and B5/A8 record ranges. Marker-like bytes within a length-closed payload are payload data and do not participate in endpoint-locus binding.

### 6.1 `a5 03 34` freeform surface (consolidated class)

Payload: `degU` and `K_U` (`4n+1` codes), array marker (`0x0c` or `0x08 0x09`), `K_U` distinct U knots (f64le), then `degV`/`K_V`/marker/V-knots, a mode byte (`0x01` non-rational / `0x05` rational), the pole grid (nu×nv×3 f64le, 24-byte stride), an optional weight program, and a limit/parameterization tail. **Only distinct knots are stored; multiplicities are an implicit clamped quintic-C2 policy** (`[6,3,…,3,6]` for degree 5), so `n_control = Σmult − degree − 1 = 3·K` (degree 5) or 2 (degree-1). Poles and `05 08 01` vertex coordinates share the identity world frame. Rational weights (mode `0x05`) are a separate compact `02`-run program after the poles (`02` = copy-previous-row; expands a palindromic seed to the full grid). The tail carries current-limits + original parameterization (`param_after = coef·param_before + shift`) and extrapolation flags/data.

The 6-byte `b2 03 2e 01 05 05` record following an `a5 03 34` core is a standalone object.

### 6.2 `a5 03 32` freeform 3D curve / rolling-ball fillet

Frames an explicit rolling-ball surface jet. Header (`K`, degree 5, K-repeat, array marker), K distinct knots, then three K×80-byte blocks carrying values, first derivatives, and second derivatives. Each aligned row has ten f64 channels: Limit1 `[0:3]`, Limit2 `[3:6]`, Center `[6:9]`, θ `[9]`, satisfying `|Center−Limit1|=|Center−Limit2|` and `θ = 2·arcsin(|Limit2−Limit1|/(2·|Limit1−Center|))`. Every scalar channel is reconstructed fit-free by the stored per-span quintic Hermite jet. The surface is `P(t,φ) = Center(t) + Rot(Limit1(t)−Center(t), normalize((Limit1(t)−Center(t))×(Limit2(t)−Center(t))), φ·θ(t))`. The complete aligned jet transfers as a procedural carrier with multiplicity six at every piece boundary; no NURBS fitting or constant-radius reduction is permitted. Per-site radius classifies constant versus variable-radius instances. A part with freeform edges can carry zero `a5 03 32` records.

### 6.3 Consolidated class-`0x20` pcurve-on-surface (`CATPSpline`)

A 2D spline in a support surface's parameter space. Its payload stores `degU`, `K` definition points, one global curve parameter, one `(U,V)` pair per point, and 2D tangent and second derivative values. The leading operand `op1` is an absolute persistent CGM object tag. The `mode/offset` byte encodes the leading-extrapolation count `q = (mode−1)/4`.

`b2 03 20` is the B-family form with the same support reference, degree, knot-site parameters, U/V values, first derivatives, second derivatives, and native range. Wide `a6/a7` and `b3/b4` records use identical payload grammars. Terminal `0x07`, optionally followed by one zero padding byte, exhausts every framed form.

For a co-parametric edge block, each uniquely bound analytic carrier lifts every pcurve definition site into the common edge parameterization. A NURBS carrier with signed normal offset `d` lifts a site as `S(u,v) + d·normalize(Su(u,v)×Sv(u,v))`; a degenerate tangent pair has no lift. All liftable sides have equal site counts and their index-aligned 3D sites agree within `2e-3` mm. Disagreement invalidates the shared locus sequence rather than selecting one side. Its first and last sites are the shared 3D endpoint loci.

A complete consolidated edge run is the contiguous framed sequence `F:20, F:20, B:23, B:06, B:06, B:5e`, where both class-`20` records use the same A or B family. The first two records are its side pcurves, `23` is their shared parameter packet, the two `06` records are the side uses in the same order, and `5e` is the native edge node. In the five-reference `5e` layout the references identify, in order, the 3D curve support, start vertex, end vertex, start endpoint parameter, and end endpoint parameter. Curve and parameter references use compact integers. Each vertex reference independently uses a compact integer, `06 <u8>`, `0a <u16le>`, or a raw byte that is not a compact token. An intervening framed record terminates the run.

More generally, an immediately adjacent B-family class-`23`, class-`24`, or class-`25` record is the edge-definition frame owned by a `B:06, B:06, B:5e` run. The relation is structural: an intervening framed record leaves the use run without an edge-definition frame. Each definition retains its width, flag, header token, class, and complete class-specific payload.

Class-`23` and class-`24` scalar edge definitions have payload `82 <compact operand> <compact operand> <persistent operand> <eight or nine f64le scalars>`. The persistent operand uses the compact grammar or `0a <u16le>`. The scalar lane consumes the remainder of the frame, every value is finite, and scalar 2 equals the final scalar. The nine-scalar class-`23` form additionally carries three equal `(start,end,tolerance)` triples with unit scalar 5. A compact class-`24` definition has payload `81 <compact operand> 0f 87` and no scalar lane; the preceding width-coded frame header token is separate.

A class-`25` scalar definition begins `82 <allocation operand> <allocation operand> <persistent operand>`. An allocation operand uses the compact grammar, `06 <u8>`, `0a <u16le>`, or one raw nonzero byte congruent to 2 or 3 modulo 4. The persistent operand uses the compact grammar, `0a <u16le>`, or `0b <u16le>`; the two explicit leads remain distinct encodings of the same numeric lane. The remaining payload is either 7, 8, 9, or 10 finite `f64le` scalars, or five finite `f64le` scalars followed by a marker and a finite trailing scalar lane. Marker `82` carries 5–7 trailing scalars, `83` carries 8–9, `89` carries 20, and `8b` carries 24. The marker is a scalar-lane boundary, not part of either adjacent scalar.

A class-`25` descriptor edge run is the contiguous B-family sequence `18,25,06,06,5e`. Its class-`18` payload is `08 <identity:u16le> <control> <two or three f64le scalars>`, where control is `02` or `0a` and every scalar is finite. The descriptor is owned by that edge run only when the adjacent class-`25` payload closes under its scalar grammar.

An analytic circle edge run is the contiguous B-family sequence `18,19,23,06,06,5e`. The exact class-`18` descriptor and typed class-`19` arc-length circle are the analytic carrier of the edge whose class-`23` definition has eight scalar lanes. Any intervening framed record breaks the carrier relation.

Every structurally complete `a5/a6/a7` or `b2/b3/b4 03 20` jet is retained as a typed native pcurve record when its support reference or edge run remains unbound. Once bound to a support surface and edge occurrence, the equivalent neutral degree-5 NURBS uses sixfold piece boundaries and retains the native evaluation range. Support identity and leading-extrapolation count remain attached to the native record.

A `B:06` side-use payload ending in `0x84` or `0x88` stores `0x80+n`, exactly `n` compact allocation references, then that terminal sense byte. The counted vector must consume the complete preceding payload; a non-closing payload has no decoded reference vector. The two terminal values are the two edge-use senses. For a complete five-reference edge run whose node curve reference is `c`, the two use-reference vectors are `(c−2,c−1)` then `(c−1,c)`. Checked subtraction is required. The node's final two references are the allocation-local endpoint selectors `(2,1)` and do not occupy the use chain. The use vectors do not identify the owning faces.

The start/end references form a global native vertex-identity namespace across complete edge runs. Equal references join incident physical edges independently of endpoint coordinates. Curve-support references are local to each run allocation and repeat across distinct physical edges; serialized run occurrence identifies the graph edge. Connected components of this graph partition disconnected edge populations before face ownership is applied.

Every complete run is retained as a typed native historical edge record referencing the two typed pcurve records by identity. It retains the shared range and tolerance, both use vectors and senses, endpoint identities, local selectors, and terminal node layout byte.

### 6.4 Consolidated guide and topology metadata

- **`a5 03 39`** stores `K`, degree, repeated `K`, distinct knots, three `K × 6 × f64le` blocks, and a 48-byte tail. Each position-site pair is two triples `(P,Q)` satisfying `|Q−P|=1`; `P` is a guide-curve point and `Q−P` is its unit reference direction. The next two blocks contain first and second derivatives of all six channels. For degree 5, the first three channels of the position and derivative blocks define the exact guide curve. Each knot span becomes one quintic Bézier segment using its two endpoint positions, first derivatives, and second derivatives; adjacent segments join in one non-rational clamped B-spline with multiplicity six at every stored knot.
- **`b2 03 18`** stores surface-chart data after a two-byte payload prefix. Length `0x12` is `(u,v)`, `0x1a` is `(station,u,v)`, and `0x2a` is five unsplit f64 values.
- **`b2 03 37`** stores a compact-int persistent-tag reference list followed by f64 `1.0`; its payload length is `0x22`, `0x24`, or `0x26`.
- **`b2/b3/b4 03 62` owner packet:** `0x89`, exactly nine alternating strong/weak identities, and a 62-byte numeric tail. Two reference encodings occur. If the first strong token is `0x0a`, all five strong identities use `0x0a <u16le>` and the four weak identities use compact integers. Otherwise the five strong identities use width-coded compact integers and the four weak identities are raw one-byte values. Reference widths determine the payload length; the nine identities and tail consume the complete frame. The tail is `<header:5B> <lower:[f64le;2]> <upper:[f64le;2]> 01 <bound0:[f32le;2]> <bound1:[f32le;2]> <bound2:[f32le;2]>`. Every scalar is finite, both coordinates of `lower` are strictly less than the corresponding coordinates of `upper`, and each binary32 bound is strictly increasing. Header bytes zero and four are `0x84` and `0x0d`; header byte one is `0x41` or `0xc1`.
- **Count-framed `b2/b3/b4 03 62` owner packet:** `0x80+n`, exactly `n` persistent identities, then a nonempty class-specific tail. Persistent identities use the compact integer grammar or `0x0a <u16le>`. The count fixes the reference-lane boundary.
- **Counted `b2/b3/b4 03 61` family:** `0x80+n`, exactly `n` compact values, then a nonempty class-specific tail ending in `0x03`. Long `61` records without the leading count use a separate payload grammar.
- **Long `b2/b3/b4 03 61` family:** eight prefix bytes, `0x06`, a nonempty strictly increasing `u16le` member lane, `0xfe`, five `0x0a <u16le>` persistent identities, one finite f64le scalar, and terminal `0x03`. The fixed 25-byte suffix determines the member-lane boundary.
- **`b2/b3/b4 03 5f` link:** payload `82 <width-coded target> 03 05`. The target uses the compact integer grammar; its encoded width determines the payload length.
- **Owner allocation link:** when a `5f` link is immediately followed by a `62` owner packet, the owner's final identity equals `5f.target + 1` (the fixed nine-reference form uses its ninth identity). Both checked successor identity and framed adjacency are required; an intervening record breaks the link.
- **`b2 03 3b`** has payload length `0x20`: compact references followed by f64 angular scale and cone half-angle.
- **`b2 03 23`** stores `[lo,hi,eps, lo,hi,1.0, lo,hi,eps]` as nine f64 values. The repeated range is the native parameter interval shared by the two preceding pcurves.

### 6.5 `b2 03 19/28/29/31/30/60` support and construction records

- **`b2 03 19`** stores a compact record id and five f64 values `(c1,c2,radius,arc_lo,arc_hi)`. The interval is arc length; a full circle satisfies `arc_hi−arc_lo=2πr`.
- **`b2 03 28`** analytic cylinder support: layout `0x5a` stores origin, a frame token (`0x19` = stored 2-vector is the axis; `0x1c` = it is the ref direction and `axis=(vy,−vx,0)`), radius, u/v ranges, and terminator. Layout `0x52` implies `axis=+x`, `ref=+y`. Layout `0x62` stores origin, token `0x0e`, a unit 2-vector, scalar `1`, radius, strictly increasing u/v ranges, `0x03`, and a finite phase scalar. Its u span does not exceed one circumference. Resolved cylinders use the arc-length chart `u = radius·angle(P about axis from ref), v = (P−origin)·axis`.
- **`b2 03 29`** stores apex, two transverse unit vectors, axis, half-angle, angular offset, slant range, and angular scale. Its chart is `P(U,V)=apex+V·(cos(half)·axis+sin(half)·(cos(U/scale)·T1+sin(U/scale)·T2))`.
- **`b2 03 31`** constant-offset support: token `05`, a compact `08 <u16le>` or `0c <u24le>` carrier reference, signed `d:f64le`, then `[u0,v0,u1,v1]:4×f64le`. It defines `O(u,v) = S(u,v) + d·n(u,v)` reconstructed from the carrier's analytic NURBS partials. The referenced consolidated carrier is the unique NURBS whose parameter domain contains the box and whose V-knot lane contains both `v0` and `v1` within `1e-3`. The construction preserves the carrier's U/V senses and has no extension flags.
- **`b2 03 30`** construction-use wrapper with a `kind` discriminant: `0x01` offset (semantically identical to `b2 03 31` and resolved through the same carrier-domain rule), `0x19` offset-of-fillet (`R_eff = R_support + |d|`), `{0x05,0x15}` variable-radius domain wrappers. The wrapper payload ends after its four kind-specific f64 scalars.
- **`b2 03 65`** is the constant group separator `81 03 05 0d`.
- **`b2 03 60`** is a two-compact-integer typed group opener. The first integer is the group id; type `3` opens a cylinder chain. Following `<pre> 03 28 5a <compact id>` frames carry the same 90-byte cylinder payload as standalone layout `0x5a` and belong to the type-3 group until the next opener.

### 6.6 `a8 03` common object-stream freeform class

Frame: `a8 03 <cls> <payload_len:u32le @+3> <object_id:u32le @+7> <payload @+11>`. The family stores an inline `object_id` at `+7`, explicit multiplicity vectors, mixed degrees, and an inline rational weight grid after the poles. `a8 03 32` stores the same complete degree-5 rolling-ball value/derivative jet and exact procedural surface as the consolidated `a5 03 32` form, followed by a fixed 59-byte tail. Its knots are strictly increasing. The endpoint multiplicities are six and every interior multiplicity is one or three. `a8 03 20` stores a pcurve.

For `a8 03 34`, the lead byte, U degree/flags/distinct knots/multiplicities, V degree/flags/distinct knots/multiplicities, and mode form a complete parameter-lattice header. The pole counts are `sum(multiplicities) - degree - 1` independently in U and V. Header validity is independent of whether the following pole representation is the inline XYZ grid.

The elided-pole form places the fixed 141-byte range/affine/extrapolation tail immediately after the mode byte. The byte after that tail is the end of the `a8` frame or the first owned A/B-family child record. It carries no inline XYZ pole grid or rational-weight grid. Its external pole allocation is an unframed `nu×nv` XYZ grid, followed by the rational-weight grid when `mode=0x05`, occupying the complete gap between a length-closed `b5 03 21` pcurve and the next A/B-family frame. Grid cardinality comes from the elided surface header. A grid binds only when its byte length, finite coordinate payload, and following frame boundary select one allocation.

**In-stream object-id resolver:** `a8 03` and `b5 03` records hold an inline `object_id`; references are compact tokens selecting an id width (`18`→u16, `38`→u24). Binding is an **in-stream walk** (index `object_id → record` while walking; resolve each ref), not a byte-offset directory. The `object_id` is a dense creation-order ordinal (monotonic with offset, with clean segment resets), so ids can equivalently be assigned by counting objects.

### 6.7 Object-stream topology (`b5 03`)

Object records occur in length-closed runs containing both common-form A8 and
B5 frames. The flag byte is `03`, `13`, or `83`; topology-bearing frames use
`03`, while alternate-flag records can bridge adjacent topology frames in the
same run. Starting at a frame boundary, advance by the declared frame length;
the next byte is either another A8/B5 frame or the run terminator. Marker bytes
inside an accepted frame payload do not start peer records. An A8 wrapper may
own a nested length-closed B5 run; that run is walked recursively within the A8
payload boundary. Repeated byte-identical typed records with the same object id
are one object. A unique isolated geometry frame is admitted when a retained
`5f` or `62` topology record references its object id; an unreferenced isolated
frame is not part of the topology graph. Surface classes `2c`, `2e`, `30`,
`38` retains its identity and payload as an opaque carrier, so
their faces and loops remain connected without assigning unsupported geometry.

- `b5 03 5f` (per-face node): first ref token names the surface (`b5 03 27` plane / `28` cylinder / `2d` revolution / `a8 03 34` bspline), resolved through the object-id map. The bspline subset binds injectively to `a8 03 34`. The `5f` stream rank is the native face ordinal.
- `b5 03 62` and wide-header `a8 03 62` (loop node): payload `<n_refs> (pcurve_ref edge_ref)* surface_ref <edge_count> <tail>`, `n_refs = 2*edge_count+1`. A cardinality below 128 is one byte `80+n`; larger cardinalities use the same positional-byte token as an object id. Member ref tokens use a positional-byte mask: `08 <lo>`, `10 <mid>`, `18 <lo><mid>`, `20 <page>`, `28 <lo><page>`, `30 <mid><page>`, and `38 <lo><mid><page>`. Omitted id bytes are zero; present bytes occupy bits 0–7, 8–15, and 16–23 respectively. A single byte in `80..ff` names object id `byte - 80`. The remaining tail begins with `05`; its topology metadata does not alter member identity or order.
- `b5 03 21` (pcurve): `catia_support_ref` is the owning surface's `object_id` directly. The 3D edge is the pcurve lifted through the surface (plane / cylinder arc-length `θ=u/r` / surface-of-revolution / bspline), and the clamped end poles land on `05 08 01` vertices to f32 round-trip.
- `a8/b5 03 20` (degree-5 pcurve jet): the common object-stream payload stores a width-coded support reference, degree, strictly increasing distinct knots and multiplicities, a channel-mode byte, then UV position, first-derivative, and second-derivative channels and an increasing native range. Terminal `0x07` exhausts the frame. Endpoint multiplicities are six and every interior multiplicity is three. The support reference uses the object-stream reference token grammar, including the split 24-bit `28 lo hi` form. Channel modes are `4n+1` codes carrying the same complete Euclidean UV jet. Each adjacent knot pair defines the exact quintic Bézier poles from its two endpoint jets; knot multiplicity is six at every piece boundary in the equivalent piecewise representation.
- An `a8 03 20` jet transfers to the pcurve arena when its object-stream topology is not reference-closed. Its object id, support reference, channel mode, explicit multiplicities, and native evaluation range remain attached to the exact piecewise carrier.
- `b5 03 18` (analytic line pcurve): mode `01` payload `81 surface_ref 01 <u0:f64le> <v0:f64le> <du:f64le> <dv:f64le> <t0:f64le> <t1:f64le>` defines `P(t) = (u0,v0) + t(du,dv)`. Mode `05` payload `81 surface_ref 05 <u:f64le> <v0:f64le> <v1:f64le>` defines the isoparametric line from `(u,v0)` to `(u,v1)`. Mode `09` payload `81 surface_ref 09 <v:f64le> <u0:f64le> <u1:f64le>` defines the transverse isoparametric line from `(u0,v)` to `(u1,v)`. Intervals are increasing. The equivalent clamped B-spline has degree 1, endpoint knots with multiplicities `[2,2]`, and endpoint poles equal to the interval endpoints.
- `b5 03 19` (analytic circle pcurve): payload `81 surface_ref <center_u:f64le> <center_v:f64le> 05 05 <radius:f64le> <t0:f64le> <t1:f64le> <orientation:f64le> <phase:f64le>`, with positive radius, increasing arc-length interval, and `orientation` equal to `-1` or `+1`. Its angle is `phase + orientation*t/radius`. Split the interval into angular spans of at most π/2; each span is an exact rational quadratic with middle weight `cos(Δangle/2)`, while its knots remain in the stored arc-length parameter.
- `b5 03 1a/1d` (conic pcurves): both begin `81 surface_ref`. Class `1a` then stores `center_u:f64le center_v:f64le 05 05 diameter_u:f64le diameter_v:f64le conjugate_angle:f64le parameter_start:f64le parameter_end:f64le orientation:f64le period:f64le`. Every scalar is finite. The support reference binds the record to its loop surface. A class-`1a` circular conic has `conjugate_angle = π/2`, `orientation ∈ {-1,1}`, `parameter_start < parameter_end`, and `period = π hypot(diameter_u, diameter_v)`. Its radius is half the diameter-vector length, its zero-station direction is the normalized diameter vector, and station `s` has angle `orientation 2πs/period`. This curve is an exact rational quadratic arc over the stored parameter interval.
- A sphere class-`1d` pcurve stores `u_min:f64le u_max:f64le v_min:f64le v_max:f64le 05 81 chart_shift:f64le direction:f64le zero:f64le 1d radius:f64le slope:f64le reciprocal_scale:f64le phase:f64le zero:f64le`. `direction ∈ {-1,1}`, `reciprocal_scale = -direction/radius`, and both zero fields are zero. The radius equals the sphere radius. Its chart bounds satisfy `[u_min,u_max] = radius·[azimuth_min,azimuth_max]` and `[v_min,v_max] = [chart_origin,chart_origin + 2π radius]`. It is the great circle whose sphere-local coordinates satisfy `tan(latitude) = slope·cos(azimuth − geometric_phase)`, where `geometric_phase = chart_shift/radius + phase`.
- `b5 03 2e` (freeform surface alias): the complete payload is either one reference token or cardinality `81` followed by one reference token, naming an `a8 03 34` freeform surface. The alias and target identify the same surface carrier and parameter chart.
- `b5 03 38` (surface-chart alias): the complete payload is `81 surface_ref 05 05 09`. The alias and target identify the same surface carrier and parameter chart.
- `b5 03 2c` (extrusion surface): payload `81 directrix_ref <direction:3f64le> <u0:f64le> <u1:f64le> <u_scale:f64le> <u_origin:f64le> <v0:f64le> <v1:f64le> 05 05`. `direction` is unit length, both intervals are increasing, `u_scale=1`, and `u_origin=0`. The native chart is `directrix(V) + U·direction`; the stored U interval bounds the extrusion and the V interval equals the directrix's solved parameter range.
- `a8 03 25` (extrusion directrix): payload `82 support_wrapper_ref pcurve1_ref <sampled-cache> <t0:f64le> <t1:f64le> <fit_tolerance:f64le> 01`. The parameter interval is increasing and the cache tolerance is positive. `support_wrapper_ref` names a class-`24` record with payload `81 pcurve0_ref 81 01 <t0:f64le> <t1:f64le> <zero:f64le> 01`. The two pcurves and their distinct native ranges define an exact two-surface intersection on their referenced supports; `[t0,t1]` is the solved-curve range. The sampled cache remains approximate and does not replace that construction.
- `b5 03 30` (offset surface): payload `82 result_carrier_ref source_surface_ref <distance:f64le> <carrier_kind:u8> <u0:f64le> <u1:f64le> <v0:f64le> <v1:f64le>`. The bounds are strictly increasing. `carrier_kind` is `15` for a plane result carrier, `05` for a cylinder, `0d` for a torus, `19` for an `a8 03 32` rolling-ball result carrier, and `01` for a class-`31` result cache. The first reference supplies the result geometry and parameter chart; the second reference and signed distance define the exact offset construction.
- `b5 03 31` (offset result cache): payload `81 cached_source_ref <distance:f64le> <u0:f64le> <v0:f64le> <u1:f64le> <v1:f64le>`. Its source resolves to the same surface as the enclosing class-`30` source alias. Its distance and bounds equal the class-`30` values, with the bounds interleaved instead of grouped.
- `b5 03 37` (support-bound surface construction): payload `85 result_carrier_ref support0_ref support1_ref pcurve0_ref pcurve1_ref <control0:u8> <control1:u8> <construction_radius:f64le> <control2:u8> <control3:u8> <zero:f64le> <control4:u8> <control5:u8>`. The construction radius is positive and the second scalar is zero. Each pcurve begins with the reference of its index-aligned support. The first reference supplies the exact result geometry and parameter chart independently of support-carrier resolution. For a cylinder result the construction radius equals its analytic radius; for a torus result it equals the minor radius; for a sphere result it equals that carrier's stored construction radius and may differ from the sphere radius. An `a8 03 32` result reference supplies the complete rolling-ball jet and its persistent carrier identity.
- `b5 03 3b` (two-scalar support-bound surface construction): payload `85 result_carrier_ref support0_ref support1_ref pcurve0_ref pcurve1_ref <controls:6u8> <scalar0:f64le> <scalar1:f64le>`. Both scalars are positive. Each pcurve names its index-aligned support; a B5 pcurve carries that reference first, while an A8 class-`20` pcurve carries it in the class-`20` support field. The first reference supplies an exact plane or cone result carrier and its parameter chart. For a cone result, `scalar1` equals the carrier half-angle.
- `b5 03 5f` (face node): dominant payload `<0x80 + n_refs> surface_ref loop_ref... <05-tail>`; the first reference is the carrier and the remaining references are owned loops.
- `b5 03 5e` (edge node): payload `85 curve_ref start_vertex_ref end_vertex_ref start_parameter_ref end_parameter_ref <tail>`. The tail is `21`, `22`, `25`, `29`, or `2a` in the object-stream topology and `01` in the standard stream's co-stored identity table. The second and third references are native `5d` vertex identities. Their sharing closes the fixed loop sequence exactly; coincident coordinate loci do not merge distinct identities. The fourth and fifth references name the ordered class-`06` incidences for the edge start and end. They remain distinct when a closed edge has equal start and end vertex identities.
- `b5 03 5d` (vertex identity): payload `81 incidence_ref 00` binds the vertex to one class `05` incidence roster.
- `b5 03 05` (vertex incidence roster): payload `<0x80 + count> incidence_ref{count}` names every class `06` parameter incidence at the vertex.
- `b5 03 06` (parameter incidence): payload `<0x80 + count> curve_ref{count} <0x80 + count> (<parameter:f64le> <control:compact-u32>){count}`. Each finite parameter and compact control are retained index-aligned with their curve reference. References to typed pcurves evaluate at their paired parameter and lift through the pcurve support to the vertex locus.
- `b2/b3/b4 03 5e` (width-coded edge node): after the B-family payload length and width-coded header token, the payload is `curve_ref start_vertex_ref end_vertex_ref start_parameter_ref end_parameter_ref <tail>`, with all five references encoded by `compact_int`. The terminal byte is retained independently. The second and third references are native vertex identities.
- An `a8 03 34` surface with an unresolved indirect pole program remains an identity-bearing surface node. Its object id participates in face, loop, and pcurve references and its payload is retained byte-exactly; unresolved carrier geometry does not erase those topology references.
- `b5 03 27/28/29/2a/2b/2d` analytic surfaces: plane origin+normal, cylinder origin+axis+radius, cone apex+orthonormal frame+half-angle+slant chart, sphere center+radius-scaled frame, torus center+orthonormal frame+major/minor radii, and revolution profile reference+axis origin+direction+angular gauge. The profile reference uses the same positional-byte token forms as loop members; the axis fields immediately follow its variable-width token.

For `b5 03 29`, the 185-byte payload is `<lead> <apex:3f64> <direction_x:3f64> <direction_y:3f64> <axis:3f64> <half_angle:f64> <16B> <angular_offset:f64> <slant_min:f64> <slant_max:f64> <angular_scale:f64> <32B>`. The slant interval is increasing and may begin at the apex; `|slant_min|≤1e-12` denotes zero. Its native chart evaluates with `azimuth=U/angular_scale` and `P(U,V)=apex+V·(cos(half_angle)·axis+sin(half_angle)·(cos(azimuth)·direction_x+sin(azimuth)·direction_y))`. Neutral cone parameters are `(azimuth,(V-slant_min)·cos(half_angle))`; the neutral carrier origin is the axis point at `slant_min`, with radius `slant_min·sin(half_angle)`.

For `b5 03 2a`, the 153-byte payload is `80 center:3f64le scaled_x:3f64le scaled_y:3f64le scaled_axis:3f64le radius:f64le azimuth_min:f64le azimuth_max:f64le latitude_min:f64le latitude_max:f64le construction_radius:f64le chart_origin:f64le`. The sphere radius and construction radius are positive. Each scaled direction has length `radius`, and their normalized forms satisfy `direction_x × direction_y = axis`. Both angular intervals are increasing. The exact sphere carrier has `center`, `radius`, polar `axis`, zero-azimuth `direction_x`, and a periodic native V interval `[chart_origin,chart_origin + 2π radius]`.

For `b5 03 2b`, the 201-byte payload is `<lead> <center:3f64> <direction_x:3f64> <direction_y:3f64> <axis:3f64> <major_radius:f64> <minor_radius:f64> <64B> <major_scale:f64> <minor_scale:f64> <8B>`. Both geometric radii and both parameter scales are positive. Native parameters use the independent gauges `major_angle=U/major_scale` and `minor_angle=V/minor_scale`. The point is `center+(major_radius+minor_radius·cos(minor_angle))·(cos(major_angle)·direction_x+sin(major_angle)·direction_y)+minor_radius·sin(minor_angle)·axis`. The neutral chart is `(major_angle,minor_angle)`.

For `b5 03 2d`, the payload begins `81 profile_ref axis_origin:3f64le reference_x:3f64le reference_y:3f64le axis_direction:3f64le`. The three directions are unit length and `reference_x × reference_y = axis_direction`. Surface U is the referenced `0e` line or `0f` arc profile parameter and surface V is stored revolution arc length. The revolution angle is `V/gauge_radius`. Reversing a negative gauge together with the axis leaves the surface invariant and gives an increasing angular interval. An exact rational NURBS representation uses degree-2 angular spans of at most π/2 with middle weight `cos(Δθ/2)`; its U knots are the profile knots and its V knots remain in the stored arc-length coordinate.

**Object-stream topology:** `b5 03 5f` nodes are faces in native ordinal order. Distinct `b5 03 5e` identifiers referenced by loop nodes are physical edges. A face's loop references select its topology-owning `b5 03 62` nodes; structurally parseable `62` allocations not referenced by a face do not belong to the B-rep. Each selected loop stores its ordered edge occurrences. A paired pcurve lifted through its carrier defines the edge curve. The edge's `5d` references define logical vertex identity independently of coincident or separated endpoint loci. The fixed serialized loop sequence has exactly one head-to-tail endpoint-sense assignment through those identities. A shell-wide Boolean gauge requires the two uses of every twice-used physical edge to have opposite traversal senses. Flipping one loop reverses its member order and toggles every member sense; an inconsistent gauge invalidates the connected topology. A logical vertex's tolerance is the maximum distance from its representative coordinate to an incident lifted endpoint. Pcurve degree and carrier identify lines, circles, and B-splines. An object-stream file contains one body. Faces connected by a shared physical-edge identifier belong to one shell; each connected face component is a separate region and shell of that body. A component is closed when every used edge has exactly two uses. The body is solid when every component is closed, sheet when no component is closed and no edge has more than two uses, and general when closed and open components coexist or an edge has more than two uses. The 3D edge geometry uses f32 endpoint coordinates, and native pcurves have degree 1 or 2.

A loop pcurve occurrence may have no standalone object frame. Its occurrence id is still support-bound when the loop pairs it with an edge and either the edge's class `23`/`24`/`25` curve wrapper names the id or both endpoint `06` parameter-incidence records contain the id. Class `23`/`24`/`25` curve wrappers begin `82 pcurve0_ref pcurve1_ref`; the references are the edge's two support-side occurrences. The loop surface is the occurrence's carrier. Such an occurrence has no serialized parameter-space control net.

Plane lifting is affine: every pcurve pole `(u,v)` maps to `origin + u·direction_u + v·direction_v`, preserving degree, knots, and weights. On a cylinder, constant U with varying V is an axis-parallel line; constant V with varying U is a circle centered at `origin + V·axis`, with cylinder axis, reference direction, and radius. Other pcurve/carrier compositions retain the pcurve-on-surface construction until an exact solved 3D carrier is available.

The class-`5e` start and end parameter references select the edge's trim stations on its pcurve. Their ordered span is the parameter interval of the pcurve-on-surface boundary or intersection construction, and their lifted loci determine the logical-vertex residuals; the pcurve's full knot domain applies only when no valid incidence span exists. Each serialized loop slot is a pcurve occurrence. Multiple slots may reference one source pcurve object; they retain the same control net but have independently selected parameter ranges, with equal-range occurrences sharing one geometric carrier. The two support occurrences of one physical edge retain independent ordered pcurve intervals. Equal increasing intervals are the construction's solved-curve interval directly; otherwise the solved curve uses `[0,1]` and maps that interval affinely onto each support interval, including a decreasing support traversal. An affine plane lift retains those stations as the 3D NURBS edge interval; when their order decreases, reverse the 3D control points, weights, and reflected knot vector so the edge interval remains increasing. An analytic line lift uses signed distance along its unit direction and reverses that direction when the native start station follows the native end station. For a cylinder or cone latitude, torus latitude, or torus meridian, the varying native chart coordinate divided by its carrier scale is an unwrapped circle angle. Positive rational weights and monotone control ordinates in that angular coordinate prove a branch with no turnback. A branch of at most one turn becomes a canonical increasing circle interval; a decreasing branch reverses the circle axis, and a negative latitude radius reverses the reference direction. Both interval endpoints evaluate to the edge's ordered `5d` vertex loci within the edge tolerance.

A degree-1 cylinder pcurve with both U and V varying is a circular helix. Its angular interval is `[U0/r,U1/r]`; its axial rise per radian is `(V1−V0)/(U1/r−U0/r)`, so the pitch vector per full turn is `2π·rise_per_radian·axis`. Reversing the two endpoints to order the angular interval does not change the curve. The helix radius vectors are `r·reference_x` and `r·(axis×reference_x)` and its radial growth is zero.

A constant-U or constant-V pcurve on a tensor-product NURBS surface, including an exact revolution cache in the native chart, is the corresponding exact surface isocurve. Fixing U contracts each V-column in homogeneous coordinates by the U basis values; fixing V contracts each U-row by the V basis values. The varying direction retains its degree, knot vector, and periodic flag. Each resulting control point is the contracted homogeneous numerator divided by its contracted weight. Positive pcurve weights and monotone poles in the varying surface coordinate prove that an endpoint-incidence span has no turnback. Evaluating the two incidences yields the isocurve's trim coordinates; decreasing traversal reverses the isocurve poles, weights, and reflected knot vector, and both resulting interval endpoints agree with the ordered edge vertices.

Coincident `05 08 01` rows share an endpoint locus. For topology subsets whose
allocation identity is otherwise unresolved, the locus binds to the lowest
serialized matching row. A one-edge loop is closed when the edge's ordered
vertex identities are equal and traverses in the native edge direction. A
multi-edge loop is emitted only when the resulting ordered edge sequence has
exactly one closed head-to-tail sense assignment. Faces or loops
with unresolved references, endpoint lifts, or chain sense remain outside the
connected graph.

---

## 7. Outer schema and object records

### 7.1 `7C02` source-schema catalogs

```text
catalog := 7C 02 <total_len:u32le> <count:atom> entry{count-1}
entry   := <inclusive_len:u8> <utf8[inclusive_len-1]>
```

`total_len` includes the marker and length field. The entries consume the frame exactly. A `7C02` candidate contained by another complete catalog extent is entry data, not an independent catalog. The first four entries are `CATCatalogManager`, `catalogManager`, `catalogLinks`, and the empty string. The catalog names source classes and fields available to the serialized object schema; a name does not declare an object instance.

### 7.2 `7C08` object graph

An object graph is preceded by a contiguous entity-table run containing one `7C05` record for each serialized `7C09` record. A `7C05` record is `7C 05 <total_len:u32le> <lead:u8> <definition> <value> <record_suffix>`, where `definition := 7C 06 <definition_len:u32le> <definition_prefix> EA <entity_id:u32le> <definition_suffix>` and `value := 7C 07 <value_len:u32le> <value_payload>`. Both nested lengths are total lengths measured from their markers. The definition frame ends at the value marker, and the value frame ends at or before the enclosing record end. Within `definition_prefix`, `32` begins a fixed-width five-byte atom, so an `EA` among its four value bytes is not the identity delimiter. Entity identities are nonzero and strictly increase across a run but may be sparse. One `DE` byte separates the run from its paired `7C08` marker, and the run has the same record count as that graph. Pair `7C05` and `7C09` records by serialized position. Preserve the complete definition prefix, definition suffix, value payload, and record suffix.

Walk the complete definition prefix from its first byte. `32 <ordinal:u32le>` is one fixed-width source-schema selector; its four ordinal bytes cannot begin another selector. Every other byte advances the walk by one. A `32` with fewer than four remaining ordinal bytes is not assigned. Resolve an in-range ordinal to the exact entry in the object graph's associated catalog and preserve the ordinal, selected entry identity, UTF-8 name, and prefix-relative marker offset. An out-of-range ordinal remains a selector without an entry or name.

A complete numeric-tuple value payload is `<prefix0:one_byte_atom> <prefix1:one_byte_atom> E8 <type:compact_atom> 37 <layout:one_byte_atom> <value:one_byte_atom> <item+> FE FE`. An item is `E6 <bits:u64le>` or one zero-payload control byte in `E7..E9`; at least one item is binary64. A one-byte atom is `80..D0 → byte-80`. A compact atom additionally admits `D1..E4 <low:u8> → (prefix-D1)*256+low+1`. The production is assigned only when it consumes the complete `7C07` payload.

A complete reference-signature value payload is `32 <first:u32le> <prefix:one_byte_atom> E8 <type:compact_atom> 37 <layout:one_byte_atom> 81 <signature:printable_utf8+> FE 32 <second:u32le> <closing:one_byte_atom> E9 <closing_type:compact_atom> 08 37 FE FE FE`. Both references and the signature are assigned only when this production consumes the complete `7C07` payload.

Every complete `7C07` payload also uses the lossless value-field tokenization defined for `7C0B`: schema selectors, binary64 values, tagged zero-payload markers, untagged `E6..E9` opcodes, `37` packet separators, inline byte strings, compact atoms, `FE` terminators, and literal bytes. Typed multi-byte fields take precedence over marker-shaped bytes inside their payloads. Bytes outside an assigned production remain ordered literal fields.

Each `32 <ordinal:u32le>` value field whose ordinal is within the object graph's associated schema catalog selects that entry. Preserve its payload-relative offset, ordinal, selected entry identity and name, the complete ordered token sequence through the byte before the next catalog-valid selector or through payload end, and every complete value packet within that token sequence. A marker with an out-of-range ordinal remains part of the preceding encoded value and does not create a selection.

`E8 <value-selector:u16le> 37 FE FE` is a double-terminated compact value packet. Preserve the `E8` payload offset and the two-byte little-endian selector. The selector bytes are fixed-width data even when either byte is also valid as a value-program atom or opcode. Packet assignment requires the complete six-byte production.

`E9 <type-selector:u16le> <layout:u8> 37 FE FE` is a double-terminated layout-bearing value packet. Preserve the `E9` payload offset, two-byte little-endian type selector, and uninterpreted layout byte and its payload offset. The selector and layout bytes are fixed-width data independent of their value-program token classes. Packet assignment requires the complete seven-byte production.

Each nested total-length tree is rooted at `7C 08`; every root is an independent object graph whose consecutive `7C09` children exactly cover the declared root extent, and each `7C09` object holds a lead-coded head and a `7C0A` tagged-atom payload. A `7C08`, `7C0B`, or `7C02` candidate contained by another complete root extent is payload data, not an independent graph, value block, or catalog. The payload begins at the unique `7C0A` frame whose stored total length lands exactly on the enclosing `7C09` end and ends with an unconsumed `FE` terminator; bytes after that terminator and a length-framed field that consumes it invalidate the graph. Literal `7C0A` bytes in the variable head do not delimit the payload, and multiple length-closing candidates invalidate the graph. It is the **feature/object-ownership layer**, not the expanded face→loop→coedge table or the port→vertex collapse. A separator-form `7C09` head is `<lead> 01 <owner_ref> <class_ref> [storage_ref]`. Compact heads encode the role count in the lead: `02 <owner_ref>`, `12 <owner_ref> <class_ref>`, and `52 <owner_ref> <class_ref> <storage_ref>`. Other separator-free leads do not assign reference-shaped tokens to these roles. Its ordinary references use `80..d0 → byte−80` and its paged references use `d1..e4 <byte> → (prefix−d1)·256+byte+1`. A paged prefix without its low byte and bytes outside those assigned forms remain literals; they do not create references. `owner_ref` is an entity identity in the paired `7C05` run. Class refs are zero-based per-file schema ordinals rather than entity identities or global type codes. `7C0A` atoms use the same compact and paged forms, plus raw `0x51..0x7F` and escaped `0x80 <value:u32le>`. A fixed-width reference is `0x32 <entity_id:u32le>`; its four identity bytes are not independent atoms. E5 blobs inside `7C0A` are templated descriptor records (≈59 or 46 bytes), not NURBS payloads. The class30 pair records and `76 ac 7f`-delimited handle table are coedge/half-edge sub-tables, not the port→vertex relation.

All `7C09` records in one graph carrying the same `owner_ref` are fields of one serialized design object. Design objects are ordered by the byte offset of their first field in the graph; each retains that first-field offset and its zero-based position in this order. Preserve each group's field source order and class-specific storage forms. Preserve the distinct resolved field-class entries and names in first field order as the object's field vocabulary. When `owner_ref` equals an identity in the paired entity table, it selects the positionally paired `7C09` record. The design object grouping that contains this selected record is the object's structural owner; a selected record contained by its own group contributes no owner link. In separator-form groups, retain that record's resolved class entry, class name, and `storage_ref` as the owner class and owner storage form. In compact groups, the selected record is an identity anchor rather than a class declaration; leave owner class and storage unset and use the group's ordered field-class vocabulary as its type evidence. An unresolved owner identity does not invalidate the field group. A schema class name labels its field record and is not by itself a neutral feature operation.

A compact `0x81` reference field, a fixed-width `0x32` reference field, and every reference item in a count-complete `0x3b` list select an entity identity in the paired `7C05` run. Each decoded reference and list item retains the byte offset of its tag or item start within the `7C0A` payload. A decoded reference also retains whether it is a standalone field or a list item; a list-item reference retains the list tag offset and its zero-based position among all items in that list. A payload terminator cannot satisfy a list item, and a list that reaches the terminator before its declared item count contributes no reference links. Preserve every stored identity in payload order and link each resolved identity to its positionally paired field record. When the selected record carries an `owner_ref`, preserve the corresponding design-object link on the reference. Every non-self inter-object occurrence retains its source field, source field class, source payload offset, reference container, stored target identity, exact target field, target field class, and target design object in field and payload order; repeated occurrences are distinct. A field class is absent from the relation when its record has no resolved source-schema entry. These links form a general cyclic object graph; their order does not imply construction order or regeneration dependency. An identity absent from the entity table remains a reference link without a target. References to records without a materialized owner group remain record links without an inter-object reference.

A repeated-reference suffix is `<48:atom> <count:atom> <references[count]> <count:atom> <references[count-1]> <129:atom> FE`, with `count >= 2`. The second reference vector must equal the first vector without its final item, and the suffix must end the complete `7C0A` payload. The repeated vector is preserved as ordered entity identities; the final item of the first vector is preserved as the terminal reference. Preserve both count-atom offsets. A count mismatch, changed repeated reference, non-reference vector item, or trailing field prevents assignment of this production.

The payload prefix can bind this suffix to a source-schema ordinal in either of two productions: `<59-byte blob> <5:atom> <46:atom> <schema-ref:atom>` or `<schema-ref:atom> <34:atom> <59-byte blob> <5:atom>`. The production can occur after earlier fields. Preserve the schema ordinal, its payload offset, which field order was serialized, and the exact selected entry and name from the graph's schema catalog. Assign no preamble when neither production occurs or when more than one production occurs before the suffix.

The following `7C02` schema catalog stores UTF-8 strings. A nonzero first byte is the inclusive one-byte-header-plus-string length. A zero first byte selects a following `u32le` string-byte length after the five-byte header. It either immediately follows the `7C08` graph or follows the graph's intervening `7C0B` value block. Preserve the graph's exact catalog link and the catalog's framing offset. String values may contain line feeds and non-ASCII unit symbols. Its fixed first four entries are `CATCatalogManager`, `catalogManager`, `catalogLinks`, and the empty string. A `7C09` head's `class_ref` is the zero-based ordinal of its class name in this catalog; preserve both the selected entry identity and its string value on the field record.

### 7.3 `7C0B` visualization value blocks

```text
value_block := 7C 0B <declared_len:u32le> <payload[declared_len-6]> FE 7C 02 ...
```

`declared_len` measures from the `7C0B` marker through the byte before the terminator. The complete block occupies `declared_len + 1` bytes. The trailing `FE` is followed immediately by the associated `7C02` source-schema catalog.

The value block begins at the exact end offset of its preceding `7C08` object graph and owns the immediately following catalog as its source schema. It stores visualization, color, display-offset, chirality, scale-context, and presentation schema values rather than feature parameters. The two frame boundaries identify both structural relations without scanning or name matching. A `7C0B` or `7C02` candidate contained by a complete value-block extent is value payload, not an independent block or catalog.

The payload is a serialized token stream. `32 <ordinal:u32le>` stores a source-schema selector candidate. An ordinal below the source schema's entry population selects that zero-based entry; an ordinal equal to the population is the absent-schema sentinel. A larger ordinal remains a fixed-width field without delimiting a schema-selected value. `87 E6 <bits:u64le>` stores one IEEE-754 binary64 value. `87 E7` and `87 E8` are zero-payload markers. `8E <code:E8..EF> 84 <bytes[code-E7]>` stores one through eight inline bytes. `80..D0` stores the unsigned atom `byte - 80`; `D1..E4 <low:u8>` stores `(byte - D1) * 256 + low + 1`. The longer assigned forms take precedence over atom recognition. Multi-byte token payloads are opaque to token recognition: marker-like bytes inside them do not start another token. Bytes outside these forms are single-byte literals, so tokenization preserves the complete payload without residual bytes.

Resolve every in-range selector to the exact entry in the block's source schema and preserve both the entry identity and its UTF-8 string value. An in-range selector begins one stored value; its complete encoded value is every following token before the next in-range selector, absent-schema sentinel, or end of the payload. Consecutive delimiting selectors give the first selector an empty encoded value. The selector's absolute marker offset is its persistent source identity; preserve its containing block, payload offset, and serialized order. Preserve the absent-schema sentinel as a selector without an entry link, name, or encoded value.

### 7.4 Outer alias rows

```text
alias_row := <lead:u32le> 01 00 04 00 <tag:u32le> <flag:u8> <f1:3B> <f2:u32le> <f3:u32le>
```

The low 24 bits of `tag` are the persistent roster tag; the high byte remains part of the stored word. An alias core overlapping a complete `7C02`, `7C08`, or `7C0B` extent is framed field or payload data rather than an outer roster row. Exact lead values `0x8e` and `0x8f` are ordinal-linked storage forms. `f1[2]` is a one-based `7C09` ordinal in the unique object graph with the greatest record population. An in-range ordinal links the row to that exact record. When the selected record carries an `owner_ref`, the row also links to the corresponding design object. Ordinal zero and values beyond that graph's record population carry no object-record or design-object link. The complete lead, flag, F1, F2, and F3 fields remain attached to the alias row.

Grouped alias rows carry this header before a bounded storage prefix:

```text
alias_group_header := 02 00 <prototype:u32le> <group_id:u32le>
                      00 05 00 01 00 00 00 30 00 00

alias_group_storage := <bit:u8> 00 00
                     | <bit:u8> <bit:u8> 00 00
                     | <bit:u8> 01 00 <word:u32le>
                     | <bit:u8> <bit:u8> 01 00 <word:u32le>

grouped_alias_row := alias_group_header alias_group_storage
                     01 00 04 00 <tag:u32le> <flag:u8>
                     <f1:3B> <f2:u32le> <f3:u32le>
```

Each `bit` is exactly zero or one. `prototype` identifies the node kind and `group_id` identifies the node group. The group's four-byte target allocation slot begins at `f1[2]` and continues through the first three bytes of `f2`; its low byte is therefore also the object-record ordinal. The complete storage prefix remains attached to the alias membership.

---

## 8. Zero-entity `a9 03` variant

Record framing `a9 03 XX YY <payload[YY+8]>`, `record_length = YY + 12`; records reference each other by **global record ordinal** into the `a9 03` stream.

Unframed `05 08 01` coordinate rows lie outside every declared record extent and outside the extended logical extents of support records whose inline data continues past the nominal frame. Marker-like bytes within either extent are record payload, not vertex rows. Connected zero-entity topology derives logical vertex coordinates from lifted support endpoints; unframed coordinate rows are independent fallback geometry.

Record families: `5f 0c` face (24 B), `5e 1a` edge-stride (38 B), `62 xx` edge-loop, `06 38` coedge (68 B, two per edge), `5d 06` vertex marker, `25 69` edge side-pair header, `21 71` curve-support-on-surface, `27 6a` plane, `28 8a` cylinder-family, `29 b8` cone-family, `2b c8` circle/arc/torus, `34 c8`/`34 5e` bspline carriers, `05 0b/10/15` vertex-incidence.

- **`62xx` loop** is an alternating even/odd lane; `edge_count = (flag_at_+12 − 0x81)/2`. The even lane satisfies `A[j] = T − g − j`. **Loop-class byte = location:** `0x50` = inner (hole) loop, `0x41`/`0xc1` = non-inner; the `0x50` count equals the hole count. The outer loop is first, followed by inner loops in ascending terminal-id order.
- A face-family record with counted references `[R0, R1, ..., Rm]` defines ordered loop terminals `T[j] = R0 - R[j+1]`. Concatenate loops in face-record order and each loop's members in serialized order. For an owned `21xx` support occurrence with local slot `s` at `+12`, its first-lane loop member is `A = T - s`. `A` identifies a face-local support occurrence rather than a global `0638` identifier.
- **Coedge sense** is a packed 3-bit-per-coedge stream after the reference lane: code 7 = forward and code 2 = reversed relative to the stored edge direction. The `0638` `(1,2)` byte identifies the positional twin; the `62xx` stream stores orientation.
- A `2569` side-pair header supplies base columns `[B0, B1]`. Its two following `0638` records carry side numbers `1` and `2`; the side-slot pair is `(B0 + side, B1 + side)`. This pair identifies an oriented use in the `2569`/`0638` topology namespace and does not directly address a `21xx` support or `05xx` vertex item.
- **Carrier run = per-face surface:** a carrier (`276a`/`288a`/`29b8`/`2bc8`) followed by a maximal run of `21xx` supports; face order aligns 1:1. Surface kind is in the payload f64, not the tag.
- **Zero-entity edge carrier:** a `21xx` coedge's f64 tail is `(u0,v0,u1,v1)` on its owner-run carrier. Lift per kind: plane direct-UV; cylinder `θ=u/radius`; cone `u` is the angle directly; torus `θ=u/R` and `φ=v/r`. The two lifted UV pairs define the edge endpoint coordinates. Bspline carriers `34c8`/`345e` store the **full NURBS pole grid inline** (`34c8` 7×7 @+167, `345e` 5×7 @+141).
- **Physical edge carrier from radial supports:** either radial `21xx` occurrence supplies the physical edge's 3D carrier. A plane support maps every UV NURBS pole affinely through the plane frame and preserves degree, knots, weights, and periodicity. A constant-U cylinder or cone support maps to a line; a constant-V cylinder maps to a circle; a constant-V cone maps to a circle or ellipse according to its carrier ratio. Constant-U and constant-V torus supports map to meridian and latitude circles. A constant-U or constant-V support on a tensor-product NURBS surface contracts the fixed-direction basis in homogeneous coordinates; the varying direction's degree, knots, and periodicity become the curve representation. The physical edge retains this carrier when its opposite radial occurrence uses a support family without inline poles.
- A physical edge with two radial `2191` occurrences is the intersection of the two owner-run surfaces. Both occurrences store cubic UV pcurves on the same increasing parameter interval. The two surface-evaluated traces are approximation caches of that intersection and may differ in model space; neither trace replaces the two-surface construction.
- A `34c8`/`345e` carrier stores distinct U knots, tagged U multiplicities, two tagged V-dimension words, a one-byte V marker, distinct V knots, tagged V multiplicities, a three-byte pole marker, then the row-major f64 XYZ pole grid. The grid continues past the carrier's nominal framed length. Degrees are `first_multiplicity-1`; control counts are `sum(multiplicities)-degree-1`.
- Packed loop sense orients each lifted endpoint pair. Consecutive oriented occurrences satisfy `end[j] = start[(j+1) mod n]` within `2e-3` mm. A single unlifted occurrence between two lifted occurrences has endpoints `[end[j-1], start[j+1]]`. Two occurrences are radial twins when their unordered endpoint pairs are uniquely equal within the same tolerance. Unique radial pairs establish connected face components. A coincident endpoint group partitions by those components and forms one physical edge for each component containing exactly two occurrences. Other ambiguous or unpaired endpoint groups do not form a physical edge. Each connected face component is a separate shell and body.
- Every face has exactly one outer loop. Outer-loop class `0x41` gives forward face sense and class `0xc1` gives reversed face sense. Class `0x50` marks an inner loop. For non-seam boundaries, the packed occurrence senses produce positive UV winding for `0x41` and negative UV winding for `0xc1`; the class remains authoritative when a periodic seam makes the signed UV area zero.
- **`2bc8` carrier kind:** `major≠minor` is a torus. `major==minor` is a degenerate horn torus.
- Edge curve kind: two coaxial surfaces of revolution intersect in circles (exact theorem); a plane cuts a cylinder in a circle (⊥), lines (∥), or ellipse (oblique): classify per `|cos∠(plane_normal, cyl_axis)|`.

The `5e1a` edge-stride, `0638` coedge-twin, `2569` side-pair header, and `2171` support head have the layouts described above. The `2171` f64 tail stores `(u0,v0,u1,v1)` at `+93`, `+101`, `+109`, and `+117`.

Inline support pcurves share a clamped NURBS grammar. Distinct f64 knots are followed by equally many tagged `u32` multiplicities; `degree = first_multiplicity - 1`, `control_count = sum(multiplicities) - degree - 1`, and the full knot vector repeats each distinct knot by its multiplicity. Pole pairs follow the final multiplicity token. The families are:

| Tag | Distinct knots | Multiplicities | Pole start | Degree/control count | Weights |
| --- | --- | --- | --- | --- | --- |
| `2145` | `+67..+107`, stride 8 | `+115..+140`, stride 5 | `+145` | 3 / 12 | none |
| `2171` | `+67,+75` | `+83,+88` | `+93` | 1 / 2 | none |
| `2172` | `+67..+115`, stride 8 | `+123..+153`, stride 5 | `+158` | 3 / 14 | none |
| `2191` | `+67,+75` | `+83,+88` | `+93` | 3 / 4 | none |
| `2199` | `+67,+75` | `+83,+88` | `+93` | 2 / 3 | three f64 values after the poles |
| `219f` | `+67..+123`, stride 8 | `+131..+166`, stride 5 | `+171` | 3 / 16 | none |
| `21d6` | `+67,+75,+83` | `+91,+96,+101` | `+106` | 2 / 5 | five f64 values after the poles |
| `21e8` | `+67,+75,+83,+91,+99` | `+107,+112,+117,+122,+127` | `+132` | 3 / 7 | none |

The pole coordinates use the carrier's native parameter units. Neutral IR conversion is `(u/r,v)` for cylinders, `(u,v cos α)` for cones, `(u/R,v/r)` for tori, and identity for planes and NURBS surfaces. The first and last neutral poles equal the support endpoint pair. `2118` is degenerate and has no pcurve payload. A radial `2118` pair owned by a plane and torus defines their intersection branch. The plane normal is perpendicular to the torus axis. Let `d` be the signed plane offset from the torus center, `R,r` the torus radii, `φ` the torus minor angle, and `q` the in-plane transverse coordinate; the branch satisfies `ρ=R+r cos φ`, `q=±sqrt(ρ²-d²)`, and axial coordinate `r sin φ`. The endpoint signs select `±`, and the edge occupies the shorter monotone `φ` interval between its endpoints. The `2145`, `2172`, and `219f` logical records own the 256 bytes following their nominal frames. Their logical lengths are respectively 337, 382, and 427 bytes; the continuation contains the remainder of the inline row-major f64 `(u,v)` pole array.

---

## 9. E5 `0D 03` stream variant

Framing `E5 0D 03 <cls> <sub> <payload_size_u16le> 00 00 00 <record_id_u32le> <payload>`, stride `payload_size + 13`, from the preamble or the strongest FINJPL walk. Reference tokens: hi-bit byte `b`→`b−0x80`; `08 <lo>`→lo; `10 <hi>`→hi<<8; `18 <lo><hi>`→u16le.

Classes: `0x01` body, `0x00` advanced face, `0x08` datum/template face, `0x09` edge loop, `0x0d` reference bundle, `0x0e` parameter-bound, `0x96`/`0x97` UV line/circle pcurve, `0xa0` complex/spline pcurve, `0xc0`/`0xc1` boundary/intersection curve support, `0xc8`/`0xc9`/`0xca`/`0xcc` plane/cylinder/cone/torus carrier, `0xfe` vertex, `0xff` trimmed edge-use.

E5 `05 08 01` coordinate rows occupy an unframed contiguous run outside the declared extents of `e5 0d 03` records. The governing run is the unique run whose row count equals the distinct endpoint-vertex population referenced by the transferred edge uses. A matching byte sequence inside a framed record payload is payload data, and multiple matching runs leave the coordinate binding unresolved.

An E5 face component is closed when every used edge has exactly two uses. A body containing only closed components is solid, one containing only open manifold components is sheet, and one mixing closed and open components or containing a non-manifold edge is general.

**Topology:** a class-`0x01` body references one class-`0x08` root whose counted face roster names the body's class-`0x00` faces. Faces in one body connected through a shared `0xff` edge-use identity form one region and shell; disconnected face components remain separate regions and shells of that body. A face is `<0x81 + loop_count> <surface_ref> <loop_ref>* <01 00>`: loop location is structural (`loop_count==1` simply bounded, `>1` = 1 outer + `loop_count−1` holes; `Σ(loop_count−1)` = part hole count). A loop is `<0x81 + 2*edge_count> (pcurve_ref edge_use_ref)* surface_ref`. An edge-use (`0xff`) is `85 <curve_support_ref> <start_vertex> <end_vertex> <param_start> <param_end> <tail>`; the `0x85` lead counts the five reference fields, and the remaining bytes form a separate tail. `param_start` and `param_end` select class-`0x0e` bound records. Each bound pairs its counted representation refs positionally with `(parameter:f64le, code:u32le)` entries. The unique entries naming the loop's occurrence pcurve give its edge-start and edge-end parameters. Their span sign relative to the pcurve's native range fixes occurrence parameter direction; the bound interval may be an affine rescaling of the native interval. The loop's pcurve is the face occurrence curve. The edge's `0xc0`/`0xc1` support separately references its boundary or intersection construction views; those record identities need not equal the occurrence pcurve identity. **Vertex ref → index** is sorted-ref-rank. The binding is valid only when each edge endpoint identity closes against decoded bytes: evaluating the paired occurrence pcurve through its referenced surface, or evaluating an explicit 3D curve carrier, yields the two mapped `05 08 01` coordinates within f32 precision.

**E5 orientation** is `absolute_sense = g_loop × relative_chain_sense`, where `relative_chain_sense` is the unique head-to-tail vertex-chain closure of the fixed cyclic member list. Applying `g_loop=-1` reverses the cyclic member order and toggles every relative member sense. A two-edge digon has two complementary relative closures; its canonical relative gauge traverses the first serialized edge forward. The loop trailer contains `3*edge_count+4` signed ternary words in `{-1,0,+1}`; `ref_aligned_signs[1]` is nonzero and stores loop role (`+1` outer, `−1` inner), while reserved lanes may be zero. Each edge having exactly two loop occurrences imposes the manifold-coherence equation `g_A·g_B = −r_A·r_B`. Boundary and non-manifold edge populations impose no pair equation. Each consistent connected parity component is solved independently; its global sign follows majority `face_trailer_sign` alignment, with the first serialized loop as the stable gauge when the alignment count ties. A frustrated component has no absolute orientation.

**E5 surface carriers** use these byte layouts: plane `0xc8` is `90+8n` B, cylinder/circle `0xc9` is 137 B, cone `0xca` is 185 B, and torus `0xcc` is 201 B. A plane stores its origin at `+14`, a finite transform-scalar lane beginning at `+39`, and its four natural UV bounds in the final 32 bytes. **Edge curve descriptors** evaluate pcurves on their carriers: cylinder isoparametric curves yield circles or lines, torus isoparametric curves yield circles, and cone isoparametric curves yield circles. A non-intersection `0xc0` support with one analytic line pcurve lifts to that exact 3D line or circle; a plane `0x97` pcurve lifts to the corresponding plane-frame circle. A non-isoparametric pcurve on a nonplanar carrier remains the exact parametric surface-curve construction `C(t)=S(p(t))`; no endpoint chord or fitted cache replaces it. Lifted edges use the endpoint-ordered distance or canonical increasing angular interval. Torus and cone boundary UV pcurves are co-parameterized to the 3D edge angle parameter. The `0xa0` UV jet encodes a degree-5 C2 curve; a square Hermite solve recovers its exact control net. On a plane carrier, the affine plane frame maps that UV control net and unchanged knots, weights, and parameter interval to the exact 3D edge curve.

E5 `0x96` p-curves store `<surface_ref>, origin_u, origin_v, dir_u, dir_v, param_lo, param_hi` as f64 values. E5 `0x97` p-curves store `<surface_ref>, center_u, center_v, <code0:u32>, <code1:u32>, radius, param_lo, param_hi, tail0, tail1>`. Their native UV circle is converted to a rational quadratic arc before the owning carrier's independent U/V chart scales transform every control point; unequal scales therefore produce an exact neutral-chart ellipse, not a circle with a rescaled scalar radius. Cylinder U is arc length (`u=U_native/radius`); torus U and V are arc lengths (`u=U_native/major_radius`, `v=V_native/minor_radius`). A cone carrier stores `u_scale:f64` at record `+158` and `v_scale:f64` at `+166`; its neutral chart is `u=U_native/u_scale`, `v=V_native*cos(half_angle)/v_scale`. `0xc0` is a one-pcurve boundary support and `0xc1` is a two-pcurve intersection support. Edge type follows `0xff -> 0xc0/0xc1 -> pcurve -> carrier`. When both `0xc1` support-side pcurves lift to the same analytic 3D locus and equal endpoint-ordered sweep, the edge retains that exact cache together with the ordered two-surface intersection construction. A support-side reference may instead name a wrapper; the two radial loop occurrences then supply the construction's ordered `(surface,pcurve)` sides after both endpoint lifts agree with the edge identities and their parameter intervals agree componentwise within `1e-9`. When only one radial side resolves, that side retains the exact parametric construction `C(t)=S(p(t))` without asserting the unresolved second support or a solved cache. Endpoint agreement alone does not select a solved cache.

A class-`0xc8` plane stores its origin and natural UV bounds but no orientation vectors. Parallel axes from adjacent cylinder, cone, or torus carriers are the plane-normal constraint for rank-one boundary charts; the relevant shared `0xc1` support contains a circumferential `0x96` pcurve (`dir_v=0`, `dir_u!=0`).

The complete plane frame follows from its occurrence-pcurve endpoint UV values and the referenced edge vertices by `P-O = U*u_axis + V*v_axis`. A full-rank endpoint set solves both axes by least squares and must produce one orthonormal frame within `2e-3` mm. A rank-one diameter set determines its represented in-plane axis; the known plane normal supplies the perpendicular axis. Simultaneous reversal of both in-plane axes is fixed by requiring the first nonzero component of `u_axis` to be positive.

Plane `0x97` circle pcurves use arc-length parameters, normalized to angle by `t_neutral=t_native/radius`; they transfer as exact piecewise rational quadratic arcs. `0xa0` pcurves store degree-5 position/first-derivative/second-derivative jets at distinct knot sites. Each UV component and both derivative orders scale by the owning carrier's native-to-neutral chart factor. Each adjacent site pair defines one exact quintic Bézier span from the two endpoint jets. The neutral NURBS uses sixfold span boundaries, preserving every quintic span and its native parameterization without fitting. A plane carrier additionally maps the complete UV control net affinely into the exact 3D edge curve.

E5 carrier frames use f64 fields. `0xc9` stores origin, `frame_u`, `frame_v`, radius, and angular/arc data, with `axis = frame_u × frame_v`. `0xca` stores origin, `frame_u`, `frame_v`, axis, angle, reference radius, UV bounds, and the native-U scale at `+158`. `0xcc` stores origin, `frame_u`, `frame_v`, axis, major radius, minor radius, and UV bounds.

An E5 `0xa0` UV jet is a nonperiodic degree-5 C2 B-spline p-curve. Its knot-site position, first derivative, and second derivative determine the B-spline poles through a square Hermite system. Duplicating each interior knot yields the local quintic Bezier controls.

The E5 root `0x08` sign tape contains one face-aligned sign for each class-`00` face, followed by two additional signs. The two trailing signs have no assigned semantic role.

For plane-carrier `0xa0` cases, evaluating the UV jet on its `0xc8` plane produces the same 3D point set as the primitive circle supplied by the paired `0x96` view. The native `0xa0` parameter is not an affine primitive-circle angle parameter.

---

## 10. FBB-only partial-spine variant

A nested-`V5_CFV2` file with a valid FBB face group and `05 08 01` vertices whose post-FBB edge tables and trim packets use one common selected handle width `W∈{1,2,3}` across **two** edge tables (`kind=0x01` then `kind=0x02`). Widths 1 and 3 use delimiter `10 24 04 ff ff 00 00 00`. A width-2 delimiter is `10 F4 04 ff ff 00 00 00`, where the high nibble `F` is a nonzero family discriminator other than `2`; both delimiters carry the same discriminator. The table walk selects the width/delimiter pair that lands exactly on both delimiters and the counted vertex table.

```text
edge_table := 01 <kind∈{0x01,0x02}> count(row_count) edge_row{row_count}
edge_row   := 02 count(arity) <arity × handle:u(8W)be>
post_fbb   := edge_table(kind=0x01) delimiter edge_table(kind=0x02) delimiter vertex_table
```

The two tables end at the vertex table. Their combined row count equals the `0x60` curve-support count. The selected width preserves record boundaries. The concatenation binds in row order to the `0x60` table: where `0x60_row[i]` is a line, `FBB edge_row[i].arity == 2`. The table split carries no line-versus-curve meaning. The bound `0x60` row provides edge kind, adjacent faces, circle center, and radius. Surface intersection resolves endpoints between analytic faces.

Each FBB-only edge-row handle is a same-width trim-mesh boundary-vertex handle. Each row matches a contiguous forward or reversed recovered trim-boundary run, and its analytic curve comes from the positionally bound `0x60` row. Endpoint-port to logical-vertex collapse remains separate.

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
