# SolidWorks `.sldprt`: Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

## 1. File container

SLDPRT uses two outer envelopes. The block envelope begins with the header and
frames below. The compound-document envelope begins with the OLE2 magic
`d0 cf 11 e0 a1 b1 1a e1`; its UTF-16LE directory contains the
`ISolidWorksInformation` stream.

The compound-document envelope uses Compound File Binary version 3 or 4. Its
header sector is 512 or 4096 bytes respectively; regular sectors have the same
size and mini sectors are 64 bytes. Header DIFAT entries and chained DIFAT
sectors identify FAT sectors. FAT chains identify the directory stream, the
root mini stream, the mini-FAT stream, and regular streams. Directory entries
are 128 bytes and carry a NUL-terminated UTF-16LE name, object type, left and
right sibling identifiers, child identifier, first sector, and u64 stream size.
The sibling trees and storage children form slash-qualified stream paths such
as `Contents/Config-0-Partition`.

Streams smaller than the 4096-byte mini-stream cutoff use 64-byte sectors in
the root storage stream and follow the mini FAT. Other streams follow the
regular FAT. In both allocation modes, the chain contains exactly
`ceil(stream_size / sector_size)` sectors and the final sector is truncated to
the directory entry's stream size.

Compound streams whose names end in `__ZLB` use a nested semantic-payload
wrapper. The wrapper is the 16-byte magic
`23 1d d5 71 da 81 48 a2 a8 58 98 b2 1b 89 ef 99`, followed by the
uncompressed byte count as u32 LE, the complete zlib-member byte count as u32
LE, that zlib member, and an 8-byte trailer. The zlib member expands to exactly
the declared uncompressed byte count. Decoders retain the wrapper bytes and
apply semantic parsers to the inflated bytes.

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

`Contents/Config-0-Partition` and `Contents/Config-0-Deltas` carry body B-rep records. `Contents/Config-0-ResolvedFeatures` carries feature-input sketch profiles. Legacy compound documents with no `ResolvedFeatures` stream store the same feature-input arena directly in the numeric `Contents/Config-N` stream. When explicit `ResolvedFeatures` streams exist, plain `Config-N` streams are not feature-input lanes. `Config-0-GhostPartition`, `Contents/Definition`, and `Config-0-LWDATA` are separate payload families.

`ResolvedFeatures` sketch entities begin with `ff ff 1f 00 03`. A little-endian u32 at marker +17 is interpreted with the record layout. Coordinate-bearing values `0`, `1`, `2`, and `3` denote point, curve, arc, and constrained point. Marker +48 stores a finite little-endian f64 state value. The trailing little-endian u32 is the feature-local object identifier; `ff ff ff ff` is null. A coordinate-bearing marker has the 12-byte prefix `ff ff ff ff ff ff ff ff 00 00 80 bf` at marker +5. One layout stores `1e 00` at marker +64 and two finite little-endian f64 coordinate fields at marker +66 and +74, in metres. Current and compact legacy profile-point candidates may instead store `1e 00` at marker +56 and the coordinate pair at marker +58 and +66. A compact legacy profile-point candidate ends at the next sketch marker at marker +134, +138, +146, +150, +154, +161, or +162. Legacy solved-geometry-locus markers use the same offsets for all geometry types. The four bytes `04 00 02 00` at marker +23 identify an extended profile vertex, or a coordinate marker used as a vertex when an indexed profile curve owned by the same feature references its object index. A coordinate-bearing geometry-locus marker selected by a resolved object-index, coordinate-roster, or local-link curve endpoint is likewise a profile vertex, irrespective of its native value, unless the marker itself resolves as a two-ended curve. Vertex status propagates transitively through the typed endpoint graph. Unselected coordinate markers with these loci are display handles and do not participate in solved sketch geometry. Marker +84 stores a little-endian u16 local-link count of one or two. Marker +86 begins that many adjacent 12-byte cells. Every cell uses one homogeneous curve-selector tag, followed by a little-endian u16 feature-local object identifier, `ff ff ff ff`, and four zero bytes. Two zero bytes and `fe ff ff ff` terminate the vector. Coordinate records are 142, 152, 154, 158, or 162 bytes. Their object identifier is layout-selected at marker +138, +148, +150, +144, or +158 respectively, and they may be followed by a four-byte separator before the next marker. The 92-byte reference-bearing variant stores two little-endian u16 feature-local object identifiers at marker +64 and +66, a little-endian u16 selector at marker +68, zero at marker +70, little-endian f64 `-1.0` at marker +72, and its object identifier at marker +88. Each referenced identifier is resolved independently against typed sketch markers owned by the same feature object as the referencing marker.

The compact `ff ff 07 00 01` generation stores its native value u32 at marker +13, profile locus `04 00 02 00` at marker +19, and role u16 at marker +23. A role-1 vertex has value `1`, `1e 00` at marker +42, and two finite f64 coordinates at marker +44 and +52 in metres. A role-1 indexed line has value `0`, state u16 `1` at marker +25, byte `04` followed by ten zero bytes at marker +31, zero-based endpoint u16 values at marker +42 and +44, u32 `1` at marker +46, f64 `-1` at marker +50, and the next sketch marker at marker +68. Adding one to each endpoint value gives the object index of an endpoint vertex owned by the same feature object.

A current- or extended-prefix 154-byte linked profile-point record stores value u32 `0` at marker +17, profile locus `04 00 02 00` at marker +23, role u16 `1` at marker +27, `1e 00` at marker +56, and the finite planar coordinate pair at marker +58 and +66. Marker +74 is zero, marker +76 stores link count u16 `2`, and two typed 12-byte curve-incidence cells occupy marker +78 through +101. Each cell stores a recognized curve-selector tag, a u16 curve identifier, `ff ff ff ff`, and four zero bytes. The bytes `00 00 fe ff ff ff` terminate the links at marker +102, marker +108 through +145 are zero, and marker +146 stores a non-null u32 record identity. The following marker's object identifier occupies marker +150, and its sketch prefix begins at marker +154. The extended prefix also has a 158-byte form with zeros from marker +108 through +143, non-null u32 identities at marker +144 and +148, zero u16 at marker +152, the following marker's object identifier at marker +154, and its sketch prefix at marker +158. For a current-prefix kind-1 indexed curve, its feature-local object index selects incidence cells by curve identifier. A selector tag supplies the curve's endpoints when exactly two linked profile-point records carry that tag and curve identifier and no second selector tag supplies another pair.

The compact generation's selected-axis line has value `0`, role u16 `2`, `0c` followed by seven zero bytes at marker +31, endpoint u16 values at marker +42 and +44, f64 `-1` at marker +50, zero u32 at marker +58, and the next sketch marker at marker +80. Its header bytes at marker +5 are `ff ff ff ff 04 00 ff ff`. Adding one to each endpoint value gives the object index of a coordinate-bearing endpoint marker owned by the profile object.

The compact `moCompRefPlane_c` reference record is 67 bytes. It stores a nonzero, non-null u32 identity at record +0, six zero bytes at record +4, the referenced plane's u16 feature-object identifier at record +10, 27 zero bytes at record +12, f64 `1` at record +39, and the 16-byte trailer `00 00 00 04 00 00 00 f9 ff ff ff 00 00 00 00 65` at record +47. The referenced identifier resolves against feature-object name identifiers in the same feature-input lane.

A current- or extended-prefix coordinate marker with value u32 `1` at marker +17, profile locus `04 00 02 00` at marker +23, role u16 `1` at marker +27, and `1e 00` at marker +56 is a profile vertex. Its coordinates are the f64 pair at marker +58 and +66 even when the record contains an inline line-handle declaration.

A current-prefix marker with value u32 `2` at marker +17, geometry locus `05 00 01 00` at marker +23, and role u16 `1` at marker +27 is a profile line. Its neutral geometry is the chord between its two linked or indexed point markers. This locus rule applies to coordinate-bearing and indexed record layouts. A current-prefix value-2 marker with profile locus `04 00 02 00` is instead a bounded arc.

An extended-prefix 134-byte value-2 profile-locus record carries `1e 00` at marker +56 and a finite planar center pair at marker +58 and +66. Marker +74 and marker +76 select either direct form `(1, 0)` or counted form `(0, 1..3)`. Marker +78 through +81 are zero. Marker +82 is zero in direct form and zero or one in counted form. Marker +84 stores i32 `-2`, marker +88 through +129 are zero, marker +130 stores the object index, and the next sketch marker begins at marker +134. When the next two coordinate-bearing markers owned by the same feature object are distinct and their midpoint equals the stored center, the record is the line chord between those markers. A non-midpoint center remains a native curve handle.

An extended-prefix kind `0` bounded curve in the 104-byte compact indexed layout carries a circular arc when exactly one coordinate-bearing geometry marker in the feature object is equidistant from its two distinct endpoints. The selected marker is the arc center, and the angle order represents the minor arc between the endpoints. Zero or multiple admissible centers leave the curve unresolved.

A compact-legacy kind `2` bounded curve with locus `05 00 01 00` and the compact indexed endpoint layout is a semicircle when exactly one feature-local point marker lies at the endpoint midpoint. That point is the circle center and the endpoint chord is the diameter.

The current, compact legacy, and extended marker prefixes have a solved curve or arc with role u16 `1` at marker +27 that uses a compact indexed record. Its locus is `05 00 01 00` or the profile locus `04 00 02 00`. It stores `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and two little-endian u16 endpoint indices at marker +56 and +58. Values `0` and `1` at marker +17 denote curves; value `2` denotes a bounded curve. For current-prefix records and compact-legacy records with locus `05 00 01 00`, adding one to each endpoint index produces the object index of a coordinate-bearing point marker owned by the same feature object. A compact legacy profile-locus record uses either zero-based ordinals in all coordinate-bearing geometry markers owned by the feature object, ordered by marker offset, or direct nonzero object indices when both fields uniquely select coordinate-bearing point markers. An extended-prefix 84-byte, 104-byte, or terminal 102-byte profile-locus record first interprets both fields as direct point object identifiers. Identifier zero selects the unique coordinate-bearing point without an explicit object identifier. If that pair does not resolve, both fields are direct ordinals in the complete feature-local sketch-marker roster in marker order; both addressed entries must be coordinate-bearing point markers. In an extended-prefix 96-byte record, adding one to each endpoint index produces the object index of a coordinate-bearing point marker owned by the feature object. A complete following role-2 detail record supplies the bounded curve's unit tangent and distinguishes a circular arc from a straight line. Without that detail record, a compact legacy profile-locus bounded curve is the straight endpoint chord. One layout is 84 bytes and ends immediately before the next sketch marker; its little-endian u32 at marker +80 is the object index, not an endpoint identifier. The 96-byte layout stores little-endian u32 selector `1` at marker +60, f64 `-1` at marker +64, zero u32 fields at marker +72 and +76, a zero u16 and state u16 at marker +80, zero u32 at marker +84, and two u32 identifiers at marker +88 and +92. The 104-byte layout stores the same selector and f64, signed selector `1` or `-1` at marker +72, a u16 state at marker +76, four consecutive little-endian i32 `-2` cells at marker +78, zero u16 at marker +94, and two u32 identifiers at marker +96 and +100. The 102-byte terminal layout has the same signed selector, state, and four i32 cells, followed by eight zero bytes at marker +94 and no following sketch marker.

In a compact-legacy profile-locus 104-byte record or terminal 102-byte record, equal coordinate-roster ordinals at marker +56 and +58 identify one radial witness of a complete circle rather than two curve endpoints. Ordinary circles have kind `1` or `2`, role u16 `1` at marker +27, state u16 `1` at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, and selector u32 `1` at marker +60. Their 104-byte form has the compact indexed trailer and a next sketch marker at marker +104; their terminal form has eight zero bytes at marker +94 and ends at marker +102. A construction circle has kind `7`, bytes `ff ff ff ff 04 00 ff ff` at marker +5, role u16 `2`, state u16 `0` at marker +29, `00 00 80 bf 00 00 0c 00` at marker +31, selector u32 `0` at marker +60, signed selector `1` at marker +72, state u16 `8` at marker +76, four i32 `-2` cells at marker +78, zero u16 at marker +94, and the next sketch marker at marker +104. Both forms store f64 `1` at marker +48 and f64 `-1` at marker +64. The ordinal addresses the complete feature-local coordinate-bearing marker roster in marker order, including relation markers with coordinates. The circle center is the unique coordinate position whose distance from the radial witness equals one positive diameter parameter owned by the profile, divided by two. Coordinate markers at the same position are one center candidate. Zero or multiple center-radius pairs leave the record unresolved.

The current, compact legacy, and extended marker prefixes have a solved curve or arc with locus `05 00 01 00` and role u16 `1` that uses a 92-byte record. All three prefixes also use this record with profile locus `04 00 02 00`. It stores `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, endpoint u16 values at marker +64 and +66, `01 00 00 00` at marker +68, and f64 `-1` at marker +72. Values `0` and `1` at marker +17 denote curves; value `2` is not construction geometry. In files whose active Parasolid schema has modeller generation 31 or later, a current-prefix profile-locus record uses zero-based ordinals in all coordinate-bearing geometry markers owned by the feature object, ordered by marker offset. A value-2 record without a complete following role-2 detail record is a line between those endpoints. For locus `05 00 01 00`, adding one to each endpoint value produces the object index of a coordinate-bearing point marker owned by the same feature object. For a compact legacy record with profile locus `04 00 02 00`, each value is instead a zero-based ordinal in all coordinate-bearing geometry markers owned by the feature object, ordered by marker offset. The endpoint trailer belongs to the curve record and is not a relation-link vector. The next sketch marker begins at marker +92.

A compact legacy 92-byte kind `0` profile line also uses either locus with `00 00 80 bf 00 00 05 00` at marker +31. It stores f64 `1` at marker +48, zero u32 at marker +60, zero-based coordinate-roster ordinals at marker +64 and +66, a u16 local-link selector at marker +68, reserved zero u16 at marker +70, and f64 `-1` at marker +72. The coordinate roster and endpoint semantics are the compact legacy profile roster defined above.

A compact legacy linked coordinate marker has kind `0`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, and f64 `1` at marker +48. Marker +56 stores coordinate tag `1a 00`; marker +58 and marker +66 store the finite planar coordinate pair in metres. Marker +74 is zero u16, and marker +76 stores local-link count u16 `2`. The 154-byte variant stores two homogeneous 12-byte cells at marker +78. Each cell stores its selector tag, a distinct nonzero u16 feature-local marker identifier, `ff ff ff ff`, and four zero bytes. Bytes `00 00 fe ff ff ff` terminate the links at marker +102, marker +108 through +149 are zero, marker +150 stores the coordinate marker's object index, and the next sketch marker begins at marker +154. The 146-byte variant omits each cell's four zero bytes, terminates the links at marker +94, stores zero bytes from marker +100 through +137, stores its local identifier at marker +138 and object index at marker +142, and begins the next sketch marker at marker +146. The 162-byte shifted variant stores eight zero bytes at marker +56, coordinate tag `1a 00` at marker +64, and the finite planar coordinate pair at marker +66 and +74. Marker +82 is zero u16, marker +84 stores link count u16 `2`, and two homogeneous 12-byte cells begin at marker +86. The terminator begins at marker +110, marker +116 through +157 are zero, marker +158 stores a nonzero, non-null local identifier, and the next sketch marker begins at marker +162.

A compact-legacy 170-byte profile point may carry an `sgLineHandle` child after its coordinate pair. The marker has value u32 `0` or `1`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite planar coordinates at marker +58 and +66 in metres. Marker +74 stores `00 00 03 00` for value `0` and `00 00 02 00` for value `1`, followed by `ff ff 01 00 0c 00` and the 12 bytes `sgLineHandle`. Marker +96 stores `03 00 ff ff ff ff 00 00 00 00`. A reference cell at marker +106 has a variable two-byte selector and little-endian u16 identifier, followed by `ff ff ff ff` and four zero bytes. Marker +118 stores `00 00 fe ff ff ff`, marker +124 through +165 are zero, and the next sketch marker begins at marker +170. The child makes the owning marker a point in both value forms.

An extended-prefix line-handle point has native code `0` or `2`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite planar coordinates at marker +58 and +66 in metres. The declaring 170-byte form has native code `2` and stores handle state `00 00 02 00` or `00 00 03 00` at marker +74, followed by `ff ff 01 00 0c 00` and `sgLineHandle`. Its null reference cell begins with u16 zero for state `2` and u16 `3` for state `3`, followed by `ff ff ff ff` and four zero bytes. One typed reference cell, `00 00 fe ff ff ff`, and 42 zero bytes complete the record before the next marker. The linked 154-byte form pairs native code `2` with state `00 00 02 00` or native code `0` with state `00 00 03 00`. It stores two typed 12-byte reference cells at marker +78 and marker +90 with one nonzero selector and consecutive u16 identifiers, `00 00 fe ff ff ff` at marker +102, 36 zero bytes at marker +108, two zero bytes at marker +148, and a nonzero u32 identity at marker +150 before the next marker. Both forms define points.

An extended-prefix compact linked line-handle point has native code `0`, `1`, or `2`, header bytes `ff ff ff ff ff ff ff ff`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite planar coordinates at marker +58 and +66 in metres. Marker +74 stores `00 00 02 00`. Marker +78 and marker +86 contain typed eight-byte reference cells with one common nonzero selector and consecutive u16 identifiers, followed by `ff ff ff ff`. Marker +94 stores `00 00 fe ff ff ff`, marker +100 through +141 are zero, and marker +142 stores a non-null u32 identity. The next sketch marker begins at marker +146 or follows a four-byte zero separator at marker +150. The record defines a point.

A current-prefix 146-byte geometry-locus profile point has native code `2`, locus `05 00 01 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, eight zero bytes, coordinate tag `1e 00` at marker +64, and finite planar coordinates at marker +66 and +74 in metres. Marker +82 stores u32 `1`, marker +86 through +91 are zero, marker +92 stores `fe ff ff ff 00 00`, marker +98 through +131 are zero, marker +132 stores a nonzero, non-null u32 identity, marker +136 through +141 are zero, marker +142 stores `ff ff ff ff`, and the next marker begins at marker +146.

A compact-legacy 146-byte inline arc has value u32 `2`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and eight zero bytes at marker +56. Marker +64 stores `1a 00`. The finite planar center pair is at marker +66 and +74, and the finite start and end pairs are at marker +96/+104 and marker +112/+120, in metres. Marker +86 through +91 are zero, marker +92 stores u16 `1`, marker +128 through +131 and marker +136 through +141 are zero, and the next sketch marker begins at marker +146. The two endpoint pairs have equal nonzero distance from the center. Each pair binds the unique coordinate-bearing point marker in the same feature object with equal coordinates.

A compact-legacy 138-byte inline corner arc has value u32 `2`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and tag `16 00` at marker +56. The finite planar corner pair is at marker +58 and +66. Marker +74 stores u16 `11`, marker +76 through +83 are zero, and marker +84 stores u32 `9`. The finite start and end pairs are at marker +88/+96 and marker +104/+112, in metres. Marker +120 through +123 and marker +132 through +133 are zero, marker +128 stores u32 `2`, marker +134 stores a non-null object index, and the next sketch marker begins at marker +138. The corner combines one coordinate from each endpoint; the opposite rectangle corner is the circle center. Exactly one of the two coordinate pairings must equal the stored corner, and the endpoints must be distinct and equidistant from the opposite corner. Each endpoint pair binds the unique coordinate-bearing point marker in the same feature object with equal coordinates.

When a revolution has no separate axis reference, one current-prefix compact or 92-byte indexed curve with value `2` at marker +17 carries the selected axis as its endpoint chord. An indexed curve with value `0` or `2` carries the selected chord when it is immediately followed by a complete role-2 curve-detail record. In both forms, every endpoint-owned point marker in the profile object lies on one side of the infinite line through the chord. Unreferenced coordinate handles do not participate in this bound. The curve remains profile geometry; the chord line is the revolution axis. A detailed arc therefore contributes its diameter, not its tangent, as the axis.

The extended-prefix horizontal-axis form is a 92-byte record with value u32 `4` at marker +17, profile locus `04 00 02 00` at marker +23, role u16 `2` and zero u16 at marker +27, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, zero through marker +63, endpoint u16 values at marker +64 and +66, zero u32 at marker +68, f64 `-1` at marker +72, and zero u32 at marker +80. Adding one to each endpoint value gives the object index of an endpoint point marker owned by the profile object. The endpoint chord is the selected revolution axis.

The current-prefix vertical-axis form is an 84-byte record with value u32 `5` at marker +17, profile locus `04 00 02 00` at marker +23, role u16 `2` and zero u16 at marker +27, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, endpoint u16 values at marker +56 and +58, zero u32 at marker +60, f64 `-1` at marker +64, and zero u32 at marker +72. Adding one to each endpoint value gives the object index of an endpoint point marker owned by the profile object. The endpoint chord is the selected revolution axis.

The current prefix also has an alternate 84-byte indexed-curve roster with geometry locus `05 00 01 00`. A role-1 curve has state u16 `1` at marker +29, `00 00 80 bf` at marker +31, a nonzero four-byte state at marker +35, f64 `1` at marker +48, zero-based endpoint u16 values at marker +56 and +58, u32 `1` at marker +60, f64 `-1` at marker +64, zero u32 at marker +72, two u32 identities at marker +76 and +80, and the next sketch marker at marker +84. Its marker +5 header is either eight `ff` bytes or `ff ff ff ff 00 00 ff ff`. Adding one to each endpoint value gives endpoint object indices.

The selected-axis member of the alternate roster has role u16 `2`, zero state u16 at marker +29, `00 00 80 bf 00 00 0d 00` at marker +31, zero u32 at marker +60, header `ff ff ff ff 04 00 ff ff` at marker +5, and two nonzero u32 identities at marker +76 and +80. A role-2 member with `00 00 80 bf 00 00 0c 00` and null identities is not selected. The selected member's endpoint chord is the revolution axis.

The compact-legacy selected-axis form uses role u16 `2` and endpoint u16 values at marker +64 and +66. Adding one to each endpoint value gives the object index of a coordinate-bearing endpoint marker owned by the profile object. The selected-axis reference establishes point semantics for those endpoints even when an endpoint marker reuses a curve or arc native code.

A compact-legacy 92-byte profile selected-axis record has value u32 `2`, profile locus `04 00 02 00`, role u16 `2`, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, and zero bytes from marker +56 through +63. Marker +64 and +66 are zero-based ordinals in all coordinate-bearing geometry markers owned by the profile object in marker order. Marker +68 is zero u32 and marker +72 is f64 `-1`. The state trailer stores `00 00 01 00` or `00 00 02 00` at marker +80 and zero u32 at marker +84. The identity trailer instead stores zero u32 at marker +80 and the same nonzero, non-null u32 identity at marker +84 and +88. The next sketch marker begins at marker +92. The endpoint chord is selected construction geometry, and both addressed markers are profile points irrespective of their native value.

Another compact legacy selected-axis form is a 92-byte kind `0` record with locus `05 00 01 00`, role u16 `2`, `00 00 80 bf 00 00 0d 00` at marker +31, f64 `1` at marker +48, two zero-based coordinate-roster ordinals at marker +64 and +66, zero u32 at marker +68, and f64 `-1` at marker +72. The coordinate roster contains every coordinate-bearing geometry marker owned by the profile object in marker order. The next sketch marker begins at marker +92.

A legacy-prefix 92-byte selected-axis record has profile locus `04 00 02 00` at marker +23, role u16 `2` and zero state u16 at marker +27, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, zero through marker +63, endpoint u16 values at marker +64 and +66, zero u32 at marker +68, and f64 `-1` at marker +72. Value u32 `5` at marker +17 has zero u32 at marker +80 and two distinct nonzero u32 identities at marker +84 and +88. Value u32 `6` at marker +17 has `00 00 02 00` at marker +80, zero u32 at marker +84, and a nonzero u32 identity at marker +88. The next sketch marker begins at marker +92. Adding one to each endpoint value gives endpoint object indices. The endpoint chord is the selected revolution axis. A role-2 value-3 record with endpoint values at marker +56 and +58 is not selected construction geometry.

A legacy wide indexed curve may end at marker +112 instead of marker +92. This trailer stores selector i32 `-1` or `1` at marker +80, a nonzero u16 local state at marker +84, four consecutive i32 `-2` reference sentinels at marker +86, zero u16 at marker +102, and two distinct non-sentinel u32 identities at marker +104 and +108. Its endpoint u16 values remain at marker +64 and +66; adding one gives endpoint object indices. The current-prefix 112-byte profile-line variant has kind u32 `2`, locus `05 00 01 00`, role u16 `1`, selector u16 `1` at marker +29, state bytes `00 00 44 00` at marker +35, eight zero bytes at marker +56, signed selector i32 `-1` at marker +80, and local state u16 `4` at marker +84. It uses the same four `-2` sentinels, zero u16, two distinct identities, and one-based endpoint object-index interpretation.

A terminal legacy wide indexed curve ends at marker +128. It has profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, zero-based coordinate-roster ordinals at marker +64 and +66, u32 `1` at marker +68, f64 `-1` at marker +72, and 48 zero bytes from marker +80 through +127. Its coordinate roster contains every coordinate-bearing geometry marker owned by the profile object in marker order.

A terminal legacy state-2 profile curve has kind u32 `0`, profile locus `04 00 02 00`, role u16 `1`, zero state u16 at marker +29, `00 00 80 bf 00 00 05 00` at marker +31, and f64 `1` at marker +48. The wide 92-byte form has eight zero bytes at marker +56, endpoint u16 identities at marker +64 and +66, zero u32 at marker +68, f64 `-1` at marker +72, `00 00 02 00` at marker +80, and zero u32 at marker +84. The compact 84-byte form shifts the endpoint, selector, f64, state, and trailing zero fields eight bytes earlier. The endpoint identities are consecutive. An identity selects the unique point whose local ID equals it or whose object index plus one equals it. The next sketch marker begins at marker +92 or +84.

A legacy unlocated geometry handle has kind u32 `0`, geometry locus `05 00 01 00`, role u16 `1`, zero state u16 at marker +29, `00 00 80 bf 00 00 05 00` at marker +31, and f64 `1` at marker +48. The wide form stores `12 00` at marker +64, 26 zero bytes, `fe ff ff ff` at marker +92, and 42 zero bytes through marker +137; the next sketch marker begins at marker +142 after its four-byte object-index prefix. The compact form shifts these fields and the next marker eight bytes earlier. The handle carries no coordinate, curve, or constraint incidence and defines no neutral sketch geometry.

A compact-legacy kind-1 profile curve in the 112-byte wide layout is a complete circle when its endpoint u16 values at marker +64 and +66 are equal and greater than zero. The common value is a zero-based ordinal in all coordinate-bearing geometry markers owned by the profile object in marker order. It selects a radial witness; the immediately preceding coordinate-roster marker is the center. The distance between those positions is the positive circle radius.

An extended-prefix horizontal relation may carry a boundary-axis candidate in a 112-byte record. It has value u32 `4` at marker +17, profile locus `04 00 02 00` at marker +23, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, zero through marker +63, endpoint u16 values at marker +64 and +66, u32 `1` at marker +68, f64 `-1` at marker +72, u32 `-1` at marker +80, a nonzero u16 state at marker +84, four consecutive i32 `-2` cells at marker +86, zero u16 at marker +102, and two u32 identities at marker +104 and +108. Adding one to each endpoint value gives endpoint object indices. When exactly one such relation chord has every object-indexed profile-curve endpoint on one side of its infinite line, that chord is the selected revolution axis.

The selected axis line may be omitted from the curve roster while its two endpoint markers remain. In this form, exactly two coordinate-bearing point markers in the profile object are not endpoints of any indexed profile curve. Every object-indexed profile-curve endpoint lies on one side of the infinite line through their chord. The two retained points are the revolution axis endpoints.

The selected axis may instead omit its line while retaining one coordinate-bearing marker at the profile origin. Candidate axis lines pass through that origin marker and an object-indexed profile-curve endpoint. Collinear candidates denote one line. Among candidates having every object-indexed profile-curve endpoint on one side, the unique line incident to the greatest number of distinct object-indexed profile-curve endpoints is the revolution axis.

The selected axis and its endpoint markers may both be omitted when the axis is a principal axis of the profile sketch. The horizontal and vertical lines through the profile origin are candidates. A candidate must have every object-indexed profile-curve endpoint on one side and be incident to at least two distinct object-indexed profile-curve endpoints. The candidate with uniquely greatest endpoint incidence is the revolution axis.

An omitted axis may coexist with other unreferenced coordinate handles. In this form, exactly two non-curve endpoint markers store little-endian u32 `1` at marker +76. Every object-indexed profile-curve endpoint lies on one side of the infinite line through the selected pair.

An omitted axis may share its first point with a profile curve. Its sole non-curve endpoint immediately follows that shared coordinate-bearing point in the feature-local marker roster and stores little-endian u32 `1` at marker +76. Every object-indexed profile-curve endpoint lies on one side of the infinite line through the retained pair.

The extended marker prefix `ff ff 1f 00 01` has a profile-vertex layout with type u32 `1` at marker +17, `04 00 02 00` at marker +23, role u16 `1` at marker +27, `1e 00` at marker +56, and vertex coordinates at marker +58 and +66. Coordinate-bearing geometry values use record boundaries at marker +134, +138, +140, or +144. Its 84-byte curve records use the same marker +23 and +27 values, u32 `1` at marker +60, and zero-based endpoint u16 values at marker +56 and +58. Each endpoint value indexes all coordinate-bearing geometry markers owned by the feature object in marker order. A curve followed at marker +84 by a complete coordinate-bearing marker is a line. A curve followed by the extended marker prefix with a non-coordinate curve-detail body uses that detail to distinguish line and arc geometry.

Four extended-prefix 84-byte value-1 profile-locus records define an axis-aligned rectangle when their zero-based endpoint values form one simple four-vertex cycle in the complete feature-local sketch-marker roster and exactly two addressed roster entries are coordinate-bearing point markers at opposite cycle vertices. The two stored point coordinates must differ on both axes. The other two corners are the remaining members of the Cartesian product of those axis coordinates. Marker +72 is zero u16, marker +74 is state u16 `0` or `2`, and the following record begins at marker +84.

The same extended 84-byte line layout may use geometry locus `05 00 01 00`. A complete sketch marker at marker +84 distinguishes the line from a non-coordinate curve-detail body.

An extended-prefix compact profile curve with native code `1` or `2` and geometry locus `04 00 02 00` may store two zero-based feature-local point object identifiers as u16 values at marker +56 and marker +58. Object identifier zero selects the unique coordinate-bearing point without an explicit object identifier. The pair is valid only when both identifiers resolve uniquely to distinct point markers. This direct-object form may end at a marker at +84 or +104, or at the 102-byte terminal record boundary. The terminal form may store either the compact `-1` or `1` selector and reference sentinel, or 30 zero bytes after the f64 `-1` at marker +64.

An extended-prefix full circle has native code `0`, geometry locus `05 00 01 00`, role u16 `1`, u16 `1` at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, and f64 `1` at marker +48. Equal nonzero u16 values at marker +56 and marker +58 select a zero-based point from the feature-local coordinate point roster; roster point zero is the center and the selected point is radial. Marker +60 stores u32 `1`, marker +64 stores f64 `-1`, marker +72 stores i32 `1`, marker +78 through +93 store four `fe ff ff ff` sentinels, and marker +94 is zero u16. The record ends at a sketch marker at +104 or at the 102-byte terminal boundary. The distance from the center point to the radial point is the circle radius.

A compact-legacy 162-byte full-circle record has native code `2`, geometry locus `05 00 01 00`, role u16 `1`, zero u16 at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, eight zero bytes at marker +56, coordinate tag `1e 00` at marker +64, and the finite center pair at marker +66 and +74. Marker +82 is zero and marker +84 stores link count u16 `2`. Two typed 12-byte reference cells occupy marker +86 through +109; they share one nonzero selector, carry nonzero u16 indices, and end with `ff ff ff ff` and four zero bytes. The terminator `00 00 fe ff ff ff` occupies marker +110, followed by 42 zero bytes and the next point's u32 object identifier at marker +158. The point record beginning at marker +162 has that object identifier and supplies the radial point. The center-to-point distance is the circle radius.

An `sgSlot_c` declaration may immediately precede a current- or extended-prefix slot record with geometry locus `05 00 01 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, and f64 `1` at marker +48. Six 12-byte typed reference cells begin at marker +72 for the current prefix and marker +64 for the extended prefix. Every cell stores a nonzero u16 selector, a u16 index, `ff ff ff ff`, and four zero bytes. The first, third, and fourth cells have one selector; the second has a distinct selector. Their indices order the four feature-local non-coordinate curve markers around the slot boundary. The fifth and sixth cells have a third selector and index the feature-local coordinate-bearing point roster. In the four-curve form, the boundary indices are one permutation of `0..3`, the center indices select two distinct points, and the ordered boundary contains two lines and two arcs. When one arc is native, the other three curves form one unambiguous partial cycle, and exactly one selected center is not the solved arc's center, the missing arc joins the two open cycle endpoints about that remaining center. The completed boundary is one closed profile.

A detailed curve record is immediately followed by a curve-detail marker of the same generation: at curve +84 for the compact record, curve +92 for the 92-byte wide indexed record, and curve +112 for the 112-byte wide indexed record. The detail stores `ff ff ff ff 04 00 ff ff` at detail +5, repeats the curve's four-byte locus at detail +23, stores role u16 `2` at detail +27, `00 00 80 bf 00 00 0c 00` at detail +31, f64 `1` at detail +48, and a unit 2D start tangent as f64 values at detail +64 and +72. A tangent parallel to the endpoint chord defines a line. Otherwise the tangent and endpoints determine one circle: its center lies on the endpoint normal to the tangent and is equidistant from both endpoints. The bounded arc is the minor sweep between those endpoints.

Keywords feature attributes that contain object identifiers use the feature's `id` namespace. `DissectableChildren` is a separator-delimited ordered list of child object identifiers. A single sketch child of an extrusion is that extrusion's profile dependency.

Keywords records bound to `moAlignGroup_c`, `moAttribute_c`, and `moConfigCommentsFolder_c` are metadata objects, not regeneration features. A classless source `-1` record whose name prefixes an `moAttribute_c` instance name is that attribute family's definition object.

A classless Keywords `Feature` whose ordered dimension children exactly cover its parameters uses the parameter schema as its operation class. A `D2` value with a diameter modifier and no parameter outside `D1` and `D2` is a cosmetic thread. Exactly `D1` and `D2`, with positive distance `D1` and an explicitly unit-suffixed `D2` angle strictly between zero and pi, is a distance-angle chamfer.

An extrusion bound to `moCut_c` has Boolean operation cut independently of its localized Keywords type token.

In the legacy repeated-class form, a non-direct feature name carries a u16 class token immediately before its name marker. A token group is a Hole Wizard group when the feature-input lane declares exactly one Hole Wizard class and every uniquely named history feature in the group is immediately followed by two consecutive profile-feature records. The two profiles are the operation's position and generated-profile sketches.

A generated Hole Wizard profile with two diameter-displayed dimensions, two length dimensions, and one angle dimension is threaded when its ordered dimension roles are diameter, length, diameter, length, angle. These fields are the drill diameter, drill depth, nominal thread major diameter, thread depth, and drill-point included angle. The profile is counterbored when the same dimension multiset terminates with a diameter-displayed dimension: the smaller and larger diameters are the drill and counterbore diameters, the smaller and larger lengths are the counterbore and drill depths, and the angle is the drill-point included angle. A profile with one diameter, one length, and one angle carries a simple drilled hole with drill diameter, drill depth, and drill-point included angle.

Keywords element order is serialization order, not regeneration order. Neutral regeneration order is the stable topological order of parent and dependency references; unrelated features retain their serialization order.

Feature-tree node roles are invariant across configurations. A configuration snapshot changes a tree node's suppression and graph state without changing its role.

A named feature-input object bound to a classless history `Sketch` record with a nonzero source identifier is a profile-feature object. It participates in the same object-order ownership rules as an object whose class is `moProfileFeature_c`.

An extrusion feature-input object stores a little-endian u32 form code before its object-name record. A direct class declaration is preceded by the form code and four or eight zero bytes. A repeated-class name is preceded by the form code, four or eight zero bytes, and its little-endian u16 class token. A repeated `moICE_c` object may instead store the form code immediately before its non-sentinel high-bit class token. The padding width is selected by the record schema and is self-delimiting because every padding byte is zero. A direct `moICE_c` declaration immediately preceded by `ff ff ff ff` is the sentinel cut form.

An extrusion object-name record is followed by four zero bytes, a little-endian u16 family word, a one-byte Boolean operation, one schema byte, the repeated little-endian u32 object identifier, and four zero bytes. The terminated trailer then stores `ff fe ff`. The sparse trailer instead stores six zero bytes, `01 00`, a nonzero u16 token, 12 zero bytes, and a second nonzero u16 token. The family word is `0x0140` on `moExtrusion_c` objects and `0x01ca` on `moICE_c` objects. Operation `00` on an `moExtrusion_c` object joins the extrusion result. Operation `02` on an `moICE_c` object subtracts it. Operation `00` on an `moICE_c` object does not carry the Boolean operation; the object falls back to class-scoped form semantics. Objects without either complete trailer use the same form semantics. `moExtrusion_c` form codes `1`, `4`, and `82` join. `moICE_c` form codes `6`, `21`, and `0x3ee4f8b5` join; codes `1`, `2`, `5`, `7`, `10`, `14`, `15`, and `0xffffffff` subtract. Form code `3` joins and form code `11` subtracts for either class.

A compact extrusion end-spec child carries its class either as a lane-scoped class token whose high byte has its high bit set and which is not `ff ff`, or as a direct `ff ff 01 00 0b 00 moEndSpec_c` declaration followed by two zero bytes. The direct form uses the declaration's final `_c` bytes as its anchor; the declaration ends at anchor +2. Header-shaped byte runs with zeros at the class position belong to fillet edge-set records of the `edgeRadiusObject_c` family, not to end specs; those runs carry a class token at anchor +24, the constant `02 02 01 00` at anchor −32, and two f64 `0.5` values ending at anchor −11 and anchor −3. Anchor +2 is zero, anchor +4 and anchor +8 are little-endian Boolean words, anchor +12 is a Boolean direction flag, and anchor +16 is zero. The little-endian u32 first-direction termination code is at anchor +18 and the little-endian u32 second-direction termination code is at anchor +22. A second-direction code `0` means the extrusion travels in one direction only, and code `1` is a through-all second direction. Code `0` is blind, code `1` is through-all, code `2` is through-next, and code `9` is through-all in both directions and always carries second-direction code `1`. A blind child has second-direction code `0` and owns its depth as the display-distance dimension child at anchor +26; the dimension name does not alter its role. The compact blind form omits the second-direction word and begins the same dimension child at anchor +22. A single-direction through-all or through-next child has two zero u32 words after the first-direction code, `01 00 00 01`, 56 zero bytes, `00 00 01 00`, six zero bytes, and either a u16 follow-on object token with two zero bytes or the direct-class prefix `ff ff 01 00`. A single-direction through-all child may instead own a display-distance dimension child immediately at anchor +26; that retained display dimension does not change the termination. The dimensioned traversal form has two zero u32 words after the first-direction code, `01 00 00 01`, ten zero bytes, little-endian u32 `1`, twenty zero bytes, and the display-distance dimension child at anchor +68; its retained dimension likewise does not change the termination. A two-direction child with first-direction code `1` continues after a zero u32 word at +26 with the same `01 00 00 01` run; both directions are through-all. A two-direction child with first-direction code `9` continues at +26 with a display-distance dimension child carrying the retained blind depth, which the through-all-both termination does not consume. A blind child with a through-all second direction carries first-direction code `0`, `1` at +4, second-direction code `1`, and its first-direction depth as the dimension child at +26. End-spec children belong to the extrusion object whose bound feature-name record precedes them; that object extends through immediately following profile-feature and `moCosmeticThread_c` child objects to the next feature-name record.

When operation objects and their dimension children form separate ordered groups, a blind end-spec belongs to the unique extrusion whose sole dimension has the same name and value as the first named scalar following that end-spec. A zero or multiple matching extrusion leaves the end-spec unbound.

Termination code `6` is mid-plane and code `5` is offset-from-face. The child has one zero u32 word after the code and then a display-distance dimension child, either as a direct `moDisplayDistanceDim_c` class declaration or as a repeated lane-scoped class token followed by two zero bytes. The dimension child continues with a 16-byte block. Byte 8 is `0` or `0x40`, byte 9 is a flags byte whose low three bits are clear, and every other byte is zero. The block is followed by `ff ff 00 00`, a mark byte `01` or `03`, `ff ff ff ff`, four zero bytes, and the little-endian f32 `-1.0`. A mid-plane extrusion owns its `Depth` or `D1` scalar as the total travel split evenly around the profile plane. A mid-plane extrusion without a `Depth` or `D1` scalar that owns exactly one length-valued scalar under a user-defined name uses that scalar as the total travel. An offset-from-face extrusion owns its dimension scalar as the offset distance from the terminating face. That face follows later in the same feature interval as the bytes `01 01 00` and an `moSingleFaceRef_w` child, either as a direct class declaration or as repeated lane-scoped class tokens, whose reference body opens with a token followed by `02 00 00 00 40 00 00` and continues with a termination-reference selection vector selecting the face.

A legacy `moSingleFaceRef_w` body starts with two non-null high-bit u16 tokens, u32 value `2`, a one-byte form `0` or `0x40`, two zero bytes, and two copies of one nonzero u32 owner identifier. After zero fill, a control record begins at body +44 or +48 with a non-null high-bit u16 token, u32 value `1`, four zero bytes, a u32 component count, `00 02 00 00`, one four-byte value, and two identical eight-byte witnesses. Component entries begin 40 bytes after the control record. Each has a non-null high-bit u16 instance, two zero bytes, a 12-byte type signature, and an optional u32 local identifier; zero gaps of 2, 4, 6, or 8 bytes separate entries. The serialized count is either the component count or exceeds it by two implicit roots. The path terminates with `ff ff ff ff` and four zero bytes, or with 20 or 24 zero bytes followed by a nonzero u32 source identifier.

Termination code `3` is up-to-vertex. The child has two zero u32 words after the code and then a point-reference child at +30 in one of two forms. A point form is declared `ff ff 01 00 0c 00 moPointRef_w` or begins directly with its repeated lane-scoped tokens; its body is a class token, a second token `a9 80` or `2b 80`, u32 `2`, a zero selector byte, and two zero bytes, followed by object identifiers and a termination-reference selection vector whose final counted entry's component identifier is the feature-local vertex identifier. An edge-endpoint form is declared `ff ff 01 00 0f 00 moEndPointRef_w` with a nested `ff ff 01 00 0c 00 moCompEdge_c` child whose body is a class token, u32 `2`, and selector byte `40`; an `moEdgeRef_c` child and a u32 endpoint selector precede the selection vector, which selects the edge carrying the endpoint.

Termination code `4` is to-face. Token +22 is a Boolean reference-side flag, token +26 is zero, and +30 through +32 are `01 01 00`. The following child is an `moSingleFaceRef_w`, either as a direct class declaration or as a repeated lane-scoped class token. Its body starts with two non-null lane-scoped class tokens followed by `02 00 00 00 40 00 00`. Its selection vector uses the duplicated 16-byte component marker. Marker −12 stores a positive little-endian u32 slot count, marker −8 is `00 02 00 00` or `00 03 00 00`, and marker +16 is zero. An earlier single-face path form instead repeats a nonzero owner word at body +11 and +15. Its path control begins at body +44, +48, +84, or +88. Bytes between body +19 and the control are zero, or contain one 16-byte `ff` run with zero bytes on both sides. The control has a non-null lane token, a little-endian u32 `1`, four zero bytes, a positive slot count, selector `00 02 00 00` or `00 03 00 00`, and two equal eight-byte identity words. Its path entries begin at control +40. Path entries have a four-byte instance cell whose u16 token is non-sentinel and has its high bit set, a 12-byte type signature, and an optional little-endian u32 component identifier. The slot count equals the typed-entry count or includes one or two serialized root slots; a root-adjusted path is complete only when the resulting typed entries end at a terminal slot. A positive u16-range u32 path-slot word may separate entries. Inter-entry padding is zero fill of two, four, or eight bytes, `ff ff ff ff`, `ff ff ff ff 00 00 00 00`, `a0 86 01 00 00 00 00 00`, or `ff ff ff ff` followed by six zero bytes. Signature bytes 4 through 7 are the little-endian native object id of the feature traversed by that path entry; one path may traverse multiple feature identities. The final counted slot may be `ff ff ff ff 00 00 00 00` or a terminal-owner cell containing 20 or 24 zero bytes and the nonzero little-endian u32 native object id of the feature result owning the selected face. The same inter-entry gap grammar may separate the final typed component from this terminal slot. Otherwise the final entry's signature identifies that result. An explicit terminal owner is also a producer of the selected surface even when its identity is absent from the typed component path. The final typed component identifier is the result's feature-local face id. The ordered typed components form the native terminating-face path and are retained as a feature-input surface selection owned by the extrusion. The consuming extrusion depends on every uniquely identified history feature traversed by the path. Up-to-vertex and offset-from-face termination-reference vectors share this grammar and may additionally carry a leading identifier-less component cell that repeats the first counted entry's signature, an `a0 86 01 00` filler word, or an `01 00 00 00` slot word between counted entries; their counted slots need not all carry component entries. Up-to-vertex and offset-from-face terminations are likewise retained as feature-input surface selections, and the up-to-vertex and offset-from-face extrusions depend on every uniquely identified traversed feature.

An extrusion object without an `EndCondition` attribute, without an owned `Depth` or `D1` scalar, and without a decoded compact end-spec termination has an unresolved extent. The class, profile reference, direction, draft, and Boolean operation remain independently meaningful.

An extrusion object without `Profile` or `DissectableChildren` has an unresolved profile unless it has the following dissected-child signature. A nested profile stream owned by an extrusion resolves the profile to its transferred sketch.

An extrusion immediately followed by an `moProfileFeature_c` object whose `Description` equals its name and whose name ends in `<n>` for decimal `n` uses that following feature as its dissected profile. This child signature applies when the extrusion omits its `Dissectable` property and supersedes an immediately preceding generic profile object. An ordinary following profile without the child signature is not an extrusion operand.

A dissected child with exactly one dependency on a planar sketch feature selects that feature's complete profile when the sketch contains exactly one profile chain. The child is a profile-selection tree record, not another owner of the sketch. A sketch with multiple profile chains does not identify which chain the child selects and leaves the child profile unresolved.

A planar Parasolid profile stream is enclosed by the feature object whose bound feature-name record precedes the stream offset and whose next bound feature-name record follows it. A sweep object with exactly one enclosed planar profile stream uses the transferred sketch as its cross-section profile. Zero or multiple enclosed profile streams leave the sweep profile unresolved.

A compact line-only planar profile carries one `moSketchRegion_c` object in its feature interval. The class name is followed by a schema-local u16 region token, a u16 curve count, and that many ordered 12-byte curve references. Every reference contains one common curve-family u16 token, a one-based u16 line-handle ordinal, `ff ff ff ff`, and four zero bytes. The ordinals form the complete set `1..count`; their stored order is the closed profile order. The sole `sgLineHandle` declaration in the interval owns the consecutive coordinate-bearing handle records beginning with the record that encloses the declaration. Each ordered handle coordinate is the start of its line and the next ordered coordinate is its end; the final line ends at the first coordinate.

A compact line-only planar profile may instead carry an `moSketchChain_c` roster. Its u16 vertex count is followed by that many one-based u32 point-handle ordinals. The ordinals form the complete set `1..count`. The roster continues with u32 `1`, u16 zero, u32 `count + 2`, `ff ff ff ff`, eight zero bytes, two u32 values equal to `count + 1`, `ff fe ff 00 00 00`, and `ff ff ff ff`. The unique consecutive run of `count` coordinate-bearing point or constrained-point handles in the feature interval supplies the vertices. Roster order is closed profile order; each ordered vertex begins one line and the next ordered vertex ends it.

When one compact profile contains both rosters with the same count, corresponding region and chain entries are the first and second handle ordinals of one line. Each line retains both native endpoint identities, and shared physical coordinates have one canonical locus. The line set is ordered into the closed profile by shared endpoint incidence. Every handle is incident to exactly two lines, all lines belong to one cycle, and each profile use records whether traversal reverses the stored endpoint pair.

If the paired entries do not consume every addressed handle as one closed cycle, four unique addressed handle coordinates still define a line profile when their sketch coordinates are exactly the Cartesian product of two distinct u values and two distinct v values. The profile is the four axis-aligned perimeter segments in counterclockwise corner order. Duplicate corners, a third value on either axis, or a missing Cartesian corner do not define a profile.

A marker-only profile without either roster defines the same rectangle when exactly one set of four owned coordinate-bearing handles forms such a Cartesian product and its two positive axis spans match two distinct owned driving linear-dimension scalars. Linear-dimension scalars store metres; comparison uses their millimetre values. Angular and circular-diameter scalars do not select rectangle spans. Multiple dimension-compatible Cartesian products leave the profile unresolved.

A marker-only circular profile with exactly one coordinate-bearing line-or-circle handle uses that handle coordinate as its common center. Its coordinate-bearing point and constrained-point handles are radial witnesses. The profile defines one circle per owned diameter-displayed length when the number of radial witnesses equals the number of diameters and Euclidean marker distance establishes a one-to-one match between every witness and one half-diameter. Missing, repeated, or ambiguous matches leave the circles unresolved.

An `moRevolution_c` or `moRevCut_c` history record stores its one-sided revolution angle in named parameter `Angle`, otherwise in `D1`, otherwise in the first dimension child in feature-content order whose value is a positive history angle. A two-sided revolution uses the corresponding `Angle2`, `D2`, or second ordered positive angle dimension. `moRevCut_c` is a subtractive revolution and therefore has cut Boolean semantics.

The revolution object interval may contain a declared or repeated `moLineRef_w` instance that stores its profile owner and placed revolution axis. Before the handle words, one source cell contains the u32 source identifier of one profile object in the same history, a nonzero opaque u32 identity, a non-null high-bit u16 token, a u16 variant, and `ff ff ff ff`. Two or three consecutive `c7 cf ff ff` handle words are followed by a zero u32 and a nonzero u32 stream address. Zero padding of 0, 4, or 8 bytes precedes a scalar record containing 6, 8, or 9 little-endian f64 values. The longest structurally terminated scalar layout is the record layout. Its first xyz triple is a point on the axis in metres and its final xyz triple is the unit axis direction. Zero padding of at most 24 bytes separates the scalar record from either the next class declaration or the next non-null high-bit instance token. Zero or multiple source-and-line placements in the object interval leave the placed axis unresolved.

A declared `moLineRef_w` variant stores the source cell at declaration offset `+64`, followed by u32 values `1`, `1`, `0`, a nonzero stream address, 12 zero bytes, and two `c7 cf ff ff` handle words. Sixteen zero bytes follow the handles. Six little-endian f64 values at declaration offset `+132` store the axis point in metres and unit axis direction as consecutive xyz triples. At most 24 zero bytes separate the scalar record from the next class declaration.

A three-handle declared variant stores the source cell at declaration offset `+100`, followed by a zero u32, a nonzero variant u32, u32 value `1`, a zero u32, a nonzero stream address, 12 zero bytes, and three `c7 cf ff ff` handle words. One record layout stores a zero u32 after the handles and nine little-endian f64 values at declaration offset `+164`. Another stores a zero u32, a nonzero stream address, 12 zero bytes, and `ff ff ff ff` after the handles, followed by eight little-endian f64 values at declaration offset `+184`. The scalar record's first xyz triple is the axis point in metres and its final xyz triple is the unit axis direction. At most 24 zero bytes separate the record from the next class declaration.

An extended two-handle variant stores the source cell at declaration offset `+64`, followed by a zero u32, a nonzero variant u32, u32 value `1`, a zero u32, a nonzero stream address, 12 zero bytes, and two `c7 cf ff ff` handle words. Eight little-endian f64 values at declaration offset `+120` store the axis point in the first xyz triple and unit direction in the final xyz triple. At most 24 zero bytes separate the record from the next class declaration.

The nested-sketch-handle variant stores the source cell at declaration offset `+87`, followed by a nonzero variant u32, u32 value `1`, a zero u32, a nonzero stream address, 12 zero bytes, and two `c7 cf ff ff` handle words. A zero u32, a nonzero stream address, and eight zero bytes follow the handles. Seven little-endian f64 values at declaration offset `+155` store the axis point in the first xyz triple and unit direction in the final xyz triple. The eight-byte cell immediately after scalar seven is nonzero and selects the seven-scalar record length.

An `moTempAxisRef_w` declaration stores two `c7 cf ff ff` handle words at declaration offsets `+223` and `+227`, followed by a zero u32 and a nonzero stream address. Nine little-endian f64 values at declaration offset `+239` store the axis point in metres in the first xyz triple and the unit axis direction in the final xyz triple. At most 24 zero bytes separate the record from the next class declaration.

The Hole Wizard `moTempAxisRef_w` form stores the handle pair at declaration offsets `+267` and `+271`, a zero u32 at `+275`, and a nonzero stream address at `+279`. Seven little-endian f64 values begin at `+299`: the first is the hole depth in metres, the next xyz triple is an axis point in metres, and the final xyz triple is the unit operation direction. The scalar record ends at `+355`; at most 24 zero bytes separate it from the next class declaration or typed high-bit token.

An `moRevolution_c` object's Boolean form is the little-endian u32 followed by four zero bytes immediately before its declared class marker or compact object-name token. Forms `5`, `6`, `11`, `60`, `20322`, and `22016` create a new body. Form `8` joins existing bodies. Other forms have unresolved Boolean semantics.

An `moRevolution_c` or `moRevCut_c` object consumes the immediately preceding profile feature-input object as its profile. When profile objects and revolution objects form separate ordered groups, the line-reference source identifies the consumed profile object. A profile history record without an XML source ID uses its unique feature-input object ID. A non-unique display name does not establish object identity.

The compact `moCompRefPlane_c` record binds profiles to a principal plane. Its nonzero little-endian u32 feature source identifier precedes an opaque u32 identity, `00 00 03 00`, 27 zero bytes, the little-endian f64 value `1`, three zero bytes, a roster count from `2` through `4`, three zero bytes, one byte `f9`, `fb`, or `ff`, three `ff` bytes, four zero bytes, and an object-kind byte of at least `65`. A record inside the profile object applies to that profile. Otherwise a record spanning the immediately preceding object through the profile applies. A sole declared record in the complete feature-input lane is the lane default. A profile-local or immediately preceding component reference overrides that default; component references belonging to other profile objects do not make the declared default ambiguous. Source identifiers `2`, `3`, and `4` select the Front, Top, and Right principal planes. Their model-space `(origin, normal, u-axis)` frames are respectively `((0,0,0),(0,-1,0),(0,0,-1))`, `((0,0,0),(0,0,1),(1,0,0))`, and `((0,0,0),(1,0,0),(0,0,-1))`.

An offset reference-plane object has a length-valued `D1`. An angular `D1` does not carry offset distance; a reference-plane object with angular `D1` and a complete frame carries that frame as direct datum-plane placement. An offset reference-plane object identifies its source plane with the unique nonzero little-endian u32 source identifier immediately followed by `02 00 00 00 00 05 00 00 00 00 00 00 00 00 00 2d 80 2b 80`. The typed 46-byte form stores the source identifier at +0. A principal-plane target has signature `79 2a e1 3b` at +4, zero u16 at +8, and principal selector u32 `1` or `2` at +10. A constructed-plane target has signature `30 92 ab 53` at +4 and six zero bytes at +8. Both forms store u32 `1` at +14, u32 zero at +18, an identity word at +22, twelve zero bytes at +26, and terminator `c7 cf ff ff c7 cf ff ff` at +38. Exactly one known non-self source across the compact and typed forms identifies the reference. The object may instead carry one complete plane frame whose plane is parallel to exactly one decoded datum frame at the absolute `D1` distance; that datum is the source plane. Coincident or multiply matching frames do not identify a source. The offset plane inherits the source plane normal and u-axis and translates its origin by the signed dimension along the normal.

The dual-frame offset layout stores exactly two complete 97-byte fixed frames in the reference-plane object interval. The first frame is the offset result and the second frame is its source plane. Their normals and u-axes have the same direction, their origin displacement has no component tangent to the source plane, and the absolute normal displacement equals `D1`. The second frame identifies exactly one geometrically coincident preceding datum plane or reserved principal plane. A reserved principal plane remains a valid source when its feature-tree record follows the offset-plane record because its native identity denotes a built-in datum.

A component reference-plane record may instead store its nonzero u32 feature source identifier before an opaque u32 identity, six zero bytes, a one-byte value `1`, and a right-handed 3×3 orthonormal f64 basis beginning at offset `+15`. The first, second, and third basis triples are the u-axis, v-axis, and normal. Three f64 values at offsets `+87`, `+95`, and `+103` store the origin in metres. The homogeneous f64 value `1` at offset `+111`, three zero bytes, u32 value `4` at offset `+122`, and `ff ff ff ff` terminate the placement. The source identifier identifies the referenced datum-plane feature; the placement itself supplies the profile frame.

A profile consisting of one full circle also carries a geometric owner signature. Its solved radius equals one radius dimension or half one diameter dimension owned by the corresponding planar sketch feature. When exactly one sketch feature has that radius signature, the signature owns the profile and supersedes interval enclosure. The profile remains interval-bound when the signature has zero or multiple matching sketch features.

Each compact `moDeleteBodyData_c` body-state roster is followed by a `30 80` Boolean field. Value `1` deletes the selected bodies; value `0` retains the selected bodies and deletes the complement.

A sketch marker belongs to the Keywords feature object whose bound feature-name record precedes the marker and whose next bound feature-name record follows it. The little-endian u32 immediately before `ff ff 1f 00 03` is the feature-local object index; `ff ff ff ff` denotes no index. The marker's trailing u32 is its separate local identifier. Object indices and local identifiers are independently scoped to the feature object.

Coordinate-bearing marker codes `0`, `1`, `2`, and `3` identify point, line-or-circle, arc, and constrained-point geometry handles. Relation codes `1..27` identify distance, angle, radius, horizontal, vertical, tangent, parallel, perpendicular, coincident, concentric, symmetric, midpoint, intersection, equal, diameter, offset-edge, fixed, the seven quadrant and cardinal arc-angle relations, horizontal-points, vertical-points, and collinear relations in that order. Codes `4..27` retain relation semantics in both coordinate-bearing and reference-bearing layouts. The marker layout disambiguates the reused codes `1..3`.

Coordinate-bearing geometry handles and no-coordinate relation handles reuse feature-local identifiers. A handle reference with one coordinate-bearing candidate selects that geometry handle. With zero or multiple coordinate-bearing candidates, the identifier resolves only when it has one candidate in the complete feature-local marker set.

A compact legacy non-coordinate kind `0` profile line may carry its endpoint incidence as a counted local-link vector. It has profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and link count u16 `2` at marker +84. Two homogeneous 12-byte selector cells begin at marker +86. Each cell stores the selector u16, an endpoint local ID u16, `ff ff ff ff`, and four zero bytes. The vector terminator is two zero bytes followed by `fe ff ff ff`. The two local IDs resolve against coordinate-bearing geometry markers owned by the same feature object.

A referenced line-or-circle handle with exactly two incident coordinate-bearing point handles carries the line from the first point in marker order to the second. Point incidence may be stored by a link in either direction. Every admissible sketch placement must produce the same two distinct endpoints. When the endpoint pair identifies one existing profile line under every admissible placement, the handle identifies that line; otherwise it carries a construction line and retains its native identity there. Geometry-handle reachability is the undirected transitive closure of dimensional operands and marker links.

A horizontal, vertical, or fixed relation marker constrains its sole resolved reverse-owner curve when every forward entity is either that curve or a relation-owned construction point. Otherwise, a horizontal or vertical relation constrains the single profile entity common to all of its resolved linked loci. When its two linked markers instead identify two distinct profile loci, it aligns those loci along the corresponding sketch coordinate. A fixed relation marker constrains the single profile entity common to all of its resolved linked loci. The relation remains native when none of these arity forms resolves uniquely.

A recognized relation marker whose resolved operands do not satisfy the relation's typed arity and locus-kind invariants remains a native constraint with its ordered local identifiers and resolved native references.

A parallel, perpendicular, tangent, equal, collinear, or concentric relation marker constrains its two distinct reverse-owner curves when every resolved forward entity is one of those curves or a relation-owned construction point. Otherwise it constrains its two distinct linked profile entities when every link identifies exactly one entity. The relation remains native when neither form resolves exactly two entities.

A coincident relation marker constrains its distinct linked profile loci when every link identifies exactly one locus and at least two loci remain after deduplication. The relation remains native when a link identifies zero or multiple loci.

A horizontal-points or vertical-points relation marker aligns its two distinct linked profile loci along the corresponding sketch coordinate when every link identifies exactly one locus. The relation remains native when a link identifies zero or multiple loci or the resolved locus count is not two.

A compact dimensional relation instance contains one or two adjacent scalar records with the same owning sketch, declared relation class, and ordered operand cells. A third scalar starts another instance even when its operands repeat. A circular-dimension instance may instead contain one display-role scalar with one operand followed immediately by two same-name scalars with the same two operands; their first operand equals the display operand and their second operand is the circle center. A scalar separated by any other scalar record starts another instance. An instance has a parameter scalar only when exactly one member has the driving role and has a display scalar only when exactly one member has the display role. An instance without a parameter scalar does not encode a dimensional constraint.

Non-coordinate sketch-marker type codes `1..85` use the constraint taxonomy. Codes `1..27` are distance, angle, radius, horizontal, vertical, tangent, parallel, perpendicular, coincident, concentric, symmetric, midpoint, at-intersection, equal-size, diameter, offset-edge, fixed, arc-angle-90, arc-angle-180, arc-angle-270, arc-cardinal-top, arc-cardinal-bottom, arc-cardinal-left, arc-cardinal-right, horizontal-points, vertical-points, and collinear. Codes `28..47` are coradial, grid-snap, length-snap, angle-snap, use-edge, ellipse-angle-90, ellipse-angle-180, ellipse-angle-270, ellipse-cardinal-top, ellipse-cardinal-bottom, ellipse-cardinal-left, ellipse-cardinal-right, at-pierce, doubled-distance, merge-points, three-point-angle, arc-length, normal, normal-points, and sketch-offset. Codes `48..67` are along-X, along-Y, along-Z, along-X-points, along-Y-points, along-Z-points, parallel-YZ, parallel-ZX, intersection, patterned, iso-by-point, same-isoparametric, fit-spline, equal-curvature, equal-tangent, tangent-face, along-X-3D, along-Y-3D, along-X-points-3D, and along-Y-points-3D. Codes `68..85` are traction, belt-traction, block-fixed-lock, block-normal-lock, block-rotate-lock, fake-slot, fixed-slot, same-slots, linear-pattern-count, circular-pattern-count, radial-offset, planar-offset, aligned-equal-curvature-3D, flange-face-distance, conic-rho, C3-touch, doubled-angle, and same-curve-length. Codes outside this range are native extensions.

A coradial relation has exactly two circular entities. Their solved centers and radii are equal. Full circles and bounded circular arcs participate with the radius and center of their common supporting circle.

A merge-points relation has two or more point loci at one solved sketch position. It has the same neutral coincidence invariant as a coincident-loci relation; its distinct native code is retained as source identity.

Ellipse-angle codes `33`, `34`, and `35` constrain a bounded ellipse's positive parameter sweep to π/2, π, and 3π/2 radians. The sweep is `(end − start) mod 2π`; a nonzero whole-turn difference represents 2π. The relation is invalid on a full ellipse or another geometry family.

A named scalar begins with `04 80 ff fe ff`, followed by a u8 UTF-16 code-unit count, that many UTF-16LE code units, a scalar header, and a finite little-endian f64 value. The primary and legacy layouts use the 22-byte header `00 00 00 00 00 00 00 40 ff ff ff ff 00 00 00 00 ff fe ff 00 00 00`. Scalar trailer offsets are relative to the byte immediately after the f64. Trailer +3 stores the little-endian u32 scalar object identifier. In the primary layout trailer +24 stores `00 00 00 02 00`, trailer +29 stores role `0` for driving or `1` for display, and operand cells begin at trailer +35. In the legacy layout trailer +24 stores `0f 00 00 00 02 00`, trailer +30 stores the same role, and operand cells begin at trailer +36. These operand cells repeat every 12 bytes. Each cell stores its little-endian u16 tag at +0, its u16 marker address at +2, `ff ff ff ff` at +4, and four zero bytes at +8. The compact layout uses the 18-byte header `00 00 00 00 00 00 00 40 ff ff ff ff 00 00 00 00 00 00`; trailer +21 stores `01 00 00 00 02 00`, trailer +27 stores the same role, and two adjacent eight-byte operand cells begin at trailer +35. Each compact cell stores its tag and marker address at +0 and +2 and `ff ff ff ff` at +4. The name length therefore moves the value and every trailer field together.

The instance operand list is the first scalar record's complete ordered operand-cell list. Tags `d6 80` and `e1 80` use a zero-based ordinal within the tag's marker family, ordered by marker byte offset in the owning feature object. In a compact profile containing paired region and chain rosters, `d6 80` instead uses a zero-based ordinal within all coordinate-bearing handles in marker byte order. Circular-dimension tag `fe 83` uses a zero-based curve-handle ordinal. Circular-dimension tags `b6 8a`, `9d 92`, and `69 bd` use a zero-based point or constrained-point ordinal. Tags `7b 83`, `86 83`, `cb 8d`, and `da 8d` first use a feature-local marker identifier qualified by the tag's marker family. When the identifier selects no marker in that family, the same value is a zero-based ordinal within the compatible marker family in byte order. Multiple identifier matches remain unresolved. Tags `7c bc` and `87 bc` use the precedence defined below.

A `d6 80` point ordinal beyond its applicable ordinal sequence addresses the relation handle with that feature-local identifier. The transitive closure of its local links selects one point or constrained-point terminal. If that terminal is already selected by the paired operand, the uniquely remaining compatible point terminal in the feature object is selected. Multiple unclaimed terminals remain unresolved.

When a qualified curve operand identifier selects no coordinate-bearing curve handle, a reference-bearing marker with that identifier selects the unique line, circle, or arc handle among its resolved local links. This indirection precedes compatible-family ordinal fallback. Zero linked curve handles continue to ordinal fallback; multiple linked curve handles remain unresolved.

Operand-cell tags `d6 80`, `cc 80`, `b6 8a`, `cb 8d`, `9d 92`, and `69 bd` address point loci. Tags `7b 83` and `7c bc` address point-qualified geometry handles. Tags `e1 80`, `86 83`, `fe 83`, `da 8d`, and `87 bc` address line-or-circle handles. Tags `cc 80`, `fe 83`, `b6 8a`, `9d 92`, and `69 bd` are used by circular dimensions.

A point operand projects to a typed sketch constraint only when its marker identifies exactly one profile locus. A coordinate shared by multiple profile loci does not select one by ordering. A referenced coordinate-bearing point handle remains a distinct point locus when its coordinate coincides with profile geometry.

When exactly one operand of a horizontal-points or vertical-points relation identifies a profile locus, the other operand identifies the sole distinct locus in the complete owning sketch with the same vertical or horizontal coordinate, respectively. Zero or multiple aligned loci leave the relation unresolved.

A circular dimension whose operand marker does not identify a profile locus selects the unique circle or circular arc in the owning sketch whose solved radius equals the radius parameter or half the diameter parameter. Zero or multiple radius matches leave the relation native.

A unique profile-derived coordinate transform supersedes placement-derived projection. When multiple profile-compatible transforms remain, an axis-aligned placement selects the transforms with its signed axis permutation while retaining the profile-derived translation. When no profile transform exists and the sketch normal, u-axis, and `normal × u` axis are each parallel to distinct world axes, the marker coordinate pair supplies the two ascending world-coordinate components other than the normal axis. The signed axis permutation and sketch origin supply the fallback sketch coordinates. An arbitrarily rotated in-plane frame does not determine marker-coordinate semantics without profile anchors.

A circular dimension with one point, constrained-point, or line-or-circle handle operand, one length parameter with radius or diameter display, and a unique feature-input-to-sketch coordinate transform carries a full circle centered at the transformed handle coordinate. In the two-operand circular form, the second operand supplies the center and the first remains the display handle. Multiple equally scoring transforms are equivalent when they produce the same multiset of centers and radii for every circular dimension owned by the feature; the canonical transform orders axis swap, axis signs, then translation. When no equal circle exists, the circle is added to the sketch without adding it to a selected profile chain. The relation record is the circle's native geometry carrier.

A point or constrained-point marker addressed by a sketch relation and absent from selected-profile geometry carries a construction point when principal-plane placement supplies its sketch coordinate or every admissible signed-axis transform maps it to the same sketch coordinate. The point handle remains a distinct construction locus when its coordinate is occupied by a profile endpoint, curve center, or point.

When a circular-dimension operand with tag `83fe` has no explicit line-or-circle marker, the feature's non-origin coordinate point markers form ordered center/radial-point pairs. The operand index addresses the pair ordinal. The pair is accepted only when its Euclidean radius equals the driving radius or half the driving diameter.

When profile loci do not determine the feature-input coordinate transform, cylindrical surface carriers normal to the sketch plane provide circle-center anchors. The cylinder axis origin projects into sketch coordinates along the sketch `u` axis and `normal × u` axis. A cylinder is compatible with a circular dimension when its radius equals the driving radius or half the driving diameter. A signed-axis transform qualifies only when it maps every dimensioned center to a distinct compatible projected cylinder center. Multiple qualifying transforms are equivalent only when they produce the same complete multiset of centers and radii.

A non-coordinate marker with type code `12` is a midpoint relation. It has exactly two linked markers: one point or constrained-point marker and one line, circle, or arc marker. Link order is not significant. Each linked marker must identify exactly one profile locus; the point locus is constrained to the midpoint of the entity owning the other locus.

Non-coordinate marker codes `18`, `19`, and `20` constrain one resolved circular-arc entity to positive angles π/2, π, and 3π/2 radians respectively. The relation remains native unless all linked loci identify the same single profile entity.

A feature-input class declaration is `ff ff 01 00`, a little-endian u16 byte length, and an ASCII class name. When the following record begins at declaration offset `+ 6 + length`, that record is an instance of the declared class. A feature-name record begins with the lane's u16 name-class token, `ff fe ff`, a u8 UTF-16 code-unit count, and the UTF-16LE name. The token has its high bit set and is established by the lane's first name record: the first class declaration directly followed by the token and the `ff fe ff` prefix. Every feature-name record in the lane repeats the same token. The little-endian u32 at eight bytes after the name is the feature object ID. It equals the corresponding Keywords feature `id` and binds the records independently of the display name.

A repeated class instance stores a little-endian u16 class token immediately before its feature-name marker. The token is scoped to the `ResolvedFeatures` lane. Repeated instances with the same token have the same declared class.

A sketch-input entity starts with the prefix `ff ff 1f 00 03`, `ff ff 07 00 01`, or `ff ff 1f 00 01`. Each prefix is followed by eight `ff` bytes and the little-endian f32 `-1.0`; each stores the kind at marker +17, the geometry/display role at marker +23, and the state value at marker +48. A model-space point with geometry role `05 00 01 00` and profile role u16 `1` stores marker `0e 00` followed by three finite little-endian f64 xyz coordinates in metres. The `ff ff 1f 00 03` kind-`0` layout places the marker at +56 and coordinates at +58; its kind-`1` layout places them at +64 and +66. The `ff ff 07 00 01` kind-`0` layout places them at either +56 and +58 or +64 and +66. An object-indexed `ff ff 1f 00 01` kind-`0` point places them at +64 and +66. The `ff ff 1f 00 01` kind-`1` layout with geometry role `04 00 02 00` also places them at +64 and +66. In either `... 00 01` geometry-locus layout, `1e 00` is at marker +56 and the two finite little-endian f64 coordinate fields are at marker +58 and +66, in metres. In the `ff ff 1f 00 01` indexed-curve layout, a non-coordinate profile curve has role u16 `1` at marker +27, stores zero-based feature-local marker-roster indices as little-endian u16 values at marker +56 and +58, and stores little-endian u32 `1` at marker +60. A complete indexed-curve body remains non-coordinate when its first u16 endpoint index is `30` and therefore encodes `1e 00` at marker +56. The 84-byte object-indexed profile-line layout instead has kind `0`, role bytes `04 00 02 00`, profile role u16 `1`, selector bytes `00 00 80 bf 00 00 05 00` at marker +31, little-endian f64 `1.0` at marker +48, two nonzero distinct little-endian u16 feature-local point object IDs at marker +56 and +58, four zero bytes at marker +60, and little-endian f64 `-1.0` at marker +64. The next sketch-input marker starts at marker +84. In a sequence of `ff ff 1f 00 01` kind-`0` indexed profile lines with the same body, the final 84-byte record remains a line when no subsequent sketch-input marker follows it. A 92-byte `ff ff 1f 00 03` kind-`2` indexed arc with a coordinate-bearing kind-`2` center carrier stores two nonzero distinct direct feature-local point object IDs at marker +64 and +66. The unique coordinate-bearing kind-`2` marker equidistant from both points supplies the arc center. Kind `2` without that compact profile selector or a 92-byte indexed-curve trailer is the selected construction line. A connected run of at least two indexed arcs carries circular arcs when it addresses at least three distinct roster points, exactly one circle contains every addressed point, and every ordered endpoint pair has a positive counterclockwise sweep no greater than π. The shared circle supplies each arc center and radius. In the `ff ff 07 00 01` indexed-line layout, coordinate-bearing records are the same one-based point roster. A non-coordinate record with profile-curve role `1` or `2` at marker +27 stores its zero-based point-roster indices as little-endian u16 values at marker +64 and +66. Role `1` is a profile line and role `2` is the selected construction line. Marker-backed curve components form neutral profiles only when every endpoint has degree two and the component is one closed cycle; traversal direction records whether each stored endpoint pair is reversed. Role `2` curves are construction geometry and do not participate in profiles. Other marker entities are not construction geometry. A planar profile object may carry one complete reference-plane frame using the same matrix, fixed, angular, minimal, or compact frame encoding as a constructed reference plane. That frame places every coordinate-bearing entity owned by the profile object. The sole selected construction line in a revolution profile is its placed axis.

An `moCosmeticThread_c` or `moDerivedCosmeticThread_c` feature is a non-geometric thread annotation. Its diameter-displayed `D2` length scalar is the nominal thread diameter. A `D1` length scalar selects blind termination and stores the axial blind length; omission of `D1` selects through termination. Both scalar payloads use native meters. The `D2` scalar object identifier is one less than the thread feature object identifier. Its following child tail begins after the scalar value and ends before the next scalar value or feature-name record. An `moCylinderRef_w` child in that tail carries the attached cylindrical face, whose radius is half `D2`. Its body starts with its class token, a nested class token, `02 00 00 00`, a selector byte `00` or `40`, and two zero bytes. The component-path marker starts 66, 70, 94, or 106 bytes after the body. A direct class declaration ends 21 bytes before the body, placing the marker 87, 91, 115, or 127 bytes after the declaration. The marker has a positive little-endian u32 slot count at −12, selector `00 02 00 00` or `00 03 00 00` at −8, and two zero bytes after its duplicated 16-byte identity. The first ordered typed component is the attached face; subsequent typed components retain its owning path. A marker whose slots use persistent object tokens instead of typed component entries still identifies the cylinder-reference operand.

When `moCylinderRef_w` directly wraps an `moCompFace_c` child, the component-face body starts with its class token, `02 00 00 00`, and two zero bytes. Its component-path marker starts 92 bytes after the component-face body.

The sketch-surface component-path variant stores u32 count `5`, kind `00 03 00 00`, four opaque bytes, the component-path marker, and two zero bytes. The count includes two implicit root slots; three 20-byte heterogeneous component entries follow. A two-byte zero alignment word may precede the third entry.

A compact `moDeleteBody_c` object ends with a little-endian u32 schema word `11000`, two zero u32 words, a u32 selection count, that many ordered u32 regeneration-input-local body identifiers, the sentinel `ff ff ff ff`, and three zero u32 words. The object ends after those zero words or one additional zero u32 word. The ordered identifiers are the persistent body selection consumed by the delete/keep operation. When another instance of the same declared class follows, its lane-scoped u16 repeated-class token lies between the selection terminator and the next feature-name marker.

The lane-scoped u16 token immediately following the sole `moDeleteBodyData_c` class declaration opens each body-state record owned by a compact delete/keep object. The token is followed by `2b 80 02 00 00 00 00 00 00`, the feature-local u32 body identifier twice, 28 zero bytes, 16 `ff` bytes, and 20 zero bytes. Body-state records precede the selection vector and retain their ordered local identifiers independently of the selected identifier list. The retained roster equals the ordered records between the owning feature-name record and its selection vector. The state roster is not a retention-mode discriminator.

A `moCompEdge_c` child carries an ordered compact edge-selection vector. The vector marker is `7d c3 94 25 ad 49 b2 54 7d c3 94 25 ad 49 b2 54`. A little-endian u32 count occurs at marker −12, marker −8 begins `00 02 00 00`, and two zero bytes follow the marker. An entry-form vector contains count entries, each with a four-byte instance cell, a 12-byte type signature, and a little-endian u32 feature-local identifier. Type signatures may be uniform or heterogeneous. Consecutive entries are adjacent or separated by four zero bytes, four `ff` bytes, eight zero bytes, `ff ff ff ff 00 00 00 00`, or `a0 86 01 00 00 00 00 00`. The count may instead cover one terminal feature-reference cell following `count − 1` entries. That 36-byte cell begins `01 00 00 00 00 00 00 00 4a 80 00 00`, carries a nonzero class token, `37 00`, a nonzero little-endian u32 feature source identifier, a nonzero four-byte type identity, and 12 zero bytes. A compact-ID vector instead contains count little-endian u16 edge identifiers, 16 zero bytes, and `ff fe ff`.

Every structurally valid edge-selection vector in a fillet or chamfer feature-object interval belongs to that feature. Multiple vectors retain stream order as one ordered native edge selection. The first vector following a child declaration and repeated children whose body begins `2d 80 02` use the same vector grammar.

In an entry-form edge-selection vector, type-signature bytes 4 through 7 contain the native feature object ID traversed by that path entry. The ordered component identifiers form the persistent edge identity. The terminal feature-reference cell identifies the feature result owning that identity when present. Otherwise the last entry whose native object ID uniquely names a history feature identifies the owner; trailing entries may name non-feature result components. An object ID shared by multiple history features leaves the owner unresolved. The consuming fillet or chamfer depends on the ordered unique features traversed by all entries and terminal feature-reference cells.

A `moCompSurfaceBody_c` child of `moThicken_c` carries the selected surface components. Its lane-scoped class token occurs 103 bytes before the duplicated vector marker. Marker −12 is the little-endian schema word `6`; marker −8 begins with `04 02 00 00`; two zero bytes follow the marker. Entries contain a four-byte instance cell, one 12-byte type signature shared by the vector, and one little-endian u32 feature-local component identifier. Entries are adjacent or separated by one four-byte instance ordinal. The vector ends when the shared entry signature ends.

`moExtrusion_c` and `moICE_c` are extrusion feature classes. `moProfileFeature_c` and `mo3DProfileFeature_c` are planar and spatial sketch feature classes. `moOriginProfileFeature_c` is the built-in model-origin tree node and carries no sketch geometry. `moCombineBodies_c` is the body-Boolean feature class. `moConstSurfRef_w`, `moEndPointRef_w`, `moGeneralCurveRef_w`, `moLineRef_w`, `moSingleFaceRef_w`, `moSolidRef_w`, `moCompReferenceCurve_c`, and `moCompSurfaceBody_c` identify reference objects rather than feature operations.

A spatial-sketch vertex record begins with `ff fe ff 06` followed by the UTF-16LE string `Vertex`. At record offset `+43`, `0e 00` identifies three little-endian f64 model-space coordinates at offsets `+45`, `+53`, and `+61`. A vertex-record run contains a positive even number of records. Each consecutive pair owns one bounded line in stored vertex order. An object-indexed marker-backed spatial point begins with `ff ff 07 00 01` or `ff ff 1f 00 03`, eight `ff` bytes, little-endian f32 `-1.0`, role bytes `04 00 02 00`, and profile role u16 `1`. Its non-sentinel little-endian u32 feature-local object ID immediately precedes the marker. In the first form, marker +56 contains `0e 00` and the three little-endian f64 model-space coordinates begin at marker +58. In the second form, marker +64 contains `0e 00` and the coordinates begin at marker +66. Each coordinate is zero or a normal finite f64. Point and line records retain their byte order within one spatial-sketch feature object. These spatial point records identify a spatial profile independently of its feature-class declaration. Spatial geometry is not projected into a planar sketch coordinate system.

A compact `moCombineBodies_c` object carries its target and tool as the first and second type-3 component-path vectors in its feature-object interval. A type-3 vector uses the duplicated component marker, a positive count at marker −12, `00 03 00 00` at marker −8, two zero bytes after the marker, and heterogeneous 20-byte typed path entries with the same separator grammar as edge component paths. The count either equals the entry count or includes one terminal null slot encoded as `ff ff ff ff 00 00 00 00`. The two paths retain their ordered native identities independently of the Boolean operation.

The compact Combine operation is a little-endian u32 at feature-name marker offset `+ 117 + 2 × name-code-unit-count`. Twelve zero bytes precede it; six zero bytes and `ff ff ff ff` follow it. Values `0`, `1`, and `2` mean join, subtract, and intersect respectively.

An extrusion object immediately following a `moProfileFeature_c` object consumes that profile feature. A profile feature may instead be followed by a sequence containing only sketch-block definition and sketch-block instance objects and then the extrusion. In that form, the profile feature's `DissectableChildren` identifiers are exactly the distinct object identifiers of the intervening sketch-block definitions; the extrusion consumes the profile feature containing those blocks. A compact extrusion without `DissectableChildren` also consumes a `moProfileFeature_c` object immediately following it. The profile feature is an ordered dependency of the extrusion. These adjacency forms are independent of the `DissectableChildren` property used by explicitly linked extrusion objects.

The inline extrusion operation trailer establishes the extrusion object family independently of its class token. This applies when a repeated token is shared by more than one declared extrusion class.

An integer or Boolean Keywords dimension without dimensional-relation ownership is discrete. A same-named native f64 scalar binds to that dimension only when it exactly represents the existing integer or Boolean value. Other same-named native scalars in the feature-object interval belong to a different semantic field.

`moSweep_c` produces a solid sweep. Compact operation code `15` joins the swept result to the existing body. Its Boolean operation remains independently unresolved when no recognized operation carrier is present. `moSweepRefSurface_c` produces a surface sweep.

A solid or surface sweep's `moGeneralCurveRef_w` child identifies its path. A declared component-profile form contains an `moCompProfile_c` child followed immediately by `2b 80 02 00 00 00 00 00 00 00`. A compact component-profile form prefixes the same bytes with `01 00 dd 94 df 94`. In both forms, prefix +45 through +60 is `ff`. The older record stores the referenced feature's nonzero u32 source ID at prefix +69; source +12 through +15 and +20 through +31 are zero, source +32 and +36 are `c7 cf ff ff`, source +40 is zero, and source +44 is `f8 2a 00 00`. The newer record stores the source ID at prefix +81; source +8 through +15 are zero, source +16 is u32 `0x65`, source +20 is zero, source +24 is `ff ff ff ff`, source +28 is zero, source +32, +36, and +40 are `c7 cf ff ff`, source +44 is zero, and source +48 is `f8 2a 00 00`. A component-profile source ID naming a planar sketch feature makes that sketch the sweep path.

A solid `moSweep_c` object without an enclosed profile stream can be preceded and followed immediately by `moProfileFeature_c` objects. When the preceding object owns one resolved neutral sketch, that sketch is the sweep path and supersedes an opaque general-curve reference. When the following object owns one resolved neutral sketch, that sketch is the sweep cross-section profile. Each resolved sketch feature is a regeneration dependency. A missing, multiply addressed, non-profile, or unresolved adjacent object leaves the corresponding sweep operand unresolved.

A repeated general-curve form with a two-byte wrapper token, two zero bytes, a two-byte child token, and the compact child prefix `2b 80 02 00 00 00 00 00 00 00` retains the wrapper offset as its stable native path identity when it carries no component-profile source record.

An `moCompReferenceCurve_c` child identifies the sweep cross-section independently of the `moGeneralCurveRef_w` path. In the declared direct form, the class name is followed by `2b 80 02 00 00 00 00 00 00 00`; the referenced curve feature's nonzero source ID uses the same older and newer source-record layouts as a component-profile source. In the generated-component form, the lane-scoped wrapper token is followed by a child token and `2b 80 02 00 00 04 00 00`. Its duplicated component marker carries a positive count at marker −12 and `04 02 00 00` at marker −8. The homogeneous 20-byte typed entries use a six-byte first-entry separator consisting of a nonzero u16 and four zero bytes when present. The count includes one terminal slot after the entries; that slot is eight zero bytes followed by `f8 2a 00 00`. Type-signature bytes 4 through 7 identify the feature result owning the persistent curve components, and the ordered entry identifiers form the feature-local cross-section identity.

A `moCombineBodies_c` object is a body-Boolean feature independently of whether its Keywords element carries `Operation`, `Target`, or `Tools` attributes. Compact operation and component-path carriers supply absent attributes independently.

A planar sketch history name ending in `<N>`, where `N` is one or more decimal digits, aliases the uniquely named unsuffixed sketch when both records have the same XML element tag, resolved feature-input class, ordered content, and complete parameter map. The unsuffixed history feature remains the sole owner of the solved sketch geometry, and the geometry-less alias feature depends on that owner. Feature operands naming the alias bind to the owner's sketch and depend on the unsuffixed owner. A missing base, multiple matching bases, or any record-content difference leaves the alias operand native.

Keywords `Configuration` elements carry a non-empty, document-unique `Name`; `Material` carries the configuration material override and the remaining attributes are configuration-local named values. A configuration whose name equals `swModel/@swConfigurationName` is active. A missing or unmatched active name leaves every configuration inactive.

The `swConfigurationList/swConfiguration` element identifies the same configuration by decimal `swID`. `swConfigurationNeedsUpdate="YES"` states that the document carries no current evaluated snapshot for that configuration; its feature and parameter state requires regeneration. `swConfigurationNeedsUpdate="NO"` states that its cached state is current. `swMostRecentConfiguration`, `swConfigurationFlags`, and `swConfigurationAlternateName` are configuration-manifest metadata.

Keywords `Feature` elements use the `Type` attribute as their operation-family token. All feature instances with the same exact `Type` token use the same feature-input class. A directly object-ID-bound class instance therefore supplies the class of the other instances carrying that token. `Helix/Spiral`, `Surface-Sweep`, and `Thicken` denote helix, surface-sweep, and face-thickening operations independently of the localized display name in `Name`.

Sketch relations use named scalar records with reference cells at fixed scalar-record slots. Point references use `d6 80`, `cc 80`, `52 81`, `7b 83`, or `7c bc`; line references use `e1 80`, `86 83`, or `87 bc`. Point-point, line-line, and point-line distance relations follow from the operand pair. Two `52 81` or two `cb 8d` cells carry horizontal or vertical point-point distance according to the relation declaration. Two `da 8d` cells carry an angular relation. An `sgCircleDim` declaration followed by one `cc 80`, `fe 83`, `b6 8a`, `9d 92`, or `69 bd` cell carries a diameter dimension. A display-role scalar names an existing dimension owned by the same sketch when no driving relation or earlier display-only relation claims that dimension. The relation family supplies the dimension unit independently of the display scalar value. Scalar records with the same owning sketch, relation family, and ordered operand sequence belong to one relation instance. Display-role and driving-role scalars are distinct. A unique driving scalar stores the target parameter. An operandless driving scalar separated from its display records by another complete relation belongs to the unique unresolved relation with the same owning sketch and dimension name.

Two `0f 82` reference cells in a driving scalar address solver-point indices through their u16 entity-index fields. The scalar value is the Euclidean distance in metres between those points. Solver points occupy the finite coordinate positions of coordinate-bearing markers in the owning feature object. A solver-point index has a resolved position when the complete connected point-distance graph has at least one satisfying assignment and arc consistency leaves one position for that index. A coordinate-roster ordinal beyond the explicit marker roster uses the resolved solver-point position with the same index.

In a Hole Wizard position sketch, `52 81` marker addresses are opaque point-node identities shared across its dimensional relations. Matching bore-cylinder axes and the sketch origin are candidate node loci. A placement set is defined when exactly one set of loci admits an injective node assignment satisfying every driving point distance, horizontal distance, and vertical distance. Horizontal and vertical relation axes may be exchanged as a pair to account for the native-to-sketch coordinate ordering. The bore origins are projected onto the sketch plane and the placement direction is the sketch normal. Multiple satisfying bore sets leave the placements unresolved.

A Hole Wizard position sketch may instead store each authored position as the coordinate pair of an object-indexed line-or-circle marker. The complete marker-position set selects bore axes of the hole radius when exactly one set of consistently directed parallel axes is congruent to it: a bijection must preserve every pairwise distance. Coaxial cylindrical surfaces form one candidate axis. A Hole Wizard temporary-axis record selects the surface direction on each carrier. Without that record, oppositely directed surfaces on the same axis leave the carrier unresolved. Multiple congruent axis sets or an incomplete marker set leave the placements unresolved.

A spatial Hole Wizard position sketch may contain construction points and paired points on bore axes. An object-indexed point selects a placement only when its model-space position lies on exactly one cylinder carrier of the hole radius. The cylinder origin and axis define the placement. Multiple points selecting the same carrier collapse to one placement. Points selecting no carrier are construction geometry. A point selecting multiple distinct carriers leaves the placements unresolved.

A `7b 83` point reference is qualified by its local identifier. The identifier can select a point, constrained-point, line-or-circle, or arc marker.

A `7c bc` point reference first selects the sole coordinate-bearing point, constrained-point, line-or-circle, or arc marker whose object index equals the address. Relation markers sharing that object index do not participate. Without an indexed coordinate marker, the reference selects a point or constrained-point marker with the addressed local identifier, then the in-range zero-based point-family marker ordinal in byte order. When the ordinal is out of range, one line-or-circle or arc marker with the addressed local identifier supplies the qualified point. For a curve marker selected by either tag, the marker's stored coordinate is the point locus; the curve marker retains its curve identity independently of that qualified locus.

In the compact legacy prefix, a coordinate-bearing kind `1` marker with `04 00 02 00` at marker +23 and profile role u16 `1` at marker +27 is a line whose start is its own coordinate and whose end is the coordinate of the immediately following sketch marker in the same feature object. The following marker retains its independent geometry identity; its coordinate also serves as the line endpoint.

An `87 bc` curve reference first selects the sole coordinate-bearing line-or-circle or arc marker whose object index equals the address. Without an indexed coordinate marker, the reference selects a curve marker with the addressed local identifier, follows one reference-bearing marker with that identifier to its unique curve target, then uses the in-range zero-based curve-family marker ordinal in byte order.

A coordinate-less point or constrained-point handle used by a dimensional relation can be relation-qualified. When exactly one point-point, horizontal-distance, or vertical-distance operand has a profile locus and the stored distance selects one physical coordinate in the complete owning sketch under the relation's Euclidean, horizontal, or vertical metric, that coordinate is the other operand's construction-point locus for that relation. Coincident profile loci at the selected coordinate are one physical-coordinate match. The relation-qualified locus does not assign a global position to the native handle and is distinct for each relation and operand position.

Distinct operand addresses in one binary relation select distinct markers. When address resolution initially converges on one marker, resolution of either operand excludes that marker from the other operand's exact local-identifier and reference-link candidates.

A point-point dimension projects to neutral form only when its operands resolve to distinct profile loci. A line-line distance or angular dimension projects only when its operands resolve to distinct profile entities.

When exactly one point-distance operand identifies a locus, the other operand identifies the sole distinct point locus in the complete owning sketch at the stored distance. Zero or multiple distance-compatible loci leave the operand unresolved.

When both point-distance references identify loci whose separation differs from the stored distance, each referenced locus independently selects its sole distinct locus at the stored distance. The dimension uses the resulting pair only when these searches produce one unique unordered pair. The repaired pair therefore retains a referenced locus; two different pairs, no pair, or a scalar-compatible pair unrelated to both references leaves the relation native.

When neither point-distance operand identifies a locus, the operands identify the sole unordered pair of profile loci in the complete owning sketch separated by the stored distance. Zero or multiple distance-compatible pairs leave both operands unresolved.

When exactly one line-distance operand identifies a profile line, the other operand identifies the sole distinct parallel profile line in the complete owning sketch at the stored perpendicular distance. When neither operand identifies a line, the operands identify the sole unordered parallel line pair at that distance. Zero or multiple compatible lines or pairs leave the missing operands unresolved.

When both line-distance references identify lines whose perpendicular separation differs from the stored distance, each referenced line independently selects its sole distinct parallel line at that distance. The dimension uses the resulting pair only when these searches produce one unique unordered pair. The repaired pair retains a referenced line; ambiguity and unrelated scalar-compatible pairs leave the relation native.

When exactly one angular-dimension operand identifies a profile line, the other operand identifies the sole distinct profile line in the complete owning sketch whose unsigned direction angle equals the stored angle. When neither operand identifies a line, the operands identify the sole unordered profile-line pair at that angle. The unsigned direction angle is the arccosine of the normalized direction dot product and lies in `[0, pi]`. Zero or multiple compatible lines or pairs leave the missing operands unresolved.

When both angular-dimension references identify lines whose unsigned direction angle differs from the stored angle, each referenced line independently selects its sole distinct line at that angle. The dimension uses the resulting pair only when these searches produce one unique unordered pair. The repaired pair retains a referenced line; ambiguity and unrelated angle-compatible pairs leave the relation native.

For a point-line dimension, a resolved point operand identifies the sole profile line in the complete owning sketch at the stored perpendicular distance, and a resolved line operand identifies the sole profile locus at that distance. When neither operand resolves directly, the operands identify the sole ordered profile-locus and profile-line pair at that distance. Zero or multiple compatible candidates leave the missing operands unresolved.

When both point-line references identify operands whose perpendicular separation differs from the stored distance, the referenced point independently selects its sole line at that distance and the referenced line independently selects its sole point locus at that distance. The dimension uses the resulting ordered pair only when these searches produce one unique pair. The repaired pair retains a referenced operand; ambiguity and unrelated scalar-compatible pairs leave the relation native.

For horizontal and vertical point-point dimensions, a resolved operand identifies the sole distinct profile locus in the complete owning sketch whose absolute horizontal or vertical displacement equals the stored distance. When neither operand resolves directly, the operands identify the sole unordered profile-locus pair with that axis displacement. Zero or multiple compatible loci or pairs leave the missing operands unresolved.

When both horizontal- or vertical-distance references identify loci whose axis displacement differs from the stored distance, each referenced locus independently selects its sole distinct locus at that axis displacement. The dimension uses the resulting pair only when these searches produce one unique unordered pair. The repaired pair retains a referenced locus; ambiguity and unrelated scalar-compatible pairs leave the relation native.

A relation instance without a driving scalar uses its display scalar's attached name record to identify an existing same-named parameter owned by the same sketch feature. The binding requires one parameter and applies only when no driving relation or earlier display-only relation has claimed that parameter.

Distance, horizontal-distance, vertical-distance, and circular-dimension driving scalars store metres. Angular driving scalars store radians. These relation-family units apply independently of the owning Keywords dimension's expression spelling.

A bare integer Keywords dimension bound to a unique driving distance or circular-dimension scalar denotes millimetres rather than a discrete count. The scalar supplies its evaluated length and native identity while the original expression remains unchanged.

A bare `0` or `1` Keywords dimension bound to a unique driving distance or circular-dimension scalar denotes millimetres rather than a Boolean. The scalar supplies its evaluated length and native identity while the original expression remains unchanged.

A bare integer Keywords dimension bound to a unique driving angular scalar denotes milliradians rather than a discrete count. The scalar supplies its evaluated angle in radians and native identity while the original expression remains unchanged.

A bare `0` or `1` Keywords dimension bound to a unique driving angular scalar denotes milliradians rather than a Boolean. The scalar supplies its evaluated angle in radians and native identity while the original expression remains unchanged.

An otherwise untyped bare Keywords integer, including `0` and `1`, is a dimensionless integer. The case-insensitive literals `true` and `false` are Boolean values.

A uniquely owned feature-input scalar is the evaluated value of the same-named Keywords dimension. Length-valued feature scalars store metres and angular feature scalars store radians. Keywords dimension text remains the parameter expression; its unitless numeric spelling does not replace the evaluated scalar. Feature operation semantics use the evaluated scalar converted to millimetres or radians.

A nonempty Keywords parameter value with no scalar literal, operator, grouping delimiter, function delimiter, or definite parameter-reference spelling is literal text. Leading and trailing whitespace are not part of the text value. A double-quoted identifier, an identifier containing `@`, and `D` followed only by decimal digits remain parameter-reference spellings.

A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot `N`. Scalars from configuration-scoped lanes do not replace the document-level parameter value or its native identity. Every evaluable document expression and every scalar resolved in the scoped lane contributes its typed value to that configuration's parameter state.

The same lane supplies configuration-local feature operation state. Feature classes, operation discriminators, compact termination records, profile adjacency, path references, and selection records are evaluated within their `Config-N-ResolvedFeatures` lane. They do not define document-global feature semantics unless every applicable lane yields the same state. A resolved configuration carries one evaluated feature state for every document feature.

A Keywords configuration's decimal `id` attribute is the slot identity for `Config-N-ResolvedFeatures`. It is independent of `Config-N-Partition`. The stored configuration id selects the same-numbered resolved-features lane before regeneration ordinal fallback. A configuration with an explicit stored id does not participate in ordinal fallback.

When exactly one configuration is active and no configuration-scoped lane supplies its state, the document-level feature and parameter projections are its evaluated state. Its snapshot contains the final document-level state of every feature and every parameter with an evaluated value. Configuration-scoped lanes take precedence and supply independently evaluated snapshots.

An unquoted Keywords expression identifier does not begin with an ASCII digit or `.`. A numeric-leading token is a numeric literal, including when a parameter in the same owner has that token as its name. A numeric parameter name is referenced with double quotes.

Keywords length literals use the suffixes `uin`, `mil`, `mm`, `cm`, `in`, `ft`, `nm`, `um`, `µm`, `μm`, `Å`, `A`, and `m`. Their millimetre scale factors are respectively `0.0000254`, `0.0254`, `1`, `10`, `25.4`, `304.8`, `0.000001`, `0.001`, `0.001`, `0.001`, `0.0000001`, `0.0000001`, and `1000`. A unit suffix is part of the numeric literal and determines its length dimension before expression evaluation.

Point-reference object indices address sketch-marker local identifiers within the owning feature object. A reference resolves when that local identifier is unique in the feature object.

Operand tags `80d6`, `80cc`, `837b`, `8ab6`, `8dcb`, `929d`, `bc7c`, and `bd69` select point loci, including the point-qualified curve forms defined above. Tags `80e1`, `8386`, `83fe`, `8dda`, and `bc87` select line, circle, or arc markers.

A uniquely linked reference-handle chain resolves to its terminal profile locus. Linked loci intersect a non-unique coordinate-derived locus or entity set for the same handle; a single remaining locus or entity resolves the handle. Cyclic chains and chains with multiple terminal loci remain unresolved. A relation handle whose linked chains identify one common profile entity identifies that entity.

Constraint incidence is bidirectional. A coordinate-bearing geometry marker owns a relation marker when one of its resolved local-link cells targets that relation marker. Native constraints retain these reverse owners in marker order separately from the relation marker's forward reference-handle operands.

A geometry marker's link to a relation marker records constraint incidence, not locus equivalence. Locus resolution does not traverse from a geometry marker through the relation marker to another operand of that relation.

Feature-input geometry-handle coordinates and the nested Parasolid profile differ by a signed axis permutation and constant translation per sketch feature. A unique transform mapping at least two distinct geometry-handle coordinates onto compatible profile anchors binds every matching geometry or relation marker coordinate to those loci. Profile loci are the primary anchors. When they do not determine a frame, point handles admit entity endpoints and centers, line-or-circle handles admit line endpoints, midpoints, and circular centers, and arc handles admit arc centers. Relation-marker coordinates do not participate in selecting the frame. The identity axis permutation has precedence when it has a unique translation. When equally scoring signed axis permutations include zero-translation transforms, translated transforms are excluded. A reference marker whose linked endpoint markers share one profile entity identifies that entity.

When profile anchors do not determine the transform, planar sketch placement supplies it. The two feature-input coordinate fields are the model-coordinate components whose axes are not the dominant component of the sketch normal. The omitted model-coordinate component is the unique value on the sketch plane. Subtracting the sketch origin and projecting the resulting model-space vector onto the sketch `u` axis and `normal × u` axis yields the local sketch coordinate. Axis-aligned placements reduce to a signed axis permutation and translation.

Multiple valid signed-axis transforms bind a marker when every transform maps that marker to the same normalized profile locus set. A transform-dependent marker remains unresolved.

A primary line-or-circle geometry handle on a transformed line segment identifies that line entity. Line-interior entity anchors are scoped to primary line-or-circle geometry records and cannot satisfy point operands or display handles. A curve handle matching an endpoint or center locus identifies the owning curve entity rather than an endpoint locus.

When a point or constrained-point marker maps to a shared profile coordinate, its incident start and end loci are geometrically equivalent. The lexicographically first locus is the canonical operand. Line-or-circle and arc markers retain every compatible entity at a shared coordinate.

A coordinate-less point or constrained-point marker linked to two or more resolved profile entities identifies their endpoint only when every linked entity has exactly one common stored endpoint coordinate. The lexicographically first locus at that coordinate is canonical. Center loci, analytic intersections away from stored endpoints, multiple shared endpoints, unresolved links, and links spanning sketches leave the point marker unresolved.

Point-distance operands select explicit profile loci. Line-distance and angular operands select the profile entity shared by their linked endpoint markers. A non-dimensional relation's point-locus roster is the unique loci selected by its forward local links plus coordinate-bearing markers whose local link targets the relation marker. Three-operand symmetric and at-intersection relations therefore combine the reference marker's two forward links with one reverse-owned locus. A relation with resolved operands and one driving scalar maps to the corresponding neutral distance, horizontal-distance, vertical-distance, angle, radius, or diameter constraint. Relation-marker coordinates do not identify constraint operands. A reference identifier equal to the relation marker's local identity or object index identifies the relation marker itself, even when the generic identifier resolver selects another marker with the same value; it is not constraint incidence. A relation marker without another linked marker or a reverse geometry owner does not produce a sketch constraint. A known relation whose completely resolved neutral entities cannot belong to the relation's required operand family is inactive. Its native definition and operands remain retained.

A dimensional relation maps to neutral form only when its evaluated operand geometry measures the driving scalar. Point-point distance uses Euclidean locus distance; horizontal and vertical distance use absolute displacement on the corresponding sketch axis; point-line and line-line distance use perpendicular distance; angle uses the unsigned line direction angle; radius and diameter use the circular entity radius. A resolved identity with a different evaluated measurement retains native relation semantics.

Horizontal and vertical relations require their evaluated line or point-locus operands to be aligned on the corresponding sketch axis. A resolved identity whose evaluated geometry is not aligned retains native relation semantics.

Parallel, perpendicular, collinear, tangent, equal-size, and concentric relations require the resolved entities to satisfy the corresponding evaluated geometric invariant. Parallel and perpendicular compare line directions; collinearity additionally requires zero line separation; tangency compares line-to-circle or circle-to-circle contact; equal-size compares line length, circular radius, or both ellipse radii; concentricity compares centered-entity centers. Unsupported entity-family combinations and geometrically inconsistent operands retain native relation semantics.

Coincident relations require every resolved locus to evaluate to one sketch coordinate. A midpoint relation requires its point locus to evaluate to the midpoint of the resolved bounded line or circular arc. A symmetric relation with two point loci and one line-entity locus uses the line as its axis; the point projections onto the axis are equal and their signed perpendicular distances are opposites. An at-intersection relation has one point locus and two distinct entity loci. The point must lie within both bounded line, circle, circular-arc, or ellipse domains. Fixed arc-angle codes `18`, `19`, and `20` require the resolved arc sweep to equal π/2, π, and 3π/2 radians respectively. Identity-resolved operands whose evaluated geometry does not satisfy the relation retain native relation semantics.

`Helix/Spiral` history records use positional dimensions when explicit axis placement is absent: `D3` is the signed total axial rise, `D4` is the signed axial rise per revolution, `D5` is the positive revolution count, and `D7` is the start angle. The history record owns the unresolved construction axis and radius.

The corresponding feature-input object contains one nested schema-format `13006` Parasolid mesh stream. Its polyline coordinate array is a big-endian u32 scalar count, the `00 22` array tag, and `count / 3` consecutive big-endian f64 xyz triples. The ordered points sample the helix from start to end. Their circular projection determines the axis placement and radius; their signed displacement along that axis determines total rise and pitch.

An `moCurvePattern_c` feature-input object is immediately preceded by its seed feature object and followed by its path feature object. The preceding object identifies the repeated neutral feature. When the following object is an `moProfileFeature_c` sketch with one resolved neutral sketch, that sketch is the curve-driven pattern path. Seed and path resolve independently. A missing or multiply addressed adjacent object leaves its operand unresolved; a following object that is not a resolved sketch leaves the path unresolved.

An `moLineRef_w` declaration has two direction layouts. When two consecutive `c7 cf ff ff` words occur at declaration offsets `+136` and `+140` and `f8 2a 00 00` occurs at `+148`, three little-endian f64 values at `+200`, `+208`, and `+216` store its unit xyz direction. When three consecutive `c7 cf ff ff` words occur at `+144`, `+148`, and `+152` and `f8 2a 00 00` occurs at `+160`, the unit xyz direction is at `+220`, `+228`, and `+236`.

A compact line reference starts with two consecutive `c7 cf ff ff` words, one zero u32, and a nonzero u32 stream address. One layout continues with 16 zero bytes and nine finite little-endian f64 values, followed by u32 values `1` and `1`. Its trailer is either 16 zero bytes or 12 zero bytes followed by a non-sentinel high-bit u16 instance token. A six-scalar layout continues with 24 zero bytes, six finite little-endian f64 values at offsets +40 through +87, 16 zero bytes, u32 values `1` and `1`, and 24 zero bytes. The other layouts continue with eight zero bytes and seven, eight, or nine finite little-endian f64 values. A nonzero eight-byte cell after the seventh scalar selects the seven-scalar form. A zero cell is shared by the eight- and nine-scalar forms. In every layout, the final three scalar values are the unit xyz direction. Handle words belonging to a declared `moLineRef_w` layout are not a second compact reference.

An `moLPattern_c` feature-input object is immediately preceded by its seed feature object. That preceding object identifies the repeated neutral feature. One structurally valid declared or compact line reference before the next feature object supplies the first linear-pattern direction. Two ordered references supply a rectangular two-direction pattern: `D1` and `D3` carry the first count and spacing, while `D2` and `D4` carry the second count and spacing. More than two direction references leave the corresponding linear-pattern input unresolved. A missing or multiply addressed preceding object leaves the seed unresolved.

Built-in reference-plane history records have native class `moRefPlane_c` and no dimensions or extra attributes. Within that class, source IDs `2`, `3`, and `4` identify the Front, Top, and Right principal planes.

Legacy compound histories without feature-input classes identify the same principal planes by a complete triplet of classless, parameterless, propertyless `Feature` records at source IDs `2`, `3`, and `4` with one shared nonempty native type token. An incomplete triplet or differing type tokens has no principal-plane identity.

Among classless, parameterless, propertyless history records, `Feature` source ID `1` is the annotations container and `Sketch` source ID `5` is the model origin. Other source IDs are positions in an optional-node sequence rather than role codes.

`moFixedRefPlnData_c` and `moConstraintPerpPlnTanOneCylinderRefplaneData_c` store a 97-byte constructed reference-plane frame immediately after the class name. `moFaceRefPlnData_c` stores either that frame or the 81-byte minimal frame. Three f64 values at offsets `+0`, `+8`, and `+16` store xyz origin coordinates in metres. Three f64 values at `+24`, `+32`, and `+40` store the unit normal. Byte `+48` is `1`. Unit in-plane u- and v-axes occupy the unaligned f64 triples at `+49`, `+57`, `+65` and `+73`, `+81`, `+89`. The three basis vectors are pairwise orthogonal. The frame belongs to the immediately preceding feature object and precedes the next feature object.

The `moConstraintMidPlaneRefplaneData_c` class declaration is followed by eight zero bytes, an f64 geometric tolerance, an f64 signed plane distance in metres, and an xyz unit normal as three f64 values. The resulting plane satisfies `normal · position = distance`. The record does not store an independent in-plane axis.

A zero-origin angular reference-plane frame stores redundant normal z and y components as two f64 values, followed by byte `1`, an unaligned xyz unit u-axis, an xyz unit normal, and an xyz unit v-axis. The three vectors begin 17, 41, and 65 bytes after the first redundant component and are pairwise orthogonal. Normal x is zero; the redundant components equal normal z and y. Twenty-four zero bytes and f64 value `1` terminate the 121-byte frame.

A compact reference-plane frame stores xyz origin coordinates in metres at offsets `+0`, `+8`, and `+16`, normal x and y at `+24` and `+32`, and the complete xyz unit u-axis at `+40`, `+48`, and `+56`. Byte `+64` is zero. V-axis x and y are unaligned f64 values at `+65` and `+73`, and byte `+81` is zero. The omitted v-axis z and normal z are the unique values that make u and v unit and orthogonal and make `normal = u × v` while preserving the stored normal components.

Legacy coordinate-frame groups without serialized plane-reference wrappers contain three consecutive `moRefPlane_c` history records followed by three consecutive `moRefAxis_c` records. All six history ordinals and source IDs are consecutive. The axes use ordered plane pairs `(0,1)`, `(0,2)`, and `(2,1)` in axis-record order.

Each `PMISemanticDataDB` dimension uses `cadText` value `<dimension-name>@<feature-name>` to identify its owning history parameter. The binding is valid when the feature name is unique and all records for the same owner and dimension name encode the same value. `Linear`, `Diameter`, and `Radial` values are f64 metres. These values supply history dimensions when the Keywords record omits them; an explicit Keywords dimension has precedence.

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

- **Bridge `00 0e`:** `refs[2]` = owning loop-head, `refs[4]` = primary surface carrier (compact analytic or `00 7c`), `marker` = face orientation versus the surface natural normal (`0x2b` forward / `0x2d` reversed). `ref0` = owner/use discriminator. The five references are either adjacent big-endian u16 cells followed by the marker at body +26 or `[hi][lo][01]` cells followed by the marker at body +31.
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

For an edge with exactly two coedge uses, equal coedge markers require opposite face senses and opposite coedge markers require equal face senses. The bridge marker of one face anchors the face-sense parity of each connected shell component; applying the edge parity across the component determines every other face sense.

### 4.2 Deltas encodings

Deltas streams re-encode records in prefixed/tripled forms (each ref stored as a `[hi][lo][01]` triple) or as `[disc][attr]` adjacency tables; the magic occurs at the family-specific position within the record window.

| Tag                    | Deltas form | Magic    | Anchor                                 |
| ---------------------- | ----------- | -------- | -------------------------------------- |
| `00 0e`                | tripled     | body +9  | owner triple before magic; five ref triples then marker after |
| `00 0f`                | tripled     | none     | four ref triples at body +6            |
| `00 10`                | prefixed    | body +9  | ref slot 2 = curve carrier             |
| `00 11`                | tripled     | none     | slot4 vuse, slot5 twin, slot6 edge-use |
| `00 12`                | prefixed    | body +21 | refs-before-magic slot 4 = point attr  |
| `00 1d`                | prefixed    | none     | xyz after `[hi][lo][01]*` run          |
| `00 1e/1f/20/32/33/35` | prefixed    | none     | f64 block after `2b`/`2d` marker       |

Post-magic `00 10` reference cells appear as `[01][hi][lo]` or `[hi][lo][01]` triples. Partition and deltas streams in the same outer block share a site namespace. Prefixed and tripled references encode the same u16 attribute values as bare references.

A deltas stream groups its records into change sets. Each change set carries a **change roster**:

```
00 01 00 01  ( attr u16 BE  class u16 BE )*  00 01
```

Roster entries name same-site nodes by attribute and node class, mixing topology, geometry-carrier, and entity classes. Roster membership records that a node belongs to the change set; it does not determine whether the node persists in the final state — a roster names retained, rewritten, and superseded nodes alike. A rostered node with no stored record in any same-site stream has no payload; references to it resolve to nothing.

A deltas change set can re-create a body's faces under new attributes. A deltas bridge with a full record denotes a face of the final stored state; the partition faces it supersedes do not persist. The partition base plus deltas-only bridges therefore overcounts the final face set by the superseded partition faces. A partition without bridges takes its face membership entirely from the deltas stream.

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

In schema 33103, solid ownership follows the same `0x17 -> [0x19] -> 0x1b -> 0x1f -> 0x21 -> 0x23` hierarchy with `0x1b/flo1` as the solid region. A body-reachable `0x1d/flo1` record is a sheet region and references its face-list head in slot 0. `0x1d/flo2` belongs to the face-connectivity web and is not a sheet discriminator.

Schema-33103 canonical faces are the connected components of the disc15/flo1 adjacency graph. Disc13/flo2 face-list heads bind to bodies by the shared `slot0` cluster key. Each head seeds the component with maximum overlap in its section interval; component assignment is one-to-one. The complete component, not the interval contents, is the body's face set.

A cylindrical face spanning a complete angular period stores two loops. Each loop contains one coedge whose edge is closed and whose carrier is a circle coaxial with the cylinder. The stored topology omits the longitudinal seam. Its endpoints are the two circle points `center - ref_direction * radius`, where `ref_direction` is the cylinder carrier's reference direction. The neutral topology joins these endpoints with one line edge, inserts two oppositely oriented radial coedges for that edge, and combines the two circular coedges and two seam coedges into one loop.

Disc14 ownership uses the entity-level shell and face-use lattice. A `0x1a` region reaches each `0x16` shell. A shell reaches its `0x20` face-use through same-site entity references; `0x20.slot3` advances around a shell ring. `0x20.slot2` resolves the canonical face directly or through `0x18.slot2` and `0x1e.slot2` intermediates. The ring closes when the next face-use equals the first. A partition containing one `0x1a` region and one reachable `0x16` shell owns every disc14 face when the `0x20` lattice maps one-to-one onto the complete disc14 face set.

In the disc20 face layout, a `0x1a` region reaches one `0x16` shell. Each canonical `0x20/flo1` face names a `0x24/flo4` node in slot 1. The `0x24` node back-references the face in slot 2 and names a `0x26/flo3` node in slot 1; the `0x26` node back-references the `0x24` node in slot 2. A complete reciprocal lattice assigns every disc20 face to the single shell.

Schema 36001 also carries a single-region disc20 layout with one `0x1a` region. Its descending root chain is `0x1a.slot2 -> 0x18`, `0x18.slot2 -> 0x16`, and `0x16.slot2 -> 0x14`. Its ascending chain is `0x1a.slot1 -> 0x1c`, `0x1c.slot1 -> 0x22`, `0x22.slot1 -> 0x24`, `0x24.slot1 -> 0x26`, and `0x26.slot1 -> 0x2e`. When both chains are complete and the region reaches exactly one `0x16` shell, every canonical `0x20/flo1` face in the site belongs to that shell.

A second schema-36001 single-region layout uses one `0x1a/flo1` region. Its upper root chain is `0x1a.slot1 -> 0x20`, `0x20.slot1 -> 0x28`, `0x28.slot1 -> 0x2a`, and `0x2a.slot1 -> 0x2c`. Its lower root chain is `0x1a.slot2 -> 0x18`, `0x18.slot2 -> 0x16`, `0x16.slot2 -> 0x14`, `0x14.slot2 -> 0x10`, and `0x10.slot2 -> 0x0e`. When both chains are complete, every canonical face in the site belongs to the sole `0x16` shell.

The compact single-region layout uses one `0x1a/flo2` region. `0x1a.slot1` is either a sentinel or names a `0x1c` record; `0x1c.slot1` is either a sentinel or names a `0x1e` companion. Its lower root chain is `0x1a.slot2 -> 0x18`, `0x18.slot2 -> 0x14`, `0x14.slot2 -> 0x12`, and `0x12.slot2 -> 0x10`. `0x10.slot2` either names a `0x0e` terminal or is a sentinel when the site's canonical faces are the `0x0e` records. The `0x14` record is the shell root. A complete lower chain and a valid upper branch assign every canonical face in the site to that shell.

The sparse single-region layout uses one `0x1a/flo2` region and the root chain `0x1a.slot2 -> 0x18`, `0x18.slot2 -> 0x16`, `0x16.slot2 -> 0x12`, `0x12.slot2 -> 0x10`, and `0x10.slot2 -> 0x0e`. The `0x16` record is the shell root. A complete chain assigns every canonical disc14 face in the site to that shell.

The disc1c-root layout uses one `0x1c/flo2` root with a slot-1 sentinel and the chain `0x1c.slot2 -> 0x18`, `0x18.slot2 -> 0x16`, `0x16.slot2 -> 0x14`, `0x14.slot2 -> 0x12`, and `0x12.slot2 -> 0x10`. The `0x16` record is the shell root. A complete chain assigns every canonical disc0e face in the site to that shell.

The direct-shell layout uses one `0x1a/flo2` region with a slot-1 sentinel and the chain `0x1a.slot2 -> 0x16`, `0x16.slot2 -> 0x12`, `0x12.slot2 -> 0x10`, `0x10.slot2 -> 0x0e`, and `0x0e.slot2 -> 0x0c`. The `0x16` record is the shell root. A complete chain assigns every canonical disc14 face in the site to that shell.

The disc20-root layout uses one `0x20/flo2` root with a slot-1 sentinel and the chain `0x20.slot2 -> 0x1e`, `0x1e.slot2 -> 0x1c`, `0x1c.slot2 -> 0x18`, `0x18.slot2 -> 0x16`, `0x16.slot2 -> 0x14`, `0x14.slot2 -> 0x12`, `0x12.slot2 -> 0x10`, and `0x10.slot2 -> 0x0e`. The `0x16` record is the shell root. A complete chain assigns every canonical `0x22/flo4` face in the site to that shell.

The shifted-disc16 layout uses one `0x1c/flo2` region with a slot-1 sentinel and the prefix `0x1c.slot2 -> 0x1a` and `0x1a.slot2 -> 0x18`. Its lower branch is either `0x18.slot2 -> 0x12`, `0x12.slot2 -> 0x10`, and `0x10.slot2 -> 0x0e`, or `0x18.slot2 -> 0x14`, `0x14.slot2 -> 0x10`, `0x10.slot2 -> 0x0e`, and `0x0e.slot2 -> 0x04`. The `0x18` record is the shell root. A complete branch assigns every canonical `0x16/flo1` face in the site to that shell.

The shifted-disc18 layout uses one `0x20/flo2` region with a slot-1 sentinel and the chain `0x20.slot2 -> 0x1c`, `0x1c.slot2 -> 0x1a`, `0x1a.slot2 -> 0x16`, `0x16.slot2 -> 0x14`, `0x14.slot2 -> 0x0e`, and `0x0e.slot2 -> 0x04`. The `0x1a` record is the shell root. A complete chain assigns every canonical `0x18/flo1` face in the site to that shell.

The disc1e-root layout uses one `0x1e/flo2` region with a slot-1 sentinel and the chain `0x1e.slot2 -> 0x1a`, `0x1a.slot2 -> 0x18`, `0x18.slot2 -> 0x16`, and `0x16.slot2 -> 0x12`. The `0x16` record is the shell root. `0x12.slot2` begins a nonempty chain of `0x10/flo2` records linked through slot 2 and terminated by a sentinel. A complete chain assigns every canonical `0x0e/flo1` face in the site to that shell.

The disc12-face layout uses one `0x1a/flo2` region with a slot-1 sentinel and the chain `0x1a.slot2 -> 0x18`, `0x18.slot2 -> 0x16`, `0x16.slot2 -> 0x10`, `0x10.slot2 -> 0x0e`, and `0x0e.slot2 -> 0x04`. The `0x16` record is the shell root. A complete chain assigns every canonical `0x12/flo1` face in the site to that shell.

The disc04-face layout uses one `0x20/flo2` region with a slot-1 sentinel and the chain `0x20.slot2 -> 0x1c/flo2`, `0x1c.slot2 -> 0x1a/flo2`, `0x1a.slot2 -> 0x18/flo1`, `0x18.slot2 -> 0x14/flo2`, `0x14.slot2 -> 0x12/flo2`, and `0x12.slot2 -> 0x0e/flo2`. The `0x1a` record is the shell root. A complete chain assigns every canonical `0x04/flo1` face in the site to that shell.

The disc1e-disc04-face layout uses one `0x1e/flo2` region with a slot-1 sentinel and the chain `0x1e.slot2 -> 0x1c/flo2`, `0x1c.slot2 -> 0x1a/flo2`, `0x1a.slot2 -> 0x16/flo1`, `0x16.slot2 -> 0x14/flo2`, `0x14.slot2 -> 0x12/flo2`, `0x12.slot2 -> 0x10/flo2`, and `0x10.slot2 -> 0x0e/flo2`. The `0x1a` record is the shell root. A complete chain assigns every canonical `0x04/flo1` face in the site to that shell.

The compact-disc16-face layout uses one `0x1a/flo2` region with a slot-1 sentinel and the chain `0x1a.slot2 -> 0x14/flo2`, `0x14.slot2 -> 0x10/flo2`, `0x10.slot2 -> 0x0e/flo2`, and `0x0e.slot2 -> 0x04/flo1`. The `0x14` record is the shell root. The site contains equal nonzero populations of canonical `0x16/flo1` faces and `0x18/flo1` face-use records. A complete chain and paired populations assign every canonical face in the site to that shell.

The compact-disc12-face layout uses one `0x20/flo2` region with a slot-1 sentinel and the chain `0x20.slot2 -> 0x1e/flo2`, `0x1e.slot2 -> 0x1c/flo2`, `0x1c.slot2 -> 0x14/flo2`, `0x14.slot2 -> 0x10/flo2`, `0x10.slot2 -> 0x0e/flo2`, and `0x0e.slot2 -> 0x04/flo1`. The `0x14` record is the shell root. The site contains equal nonzero populations of canonical `0x12/flo1` faces, `0x1a/flo1` face-use records, and `0x22/flo4` use nodes. A complete chain and equal populations assign every canonical face in the site to that shell.

The disc1e-disc0e-face layout uses one `0x1e/flo2` region with a slot-1 sentinel and the prefix `0x1e.slot2 -> 0x1a/flo2`, `0x1a.slot2 -> 0x18/flo2`. The `0x18` record is the shell root. Its slot 2 begins a nonempty acyclic chain of `0x16/flo2` records that terminates at `0x14/flo2 -> 0x10/flo2`; the `0x10` slot 2 is a sentinel. The site contains equal nonzero populations of canonical `0x0e/flo1` faces, `0x12/flo1` face-use records, and `0x1c/flo4` use nodes. A complete chain and equal populations assign every canonical face in the site to that shell.

The disc04-root layout uses one `0x04/flo2` region whose slot 1 names a `0x10/flo2` shell. The shell closes the region link through slot 2. Its slot 1 begins the ascending chain `0x10 -> 0x12/flo2 -> 0x14/flo1 -> 0x18/flo2 -> 0x1a/flo2 -> 0x1c/flo2`; the terminal `0x1c` slot 1 is a sentinel. The site contains equal nonzero populations of canonical `0x0e/flo1` faces, `0x16/flo1` face-use records, and `0x1e/flo4` use nodes. A complete reciprocal root link, ascending chain, and equal populations assign every canonical face in the site to that shell.

The compact-disc0e-face layout uses one `0x20/flo2` region with a slot-1 sentinel and the chain `0x20.slot2 -> 0x1e/flo2`, `0x1e.slot2 -> 0x1c/flo2`, `0x1c.slot2 -> 0x16/flo2`, `0x16.slot2 -> 0x14/flo2`, and `0x14.slot2 -> 0x10/flo2`; the terminal `0x10` slot 2 is a sentinel. The `0x16` record is the shell root. The site contains equal nonzero populations of canonical `0x0e/flo1` faces, `0x1a/flo1` face-use records, and `0x22/flo4` use nodes. A complete chain and equal populations assign every canonical face in the site to that shell.

The disc22-disc12-face layout uses one `0x22/flo2` region with a slot-1 sentinel and the chain `0x22.slot2 -> 0x20/flo2`, `0x20.slot2 -> 0x1c/flo2`, `0x1c.slot2 -> 0x1a/flo2`, and `0x1a.slot2 -> 0x14/flo2`; the terminal `0x14` slot 2 is a sentinel. The `0x1a` record is the shell root. The site contains equal nonzero populations of canonical `0x12/flo1` faces and `0x1e/flo1` face-use records, plus one additional `0x24/flo4` closure node. A complete chain and these population invariants assign every canonical face in the site to that shell.

The disc22-disc18-face layout uses one `0x22/flo2` region with a slot-1 sentinel and the chain `0x22.slot2 -> 0x20/flo2`, `0x20.slot2 -> 0x1a/flo2`, `0x1a.slot2 -> 0x16/flo2`, and `0x16.slot2 -> 0x10/flo2`; the terminal `0x10` slot 2 is a sentinel. The `0x16` record is the shell root. The site contains equal nonzero populations of canonical `0x18/flo1` faces, `0x1e/flo1` face-use records, and `0x24/flo4` use nodes. A complete chain and equal populations assign every canonical face in the site to that shell.

The disc1e-disc14-face layout uses one `0x1e/flo2` region with a slot-1 sentinel and the chain `0x1e.slot2 -> 0x1a/flo2`, `0x1a.slot2 -> 0x18/flo2`, `0x18.slot2 -> 0x16/flo2`, `0x16.slot2 -> 0x12/flo2`, `0x12.slot2 -> 0x0e/flo2`, and `0x0e.slot2 -> 0x04/flo1`. The `0x16` record is the shell root. The site contains equal nonzero populations of canonical `0x14/flo1` faces and `0x1c/flo1` face-use records. A complete chain and equal populations assign every canonical face in the site to that shell.

The disc1e-disc10-face layout uses one `0x1e/flo2` region with a slot-1 sentinel and the chain `0x1e.slot2 -> 0x1c/flo2`, `0x1c.slot2 -> 0x1a/flo2`, `0x1a.slot2 -> 0x16/flo2`, `0x16.slot2 -> 0x14/flo2`, `0x14.slot2 -> 0x0e/flo2`, and `0x0e.slot2 -> 0x04/flo1`. The `0x16` record is the shell root. The site contains equal nonzero populations of canonical `0x10/flo1` faces, `0x18/flo1` face-use records, and `0x20/flo4` use nodes. A complete chain and equal populations assign every canonical face in the site to that shell.

The direct-disc12-face layout uses one `0x1a/flo2` region with a slot-1 sentinel and the chain `0x1a.slot2 -> 0x16/flo2`, `0x16.slot2 -> 0x14/flo2`, `0x14.slot2 -> 0x10/flo2`, `0x10.slot2 -> 0x0e/flo2`, and `0x0e.slot2 -> 0x04/flo1`. The `0x16` record is the shell root. The site contains equal nonzero populations of canonical `0x12/flo1` faces and `0x18/flo1` face-use records, plus two additional `0x1c/flo4` closure nodes. A complete chain and these population invariants assign every canonical face in the site to that shell.

The disc1e-compact-disc04-face layout uses one `0x1e/flo2` region with a slot-1 sentinel and the chain `0x1e.slot2 -> 0x1a/flo2`, `0x1a.slot2 -> 0x18/flo2`, `0x18.slot2 -> 0x14/flo2`, `0x14.slot2 -> 0x12/flo2`, `0x12.slot2 -> 0x10/flo2`, and `0x10.slot2 -> 0x0e/flo1`; the terminal `0x0e` slot 2 is a sentinel. The `0x18` record is the shell root. The site contains equal nonzero populations of canonical `0x04/flo1` faces, `0x1c/flo1` face-use records, and `0x20/flo4` use nodes. A complete chain and equal populations assign every canonical face in the site to that shell.

The disc20-compact-disc04-face layout uses one `0x20/flo2` region with a slot-1 sentinel and the chain `0x20.slot2 -> 0x1e/flo2`, `0x1e.slot2 -> 0x1c/flo2`, `0x1c.slot2 -> 0x18/flo2`, `0x18.slot2 -> 0x16/flo2`, `0x16.slot2 -> 0x10/flo2`, and `0x10.slot2 -> 0x0e/flo1`; the terminal `0x0e` slot 2 is a sentinel. The `0x18` record is the shell root. The site contains equal nonzero populations of canonical `0x04/flo1` faces, `0x1a/flo1` face-use records, and `0x22/flo4` use nodes. A complete chain and equal populations assign every canonical face in the site to that shell.

The disc20-disc12-face layout uses one `0x20/flo2` region with a slot-1 sentinel and the chain `0x20.slot2 -> 0x1a/flo2`, `0x1a.slot2 -> 0x18/flo2`, `0x18.slot2 -> 0x16/flo2`, `0x16.slot2 -> 0x14/flo2`, `0x14.slot2 -> 0x10/flo2`, and `0x10.slot2 -> 0x04/flo1`. The `0x16` record is the shell root. The site contains equal nonzero populations of canonical `0x12/flo1` faces and `0x1e/flo1` face-use records. A complete chain and equal populations assign every canonical face in the site to that shell.

The disc1e-direct-disc04-face layout uses one `0x1e/flo2` region with a slot-1 sentinel and the chain `0x1e.slot2 -> 0x1c/flo2`, `0x1c.slot2 -> 0x1a/flo2`, `0x1a.slot2 -> 0x14/flo2`, `0x14.slot2 -> 0x10/flo2`, and `0x10.slot2 -> 0x0e/flo1`; the terminal `0x0e` slot 2 is a sentinel. The `0x1a` record is the shell root. The site contains equal nonzero populations of canonical `0x04/flo1` faces, `0x18/flo1` face-use records, and `0x20/flo4` use nodes. A complete chain and equal populations assign every canonical face in the site to that shell.

The disc1c-compact-disc04-face layout uses one `0x1c/flo2` region with a slot-1 sentinel and the chain `0x1c.slot2 -> 0x1a/flo2`, `0x1a.slot2 -> 0x16/flo2`, `0x16.slot2 -> 0x14/flo1`, `0x14.slot2 -> 0x12/flo2`, and `0x12.slot2 -> 0x0e/flo2`; the terminal `0x0e` slot 2 is a sentinel. The `0x16` record is the shell root. The site contains equal nonzero populations of canonical `0x04/flo1` faces, `0x18/flo1` face-use records, and `0x1e/flo4` use nodes. A complete chain and equal populations assign every canonical face in the site to that shell.

Sites whose entity families fall outside these disc layouts carry the same ownership content in a class-number-independent form. The bridge owner field names a canonical face entity whose slot 0 is the bridge attribute. A body list head is a `flo = 2` record shaped `[key, root, 1, …]` whose root record carries slot 0 equal to `key` and slot 2 naming the head. The root begins a descending chain of records that share slot 0 = `key` and link through slot 1; the chain terminates at a slot-1 sentinel, and each chain is one stored body with one shell. List heads partition the stream into section intervals in offset order; a body owns the canonical faces whose entity records lie in its interval. A sole chain owns every canonical face in the site.

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

The line direction is unit length. Every axis, normal, and reference direction is unit length; each axis or normal is orthogonal to its paired reference direction. The cone fields satisfy `sin² + cos² = 1`. Both torus radii are positive; the minor radius can equal or exceed the major radius (a spindle torus).

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

An edge's support curve can instead point to a `00 85` **bounded-curve wrapper**. After the compact header and orientation marker, the wrapper stores the source curve attr, start xyz, end xyz, and the source parameter interval as eight f64 BE values. The optional `ff` byte after the tag shifts the compact header by one byte. The stored endpoint coordinates equal evaluation of the referenced source curve at the two interval parameters; the wrapper retains the source curve's geometry and bounds its use.

The Parasolid partition and deltas grammar contains no two-dimensional UV pcurve control array. The `00 2d`, `00 7f`, and `00 80` arrays carry 3D or homogeneous control nets and knot data.

Planar pcurves are the exact inverse of the edge carrier in the support plane frame. Lines remain lines. Coplanar circles and ellipses remain analytic circles and ellipses with the same angular parameter; an edge axis opposite the plane normal reverses the parameter-plane rotation.

An elliptical edge on a cylindrical face has a polar-harmonic pcurve. Its radial-plane coefficients determine cylinder azimuth with `atan2`; its axial coefficients preserve the ellipse carrier's angular parameter. The radial harmonic has constant magnitude equal to the cylinder radius.

A coaxial circle on a circular cylinder or cone is a constant-axial-coordinate pcurve. A coaxial circle on a torus is a constant-minor-angle pcurve. The azimuth origin is the circle reference direction expressed in the surface frame; the azimuth parameter direction is positive when the circle and surface axes agree and negative when they oppose.

A spherical pole-closing edge collapses to the pole `center + radius·axis`. That pole is an existing boundary vertex of the three-circle patch; the seam does not add a point or vertex. Its spatial carrier is degenerate at the pole. Its pcurve is `v = π/2` over the azimuth interval `[0, 2π]`; every parameter value maps to the same pole vertex.

A NURBS surface boundary that shares a complete control row, knot vector, degree, and rational weight vector with its NURBS edge curve is isoparametric. A degree-one clamped surface column with equal endpoint weights is affine; a collinear spatial line has an exact affine pcurve obtained by projecting its origin and unit direction onto that column.

A quadratic rational NURBS edge on a cylinder has a polar-NURBS pcurve when every Bézier span satisfies the homogeneous polynomial identity `X² + Y² = R²W²` in the cylinder radial frame. Its axial control channel is the projection of the same spatial poles onto the cylinder axis. The pcurve shares the edge curve's degree, knots, weights, and parameter; its stored range is the interval whose evaluated endpoints coincide with the edge vertices.

A NURBS surface that is degree one and clamped in `u`, with equal weights at corresponding poles of its two control rows, is ruled in `u`. A spatial line coincident with a fixed-`v` ruling has an affine pcurve: `v` is the common row parameter and `u(t)` is the line parameter projected onto the evaluated ruling vector.

### 7.3 Surface-intersection curve carriers

An edge's `00 10.refs[3]` can point to either intersection carrier for a curve defined by the intersection of two support surfaces. The compact composite has this shape:

```
00 26  attr u16 BE  ordinal u32 BE  refs u16 BE[5]  marker u8 (0x2b|0x2d)
       payload u16 BE[6] = [support0, support1, chart, term_start, term_end, uv]
```

The intersection-data form replaces the two-byte composite tag with `00 01 5a`; the attribute, ordinal, five header references, marker, and six payload references have the same layout and semantics.

`support0` and `support1` reference the two intersected surface records. The remaining payload references resolve to three witness records:

- **`00 28` chart** — the solved point cache: `count u32 BE, attr u16 BE, base_parameter f64 BE, base_scale f64 BE, chart_count u32 BE, chordal_error f64 BE`, one further f64, then two absent-value sentinels `-3.14158e13` at body +36 and +44, then `count` point entries at body +52. An entry is either 88 bytes (point xyz, then a unit tangent at entry +56) or a bare 24-byte point. `chart_count == count`; `base_scale` is nonzero; `chordal_error` is positive. Chart points lie exactly on both support surfaces. The parameter at point `k+1` is the parameter at point `k` plus the chord length times `base_scale`, starting from `base_parameter`.
- **`00 29` terminator** — an exact curve endpoint: `count u32 BE (1|2), attr u16 BE`, a kind label, then the endpoint xyz as f64 BE. The label is one kind character `L` (limit), `H` (ring), or `T` (terminator), optionally followed by a second character `?`, `F`, or `S`. A ring composite names one `H` terminator in both endpoint slots and its chart closes onto itself. Each terminator sits within `chordal_error` of the corresponding chart endpoint; the terminator, not the chart endpoint, is the exact curve end.
- **`00 cc` support-UV values** — `count u32 BE, attr u16 BE, width u8 (2|3|4)`, then `count` f64 BE values, `width` per chart point. `width` 4 carries a UV pair on each support surface. The value count is `width × n` for `n` chart points, or `width × (n + 1)` when the curve crosses a periodic seam of a support surface and the extra row restates the endpoint in the wrapped parameterization. A composite can reference a UV slot with no stored `00 cc` record; the chart and terminators alone define the solved curve.

Terminator and support-UV bodies also occur inline after their field labels: `term_use` followed by `00 00 00 01 01 63 43 5a`, and `values` followed by `00 00 00 02 01 66 01`, each directly preceding the same body layout.

The chart is a solved cache: the exact curve is the surface–surface intersection, and the chart polyline through the terminators reproduces it to within `chordal_error`.

### 7.4 Constant-radius rolling-ball surfaces

A `00 38` surface carrier defines a circular rolling-ball blend between two support surfaces:

```
00 38 [ff]? attr u16 BE  ordinal u32 BE  refs u16 BE[5]
             marker u8 (0x2b|0x2d)
             selector u8 (45|52)  support0 u16 BE  support1 u16 BE  spine u16 BE
             offset0 f64 BE  offset1 f64 BE  side0 f64 BE  side1 f64 BE
```

For selector `52`, `support0` and `support1` directly name the two support-surface carriers. For selector `45`, `support1` remains direct and `support0` names a selector-`52` support-pair record. A support-pair record has zero `offset0` and `offset1`; its first two references name candidate surfaces and its third reference names their intersection curve. The pair member whose face shares a topological edge with the selector-`45` face is that face's first blend support. Exactly one pair member is adjacent.

For a blend record, `spine` names the center/spine curve that selects the blend branch. `abs(offset0) == abs(offset1) > 0`; their common magnitude is the constant rolling-ball radius. A sign difference between the offsets reverses the second support relative to the first. Each `side` value is exactly `+1` or `-1`; a negative value independently reverses that support's natural-normal side. Lengths are metres.

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
