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
| BITMAPINFOHEADER size 40 + valid bit depth      | BMP thumbnail               |
| OLE2 compound-file header                       | OLE2                        |
| `PS 00 00`                                      | plain Parasolid stream      |
| Parasolid wrapper magic at offset 4             | wrapped Parasolid           |
| nested compressed member → `PS 00 00`           | nested Parasolid            |
| `uoTempBodyTessData_c` / `uoTempFaceTessData_c` | tessellation / DisplayLists |
| `ff ff 01 00`                                   | SW Objects                  |
| `<?xml` / UTF-16LE BOM+XML / byte0 `86`+XML     | XML                         |
| `unqlite`                                       | UnQLite database            |

PNG preview dimensions and encoding fields are in `IHDR`. BMP thumbnail width, height, planes, bit depth, compression, and image size are in the 40-byte BITMAPINFOHEADER after the leading file-size word. The `swSolidWorks` XML root carries version, creation time, and path; its `swModel` child carries model and configuration names.

`Contents/Config-0-Partition` and `Contents/Config-0-Deltas` carry body B-rep records. `Contents/Config-0-ResolvedFeatures` carries feature-input sketch profiles. `Config-0-GhostPartition`, `Contents/Definition`, and `Config-0-LWDATA` are separate payload families.

`ResolvedFeatures` sketch entities begin with `ff ff 1f 00 03`. A little-endian u32 at marker +17 is the entity type: `0` point, `1` curve, `2` arc, `3` constrained point. Marker +48 stores a finite little-endian f64 state value. The trailing little-endian u32 is the feature-local object identifier; `ff ff ff ff` is null. A coordinate-bearing marker has the 12-byte prefix `ff ff ff ff ff ff ff ff 00 00 80 bf` at marker +5, `1e 00` at marker +64, and two finite little-endian f64 coordinate fields at marker +66 and +74, in metres. Coordinate records are 142, 152, or 162 bytes, place the object identifier at marker +138, +148, or +158 respectively, and may be followed by a four-byte separator before the next marker. The 92-byte reference-bearing variant stores two little-endian u16 feature-local object identifiers at marker +64 and +66, a little-endian u16 selector at marker +68, zero at marker +70, little-endian f64 `-1.0` at marker +72, and its object identifier at marker +88. Each referenced identifier is resolved independently against typed sketch markers owned by the same feature object as the referencing marker.

A sketch marker belongs to the Keywords feature object whose bound feature-name record precedes the marker and whose next bound feature-name record follows it. Marker local identifiers are scoped to that feature object.

A feature-input class declaration is `ff ff 01 00`, a little-endian u16 byte length, and an ASCII class name. When the following record begins at declaration offset `+ 6 + length`, that record is an instance of the declared class. A feature-name record begins with `04 80 ff fe ff`, a u8 UTF-16 code-unit count, and the UTF-16LE name. The little-endian u32 at eight bytes after the name is the feature object ID. It equals the corresponding Keywords feature `id` and binds the records independently of the display name.

A repeated class instance stores a little-endian u16 class token immediately before its feature-name marker. The token is scoped to the `ResolvedFeatures` lane. Repeated instances with the same token have the same declared class.

`moExtrusion_c` and `moICE_c` are extrusion feature classes. `moProfileFeature_c` and `mo3DProfileFeature_c` are planar and spatial sketch feature classes. `moCombineBodies_c` is the body-Boolean feature class. `moConstSurfRef_w`, `moEndPointRef_w`, `moGeneralCurveRef_w`, `moLineRef_w`, `moSingleFaceRef_w`, `moSolidRef_w`, `moCompReferenceCurve_c`, and `moCompSurfaceBody_c` identify reference objects rather than feature operations.

Keywords `Feature` elements use the `Type` attribute as their operation-family token. All feature instances with the same exact `Type` token use the same feature-input class. A directly object-ID-bound class instance therefore supplies the class of the other instances carrying that token. `Helix/Spiral`, `Surface-Sweep`, and `Thicken` denote helix, surface-sweep, and face-thickening operations independently of the localized display name in `Name`.

Sketch relations use named scalar records with reference cells at fixed scalar-record slots. Point references use `d6 80`, `7b 83`, or `7c bc`; line references use `e1 80`, `86 83`, or `87 bc`. Point-point, line-line, and point-line distance relations follow from the operand pair. Two `cb 8d` cells carry horizontal or vertical point-point distance according to the relation declaration. Two `da 8d` cells carry an angular relation. An `sgCircleDim` declaration followed by one `fe 83` cell carries a circle-diameter relation. Scalar records with the same owning sketch, relation family, and ordered operand sequence belong to one relation instance. Display-role and driving-role scalars are distinct. A unique driving scalar stores the target parameter.

Point-reference object indices address sketch-marker local identifiers within the owning feature object. A reference resolves when that local identifier is unique in the feature object.

Feature-input marker coordinates and the nested Parasolid profile use parallel two-dimensional axes with a constant translation per sketch feature. A unique translation mapping at least two distinct marker coordinates onto profile loci binds the matching markers to those loci. A reference marker whose linked endpoint markers share one profile entity identifies that entity.

Point-distance operands select explicit profile loci. Line-distance and angular operands select the profile entity shared by their linked endpoint markers. A relation with resolved operands and one driving scalar maps to the corresponding neutral distance, horizontal-distance, vertical-distance, angle, or diameter constraint.

`Helix/Spiral` history records use positional dimensions when explicit axis placement is absent: `D3` is the initial radius, `D4` is the signed total axial rise, `D5` is the positive revolution count, and `D7` is the start angle. The history record owns the unresolved construction axis.

Built-in reference-plane history records have no dimensions or extra attributes. Source IDs `2`, `3`, and `4` identify the Front, Top, and Right principal planes. Names, element tags, and type strings do not affect the role.

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

The schema identifier has the form `SCH_<modeller>_<schema>_<format>`. Partition and deltas streams contain body geometry records. Class-definition payloads use `C` for class, `I` for instance, `A` for attribute, `D` for data, and `Z` for a Z-block container.

### 3.2 Sites and attribute scope

An attribute id is **not** globally unique. A **site** is one validated outer block (identified by its marker offset). Partition and deltas streams in the same outer block share a site namespace; streams in different outer blocks are distinct sites.

Compact analytic records, `00 11` coedges, `00 12` vertex-uses, and `00 1d` points use `(site_id, attr)` identity because their attributes can repeat across sites. Bridges (`00 0e`), loop heads (`00 0f`), and edge-uses (`00 10`) carry globally unique attributes, but their references to site-scoped families remain in the referring record's site. Partition and deltas records in one site share an attribute namespace.

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

- **Bridge `00 0e`:** `refs[2]` = owning loop-head, `refs[4]` = primary surface carrier (compact analytic or `00 7c`), `marker` = face orientation versus the surface natural normal (`0x2b` forward / `0x2d` reversed). `ref0` = owner/use discriminator.
- **Loop head `00 0f`:** `refs[1]` = first coedge, `refs[2]` = owning bridge, `refs[3]` = next sibling loop head.
- **Edge-use `00 10`:** `refs[0]` = canonical forward coedge (`0x2b`), `refs[3]` = support curve (compact analytic or `00 86`).
- **Coedge `00 11`:** `refs[1]` owning loop, `refs[2]`/`refs[3]` reciprocal ring links (prev/next), `refs[4]` start vertex-use, `refs[5]` twin coedge, `refs[6]` edge-use, `marker` sense vs canonical (`0x2b` forward, `0x2d` reversed).
- **Vertex-use `00 12` / point `00 1d`:** `00 12.refs[4]` = point attr; a bare `00 1d` record has four references at body +6, requires reference 0 to be sentinel `0` or `1`, and stores xyz as three f64 BE at body +14, in metres. Attrs `0` and `1` are sentinels, not world points. A `[00 1d][attr]` adjacency-table entry does not satisfy the reference-0 sentinel invariant and is not a point record.

A support surface belongs to a face through `face -> bridge -> bridge.refs[4] -> carrier`. Face and carrier attribute equality does not establish ownership. The carrier reference uses the bridge's site.

### 4.1 Stored edge direction

```
canonical coedge = same-site coedge with attr == 00 10.refs[0]   (marker always 0x2b)
edge.start_vertex = canonical.start_vertex_use → 00 12 → 00 1d
edge.end_vertex   = partner coedge (same edge_use_attr).start_vertex_use → …
```

The `00 10.refs[0]` coedge anchors the stored edge direction. Sentinel attributes `0` and `1` do not reference vertex-use or point records.

### 4.2 Deltas encodings

Deltas streams re-encode records in prefixed/tripled forms (each ref stored as a `[hi][lo][01]` triple) or as `[disc][attr]` adjacency tables; the magic occurs at the family-specific position within the record window.

| Tag                    | Deltas form | Magic    | Anchor                                 |
| ---------------------- | ----------- | -------- | -------------------------------------- |
| `00 10`                | prefixed    | body +9  | ref slot 2 = curve carrier             |
| `00 11`                | tripled     | none     | slot4 vuse, slot5 twin, slot6 edge-use |
| `00 12`                | prefixed    | body +21 | refs-before-magic slot 4 = point attr  |
| `00 1d`                | prefixed    | none     | xyz after `[hi][lo][01]*` run          |
| `00 1e/1f/20/32/33/35` | prefixed    | none     | f64 block after `2b`/`2d` marker       |

Partition and deltas streams in the same outer block share a site namespace. Prefixed and tripled references encode the same u16 attribute values as bare references.

## 5. Entity records and face families

Top-level entity families: `00 51` entity, `00 52` wrapper/container, `00 53` color/property/helper, `00 54` metadata. Common header: `flags u32 BE`, `attr u16 BE`, `seq u32 BE`, `disc u16 BE`.

`flo` is the low byte of `flags`. An optional `ff` byte can occur between the `00 51` tag and `flags`; it shifts every following field by one byte. Entity-family bodies have fixed slot counts keyed by `(schema, disc, flo)`, so `00 51` and `00 52` byte values inside slots are data rather than record delimiters.

Face records use these families:

| Family       | Record invariant                                      |
| ------------ | ----------------------------------------------------- |
| disc14       | `00 51`, `disc == 0x0014`, six-u16 slot prefix        |
| disc15/flo=1 | `00 51`, `disc == 0x0015`, `flo == 1`, six-u16 prefix |
| disc20/flo=1 | `00 51`, `disc == 0x0020`, `flo == 1`, six-u16 prefix |

The bridge owner field `00 0e.ref0` joins the topology bridge to an entity-family face attribute.

---

## 6. Body records

An explicit body root is an entity record with `disc == 0x0017` and `flo == 2`:

```
00 51
[ff]?
flags      u32 BE       ; low byte = 2
attr       u16 BE       ; body identity within the site
seq        u32 BE
disc       u16 BE       ; 0x0017
slots      u16 BE[6]
```

Slot values `0` and `1` are sentinels. Values greater than 1 are entity attributes in the same site. Multiple disc17 records represent distinct stored bodies even when their face geometry touches.

Disc14 and disc15 face records use the common six-slot prefix in §5. Disc15/flo2 is a face-list-head family in schema 32001; disc15/flo1 is a face family in schema 33103. The exact body-to-face relation is carried by entity references, not geometric connectivity.

In schema 32001, `0x17.slot1` references a region directly or through `0x19.slot1`. A `0x1b/flo2` target denotes a solid region; a `0x1d/flo1` target denotes a sheet region. Solid shell ownership follows `0x1b -> 0x1f -> 0x21 -> 0x23`. The terminal `0x23` record reaches the owned face records.

In schema 33103, solid ownership follows the same `0x17 -> [0x19] -> 0x1b -> 0x1f -> 0x21 -> 0x23` hierarchy with `0x1b/flo1` as the solid region. `0x1d/flo2` belongs to the face-connectivity web and is not a sheet discriminator.

Schema-33103 canonical faces are the connected components of the disc15/flo1 adjacency graph. Disc13/flo2 face-list heads bind to bodies by the shared `slot0` cluster key. Each head seeds the component with maximum overlap in its section interval; component assignment is one-to-one. The complete component, not the interval contents, is the body's face set.

Disc14 ownership uses the entity-level shell and face-use lattice. A `0x1a` region reaches each `0x16` shell. A shell reaches its `0x20` face-use through same-site entity references; `0x20.slot3` advances around a shell ring. `0x20.slot2` resolves the canonical face directly or through `0x18.slot2` and `0x1e.slot2` intermediates. The ring closes when the next face-use equals the first. A partition containing one `0x1a` region and one reachable `0x16` shell owns every disc14 face when the `0x20` lattice maps one-to-one onto the complete disc14 face set.

In the disc20 face layout, a `0x1a` region reaches one `0x16` shell. Each canonical `0x20/flo1` face names a `0x24/flo4` node in slot 1. The `0x24` node back-references the face in slot 2 and names a `0x26/flo3` node in slot 1; the `0x26` node back-references the `0x24` node in slot 2. A complete reciprocal lattice assigns every disc20 face to the single shell.

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

The Parasolid partition and deltas grammar contains no two-dimensional UV pcurve control array. The `00 2d`, `00 7f`, and `00 80` arrays carry 3D or homogeneous control nets and knot data.

Planar pcurves are the exact inverse of the edge carrier in the support plane frame. Lines remain lines. Coplanar circles and ellipses remain analytic circles and ellipses with the same angular parameter; an edge axis opposite the plane normal reverses the parameter-plane rotation.

An elliptical edge on a cylindrical face has a polar-harmonic pcurve. Its radial-plane coefficients determine cylinder azimuth with `atan2`; its axial coefficients preserve the ellipse carrier's angular parameter. The radial harmonic has constant magnitude equal to the cylinder radius.

A coaxial circle on a circular cylinder or cone is a constant-axial-coordinate pcurve. A coaxial circle on a torus is a constant-minor-angle pcurve. The azimuth origin is the circle reference direction expressed in the surface frame; the azimuth parameter direction is positive when the circle and surface axes agree and negative when they oppose.

A spherical pole-closing edge has no spatial curve. Its pcurve is `v = π/2` over the azimuth interval `[0, 2π]`; every parameter value maps to the same pole vertex.

A NURBS surface boundary that shares a complete control row, knot vector, degree, and rational weight vector with its NURBS edge curve is isoparametric. A degree-one clamped surface column with equal endpoint weights is affine; a collinear spatial line has an exact affine pcurve obtained by projecting its origin and unit direction onto that column.

A quadratic rational NURBS edge on a cylinder has a polar-NURBS pcurve when every Bézier span satisfies the homogeneous polynomial identity `X² + Y² = R²W²` in the cylinder radial frame. Its axial control channel is the projection of the same spatial poles onto the cylinder axis. The pcurve shares the edge curve's degree, knots, weights, and parameter; its stored range is the interval whose evaluated endpoints coincide with the edge vertices.

A NURBS surface that is degree one and clamped in `u`, with equal weights at corresponding poles of its two control rows, is ruled in `u`. A spatial line coincident with a fixed-`v` ruling has an affine pcurve: `v` is the common row parameter and `u(t)` is the line parameter projected onto the evaluated ruling vector.

## 8. Auxiliary lanes

- **DisplayLists tessellation** uses a 6-descriptor table: List A strip lengths, Positions/Normals f32 metres, and Lists B/C/D. `C = sum(ListA)`, `ListC[i] = 2*ListA[i] - 2`, and `TriCount = C - 2*N`.
- **Materials / metadata** live in SW Objects blocks: `moVisualProperties_c` contains material names and RGB `0x00BBGGRR` values; names use UTF-16LE. `moBBoxCenterData_c` contains the bounding-box center and maximum radius in metres. `moDefaultRefPlnData_c` contains datum planes through the origin.
- **Per-face appearance** is generation-specific. A disc14 face is followed by an adjacent `00 53 flo=3` color record. A schema-33103 disc15 face stores the color-record attribute in slot 5. The color record stores RGB as three f64 BE values at body +6 and an inline `00 51` face link. An optional `ff` byte after `00 53` shifts the body by one byte.
- **XML / UnQLite** carry OPC parts, document/feature metadata, unit metadata (`SW_UnitsLinear=0` = millimetres), and MessagePack UI data: auxiliary, not the exact B-rep.

---

## 9. Units

- **Length fields are metres.** Model-space coordinates, radii, and length-bearing values in compact analytic records, `00 1d` points, and B-spline control grids are stored in metres.
- Coordinates and radii convert from metres to millimetres by a factor of `1000`. Rational B-spline poles require dehomogenization (`[x*w, y*w, z*w, w] -> xyz`) before conversion.
- Directions, normals, axes, reference directions, knot values, and weights are dimensionless.
- `SW_UnitsLinear=0` denotes millimetre document units. It does not change stored coordinate values.
- Model coordinates use the world frame. No per-file rotation, translation, or scale field applies to these coordinates.

---

## 10. Inline record framing

Inline `00 51` subrecords use a fixed slot count selected by the Parasolid schema, disc, low flag byte, and optional prefixed form. `00 51` and `00 52` byte occurrences do not delimit records.

For a prefixed subrecord, `body[14] == 0x01`, each slot is `[01][hi][lo]`, the final byte is `00`, and `end = pos + 14 + 3*slot_count + 1`. For a bare subrecord, `end = pos + 14 + 2*slot_count`.
