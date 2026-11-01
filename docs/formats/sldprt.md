# SolidWorks `.sldprt`: Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/sldprt.md`](../layouts/sldprt.md), generated from `docs/layouts/sldprt.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. `cargo test -p cadmpeg --test layout_tables` proves the two agree.

## 1. File container

SLDPRT uses two outer envelopes. The block envelope begins with the header and
frames below. The compound-document envelope begins with the OLE2 magic
`d0 cf 11 e0 a1 b1 1a e1`; its UTF-16LE directory contains the
`ISolidWorksInformation` stream.

The compound-document envelope uses Compound File Binary version 3 or 4. Its
header sector is 512 or 4096 bytes respectively; regular sectors have the same
size and mini sectors are 64 bytes. Header DIFAT entries and chained DIFAT
sectors identify FAT sectors; the last DIFAT sector links to ENDOFCHAIN, and a
FREESECT link there is malformed. FAT chains identify the directory stream, the
root mini stream, the mini-FAT stream, and regular streams. Directory entries
are 128 bytes and carry a NUL-terminated UTF-16LE name, object type, left and
right sibling identifiers, child identifier, first sector, and u64 stream size.
The sibling trees and storage children form slash-qualified stream paths such
as `Contents/Config-0-Partition`.

A directory slot whose object-type byte is zero is unallocated. Its name,
links, color, sector, and size fields are not interpreted, even when those
fields contain nonzero stale bytes. The slot remains in the directory index so
directory identifiers retain their numeric positions, but it does not enter a
sibling tree or a storage path. A nonzero object type is `1` for a storage, `2`
for a stream, or `5` for the root storage.

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

Native block payloads that carry wrapped Parasolid streams use a chained-section
form. The payload is a sequence of sections, with optional zero padding inside
each section:

```
section := chain_len u32 LE, magic bytes[16], frame+, zero padding
frame   := uncompressed_size u32 LE, zlib_member_size u32 LE,
           zlib_member[zlib_member_size]
```

The magic is the same 16-byte value as the compound wrapper. `chain_len` is the
number of bytes after the `chain_len` field and before the next `chain_len`
field, or the end of the payload. It includes the magic, every frame, and the
section's zero padding. Each frame inflates to exactly `uncompressed_size`;
the declared member extent is the complete zlib member. Concatenating the
inflated frames of one section in payload order produces one complete
Parasolid stream. The concatenation occurs before stream-header parsing and
record scanning, so records may cross frame boundaries. Every section is
extracted independently, including partition and deltas sections. A section
with one frame is the degenerate chained form.

### 1.1 Outer header and block frame

The file starts with an 8-byte header: `file_id` (u32), then `version` (u32 **big-endian**; observed value `0x00000004`). The decoder stores the word in the `outer_version` attribute; every value decodes (`dialect.rs`). The rest is a sequence of compressed blocks.

Native compressed members use ZIP local-file-header fields with a substituted
four-byte signature token and nibble-swapped member names. The signature token
is retained as opaque bytes. The archive coordinate zero is absolute file offset
8. The field marker begins four bytes after the local signature. In the native
DEFLATE profile, the marker is `14 00 06 00 08 00`: version needed 20,
general-purpose flags 6, and compression method 8.

The layout follows [PKWARE ZIP APPNOTE §§4.3.7 and 4.4.6](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).
All fields below use little-endian order. `zip_local_header` fixed size is 30 bytes.
Offsets start at the substituted local signature.

| Offset | Type | Field |
| ---: | --- | --- |
| +0 | bytes[4] | signature |
| +4 | u16 | version_needed |
| +6 | u16 | flags |
| +8 | u16 | method |
| +10 | u16 | mod_time |
| +12 | u16 | mod_date |
| +14 | u32 | crc32 |
| +18 | u32 | comp_sz |
| +22 | u32 | uncomp_sz |
| +26 | u16 | name_len |
| +28 | u16 | extra_len |


The encoded name follows the fixed header, then `extra_len` extra bytes, then
`comp_sz` raw-DEFLATE bytes. The payload expands to exactly `uncomp_sz` bytes
and has CRC-32 `crc32`. Swapping the high and low nibble of each name byte
produces names such as `Contents/Config-0-Partition` and `PreviewPNG`.

The zero-extra local-field projection has a 26-byte fixed prefix. It starts at
the field marker and preserves the existing packed field names:

```text
marker bytes[6] ; version needed, flags, method
type_id u32 LE ; packed ZIP modification time/date
crc32 u32 LE ; CRC-32 of decompressed payload
comp_sz u32 LE ; compressed size
uncomp_sz u32 LE ; uncompressed size
pre_sz u32 LE ; name length with zero extra length
```

In that projection, `pre_sz` packs the adjacent ZIP u16 name length and zero
u16 extra length; `block_end = marker_offset + 26 + pre_sz + comp_sz`.
With a nonzero extra length, the complete ZIP extent instead uses the sum of
the two u16 lengths. The packed u32 is not that sum.

`type_id` packs two ZIP u16 fields: MS-DOS modification time in the low half
and modification date in the high half. It does not identify a payload type.
The time encodes seconds divided by two in bits 0..4, minutes in bits 5..10,
and hours in bits 11..15. The date encodes day in bits 0..4, month in bits
5..8, and year minus 1980 in bits 9..15. Preserve the raw bits even when they
do not form a valid calendar date or clock time. Local and central fields for
one member carry the same raw time/date, CRC, sizes, name length, and name.

### 1.2 Cache-cell section-index grid

The cache marker-relative fixed prefix is 26 bytes. `cache.marker at +0`
is the six-byte marker; `cache.type_id at +6` is the packed ZIP time/date
u32 LE. The size and name-length fields are u32 LE.

The bytes before the first valid block and the long inter-block gaps hold a **fixed-cell section-index grid** reusing the outer marker. These cells are **not** compressed payloads. The three size-shaped header fields are redundant scalings of one logical value `L`:

```
field@+10 == 2*L
field@+14 == L // 2
field@+18 == L
field@+22 == name_len       ; nibble-swapped section name follows at +26
```

Each file has one cache-cell stride `S`; every `L` is an integer multiple (`L = N*S`, `N in 5..14`).

A valid cache cell satisfies `two_L == 2L`, `half_L == L//2`, `0 < name_len < 500`, and has a printable nibble-swapped name.

The outer-header end or the exact end of a preceding validated block anchors
a cache-cell run. Each following cell starts at the preceding cell's end.
For an independently anchored `cell_start`, the complete ranges are:

```
marker_offset = cell_start + N
name_end      = marker_offset + 26 + name_len
cell_end      = cell_start + S
prefix        = bytes[cell_start .. marker_offset]
trailing      = bytes[name_end .. cell_end]
```

The marker displacement equals `L/S`; the complete header and name fit within
the cell. Cell ranges do not overlap each other or validated block ranges.
The prefix and trailing ranges are opaque retained bytes. A retained cell
preserves both ranges byte for byte. Bytes between the last complete cell and
the following block are outside that cell.

A physically attached run ends exactly four bytes before a validated local
field marker; the four bytes are that member's substituted local signature.
Each cell in that run carries the successor local member's complete raw
modification time/date u32. The decoded cell name does not select that word.
A cell without that physical-successor relation retains its raw word
without a current-member association.

### 1.3 Tail section directory

The tail directory consists of contiguous ZIP central-directory file headers
with substituted four-byte signature tokens and nibble-swapped member names.
Its field order follows [PKWARE ZIP APPNOTE §4.3.12 and §§4.4.12–4.4.16](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).
All multibyte fields are little-endian. `zip_central_header` fixed size is 46 bytes.
Offsets start at the substituted central signature.

| Offset | Type | Field |
| ---: | --- | --- |
| +0 | bytes[4] | signature |
| +4 | u16 | version_made_by |
| +6 | u16 | version_needed |
| +8 | u16 | flags |
| +10 | u16 | method |
| +12 | u16 | mod_time |
| +14 | u16 | mod_date |
| +16 | u32 | crc32 |
| +20 | u32 | comp_sz |
| +24 | u32 | uncomp_sz |
| +28 | u16 | name_len |
| +30 | u16 | extra_len |
| +32 | u16 | comment_len |
| +34 | u16 | disk_start |
| +36 | u16 | internal_attrs |
| +38 | u32 | external_attrs |
| +42 | u32 | local_offset |


The encoded name follows the fixed header, then `extra_len` extra bytes, then
`comment_len` comment bytes. The next central header begins at
`central_start + 46 + name_len + extra_len + comment_len`. A marker-like
pattern elsewhere in the file does not establish a directory entry.

`zip_central_descriptor` fixed size is 14 bytes. It is the final fixed portion
of a central header, beginning at central signature +32:

| Descriptor offset | Type | Field |
| ---: | --- | --- |
| +0 | u16 | comment_len |
| +2 | u16 | disk_start |
| +4 | u16 | internal_attrs |
| +6 | u32 | external_attrs |
| +10 | u32 | local_offset |


`comment_len` is the file-comment byte count. `disk_start` identifies the disk
on which the local member begins. `internal_attrs` bit 0 is the ZIP producer's
apparently-text hint; zero indicates apparently binary data. It does not
identify XML or a SolidWorks payload family. `external_attrs` uses the host
system interpretation selected by `version_made_by`. These are fields, even
when their values are zero.

`local_offset` is relative to archive coordinate zero at absolute file offset
8. Thus the local signature is at `local_offset + 8` and its field marker is
at `local_offset + 12`. The referenced local header carries the same member
name, raw time/date, CRC, and compressed/uncompressed sizes. An empty member
has its own local header with CRC zero, compressed size two, uncompressed size
zero, and raw-DEFLATE payload `03 00`.

The empty-member scan projection has a 40-byte marker-relative fixed prefix.
It preserves these existing packed field names:

```text
tail.marker at +0       bytes[6] ; version needed, flags, method
tail.type_id at +6      u32 LE ; packed modification time/date
tail.zero_at_10 at +10  u32 LE ; CRC zero
tail.size at +14        u32 LE ; compressed size
tail.zero_at_18 at +18  u32 LE ; uncompressed size zero
tail.name_len at +22    u32 LE ; name length plus zero extra length
tail.descriptor at +26  bytes[14]
```

This projection describes the empty-member, zero-extra-length subset of the
central header. It does not describe ordinary nonempty central entries. The
six bytes after an encoded name in a contiguous zero-extra, zero-comment
chain are the next central signature token and its u16 version-made-by field.
They are not an entry-local trailer. The final entry has no implied next-header
trailer. Existing container projections may retain those lookahead bytes
without assigning them to the preceding entry.

### 1.4 Tail padding grids and end-of-directory root

The real central directory contains one record per validated local member,
including each empty member. An EOCD root carries the real record count and
the archive-relative central-directory offset. Its body follows
[PKWARE ZIP APPNOTE §4.3.16](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT).
The substituted signature is opaque retained bytes.

Let `S` be the first real central signature, `M=S+6` its field marker, and `R`
the real central-record count. A root with body start `h` has disk fields zero,
both counts equal to `R`, zero comment length, and both locator equations:

```text
central_offset + 8 = S
central_size = root_signature_start - S = (h+2)-M
```

The declared size spans from the first real central signature to the root
signature, including intervening padding. The count selects real central
records; central-like padding headers do not increase it. Root selection uses
both locators and the count, rather than a zero-valued field or position alone.

`named_padding_header` fixed size is 40 bytes. All multibyte fields are little-endian.

| Offset | Type | Field |
| ---: | --- | --- |
| +0 | bytes[6] | marker |
| +6 | u32 | time_date |
| +10 | u32 | two_l |
| +14 | u32 | half_l |
| +18 | u32 | l |
| +22 | u16 | name_len |
| +24 | u16 | extra_len |
| +26 | bytes[14] | descriptor |


`footer_padding_body` fixed size is 18 bytes. All multibyte fields are little-endian.

| Offset | Type | Field |
| ---: | --- | --- |
| +0 | u32 | disk_fields |
| +4 | u16 | entries_disk |
| +6 | u16 | entries_total |
| +8 | u32 | named_stride |
| +12 | u32 | five_l |
| +16 | u16 | comment_len |


`zip_eocd_header` fixed size is 22 bytes. All multibyte fields are little-endian.

| Offset | Type | Field |
| ---: | --- | --- |
| +0 | bytes[4] | signature |
| +4 | u16 | disk_number |
| +6 | u16 | central_disk |
| +8 | u16 | entries_disk |
| +10 | u16 | entries_total |
| +12 | u32 | central_size |
| +16 | u32 | central_offset |
| +20 | u16 | comment_len |


The named padding grid, when present, starts two bytes after the final real
central name. Its stride is `B`; each cell starts at `x` and its marker is at
`x+N`. Its scalars obey `L=N*B`, `two_l=2L`, and `half_l=L//2`. The name uses
the central-like prefix above and has zero extra length. The fixed prefix and
name fit wholly inside `[x,x+B)`. Adjacent cells begin exactly one stride
apart. Prefix and residual bytes remain opaque. These records are padding,
not local members or additional central-directory entries.

If `T` is the end of the final named cell, the anonymous footer starts at
`T-2`. The two bytes lie in the named cell's opaque residual. Ordinary footer
cells have stride `Q`, body displacement `N`, zero disk fields and comment
length, both apparent counts equal to `R`, `named_stride=B`, and
`five_l=5*N*Q`. The complete body fits within its physical cell.

The real root is a 22-byte cell whose body begins four bytes after its
substituted signature. Walk the footer by `Q` for each ordinary cell and 22
for the root; the resulting ranges tile the footer through EOF. A root can
occur between ordinary cells. In the root-only form, the final real name ends
at the root signature and the 22-byte root ends at EOF; no named or ordinary
footer stride is inferred.

A valid grid has independently anchored cell starts, complete cell bounds,
consistent scaling equations, and exact EOF closure. Opaque prefix, residual, and signature bytes are retained exactly;
zero or `ff` bytes are valid opaque values. Their values do not select
directory membership.

---

## 2. Block payload families

Payload kind is determined from **decompressed bytes**, not from `type_id`.

| Signature at/near payload start                 | Family                      |
| ----------------------------------------------- | --------------------------- |
| `89 50 4e 47`                                   | PNG preview                 |
| BITMAPINFOHEADER size 40 + valid bit depth      | BMP thumbnail               |
| OLE2 compound-file header                       | OLE2                        |
| `PS 00 00`                                      | plain Parasolid stream      |
| Parasolid chained-section magic at offset 4     | wrapped Parasolid           |
| nested compressed member → `PS 00 00`           | nested Parasolid            |
| `uoTempBodyTessData_c` / `uoTempFaceTessData_c` | tessellation / DisplayLists |
| `ff ff 01 00`                                   | SW Objects                  |
| `<?xml` / UTF-16LE BOM+XML / byte0 `86`+XML     | XML                         |
| `unqlite`                                       | UnQLite database            |

PNG preview dimensions and encoding fields are in `IHDR`. BMP thumbnail width, height, planes, bit depth, compression, and image size are in the 40-byte BITMAPINFOHEADER after the leading file-size word. The `swSolidWorks` XML root carries version, creation time, and path; its `swModel` child carries model and configuration names.

`Contents/Config-0-Partition` and `Contents/Config-0-Deltas` carry body B-rep records. `Contents/Config-0-ResolvedFeatures` carries feature-input objects and nested Parasolid streams. A nested stream whose Parasolid description begins `: TRANSMIT FILE` has a kernel transmit header. This header and its numeric `SCH_<n>_…` schema do not identify a sketch or a support snapshot. A nested kernel transmit with a solid BODY (`body_type` `1`) established by the typed ownership graph is a solid-body snapshot. Its planar faces are not sketch profiles. A profile can instead carry a sheet BODY (`body_type` `3`) under the same kernel header and schema. A nested stream described by the sketch's name with schema `SCH_SW_33103_11000` is the sketch's profile. Legacy compound documents with no `ResolvedFeatures` stream store the same feature-input arena directly in the numeric `Contents/Config-N` stream. When explicit `ResolvedFeatures` streams exist, plain `Config-N` streams do not supply modeling-operation classes, parameters, configuration state, or history feature classes. A numeric `Config-N` stream containing `sgSketch` and at least one sketch-entity class is a supplemental config-object lane. Its complete persistent component paths remain operation operands when they bind uniquely to an already classified history feature. When `moFeatureDimHandle_c` is present, only sketch markers before its first declaration belong to user sketch objects; later markers belong to display state. Without that declaration, the sketch-marker interval extends to the end of the lane. Runs separated by at least 4096 bytes are distinct sketch objects. The runs bind in stream order to otherwise unrepresented non-origin sketch history objects in ascending source-identifier order, and only when the run and owner counts are equal. Constraint-bearing markers in a bound run belong to the same sketch object as the run's geometry markers. `Config-0-GhostPartition`, `Contents/Definition`, and `Config-0-LWDATA` are separate payload families.

A supplemental hex-profile sketch object has one coordinate marker for each object index 9 through 14 in ascending stream order, coordinate markers 4 and 5 for the horizontal construction axis, coordinate markers 7 and 8 for the vertical construction axis, an origin coordinate marker with object index 3 or a null object index and zero coordinates, a circle-center coordinate marker 15, a construction-circle radial marker 16, and an ordinary-circle radial marker 17. It has exactly nine non-coordinate line-or-circle markers in stream order: horizontal axis, vertical axis, construction circle, and six cyclic profile edges. The six indexed vertices form the outer profile; markers 15 and 17 define the ordinary circle; the origin and marker 16 define the construction circle.

A supplemental collinear dimension sketch object has exactly four non-coordinate line-or-circle markers. The first stores `1e 00` at marker +56, a finite negative f64 horizontal coordinate at marker +58, and zero f64 at marker +66. The unique null-index coordinate marker is the origin. Coordinate markers 18, 19, and 21 are collinear with the origin, have nonnegative horizontal coordinates, and form a strictly ordered four-point chain after coincident loci are collapsed. The first line joins the inline negative point to the origin; the remaining lines join consecutive chain points. Every distinct coordinate locus and the inline negative locus is a neutral sketch point.

`ResolvedFeatures` sketch entities begin with `ff ff 1f 00 03`. A little-endian u32 at marker +17 is interpreted with the record layout. Coordinate-bearing values `0`, `1`, `2`, and `3` denote point, curve, arc, and constrained point. Marker +48 stores a finite little-endian f64 state value. The trailing little-endian u32 is the feature-local object identifier; `ff ff ff ff` is null. A coordinate-bearing marker has the 12-byte prefix `ff ff ff ff ff ff ff ff 00 00 80 bf` at marker +5. One layout stores `1e 00` at marker +64 and two finite little-endian f64 coordinate fields at marker +66 and +74, in metres. Current and compact legacy profile-point candidates may instead store `1e 00` at marker +56 and the coordinate pair at marker +58 and +66. A compact legacy profile-point candidate ends at the next sketch marker at marker +134, +138, +146, +150, +154, +161, or +162. Legacy solved-geometry-locus markers use the same offsets for all geometry types. The four bytes `04 00 02 00` at marker +23 identify an extended profile vertex, or a coordinate marker used as a vertex when an indexed profile curve owned by the same feature references its object index. A coordinate-bearing geometry-locus marker selected by a resolved object-index, coordinate-roster, or local-link curve endpoint is likewise a profile vertex, irrespective of its native value, unless the marker itself resolves as a two-ended curve. Vertex status propagates transitively through the typed endpoint graph. Other unselected coordinate markers with these loci are display handles and do not participate in solved sketch geometry. Marker +84 stores a little-endian u16 local-link count of one or two. Marker +86 begins that many adjacent 12-byte cells. Every cell uses one homogeneous curve-selector tag, followed by a little-endian u16 feature-local object identifier, `ff ff ff ff`, and four zero bytes. Two zero bytes and `fe ff ff ff` terminate the vector. Coordinate records are 142, 152, 154, 158, or 162 bytes. Their object identifier is layout-selected at marker +138, +148, +150, +144, or +158 respectively, and they may be followed by a four-byte separator before the next marker. The 92-byte reference-bearing variant stores two little-endian u16 feature-local object identifiers at marker +64 and +66, a little-endian u16 selector at marker +68, zero at marker +70, little-endian f64 `-1.0` at marker +72, and its object identifier at marker +88. Each referenced identifier is resolved independently against typed sketch markers owned by the same feature object as the referencing marker.

The extended prefix has a geometry-locus profile vertex with value u32 `1`, locus `05 00 01 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` or `00 00 80 bf 00 00 05 00` at marker +31, f64 `1` at marker +48, `1e 00` at marker +56, and its finite planar coordinates at marker +58 and +66. The 134-byte form stores state u32 `0` or `1` at marker +74, zero through marker +83, `fe ff ff ff` at marker +84, zero through marker +129, and `ff ff ff ff` at marker +130 before the next marker. The 138-byte identity-bearing form stores state u32 `1` at marker +74, zero through marker +83, `fe ff ff ff` at marker +84, zero through marker +123, the same nonzero, non-null u32 identity at marker +124 and +128, and `00 00 01 00 00 00` at marker +132 before the next marker.

The extended prefix defines a 138-byte geometry-locus point with header `ff ff ff ff ff ff ff ff` at marker +5, native value u32 `2`, locus `05 00 01 00`, role u16 `1`, marker +29 u16 zero, selector `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, `1e 00` at marker +56, and finite planar coordinates at marker +58 and +66. Marker +74 stores u16 state `0` followed by u16 link count `1`; marker +78 through +81 are zero; marker +82 stores i32 `-1`; marker +86 through +123 are zero; marker +124 and +128 store distinct nonzero, non-null u32 identities; and marker +132 stores `00 00 01 00 00 00`. The next sketch marker begins at marker +138.

The current prefix has a 134-byte geometry-locus profile vertex with value u32 `1`, locus `05 00 01 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, `1e 00` at marker +56, and finite planar coordinates at marker +58 and +66. Marker +74 stores `00 00 01 00`, marker +78 is zero u32, marker +82 stores u16 `1`, marker +84 stores i32 `-2`, marker +88 through +129 are zero, and marker +130 stores the following marker's nonzero, non-null u32 object identifier. The following sketch marker begins at marker +134.

The legacy prefix has the same 134-byte geometry-locus point layout, coordinate offsets, state fields, sentinel, zero trailer, following-object identifier, and record boundary. A native value `2` variant stores u32 `1` at marker +74, zero bytes through marker +125, and two distinct nonzero, non-null u32 identities at marker +126 and +130 before the next marker.

An extended-prefix 134-byte value-2 geometry-locus record is a selected construction line when marker +29 is zero u16, marker +74 stores u32 state `1`, marker +78 through marker +83 are zero, marker +84 stores i32 `-2`, marker +88 through marker +125 are zero, and marker +126 and marker +130 store nonzero, non-null u32 identities. Its first endpoint is the finite coordinate at marker +58 and +66. The identity at marker +130 selects the unique coordinate-bearing point, constrained-point, line-or-circle, or arc marker with that object index in the same feature object. That marker's coordinate is the second endpoint. A sequence whose trailing identities select subsequent coordinate-bearing curve markers defines consecutive construction-line segments and can terminate at a point marker.

The extended prefix defines a 96-byte geometry-locus selected construction line with header `ff ff ff ff 04 00 ff ff` at marker +5, native value u32 `0`, locus `05 00 01 00`, role u16 `2`, marker +29 u16 zero, selector `00 00 80 bf 00 00 0c 00` at marker +31, and f64 `1` at marker +48. Its distinct nonzero u16 endpoint object identifiers are at marker +56 and +58; marker +60 through +63 are zero; marker +64 stores f64 `-1`; marker +72 through +79 are zero; marker +80 stores `00 00 04 00`; marker +84 through +87 are zero; and nonzero, non-null u32 identities occupy marker +88 and +92. The next sketch marker begins at marker +96. The endpoint identifiers select unique coordinate-bearing markers by feature-local object index, and the neutral geometry is their construction-line chord.

The compact `ff ff 07 00 01` generation stores its native value u32 at marker +13, bytes +17 through +18 are zero, profile locus `04 00 02 00` at marker +19, and role u16 at marker +23. Its header at marker +5 is either eight `ff` bytes or `ff ff ff ff 04 00 ff ff`. A role-1 vertex has value `1`, `1e 00` at marker +42, and two finite f64 coordinates at marker +44 and +52 in metres. A role-1 indexed line has value `0`, state u16 `1` at marker +25, bytes +27 through +30 are zero, byte `04` at marker +31, zero byte at marker +32, u16 `0` or `24` at marker +33, and seven zero bytes at marker +35 through +41, zero-based endpoint u16 values at marker +42 and +44, u32 `1` at marker +46, and f64 `-1` at marker +50. In the profile variant with u16 `24`, marker +58 stores zero u16, marker +60 stores a feature-local object u16, marker +62 stores zero u16, marker +64 stores a feature-local object u16, and marker +66 stores zero u16. The 68-byte form ends at the next sketch marker; adding one to each endpoint value gives the object index of an endpoint vertex owned by the same feature object. A role-2 68-byte curve uses byte `0c` at marker +31 and the same endpoint fields. When its one-based pair does not resolve two distinct coordinate-bearing points, the raw fields are zero-based ordinals in the feature-owned coordinate-bearing marker roster in marker order. The geometry-locus 90-byte line form has native value `1`, bytes +17 through +18 are zero, locus `05 00 01 00` at marker +19, role u16 `1`, state u16 `1`, bytes +27 through +30 are zero, the same byte `04` body at marker +31, u32 `1` at marker +58, bytes +62 through +63 are not interpreted by this form, four consecutive i32 `-2` cells at marker +64, zero u16 at marker +80, nonzero non-null u32 identities at marker +82 and +86, and the next sketch marker at marker +90. Its endpoint values are zero-based ordinals in the feature-owned sketch-marker roster, ordered by marker offset; each selected entry must be a coordinate-bearing point. The terminal form extends the line record through marker +137, has zero bytes from marker +82 through +135, and is followed by `08 80` at marker +136. An 84-byte compact-legacy roster curve uses native/role/selector combinations `(0, 1, 05)`, `(1, 1, 0c)`, or `(2, 2, 0c)`, with f64 `1` at marker +48, zero u32 at marker +60, f64 `-1` at marker +64, and the next sketch marker at marker +84. Its distinct endpoint u16 values at marker +56 and +58 are zero-based ordinals in the complete feature-local coordinate-bearing marker roster, including coordinate-bearing relation markers, and each selected entry is a point, constrained point, line-or-circle, or arc. Profile curves use trailer state `00 00 00 00` or `00 00 02 00`; selected construction curves use state `00 00 01 00` with zero plus a nonzero non-null identity, or zero state with one repeated nonzero non-null identity.

A packed compact-generation curve stores four zero bytes at marker +25, f64 `1` at marker +40, distinct zero-based coordinate-roster endpoint ordinals at marker +48 and +50, zero u32 at marker +52, f64 `-1` at marker +56, zero u16 at marker +64, local state u16 at marker +66, zero u32 at marker +68, a nonzero non-null identity at marker +72, and the next sketch marker at marker +76. A value-0 role-1 curve uses selector byte `05` at marker +29 and local state `0`, `1`, or `2`; it is a profile line. A value-1 role-2 curve uses selector byte `0c` and local state `2`; it is the selected construction line. When both profile-line ordinals lie beyond the explicit coordinate roster, the sole such unresolved line in a feature closes the unique connected open chain of solved role-1 curves; the chain's two degree-one points are its endpoints.

A compact-generation full circle has value u32 `1`, profile locus `04 00 02 00`, role u16 `1`, state u16 `1` at marker +25, byte `04` followed by ten zero bytes at marker +31, the same zero-based radial-point ordinal at marker +42 and +44, u32 `1` at marker +46, f64 `-1` at marker +50, u32 `1` at marker +58, zero u16 at marker +62, four i32 `-2` cells at marker +64, and zero u16 at marker +80. The continuation form stores a nonzero, non-null identity at marker +82, the following marker's nonzero, non-null object identifier at marker +86, and the next sketch marker at marker +90. The terminal form stores 30 zero bytes at marker +82, u16 `4` at marker +112, and an `sgCircleDim` class declaration at marker +114. Feature-owned sketch markers form a zero-based roster ordered by marker offset. The first coordinate-bearing point is the center and the repeated ordinal selects a coordinate-bearing radial point. Their finite nonzero distance is the radius.

A current- or extended-prefix 154-byte linked profile-point record stores value u32 `0` or `2` at marker +17, profile locus `04 00 02 00` at marker +23, role u16 `1` at marker +27, `1e 00` at marker +56, and the finite planar coordinate pair at marker +58 and +66. Marker +74 is zero, marker +76 stores link count u16 `2`, and two typed 12-byte curve-incidence cells occupy marker +78 through +101. Each cell stores a recognized curve-selector tag, a u16 curve identifier, `ff ff ff ff`, and four zero bytes. The identifiers are distinct. The bytes `00 00 fe ff ff ff` terminate the links at marker +102, marker +108 through +145 are zero, and marker +146 stores a non-null u32 record identity. The following marker's object identifier occupies marker +150, and its sketch prefix begins at marker +154. The extended prefix also has a 158-byte form with zeros from marker +108 through +143, non-null u32 identities at marker +144 and +148, zero u16 at marker +152, the following marker's object identifier at marker +154, and its sketch prefix at marker +158. The legacy prefix uses this 158-byte tail for a value u32 `1` geometry-locus point with locus `05 00 01 00` and link count u16 `2`. For a current-prefix kind-1 indexed curve, its feature-local object index selects incidence cells by curve identifier. A selector tag supplies the curve's endpoints when exactly two linked profile-point records carry that tag and curve identifier and no second selector tag supplies another pair.

Two 154-byte coordinate-only linked-point forms use the same header, coordinate pair, incidence cells, terminator, 42-byte zero trailer, and following marker position. The extended-prefix form has profile locus `04 00 02 00`, state u16 `1`, and link count u16 `3`. The legacy-prefix form has geometry locus `05 00 01 00`, zero state u16, and link count u16 `2`. Their native code is `0` or `2`. Each incidence selector is nonzero and non-null, and the two `(selector, identifier)` pairs are distinct. These incidence cells identify the point record but do not define curve endpoints.

A legacy-prefix 154-byte geometry-locus linked profile-point record has native value `1`, role u16 `1`, zero state u16 at marker +29, coordinate tag `1e 00` at marker +56, finite coordinates at marker +58 and +66, zero u16 at marker +74, link count u16 `2` at marker +76, and two typed 12-byte incidence cells at marker +78 and +90. The cells share the record's curve-selector type and store a u16 feature-local identifier, `ff ff ff ff`, and four zero bytes. The link terminator begins at marker +102, marker +108 through +149 are zero, the record-local identifier is at marker +150, and the next sketch marker begins at marker +154. Zero is a valid incidence identifier.

A legacy-prefix geometry-locus alternate-tag point family has eight `ff` bytes at marker +5, f32 `-1` at marker +13, zero bytes at marker +21 and +22, geometry locus `05 00 01 00` at marker +23, native value `0`, `1`, or `2`, role u16 `1`, state u16 `1` at marker +29, `00 00 80 bf 00 00 04 00` or `00 00 80 bf 00 00 05 00` at marker +31, zero bytes from marker +39 through +47, f64 `1` at marker +48, and coordinate tag `12 00`, `13 00`, or `16 00` at marker +56. Its finite planar coordinates are at marker +58 and +66. The 134-byte forms store either a nonzero, non-null u16 count at marker +74 followed by zero bytes through marker +83, or zero at marker +74, u16 `1` at marker +76, zero bytes through marker +81, and u16 `1` at marker +82. Both forms store i32 `-2` at marker +84, zero bytes from marker +88 through +129, a u32 value other than `ffff` at marker +130, and the next sketch marker at marker +134. The 138-byte form stores zero at marker +74, u16 `1` at marker +76, zero bytes through marker +81, u16 `1` at marker +82, i32 `-2` at marker +84, zero bytes from marker +88 through +123, nonzero non-null u32 identities at marker +124 and +130, zero bytes at marker +128 and +129, the marker +130 identity again at marker +134, and the next sketch marker at marker +138. The 154-byte form stores zero at marker +74, link count u16 `2` at marker +76, and two typed 12-byte incidence cells at marker +78 and +90. Each cell stores a recognized line-or-circle/arc selector, a u16 identifier other than `ffff`, `ff ff ff ff`, and four zero bytes; the `(selector, identifier)` pairs are distinct. The terminator `00 00 fe ff ff ff` begins at marker +102, marker +108 through +149 are zero, marker +150 stores a u32 value other than `ffff`, and the next sketch marker begins at marker +154. Every form emits a point.

A legacy-prefix 84-byte geometry-locus indexed curve has eight `ff` bytes at marker +5, f32 `-1` at marker +13, native value u32 `2` at marker +17, zero bytes at marker +21 and +22, geometry locus `05 00 01 00` at marker +23, role u16 `1`, state u16 `2` at marker +29, selector `00 00 80 bf 00 00 04 00` at marker +31, zero bytes through marker +47, and f64 `1` at marker +48. Its distinct endpoint u16 values at marker +56 and +58 are zero-based ordinals in the complete feature-local coordinate-bearing marker roster. Marker +60 stores u32 `1`, marker +64 stores f64 `-1`, marker +72 stores zero u32, and distinct nonzero, non-null u32 identities occupy marker +76 and +80. The next sketch marker begins at marker +84. The record is a bounded-arc candidate; when no unique center or following tangent detail defines an arc, its endpoint chord is a line.

The standard extended and legacy 146-byte linked profile-point record has kind u32 `0`, `1`, or `2`. It has profile locus `04 00 02 00` or geometry locus `05 00 01 00`, role u16 `1`, zero state u16 at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, a coordinate tag at marker +56, and finite f64 coordinates at marker +58 and +66. The extended prefix uses coordinate tag `1e 00`; the legacy prefix uses `1a 00` or `1e 00`. Marker +74 is zero and marker +76 is u16 `2` or `3`. Two eight-byte incidence cells at marker +78 and +86 each store a nonzero selector tag, a u16 curve identifier, and u32 `-1`. The `(selector, identifier)` pairs are distinct; an identifier may be zero or may occur under two different selectors. The link terminator `00 00 fe ff ff ff` begins at marker +94. The 146-byte trailer contains zero bytes through marker +133, state u16 `0` or `1` at marker +134, six zero bytes, and one non-null u32 identity at marker +142; 38 zero bytes followed by distinct nonzero, non-null u32 identities at marker +138 and +142; or 36 zero bytes, a nonzero, non-null u32 identity at marker +136, zero u16 at marker +140, and a distinct nonzero, non-null u32 identity at marker +142. A terminal form requires zero state at marker +74, link count u16 `2`, zero bytes from marker +100 through +141, and null u32 identity at marker +142. The next sketch marker begins at marker +146. A 150-byte continuation form requires zero state u16 at marker +74 and link count u16 `2` at marker +76. It has 36 zero bytes at marker +100, a nonzero, non-null u32 identity at marker +136, zero u16 at marker +140, u32 state `1` at marker +142, and the following marker's nonzero, non-null object index at marker +146; the next sketch marker begins at marker +150. Both link-state values and every complete identity trailer define a point.

A scaled extended-prefix 146-byte linked profile-point record has native value u32 `0`, profile locus `04 00 02 00`, role u16 `1`, zero state u16 at marker +29, selector `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite f64 coordinates at marker +58 and +66. Marker +74 is zero. Marker +76 stores a link count equal to twice the u16 trailer state at marker +134; the trailer state is at least `2`. Two distinct eight-byte incidence cells at marker +78 and +86 store nonzero selector tags, u16 curve identifiers, and u32 `-1`. The link terminator `00 00 fe ff ff ff` begins at marker +94. Bytes +100 through +133 and +136 through +141 are zero, marker +142 stores a non-null u32 identity, and the next sketch marker begins at marker +146. This form emits a point.

An extended-prefix four-link-state profile point has native value u32 `0`, profile locus `04 00 02 00`, role u16 `1`, zero state u16 at marker +29, selector `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite f64 coordinates at marker +58 and +66. Marker +74 is zero, marker +76 stores link count `4`, and two distinct eight-byte incidence cells at marker +78 and +86 store nonzero selector tags, u16 identifiers, and u32 `-1`. The link terminator `00 00 fe ff ff ff` begins at marker +94. Bytes +100 through +133 are zero and marker +134 stores trailer state `2`. The variable trailer contains a sketch-marker prefix at marker +146, +150, +162, or +174; the +150 form may begin a nested child body. This form emits a point.

The legacy prefix has a 140-byte single-incidence profile-point record with native code and link-state u16 pairs `(0, 2)`, `(1, 2)`, or `(2, 1)`. It stores eight `ff` bytes at marker +5, f32 `-1` at marker +13, native code u32 at marker +17, zero bytes at marker +21 through +22, profile locus `04 00 02 00`, role u16 `1`, zero state u16 at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite f64 coordinates at marker +58 and +66. Marker +74 is zero. One 12-byte incidence cell at marker +78 stores a nonzero selector, a nonzero u16 curve identifier, u32 `-1`, and four zero bytes. The terminator `fe ff ff ff 00 00` begins at marker +90. The terminal trailer stores zero bytes from marker +96 through +135 and a nonzero u32 identity at marker +136; the identity may be null. The paired trailer stores zero bytes from marker +96 through +127, a nonzero, non-null u32 identity at marker +128, four zero bytes at marker +132, and a distinct nonzero, non-null u32 identity at marker +136. The next sketch marker begins at marker +140.

A shared-f32 140-byte profile-point variant has native code `1`, link-state u16 `1`, `2`, or `3`, and the same fixed fields through the link terminator. Its incidence selector is nonzero and non-null; its u16 curve identifier may be zero but is not `ffff`. A terminal trailer has zero bytes from marker +96 through +135 and a nonzero, non-null u32 identity at marker +136. A shifted paired trailer has zero bytes from marker +96 through +131, a nonzero, non-null u32 identity at marker +132, and a distinct nonzero, non-null u32 identity at marker +136. A paired trailer can also use the marker +128 identity and zero bytes at marker +132 through +135. The next sketch marker begins at marker +140. These complete identity trailers define a point.

A legacy-prefix 144-byte shared-f32 single-incidence profile point has native code `1`, link-state u16 `1`, `2`, or `3`, and the same fixed fields through its incidence cell. Its incidence selector is nonzero and non-null; its u16 curve identifier may be zero but is not `ffff`. Four zero bytes occupy marker +90 through +93. The terminator `fe ff ff ff 00 00` begins at marker +94. The trailer stores zero bytes from marker +100 through +139 and a nonzero, non-null u32 identity at marker +140. The next sketch marker begins at marker +144. This complete identity trailer defines a point.

The extended prefix has a terminal 180-byte profile-point record with kind u32 `2`, profile locus `04 00 02 00`, role u16 `1`, zero state u16 at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite f64 coordinates at marker +58 and +66. Marker +74 stores u32 `1`, marker +78 through +83 are zero, marker +84 stores i32 `-2`, and marker +88 through +173 are zero. Marker +174 stores a nonzero, non-null u16 identity, marker +176 stores a nonzero, non-null u16 selector, and marker +178 is zero.

The compact generation's selected-axis line has value `0`, role u16 `2`, `0c` followed by seven zero bytes at marker +31, endpoint u16 values at marker +42 and +44, f64 `-1` at marker +50, zero u32 at marker +58, and the next sketch marker at marker +80. Its header bytes at marker +5 are `ff ff ff ff 04 00 ff ff`. Adding one to each endpoint value gives the object index of a coordinate-bearing endpoint marker owned by the profile object.

The compact `moCompRefPlane_c` reference record is 67 bytes. It stores a nonzero, non-null u32 identity at record +0, six zero bytes at record +4, the referenced plane's u16 feature-object identifier at record +10, 27 zero bytes at record +12, f64 `1` at record +39, and the 16-byte trailer `00 00 00 04 00 00 00 f9 ff ff ff 00 00 00 00 65` at record +47. The referenced identifier resolves against feature-object name identifiers in the same feature-input lane.

A current- or extended-prefix coordinate marker with value u32 `1` at marker +17, profile locus `04 00 02 00` at marker +23, role u16 `1` at marker +27, and `1e 00` at marker +56 is a profile vertex. Its coordinates are the f64 pair at marker +58 and +66 even when the record contains an inline line-handle declaration.

A current-prefix marker with value u32 `2` at marker +17, geometry locus `05 00 01 00` at marker +23, and role u16 `1` at marker +27 is a profile line. Its neutral geometry is the chord between its two linked or indexed point markers. This locus rule applies to coordinate-bearing and indexed record layouts. A current-prefix value-2 marker with profile locus `04 00 02 00` is instead a bounded arc.

An extended-prefix 134-byte value-2 profile-locus record carries `1e 00` at marker +56 and a finite planar center pair at marker +58 and +66. Marker +74 and marker +76 select either direct form `(1, 0)` or counted form `(0, 1..3)`. Marker +78 through +81 are zero. Marker +82 is zero in direct form and zero or one in counted form. Marker +84 stores i32 `-2`, marker +88 through +129 are zero, marker +130 stores the object index, and the next sketch marker begins at marker +134. When the next two coordinate-bearing markers owned by the same feature object are distinct and their midpoint equals the stored center, the record is the line chord between those markers. A non-midpoint center remains a native curve handle.

An extended-prefix kind `0` bounded curve in the 104-byte compact indexed layout carries a circular arc. The profile-locus form carries selector `00 00 04 00` at marker +35. The geometry-locus form carries selector `00 00 05 00` at marker +35. Both forms store zero-based coordinate-roster endpoint indices at marker +56 and +58; one less than the smaller endpoint index selects the center. The selector identifies the complete coordinate-roster entry at that zero-based position. If that entry is not equidistant from the two distinct endpoints, the selector identifies the unique coordinate-bearing marker whose object index has the same value. If that indexed center is absent, exactly one coordinate-bearing geometry marker in the feature object must be equidistant from both endpoints. When more than one record-specific roster or index-base interpretation is structurally admissible, every successful interpretation must identify the same ordered endpoint pair and center. A conflicting interpretation leaves the curve unresolved. The angle order represents the minor arc between the endpoints. An absent or ambiguous center leaves the curve unresolved.

An extended-prefix kind `0` profile-locus terminal 102-byte bounded-arc record has the same header, endpoint indices, arc selectors, and reference sentinels as the 104-byte compact indexed arc. Marker +94 through marker +101 are zero bytes, and no sketch marker follows at marker +102. It uses the same coordinate-roster, object-index, and unique-equidistant-center resolution rules as the 104-byte form. A distinct endpoint pair with no admissible center remains unresolved.

A second extended-prefix kind `0` geometry-locus 104-byte bounded-arc form carries selector bytes `00 00 80 bf 00 00 04 00` at marker +31, role and state u16 values `1` at marker +27 and +29, f64 `1` at marker +48, u32 `1` at marker +60, f64 `-1` at marker +64, signed selector `1` or `-1` at marker +72, and a u16 center index at marker +76. Its endpoint indices at marker +56 and +58 are zero-based positions in the complete coordinate-bearing geometry roster. The marker +76 value selects the center directly from that roster. The selected center must be equidistant from two distinct endpoints. If the direct roster entry does not resolve, the same feature-local object-index and unique-equidistant-center fallbacks apply. When more than one record-specific roster or index-base interpretation is structurally admissible, every successful interpretation must identify the same ordered endpoint pair and center. A conflicting interpretation leaves the arc unresolved. The two u32 trailer identities are at marker +96 and +100, and the next sketch marker begins at marker +104. Equal endpoint indices use the full-circle rule below; distinct endpoint indices define the minor arc.

The same geometry-locus arc header has a 116-byte relation-tail form. It stores zero bytes at marker +94 through +101, u32 `4` at marker +102, zero bytes at marker +106 through +111, a nonzero, non-null u32 object index at marker +112, and the next sketch marker begins at marker +116. Its endpoint and center indices use the same complete coordinate-bearing geometry roster as the 104-byte form.

An equal-index geometry-locus circle uses a terminal dimension tail when marker +94 through +127 are zero, marker +128 stores u16 `4`, marker +130 stores a nonzero, non-null u32 reference, marker +134 stores zero u16, marker +136 stores a nonzero, non-null u32 value, marker +140 through +147 stores `ff fe ff 02 44 00 31 00`, marker +148 stores f64 `2`, and marker +156 through +159 are `ff`. Marker +56 and marker +58 contain the same nonzero one-based index in the complete coordinate-bearing geometry roster. That index selects the radial point, and the preceding roster entry is the center. Marker +76 stores u16 auxiliary index.

An extended-prefix value-1 compact indexed profile-locus or geometry-locus bounded curve that ends at marker +84, marker +96, or marker +104 carries the straight chord between its two distinct endpoints when no complete role-2 tangent record follows it. A profile-locus chord can carry selector `00 00 45 00` at marker +35 instead of selector `00 00 04 00`. The value-0 marker +104 form is the circular-arc layout defined above.

A current-prefix compact indexed profile-locus bounded curve that ends at marker +84, marker +96, or marker +104 carries the straight chord between its two distinct endpoints when no complete role-2 tangent record follows it. Equal endpoint indices do not carry a chord.

An extended-prefix kind `0` geometry-locus record in the 104-byte compact indexed layout carries a full circle when marker +56 and marker +58 contain the same nonzero one-based point-roster index. Its selector bytes at marker +31 are `00 00 80 bf 00 00 04 00`; its role and state u16 values are `1`; marker +60 stores u32 `1`; marker +64 stores f64 `-1`; marker +72 stores signed selector `1` or `-1`; marker +78 contains four i32 `-2` cells; and marker +94 is zero u16. The indexed point is the radial witness, and the immediately preceding point in the feature-local point roster is the center. Their positive distance is the radius.

A current-prefix kind `2` profile-locus record in the same 104-byte compact indexed layout carries a full circle when marker +56 and marker +58 contain the same nonzero one-based coordinate-roster index. The indexed coordinate is the center, and the immediately following coordinate is the radial witness. Their positive distance is the radius.

A legacy-prefix 96-byte profile-locus record with native value `1`, state u16 `1`, and distinct nonzero, non-null identities at marker +88 and +92 first resolves marker +56 and +58 after adding one to each value as point object identifiers. If that pair does not resolve two coordinate-bearing point markers, the raw values are one-based positions in the complete feature-local sketch-marker roster in marker order. Both addressed entries must carry coordinates, and their chord is the profile line.

A compact-legacy kind `2` bounded curve with locus `05 00 01 00` and the compact indexed endpoint layout is a semicircle when exactly one feature-local point marker lies at the endpoint midpoint. That point is the circle center and the endpoint chord is the diameter.

When a compact indexed curve record admits more than one record-specific
roster or index-base interpretation, each interpretation resolves its stored
pair through the owning feature's object-indexed, coordinate-bearing marker
roster. If exactly one interpretation resolves two distinct markers, that pair
is selected. If multiple interpretations resolve complete pairs, their endpoint
coordinates must match as an unordered pair; coincident alternatives use the
stable marker identity order. Different coordinate pairs leave the indexed
interpretation unresolved, and no raw index-base reading is selected.

The current, compact legacy, and extended marker prefixes have a solved curve or arc with role u16 `1` at marker +27 that uses a compact indexed record. Its locus is `05 00 01 00` or the profile locus `04 00 02 00`. It stores `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and two little-endian u16 endpoint indices at marker +56 and +58. Values `0` and `1` at marker +17 denote curves; value `2` denotes a bounded curve. For current-prefix records and compact-legacy records with locus `05 00 01 00`, adding one to each endpoint index first produces the object index of a coordinate-bearing point marker owned by the same feature object. A current-prefix profile-locus record with distinct nonzero, non-null identities in its 84-byte, 96-byte, or 104-byte ending uses one-based positions in the complete feature-local sketch-marker roster in marker order; both addressed entries must carry coordinates. A value-1 or value-2 84-byte record whose addressed complete-roster entries do not both carry coordinates instead uses zero-based ordinals in the feature-owned coordinate-bearing point roster in marker order. When another current-prefix 84-byte, 96-byte, or 104-byte record does not resolve two distinct point object indices, raw fields that directly select two distinct point object indices are zero-based object identifiers; otherwise the raw fields are zero-based ordinals in the feature-owned coordinate-bearing point roster in marker order. A compact legacy profile-locus record uses either zero-based ordinals in all coordinate-bearing geometry markers owned by the feature object, ordered by marker offset, or direct nonzero object indices when both fields uniquely select coordinate-bearing point markers. An extended-prefix 84-byte, 104-byte, or terminal 102-byte profile-locus record first adds one to each endpoint index and resolves the resulting point object indices. If that pair does not resolve, each raw field is a direct point object identifier; identifier zero selects the unique coordinate-bearing point without an explicit object identifier. If that pair also does not resolve, both fields are direct ordinals in the complete feature-local sketch-marker roster in marker order; both addressed entries must be coordinate-bearing point markers. In an extended-prefix 96-byte record, adding one to each endpoint index produces the object index of a coordinate-bearing point marker owned by the same feature object. A current-prefix value-2 profile-locus 84-byte record is a bounded arc when exactly one coordinate-bearing marker owned by the feature object is equidistant from its two distinct endpoints; the selected marker is the center. A complete following role-2 detail record supplies the bounded curve's unit tangent and distinguishes a circular arc from a straight line. Without that detail record, a current-prefix or extended-prefix wide indexed profile-locus bounded curve and a compact legacy profile-locus bounded curve are the straight endpoint chord. Neighbouring curve incidence does not supply a tangent carrier in place of the detail record or a tangent relation. One layout is 84 bytes and ends immediately before the next sketch marker; its little-endian u32 at marker +80 is the object index, not an endpoint identifier. The 96-byte layout stores little-endian u32 selector `1` at marker +60, f64 `-1` at marker +64, zero u32 fields at marker +72 and +76, a zero u16 and state u16 at marker +80, zero u32 at marker +84, and two u32 identifiers at marker +88 and +92. The 104-byte layout stores the same selector and f64, signed selector `1` or `-1` at marker +72, a u16 state at marker +76, four consecutive little-endian i32 `-2` cells at marker +78, zero u16 at marker +94, and two u32 identifiers at marker +96 and +100. The 102-byte terminal layout has the same signed selector, state, and four i32 cells, followed by eight zero bytes at marker +94 and no following sketch marker.

In a compact-legacy profile-locus 104-byte record or terminal 102-byte record, equal coordinate-roster ordinals at marker +56 and +58 identify one radial witness of a complete circle rather than two curve endpoints. Ordinary circles have kind `1` or `2`, role u16 `1` at marker +27, state u16 `1` at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, and selector u32 `1` at marker +60. Their 104-byte form has the compact indexed trailer and a next sketch marker at marker +104; their terminal form has eight zero bytes at marker +94 and ends at marker +102. A construction circle has kind `7`, bytes `ff ff ff ff 04 00 ff ff` at marker +5, role u16 `2`, state u16 `0` at marker +29, `00 00 80 bf 00 00 0c 00` at marker +31, selector u32 `0` at marker +60, signed selector `1` at marker +72, state u16 `8` at marker +76, four i32 `-2` cells at marker +78, zero u16 at marker +94, and the next sketch marker at marker +104. Both forms store f64 `1` at marker +48 and f64 `-1` at marker +64. The ordinal addresses the complete feature-local coordinate-bearing marker roster in marker order, including relation markers with coordinates. The circle center is the unique coordinate position whose distance from the radial witness equals one positive diameter parameter owned by the profile, divided by two. Coordinate markers at the same position are one center candidate. Zero or multiple center-radius pairs leave the record unresolved.

An extended-prefix kind-`1` profile circle uses the same equal-index 104-byte or terminal 102-byte compact trailer. Its locus is `04 00 02 00`; its role, state, selector, sentinel cells, and record endings are the ordinary-circle form above. The first feature-local point marker is the center. The repeated nonzero u16 at marker +56 identifies the radial point by direct object identifier and by one-based position in the feature-local point roster. Exactly one distinct positive-radius locus selected by those interpretations supplies the circle radius; conflicting loci leave the circle unresolved.

An extended-prefix kind-`2` terminal repeated-circle roster has locus `04 00 02 00`, role u16 `1`, state u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, equal nonzero u16 values at marker +56 and +58, selector u32 `1` at marker +60, f64 `-1` at marker +64, signed selector `-1` at marker +72, four i32 `-2` cells at marker +78, and ten zero bytes at marker +94. No sketch marker follows at marker +104. The repeated u16 is the one-based position of the terminal marker in the complete feature-local coordinate-bearing marker roster. Consecutive roster markers form a repeated circle when the first marker's object index is one greater than the second marker's object index and their distance equals half one positive diameter parameter owned by the profile. At least two disjoint pairs must satisfy the same diameter, and the addressed terminal marker must be the radial marker of the final pair. Each pair supplies one circle center and radial witness.

A compact-legacy kind-`1` profile circle has eight zero bytes at marker +56 and repeats one nonzero radial index at marker +64 and +66. Marker +68 stores u32 `1`, marker +72 stores f64 `-1`, marker +80 stores signed selector `1`, marker +84 is zero u16, four i32 `-2` cells begin at marker +86, and marker +102 is zero u16. The identity-bearing form stores the same nonzero, non-null u32 identity at marker +104 and +108 and begins the next sketch marker at marker +112. The terminal form stores 24 zero bytes at marker +104. The first feature-local coordinate-bearing marker is the center. The radial index addresses both the zero-based coordinate-bearing marker roster and its one-based positions; exactly one distinct selected locus may have positive distance from the center. That distance is the radius.

The current, compact legacy, and extended marker prefixes have a solved curve or arc with locus `05 00 01 00` and role u16 `1` that uses a 92-byte record. All three prefixes also use this record with profile locus `04 00 02 00`. It stores `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, endpoint u16 values at marker +64 and +66, `01 00 00 00` at marker +68, and f64 `-1` at marker +72. The compact legacy link-bearing forms store `00 00 80 bf 00 00 44 00` or `00 00 80 bf 00 00 84 00` at marker +31 and have the same endpoint semantics. Values `0` and `1` at marker +17 denote curves; value `2` is not construction geometry. A current-prefix profile-locus record with a nonzero, non-null identity at marker +88 uses one-based ordinals in all coordinate-bearing geometry markers owned by the feature object, ordered by marker offset, unless marker +84 stores u32 `1`; that trailer selects the direct point-object-identifier form. Other current-prefix and extended-prefix profile-locus records use zero-based ordinals in the same roster. A value-2 record is a bounded arc when the endpoints and a unique equidistant center marker define a circle, or when a complete following role-2 detail record supplies a nonparallel start tangent. A detail tangent parallel to the endpoint chord defines a line. Without either arc construction, the profile-locus record is a line between its endpoints. An extended-prefix value-2 geometry-locus record uses the same arc constructions and is otherwise the straight line between its endpoints. For locus `05 00 01 00`, adding one to each endpoint value produces the object index of a coordinate-bearing point marker owned by the same feature object. For a compact legacy record with profile locus `04 00 02 00`, each value is instead a zero-based ordinal in all coordinate-bearing geometry markers owned by the feature object, ordered by marker offset. A current- or extended-prefix value-1 profile-locus record uses direct point object identifiers when the indexed interpretation does not resolve both endpoints. Direct identifier zero selects the unique coordinate-bearing point without an object identifier in the feature object. The endpoint trailer belongs to the curve record and is not a relation-link vector. The next sketch marker begins at marker +92.

A 142-byte geometry-locus coordinate record uses any shared marker prefix and kind u32 `1` or `2`. It has locus `05 00 01 00`, role u16 `1`, zero state u16 at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, eight zero bytes at marker +56, coordinate tag `1e 00` at marker +64, and two finite f64 coordinates at marker +66 and +74. The trailer stores i32 `-2` at marker +92. It then stores either 42 zero bytes at marker +96 and one u32 identity cell at marker +138, or 38 zero bytes at marker +96 and nonzero, non-null u32 identity cells at marker +134 and +138. The next sketch marker begins at marker +142. The record is a point regardless of its kind value.

An extended-prefix 92-byte construction line has value u32 `2`, profile locus `04 00 02 00`, role u16 `2`, zero state at marker +29, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, and eight zero bytes at marker +56. Marker +64 and marker +66 store distinct zero-based ordinals in the complete feature-local sketch-marker roster in marker order. Both addressed markers carry coordinates and define the line endpoints. Marker +68 through marker +71 are zero and marker +72 stores f64 `-1`. State `00 00 00 00` at marker +80 pairs with a nonzero, non-null u32 identity at marker +84; state `00 00 01 00` pairs with zero at marker +84. Marker +88 stores a nonzero, non-null u32 identity in both forms, and the next sketch marker begins at marker +92.

An extended-prefix 92-byte profile-roster construction line has value u32 `2`, profile locus `04 00 02 00`, role u16 `1`, zero state at marker +29, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, and eight zero bytes at marker +56. Marker +64 and marker +66 store distinct zero-based ordinals in the complete feature-local coordinate-bearing marker roster in marker order. The addressed coordinates define the construction-line endpoints. Marker +68 through marker +71 are zero, marker +72 stores f64 `-1`, marker +80 is zero u32, and marker +84 and marker +88 store the same nonzero, non-null identity. The next sketch marker begins at marker +92.

An extended-prefix 84-byte value-2 profile-locus construction line has role u16 `1`, zero state at marker +29, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, zero u32 at marker +60, f64 `-1` at marker +64, and one of `00 00 00 00`, `00 00 01 00`, or `00 00 02 00` at marker +72. Its two endpoint u16 values at marker +56 and marker +58 are zero-based point-object indices; adding one selects the two distinct coordinate-bearing point markers owned by the same feature object. Marker +76 and marker +80 store distinct nonzero, non-null u32 identities. The next sketch marker begins at marker +84. This record is selected construction geometry.

An extended-prefix 84-byte profile-locus construction line has value u32 `2`, role u16 `2`, zero state at marker +29, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, distinct endpoint u16 values at marker +56 and +58, zero u32 at marker +60, and f64 `-1` at marker +64. In the following-marker form, marker +72 stores `00 00 02 00`, marker +76 is zero, marker +80 stores the following marker's nonzero, non-null object index, and marker +84 begins the following marker; the endpoint values are zero-based ordinals in the feature-local coordinate-bearing marker roster. In the terminal form, marker +72 through +84 are zero and no sketch marker begins at marker +84; the endpoint values are one-based positions in the complete feature-local sketch-marker roster. Both addressed markers carry coordinates, and their chord is selected construction geometry.

A compact-legacy 112-byte referenced profile arc has value u32 `0`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and eight zero bytes at marker +56. Marker +64 and marker +66 store distinct zero-based ordinals in the complete feature-local coordinate-bearing marker roster. The coordinate immediately preceding the lower endpoint ordinal is the arc center. Marker +68 stores u32 `1`, marker +72 stores f64 `-1`, marker +80 stores i32 `1`, marker +84 is zero u16, marker +86 contains four i32 `-2` cells, marker +102 is zero u16, and marker +104 and marker +108 store the same nonzero, non-null identity. The next sketch marker begins at marker +112.

A current-prefix 104-byte indexed profile line has value u32 `2`, geometry locus `05 00 01 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and eight zero bytes at marker +56. Marker +64 and marker +66 store distinct zero-based ordinals in the complete feature-local coordinate-bearing marker roster. Marker +68 stores u32 `1`, marker +72 stores f64 `-1`, marker +80 through marker +87 are zero, marker +88 stores `00 00 01 00`, marker +92 and marker +96 are zero u32, and marker +100 stores u32 `1`. The next sketch marker begins at marker +104.

A current-prefix 104-byte compact profile line has value u32 `2`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, and f64 `1` at marker +48. Marker +56 and marker +58 store distinct endpoint indices, marker +60 stores u32 `1`, marker +64 stores f64 `-1`, marker +72 stores u32 `1`, marker +76 is zero u16, marker +78 contains four i32 `-2` cells, and marker +94 is zero u16. Marker +96 and marker +100 store the same nonzero, non-null identity. The next sketch marker begins at marker +104.

A current-prefix referenced profile curve has value u32 `1` or `2` and distinct endpoint values. Its 84-byte form stores zero u32 at marker +72 and distinct nonzero, non-null identities at marker +76 and marker +80. Its 96-byte form stores ten zero bytes at marker +72, a nonzero state u16 at marker +82, zero u32 at marker +84, and distinct nonzero, non-null identities at marker +88 and marker +92. Its 104-byte form stores signed selector `1` or `-1` at marker +72, a nonzero local state at marker +76, four i32 `-2` cells at marker +78, zero u16 at marker +94, and distinct nonzero, non-null identities at marker +96 and marker +100. Each form ends at the next sketch marker. The endpoint values are one-based positions in the complete feature-local sketch-marker roster in marker order. Both addressed markers carry coordinates and define the curve endpoints. A value-1 or value-2 84-byte form whose one-based complete-roster positions do not both carry coordinates uses the zero-based coordinate-bearing point roster instead. Value `1` carries the line chord. Value `2` carries a bounded arc when a unique equidistant center marker exists and otherwise carries the line chord.

A current- or extended-prefix zero-tail 92-byte profile curve has native value `0`, `1`, or `2`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, eight zero bytes at marker +56, distinct endpoint u16 values at marker +64 and +66, u32 `1` at marker +68, f64 `-1` at marker +72, and twelve zero bytes at marker +80. The 92-byte curve body ends before any following relation payload. Its endpoint values are zero-based positions in the feature-owned coordinate-bearing geometry roster. Both addressed markers must carry coordinates and be distinct; `ffff` is not an endpoint ordinal. Value `1` carries the line chord. Value `2` uses the bounded-arc rules above and otherwise carries the line chord. A missing or out-of-range roster entry leaves the curve native.

A current-prefix 92-byte direct profile line has value u32 `2`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and eight zero bytes at marker +56. Marker +64 and marker +66 store distinct nonzero point-object identifiers, marker +68 stores u32 `1`, marker +72 stores f64 `-1`, marker +80 is zero u32, marker +84 stores u32 `1`, and marker +88 stores a nonzero, non-null identity. The next sketch marker begins at marker +92.

An extended-prefix terminal profile line has value u32 `0`, geometry locus `05 00 01 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, and f64 `1` at marker +48. Marker +56 and marker +58 store the compact indexed endpoints, marker +60 stores u32 `1`, and marker +64 stores f64 `-1`. Marker +72 through marker +141 are zero, marker +142 stores `08 80`, marker +144 through marker +153 are zero, and marker +154 begins the control sequence `01 00 01 00 02 00 00 00 03 00 00 00 02 00 00 00`.

A compact-legacy long profile line has value u32 `0`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +23 and +25, zero at marker +27 through marker +30, u16 `4` at marker +31, and nine zero bytes at marker +33. Marker +42 and marker +44 store distinct nonzero point-object identifiers, marker +46 stores u32 `1`, marker +50 stores f64 `-1`, marker +58 stores u32 `1`, marker +62 stores u16 `7`, marker +64 contains four i32 `-2` cells, marker +80 through marker +119 are zero, and marker +120 stores u32 `16`.

A current-prefix long full circle has value u32 `2`, geometry locus `05 00 01 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and eight zero bytes at marker +56. Marker +64 and marker +66 store the same nonzero coordinate-roster index. Marker +68 stores u32 `1`, marker +72 stores f64 `-1`, marker +80 stores u32 `1`, marker +84 is zero u16, marker +86 contains four i32 `-2` cells, marker +102 through marker +133 are zero, and marker +134 stores u16 `4`. Marker +136 begins `f1 80 00 00 00 00 f3 80 04 80 ff fe ff 02 44 00 31 00`. The first coordinate-bearing point is the center; the indexed coordinate-bearing point is radial.

A compact-legacy 84-byte construction line has value u32 `2`, profile locus `04 00 02 00`, role u16 `2`, zero state at marker +29, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, and distinct nonzero u16 point-object identifiers at marker +56 and marker +58. Marker +60 is zero u32 and marker +64 stores f64 `-1`. State `00 00 01 00` at marker +72 pairs with zero at marker +76 and a nonzero, non-null u32 identity at marker +80. State `00 00 00 00` pairs with the same nonzero, non-null u32 identity at marker +76 and marker +80. The identified coordinate-bearing points define the construction-line endpoints, and the next sketch marker begins at marker +84.

A compact legacy 92-byte kind `0` profile line also uses either locus with `00 00 80 bf 00 00 05 00` at marker +31. It stores f64 `1` at marker +48, zero u32 at marker +60, zero-based coordinate-roster ordinals at marker +64 and +66, a u16 local-link selector at marker +68, reserved zero u16 at marker +70, and f64 `-1` at marker +72. The 84-byte identity-bearing variant instead stores the endpoint ordinals at marker +56 and +58, zero u32 at marker +60, f64 `-1` at marker +64, zero u32 at marker +72, and two distinct nonzero, non-null u32 identities at marker +76 and +80; the next sketch marker begins at marker +84. The coordinate roster and endpoint semantics are the compact legacy profile roster defined above.

A compact legacy linked coordinate marker has kind `0`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, and f64 `1` at marker +48. Marker +56 stores coordinate tag `1a 00`; marker +58 and marker +66 store the finite planar coordinate pair in metres. Marker +74 is zero u16, and marker +76 stores local-link count u16 `2`. The 154-byte variant stores two homogeneous 12-byte cells at marker +78. Each cell stores its selector tag, a distinct nonzero u16 feature-local marker identifier, `ff ff ff ff`, and four zero bytes. Bytes `00 00 fe ff ff ff` terminate the links at marker +102, marker +108 through +149 are zero, marker +150 stores the coordinate marker's object index, and the next sketch marker begins at marker +154. The 146-byte variant omits each cell's four zero bytes, terminates the links at marker +94, stores zero bytes from marker +100 through +137, stores its local identifier at marker +138 and object index at marker +142, and begins the next sketch marker at marker +146. The 162-byte shifted variant stores eight zero bytes at marker +56, coordinate tag `1a 00` at marker +64, and the finite planar coordinate pair at marker +66 and +74. Marker +82 is zero u16, marker +84 stores link count u16 `2`, and two homogeneous 12-byte cells begin at marker +86. The terminator begins at marker +110, marker +116 through +157 are zero, marker +158 stores a nonzero, non-null local identifier, and the next sketch marker begins at marker +162.

A compact-legacy 170-byte profile point may carry an `sgLineHandle` child after its coordinate pair. The marker has value u32 `0`, `1`, or `2`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite planar coordinates at marker +58 and +66 in metres. Marker +74 stores zero u16 followed by handle-state u16 `2` or `3`, `ff ff 01 00 0c 00`, and the 12 bytes `sgLineHandle`. The `(value, handle-state, declaration-tag)` tuple at marker +17, +76, and +96 is `(0, 2, 1)`, `(0, 3, 3)`, `(1, 2, 0)`, `(1, 2, 3)`, `(1, 3, 0)`, `(1, 3, 3)`, `(2, 2, 0)`, or `(2, 2, 3)`. The value-1 `(1, 2, 0)` tuple also permits geometry locus `05 00 01 00`. The declaration tag is followed by `ff ff ff ff 00 00 00 00`. A reference cell at marker +106 has a variable two-byte selector and little-endian u16 identifier, followed by `ff ff ff ff` and four zero bytes. Marker +118 stores `00 00 fe ff ff ff`. The trailer contains 42 zero bytes at marker +124, or 38 zero bytes followed by identical nonzero, non-null u32 identities at marker +162 and +166. The next sketch marker begins at marker +170. The child makes the owning marker a point in every value form.

The value-1 or value-2 compact-legacy point also has a combined handle declaration with profile locus `04 00 02 00` or geometry locus `05 00 01 00`. After `sgLineHandle`, marker +96 stores a non-null u16 line-handle identifier. The 177-byte form stores `ff ff ff ff ff ff 01 00 0b 00` at marker +98, the 11 bytes `sgArcHandle` at marker +108, a distinct non-null u16 arc-handle identifier at marker +119, `ff ff ff ff` at marker +121, and `00 00 fe ff ff ff` at marker +125. Marker +131 through +172 are zero, marker +173 stores a nonzero, non-null u32 identity, and the next sketch marker begins at marker +177. The 185-byte form stores `ff ff ff ff 00 00 00 00 ff ff 01 00 0b 00` at marker +98, `sgArcHandle` at marker +112, the distinct non-null arc-handle identifier at marker +123, `ff ff ff ff 00 00 00 00` at marker +125, and `00 00 fe ff ff ff` at marker +133. Marker +139 through +180 are zero, marker +181 stores the next object's nonzero, non-null u32 identifier, and its sketch marker begins at marker +185. The coordinate pair remains a point handle.

A compact-legacy 169-byte arc-handle point has value u32 `1` or `2`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite planar coordinates at marker +58 and +66 in metres. Marker +74 stores zero u16 followed by handle state u16 `2`. Marker +78 stores a typed 12-byte reference cell with a nonzero, non-null selector and identifier, `ff ff ff ff`, and four zero bytes. Marker +90 stores `ff ff 01 00 0b 00` and the 11 bytes `sgArcHandle`. Its zero u16 identifier is followed by `ff ff ff ff`, four zero bytes, `00 00 fe ff ff ff`, 42 zero bytes, and a nonzero, non-null u32 identity at marker +165. The next sketch marker begins at marker +169. The coordinate pair is a point handle.

A legacy-prefix geometry-locus alternate-tag line-handle point is a 170-byte record with native value `0`, `1`, or `2`, the alternate-tag point header, handle state u16 `2` or `3` at marker +76, `ff ff 01 00 0c 00` and `sgLineHandle` at marker +78 and +84, and a non-`ffff` u16 handle identifier at marker +96. The reference and terminator fields are `ff ff ff ff 00 00 00 00` at marker +98, `ff ff ff ff` at marker +110, and `00 00 fe ff ff ff` at marker +118. Zero bytes from marker +124 through +165 or a complete identity trailer occupy the remainder, and the next sketch marker begins at marker +170. The corresponding 169-byte arc-handle point has native value `1` or `2`, handle state u16 `2` at marker +76, a selector and u16 identifier at marker +78, `ff ff ff ff 00 00 00 00` at marker +82, `ff ff 01 00 0b 00` and `sgArcHandle` at marker +90 and +96, zero u16 at marker +107, `ff ff ff ff 00 00 00 00` at marker +109, `00 00 fe ff ff ff` at marker +117, 42 zero bytes at marker +123, and a nonzero, non-null u32 identity at marker +165. Its next sketch marker begins at marker +169. The alternate coordinate tags are valid in these handle records, and both records emit points.

A current-prefix 167-byte geometry-locus arc-handle point has eight `ff` bytes at marker +5, shared selector `00 00 80 bf` at marker +13, native value u32 `0` at marker +17, zero bytes at marker +21 through +22, geometry locus `05 00 01 00` at marker +23, role u16 `1` at marker +27, zero bytes at marker +29 through +30, selector `00 00 80 bf 00 00 04 00` at marker +31, zero bytes at marker +39 through +47, f64 `1` at marker +48, zero bytes at marker +56 through +63, coordinate tag `1e 00` at marker +64, and finite planar coordinates at marker +66 and +74 in metres. Marker +82 and marker +84 store `02 00`; marker +86 stores class declaration marker `ff ff 01 00`, marker +90 stores u16 class length `11`, and marker +92 stores `sgArcHandle`. Marker +103 stores a u16 handle identifier, marker +105 stores `ff ff ff ff`, marker +109 through +116 are zero, marker +117 stores `fe ff ff ff`, and marker +121 through +162 are zero. The following marker's object index is at marker +163 and its prefix begins at marker +167. A terminal variant inserts four zero bytes at marker +163, stores the following object index at marker +167, and begins the next marker at marker +171. The marker is a point handle. When a declared `sgEntHandle` circular-dimension operand references this exact marker, the point coordinate is the circle center and the driving radius supplies the circle radius; the relation does not require a radial-circle record.

An extended-prefix line-handle point has native code `0` or `2`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite planar coordinates at marker +58 and +66 in metres. The declaring 170-byte form has native code `2` and stores handle state `00 00 02 00` or `00 00 03 00` at marker +74, followed by `ff ff 01 00 0c 00` and `sgLineHandle`. Its null reference cell begins with u16 zero for state `2`, or u16 `1` or `3` for state `3`, followed by `ff ff ff ff` and four zero bytes. One typed reference cell, `00 00 fe ff ff ff`, and 42 zero bytes complete the record before the next marker. The compact declaring 162-byte form has native code `2` and the same handle states and declaration string. Marker +96 stores u16 `0` or `1` for state `2`, or u16 `3` for state `3`, followed by `ff ff ff ff`. Marker +102 stores a nonzero, non-null selector and marker +104 stores its nonzero, non-null u16 identifier, followed by `ff ff ff ff`, `00 00 fe ff ff ff`, 42 zero bytes, and a nonzero, non-null u32 identity at marker +158. The linked 154-byte form pairs native code `2` with state `00 00 02 00` or native code `0` with state `00 00 03 00`. It stores two typed 12-byte reference cells at marker +78 and marker +90 with one nonzero selector and consecutive u16 identifiers, `00 00 fe ff ff ff` at marker +102, 36 zero bytes at marker +108, two zero bytes at marker +148, and a nonzero u32 identity at marker +150 before the next marker. All forms define points.

The compact 162-byte declaration also has two geometry-locus forms with locus `05 00 01 00` and handle state `00 00 02 00`. The compact-legacy prefix pairs native code `2` with declaration tag u16 `0`. The extended prefix pairs native code `1` with declaration tag u16 `12`. Their coordinate, declaration, typed-reference, identity, and next-marker fields otherwise match the profile-locus compact declaration. Both forms define points.

A legacy-prefix shifted geometry-locus handle point is a coordinate marker that stores eight zero bytes at marker +56 through +63, a coordinate tag at marker +64, and finite planar coordinates at marker +66 and +74. Tags `12 00`, `13 00`, `16 00`, and `1e 00` are coordinate-bearing forms. For the handle-point form, native code `2` requires the `sgLineHandle` child declaration at marker +86, and native code `1` requires the `sgArcHandle` child declaration at marker +98. The coordinate tag `1e 00` is at marker +64 in that form. The 162-byte and 178-byte line forms use sentinels at marker +112 and +128, respectively; the 177-byte arc form uses a sentinel at marker +127. The following sketch marker begins at the record size. The child declaration is required to classify the `1e 00` marker as a line or arc handle point.

An extended-prefix compact linked line-handle point has native code `0`, `1`, or `2`, header bytes `ff ff ff ff ff ff ff ff`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite planar coordinates at marker +58 and +66 in metres. Marker +74 stores `00 00 02 00`. Marker +78 and marker +86 contain typed eight-byte reference cells with one common nonzero selector and consecutive u16 identifiers, followed by `ff ff ff ff`. Marker +94 stores `00 00 fe ff ff ff`, marker +100 through +141 are zero, and marker +142 stores a non-null u32 identity. The next sketch marker begins at marker +146 or follows a four-byte zero separator at marker +150. The record defines a point.

A current- or extended-prefix 146-byte kind-`2` profile curve with state `00 00 03 00` at marker +74 uses the same compact two-cell body as a direct line. It has profile locus `04 00 02 00`, role u16 `1`, f64 `1` at marker +48, coordinate tag `1e 00` at marker +56, and finite auxiliary coordinates at marker +58 and +66. Two typed eight-byte endpoint cells begin at marker +78 and +86. Each cell contains a selector, a distinct u16 feature-local point object index, and `ff ff ff ff`. The terminator `00 00 fe ff ff ff` begins at marker +94, marker +100 through +141 are zero, marker +142 is a nonzero, non-null identity, and the next sketch prefix begins at marker +146. The referenced coordinate-bearing points are the line endpoints.

A current-prefix 146-byte geometry-locus profile point has native code `2`, locus `05 00 01 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, eight zero bytes, coordinate tag `1e 00` at marker +64, and finite planar coordinates at marker +66 and +74 in metres. Marker +82 stores u32 `1`, marker +86 through +91 are zero, marker +92 stores `fe ff ff ff 00 00`, marker +98 through +131 are zero, marker +132 stores a nonzero, non-null u32 identity, marker +136 through +141 are zero, marker +142 stores `ff ff ff ff`, and the next marker begins at marker +146.

A compact-legacy 146-byte inline arc has value u32 `2`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and eight zero bytes at marker +56. Marker +64 stores `1a 00`. The finite planar center pair is at marker +66 and +74, and the finite start and end pairs are at marker +96/+104 and marker +112/+120, in metres. Marker +86 through +91 are zero, marker +92 stores u16 `1`, marker +128 through +131 and marker +136 through +141 are zero, and the next sketch marker begins at marker +146. The two endpoint pairs have equal nonzero distance from the center. Each pair binds the unique coordinate-bearing point marker in the same feature object with equal coordinates.

A compact-legacy 142-byte profile curve has eight `ff` bytes at marker +5, f32 `-1` at marker +13, native value u32 `2`, profile locus `04 00 02 00`, role u16 `1`, zero bytes at marker +29 through +30, `00 00 80 bf 00 00 04 00` at marker +31, and f64 `1` at marker +48. Marker +56 through +63 are zero. Marker +64 stores `12 00`, `16 00`, or `1a 00`; the finite auxiliary pair is at marker +66 and +74. Marker +82 stores u32 `11`, marker +86 through +91 are zero, and marker +92 stores an opaque variant u32. The finite start and end pairs are at marker +96/+104 and marker +112/+120. Marker +128 through +137 are zero, marker +138 stores a nonzero, non-null feature-local identity, and the next sketch marker begins at marker +142 or after an uninterpreted four-byte separator at marker +142 through +145. When the auxiliary pair has equal positive distance from both distinct endpoints, it is the arc center and the record emits the minor arc. Otherwise the record emits the straight line between the endpoints. Each endpoint pair binds the unique coordinate-bearing point marker in the same feature object with equal coordinates.

A compact-legacy 138-byte inline corner arc has value u32 `2`, profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and tag `16 00` at marker +56. The finite planar corner pair is at marker +58 and +66. Marker +74 stores u16 `11`, marker +76 through +83 are zero, and marker +84 stores u32 `9`. The finite start and end pairs are at marker +88/+96 and marker +104/+112, in metres. Marker +120 through +123 and marker +132 through +133 are zero, marker +128 stores u32 `2`, marker +134 stores a non-null object index, and the next sketch marker begins at marker +138. The corner combines one coordinate from each endpoint; the opposite rectangle corner is the circle center. Exactly one of the two coordinate pairings must equal the stored corner, and the endpoints must be distinct and equidistant from the opposite corner. Each endpoint pair binds the unique coordinate-bearing point marker in the same feature object with equal coordinates.

A geometry-locus 138-byte inline arc has locus `05 00 01 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, u16 `11` at marker +74, eight zero bytes at marker +76, u32 `1` at marker +84, and finite start and end pairs at marker +88/+96 and marker +104/+112. Marker +120 is zero u32, marker +124 stores a nonzero, non-null identity, marker +132 is zero u16, marker +134 stores the following marker's nonzero, non-null object identifier, and the next sketch marker begins at marker +138. The extended-prefix form has value u32 `2`, tag `12 00` at marker +56, its finite center pair at marker +58 and +66, and `00 00 02 00` at marker +128. The legacy-prefix form has value u32 `1`, tag `1a 00` at marker +56, a finite corner pair at marker +58 and +66, and u32 `1` at marker +128. The legacy corner combines one coordinate from each endpoint; the opposite rectangle corner is the center. The endpoints are distinct and equidistant from the center.

The extended prefix also has a 134-byte geometry-locus inline arc with value u32 `2`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, tag `1a 00` at marker +56, and its finite center pair at marker +58 and +66. Marker +74 stores u16 `11`, marker +76 through +87 are zero, and the finite start and end pairs are at marker +88/+96 and marker +104/+112. Marker +120 through +129 are zero, marker +130 stores the following marker's nonzero, non-null object identifier, and the next sketch marker begins at marker +134. The endpoints are distinct and equidistant from the center.

A packed compact-legacy 126-byte inline arc has value u32 `2`, profile locus `04 00 02 00`, role u16 `1`, `04 00 00 00` at marker +29, f64 `1` at marker +40, and tag `12 00` or `16 00` at marker +48. Its finite planar center, start, and end pairs begin at marker +50, +80, and +96, in metres. Marker +66 stores u16 `11`, marker +68 through +75 are zero, marker +76 stores a nonzero, non-null u32 identity, marker +112 through +121 are zero, and marker +122 stores a nonzero, non-null u32 object index. The next sketch marker begins at marker +126. The endpoints are distinct and have equal nonzero distance from the center.

When a revolution has no separate axis reference, one current-prefix compact or 92-byte indexed curve with value `2` at marker +17 carries the selected axis as its endpoint chord. An indexed curve with value `0` or `2` carries the selected chord when it is immediately followed by a complete role-2 curve-detail record. In both forms, every endpoint-owned point marker in the profile object lies on one side of the infinite line through the chord. Unreferenced coordinate handles do not participate in this bound. The curve remains profile geometry; the chord line is the revolution axis. A detailed arc therefore contributes its diameter, not its tangent, as the axis.

The extended-prefix horizontal-axis form is a 92-byte record with value u32 `4` at marker +17, profile locus `04 00 02 00` at marker +23, role u16 `2` and zero u16 at marker +27, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, zero through marker +63, endpoint u16 values at marker +64 and +66, zero u32 at marker +68, f64 `-1` at marker +72, and zero u32 at marker +80. Adding one to each endpoint value gives the object index of an endpoint point marker owned by the profile object. The endpoint chord is the selected revolution axis.

The compact-legacy kind-`1` selected-axis form is a 92-byte record with profile locus `04 00 02 00`, role u16 `2`, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, and zero through marker +63. Marker +64 and +66 store distinct zero-based ordinals in the complete feature-local coordinate-bearing marker roster, marker +68 is zero u32, and marker +72 is f64 `-1`. Marker +80 stores state u16 `0` or `2` followed by zero u16, marker +84 is zero u32, marker +88 is a nonzero, non-null u32 identity, and the next sketch marker begins at marker +92. The placed endpoint chord is construction geometry and the selected revolution axis.

The current-prefix vertical-axis form is an 84-byte record with value u32 `5` at marker +17, profile locus `04 00 02 00` at marker +23, role u16 `2` and zero u16 at marker +27, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, endpoint u16 values at marker +56 and +58, zero u32 at marker +60, f64 `-1` at marker +64, and zero u32 at marker +72. Adding one to each endpoint value gives the object index of an endpoint point marker owned by the profile object. The endpoint chord is the selected revolution axis.

A compact-legacy-prefix 84-byte profile line has value u32 `2`, profile locus `04 00 02 00`, role u16 `1`, zero state u16 at marker +29, `00 00 80 bf 00 00 08 00 58 00` at marker +31, seven zero bytes at marker +41, f64 `1` at marker +48, zero u32 at marker +60, f64 `-1` at marker +64, and the next sketch marker at marker +84. The extended-prefix form has the same fields and uses u16 tag `40 00` or `58 00` at marker +39. The distinct u16 values at marker +56 and +58 are zero-based ordinals in the feature-local coordinate-bearing sketch-marker roster in marker order. Marker +72 stores zero u16 and state u16 `0` or `2`; marker +76 is a non-sentinel trailer and marker +80 is a nonzero, non-sentinel identity. The endpoint chord is profile geometry.

An extended-prefix 84-byte selected construction line has value u32 `2`, profile locus `04 00 02 00`, role u16 `2`, zero state u16 at marker +29, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, zero u32 at marker +60, f64 `-1` at marker +64, and the next sketch marker at marker +84. In the terminal roster form, marker +72 stores zero u16 and state u16 `2`, marker +76 is zero u32, and marker +80 is u32 `-1`. Adding one to each distinct u16 value at marker +56 and +58 first selects endpoint object indices. If that pair does not resolve, the values are one-based ordinals in the complete feature-local sketch-marker roster in marker order; both addressed markers must carry coordinates. In the direct-trailer form, marker +56 and +58 store u16 `0` and `1`, marker +72 stores zero u16 and state u16 `0` or `1`, and marker +76 and +80 store distinct nonzero u32 endpoint object identifiers. The endpoint chord is selected construction geometry.

The extended-prefix 92-byte selected construction line has the same value, locus, role, zero state, body, and f64 state value. Marker +56 through marker +63 are zero. Two distinct u16 endpoint values are at marker +64 and +66, marker +68 is zero u32, marker +72 stores f64 `-1`, marker +80 stores zero u16 and state u16 `2`, marker +84 is zero u32, and the next sketch marker begins at marker +92. Marker +88 is an identity or the null identity. Adding one to each endpoint value first selects unique coordinate-bearing point object indices owned by the feature object. If that pair does not resolve, each nonzero value is a one-based ordinal in the feature-local coordinate-bearing point roster in marker order. The endpoint chord is selected construction geometry.

The current prefix also has an alternate 84-byte indexed-curve roster with geometry locus `05 00 01 00`. A role-1 curve has state u16 `1` at marker +29, `00 00 80 bf` at marker +31, a nonzero four-byte state at marker +35, f64 `1` at marker +48, zero-based endpoint u16 values at marker +56 and +58, u32 `1` at marker +60, f64 `-1` at marker +64, zero u32 at marker +72, two u32 identities at marker +76 and +80, and the next sketch marker at marker +84. Its marker +5 header is either eight `ff` bytes or `ff ff ff ff 00 00 ff ff`. Adding one to each endpoint value gives endpoint object indices.

The selected-axis member of the alternate roster has role u16 `2`, zero state u16 at marker +29, `00 00 80 bf 00 00 0d 00` at marker +31, zero u32 at marker +60, header `ff ff ff ff 04 00 ff ff` at marker +5, and two nonzero u32 identities at marker +76 and +80. A role-2 member with `00 00 80 bf 00 00 0c 00` and null identities is not selected. The selected member's endpoint chord is the revolution axis.

The compact-legacy selected-axis form uses role u16 `2` and endpoint u16 values at marker +64 and +66. Adding one to each endpoint value gives the object index of a coordinate-bearing endpoint marker owned by the profile object. The selected-axis reference establishes point semantics for those endpoints even when an endpoint marker reuses a curve or arc native code.

A compact-legacy 92-byte profile selected-axis record has value u32 `2`, profile locus `04 00 02 00`, role u16 `2`, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, and zero bytes from marker +56 through +63. Marker +64 and +66 are zero-based ordinals in all coordinate-bearing geometry markers owned by the profile object in marker order. Marker +68 is zero u32 and marker +72 is f64 `-1`. The state trailer stores `00 00 01 00` or `00 00 02 00` at marker +80 and zero u32 at marker +84. The identity trailer instead stores zero u32 at marker +80 and the same nonzero, non-null u32 identity at marker +84 and +88. The next sketch marker begins at marker +92. The endpoint chord is selected construction geometry, and both addressed markers are profile points irrespective of their native value.

Another compact legacy selected-axis form is a 92-byte kind `0` record with locus `05 00 01 00`, role u16 `2`, `00 00 80 bf 00 00 0d 00` at marker +31, f64 `1` at marker +48, two zero-based coordinate-roster ordinals at marker +64 and +66, zero u32 at marker +68, and f64 `-1` at marker +72. The coordinate roster contains every coordinate-bearing geometry marker owned by the profile object in marker order. The next sketch marker begins at marker +92.

A legacy-prefix 92-byte selected-axis record has profile locus `04 00 02 00` at marker +23, role u16 `2` and zero state u16 at marker +27, `00 00 80 bf 00 00 0c 00` at marker +31, f64 `1` at marker +48, zero through marker +63, endpoint u16 values at marker +64 and +66, zero u32 at marker +68, and f64 `-1` at marker +72. Value u32 `5` at marker +17 has zero u32 at marker +80 and two distinct nonzero u32 identities at marker +84 and +88. Value u32 `6` at marker +17 has `00 00 02 00` at marker +80, zero u32 at marker +84, and a nonzero u32 identity at marker +88. The next sketch marker begins at marker +92. Adding one to each endpoint value gives endpoint object indices. The endpoint chord is the selected revolution axis. A role-2 value-3 record with endpoint values at marker +56 and +58 is not selected construction geometry.

A legacy wide indexed curve may end at marker +112 instead of marker +92. This trailer stores selector i32 `-1` or `1` at marker +80, a nonzero u16 local state at marker +84, four consecutive i32 `-2` reference sentinels at marker +86, zero u16 at marker +102, and two distinct non-sentinel u32 identities at marker +104 and +108. Its endpoint u16 values remain at marker +64 and +66; adding one gives endpoint object indices. The current-prefix 112-byte profile-line variant has kind u32 `2`, locus `05 00 01 00`, role u16 `1`, selector u16 `1` at marker +29, state bytes `00 00 44 00` at marker +35, eight zero bytes at marker +56, signed selector i32 `-1` at marker +80, and local state u16 `4` at marker +84. It uses the same four `-2` sentinels, zero u16, two distinct identities, and one-based endpoint object-index interpretation.

A legacy-prefix 112-byte profile-roster curve has native kind u32 `0`, `1`, or `2`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, eight zero bytes at marker +56, and zero-based coordinate-roster endpoint ordinals at marker +64 and +66. Marker +68 stores u32 `1`, marker +72 stores f64 `-1`, marker +80 stores selector i32 `-1` or `1`, and marker +84 stores zero u16. Four i32 `-2` reference sentinels occupy marker +86 through +101, marker +102 stores zero u16, and distinct non-sentinel u32 identities occupy marker +104 and +108. The next sketch marker begins at marker +112. The endpoint ordinals address every coordinate-bearing point, constrained point, line-or-circle, and arc marker owned by the same feature object in marker order.

A legacy-prefix 104-byte profile-roster curve has native kind u32 `0`, `1`, or `2`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, eight zero bytes at marker +56, and zero-based coordinate-roster endpoint ordinals at marker +64 and +66. Marker +68 stores u32 `1`, marker +72 stores f64 `-1`, marker +80 through +87 are zero, marker +88 stores the marker local identifier, marker +92 through +95 are zero, marker +96 stores u32 `4`, and marker +100 stores u32 `1`. The next sketch marker begins at marker +104. The endpoint ordinals address the same complete coordinate-bearing geometry roster as the 112-byte form.

A terminal legacy wide indexed curve ends at marker +128. It has profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, zero-based coordinate-roster ordinals at marker +64 and +66, u32 `1` at marker +68, f64 `-1` at marker +72, and 48 zero bytes from marker +80 through +127. Its coordinate roster contains every coordinate-bearing geometry marker owned by the profile object in marker order.

An extended-prefix terminal wide indexed curve has kind u32 `0`, `1`, or `2`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, zero through marker +63, zero-based coordinate-roster ordinals at marker +64 and +66, u32 `1` at marker +68, f64 `-1` at marker +72, and 48 zero bytes from marker +80 through +127. Marker +128 stores the two-byte `0a 00` class-declaration lead-in, and a valid ASCII class declaration begins at marker +130. Its coordinate roster contains every coordinate-bearing geometry marker owned by the profile object in marker order.

An extended-prefix 104-byte wide indexed profile curve has kind u32 `0`, `1`, or `2`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, zero bytes at marker +56 through +63 and +80 through +87, zero-based coordinate-roster endpoint ordinals at marker +64 and +66, u32 `1` at marker +68, f64 `-1` at marker +72, `00 00 01 00` at marker +88 and +92, zero bytes at marker +96 through +99, and u32 `1` at marker +100. The next sketch marker begins at marker +104.

An extended-prefix terminal 164-byte wide indexed profile curve has kind u32 `0`, `1`, or `2`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, zero bytes at marker +56 through +63, zero-based coordinate-roster endpoint ordinals at marker +64 and +66, u32 `1` at marker +68, f64 `-1` at marker +72, zero bytes from marker +80 through +133, u16 `3` at marker +134, eight zero bytes at marker +136, the null u32 identity at marker +144, and 16 zero bytes at marker +148. The record ends at marker +164. Its endpoint fields are zero-based ordinals in the feature-owned coordinate-bearing geometry roster.

An extended-prefix value-2 wide indexed curve in either the 104-byte or 164-byte form is the straight chord between its two endpoints when no unique center or tangent defines a bounded arc.

A terminal legacy-prefix wide indexed line ends at marker +136 immediately before the two-byte `05 00` class-declaration lead-in. It has kind u32 `1`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, zero-based coordinate-roster ordinals at marker +64 and +66, u32 `1` at marker +68, f64 `-1` at marker +72, selector i32 `1` at marker +80, local state u16 `12` at marker +84, four consecutive i32 `-2` reference sentinels at marker +86, and 34 zero bytes from marker +102 through +135. A valid ASCII class declaration begins at marker +138.

An extended-prefix compact indexed curve may end at marker +96. It has kind u32 `0`, `1`, or `2`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, direct point object identifiers at marker +56 and +58, u32 `1` at marker +60, f64 `-1` at marker +64, ten zero bytes at marker +72, a nonzero, non-null u16 state at marker +82, zero u32 at marker +84, a u32 value at marker +88, and u32 `1` at marker +92. The next sketch marker begins at marker +96. Point object identifier zero selects the unique coordinate-bearing point without an object identifier.

An extended-prefix 96-byte selected-axis record has value u32 `2`, profile locus `04 00 02 00`, role u16 `1`, zero state u16 at marker +29, `00 00 80 bf 00 00 0c 00` at marker +31, and f64 `1` at marker +48. Marker +56 and marker +58 store distinct zero-based object indices, marker +60 is zero u32, marker +64 is f64 `-1`, and marker +72 through +81 are zero. Marker +82 stores a nonzero, non-null u16 state, marker +84 is zero u32, marker +88 stores the same value as a u32 identity, marker +92 is u32 `1`, and the next sketch marker begins at marker +96. Adding one to each endpoint value selects two distinct coordinate-bearing markers owned by the same feature object, irrespective of their native geometry kind. Their chord is the selected construction axis.

A compact-legacy 96-byte profile-roster curve has native kind u32 `1`, profile locus `04 00 02 00`, role u16 `1`, zero state u16 at marker +29, and `00 00 80 bf 00 00 0c 00` at marker +31. Its header at marker +5 is eight `ff` bytes or `ff ff ff ff 04 00 ff ff`, and marker +13 stores f32 `-1`. Marker +48 stores f64 `1`; marker +56 and marker +58 store distinct zero-based ordinals in the complete feature-local roster of coordinate-bearing point, constrained-point, line-or-circle, and arc markers; marker +60 is zero u32; marker +64 is f64 `-1`; and marker +72 through +81 are zero. Marker +82 stores a nonzero, non-null u16 state, marker +84 is zero u16, marker +86 is u16 `1`, marker +88 is zero u32, marker +92 is u32 `1`, and the next sketch marker begins at marker +96. Both addressed markers must carry coordinates and be distinct. The endpoint chord is profile geometry.

An extended- or current-prefix compact indexed curve may end at marker +120 before a typed continuation. It has kind u32 `0`, `1`, or `2`, profile locus `04 00 02 00`, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, zero-based coordinate-roster ordinals at marker +56 and +58, u32 `1` at marker +60, f64 `-1` at marker +64, and 48 zero bytes from marker +72 through +119. Marker +120 stores a nonzero, non-null u16 continuation discriminator. The continuation is either a valid ASCII class declaration beginning at marker +122 or a relation header containing a nonzero, non-null u16 selector at marker +122, zero u32 at marker +124, distinct nonzero, non-null u16 selectors at marker +128 and +130, and `ff fe ff 02 44 00 31 00` at marker +132.

A current-prefix terminal relation carrier has kind u32 `2`, geometry locus `05 00 01 00`, role and state u16 values `1` at marker +27 and +29, selector `00 00 80 bf 00 00 04 00` at marker +31, and f64 `1` at marker +48. It stores eight zero bytes at marker +56, `01 00 01 00` at marker +64, u32 `1` at marker +68, f64 `-1` at marker +72, u32 `1` at marker +80, zero u16 at marker +84, four i32 `-2` cells at marker +86, 32 zero bytes at marker +102, and u16 `3` at marker +134. A valid class declaration begins at marker +136. The terminal carrier has no coordinate-bearing or endpoint geometry; its class declaration owns the relation carrier.

A current-prefix compact indexed curve may end at marker +126 before a reference table. It has kind u32 `0`, `1`, or `2`, locus `04 00 02 00` or `05 00 01 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, zero-based point object identifiers at marker +56 and +58, u32 `1` at marker +60, f64 `-1` at marker +64, and 54 zero bytes from marker +72 through +125. Marker +126 stores a nonzero, non-null u16 table count, marker +128 through +135 are zero, marker +136 stores u32 `-1`, and marker +140 through +153 are zero. The first 52-byte table row starts at marker +154 with a nonzero, non-null u32 kind, u32 `2`, zero u32, `fe ff 00 00`, a nonzero, non-null u16 selector, zero u16, u32 `-1`, twelve zero bytes, u32 `-1`, and twelve zero bytes. Adding one to each point identifier gives its object identifier.

A terminal legacy state-2 profile curve has kind u32 `0`, profile locus `04 00 02 00`, role u16 `1`, zero state u16 at marker +29, `00 00 80 bf 00 00 05 00` at marker +31, and f64 `1` at marker +48. The wide 92-byte form has eight zero bytes at marker +56, endpoint u16 identities at marker +64 and +66, zero u32 at marker +68, f64 `-1` at marker +72, `00 00 02 00` at marker +80, and two non-null u32 trailer cells at marker +84 and +88. The compact 84-byte form shifts the endpoint, selector, f64, state, and trailer cells eight bytes earlier. A trailer cell may be zero. The endpoint identities are consecutive. An identity selects the unique point whose local ID equals it or whose object index plus one equals it. The next sketch marker begins at marker +92 or +84.

A legacy unlocated geometry handle has kind u32 `0`, geometry locus `05 00 01 00`, role u16 `1`, zero state u16 at marker +29, `00 00 80 bf 00 00 05 00` at marker +31, and f64 `1` at marker +48. The wide form stores `12 00` at marker +64, 26 zero bytes, `fe ff ff ff` at marker +92, and 42 zero bytes through marker +137; the next sketch marker begins at marker +142 after its four-byte object-index prefix. The compact form shifts these fields and the next marker eight bytes earlier. The handle carries no coordinate, curve, or constraint incidence and defines no neutral sketch geometry.

A compact-legacy kind-1 profile curve in the 112-byte wide layout is a complete circle when its endpoint u16 values at marker +64 and +66 are equal and greater than zero. The common value is a zero-based ordinal in all coordinate-bearing geometry markers owned by the profile object in marker order. It selects a radial witness; the immediately preceding coordinate-roster marker is the center. The distance between those positions is the positive circle radius.

The same complete-circle layout uses extended prefix `ff ff 1f 00 01` with kind u32 `2`. Its referenced 112-byte trailer stores selector i32 `1` at marker +80, a nonzero local state at marker +84, four consecutive i32 `-2` reference sentinels at marker +86, zero u16 at marker +102, two nonzero non-sentinel identities at marker +104 and +108, and the next sketch marker at marker +112. The two trailer identities can be equal. Its terminal form has the same selector and reference sentinels, followed by either 32 zero bytes from marker +102 through +133, the two-byte `04 00` class-declaration lead-in at marker +134, and a valid ASCII class declaration at marker +136, or 26 zero bytes from marker +102 through +127 and a valid ASCII class declaration at marker +128. The terminal local state may be zero. The equal endpoint ordinal is the zero-based radial index in the coordinate-bearing geometry roster for the referenced form and the one-based radial index in all coordinate-bearing markers owned by the profile object for the terminal form. In a terminal form, when that roster interpretation does not provide a preceding center, the equal value is also a direct point object identifier; a unique coordinate-bearing point with that object identifier supplies the radial point. The center coordinate immediately precedes the radial coordinate in every accepted interpretation. Conflicting positive-radius interpretations leave the circle unresolved.

An extended-prefix horizontal relation may carry a boundary-axis candidate in a 112-byte record. It has value u32 `4` at marker +17, profile locus `04 00 02 00` at marker +23, role and state u16 values `1` at marker +27 and +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, zero through marker +63, endpoint u16 values at marker +64 and +66, u32 `1` at marker +68, f64 `-1` at marker +72, u32 `-1` at marker +80, a nonzero u16 state at marker +84, four consecutive i32 `-2` cells at marker +86, zero u16 at marker +102, and two u32 identities at marker +104 and +108. Adding one to each endpoint value gives endpoint object indices. When exactly one such relation chord has every object-indexed profile-curve endpoint on one side of its infinite line, that chord is the selected revolution axis.

The selected axis line may be omitted from the curve roster while its two endpoint markers remain. In this form, exactly two coordinate-bearing point markers in the profile object are not endpoints of any indexed profile curve. Every object-indexed profile-curve endpoint lies on one side of the infinite line through their chord. The two retained points are the revolution axis endpoints.

The selected axis may instead omit its line while retaining one coordinate-bearing marker at the profile origin. Candidate axis lines pass through that origin marker and an object-indexed profile-curve endpoint. Collinear candidates denote one line. Among candidates having every object-indexed profile-curve endpoint on one side, the unique line incident to the greatest number of distinct object-indexed profile-curve endpoints is the revolution axis.

The selected axis and its endpoint markers may both be omitted when the axis is a principal axis of the profile sketch. The horizontal and vertical lines through the profile origin are candidates. A candidate must have every object-indexed profile-curve endpoint on one side and be incident to at least two distinct object-indexed profile-curve endpoints. The candidate with uniquely greatest endpoint incidence is the revolution axis.

An omitted axis may coexist with other unreferenced coordinate handles. In this form, exactly two non-curve endpoint markers store little-endian u32 `1` at marker +76. Every object-indexed profile-curve endpoint lies on one side of the infinite line through the selected pair.

An omitted axis may share its first point with a profile curve. Its sole non-curve endpoint immediately follows that shared coordinate-bearing point in the feature-local marker roster and stores little-endian u32 `1` at marker +76. Every object-indexed profile-curve endpoint lies on one side of the infinite line through the retained pair.

The extended marker prefix `ff ff 1f 00 01` has a profile-vertex layout with type u32 `1` at marker +17, `04 00 02 00` at marker +23, role u16 `1` at marker +27, `1e 00` at marker +56, and vertex coordinates at marker +58 and +66. Coordinate-bearing geometry values use record boundaries at marker +134, +138, +140, or +144. Its 84-byte curve records use the same marker +23 and +27 values, u32 `1` at marker +60, and zero-based endpoint u16 values at marker +56 and +58. Each endpoint value indexes all coordinate-bearing geometry markers owned by the feature object in marker order. A curve followed at marker +84 by a complete coordinate-bearing marker is a line. A curve followed by the extended marker prefix with a non-coordinate curve-detail body uses that detail to distinguish line and arc geometry.

Four extended-prefix 84-byte value-1 profile-locus records define an axis-aligned rectangle when their zero-based endpoint values form one simple four-vertex cycle in the complete feature-local sketch-marker roster. Two coordinate-bearing point markers at opposite cycle vertices determine the rectangle when their coordinates differ on both axes. Three coordinate-bearing point markers determine the rectangle when their three positions occupy distinct members of a two-by-two Cartesian product; the fourth position is the missing product member. Marker +72 is zero u16, marker +74 is state u16 `0` or `2`, and the following record begins at marker +84. Four current-prefix 84-byte kind-1 or kind-2 records use the same body and ending fields with profile locus `04 00 02 00` or geometry locus `05 00 01 00`. Adding one to each raw endpoint value gives its feature-local vertex object identifier. When the four pairs form one simple four-vertex cycle, two identified coordinate-bearing points at opposite vertices determine the rectangle when their coordinates differ on both axes. Three identified coordinate-bearing points determine the rectangle when their three positions occupy distinct members of a two-by-two Cartesian product; the fourth vertex is the missing product member.

Four compact-prefix value-1 role-1 records with state u16 `1`, selector byte `04` at marker +31, endpoint u16 values at marker +42 and +44, u32 `1` at marker +46, and f64 `-1` at marker +50 carry the same rectangle cycle in the point-object namespace. Adding one to each endpoint value gives a vertex object index. The first three records end at the next sketch marker at marker +68. The fourth record can own the relation payload that follows its 68-byte curve body. Exactly four distinct pairs must form one simple four-vertex cycle, and exactly three uniquely identified coordinate-bearing points must occupy distinct members of a two-by-two Cartesian product. The fourth point is the missing product member.

Three current-prefix 92-byte indexed records define an axis-aligned rectangle with one implicit side when two records have native value `1`, one has native value `2`, and their endpoint pairs form one simple four-vertex path. Each record has geometry locus `04 00 02 00` or `05 00 01 00`, role u16 `1`, state u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, two zero-based complete sketch-marker-roster indices at marker +64 and +66, u32 `1` at marker +68, f64 `-1` at marker +72, and the following sketch marker at marker +92. All four addressed markers must be coordinate-bearing points whose positions form one two-by-two Cartesian product. Each stored edge joins two adjacent product members. The two degree-one vertices define the implicit fourth side.

The same extended 84-byte line layout may use geometry locus `05 00 01 00`. A complete sketch marker at marker +84 distinguishes the line from a non-coordinate curve-detail body.

An extended-prefix compact profile curve with native code `1` or `2` and geometry locus `04 00 02 00` may store two zero-based feature-local point object identifiers as u16 values at marker +56 and marker +58. Object identifier zero selects the unique coordinate-bearing point without an explicit object identifier. The pair is valid only when both identifiers resolve uniquely to distinct point markers. This direct-object form may end at a marker at +84 or +104, or at the 102-byte terminal record boundary. The terminal form may store either the compact `-1` or `1` selector and reference sentinel, or 30 zero bytes after the f64 `-1` at marker +64.

A compact indexed curve record whose two endpoint identifiers resolve to a relation marker and another feature-local marker is a relation display carrier. If one object index also identifies non-relation display markers, the unique relation marker owns that endpoint reference. It is not an independent sketch curve. An independent indexed curve requires two distinct point endpoints in the owning feature-local object namespace.

An extended-prefix 104-byte geometry-locus line with native code `0` stores reference tag `1e 00` at marker +56, its first direct nonzero point object identifier at marker +58, u32 `1` at marker +60, f64 `-1` at marker +64, i32 `1` at marker +72, and its second distinct direct nonzero point object identifier at marker +76. Four i32 `-2` reference sentinels occupy marker +78 through +93, marker +94 is zero u16, and the next sketch marker begins at marker +104.

A legacy-prefix 104-byte geometry-locus profile line has native code `0`, role u16 `1`, state u16 `1`, `00 00 80 bf 00 00 05 00` at marker +31, and f64 `1` at marker +48. Marker +56 and marker +58 store distinct zero-based endpoint u16 values; adding one to each value gives the object index of a coordinate-bearing point owned by the same feature object. Marker +60 stores u32 `1`, marker +64 stores f64 `-1`, and marker +72 stores u32 `1`. Marker +76 is zero u16, four i32 `-2` sentinels occupy marker +78 through +93, and marker +94 is zero u16. Marker +96 repeats the line object's nonzero u32 identifier. Marker +100 stores the next object's distinct nonzero u32 identifier, and its sketch marker begins at marker +104.

The same tagged line may terminate before a reference-table envelope instead of a following sketch marker. Marker +94 through +149 are zero, marker +150 is `08 80`, marker +152 through +161 are zero, and marker +162 is `01 00 01 00`. Four strictly descending positive u32 counts begin at marker +166, followed by twelve u32 `1` values at marker +182 and zero u32 at marker +230. The fixed control bytes at marker +234 are `00 00 ff fe ff 00 ff ff 00 00 80 bf ff ff ff ff 01 00 00 00 ff ff ff ff`; marker +258 through +281 are zero, marker +282 is a non-sentinel u32 identity, and marker +286 through +337 are zero. Marker +338 stores u32 `3`, marker +342 stores u32 `1`, marker +346 through +352 are zero, marker +353 stores a non-sentinel u32 identity, marker +357 stores u16 `5`, and a valid ASCII class declaration begins at marker +359.

A 104-byte compact profile-locus full circle uses the extended prefix with native code `1` or the current prefix with native code `1` or `2`. The current kind `1` form is a `LineOrCircle` marker; the current kind `2` form is an `Arc` marker. Equal nonzero u16 values at marker +56 and marker +58 select the radial point. The first coordinate-bearing point marker owned by the feature object is the center. The radial value is interpreted both as a direct point object index and as a one-based index in the coordinate-bearing point roster; exactly one distinct positive-radius position must result. The record has role u16 `1`, u16 `1` at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, u32 `1` at marker +60, f64 `-1` at marker +64, i32 `1` at marker +72, four `fe ff ff ff` sentinels at marker +78, and zero u16 at marker +94. It ends at a sketch marker at +104 or at the 102-byte terminal boundary.

A current-prefix kind `1` profile-locus circle-dimension record has the same fixed header and equal nonzero u16 values at marker +56 and marker +58. Marker +60 stores u32 `1`, marker +64 stores f64 `-1`, marker +72 stores signed selector `1` or `-1`, and marker +78 through marker +93 store four `fe ff ff ff` sentinels. Marker +94 through marker +125 are zero. Marker +126 stores u16 `2` or `4`, followed at marker +128 by the class marker, length `11`, and class name `sgCircleDim`. The repeated index is a one-based ordinal in the feature-local coordinate-bearing point roster. The first point is the center and the indexed point is radial; their finite positive distance is the radius.

An extended-prefix full circle has native code `0`, geometry locus `05 00 01 00`, role u16 `1`, u16 `1` at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, and f64 `1` at marker +48. Equal nonzero u16 values at marker +56 and marker +58 select a zero-based point from the feature-local coordinate point roster; roster point zero is the center and the selected point is radial. Marker +60 stores u32 `1`, marker +64 stores f64 `-1`, marker +72 stores i32 `1`, marker +78 through +93 store four `fe ff ff ff` sentinels, and marker +94 is zero u16. The record ends at a sketch marker at +104 or at the 102-byte terminal boundary. The distance from the center point to the radial point is the circle radius.

An extended-prefix kind `1` geometry-locus 104-byte record carries a full circle when marker +56 and marker +58 contain the same nonzero zero-based radial index. It has the same fixed header, selector, state, endpoint selector, signed-radius selector, reference sentinels, and zero terminator as the 104-byte geometry-locus arc. Marker +72 stores signed selector `-1` or `1`, marker +76 stores the zero-based center index, and marker +96 through +103 are trailer identities. The radial and center indices address the complete feature-local coordinate-bearing geometry roster, whose entries are ordered by marker offset and may be points, constrained points, arcs, or line-or-circle markers. Both addressed entries must carry finite coordinates, and their positive distance is the circle radius. The record ends at a sketch marker at +104.

A compact-legacy 162-byte full-circle record has native code `2`, geometry locus `05 00 01 00`, role u16 `1`, zero u16 at marker +29, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, eight zero bytes at marker +56, coordinate tag `1e 00` at marker +64, and the finite center pair at marker +66 and +74. Marker +82 is zero and marker +84 stores link count u16 `2`. Two typed 12-byte reference cells occupy marker +86 through +109; they share one nonzero selector, carry nonzero u16 indices, and end with `ff ff ff ff` and four zero bytes. The terminator `00 00 fe ff ff ff` occupies marker +110, followed by 42 zero bytes and the next point's u32 object identifier at marker +158. The point record beginning at marker +162 has that object identifier and supplies the radial point. The center-to-point distance is the circle radius.

An `sgSlot_c` declaration may immediately precede a current-, legacy-, or extended-prefix slot record with geometry locus `05 00 01 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, and f64 `1` at marker +48. The current prefix stores six 12-byte typed reference cells from marker +72. The legacy prefix stores six packed 8-byte cells from marker +64. The extended prefix stores either cell form from marker +64. A 12-byte cell stores a nonzero u16 selector, a u16 index, `ff ff ff ff`, and four zero bytes. A packed cell omits the four zero bytes. Packed records are 126 bytes and may repeat consecutively after one declaration. The first, third, and fourth cells have one selector; the second has a distinct selector. Their indices order the four feature-local non-coordinate curve markers around the slot boundary. The fifth and sixth cells have a third selector and index the feature-local coordinate-bearing point roster. Packed indices are selector-scoped and need not form a zero-based permutation. In the 12-byte four-curve form, the boundary indices are one permutation of `0..3`, the center indices select two distinct points, and the ordered boundary contains two lines and two arcs. When one arc is native, the other three curves form one unambiguous partial cycle, and exactly one selected center is not the solved arc's center, the missing arc joins the two open cycle endpoints about that remaining center. The completed boundary is one closed profile.
The slot record is an aggregate boundary descriptor. It does not define an additional sketch curve.

An `sgCircleDim` operand whose explicit marker is an `sgSlot_c` record and whose reference class is `sgEntHandle` uses one `sgSlotHandle` class between that `sgEntHandle` declaration and the slot marker. The class must contain exactly two current 12-byte cells with selector bytes `e7 88`, `ff ff ff ff` at cell +4, and four zero bytes at cell +8. The first cell index equals both the operand entity index and the slot marker local identifier. The second cell index equals the local identifier of exactly one of the two coordinate-bearing points selected by the slot record's fifth and sixth cells. The selected point is the dimension center. A missing class, multiple classes, malformed cells, mismatched slot identity, or an unselected center leaves the relation native. The dimension carries a full construction circle centered at that point; the slot aggregate remains boundary metadata and does not add a sketch curve.

A sketch block is a run of point records followed by the curve records that use them, in stream order, within one feature object; a point record after a curve record, or a point record of another feature object, begins the next block. An indexed curve record has any of the three marker prefixes, locus `04 00 02 00` or `05 00 01 00` at marker +23, role u16 `1` or `2` at marker +27, a u16 at marker +29 that is `1` when the curve is a member of a profile contour and `0` otherwise, `00 00 80 bf 00 00` at marker +31, a flag byte at marker +37, f64 `1` at marker +48, zero bytes at marker +56 through +63, u16 start and end positions at marker +64 and +66, a u32 at marker +68 equal to the u16 at marker +29, and f64 `-1` at marker +72. The u32 at marker +17 of such a record is not an entity kind. Role `2`, or bit 3 of the flag byte, marks construction geometry. The start and end positions are zero-based positions in the block's point roster: every point record of the block in stream order, where a point record is a marker that carries coordinates or an external-reference point. An external-reference point has a serialized object in the slot at marker +35 where a local point stores a null u16; it holds a roster position. A point record with f64 `1` at marker +48 that stores `5e 00`, `1a 00`, or `16 00` at marker +64, where a local point stores `1e 00`, also holds a roster position; the two f64 values after such a tag are not the point's sketch coordinates, and a curve record that names the position is not read by roster position. The `5e 00` records occur as two consecutive records with equal f64 values that precede the first `1e 00` point record of their feature object's block. After the block's first curve record, a marker with a non-null object slot is an external-reference curve and holds no roster position. An indexed arc record additionally stores a sweep sense i32 at marker +80, `1` for counter-clockwise from start to end and `-1` for clockwise, a u16 centre position in the same roster at marker +84, and four consecutive i32 `-2` cells at marker +86. A record without those cells is a line when the u16 at marker +80 is zero. A record without those cells whose i32 at marker +80 is `1` or `-1` is an ellipse arc: that value is its sweep sense, and five u16 positions at marker +84 name the ellipse's centre and the four ends of its two axes in turn, on which its start and end lie. A record without those cells whose u32 at marker +80 is `2` is a spline: the u16 at marker +84 is its order, the degree plus one, and an object of class `cptsSplineList_c` or `modifSplineList_c` follows at marker +86. Its class tag takes one of three forms: `ff ff`, a u16 schema, a u16 name length and the name; `ff 7f` and a u32 class index; or a u16 with its high bit set. A u16 count of point handles follows, each twelve bytes: a u16 class tag with its high bit set, a u16 roster position, and eight bytes. 86 bytes after the last handle the record stores a u32 knot count, that many f64 knots in non-decreasing order, a u32 pole count, that many poles of three f64 each (x and y in the roster's coordinates, then z), and a u32 weight count. An ellipse arc or a spline is not the line between its start and end positions. An ellipse arc defines a neutral ellipse bounded from its start to its end in its sweep sense when its seven positions are local points, its two axes are perpendicular, the third and fourth axis ends are the reflections of the first and second through the centre, and its start and end satisfy the ellipse equation; the longer axis is the major axis, and equal start and end positions denote the full ellipse. An ellipse arc that does not satisfy these defines no neutral curve. A spline defines a neutral non-rational B-spline in sketch coordinates when its knot count is its pole count plus its order, its order is at least `2`, its knots are finite and span a non-empty interval, its weight count is zero, every pole's z is zero, its start and end positions are distinct local points, and its first and last poles are those points; such a spline runs from its start to its end. A spline that does not satisfy these defines no neutral curve: in some records the arrays are followed by a zlib-compressed Parasolid transmit of the curve, and in many of those the arrays are in other coordinates and their end poles are not the record's points. Some records with i32 `1` or `-1` at marker +80 store u16 `0` at marker +88 and `fe ff ff ff` at marker +90; they are not ellipse arcs and do not satisfy the ellipse arc's conditions. An arc whose start and end positions are equal is the full circle through that point. The start and the end of an arc are equidistant from its centre; a block in which an indexed arc record does not satisfy this is not read by roster position. A block stores a contour member a second time with the contour u16 `0`; the two records are one curve. A curve record with bit 0 of the flag byte set has the same body and is not read as a sketch curve: it may name a roster position at or past the block's stored point count, and the positions it names within the roster are mostly point records with the same bit set. A point record carries the flag byte at the same offset; a point with bit 0 set holds its roster position, carries no curve-incidence cell, and remains a sketch point. Ten bytes precede each record of a block: a u16, the record's u32 ordinal in its profile contour, and a u32 identifier; the first record of each array of the block is preceded by the array's u32 count as well.

A detailed curve record is immediately followed by a curve-detail marker of the same generation: at curve +84 for the compact record, curve +92 for the 92-byte wide indexed record, and curve +112 for the 112-byte wide indexed record. The detail stores `ff ff ff ff 04 00 ff ff` at detail +5, repeats the curve's four-byte locus at detail +23, stores role u16 `2` at detail +27, `00 00 80 bf 00 00 0c 00` at detail +31, f64 `1` at detail +48, and a unit 2D start tangent as f64 values at detail +64 and +72. A tangent parallel to the endpoint chord defines a line. Otherwise the tangent and endpoints determine one circle: its center lies on the endpoint normal to the tangent and is equidistant from both endpoints. The tangent is a circle witness and does not select a major-sweep alternative. The bounded arc is the minor sweep between those endpoints.

A profile-locus marker with role u16 `2` and body `00 00 80 bf 00 00 0c 00` is an auxiliary curve record rather than an independent sketch entity. It may supply detail, selection, or display state to another curve. It defines independent neutral geometry only when its complete record matches a construction-axis or construction-circle layout.

Keywords feature attributes that contain object identifiers use the feature's `id` namespace. `DissectableChildren` is a separator-delimited ordered list of child object identifiers. A single sketch child of an extrusion is that extrusion's profile dependency.

A classless Keywords `Extrusion` element is a joined blind solid extrusion. Its sole distinct positive Keywords dimension is the blind depth independently of the dimension name and of additional dimensions recovered from the feature-input stream. Repeated references to that same Keywords dimension do not make the depth ambiguous. When it has no explicit profile attribute or dissected child, the sketch with the greatest positive source identifier smaller than the extrusion's source identifier is its profile; the built-in `moOriginProfileFeature_c` record does not participate.

An extrusion with `DissectableRoot="true"`, no `Profile` attribute, no `DissectableChildren` attribute, and exactly one distinct dimension child uses the profile feature with the greatest source identifier smaller than the extrusion's source identifier; the built-in `moOriginProfileFeature_c` record does not participate. This root form supplies the profile dependency only; it does not replace an explicit or class-derived Boolean operation.

Keywords records bound to `moAlignGroup_c`, `moAttribute_c`, and `moConfigCommentsFolder_c` are metadata objects, not regeneration features. A classless source `-1` `Feature` record with type `Attribute-Definition` and a nonempty name defines an attribute family. A classless source `-1` record whose name prefixes an `moAttribute_c` instance name also defines that attribute family.

A classless Keywords `Note` element whose type is `Note`, whose text is nonempty, and which has no parameters or properties is a standalone semantic text annotation. It is not a regeneration feature. Its element text is the ordered visible text content.

A classless Keywords `Feature` whose ordered dimension children exactly cover its parameters uses the parameter schema as its operation class. A `D2` value with a diameter modifier and no parameter outside `D1` and `D2` is a cosmetic thread. Exactly `D1` and `D2`, with positive distance `D1` and an explicitly unit-suffixed `D2` angle strictly between zero and pi, is a distance-angle chamfer.

An extrusion bound to `moCut_c` has Boolean operation cut independently of its localized Keywords type token. A Keywords type token that normalizes to `CutExtrudeThin` also has Boolean operation cut. Its feature-input scalars named `D5`, `D6`, and `D7` are lengths stored in metres.

Counterbore and drilled-countersink axial profile topology is independent of the sketch origin. All required profile segments must match after one common planar translation; each segment cannot use a different translation.

An explicitly owned generated profile with one diameter dimension and one length dimension defines a simple blind hole from those dimensions. Without explicit ownership, that dimension pair participates in ordered Hole-profile pairing only when at least two sides of its closed axial rectangle are nonconstruction lines and the other sides have both endpoint markers. A completed Hole's transient profile ownership removes that profile from later ordered pairing.

In the legacy repeated-class form, a non-direct feature name carries a u16 class token immediately before its name marker. A token group is a Hole Wizard group when the feature-input lane declares exactly one Hole Wizard class and every uniquely named history feature in the group is immediately followed by two consecutive profile-feature records. The two profiles are the operation's position and generated-profile sketches. When a Hole operation's direct child list contains exactly two typed sketches and exactly one carries a complete axial Hole profile, the other sketch is its position sketch. Child order does not assign these roles. When the operation has no direct child list, it owns the unique generated Hole profile at the next two history ordinals. A Hole operation with a typed position-sketch reference owns the unique generated Hole profile adjacent to that position sketch by source identity. Without an adjacent profile, it owns the sole complete generated Hole profile whose source identity lies strictly between the position-sketch and operation source identities. Without either source-identity form, it owns the unique generated profile adjacent to the position sketch by history ordinal. When those forms yield no profile, the operation owns the sole complete generated Hole profile whose source identity lies strictly between its source identity and the next Hole operation source identity. Source adjacency, bounded position-to-operation identity, and the ordinal or operation-interval forms apply in that precedence order. A generated profile has one Hole owner; an existing owner or multiple equal-precedence claims leave later ownership unresolved. When a history contains the same nonzero number of unowned Holes and unclaimed complete generated profiles, increasing Hole ordinal owns increasing profile ordinal. Unequal cardinalities leave this ownership form unresolved. A missing source identity on the generated profile leaves the native history-record identity as the child identity.

A generated Hole Wizard profile with two diameter-displayed dimensions, two length dimensions, and one angle dimension is threaded when its ordered dimension roles are diameter, length, diameter, length, angle. These fields are the drill diameter, drill depth, nominal thread major diameter, thread depth, and drill-point included angle. The profile is counterbored when the same dimension multiset terminates with a diameter-displayed dimension: the smaller and larger diameters are the drill and counterbore diameters, the smaller and larger lengths are the counterbore and drill depths, and the angle is the drill-point included angle. A profile with two diameter-displayed dimensions, two lengths, and two angles carries a tapered threaded hole. The smaller and larger diameters are the drill and thread-major diameters, the smaller and larger lengths are the thread and drill depths, and the smaller and larger angles are the included thread taper and drill-point angles. Ordered roles length, diameter, angle, length, diameter, diameter carry a counterbored through hole with a countersunk exit. The three increasing diameters are the bore, exit-countersink, and counterbore diameters; the two increasing lengths are the counterbore depth and current through-body span; and the angle is the exit-countersink included angle. The through-body span locates the generated far-side profile and does not change the through-all termination. A generated axial profile with two diameter-displayed dimensions, two lengths, and no angle carries a counterbored through hole when at least two of its entry-wall, radial-step, and bore-wall segments are nonconstruction lines and the remaining segment has both endpoint markers. The smaller and larger diameters are the bore and counterbore diameters, and the smaller and larger lengths are the counterbore depth and current through-body span. The bore wall terminates at that span, 0.000025 mm beyond it, 0.00005 mm beyond it, or 0.001 mm beyond it. A 180° drill angle changes this construction to a blind flat-bottom counterbore and makes the larger length its depth. A generated axial profile with two diameter-displayed dimensions, one length, and one angle carries a countersunk through hole when its nonconstruction lines contain the countersink wall and the adjoining bore wall. The two walls can occupy the same radial half or opposite radial halves of the axial profile. The smaller and larger diameters are the bore and countersink diameters, the angle is the countersink included angle, and the length is the current through-body span. The bore wall terminates at that span, 0.000025 mm beyond it, 0.00005 mm beyond it, or 0.001 mm beyond it. A generated axial profile with two diameter-displayed dimensions, one length, and two angles carries a drilled countersink when at least two of its countersink, bore-wall, and drill-tip segments are nonconstruction lines and the remaining segment has both endpoint markers. The smaller and larger diameters are the bore and countersink diameters, the smaller and larger angles are the countersink and drill-point included angles, and the length is the blind drill depth. A profile with three diameter-displayed dimensions, two lengths, and two angles carries a counterdrilled blind hole. The three increasing diameters are the bore, cylindrical recess, and surface-entry diameters; the two increasing lengths are the recess and drill depths; and the two increasing angles are the entry-transition and drill-point included angles. A profile with one diameter and one length carries a flat-bottomed simple hole with bore diameter and blind depth. Adding one angle changes the bottom to the conical drill point with that included angle. A closed four-line axial profile with one axis segment, entry and terminal radial segments, and one full-depth sloping wall carries a tapered flat-bottomed hole. Its entry diameter is the radial span at axial zero, its blind depth is the axis length, and its included taper angle is twice the arctangent of the absolute radial-span difference divided by the depth.

Keywords element order is serialization order, not regeneration order. Neutral regeneration order is the stable topological order of parent and dependency references; unrelated features retain their serialization order. In a history containing a classless `Extrusion` element with one distinct Keywords dimension, positive source identifiers order otherwise unrelated features before dependency ordering.

Feature-tree node roles are invariant across configurations. A configuration snapshot changes a tree node's suppression and graph state without changing its role.

Late neutral resolution of active-model hole placement and construction fields propagates to the active configuration snapshot only when the corresponding snapshot fields are empty or unresolved. It does not replace resolved configuration-local fields or inactive configuration state.

A named feature-input object bound to a classless history `Sketch` record with a nonzero source identifier is a profile-feature object. It participates in the same object-order ownership rules as an object whose class is `moProfileFeature_c`.

An extrusion feature-input object stores a little-endian u32 form code before its object-name record. A direct class declaration is preceded by the form code and four or eight zero bytes. A repeated-class name is preceded by the form code, four or eight zero bytes, and its little-endian u16 class token. A repeated `moICE_c` object may instead store the form code immediately before its non-sentinel high-bit class token. The decimal `swVersion` value in the `Contents/SolidWorks` metadata selects four-byte padding below `12000` and eight-byte padding at `12000` or above. Missing or invalid `swVersion` does not select a width. A direct `moICE_c` declaration immediately preceded by `ff ff ff ff` is the sentinel cut form.

An extrusion object-name record is followed by four zero bytes, a little-endian u16 family word, a one-byte Boolean operation, one schema byte, the repeated little-endian u32 object identifier, and four zero bytes. The terminated trailer then stores `ff fe ff`. The 40-byte sparse form stores six zero bytes at trailer +16, u16 `1` at +22, a nonzero u16 token at +24, an optional u32 identity at +26, eight zero bytes at +30, and a second nonzero u16 token at +38. The optional identity is zero or non-null. The family word establishes the operation family independently of a shared class token. Family `0x0140` with operation byte `00` joins an ordinary extrusion result. The `moCutThin_c` class is outside this mapping. Family `0x01ca` with operation byte `00` or `02` subtracts it. The zero byte in the `0x01ca` form is not an operation discriminator independent of the family word. Objects without a complete recognized trailer use class-scoped form semantics. `moExtrusion_c` form codes `1`, `4`, and `82` join. `moICE_c` form codes `6`, `21`, and `0x3ee4f8b5` join; codes `0`, `1`, `2`, `5`, `7`, `10`, `14`, `15`, `22993`, and `0xffffffff` subtract. Form code `3` joins for either class. Form code `11` does not determine the Boolean operation.

A compact extrusion end-spec child carries its class either as a lane-scoped class token whose high byte has its high bit set and which is not `ff ff`, or as a direct `ff ff 01 00 0b 00 moEndSpec_c` declaration followed by two zero bytes. The direct form uses the declaration's final `_c` bytes as its anchor; the declaration ends at anchor +2. Header-shaped byte runs with zeros at the class position belong to fillet edge-set records of the `edgeRadiusObject_c` family, not to end specs; those runs carry a class token at anchor +24, the constant `02 02 01 00` at anchor −32, and two f64 `0.5` values ending at anchor −11 and anchor −3. Anchor +2 is zero, anchor +4 and anchor +8 are little-endian Boolean words, anchor +12 is a Boolean direction flag, and anchor +16 is zero. The little-endian u32 first-direction termination code is at anchor +18 and the little-endian u32 second-direction termination code is at anchor +22. A second-direction code `0` means the extrusion travels in one direction only, and code `1` is a through-all second direction. Code `0` is blind, code `1` is through-all, code `2` is through-next, and code `9` is through-all in both directions and always carries second-direction code `1`. A blind child has second-direction code `0` and owns its depth as the display-distance dimension child at anchor +26; the dimension name does not alter its role. When the feature has no Keywords depth, the first following value-only scalar named `D1` or `Depth` in the same object supplies the depth in metres. The compact blind form omits the second-direction word and begins the same dimension child at anchor +22. A single-direction through-all or through-next child has two zero u32 words after the first-direction code, `01 00 00 01`, 56 zero bytes, a control word `00 00 01 00` or little-endian u32 `1`, and six zero bytes. Its follow-on is a u16 object token with two zero bytes or the direct-class prefix `ff ff 01 00`. A third form stores a u16 object token, little-endian u32 `5`, and a complete display-distance dimension child. A single-direction through-all child may instead own a display-distance dimension child immediately at anchor +26; that retained display dimension does not change the termination. The dimensioned traversal form has two zero u32 words after the first-direction code, `01 00 00 01`, ten zero bytes, little-endian u32 `1`, twenty zero bytes, and the display-distance dimension child at anchor +68; its retained dimension likewise does not change the termination. A two-direction child with first-direction code `1` continues after a zero u32 word at +26 with the same `01 00 00 01` run; both directions are through-all. A two-direction child with first-direction code `9` continues at +26 with a display-distance dimension child carrying the retained blind depth, which the through-all-both termination does not consume. A blind child with a through-all second direction carries first-direction code `0`, `1` at +4, second-direction code `1`, and its first-direction depth as the dimension child at +26. End-spec children belong to the extrusion object whose bound feature-name record precedes them; native classes `moExtrusion_c`, `moICE_c`, and `moCut_c` establish that object independently of its Keywords element tag. Repeated feature-name records bound to the same feature are segments of one object. The end-spec header and its reference operands end before the next distinct feature-name record. The extrusion object extends through immediately following profile-feature and `moCosmeticThread_c` child objects to the next non-child feature-name record, but those child objects do not extend the end-spec operand interval.

When operation objects and their dimension children form separate ordered groups, a blind end-spec belongs to the unique extrusion whose sole dimension has the same name and value as the first named scalar following that end-spec. A zero or multiple matching extrusion leaves the end-spec unbound.

Termination code `6` is mid-plane and code `5` is offset-from-face. The child has one zero u32 word after the code and then a display-distance dimension child, either as a direct `moDisplayDistanceDim_c` class declaration or as a repeated lane-scoped class token followed by two zero bytes. The dimension child continues with a 16-byte block. Byte 8 is `0` or `0x40`, byte 9 is a flags byte whose low three bits are clear, and every other byte is zero. The block is followed by `ff ff 00 00`, a mark byte `01` or `03`, `ff ff ff ff`, four zero bytes, and the little-endian f32 `-1.0`. A mid-plane extrusion owns its `Depth` or `D1` scalar as the total travel split evenly around the profile plane. A mid-plane extrusion without a `Depth` or `D1` scalar that owns exactly one length-valued scalar under a user-defined name uses that scalar as the total travel. An offset-from-face extrusion owns its dimension scalar as the offset distance from the terminating face. That face follows later in the same feature interval as the bytes `01 01 00` and an `moSingleFaceRef_w` child, either as a direct class declaration or as repeated lane-scoped class tokens, whose reference body opens with a token followed by `02 00 00 00 40 00 00` and continues with a termination-reference selection vector selecting the face.

A legacy `moSingleFaceRef_w` body starts with two non-null high-bit u16 tokens, u32 value `2`, a one-byte form `0` or `0x40`, two zero bytes, and two copies of one nonzero u32 owner identifier. After zero fill, a control record begins at body +44 or +48 with a non-null high-bit u16 token, u32 value `1`, four zero bytes, a u32 component count, and a component-vector selector whose byte 1 is `02` or `03`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero. One four-byte value and two identical eight-byte witnesses follow. Component entries begin 40 bytes after the control record. Each has a non-null high-bit u16 instance, two zero bytes, a 12-byte type signature, and an optional u32 local identifier; zero gaps of 2, 4, 6, or 8 bytes separate entries. The serialized count is either the component count or exceeds it by two implicit roots. The path terminates with `ff ff ff ff` and four zero bytes, or with 20 or 24 zero bytes followed by a nonzero u32 source identifier.

Termination code `3` is up-to-vertex. The child has two zero u32 words after the code and then a point-reference child at +30 in one of two forms. A point form is declared `ff ff 01 00 0c 00 moPointRef_w` or begins directly with its repeated lane-scoped tokens; its body is a class token, a second token `a9 80` or `2b 80`, u32 `2`, a zero selector byte, and two zero bytes, followed by object identifiers and a termination-reference selection vector whose final counted entry's component identifier is the feature-local vertex identifier. An edge-endpoint form is declared `ff ff 01 00 0f 00 moEndPointRef_w` with a nested `ff ff 01 00 0c 00 moCompEdge_c` child whose body is a class token, u32 `2`, and selector byte `40`; an `moEdgeRef_c` child and a u32 endpoint selector precede the selection vector, which selects the edge carrying the endpoint. The four-byte little-endian word immediately before the duplicated selection-vector marker is retained as the opaque `endpoint_selector`; its endpoint semantics are unresolved.

Termination code `4` is to-face. Token +22 is a Boolean reference-side flag, token +26 is zero, and +30 through +32 are `01 01 00`. The following child is an `moSingleFaceRef_w`, either as a direct class declaration or as a repeated lane-scoped class token. Its body starts with two non-null lane-scoped class tokens followed by `02 00 00 00 40 00 00`. Its selection vector uses the duplicated 16-byte component marker. Marker −12 stores a positive little-endian u32 slot count, marker −8 stores a component-vector selector whose byte 1 is `02` or `03`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero, and marker +16 is zero. The end-spec header and complete single-face child establish to-face termination even when the vector entries remain a native selection. A declared long single-face body places the marker at body +209 in its standard layout. An earlier single-face path form instead repeats a nonzero owner word at body +11 and +15. Its path control begins at body +44, +48, +84, or +88. Bytes between body +19 and the control are zero, or contain one 16-byte `ff` run with zero bytes on both sides. The control has a non-null lane token, a little-endian u32 `1`, four zero bytes, a positive slot count, the same component-vector selector grammar, and two equal eight-byte identity words. Its path entries begin at control +40. Path entries have a four-byte instance cell whose u16 token is non-sentinel and has its high bit set, a 12-byte type signature, and an optional little-endian u32 component identifier. The slot count equals the typed-entry count or includes one or two serialized root slots; a root-adjusted path is complete only when the resulting typed entries end at a terminal slot. A positive u16-range u32 path-slot word may separate entries. Inter-entry padding is zero fill of two, four, or eight bytes, `ff ff ff ff`, `ff ff ff ff 00 00 00 00`, `a0 86 01 00 00 00 00 00`, or `ff ff ff ff` followed by six zero bytes. Signature bytes 4 through 7 are the little-endian native object id of the feature traversed by that path entry; one path may traverse multiple feature identities. The final counted slot may be `ff ff ff ff 00 00 00 00` or a terminal-owner cell containing 20 or 24 zero bytes and the nonzero little-endian u32 native object id of the feature result owning the selected face. The same inter-entry gap grammar may separate the final typed component from this terminal slot. Otherwise the final entry's signature identifies that result. An explicit terminal owner is also a producer of the selected surface even when its identity is absent from the typed component path. The final typed component identifier is the result's feature-local face id. The ordered typed components form the native terminating-face path and are retained as a feature-input surface selection owned by the extrusion. Instance tokens, type signatures, and byte offsets are lane-local. Paths in separate configuration lanes select the same face when their ordered component identifiers, traversed feature identities, and terminal feature identity are equal. The consuming extrusion depends on every uniquely identified history feature traversed by the path. Up-to-vertex and offset-from-face termination-reference vectors share this grammar and may additionally carry a leading identifier-less component cell that repeats the first counted entry's signature, an `a0 86 01 00` filler word, or an `01 00 00 00` slot word between counted entries; their counted slots need not all carry component entries. Each end-spec reference child has one complete component vector position in its owning feature interval. A missing or multiple complete vector leaves the component reference unresolved. Up-to-vertex and offset-from-face terminations are likewise retained as feature-input surface selections, and the up-to-vertex and offset-from-face extrusions depend on every uniquely identified traversed feature.

An extrusion object without an `EndCondition` attribute, without an owned `Depth` or `D1` scalar, and without a decoded compact end-spec termination has an unresolved extent unless it is the classless Keywords `Extrusion` form. An `EndCondition` token outside the defined token set has an unresolved extent. A blind, symmetric, or offset-from-face extent whose length scalar is absent or is not a positive length has an unresolved extent. The class, the retained end-condition token, the retained depth, second-depth, and draft scalars, the retained terminating-face and vertex references, the profile reference, the direction, and the Boolean operation remain independently meaningful.

An extrusion object without `Profile` or `DissectableChildren` has an unresolved profile unless it is the classless Keywords form or has one of the feature-input adjacency forms defined below. A nested profile stream owned by an extrusion resolves the profile to its transferred sketch, when the stream is a sketch profile (§ on `Config-0-ResolvedFeatures` above); a SolidWorks support-face snapshot resolves nothing.

An extrusion immediately followed by an `moProfileFeature_c` object whose `Description` equals its name and whose name ends in `<n>` for decimal `n` uses that following feature as its dissected profile. This child signature applies when the extrusion omits its `Dissectable` property and supersedes an immediately preceding generic profile object. An ordinary following profile without the child signature is not an extrusion operand.

A dissected child with exactly one dependency on a planar sketch feature selects that feature's complete profile when the sketch contains exactly one profile chain. The child is a profile-selection tree record, not another owner of the sketch. Named scalars serialized after this child and before the next feature object belong to the parent extrusion. A sketch with multiple profile chains does not identify which chain the child selects and leaves the child profile unresolved.

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

An offset reference-plane object has a length-valued `D1`. An angular `D1` does not carry offset distance; a reference-plane object with angular `D1` and a complete frame carries that frame as direct datum-plane placement. An offset reference-plane object identifies its source plane with the unique nonzero little-endian u32 source identifier immediately followed by `02 00 00 00 00 05 00 00 00 00 00 00 00 00 00 2d 80 2b 80`. The typed 46-byte form stores the source identifier at +0. A principal-plane target has signature `79 2a e1 3b` at +4, zero u16 at +8, and principal selector u32 `1` or `2` at +10. A constructed-plane target has signature `30 92 ab 53` at +4 and six zero bytes at +8. Both forms store u32 `1` at +14, u32 zero at +18, an identity word at +22, twelve zero bytes at +26, and terminator `c7 cf ff ff c7 cf ff ff` at +38. Exactly one known non-self source across the compact and typed forms identifies the reference. The object may instead carry one complete plane frame whose plane is parallel to exactly one decoded datum frame at the absolute `D1` distance; that datum is the source plane. Coincident or multiply matching frames do not identify a source. The offset plane inherits the source plane normal and u-axis and translates its origin by the signed dimension along the normal. A legacy face-alias reference stores the support-face point, normal, and u-axis as its reference frame. A surface-component face selection stores the resulting plane frame; its support frame is that result frame translated by signed `D1` along the normal. Explicit face-frame records store the support frame directly.

The dual-frame offset layout stores exactly two complete 97-byte fixed frames in the reference-plane object interval. The first frame is the offset result and the second frame is its source plane. Their normals and u-axes have the same direction, their origin displacement has no component tangent to the source plane, and the absolute normal displacement equals `D1`. The second frame identifies exactly one geometrically coincident preceding datum plane or reserved principal plane. A reserved principal plane remains a valid source when its feature-tree record follows the offset-plane record because its native identity denotes a built-in datum.

A component reference-plane record may instead store its nonzero u32 feature source identifier before an opaque u32 identity, six zero bytes, a one-byte value `1`, and a right-handed 3×3 orthonormal f64 basis beginning at offset `+15`. The first, second, and third basis triples are the u-axis, v-axis, and normal. Three f64 values at offsets `+87`, `+95`, and `+103` store the origin in metres. The homogeneous f64 value `1` at offset `+111`, three zero bytes, u32 value `4` at offset `+122`, and `ff ff ff ff` terminate the placement. The source identifier identifies the referenced datum-plane feature; the placement itself supplies the profile frame. This placement is authoritative for a profile attached to a mid-plane datum.

A profile consisting of one full circle also carries a geometric owner signature. Its solved radius equals one radius dimension or half one diameter dimension owned by the corresponding planar sketch feature. When exactly one sketch feature has that radius signature, the signature owns the profile and supersedes interval enclosure. The profile remains interval-bound when the signature has zero or multiple matching sketch features.

Each compact `moDeleteBodyData_c` body-state roster is followed by a `30 80` Boolean field. Value `1` deletes the selected bodies; value `0` retains the selected bodies and deletes the complement.

A sketch marker belongs to the Keywords feature object whose bound feature-name record precedes the marker and whose next bound feature-name record follows it. The little-endian u32 immediately before `ff ff 1f 00 03` is the feature-local object index; `ff ff ff ff` denotes no index. The marker's trailing u32 is its separate local identifier. Object indices and local identifiers are independently scoped to the feature object.

A current-prefix compact profile-locus spatial point has native kind u32 `0` or `1` at marker +17, profile locus `04 00 02 00` at marker +23, profile role u16 `1` at marker +27, selector `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, coordinate tag `0e 00` at marker +56, and three little-endian f64 coordinates at marker +58. Its fixed prefix ends at marker +82. The next sketch marker begins at marker +82, or four separator bytes at marker +82 through +85 precede it at marker +86; the feature-input lane end is also a boundary. A wide spatial point stores the coordinate tag at marker +64 and its coordinates at marker +66. The compact interpretation is not admitted without its boundary.

A current-prefix indexed XYZ spatial point or relation point has the same fixed prefix through xyz coordinates. The relation-point form has native kind `1`, profile locus `04 00 02 00`, profile role `1`, u16 `8` for an ordinary point or `1` for a relation point at marker +82, and u16 `1` for an ordinary point or `0` for a relation point at marker +84. It has six zero bytes at +86 and `fe ff ff ff` at +92. A next sketch marker begins at +158 or +162 after the indexed marker. The `1,0` tail selects the relation-point interpretation; it is not an ordinary geometry point. Its coordinate triplet is exposed as the model-space point carried by the relation marker, while the marker remains a relation handle in the feature-input roster.

Coordinate-bearing marker codes `0`, `1`, `2`, and `3` identify point, line-or-circle, arc, and constrained-point geometry handles. Relation codes `1..27` identify distance, angle, radius, horizontal, vertical, tangent, parallel, perpendicular, coincident, concentric, symmetric, midpoint, intersection, equal, diameter, offset-edge, fixed, the seven quadrant and cardinal arc-angle relations, horizontal-points, vertical-points, and collinear relations in that order. Codes `4..27` retain relation semantics in both coordinate-bearing and reference-bearing layouts. The marker layout disambiguates the reused codes `1..3`.

Coordinate-bearing geometry handles and no-coordinate relation handles reuse feature-local identifiers. A handle reference with one coordinate-bearing candidate selects that geometry handle. With zero or multiple coordinate-bearing candidates, the identifier resolves only when it has one candidate in the complete feature-local marker set.

A compact legacy non-coordinate kind `0` profile line may carry its endpoint incidence as a counted local-link vector. It has profile locus `04 00 02 00`, role u16 `1`, `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, and link count u16 `2` at marker +84. Two homogeneous 12-byte selector cells begin at marker +86. Each cell stores the selector u16, an endpoint local ID u16, `ff ff ff ff`, and four zero bytes. The vector terminator is two zero bytes followed by `fe ff ff ff`. The two local IDs resolve against coordinate-bearing geometry markers owned by the same feature object.

A referenced line-or-circle handle with exactly two incident coordinate-bearing point handles carries the line from the first point in marker order to the second. Point incidence may be stored by a link in either direction. Every admissible sketch placement must produce the same two distinct endpoints. When the endpoint pair identifies one existing profile line under every admissible placement, the handle identifies that line; otherwise it carries a construction line and retains its native identity there. Geometry-handle reachability is the undirected transitive closure of dimensional operands and marker links.

A current or extended profile line-or-circle handle with the geometry-locus tuple, profile role `1`, coordinate tag `1e`, and one forward coordinate-bearing point link carries the line from its own coordinate to that point. A self-link does not count as a forward link. Additional forward links to same-feature relation markers are incidence metadata. A second point link, a forward curve link, an unresolved link, or coincident endpoint coordinates leaves the handle unresolved.

A coordinate-less line-or-circle handle with one linked coordinate-bearing point can complete its line through one radius-relation marker owned by the same feature and carrying the same non-null object index. The radius-relation marker has exactly two distinct linked coordinate-bearing points, and one is the line handle's linked point. The relation's ordered point pair is the line endpoint pair. Zero or multiple matching radius-relation markers leave the line unresolved.

A horizontal, vertical, or fixed relation marker constrains its sole resolved reverse-owner curve when every forward entity is either that curve or a relation-owned construction point. Otherwise, a horizontal or vertical relation constrains the single profile entity common to all of its resolved linked loci. When its two linked markers instead identify two distinct profile loci, it aligns those loci along the corresponding sketch coordinate. A fixed relation marker constrains the single profile entity common to all of its resolved linked loci. The relation remains native when none of these arity forms resolves uniquely.

A recognized relation marker whose resolved operands do not satisfy the relation's typed arity and locus-kind invariants remains a native constraint with its ordered local identifiers and resolved native references.

A parallel, perpendicular, tangent, equal, collinear, or concentric relation marker constrains its two distinct reverse-owner curves when every resolved forward entity is one of those curves or a relation-owned construction point. Otherwise it constrains its two distinct linked profile entities when every link identifies exactly one entity. The relation remains native when neither form resolves exactly two entities.

A coincident relation marker constrains its distinct linked profile loci when every link identifies exactly one locus and at least two loci remain after deduplication. The relation remains native when a link identifies zero or multiple loci.

A horizontal-points or vertical-points relation marker aligns its two distinct linked profile loci along the corresponding sketch coordinate when every link identifies exactly one locus. The relation remains native when a link identifies zero or multiple loci or the resolved locus count is not two.

A compact dimensional relation instance contains one or two adjacent scalar records with the same owning sketch and ordered operand cells. The relation class selected by a driving scalar is authoritative. When a display scalar is the immediately preceding relation scalar before that declaration and the next relation scalar is a driving scalar with the same operands, the display scalar belongs to the driving scalar's instance. Otherwise each scalar uses the nearest preceding compatible declaration before the next relation-class declaration and within the owning feature interval. A third scalar starts another instance even when its operands repeat, except for the repeated circular-dimension form below. A circular-dimension instance may instead contain one display-role scalar with one operand followed immediately by two same-name scalars with the same two operands; their first operand equals the display operand and their second operand is the circle center. A repeated circular-dimension instance contains a contiguous run of at least two display-role scalars. Every scalar has exactly one operand, all operands have the same native tag and distinct feature-local entity indices, and all scalar names resolve to one name. The first scalar's operand is the instance operand. The run has no parameter or unique display scalar field; its first value supplies a reference diameter or radius parameter. A scalar separated by any other scalar record starts another instance. An instance has a parameter scalar only when exactly one member has the driving role and has a display scalar only when exactly one member has the display role. Except for a repeated circular-dimension run, an instance without a parameter scalar does not encode a dimensional constraint.

Non-coordinate sketch-marker type codes `1..85` use the constraint taxonomy. Codes `1..27` are distance, angle, radius, horizontal, vertical, tangent, parallel, perpendicular, coincident, concentric, symmetric, midpoint, at-intersection, equal-size, diameter, offset-edge, fixed, arc-angle-90, arc-angle-180, arc-angle-270, arc-cardinal-top, arc-cardinal-bottom, arc-cardinal-left, arc-cardinal-right, horizontal-points, vertical-points, and collinear. Codes `28..47` are coradial, grid-snap, length-snap, angle-snap, use-edge, ellipse-angle-90, ellipse-angle-180, ellipse-angle-270, ellipse-cardinal-top, ellipse-cardinal-bottom, ellipse-cardinal-left, ellipse-cardinal-right, at-pierce, doubled-distance, merge-points, three-point-angle, arc-length, normal, normal-points, and sketch-offset. Codes `48..67` are along-X, along-Y, along-Z, along-X-points, along-Y-points, along-Z-points, parallel-YZ, parallel-ZX, intersection, patterned, iso-by-point, same-isoparametric, fit-spline, equal-curvature, equal-tangent, tangent-face, along-X-3D, along-Y-3D, along-X-points-3D, and along-Y-points-3D. Codes `68..85` are traction, belt-traction, block-fixed-lock, block-normal-lock, block-rotate-lock, fake-slot, fixed-slot, same-slots, linear-pattern-count, circular-pattern-count, radial-offset, planar-offset, aligned-equal-curvature-3D, flange-face-distance, conic-rho, C3-touch, doubled-angle, and same-curve-length. Codes outside this range are native extensions.

A coradial relation has exactly two circular entities. Their solved centers and radii are equal. Full circles and bounded circular arcs participate with the radius and center of their common supporting circle.

A merge-points relation has two or more point loci at one solved sketch position. It has the same neutral coincidence invariant as a coincident-loci relation; its distinct native code is retained as source identity.

A sketch block in a feature-input lane is a chain of counted arrays of sketch-marker records: points, then lines, then arcs, then other curves. A little-endian u32 at record −14 of the first record of an array gives the record count of the array. Ten bytes precede each record: a u16 external-reference flag at record −10, the u32 contour ordinal at record −8, and a u32 identifier at record −4. An external-reference record (flag `1`) holds a slot in its array as a local record does. A marker record in an array can store `ff ff ff ff 04 00 ff ff` at marker +5 instead of eight `ff` bytes; such a record in a line array is a construction line.

A local point record with coordinate tag `1e 00` at marker +56 or marker +64 stores its planar coordinates as two f64 values after the tag, then four bytes, then the handle cells of the curves that end at the point. A handle cell stores a class reference, the u16 slot of the curve in the array of its kind, `ff ff ff ff`, and four zero bytes. The class reference is a u16 `0x8000 | index`, a u16 `ff 7f` and a u32 index, or `ff ff`, a u16 schema, a u16 length and that many ASCII class-name bytes. The tag of a class is `0x8000` or-ed with the archive index of the class. Ascending tags follow the order of the first declarations of the handle classes.

The relation table of a sketch block follows the last record of the block. Each relation row has this layout:

| Offset | Field |
|---|---|
| +0 | u32 relation code. The codes use the constraint taxonomy of the sketch-marker type codes. |
| +4 | `02 00` |
| +6 | u16 row flag, `0` or `1` |
| +8 | `00 00 fe ff 00 00` |
| +14 | the operand handle cells |

The kind of an operand is the class of its handle tag: `sgPointHandle`, `sgLineHandle`, or `sgArcHandle`. The u16 in the cell is the slot of the operand in the point, line, or arc array of the same block. The solved planar geometry of a flag-`0` row satisfies the relation of its code. An offset-edge row (`16`) between two local operands has two parallel lines or two concentric arcs. A patterned row (`57`) has two lines of equal length or two arcs of equal radius. The meaning of flag `1` is open.

A block's relation rows follow its last record, in any array, and continue to the next sketch-marker record of the lane. A flag-`0` row of codes `4..9`, `11..14` and `25..28` holds a typed neutral constraint when each operand names an entity or locus of the sketch and the row's geometry satisfies the constraint. A point operand names the start, end, or center of a curve that names the point's slot, when that locus lies at the point record's coordinates; otherwise it names the point entity. Tangency between two lines and symmetry of two lines are not typed: SolidWorks holds a symmetric-lines relation on the line carriers, not on their end points. Each other row of a known sketch holds a native constraint `sldprt:sketch-relation:<code>`. The native constraint keeps the row flag and, for each operand, the kind, the slot, the class tag, and the marker reference; an operand that does not resolve has the suffix `:unresolved`. The constraint of a row has the identifier `sldprt:model:sketch-constraint#relation-row:<lane>:<offset>` and no native reference. A row that does not identify one sketch has no constraint.

Within one operand-kind sequence, the bytes that follow a row's handle cells have the same form for every typed code: 28 bytes after one line operand, 68 bytes after two operands.

Ellipse-angle codes `33`, `34`, and `35` constrain a bounded ellipse's positive parameter sweep to π/2, π, and 3π/2 radians. The sweep is `(end − start) mod 2π`; a nonzero whole-turn difference represents 2π. The relation is invalid on a full ellipse or another geometry family.

A named scalar begins with `04 80 ff fe ff`, followed by a u8 UTF-16 code-unit count, that many UTF-16LE code units, a scalar header, and a finite little-endian f64 value. The primary and legacy layouts use the 22-byte header `00 00 00 00 00 00 00 40 ff ff ff ff 00 00 00 00 ff fe ff 00 00 00`. Scalar trailer offsets are relative to the byte immediately after the f64. Trailer +3 stores the little-endian u32 scalar object identifier. In the primary layout trailer +24 stores `00 00 00 02 00`, trailer +29 stores role `0` for driving or `1` for display, and operand cells begin at trailer +35. In the legacy layout trailer +24 stores `0f 00 00 00 02 00`, trailer +30 stores the same role, and operand cells begin at trailer +36. These operand cells repeat every 12 bytes. Each cell stores its little-endian u16 tag at +0, its u16 marker address at +2, `ff ff ff ff` at +4, and four zero bytes at +8. The compact layout uses the 18-byte header `00 00 00 00 00 00 00 40 ff ff ff ff 00 00 00 00 00 00`; trailer +21 stores `01 00 00 00 02 00`, trailer +27 stores the same role, and two adjacent eight-byte operand cells begin at trailer +35. Each compact cell stores its tag and marker address at +0 and +2 and `ff ff ff ff` at +4. A value-only scalar uses the 14-byte header `00 00 00 00 00 00 00 40 ff ff ff ff 00 00`; its trailer does not assign a driving or display role or carry relation operands. The name length therefore moves the value and every trailer field together. An `moIntegerParameter_c` scalar with the 22-byte header stores a little-endian i32 value in four bytes in place of the f64, and its trailer begins immediately after those four bytes. The value is a count. Trailer +0 through +2 are zero and trailer +3 is the nonzero scalar object identifier, which is below 2^24. The value cell is a four-byte i32 exactly when such a trailer occurs at value +4 and does not occur at value +8. An f64 cell whose trailer instead begins with a zero byte and a class token or class declaration does not satisfy the bound at value +4, because the token's high-bit byte is the identifier's high byte. A repeated class token can carry an integer scalar, so the trailer, not a class declaration, establishes the cell width.

When a 12-byte operand cell introduces a lane-local class token, the defining class declaration begins at cell +12. Every reference cell with that tag in the same lane uses the declared class. The declaration is unambiguous only when the tag introduces exactly one class.

The last class declaration before the scalar's linked name supplies the unit when it is `moLengthParameter_c` or `moAngleParameter_c` and no other name record intervenes. Length values use metres and angular values use radians. A sketch-relation family supplies the unit instead when it owns the scalar.

The instance operand list is the first scalar record's complete ordered operand-cell list. Tag `d6 80` uses a zero-based ordinal within its marker family, ordered by marker byte offset in the owning feature object. In a compact profile, its marker family is the feature-owned coordinate-bearing point and constrained-point roster; coordinate-bearing line-or-circle and arc handles do not occupy `d6 80` slots. Tag `e1 80` uses that compatible-family ordinal after its object-index and identifier rules. Circular-dimension tag `fe 83` uses a zero-based curve-handle ordinal. Circular-dimension tags `b6 8a`, `9d 92`, and `69 bd` use a zero-based point or constrained-point ordinal. Tags `7b 83`, `86 83`, `cb 8d`, and `da 8d` first use a feature-local marker identifier qualified by the tag's marker family. For `e1 80` and `86 83`, a unique compatible marker with the same feature-local object index takes precedence over trailer-identifier and ordinal fallback. When no identifier or object-index match selects a marker in the qualified family, the same value is a zero-based ordinal within the compatible marker family in byte order. Multiple identifier matches remain unresolved. Tags `7c bc` and `87 bc` use the precedence defined below.

A `d6 80` point ordinal beyond its applicable ordinal sequence addresses the relation handle with that feature-local identifier. The transitive closure of its local links selects one point or constrained-point terminal. If that terminal is already selected by the paired operand, the uniquely remaining compatible point terminal in the feature object is selected. Multiple unclaimed terminals remain unresolved.

When a qualified curve operand identifier or object-index address selects no coordinate-bearing curve handle, a reference-bearing marker with that address selects the unique line, circle, or arc handle among its resolved local links. A relation marker supplies that line handle only when its two forward links resolve to distinct coordinate-bearing point or constrained-point markers. This indirection precedes compatible-family ordinal fallback. Zero linked curve handles continue to ordinal fallback; multiple linked curve handles remain unresolved.

Operand-cell tags `d6 80`, `cc 80`, `b6 8a`, `cb 8d`, `9d 92`, and `69 bd` address point loci. Tags `7b 83` and `7c bc` address point-qualified geometry handles. Tags `e1 80`, `86 83`, `fe 83`, `da 8d`, and `87 bc` address line-or-circle handles. Tags `cc 80`, `fe 83`, `b6 8a`, `9d 92`, and `69 bd` are used by circular dimensions.

A two-cell dimensional relation scalar whose cells are native tags outside a fixed family signature is a family-scoped pair. The relation family and operand position define each cell's point or curve role; the tag has no global marker-kind meaning. A point role selects a finite point or constrained-point marker, and a curve role selects a finite line-or-circle or arc marker. Address and identifier matches precede the feature-local compatible-marker ordinal. When an explicit reference does not supply the required role, the operand may use the complete finite point roster for a point role or the solver-line roster for a curve role. Coincident point and endpoint representations count as one physical point in this complete-roster join and use the canonical shared-coordinate locus. A typed relation requires exactly one ordered geometry match at the scalar's length or angle; zero or multiple matches retain the native relation and its ordered operand cells.

For a family-scoped angular curve role, a solver-line pair is an unoriented line carrier. If `theta` is the endpoint-direction angle in `[0, pi]`, its angular witness is `min(theta, pi - theta)`. This rule applies only to the solver-line carrier; a resolved profile line keeps its stored endpoint direction.

An `sgCircleDim` operand-cell tag `4c 81` addresses the unique finite point or constrained-point marker with the same feature-local object index. This form does not use the marker's local identifier or a compatible-family ordinal.

A point operand projects to a typed sketch constraint only when its marker identifies exactly one profile locus. A coordinate shared by multiple profile loci does not select one by ordering. A referenced coordinate-bearing point handle remains a distinct point locus when its coordinate coincides with profile geometry.

When exactly one operand of a horizontal-points or vertical-points relation identifies a profile locus, the other operand identifies the sole distinct locus in the complete owning sketch with the same vertical or horizontal coordinate, respectively. Zero or multiple aligned loci leave the relation unresolved.

A circular dimension whose operand marker does not identify a profile locus selects the unique circle or circular arc in the owning sketch whose solved radius equals the radius parameter or half the diameter parameter. Zero or multiple radius matches leave the relation native.

A repeated circular-dimension run selects every circle or circular arc in the owning sketch whose solved radius equals the run's reference radius. The candidate population has at least two members and no more members than the run has scalar records. A population that fails either bound leaves the relation native.

A unique profile-derived coordinate transform supersedes placement-derived projection. When multiple profile-compatible transforms remain, an axis-aligned placement selects the transforms with its signed axis permutation while retaining the profile-derived translation. When no profile transform exists and the sketch normal, u-axis, and `normal × u` axis are each parallel to distinct world axes, the marker coordinate pair supplies the two ascending world-coordinate components other than the normal axis. The signed axis permutation and sketch origin supply the fallback sketch coordinates. An arbitrarily rotated in-plane frame does not determine marker-coordinate semantics without profile anchors.

A circular dimension with one point, constrained-point, or line-or-circle handle operand, one length parameter with radius or diameter display, and a unique feature-input-to-sketch coordinate transform carries a full circle centered at the transformed handle coordinate. In the two-operand circular form, the second operand supplies the center and the first remains the display handle. Multiple equally scoring transforms are equivalent when they produce the same multiset of centers and radii for every circular dimension owned by the feature; the canonical transform orders axis swap, axis signs, then translation. When no equal circle exists, the circle is added to the sketch without adding it to a selected profile chain. The relation record is the circle's native geometry carrier.

A point or constrained-point marker addressed by a sketch relation and absent from selected-profile geometry carries a construction point when principal-plane placement supplies its sketch coordinate or every admissible signed-axis transform maps it to the same sketch coordinate. The point handle remains a distinct construction locus when its coordinate is occupied by a profile endpoint, curve center, or point.

When a circular-dimension operand with tag `83fe` has no explicit line-or-circle marker, the feature's non-origin coordinate point markers form ordered center/radial-point pairs. The operand index addresses the pair ordinal. The pair is accepted only when its Euclidean radius equals the driving radius or half the driving diameter.

When an `sgCircleDim` operand has tag `6e 83`, no explicit marker, and a lane-local `sgEntHandle` reference, the owning feature's finite point and constrained-point markers form an even ordered roster. Each adjacent pair is a center/radial pair: the center has a nonzero local identifier, the radial marker's object index equals that center identifier, and the radial marker has a nonzero local identifier. The operand's entity index selects the pair ordinal. Every roster pair must satisfy these joins, and the selected pair's distance must equal the driving radius or half the driving diameter.

When an `sgCircleDim` operand has tag `d4 80` or `d5 80`, its explicit marker must be a finite point or constrained-point marker in the unique lane declared by the operand's `sgEntHandle` reference. The marker's local identifier equals the operand address, and the marker coordinate is the circle center. These tags use that identity directly; adjacent point-pair and radius-only carrier searches do not apply.

For any other native `sgCircleDim` tag except `4c 81`, an explicit `sgEntHandle` marker reference may supply the center only after no declared pair, child, geometry-locus, or unique circular-witness carrier resolves. The reference must resolve to a finite same-feature point or constrained-point marker in the unique declaring lane, and its local identifier must equal the operand address. A point that is the radial member of a declared pair cannot supply this fallback center. Its coordinate is the circle center. An invalid or ambiguous identity leaves the relation native.

An `sgCircleDim` operand that selects an arc marker carries a bounded arc when inline center, start, and end coordinates or exactly two endpoint markers provide finite distinct endpoints with equal nonzero radius equal to the driving radius. When neither endpoint form resolves, the arc marker carries a full circle only when exactly one later point or constrained-point marker owned by the same feature has finite distance equal to the driving radius. A missing, repeated, or inconsistent radial witness leaves the relation native. A relation or native operand handle uses the unique duplicated-link distance relation and its following radial point to select the same arc center; the radius must equal the driving radius.

A coordinate-less `line_or_circle` marker before a declared `sgEntHandle` operand identifies an arc carrier when it has exactly two identical links to one earlier same-feature `Arc` marker. Both links must carry the arc marker's local identifier, and the target arc must pass the inline-endpoint or unique-later-radial-witness test above. Exactly one such linked arc carrier is required; a missing, mismatched, or repeated carrier leaves the relation native.

An `sgCircleDim` operand whose lane-local token declares `sgEntHandle`, except tag `4c 81`, uses two adjacent markers in the feature's coordinate-bearing point, constrained-point, line-or-circle, and arc roster. The first marker identifies the circular carrier's center. The ordinary linked form requires the second marker to be a point or constrained point, the center's nonzero local identifier to equal the second marker's object index, and the second marker's local identifier to be absent or zero. The indexed point form has the same center-to-object join but requires the second point or constrained-point marker's local identifier to be nonzero. Their finite nonzero distance equals the driving radius or half the driving diameter. Exactly one matching linked pair defines the circular carrier; zero or multiple pairs leave the relation native. When no linked pair exists, exactly one admissible circular marker defines the carrier. A line-or-circle requires exactly one later point or constrained-point marker at the driving radius. An arc uses its bounded endpoints when they define the driving radius; when no endpoints resolve, it requires exactly one later point or constrained-point marker at that radius. Missing, repeated, or conflicting carriers leave the relation native.

The slot-handle circular form is separate from the adjacent point-pair forms. Its slot identity and selected-center identity must both be carried by the two exact `sgSlotHandle` cells; a radius or coordinate coincidence cannot substitute for either identity.

The wide child form declares `sgArcHandle` or `sgLineHandle` after the radial point and before the next feature marker. `sgArcHandle` declares the preceding arc-plus-point pair; `sgLineHandle` declares the preceding line-or-circle-plus-point pair. The pair is valid only when its finite center-to-point distance equals the driving radius or half the driving diameter. A declared child pair takes precedence over radius-only carrier search; multiple declared pairs or a mismatched radius leave the relation native.

A uniquely enclosed planar B-rep belongs to that sketch only when it contains every circle or arc defined by an `sgEntHandle` operand at the same transformed center and radius. A B-rep that omits any such circular carrier is the sketch's planar support geometry. Its faces, profile entities, and derived endpoint constraints are not neutral sketch records.

An explicit point center binds a circular dimension to an existing same-sketch circle or arc only when exactly one point entity has that marker as its native reference and exactly one circular entity has the same quantized center and driving radius. Multiple center points or circular entities leave the relation native.

When profile loci do not determine the feature-input coordinate transform, cylindrical surface carriers normal to the sketch plane provide circle-center anchors. The cylinder axis origin projects into sketch coordinates along the sketch `u` axis and `normal × u` axis. A cylinder is compatible with a circular dimension when its radius equals the driving radius or half the driving diameter. A signed-axis transform qualifies only when it maps every dimensioned center to a distinct compatible projected cylinder center. Multiple qualifying transforms are equivalent only when they produce the same complete multiset of centers and radii.

A non-coordinate marker with type code `12` is a midpoint relation. It has exactly two linked markers: one point or constrained-point marker and one line, circle, or arc marker. Link order is not significant. Each linked marker must identify exactly one profile locus; the point locus is constrained to the midpoint of the entity owning the other locus.

Non-coordinate marker codes `18`, `19`, and `20` constrain one resolved circular-arc entity to positive angles π/2, π, and 3π/2 radians respectively. The relation remains native unless all linked loci identify the same single profile entity.

A feature-input class declaration is `ff ff 01 00`, a little-endian u16 byte length, and an ASCII class name. When the following record begins at declaration offset `+ 6 + length`, that record is an instance of the declared class. A feature-name record begins with the lane's u16 name-class token, `ff fe ff`, a u8 UTF-16 code-unit count, and the UTF-16LE name. The token has its high bit set and is established by the lane's first name record: the first class declaration directly followed by the token and the `ff fe ff` prefix. Every feature-name record in the lane repeats the same token. The little-endian u32 at eight bytes after the name is the feature object ID. It equals the corresponding Keywords feature `id` and binds the records independently of the display name. When a Keywords feature omits that ID, one exact feature name and one directly declared class bind the feature-input object; a repeated name or class leaves it unbound.

An `moRefPoint_c` feature-input object stores its solved model-space position relative to the byte after its UTF-16LE feature name. At name-end +0, the object header is `00 00 00 00 00 00 00 c0`; name-end +8 stores the feature object ID as u32 LE; and name-end +12 stores four zero bytes. At name-end +227 or +243, sixteen zero bytes precede the position. At name-end +243 or +259, three finite f64 LE values store xyz in metres. At name-end +267 or +283, a u16 LE construction form is `4` or `5`. At name-end +269 or +285, eight zero bytes terminate the solved-position cache. The short solved-point layout ends at name-end +277. The long solved-point layout ends at name-end +293. Exactly one complete layout within the feature object supplies the datum point. Multiple layouts are equivalent only when they store the same position.

An `moCoordSys_c` feature-input object with a component-point origin and two line-axis references stores one solved X/Y coordinate frame. The 151-byte component-point record begins with family byte `2d` or `2f` and suffix `80 02 00 00 00 00 00 00 00`, followed by 35 zero bytes, sixteen `ff` bytes, and eight zero bytes. Record +69 stores a nonzero u32 LE source ID, +73 stores a nonzero non-sentinel u32 LE source stamp, +77 stores u16 zero, +79 stores u16 `1`, +81 stores six zero bytes, +87 stores a nonzero u32 LE object ID, and +91 stores twelve zero bytes. Record +103 stores `c7 cf ff ff c7 cf ff ff`, +111 stores u32 zero, +115 stores a nonzero non-sentinel u32 LE generation word, and +119 stores eight zero bytes. Three finite f64 LE values at +127, +135, and +143 store the solved origin in metres.

The extended component-point record is 165 bytes and has the same prefix through the source stamp at +73. Record +77 stores a nonzero non-sentinel u32 reference ID, +81 stores `ff ff ff ff`, +85 stores u32 zero, +89 stores a positive non-sentinel u32 reference count, +93 stores u32 `1`, +97 stores u32 zero, +101 stores a nonzero u32 object ID, and +105 stores twelve zero bytes. Record +117 stores `c7 cf ff ff c7 cf ff ff`, +125 stores u32 zero, +129 stores the shared nonzero non-sentinel generation word, and +133 stores eight zero bytes. Three finite f64 LE values at +141, +149, and +157 store the solved origin in metres.

A component-path point record has the same 69-byte prefix and nonzero source ID as the fixed component-point records. Record +73 stores seven `ff` bytes. Record +80 stores a positive u32 LE path-entry count, +84 stores a component-vector selector whose byte 1 is `02`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero, +88 stores four zero bytes, +92 stores the duplicated compact component marker, and +108 stores two zero bytes. The counted typed component path starts at +110 and uses the compact component-path entry and separator grammar. An optional terminal null slot `ff ff ff ff 00 00 00 00` follows the path. Relative to the byte after the path or optional null slot, fourteen zero bytes, u32 LE `1`, four zero bytes, a nonzero non-sentinel u32 LE object ID, and twelve zero bytes precede the solved suffix. The 48-byte solved suffix stores `c7 cf ff ff c7 cf ff ff`, u32 zero, the shared nonzero non-sentinel generation word, eight zero bytes, and three finite f64 LE origin coordinates in metres. The component path must have one complete interpretation.

A component-point origin without line-axis references uses an ordinal-axis tail. Two distinct u16 LE values at tail +0 and +2 are in the range `1..3` and select the ordered X and Y axes from the global X, Y, and Z basis. Tail +4 stores 23 zero bytes. The f64 LE value at tail +27 repeats the component-point origin Z coordinate in metres. One or two nonzero u16 LE tokens terminate the feature object. The selected X and Y axes and their cross product form the solved frame.

A two-point coordinate frame contains exactly two extended component-point records with one generation word. The 14-byte separator between the records stores u16 LE values `2`, `1`, and `0`, a nonzero u16 token, u16 `1`, and two nonzero u16 tokens. A 94-byte solved tail follows the second record. Tail +0 and +8 repeat the first point's Y and Z coordinates in metres. A unit X direction occupies tail +16 through +39. Byte +40 is zero, and the same direction is repeated at unaligned f64 offsets +41, +49, and +57. Tail +65 stores three zero bytes. Three f64 LE values at +68 store the complete first point, and tail +92 stores a nonzero u16 token. The first point is the frame origin. The perpendicular component of the vector from the first point to the second point supplies Y. The two points must not differ only along X. The cross product `X × Y` supplies Z.

An endpoint-path origin is used only when the coordinate-system object contains no fixed or component-path point origin. Its record begins with `2f 80 02 00 00 00 40 00 00 75 00 00 00 75 00 00 00`, followed by 28 zero bytes, sixteen `ff` bytes, and eight zero bytes. Record +69 stores a nonzero non-sentinel u32 LE selector, +73 stores seven zero bytes, +80 stores a positive u32 LE path-entry count, +84 stores a component-vector selector whose byte 1 is `02`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero, +88 stores a nonzero non-sentinel u32 LE token, +92 stores the duplicated compact component marker, and +108 stores two zero bytes. The counted component path starts at +110. A required null slot `ff ff ff ff 00 00 00 00` follows the path. Relative to the byte after that slot, 70 zero bytes, u32 LE `1`, four zero bytes, a nonzero non-sentinel u32 LE object ID, and twelve zero bytes precede the 48-byte solved suffix. The solved suffix uses the component-path point handles, generation, zero padding, and finite origin-coordinate layout. A coordinate-system object can contain auxiliary endpoint-path records; they do not compete with a fixed or component-path point origin.

Each line-axis record is 113 bytes. Record +0 stores `c7 cf ff ff c7 cf ff ff`, +8 stores u32 zero, +12 stores the same generation word as the component-point record, and +16 stores sixteen zero bytes. Record +32 stores a positive finite f64 carrier scalar. Three finite f64 LE values at +40 store a point on the selected line. Three f64 LE values at +64 store a unit direction. Byte +88 is zero. The same direction is repeated as three f64 LE values at +89. The first line-axis record is the X direction reference and the second is the Y direction reference.

The 29 bytes immediately after the second line-axis record complete a two-axis frame. A one-axis frame stores the same tail immediately after its sole line-axis record or after two zero bytes. Bytes +0, +1, and +2 are Boolean X, Y, and Z direction-reversal flags. The complete X/Y forms have a zero Z flag. Three finite f64 LE values at +3 store the origin in metres and equal the component-point origin. The final u16 token is nonzero. The X and Y flags reverse their corresponding directions. In the two-axis form, the solved X axis is the normalized first reference. The solved Y axis is the normalized component of the second reference perpendicular to X. The two references must not be parallel. In the one-axis form, the solved X axis is the normalized line direction. The vector from the component-point origin to the line point supplies Y after removal of its X component. The line must not contain the origin. The solved Z axis is `X × Y`. Exactly one component-point record and either one or two ordered line-axis records with one common generation word supply a solved frame. Distinct frames in configuration lanes leave the coordinate system unresolved.

A repeated class instance stores a little-endian u16 class token immediately before its feature-name marker. The token is scoped to the `ResolvedFeatures` lane. Repeated instances with the same token have the same declared class.

A sketch-input entity starts with the prefix `ff ff 1f 00 03`, `ff ff 07 00 01`, or `ff ff 1f 00 01`. Each prefix is followed by eight `ff` bytes and the little-endian f32 `-1.0`; each stores the kind at marker +17, the geometry/display role at marker +23, and the state value at marker +48. A model-space point with geometry role `05 00 01 00` and profile role u16 `1` stores marker `0e 00` followed by three finite little-endian f64 xyz coordinates in metres. The `ff ff 1f 00 03` kind-`0` layout places the marker at +56 and coordinates at +58; its kind-`1` layout places them at +64 and +66. The `ff ff 07 00 01` kind-`0` layout places them at either +56 and +58 or +64 and +66. An object-indexed `ff ff 1f 00 01` kind-`0` point places them at +64 and +66. The `ff ff 1f 00 01` kind-`1` layout with geometry role `04 00 02 00` also places them at +64 and +66. In either `... 00 01` geometry-locus layout, `1e 00` is at marker +56 and the two finite little-endian f64 coordinate fields are at marker +58 and +66, in metres. In the `ff ff 1f 00 01` indexed-curve layout, a non-coordinate profile curve has role u16 `1` at marker +27, stores zero-based feature-local marker-roster indices as little-endian u16 values at marker +56 and +58, and stores little-endian u32 `1` at marker +60. A complete indexed-curve body remains non-coordinate when its first u16 endpoint index is `30` and therefore encodes `1e 00` at marker +56. The 84-byte object-indexed profile-line layout instead has kind `0`, role bytes `04 00 02 00`, profile role u16 `1`, selector bytes `00 00 80 bf 00 00 05 00` at marker +31, little-endian f64 `1.0` at marker +48, two nonzero distinct little-endian u16 feature-local point object IDs at marker +56 and +58, four zero bytes at marker +60, and little-endian f64 `-1.0` at marker +64. The next sketch-input marker starts at marker +84. In a sequence of `ff ff 1f 00 01` kind-`0` indexed profile lines with the same body, the final 84-byte record remains a line when no subsequent sketch-input marker follows it. A 92-byte `ff ff 1f 00 03` kind-`2` indexed arc with a coordinate-bearing kind-`2` center carrier stores two nonzero distinct direct feature-local point object IDs at marker +64 and +66. The unique coordinate-bearing kind-`2` marker equidistant from both points supplies the arc center. Kind `2` without that compact profile selector or a 92-byte indexed-curve trailer is the selected construction line. A connected run of at least two indexed arcs carries circular arcs when it addresses at least three distinct roster points, exactly one circle contains every addressed point, and every ordered endpoint pair has a positive counterclockwise sweep no greater than π. The shared circle supplies each arc center and radius. In the `ff ff 07 00 01` indexed-line layout, coordinate-bearing records are the same one-based point roster. A non-coordinate record with profile-curve role `1` or `2` at marker +27 stores its zero-based point-roster indices as little-endian u16 values at marker +64 and +66. Role `1` is a profile line and role `2` is the selected construction line. Marker-backed curve components form neutral profiles only when every endpoint has degree two and the component is one closed cycle; traversal direction records whether each stored endpoint pair is reversed. Role `2` curves are construction geometry and do not participate in profiles. Other marker entities are not construction geometry. A planar profile object may carry one complete reference-plane frame using the same matrix, fixed, angular, minimal, or compact frame encoding as a constructed reference plane. That frame places every coordinate-bearing entity owned by the profile object. The sole selected construction line in a revolution profile is its placed axis.

An extended-prefix kind-`0` selector-`44` indexed line has profile locus `04 00 02 00` or geometry locus `05 00 01 00`, role and state u16 values `1` at marker +27 and +29, selector bytes `00 00 80 bf 00 00 44 00` at marker +31, and f64 `1` at marker +48. Marker +56 and marker +58 store distinct zero-based indices in the feature-owned coordinate-bearing point roster. Marker +60 stores u32 `1`, and marker +64 stores f64 `-1`. Both terminal endings store zero bytes at marker +39 through +47. A continuation ending instead stores `00 00 01 00 00 00 00 00 00` at marker +39 through +47, `00 00 01 00 02 00 00 00 02 00 00 00` at marker +72 through +83, and the next sketch-input marker begins at marker +84. A counted terminal ending stores zero bytes at marker +72 through +127, a nonzero non-null u32 at marker +128, six zero bytes at marker +132 through +137, four `ff` bytes at marker +138 through +141, and two zero bytes at marker +142 through +143. A control terminal ending stores zero bytes at marker +72 through +141, `08 80` at marker +142 through +143, zero bytes at marker +144 through +153, and the control sequence `01 00 01 00 02 00 00 00 03 00 00 00 02 00 00 00` at marker +154 through +169. Each form emits a straight line between its two selected endpoints.

A compact legacy code-`2` profile point is a 132-byte record with eight `ff` bytes at marker +5, native code u32 `2` at marker +13, zero bytes at marker +17 through +18, profile locus `04 00 02 00`, role u16 `1`, zero bytes at marker +25 through +30, selector bytes `04 00 00 00 00 00 00 00 00 00 00` at marker +31, coordinate tag `1e 00` at marker +42, and finite f64 coordinates at marker +44 and +52 in metres. Marker +60 through +61 are zero, marker +62 stores u16 `4`, and two homogeneous eight-byte operand cells begin at marker +64 and +72. The cells have one common selector kind, distinct non-sentinel u16 identifiers, and four `ff` bytes. Marker +80 stores `00 00 fe ff ff ff`, marker +86 through +119 are zero, marker +120 stores u32 `2`, marker +124 through +127 are zero, and marker +128 stores the non-null feature-local object identifier. The next sketch marker begins at marker +132. This record emits a point and contributes its coordinate to the raw coordinate roster.

An embedded compact legacy geometry handle is a 120-byte record with eight `ff` bytes at marker +5, native code u32 `0`, zero bytes at marker +17 through +18, geometry locus `05 00 01 00`, role u16 `1`, zero bytes at marker +25 through +30, selector bytes `05 00 00 00 00 00 00 00 00 00 00` at marker +31, coordinate tag `1e 00` at marker +42, and finite f64 coordinates at marker +44 and +52 in metres. Its u32 state at marker +60 is zero or nonzero and non-null, marker +64 through +69 are zero, marker +70 stores i32 `-1`, marker +74 through +115 are zero, and marker +116 stores a non-null feature-local identity. The next sketch marker begins at marker +120. This record contributes its coordinate to the raw coordinate roster and emits no sketch entity.

A terminal compact legacy diameter-circle record has kind u32 `1`, geometry locus `04 00 02 00`, profile role u16 `1`, state u16 `1`, selector bytes `04 00 00 00 00 00 00 00 00 00 00` at marker +31, a zero-based radial-roster ordinal at marker +42, u16 `0` at marker +44, u32 `1` at marker +46, and f64 `-1.0` at marker +50. Bytes +17 through +18 are zero, and bytes +27 through +30 are zero. Bytes +58 through +101 are zero, u16 `3` is at marker +102, and the terminal class declaration is `ff ff 01 00`, u16 length `11`, and class name `sgCircleDim` at marker +110. The first coordinate-bearing marker in the feature-owned raw coordinate roster is the center; the ordinal at marker +42 selects the radial marker from that same roster, including coordinate-bearing geometry handles that do not emit sketch entities. The finite nonzero center-to-radial distance is the circle radius.

An `moCosmeticThread_c` or `moDerivedCosmeticThread_c` feature is a non-geometric thread annotation. Its diameter-displayed `D2` length scalar is the nominal thread diameter. A `D1` length scalar selects blind termination and stores the axial blind length; omission of `D1` selects through termination. Both scalar payloads use native meters. The `D2` scalar object identifier is one less than the thread feature object identifier. Its following child tail begins after the scalar value and ends before the next scalar value or feature-name record. An `moCylinderRef_w` child in either the thread feature object or that scalar child tail carries the attached cylindrical face. Its body starts with its class token, a nested class token, `02 00 00 00`, a selector byte `00` or `40`, and two zero bytes. The component-path marker starts 46, 62, 66, 70, 90, 94, 102, 106, or 110 bytes after the body. A direct class declaration ends 21 bytes before the body, placing the marker 67, 83, 87, 91, 111, 115, 123, 127, or 131 bytes after the declaration. The marker has a positive little-endian u32 slot count at −12, a component-vector selector whose byte 1 is `02` or `03`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero at −8, and two zero bytes after its duplicated 16-byte identity. The first ordered typed component is the attached face; subsequent typed components retain its owning path. A marker whose slots use persistent object tokens instead of typed component entries still identifies the cylinder-reference operand. Multiple structurally complete cylinder-reference bodies in the same thread object are repeated carriers of that operand. When none of their markers has a decodable component path, exactly one B-rep cylindrical face is the attached face; zero or multiple cylindrical faces leave it unresolved. When the cylinder-reference child is absent, exactly one B-rep cylindrical face whose radius is half the nominal diameter is the attached face; zero or multiple matching faces leave it unresolved.

A `moCompEdge_c` child in a cosmetic-thread object is an alternate attached-face carrier. A declared class or a lane-scoped repeated class token opens the carrier. The carrier body either carries a compact edge-selection vector directly or begins a fixed component-edge wrapper prefix of 17 bytes: an inner high-bit u16 class token at body +0, byte `02` at +2, zero bytes at +3..+8, and equal nonzero little-endian u32 component counts at +9 and +13. The wrapper body carries an immediate `moEdgeRef_c` child whose body carries the compact edge-selection vector. A repeated `moEdgeRef_c` child body begins with `01 00 00 00 00 00 00 00`. A declared carrier interval ends at the next class declaration in the thread object. A repeated carrier interval ends at the next feature-name record. Exactly one complete compact edge-selection vector in the selected carrier interval supplies the thread surface selection; its first typed component is the generated face identity and the remaining components preserve the owning path. Component-edge carriers are considered only when neither an `moCylinderRef_w` nor an `moCompFace_c` carrier produces a candidate. Multiple complete vectors or conflicting parses leave the thread face unresolved.

When `D2` is absent, a surface selection owned by the thread supplies the nominal diameter only when it identifies one threaded-hole producer; that producer's major thread diameter is the inherited `D2` value, and differing producer diameters leave `D2` unresolved.

An `moCompFace_c` body starts with its class token and `02 00 00 00`. A following `00 00` flag places its component-path marker 68 or 92 bytes after the body start. A compact full-round fillet face carrier uses the fixed zero-flag prefix with its component-path marker 64 bytes after the body start and a zero marker tail at `+80..+81`. A `40 00` flag places the marker 100 bytes after the body start in a face-supported reference-plane body and 68 bytes after the body start in an operation face carrier. An operation `moCompFace_c` body with `40 00` flags has its component-path marker at `+68` and a zero marker tail at `+84..+85`; in this carrier, `40 00` flags at `+6..+7` and component-path marker 68 bytes after the body start. `moCylinderRef_w` uses a zero-flag layout.

A zero-flag face-supported reference-plane body may contain a nested `moFaceRef_c` class declaration before its component path. When that declaration is present, the component-path marker begins 84 bytes after the `moCompFace_c` body start. The nested declaration occupies the interval before that marker. Relative to the `moCompFace_c` body, this fixed prefix has the class token at `+0`, u32 `2` at `+2`, zero flags at `+6`, the nested declaration region at `+8..+83`, the component marker at `+84`, and a zero marker tail at `+100..+101`; the variable path follows. The marker uses the same counted component-path grammar and identifies the supporting face.

The legacy face-alias body used by an offset reference plane begins with a non-null class token and `02 00 00 00`. Bytes `+6..+41` are zero, bytes `+42..+44` are zero, bytes `+45..+60` are `ff`, and bytes `+61..+68` are zero. The u32 at `+69` is `2`; `+73` holds a nonzero four-byte type identity; bytes `+77..+90` are `00 00 03 00 00 00 01 00 00 00 00 00 00 00`. The u32 at `+91` identifies the native object that owns the selected face. Bytes `+95..+98` are zero, the u32 at `+99` is `3`, bytes `+103..+106` are zero, and two `c7 cf ff ff` handles terminate the record. When the owner is a projected feature, its component-face selection is the support face.

The sketch-surface component-path variant stores u32 count `5`, a component-vector selector whose byte 1 is `03`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero, four opaque bytes, the component-path marker, and two zero bytes. The count includes two implicit root slots; three 20-byte heterogeneous component entries follow. A two-byte zero alignment word may precede the third entry. A selector whose byte 1 is `02` uses a nonzero selector in the four bytes before the marker and terminates the entries with an ownership trailer. The trailer contains u32 `1`, one zero u32, one nonzero owner u32, and three zero u32 words. It is preceded by 8 or 20 zero bytes, or by one nonzero u32 and 8 zero bytes.

A `Fillet_c` object with exactly three compact full-round face carriers uses those carriers in center, first-side, and second-side order. Each carrier has the role-03 component-vector selector and a counted component path. Other role-03 carrier counts do not identify a full-round fillet.

A compact `moDeleteBody_c` object ends with a little-endian u32 schema word `11000`, two zero u32 words, a u32 selection count, that many ordered u32 regeneration-input-local body identifiers, the sentinel `ff ff ff ff`, and three zero u32 words. The object ends after those zero words or one additional zero u32 word. The ordered identifiers are the persistent body selection consumed by the delete/keep operation. When another instance of the same declared class follows, its lane-scoped u16 repeated-class token lies between the selection terminator and the next feature-name marker.

The lane-scoped u16 token immediately following the sole `moDeleteBodyData_c` class declaration opens each body-state record owned by a compact delete/keep object. The token is followed by `2b 80 02 00 00 00 00 00 00`, the feature-local u32 body identifier twice, 28 zero bytes, 16 `ff` bytes, and 20 zero bytes. Body-state records precede the selection vector and retain their ordered local identifiers independently of the selected identifier list. The retained roster equals the ordered records between the owning feature-name record and its selection vector. The state roster is not a retention-mode discriminator.

A `moCompEdge_c` child carries an ordered compact edge-selection vector. The vector framing is complete without a corresponding class declaration. The vector marker is `7d c3 94 25 ad 49 b2 54 7d c3 94 25 ad 49 b2 54`. A little-endian u32 count occurs at marker −12, marker −8 stores a component-vector selector whose byte 1 is `02`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero, and two zero bytes follow the marker. An entry-form vector contains count typed entries. A compact entry contains a four-byte instance cell, a 12-byte type signature, and a little-endian u32 feature-local identifier. An intermediate traversal entry may omit the identifier and end after its type signature; its absent path position is retained as `_`. A wide entry inserts one zero u32 after the type signature and stores the feature-local identifier at entry offset +20, for a total entry length of 24 bytes. Wide vectors use the same width for every entry. Type signatures may be uniform or heterogeneous. Consecutive entries are adjacent or separated by four zero bytes, four `ff` bytes, eight or 12 zero bytes, `ff ff ff ff 00 00 00 00`, or `ff ff ff ff 01 00 00 00`. An eight-byte ordinal separator stores a nonzero, non-`ff` little-endian u32 path ordinal followed by a zero u32. An eight-byte state separator stores a zero u32 followed by a nonzero, non-`ff` little-endian u32 state. A four-byte root separator stores a non-null high-bit u16 token followed by `01 00`. The count may instead cover one terminal feature-reference cell following `count − 1` entries. That 36-byte cell begins `01 00 00 00 00 00 00 00 4a 80 00 00`, carries a nonzero class token, `37 00`, a nonzero little-endian u32 feature source identifier, a nonzero four-byte type identity, and 12 zero bytes. A compact-ID vector instead contains count nonzero little-endian u16 path identifiers. It is terminated by either 16 zero bytes and `ff fe ff`, or eight zero bytes followed immediately by the non-sentinel high-bit class token of the next object body. An entry-form vector is valid only when its width, separators, root slots, and terminal form yield one complete parse; conflicting parses remain native.

A reference-list edge vector stores a record identifier at marker −4 and begins its references after the two zero bytes at marker +16. The selector at marker −8 has byte 1 equal to `02`; byte 0 is a lane-specific subtype. Each reference contains one or more adjacent hops followed by one little-endian u32 local identifier. A hop is a non-sentinel high-bit u16 instance token, u16 zero, and a 12-byte type signature. The signature contains a four-byte prefix learned from the lane, a nonzero u32 producer object identifier, and a nonzero u32 timestamp. The prefix is constant within a lane and is not fixed across lanes; its first u16 is a non-sentinel high-bit token and its second u16 is nonzero. The producer object identifier is the native object identifier of the feature that created the referenced entity. The local identifier closes the reference because it is not followed by another valid hop signature. An `ff ff ff ff` sentinel and zero through ten zero padding bytes may separate consecutive references; the references may instead be adjacent when the next hop is structurally valid. The count at marker −12 is the reference count and frames the complete reference list. It may include one terminal null slot encoded as 16 through 18 zero bytes followed by `ff fe ff`; the preceding non-null references remain complete. Reference and hop boundaries are retained independently.

Structurally valid edge-selection vectors in a fillet or chamfer feature-object interval whose component path or terminal feature-reference cell identifies a feature preceding the consumer are the feature's input edge operands. Within one history, a positive feature object ID smaller than the consumer's positive object ID precedes it; when either object omits a positive ID, Keywords serialization order supplies the fallback. When the interval contains at least one such input-owned vector, vectors without a preceding producer are cached result references and do not belong to the selection. Multiple input-owned vectors retain stream order as one ordered native edge selection. If no vector identifies a preceding producer, every structurally valid vector remains the native selection. The first vector following a child declaration and repeated children whose body begins `2d 80 02` use the same vector grammar.

The input-edge roster of a fillet object ends before its first `moEdgeDim_c` or `moVertDim_c` child. A direct dimension child begins with `20 81 08 00`, the direct class declaration, and the lane-scoped class token. A repeated dimension child begins with `20 81 10 00`, a non-sentinel high-bit instance token, and that same class token. Component vectors inside these dimension children are radius-control references and are not fillet edge operands.

In an entry-form edge-selection vector, type-signature bytes 4 through 7 contain the native feature object ID traversed by that path entry. The ordered component identifiers form the persistent edge identity. The terminal feature-reference cell identifies the feature result owning that identity when present. Otherwise the last entry whose native object ID uniquely names a history feature identifies the owner; trailing entries may name non-feature result components. An object ID shared by multiple history features leaves the owner unresolved. The consuming fillet or chamfer depends on the ordered unique features traversed by all entries and terminal feature-reference cells.

A `moCompSurfaceBody_c` child of `moThicken_c` carries the selected surface components. Its lane-scoped class token occurs 103 bytes before the duplicated vector marker. Marker −12 is the little-endian schema word `6`; marker −8 begins with `04 02 00 00`; two zero bytes follow the marker. Entries contain a four-byte instance cell, one 12-byte type signature shared by the vector, and one little-endian u32 feature-local component identifier. Entries are adjacent or separated by one four-byte instance ordinal. The vector ends when the shared entry signature ends.

A `moCompSurfaceBody_c` child of a face or surface operation carries selected surface components in the same compact vector grammar. The vector marker is the unique duplicated marker in the bounded feature-object interval after the class body. Marker −12 is the little-endian schema word `6`; marker −8 has a lane-specific subtype byte followed by `02 00 00`; two zero bytes follow the marker. Entries contain a four-byte instance cell, one 12-byte type signature, and one little-endian u32 feature-local component identifier. Entries are adjacent or separated by one four-byte instance ordinal. The vector ends when the entry signature changes or the bounded feature-object interval ends.

An `moSurfCut_c` feature-input interval contains one target-body vector followed by one cutting-surface vector. Both vectors have selector byte 1 equal to `02` and lane-specific subtype byte 0; byte 0 does not identify the semantic role. The target vector uses the counted reference-list grammar and retains every selected body/entity path in order, including an unframed flat form. The later cutting-surface vector belongs to the `moCompSurfaceBody_c` child and uses the compact surface-component grammar. The first and second role-02 vectors in the bounded interval therefore project as the target body and cutting-surface face, respectively. The target vector's terminal producer object ID identifies the generated target feature result, and the cutting-surface vector's terminal producer object ID identifies the cutting-surface feature result. The target projects as a body selection and the cutting surface projects as a face selection. The retained/discarded-side flag is independent of both vectors and remains unresolved when no native Boolean field is present.

An `moPlanarSurface_c` feature-input interval contains exactly two defining component-path vectors. Each vector has selector bytes with a nonzero lane-specific subtype followed by `02 00 00` at marker −8 and uses the counted component-path grammar. A lineage path may encode a nonterminal hop as its 16-byte instance cell and type signature without a local identifier; the next high-bit instance cell immediately follows it. The terminal hop has the same 16-byte prefix followed by its four-byte local identifier. A path may contain repeated instance tokens in consecutive hops; every hop remains an independent lineage entry. The terminal path producer object ID and local identifier identify the defining entity. The later `moPlanarSurfIdKeeper_c` vectors use selector suffix `03 00 00` and are generated-output identity records, not defining operands.

An `moPLine_c` feature-input object containing exactly one `moPLineProject_c` class in its bounded feature-object interval is a projected split-line operation. Its splitting tool is the unique planar `moProfileFeature_c` object in the same history whose positive object ID is smaller than the split-line object ID and whose complete nonempty named dimension map equals the split-line dimension map before evaluated scalar substitution. No match or multiple matches leave the tool unresolved.

`moPLineProjIdRep_c` and `moPLineSurfIdRep_c` records carry repeated persistent target-face paths. The first typed component's type-signature object ID equals the consuming `moPLine_c` object ID. The terminal typed component's type-signature object ID identifies the earlier feature result owning the selected face, and its local identifier is that result's feature-local face ID. The records can occur outside the bounded split-line feature object and bind by the consuming object ID. Repeated paths with one terminal producer and local face ID are one target face. Distinct terminal producer and local face-ID pairs are distinct target faces in stream order. The split-line operation depends on the splitting sketch and every uniquely identified target-face producer.

`moExtrusion_c` and `moICE_c` are extrusion feature classes. `moProfileFeature_c` and `mo3DProfileFeature_c` are planar and spatial sketch feature classes. `moOriginProfileFeature_c` is the built-in model-origin tree node and carries no sketch geometry. `moCombineBodies_c` is the body-Boolean feature class. `moConstSurfRef_w`, `moEndPointRef_w`, `moGeneralCurveRef_w`, `moLineRef_w`, `moSingleFaceRef_w`, `moSolidRef_w`, `moCompReferenceCurve_c`, and `moCompSurfaceBody_c` identify reference objects rather than feature operations.

A spatial-sketch vertex record begins with `ff fe ff 06` followed by the UTF-16LE string `Vertex`. At record offset `+43`, `0e 00` identifies three little-endian f64 model-space coordinates at offsets `+45`, `+53`, and `+61`. A vertex-record run contains a positive even number of records. Each consecutive pair owns one bounded line in stored vertex order. An object-indexed marker-backed spatial point begins with `ff ff 07 00 01`, `ff ff 1f 00 01`, or `ff ff 1f 00 03`, eight `ff` bytes, little-endian f32 `-1.0`, role bytes `04 00 02 00`, and profile role u16 `1`. Its non-sentinel little-endian u32 feature-local object ID immediately precedes the marker. In the `ff ff 07 00 01` form, marker +56 contains `0e 00` and the three little-endian f64 model-space coordinates begin at marker +58. In either `ff ff 1f` form, the native kind is at marker +17, marker +48 stores f64 `1`, marker +56 through +63 are zero, marker +64 contains `0e 00`, and the coordinates begin at marker +66. A current-prefix indexed XYZ spatial point has the same fixed prefix through xyz coordinates at marker +58. A continuation-bounded point stores u16 `8` at marker +82, u16 `1` at marker +84, six zero bytes at marker +86, and `fe ff ff ff` at marker +92; its admitted native-kind/locus pairs are `(0, 04 00 02 00)`, `(0, 05 00 01 00)`, and `(1, 04 00 02 00)`, and its following marker starts at marker +158 or at marker +162 after a four-byte separator at marker +158. A terminal geometry-locus kind-0 point stores u16 `1` at marker +82, u16 `0` at marker +84, six zero bytes at marker +86, and `fe ff ff ff` at marker +92, followed by zero bytes from marker +96 through +219, `08 80` at marker +220, either ten or fourteen zero alignment bytes, and the table header `01 00 01 00`. The short alignment form has ten zero alignment bytes at marker +222 through +231 and places that header at marker +232; the long form has fourteen zero alignment bytes at marker +222 through +235 and places it at marker +236. Both forms then store u32 values `1`, `2`, twelve u32 values `1`, u32 `0`, u32 `1`, u32 `0`, two zero bytes, and the fixed terminal control sequence `00 00 ff fe ff 00 ff ff 00 00 80 bf ff ff ff ff 01 00 00 00 ff ff ff ff` at marker +306 or +310. An unindexed zero-valued geometry-locus marker is an anchor and does not enter the spatial-sketch point roster. The wide forms use native kind zero for a point or a constraint-taxonomy kind from 1 through 85 for a relation handle. Each coordinate is zero or a normal finite f64. Point and line records retain their byte order within one spatial-sketch feature object. These spatial point records identify a spatial profile independently of its feature-class declaration. Spatial geometry is not projected into a planar sketch coordinate system.

A detached spatial relation-manager object begins with `moRelMgr_c`, contains `sg3DPlaneHandle`, and ends before the following `suObList`. Its owner is the unique `mo3DProfileFeature_c` whose complete `D`-numbered length-parameter name set equals the object's non-display `D`-numbered scalar name set and whose parameter lengths equal corresponding native scalar values after metre-to-millimetre conversion. No match or multiple matches leave the object unbound. Object-indexed coordinate-bearing relation handles in the bound object are the spatial-sketch point roster; coordinate-bearing native-kind-zero records are reference-geometry points. Three or more distinct, noncollinear roster points that lie on one plane define the normal axis of a hole-position sketch. Every roster point is one hole-axis origin. A collinear or noncoplanar roster does not define hole placements.

A compact `moCombineBodies_c` object carries its target and tool as the outermost recognized type-3 component-path vectors in its feature-object interval. Additional type-3 vectors may occur between these operands and are auxiliary carriers, not operands. A type-3 vector uses the duplicated component marker, a positive count at marker −12, a component-vector selector whose byte 1 is `03`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero at marker −8, two zero bytes after the marker, and typed path entries with the same separator grammar as edge component paths. A 20-byte entry contains an instance cell, a 12-byte type signature, and a local identifier. A 16-byte continuation entry contains an instance cell and a type signature with no local identifier; it is followed immediately by the next high-bit instance cell in the same lineage path. The terminal entry carries the local identifier. A 16-byte anonymous entry instead contains a type signature and local identifier with no instance cell. The count either equals the entry count or includes one terminal null slot. The standard null slot is `ff ff ff ff 00 00 00 00`; the alternate slot is ten zero bytes and may be followed by two zero alignment bytes. Each lineage hop remains an ordered component, including consecutive hops with the same instance token. The two paths retain their ordered native identities independently of the Boolean operation.

The compact Combine operation is a little-endian u32 at feature-name marker offset `+ 117 + 2 × name-code-unit-count`. Twelve zero bytes precede it. The standard trailer is six zero bytes followed by `ff ff ff ff`; the alternate trailer is two zero bytes followed by four `ff` bytes. Values `0`, `1`, and `2` mean join, subtract, and intersect respectively.

An extrusion object immediately following a `moProfileFeature_c` object consumes that profile feature. History metadata records do not interrupt this adjacency. A classless history `Extrusion` or `Cut` record with a nonzero source identifier and one bound feature-input object participates as an extrusion object in this ordering. A profile feature may instead be followed by a sequence containing only sketch-block definition and sketch-block instance objects and then the extrusion. The block sequence belongs to the profile when the profile's `DissectableChildren` identifiers equal the distinct object identifiers of the intervening definitions. Without `DissectableChildren`, one distinct positive sketch-block definition and one or more distinct positive sketch-block instances form the compact block sequence. When an instance carries a `BlockDefinition` identifier, it identifies that definition; when the field is absent, the sole definition in the sequence supplies the ownership. The extrusion consumes the profile containing that block sequence. A compact extrusion without `DissectableChildren` also consumes a `moProfileFeature_c` object immediately following it. The profile feature is an ordered dependency of the extrusion. These adjacency forms are independent of the extrusion's `Dissectable` and `DissectableChildren` attributes; an explicit dissected-child association has precedence over ordinary adjacency.

A sketch-block definition stores reusable planar geometry in its local `(u, v)` coordinates. Each block instance maps `(u, v, 0)` into the owning profile space through its placement transform. All instances in one block sequence share one oriented plane; the first instance establishes the profile origin and `u` axis. The assembled sketch contains one transformed copy of every definition entity for every instance and preserves each source profile chain in instance order. Construction entities remain in the assembled sketch even when they do not participate in a profile chain. The block definition's own sketch placement is not applied again to an instance.

A block sequence holds two `sgSketch` objects. The first follows the definition's name and holds the definition's records. The second holds the owning profile's own records; it follows the first, inside the stream interval of the definition or of a later instance object of the sequence. An `sgSketch` object written by class token is the token, a u32, two zero bytes, and its records, each led by two u32 cells. The first marker after the definition's name opens the definition's `sgSketch`; the next marker led at marker −16 by the same class token and at marker −10 by two zero bytes opens the profile's. Every marker from there to the end of the sequence belongs to the profile, and an instance object holds no `sgSketch` of its own. The profile's plane reference follows its `sgSketch`.

The profile's `sgSketch` is followed by one sketch-block instance record for each instance of the sequence. An instance record has the sketch-marker prefix, f64 `1` at marker +48, eight zero bytes at marker +56, and the object identifier of its instance as the u32 at marker −4. Nine f64 values at marker +64 are three rows: the images of the definition's `u` axis, `v` axis, and normal in the owning sketch. The first two rows are orthonormal and have a zero third component. The third row is `(0, 0, d)`, where `d` is the determinant of the in-plane components of the first two rows, `1` or `−1`. The u32 at marker +136 is `1` when the three rows are the identity and `0` otherwise; the u32 at marker +140 is `0`. Three f64 values at marker +144 are the image of the definition's origin in metres, with a zero third component. The f64 at marker +168 is the scale `1`. The u64 at marker +176 is `1` when `d` is `−1`, a mirrored instance, and `0` otherwise. Three f64 triples at marker +184, +208, and +232 are the centre, the maximum corner, and the minimum corner of the instance's bounding box in the owning sketch; the centre is the mean of the two corners.

An instance maps a definition point `(u, v)` to `origin + u · u_axis + v · v_axis` in the owning sketch's coordinates. The placement transform is that map with the third row as its normal column, so a mirrored instance is a half turn about an in-plane axis. A mirrored copy of an arc sweeps counter-clockwise from the image of the definition arc's end to the image of its start. When the owning profile has its own sketch, each instance adds its copies of the definition's entities that have neutral geometry, and of the profile chains those copies complete, to that sketch, in that sketch's plane. An instance with no resolved definition or placement adds none. The dimension and reference records that follow the profile's `sgSketch` in the sequence belong to the profile.

The inline extrusion operation trailer establishes the extrusion object family independently of its class token. This applies when a repeated token is shared by more than one declared extrusion class.

An integer or Boolean Keywords dimension without dimensional-relation ownership is discrete. A same-named native f64 scalar binds to that dimension only when it exactly represents the existing integer or Boolean value. Other same-named native scalars in the feature-object interval belong to a different semantic field.

`moSweep_c` produces a solid sweep. Compact operation code `15` joins the swept result to the existing body. Its Boolean operation remains independently unresolved when no recognized operation carrier is present. `moSweepRefSurface_c` produces a surface sweep.

A solid or surface sweep's `moGeneralCurveRef_w` child identifies its path. A declared component-profile form contains an `moCompProfile_c` child followed immediately by `2b 80 02 00 00 00 00 00 00 00`. A compact component-profile form prefixes the same bytes with `01 00 dd 94 df 94`. In both forms, prefix +45 through +60 is `ff`. The older record stores the referenced feature's nonzero u32 source ID at prefix +69; source +12 through +15 and +20 through +31 are zero, source +32 and +36 are `c7 cf ff ff`, source +40 is zero, and source +44 is `f8 2a 00 00`. The newer record stores the source ID at prefix +81; source +8 through +15 are zero, source +16 is u32 `0x65`, source +20 is zero, source +24 is `ff ff ff ff`, source +28 is zero, source +32, +36, and +40 are `c7 cf ff ff`, source +44 is zero, and source +48 is `f8 2a 00 00`. A component-profile source ID naming a planar sketch feature makes that sketch the sweep path.

A solid `moSweep_c` object without an enclosed profile stream can be preceded and followed immediately by `moProfileFeature_c` objects. When the preceding object owns one resolved neutral sketch, that sketch is the sweep path and supersedes an opaque general-curve reference. When the following object owns one resolved neutral sketch, that sketch is the sweep cross-section profile. Each resolved sketch feature is a regeneration dependency. A missing, multiply addressed, non-profile, or unresolved adjacent object leaves the corresponding sweep operand unresolved.

A repeated general-curve form with a two-byte wrapper token, two zero bytes, a two-byte child token, and the compact child prefix `2b 80 02 00 00 00 00 00 00 00` retains the wrapper offset as its stable native path identity when it carries no component-profile source record.

An `moCompReferenceCurve_c` child identifies the sweep cross-section independently of the `moGeneralCurveRef_w` path. In the declared direct form, the class name is followed by `2b 80 02 00 00 00 00 00 00 00`; the referenced curve feature's nonzero source ID uses the same older and newer source-record layouts as a component-profile source. In the generated-component form, the lane-scoped wrapper token is followed by a child token and `2b 80 02 00 00 04 00 00`. Its duplicated component marker carries a positive count at marker −12 and `04 02 00 00` at marker −8. The homogeneous 20-byte typed entries use a six-byte first-entry separator consisting of a nonzero u16 and four zero bytes when present. The count includes one terminal slot after the entries; that slot is eight zero bytes followed by `f8 2a 00 00`. Type-signature bytes 4 through 7 identify the feature result owning the persistent curve components, and the ordered entry identifiers form the feature-local cross-section identity.

A `moCombineBodies_c` object is a body-Boolean feature independently of whether its Keywords element carries `Operation`, `Target`, or `Tools` attributes. Compact operation and component-path carriers supply absent attributes independently.

An `moBaseBody_c` feature is stored body geometry. Its display name and Keywords `Type` value do not identify the construction. The feature's resolved output-body scope identifies the stored geometry that enters the subsequent history.

A Keywords `Draft` element is a face-draft operation. `Direction` is the pull direction, the `Angle` dimension is the draft angle, and `Outward` is the outward flag of a neutral-plane draft. An absent or non-angular `Angle` dimension leaves the angle unresolved. An absent or non-Boolean `Outward` value leaves the outward operand of a neutral-plane draft unresolved. The `Faces` and `NeutralPlane` selections, the pull direction, and the retained angle and outward values remain independently meaningful.

A Draft feature-input interval uses the lane-scoped `moPlaneRef_w` token for its neutral plane and drafted faces. Each plane-reference prefix has this fixed layout before its variable component path:

| Offset | Type      | Meaning                                                                                                      |
| -----: | --------- | ------------------------------------------------------------------------------------------------------------ |
|     +0 | u16 LE    | lane-scoped plane-reference token                                                                            |
|     +2 | u16 LE    | non-`ffff` tagged child token                                                                                |
|     +4 | u32 LE    | value `2`                                                                                                    |
|     +8 | bytes[3]  | `00 00 00` or `40 00 00` wrapper flags                                                                       |
|    +11 | u32 LE    | nonzero reference identity                                                                                   |
|    +15 | u32 LE    | repeated reference identity                                                                                  |
|    +19 | bytes[28] | zero                                                                                                         |
|    +47 | bytes[16] | `ff`                                                                                                         |
|    +63 | bytes[9]  | zero                                                                                                         |
|    +72 | u16 LE    | tagged instance token                                                                                        |
|    +74 | u32 LE    | role word                                                                                                    |
|    +78 | u32 LE    | zero                                                                                                         |
|    +82 | u32 LE    | component-vector cell count in `2..65`                                                                       |
|    +86 | bytes[4]  | component-vector selector: byte 1 is `02` or `03`, byte 0 is a lane-specific subtype, and bytes 2–3 are zero |
|    +90 | u32 LE    | component selector                                                                                           |
|    +94 | bytes[16] | duplicated component-vector marker                                                                           |
|   +110 | u16 LE    | zero marker tail                                                                                             |

The first complete plane-reference record in the interval identifies the neutral plane. Later complete records identify the drafted faces; semantically identical component paths denote one face. The component vector uses the count-minus-one, count-minus-two, or mixed-entry forms defined for duplicated component-vector records. An odd cell count can also encode `N` mixed entries interleaved with `N−1` u32 path-slot words; in that form, `cell_count = 2N−1`. A unique addressed direction frame between the first component vector and the next plane-reference record supplies the pull direction. The frame starts with `c7 cf ff ff c7 cf ff ff`, u32 zero, a nonzero u32 address, and eight zero bytes. Twelve finite f64 LE values occupy offsets +24 through +119. In the 120-byte aligned form, the xyz pull-direction unit vector is the final three values at +96, +104, and +112. In the 153-byte extended form, the aligned direction slots are not a unit vector, nine zero bytes occupy +120 through +128, and an unaligned xyz unit vector occupies +129, +137, and +145. Missing, repeated, non-unit, or lane-disagreeing direction frames leave the native pull direction unresolved. Explicit Keywords face selections and direction take precedence over these feature-input operands.

A compact parting-line Draft uses direct duplicated component-vector records without a plane-reference wrapper. The 30-byte fixed prefix starts 12 bytes before the duplicated marker: a bounded u32 LE cell field in `1..65`, a component-vector selector whose byte 1 is `02` for the parting-tool selection or `03` for a drafted-face selection, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero, a u32 LE selector, the 16-byte duplicated component marker, and a u16 LE zero tail. Each selection contains one or more mixed component paths. The exact eight-byte `ff ff ff ff 00 00 00 00` separator terminates each path. The shortest complete mixed path ending at that separator is one path; a separator followed by no component terminates the selection. One parting-tool record precedes the addressed pull-direction frame. The drafted-face records follow that frame. A complete compact parting-line Draft sets the parting-tool faces, drafted faces, pull direction, and angle. It leaves the neutral plane unresolved and has no outward operand.

A planar sketch history name ending in `<N>`, where `N` is one or more decimal digits, aliases the uniquely named unsuffixed sketch when both records have the same XML element tag, resolved feature-input class, ordered content, and complete parameter map. The unsuffixed history feature remains the sole owner of the solved sketch geometry, and the geometry-less alias feature depends on that owner. Feature operands naming the alias bind to the owner's sketch and depend on the unsuffixed owner. A missing base, multiple matching bases, or any record-content difference leaves the alias operand native.

Keywords `Configuration` elements carry a non-empty, document-unique `Name`; `Material` carries the configuration material override. Decimal `SourceIndex=N` binds the configuration to `Config-N-Partition`. The decimal `id` selects the `Config-N-ResolvedFeatures` slot. All other attributes are configuration-local named values. Source indices are document-unique and independent of element order and the resolved-features slot identity. A matching partition supplies the configuration body set; without a matching partition the body membership stays unresolved. A configuration without `SourceIndex` has no ordinary partition binding; the uniquely manifest-selected configuration receives the selected `swID` when its name binds uniquely.

The unique `swMostRecentConfiguration="YES"` manifest row is the active
configuration identity, including when the manifest contains several
`swModel` rows. A single `swModel/@swConfigurationName` is the name fallback
when the manifest does not provide a name. A missing or unmatched active name
leaves the Keywords configuration identity unresolved.

`swConfigurationNeedsUpdate="YES"` states that the document carries no
current evaluated snapshot for that configuration; its feature and parameter
state requires regeneration. `swConfigurationNeedsUpdate="NO"` states that
its cached state is current. `swConfigurationFlags` and
`swConfigurationAlternateName` are configuration-manifest metadata.

Keywords `Feature` elements use the `Type` attribute as their operation-family token. All feature instances with the same exact `Type` token use the same feature-input class. A directly object-ID-bound class instance therefore supplies the class of the other instances carrying that token. `Helix/Spiral`, `Surface-Sweep`, and `Thicken` denote helix, surface-sweep, and face-thickening operations independently of the localized display name in `Name`.

Sketch relations use named scalar records with reference cells at fixed scalar-record slots. Point references use `d6 80`, `b2 81`, `cc 80`, `52 81`, `7b 83`, `7c bc`, or `dd 81`; line references use `e1 80`, `86 83`, `87 bc`, or `e7 81`. Point-point, line-line, and point-line distance relations follow from the operand pair. Two `b2 81` cells carry a point-point distance; their u16 addresses are zero-based ordinals over the coordinate-bearing point and constrained-point markers in the owning feature, in marker order. Two `dd 81` cells carry a point-point distance over the same coordinate-point roster. Two `e7 81` cells carry a line-line distance over the solver-line roster defined below. A `dd 81` cell followed by an `e7 81` cell carries a point-line distance. Two `52 81` or two `cb 8d` cells carry horizontal or vertical point-point distance according to the relation declaration. Two `da 8d` cells carry an angular relation. An `sgCircleDim` declaration followed by one `cc 80`, `fe 83`, `b6 8a`, `9d 92`, or `69 bd` cell carries a diameter dimension. A relation declaration owns the first following scalar within 128 bytes in the same feature interval when its operand signature matches. A scalar claimed by zero or multiple declarations remains native. Adjacent scalar records can continue an anchored relation only within the same feature interval, with the same operands, and without an intervening relation declaration; after a complete relation, an adjacent driving scalar starts a new instance with the anchored family. A display-role scalar names an existing dimension owned by the same sketch when no driving relation or earlier display-only relation claims that dimension. The relation family supplies the dimension unit independently of the display scalar value. Scalar records with the same owning sketch, relation family, and ordered operand sequence belong to one relation instance. Display-role and driving-role scalars are distinct. A unique driving scalar stores the target parameter. An operandless driving scalar separated from its display records by another complete relation belongs to the unique unresolved relation with the same owning sketch and dimension name, unless the dimension's display states the other unit than the relation's family: a display with an angular unit is an angle, and another dimension display is a length.

A shifted value-only scalar record has a 35-byte trailer after its little-endian f64 value. The trailer starts with three zero bytes, a little-endian u32 object identifier at trailer +3, fourteen zero bytes at trailer +7, `01 00 00 00 02 00` at trailer +21, a role byte at trailer +27 (`0` driving or `1` display), and seven zero bytes at trailer +28. Four zero padding bytes precede the value after the 14-byte value-only header. The fixed trailer prefix is `feature_input_shifted_scalar_trailer`; zero or more 12-byte operand cells follow it.

An explicit scalar feature reference assigns the scalar to that feature independently of top-level feature-object byte boundaries. Byte-interval ownership applies only when the scalar has no feature reference. A driving scalar named by a declared sketch dimension stores an angle when the dimension expression has an angular unit and otherwise stores a length when the expression is a dimension display. Native lengths are metres and convert to millimetres in CADIR. A diameter display retains its diameter role after conversion.

When an `e1 80` or `e7 81` cell of a line operand does not select an explicit curve marker, the cell's u16 value is a zero-based index in a solver-line roster. Coordinate-bearing point and constrained-point markers owned by the feature form that roster in marker order: each consecutive pair defines one line. The addressed pair is valid when its transformed endpoint pair is unique and nondegenerate. For a line-line relation, both addressed pairs must be unique, nondegenerate, parallel, and have a perpendicular separation equal to the relation's driving distance. For a point-line relation, the point operand and addressed pair must have a perpendicular separation equal to the relation's driving distance.

A curve marker selected by a family-scoped line operand supplies its roster-arbitrated endpoint pair as the first candidate. When that pair fails the relation family's evaluated distance or angle invariant, the addressed solver-line pair is evaluated. A relation remains native unless one candidate interpretation supplies the required distinct, nondegenerate entities and satisfies the stored invariant.

For an `fe 83` circular-dimension operand, the one-based operand index selects a distance-relation marker by object index. Both links of that marker select the same coordinate-bearing center marker. The next coordinate-bearing marker in the same feature object is the radial point. The finite nonzero center-to-radial distance is the circle radius. Other coordinate-bearing markers in the feature object do not participate in this center/radial roster.

An `fe 83` circular-dimension object can instead end its coordinate roster with a radial point that has no local identifier. The center is the unique earlier coordinate-bearing point whose distance from that terminal point equals the stored radius. Zero or multiple radius-compatible points leave the center unresolved.

Two `00 81` or `0f 82` reference cells in a driving scalar address solver-point indices through their u16 entity-index fields. The scalar value is the Euclidean distance in metres between those points. Solver points occupy the finite coordinate positions of coordinate-bearing markers in the owning feature object. A solver-point index has a resolved position when the complete connected point-distance graph has at least one satisfying assignment and arc consistency leaves one position for that index. A coordinate-roster ordinal beyond the explicit marker roster uses the resolved solver-point position with the same index.

In a Hole Wizard position sketch, `52 81` marker addresses are opaque point-node identities shared across its dimensional relations. Matching bore-cylinder axes and the sketch origin are candidate node loci. A placement set is defined when exactly one set of loci admits an injective node assignment satisfying every driving point distance, horizontal distance, and vertical distance. Horizontal and vertical relation axes may be exchanged as a pair to account for the native-to-sketch coordinate ordering. The bore origins are projected onto the sketch plane and the placement direction is the sketch normal. Multiple satisfying bore sets leave the placements unresolved.

A Hole Wizard position sketch may instead store each authored position as the coordinate pair of an object-indexed line-or-circle or arc marker. The complete marker-position set selects bore axes of the hole radius when exactly one set of consistently directed parallel axes is congruent to it: a bijection must preserve every pairwise distance. A uniquely congruent curve-marker set has precedence over paired object-locus point handles in the same sketch. Coaxial cylindrical surfaces form one candidate axis. A Hole Wizard temporary-axis record selects the surface direction on each carrier. Without that record, oppositely directed surfaces on the same axis leave the carrier unresolved. Multiple congruent axis sets or an incomplete marker set leave the placements unresolved.

When exactly one active Hole Wizard operation has a given diameter, its typed position-sketch plane owns every reversed cylindrical face of that radius whose carrier is normal to the plane. A reversed cylinder bounds excluded material; a forward cylinder bounds material on its radial exterior. Each owned carrier intersects the position plane at the placement origin, and the position-sketch normal is the placement direction. Another active hole with the same diameter, a nonparallel carrier, an inconsistent position plane, or an empty carrier set leaves this ownership form unresolved.

When a typed position-sketch frame does not own carriers through plane incidence, its Hole Wizard operation owns every reversed cylindrical carrier of the hole radius when exactly one active operation has that diameter. Coaxial face segments are one carrier. Each distinct carrier supplies one placement axis. Another active hole of the same diameter or an empty carrier set leaves this ownership form unresolved.

A spatial Hole Wizard position sketch may contain construction points, relation handles, and paired points on bore axes. An object-indexed coordinate-bearing marker selects a placement only when its model-space position lies on exactly one cylinder carrier of the hole radius. The cylinder origin and axis define the placement. Multiple markers selecting the same carrier collapse to one placement. Markers selecting no carrier are construction geometry. A marker selecting multiple distinct carriers leaves the placements unresolved.

Identical blind drilled-hole siblings with nonempty exact placements partition the remaining reversed cylindrical carriers of their primary diameter when each sibling's seeds have one direction, sibling directions are distinct, and every residual carrier has exactly one seed direction. Placements from other complete holes of the same diameter claim their matching carriers before the partition. An unplaced same-diameter hole, an unowned carrier direction, a shared seed direction, or a seed absent from the carrier set leaves the sibling placements unexpanded.

A `7b 83` point reference is qualified by its local identifier. The identifier can select a point, constrained-point, line-or-circle, or arc marker.

A `7c bc` point reference first selects the sole coordinate-bearing point, constrained-point, line-or-circle, or arc marker whose object index equals the address. Relation markers sharing that object index do not participate. Without an indexed coordinate marker, the reference selects a point or constrained-point marker with the addressed local identifier, then the in-range zero-based point-family marker ordinal in byte order. When the ordinal is out of range, one line-or-circle or arc marker with the addressed local identifier supplies the qualified point. For a curve marker selected by either tag, the marker's stored coordinate is the point locus; the curve marker retains its curve identity independently of that qualified locus.

In the compact legacy prefix, a coordinate-bearing kind `1` marker with `04 00 02 00` at marker +23 and profile role u16 `1` at marker +27 is a line whose start is its own coordinate and whose end is the coordinate of the immediately following sketch marker in the same feature object. The following marker retains its independent geometry identity; its coordinate also serves as the line endpoint.

An `87 bc` curve reference first selects the sole coordinate-bearing line-or-circle or arc marker whose object index equals the address. Without an indexed coordinate marker, the reference selects a curve marker with the addressed local identifier, follows one reference-bearing marker with that identifier to its unique curve target, then uses the in-range zero-based curve-family marker ordinal in byte order.

A coordinate-less point or constrained-point handle used by a dimensional relation can be relation-qualified. When exactly one point-point, horizontal-distance, or vertical-distance operand has a profile locus and the stored distance selects one physical coordinate in the complete owning sketch under the relation's Euclidean, horizontal, or vertical metric, that coordinate is the other operand's construction-point locus for that relation. Coincident profile loci at the selected coordinate are one physical-coordinate match. The relation-qualified locus does not assign a global position to the native handle and is distinct for each relation and operand position. A point marker may also carry a non-point dimension or reference carrier; the relation-qualified point remains a separate construction entity.

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

For horizontal and vertical point-point dimensions, the first and second feature-input coordinate fields are the native horizontal and vertical axes. A unique signed-axis transform maps the declared native axis to profile `u` or `v`; a resolved operand identifies the sole distinct profile locus in the complete owning sketch whose displacement on that mapped profile axis equals the stored distance. When neither operand resolves directly, the operands identify the sole unordered profile-locus pair with that mapped-axis displacement. A transform that mixes the declared axis, zero compatible loci, or multiple compatible loci leaves the missing operands unresolved.

When both horizontal- or vertical-distance references identify loci whose mapped-axis displacement differs from the stored distance, each referenced locus independently selects its sole distinct locus at that displacement. The dimension uses the resulting pair only when these searches produce one unique unordered pair. The repaired pair retains a referenced locus; ambiguity, an axis-mixing transform, and unrelated scalar-compatible pairs leave the relation native.

A relation instance without a driving scalar carries a measured value in its display scalar. The decoder uses that scalar's attached name to identify a same-named parameter owned by the same sketch feature only when the parameter's typed value matches the relation-family value. If no unique matching parameter exists, the decoder materializes a reference parameter keyed by the relation and display scalar; its `native_ref` is absent because a display scalar is not writable. The reference parameter has `driving=false` on its relation constraint. A typed constraint requires unique geometry operands and agreement with the measured value; otherwise the native relation and its operands remain retained.

Distance, horizontal-distance, vertical-distance, and circular-dimension driving scalars store metres. Angular driving scalars store radians. These relation-family units apply independently of the owning Keywords dimension's expression spelling.

A bare integer Keywords dimension bound to a unique driving distance or circular-dimension scalar denotes millimetres rather than a discrete count. The scalar supplies its evaluated length and native identity while the original expression remains unchanged.

A bare `0` or `1` Keywords dimension bound to a unique driving distance or circular-dimension scalar denotes millimetres rather than a Boolean. The scalar supplies its evaluated length and native identity while the original expression remains unchanged.

A bare integer Keywords dimension bound to a unique driving angular scalar denotes milliradians rather than a discrete count. The scalar supplies its evaluated angle in radians and native identity while the original expression remains unchanged.

A bare `0` or `1` Keywords dimension bound to a unique driving angular scalar denotes milliradians rather than a Boolean. The scalar supplies its evaluated angle in radians and native identity while the original expression remains unchanged.

An otherwise untyped bare Keywords integer, including `0` and `1`, is a dimensionless integer. The case-insensitive literals `true` and `false` are Boolean values.

A uniquely owned feature-input scalar is the evaluated value of the same-named Keywords dimension. Length-valued feature scalars store metres and angular feature scalars store radians. Keywords dimension text remains the parameter expression; its unitless numeric spelling does not replace the evaluated scalar. Feature operation semantics use the evaluated scalar converted to millimetres or radians.

`VarFillet_c` is a fillet feature class. Its canonical endpoint-radius dimension names are `D0`, `D01`, `D02`, and so on, with the decimal suffix after `D0` supplying the zero-based dimension ordinal. An older lane may encode ordinal `1` as `D1`; inside `VarFillet_c`, that name is a variable-radius control and is not the constant-radius `D1` of an ordinary fillet. These radius-displayed dimensions are length-valued feature scalars. Each named dimension precedes its three-reference control vector. Exactly one reference in that vector contains instance token `8083`; that hop's type signature is the controlled vertex identity. An older edge-control vector may omit `8083` and contain three edge references; when the feature has exactly the two ordered controls `0` and `1`, their radii form the feature-wide profile for endpoint-less selected-edge vectors. A selected-edge reference-list vector contains two `8083` endpoint references in parameter order. The declared reference count frames the roster even when its references have no sentinel or padding. Matching endpoint type signatures to the control vectors assigns the evaluated vertex radii to parameters `0` and `1`. Selected edges with equal ordered endpoint radii form one variable-radius group. When every endpoint-bearing selected edge has one common ordered radius pair, that feature-wide profile also applies to selected-edge vectors that omit endpoint references.

A Keywords dimension name `TXD` followed by one or more decimal digits is formatted dimension text. A `TXD` value that is one finite scalar has that scalar's neutral value. A non-scalar `TXD` value with one or more complete angle-bracket formatting tags has a string value. The tags and surrounding text remain unchanged in the parameter expression.

A nonempty Keywords parameter value with no scalar literal, operator, grouping delimiter, function delimiter, or definite parameter-reference spelling is literal text. Leading and trailing whitespace are not part of the text value. A double-quoted identifier, an identifier containing `@`, and `D` followed only by decimal digits remain parameter-reference spellings.

A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot `N`. Scalars from configuration-scoped lanes do not replace the document-level parameter value or its native identity. Every evaluable document expression and every scalar resolved in the scoped lane contributes its typed value to that configuration's parameter state. A dependency-free document expression has the same evaluated value in every resolved configuration unless that configuration supplies a typed scalar override.

The same lane supplies configuration-local feature operation state. Feature classes, operation discriminators, compact termination records, profile adjacency, path references, and selection records are evaluated within their `Config-N-ResolvedFeatures` lane. Sketch geometry and relation records in the lane belong to the configuration-local sketch owner and sketch identity. Configuration-local records do not define document-global feature semantics unless every applicable lane yields the same state. A resolved configuration carries one evaluated feature state for every document feature.

A configuration-local offset datum plane that omits its support reference, or retains a valid support frame with an unresolved face selector, reuses the document-level datum reference when that reference is resolved and its support frame matches. A feature reference includes its preceding datum-plane dependency. A resolved face reference preserves its selected face identity. A missing or nonmatching frame remains unresolved.

A Keywords configuration's decimal `id` attribute is the slot identity for `Config-N-ResolvedFeatures`. It is independent of `Config-N-Partition`. The stored configuration id selects the same-numbered resolved-features lane before regeneration ordinal fallback. A configuration with an explicit stored id does not participate in ordinal fallback.

When exactly one configuration is active and no configuration-scoped lane supplies its state, the document-level feature and parameter projections are its evaluated state. Its snapshot contains the final document-level state of every feature and every parameter with an evaluated value. Configuration-scoped lanes take precedence and supply independently evaluated snapshots.

An unquoted Keywords expression identifier does not begin with an ASCII digit or `.`. A numeric-leading token is a numeric literal, including when a parameter in the same owner has that token as its name. A numeric parameter name is referenced with double quotes.

Keywords length literals use the suffixes `uin`, `mil`, `mm`, `cm`, `in`, `ft`, `nm`, `um`, `µm`, `μm`, `Å`, `A`, and `m`. Their millimetre scale factors are respectively `0.0000254`, `0.0254`, `1`, `10`, `25.4`, `304.8`, `0.000001`, `0.001`, `0.001`, `0.001`, `0.0000001`, `0.0000001`, and `1000`. A unit suffix is part of the numeric literal and determines its length dimension before expression evaluation.

Point-reference cells address feature-local object indices within the owning feature object. A reference resolves when that object index is unique in the feature object. Marker trailer local identifiers are a separate namespace used by operand tags whose layout specifies local-identifier resolution.

Operand tags `80d6`, `80cc`, `837b`, `8ab6`, `8dcb`, `929d`, `bc7c`, and `bd69` select point loci, including the point-qualified curve forms defined above. Tags `80e1`, `8386`, `83fe`, `8dda`, and `bc87` select line, circle, or arc markers. In a declared `sgPntPntDist`, `sgPntPntHorDist`, `sgPntPntVertDist`, or `sgAnglDim` relation, tag `80d5` is the ordered two-operand point or relation-marker form. In a declared `sgPntPntDist` relation, tags `8138` and `80ac` are additional ordered two-operand point or relation-marker forms. In a declared `sgLLDist` relation, tag `810f` is the ordered two-operand line or relation-marker form. The relation class selects the operand family; these tags do not change the generic object-index and local-identifier rules.

A uniquely linked reference-handle chain resolves to its terminal profile locus. Linked loci intersect a non-unique coordinate-derived locus or entity set for the same handle; a single remaining locus or entity resolves the handle. Cyclic chains and chains with multiple terminal loci remain unresolved. A relation handle whose linked chains identify one common profile entity identifies that entity.

Constraint incidence is bidirectional. A coordinate-bearing geometry marker owns a relation marker when one of its resolved local-link cells targets that relation marker. Native constraints retain these reverse owners in marker order separately from the relation marker's forward reference-handle operands.

A geometry marker's link to a relation marker records constraint incidence, not locus equivalence. Locus resolution does not traverse from a geometry marker through the relation marker to another operand of that relation. An axis relation may traverse its non-owner links through intermediate relation handles; those handles are not geometric operands, and the traversal collects only point or constrained-point loci reached through the relation graph and reverse point incidence.

Feature-input geometry-handle coordinates and the nested Parasolid profile differ by a signed axis permutation and constant translation per sketch feature. A unique transform mapping at least two distinct geometry-handle coordinates onto compatible profile anchors binds every matching geometry or relation marker coordinate to those loci. A projected entity whose native reference is the coordinate marker supplies a direct anchor; a unique transform over direct anchors takes precedence. Profile loci are the primary unbound anchors. When they do not determine a frame, point handles admit entity endpoints and centers, line-or-circle handles admit line endpoints, midpoints, and circular centers, and arc handles admit arc centers. Relation-marker coordinates do not participate in selecting the frame. The identity axis permutation has precedence when it has a unique translation. When equally scoring signed axis permutations include zero-translation transforms, translated transforms are excluded. A reference marker whose linked endpoint markers share one profile entity identifies that entity.

A point-point dimension is a full line-length dimension when one point operand directly owns one bounded profile line, the other point operand is the sole linked operand of a `Distance` relation marker, one stored coordinate delta is half the dimension value, and the owned line length equals the dimension value. The neutral dimension constrains the two endpoints of the owned line.

When profile anchors do not determine the transform, planar sketch placement supplies it. The two feature-input coordinate fields are the model-coordinate components whose axes are not the dominant component of the sketch normal. The omitted model-coordinate component is the unique value on the sketch plane. Subtracting the sketch origin and projecting the resulting model-space vector onto the sketch `u` axis and `normal × u` axis yields the local sketch coordinate. Axis-aligned placements reduce to a signed axis permutation and translation.

Multiple valid signed-axis transforms bind a marker when every transform maps that marker to the same normalized profile locus set. A transform-dependent marker remains unresolved.

A primary line-or-circle geometry handle on a transformed line segment identifies that line entity. Line-interior entity anchors are scoped to primary line-or-circle geometry records and cannot satisfy point operands or display handles. A curve handle matching an endpoint or center locus identifies the owning curve entity rather than an endpoint locus.

When a point or constrained-point marker maps to a shared profile coordinate, its incident start and end loci are geometrically equivalent. The lexicographically first locus is the canonical operand. Line-or-circle and arc markers retain every compatible entity at a shared coordinate.

A coordinate-less point or constrained-point marker linked to two or more resolved profile entities identifies their endpoint only when every linked entity has exactly one common stored endpoint coordinate. The lexicographically first locus at that coordinate is canonical. Center loci, analytic intersections away from stored endpoints, multiple shared endpoints, unresolved links, and links spanning sketches leave the point marker unresolved.

Point-distance operands select explicit profile loci. Line-distance and angular operands select the profile entity shared by their linked endpoint markers. A line handle with two forward links can use any coordinate-bearing marker as an endpoint when that marker's coordinate has one materialized point locus; the two distinct linked loci select the unique bounded profile line containing both points. A direct point locus on the line handle does not replace this line identity. An `8386` line operand can address a non-relation handle by local identifier or object index when that handle has exactly two distinct forward-linked coordinate-bearing markers. A relation marker can supply the same line identity when its object index or local identifier selects it and its two forward links are distinct coordinate-bearing point or constrained-point markers. A coordinate-bearing line operand with one forward coordinate-bearing point link and additional relation-incidence links uses its own coordinate and that point as the line endpoints. A self-linked coordinate-bearing line operand uses its own coordinate and its one other coordinate-bearing link in the same way. A non-dimensional relation's point-locus roster is the unique loci selected by its forward local links plus coordinate-bearing markers whose local link targets the relation marker. Three-operand symmetric and at-intersection relations therefore combine the reference marker's two forward links with one reverse-owned locus. A relation with resolved operands and one driving scalar maps to the corresponding neutral distance, horizontal-distance, vertical-distance, angle, radius, or diameter constraint. Relation-marker coordinates do not identify constraint operands. A reference identifier equal to the relation marker's local identity identifies the relation marker itself, even when the generic identifier resolver selects another marker with the same value; it is not constraint incidence. The relation marker's object index is not a constraint-owner discriminator. A relation marker without another linked marker or a reverse geometry owner does not produce a sketch constraint. A known relation whose completely resolved neutral entities cannot belong to the relation's required operand family is inactive. Its native definition and operands remain retained.

A dimensional relation instance carries authoritative operand identities whenever every stored operand resolves to the relation family's required locus or entity family. Paired display and driving scalars repeat the same ordered operands. Point-point distance uses Euclidean locus distance; horizontal and vertical distance use absolute displacement on the profile axis mapped from the declared native axis by a unique signed-axis transform; point-line and line-line distance use perpendicular distance; angle uses the unsigned line direction angle; radius and diameter use the circular entity radius. A typed relation whose evaluated measurement differs from the driving scalar is inactive. Evaluated measurement resolves only missing or ambiguous operand identities; the relation remains native when no unique satisfying assignment exists. An unresolved `8386` address that uniquely identifies a coordinate-bearing point marker restricts its line candidate to bounded profile lines containing that point. The stored distance must leave one candidate relative to the resolved line operand. When distinct line operand identifiers resolve through compatibility fallback to the same line, the resolved line and its unique partner at the stored distance are the ordered operands.

Horizontal and vertical relation codes determine the native coordinate axis and are not inferred from evaluated geometry. A unique signed-axis transform maps that native axis to profile `u` or `v`, and the neutral definition uses the mapped profile axis. A transform with a mixed axis, or multiple transforms that map the native axis differently, retains the relation native. An axis relation with intermediate relation handles maps to a two-point relation only when exactly two distinct point loci resolve through the handle graph. An axis relation with no reverse geometry owner and exactly two forward point or constrained-point links uses both links as operands even when one link's local identifier equals the relation marker's local identifier or object index. A coordinate-bearing target selects its unique same-native point entity. A coordinate-less point proxy selects its unique linked point locus. The typed relation is inactive when its evaluated operands do not satisfy the mapped profile-axis displacement.

Parallel, perpendicular, collinear, tangent, equal-size, and concentric relations require the resolved entities to satisfy the corresponding evaluated geometric invariant. Parallel and perpendicular compare line directions; collinearity additionally requires zero line separation; tangency compares line-to-circle or circle-to-circle contact; equal-size compares line length, circular radius, or both ellipse radii; concentricity compares centered-entity centers. Unsupported entity-family combinations and geometrically inconsistent operands retain native relation semantics.

Coincident relations require every resolved locus to evaluate to one sketch coordinate. A midpoint relation requires its point locus to evaluate to the midpoint of the resolved bounded line or circular arc. A symmetric relation with two point loci and one line-entity locus uses the line as its axis; the point projections onto the axis are equal and their signed perpendicular distances are opposites. An at-intersection relation has one point locus and two distinct entity loci. The point must lie within both bounded line, circle, circular-arc, or ellipse domains. Fixed arc-angle codes `18`, `19`, and `20` require the resolved arc sweep to equal π/2, π, and 3π/2 radians respectively. Identity-resolved operands whose evaluated geometry does not satisfy the relation retain native relation semantics.

`Helix/Spiral` history records use positional dimensions when explicit axis placement is absent: `D3` is the signed total axial rise, `D4` is the signed axial rise per revolution, `D5` is the positive revolution count, and `D7` is the start angle. The history record owns the unresolved construction axis and radius.

The corresponding feature-input object contains a nested schema-format `13006` neutral-binary Parasolid construction curve. Its wire `BODY` has `body_type=2`. The embedded BODY schema supplies the effective field sequence, including inserted fields; `BODY.boundary_curve` selects the boundary-curve chain. Each eligible chain member is type 134 `B_CURVE`; its `nurbs` reference selects type 136 `NURBS_CURVE`, whose `bspline_vertices` reference selects type 45 `BSPLINE_VERTICES`. The scalar count equals `n_vertices * vertex_dim`; XYZ construction requires `vertex_dim=3`, a false rational flag, and finite coordinates. Stored small XMT values are logical node indices plus one. `00 22` is stored XMT 34, not an array tag or a curve selector. Reference identity selects the array independently of its size or position. Every wire BODY in the feature-input stream contributes its boundary-curve path; record order does not select one.

A BODY-leading schema-13006 construction stream has an eight-byte prefix immediately after its schema token: u16 BE maximum node type at `+0`, u32 BE zero user-field width at `+2`, and u16 BE BODY type `12` at `+6`. Embedded BODY schema data follows this prefix.

First-of-type schema data occurs between the u16 BE node type and its payload. The matching-base-schema marker is `ff`; later nodes of the same type omit it. A fixed-node payload begins with its XMT identity. A variable-node payload begins with a u32 BE element count and then its XMT identity. The BODY schema edit sequence applies Copy, Delete, Insert, and Append operations to the base effective fields and ends with `Z`. Unknown or inconsistent schema edits supply no construction path. A missing or wrong-type reference, duplicate reachable identity, invalid count or dimension, non-finite coordinate, boundary cycle, or more than one complete eligible path supplies no unique construction curve. An unreferenced finite array supplies no path.

With short XMT and pointer fields, the B_CURVE payload is 21 bytes: XMT at `+0`, u32 node ID at `+2`, attribute pointer at `+6`, owner at `+8`, next at `+10`, previous at `+12`, geometric owner at `+14`, sense byte at `+16`, NURBS pointer at `+17`, and auxiliary data pointer at `+19`. Its sense is ASCII `+` or `-`. The NURBS_CURVE payload is 25 bytes: XMT at `+0`, u16 degree at `+2`, u32 vertex count at `+4`, u16 vertex dimension at `+8`, u32 distinct-knot count at `+10`, knot-type byte at `+14`, periodic/closed/rational logical bytes at `+15`/`+16`/`+17`, curve-form byte at `+18`, control-vertex pointer at `+19`, multiplicity pointer at `+21`, and knot pointer at `+23`. Larger pointer indices use the variable-width XT pointer encoding and shift later fields. Curve-form values are unsigned bytes; the base enum names `1` as unset and `3` as polyline. The type-45 variable count/XMT prefix is the [B-spline array header](../layouts/sldprt.md#bspline_array_header).

Type 45 stores NURBS control vertices, which need not lie on the represented curve. An unset curve form does not declare a polyline. Model-space coordinates in these embedded SolidWorks XT streams are metres; multiplication by 1000 converts them to millimetres. A circular-helix fit of the ordered construction controls is approximate. Its maximum absolute coordinate residual is bounded by `5e-4` times the fitted radius. The fitted unit axis is the normalized cross product of the cosine and sine fit vectors; no residual-dependent component snapping applies.

An `moCurvePattern_c` feature-input object is immediately preceded by its seed feature object and followed by its path feature object. The preceding object identifies the repeated neutral feature. When the following object is an `moProfileFeature_c` sketch with one resolved neutral sketch, that sketch is the curve-driven pattern path. Seed and path resolve independently. A missing or multiply addressed adjacent object leaves its operand unresolved; a following object that is not a resolved sketch leaves the path unresolved.

An `moLineRef_w` declaration has two direction layouts. When two consecutive `c7 cf ff ff` words occur at declaration offsets `+136` and `+140` and `f8 2a 00 00` occurs at `+148`, three little-endian f64 values at `+200`, `+208`, and `+216` store its unit xyz direction. When three consecutive `c7 cf ff ff` words occur at `+144`, `+148`, and `+152` and `f8 2a 00 00` occurs at `+160`, the unit xyz direction is at `+220`, `+228`, and `+236`.

A declared line-reference interval may instead contain two consecutive `c7 cf ff ff` words followed by a zero u32, a nonzero stream address, 16 zero bytes, and seven finite little-endian f64 values. The seven-scalar form continues with 16 zero bytes, u32 values `1` and `1`, 12 zero bytes, a non-sentinel high-bit u16 instance token, 16 zero bytes, and `ff ff`. Its final three scalar values are the unit xyz direction. Each structurally complete form in declaration order supplies one direction.

A compact line reference starts with two consecutive `c7 cf ff ff` words and one zero u32. Addressed forms then store a nonzero u32 stream address. One layout continues with 16 zero bytes and nine finite little-endian f64 values, followed by u32 values `1` and `1`. Its trailer is either 16 zero bytes or 12 zero bytes followed by a non-sentinel high-bit u16 instance token. A seven-scalar form with the same 16-byte prefix is selected by a nonzero eight-byte cell after the seventh scalar or by a tagged trailer: either 34 zero bytes, or 14 zero bytes followed by u32 values `1`, `1` and 12 zero bytes, then a non-sentinel high-bit u16 token, 16 zero bytes, and `ff ff`. A six-scalar layout continues with 24 zero bytes, six finite little-endian f64 values at offsets +40 through +87, 16 zero bytes, u32 values `1` and `1`, and 24 zero bytes. Shifted layouts continue with eight zero bytes and begin their scalar sequence at +24. The shifted nine-scalar trailer is u32 values `1`, `1`, 12 zero bytes, a non-sentinel high-bit u16 token, 16 zero bytes, and `ff ff`. A shifted seven-scalar trailer is either 36 zero bytes followed by that token-and-`ff ff` suffix or eight zero bytes followed by two nonzero u32 values. Addressless forms store zero in the stream-address field. One continues with eight zero bytes and seven finite scalars at +24 through +79; a zero u32 terminates the object or precedes its high-bit instance token. The other continues with 16 zero bytes, seven finite scalars at +32 through +87, 16 zero bytes, u32 values `1`, `1`, and 24 zero bytes. In every layout, the final three scalar values are the unit xyz direction. If an unshifted compact body has zero at the byte after the eighth scalar and both the eight-scalar final triple and the overlapping nine-scalar final triple are unit vectors, no width is selected by geometric plausibility; the handle contributes a direction only when both triples are identical. Distinct candidates leave the handle unresolved and never produce two references. Handle words belonging to a declared `moLineRef_w` layout are not a second compact reference.

An `moLPattern_c` interval without a line-reference record carries each displayed translation direction in the corresponding `D3` or `D4` dimension instance. Relative to the dimension's UTF-16 name marker, a positive little-endian f64 spacing in metres is at `+32` and the unit xyz direction is the unaligned little-endian f64 triple at `+161`, `+169`, and `+177`. The stored spacing identifies the dimension instance against the history parameter. `D3` is the first direction and `D4` is the second. A missing, duplicate, non-unit, or spacing-inconsistent instance does not supply that direction.

An `moLPattern_c` feature-input object is immediately preceded by its seed feature object. That preceding object identifies the repeated neutral feature. An immediately following `moDerivedCosmeticThread_c` object is the pattern's generated child and does not terminate the pattern interval; in this form the nearest preceding `moCosmeticThread_c` history feature is the seed. One structurally valid declared or compact line reference before the next feature object supplies the first linear-pattern direction. Two ordered references supply both directions. When no line-reference layout is present, the `D3` and `D4` display-dimension instances supply the corresponding directions. `D1` and `D3` carry the first count and spacing, while `D2` and `D4` carry the second count and spacing. A unique `moNumberDim_c` and `ParallelPlaneDistanceDim_c` pair may instead carry the first count and spacing. The first parameter-name marker within 128 bytes of each declaration identifies its history parameter; the count is a positive integer and the spacing is a positive length in millimetres. More than two line-reference directions leave the corresponding linear-pattern input unresolved. A missing or multiply addressed preceding object leaves the seed unresolved.

An `moCirPattern_c` interval retains its repeated seed through `moCirPatternSurfIdRep_c` generated-surface identity records. A complete identity path whose first component signature identifies the consuming circular-pattern object and whose terminal `feature_source_id` identifies one unique earlier history feature binds that feature as the seed. All matching identity records must agree on that terminal feature; otherwise the seed remains unresolved. The interval's axis is one complete `moTempAxisRef_w` nine-scalar carrier in the same feature-input object interval. The declaration starts with `ff ff 01 00`, name length `15` at `+4`, and class name `moTempAxisRef_w` at `+6`. Its fixed carrier occupies offsets `+0..+311` after the class declaration; the next class declaration starts at `+312` after zero padding of at most 24 bytes. The first xyz triple at `+239` is the axis origin in metres and the final xyz triple at `+287` is the unit axis direction. All complete carriers in the interval must agree on the origin and direction. A missing or conflicting carrier leaves the circular-pattern axis unresolved. A resolved axis combines with positive `Angle` and `Count` parameters to form the neutral circular pattern.

An `moMoveFace_c` feature with exactly one `moDirectionSpec_c` declaration and exactly one `moLineRef_w` declaration in its object uses translation mode. Its `D1` scalar is a length stored in metres. The unique unit direction from the declared or compact line-reference layouts is the translation direction, and `D1` is the translation distance. A missing declaration, repeated declaration, or non-unique line-reference direction leaves the motion unresolved.

An `moMoveCopyBody_c` non-copy translation owns one `moMoveCopyBodyData_c` child. The child stores a positive u32 body count followed by that many nonzero u32 feature-local body identities, `ff ff ff ff`, and eight zero bytes. Nine little-endian f64 values form an identity 3 by 3 matrix. Little-endian u64 `1`, three finite origin coordinates, f64 `1.0`, and three finite translation coordinates follow. Coordinates use metres. The non-copy trailer at matrix offset +200 is `01 00 00 00 00 00 01 00`. A non-identity matrix or a different trailer does not use this translation form.

An `moMirrorPattern_c` or `moMirrorSolid_c` seed vector uses the duplicated 16-byte component marker. Marker −12 stores a little-endian u32 slot count from 2 through 65, marker −8 through marker −1 are zero, and marker +16 through marker +17 are zero. The count either equals the component count or includes one or two root slots that have no component. A component is a tagged 20-byte path entry or an anonymous 16-byte entry consisting of the 12-byte type signature and u32 local id. Tagged-only paths use the edge component-path separator grammar. Mixed paths use zero or `ff` word fill of 4, 8, or 12 bytes; a root boundary within a mixed path is `01 00 00 00` followed by four or six zero bytes. The path is valid only when the count, entry widths, separators, and root slots yield one complete parse. The last uniquely identified history feature traversed by each path is its mirror seed. A terminal component whose signature has no history feature does not invalidate the preceding seed identity. Separate paths supply an ordered seed set. An immediately following `moDerivedCosmeticThread_c` object is the mirror's generated child and does not terminate the mirror interval; when the interval has no seed vector, the nearest preceding `moCosmeticThread_c` history feature is the seed. The seed set and mirror plane resolve independently. Multiple configuration lanes must yield the same ordered seed set.

The mirror-plane surface vector uses the same duplicated marker with a component-vector selector whose byte 1 is `02`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero at marker −8. Its little-endian u32 count at marker −12 either equals the mixed component count or includes one root slot that has no component. The terminal component's feature source identifier and feature-local identity select the surviving face through `FACE_ID_2001`. Exactly one planar face supplies the mirror-plane origin and normal. A proper-rigid transformed plane supplies its transformed origin and normal.

Built-in reference-plane history records have native class `moRefPlane_c` and no dimensions or extra attributes. Within that class, source IDs `2`, `3`, and `4` identify the Front, Top, and Right principal planes.

A classless `Feature` record with native type token `Plane` is a reference-plane record. Legacy compound histories without feature-input classes identify the principal planes by a complete triplet of classless, parameterless, propertyless `Plane` records at source IDs `2`, `3`, and `4`. An incomplete triplet or differing type tokens has no principal-plane identity.

Native-class principal-plane roles require the source IDs `2`, `3`, and `4`. A native-class triplet at source IDs `3`, `4`, and `5` has no principal-plane role. A classless `Feature` triplet with no source IDs uses its ordered records as Front, Top, and Right only when exactly one complete candidate triplet exists; a history with more than one complete candidate triplet does not bind any of them.

Among classless, parameterless, propertyless history records, `Feature` source ID `1` is the annotations container and `Sketch` source ID `5` is the model origin. Other source IDs are positions in an optional-node sequence rather than role codes.

A classless Keywords `Feature` whose `Type` token is `EquationDriven` is the equation container node. The token is a role code and identifies the container independently of its source identifier, its display name, and its dimension children. The container's dimensions are document-global: an unqualified expression identifier that names no dimension of the referencing feature resolves to the same-named container dimension.

The Keywords history stores each dimension's displayed value, not the equation that drives it. The equation manager is in the configuration object stream `Contents/Config-N`, where `N` is a decimal configuration slot. The class declaration `ff ff 01 00 0a 00` + `moRelMgr_c` is followed by a little-endian u16 relation count and that many relation objects, with the objects each relation owns between them. The first relation opens with the class declaration `ff ff 01 00 0c 00` + `moRelation_c`. Every later relation opens with the u16 repeat token of that class, which has bit 15 set. A relation's text follows its class tag as a counted UTF-16LE string: `ff fe ff`, a u8 code-unit count, then those code units. A count byte of `ff` is followed by a u16 count. Six bytes follow the text; byte 1 is `02` or `03` and bytes 3 through 5 are zero. Byte 0 is `00`, `01`, or `08` and byte 2 is `00` or `01`; their meaning is not established. The relation's first equation-side object follows them. The first relation's side opens with the class declaration `ff ff 01 00 13 00` + `moRelEquationSide_c`; because that class is declared directly after the relation class, every later side opens with the relation repeat token plus two. A string behind a class token is a relation only when that side token follows its six bytes.

The relation text is one line of the equation manager, in the manager's stored order. A line that opens with an apostrophe is a comment. Any other line is a double-quoted name, `=`, and an expression; an apostrophe outside double quotes ends the expression and starts a trailing comment. A name containing `@` is `dimension@feature` and the line drives that dimension. A name without `@` is a document-global variable. Names inside an expression are double-quoted in the same two forms. A name may be assigned by more than one line, and a line may read a name that a later line assigns. A dimension name in an expression may differ in letter case from the dimension it names. A relation object may also occur in `Contents/Config-N-ResolvedFeatures` with no manager in that section. A part that stores several configuration object streams stores the manager in each.

A relation's sides are stored as tokens. A token is a non-empty counted string followed by four little-endian u32 values: a lead word, the token's first and one-past-last character position within its side, and a kind. A quoted name has lead word 1; an operator, a number, and a function name have lead word 8; a trailing comment has lead word 4. A name's range is as long as its text. An operator's range can be wider than its text. The assigned side follows the side class tag and a u16 token count. The assigned name is then repeated as a counted string with no token words, and the expression side follows with its own token count.

A global-variable name token directly follows a counted string that holds the variable's evaluated number as decimal text. That string is empty on the assigned side. The number has no unit. A dimension reference opens with the class tag of `moRelDimension_c`, four zero bytes, the class tag of `moDimRefWrapper_c`, and the dimension's value as a little-endian f64: metres, radians, or a count. The dimension's handle objects follow, then its name token. The wrapper class is declared directly after the relation-dimension class, so its repeat token is that class's plus two. On the assigned side the reference follows the side's tag and count. In the expression it follows the previous token's four words.

The name in a token is the name the manager resolved. It can differ from the line's text in letter case, and it differs wholly when the line names a dimension that does not resolve. An assigned name without `@` can be stored as a dimension reference. The value stored on the assigned side is not recomputed from the line and can be the result of an earlier input. In a part that stores several streams it can be another configuration's value while the value stored where the dimension is read is the stream's own, and a whole stream can hold another configuration's values.

A complete native-class roster selects one feature-manager allocation layout. The current layout has `moDocsFolder_c`, `moCommentsFolder_c`, `moSolidBodyFolder_c`, and `moSurfaceBodyFolder_c` at source IDs `7`, `8`, `9`, and `10`. The legacy layout has `moOriginProfileFeature_c`, `moSurfaceBodyFolder_c`, `moSolidBodyFolder_c`, `moDocsFolder_c`, and `moCommentsFolder_c` at source IDs `6`, `9`, `10`, `12`, and `13`. The default-frame layouts have `moDetailCabinet_c`, three `moRefPlane_c` records, and `moOriginProfileFeature_c` at source IDs `1` through `5`. One default-frame layout has classless light nodes at `6`, `7`, and `8`. Another also has `moSolidBodyFolder_c` and `moSurfaceBodyFolder_c` at `7` and `8`, with classless light nodes at `6`, `10`, `11`, and `12`. The origin-at-six layout has `moDetailCabinet_c` at `1`, three `moRefPlane_c` records at `3`, `4`, and `5`, `moOriginProfileFeature_c` at `6`, and classless light nodes at `2`, `7`, and `8`. A layout is valid only when exactly one complete roster matches.

Within the current layout, classless `Feature` source ID `6` is the lights-and-cameras container, `12` is an ambient light, and `13`, `14`, and `15` are directional lights. Within the legacy and origin-at-six layouts, source ID `2` is the lights-and-cameras container, `7` is an ambient light, and `8` is a directional light. Within the default-frame light layout, source IDs `6`, `7`, and `8` have those roles. Within the folders-at-seven layout, source ID `6` is the lights-and-cameras container, `10` is an ambient light, and `11` and `12` are directional lights. Another classless or scene-class light with the same nonempty native kind as the layout's unique ambient-light or first directional-light node has that node's role. A classless payload-free `Feature` at source ID `-1` is the sheet-metal node. A classless `Feature` with an empty name and no payload is the exploded-views container.

`moConstraintCoincLineParallelPlaneRefplaneData_c`, `moConstraintPerpPlnTanOneCylinderRefplaneData_c`, and `moFacePtRefPlnData_c` store a 97-byte constructed reference-plane frame immediately after the class name. `moFixedRefPlnData_c` uses the same 97-byte extent and either the pairwise-orthogonal form or the repeated-normal form. `moDefaultRefPlnData_c` and `moConstraintPrllPlnTanOneCylinderRefplaneData_c` store the 81-byte minimal frame at the same position. `moFaceRefPlnData_c` stores either 97-byte form or the 81-byte form. Three f64 values at offsets `+0`, `+8`, and `+16` store xyz origin coordinates in metres. Three f64 values at `+24`, `+32`, and `+40` store the unit normal. Byte `+48` is `1` in the 97-byte frame. In the pairwise-orthogonal form, unit in-plane u- and v-axes occupy the unaligned f64 triples at `+49`, `+57`, `+65` and `+73`, `+81`, `+89`; the three basis vectors are pairwise orthogonal. In the repeated-normal form, exactly one of those two triples is a unit vector orthogonal to the normal and the other is codirectional or antiparallel to the normal; the orthogonal triple is the plane u-axis. The 81-byte frame has zero bytes at `+48` through `+55`, byte `+56` equal to `0x80`, an f64 `-0.0` at `+57`, the negated origin z at `+65`, and f64 `1` at `+73`. The frame belongs to the immediately preceding feature object and precedes the next feature object.

An `moFaceRefPlnData_c` record may carry the supporting face after its frame as one counted component-path vector. The vector has the duplicated 16-byte component marker, a positive little-endian u32 slot count at marker −12, a component-vector selector whose byte 1 is `02`, whose byte 0 is a lane-specific subtype, and whose final two bytes are zero at marker −8, and two zero bytes at marker +16. Its tagged and identifier-less entries use the compact mixed component-path grammar. The slot count equals the entry count or includes one serialized root slot. Exactly one complete vector in the reference-plane object identifies the supporting face. The final component identifier is the feature-local face identifier, and the final component source is the producing feature.

`moConstraintCoincLineAtAnglePlaneRefplaneData_c` stores a 121-byte matrix reference-plane frame immediately after the class name. Its origin, normal, and byte `1` use offsets `+0` through `+48` of the 97-byte frame. A right-handed orthonormal 3×3 matrix occupies the unaligned f64 fields at offsets `+49` through `+113` in row-major order. The first matrix column is the plane u-axis, and the third column is codirectional with the stored normal.

When no data-class anchor selects a reference-plane frame layout, every structurally valid encoding in the feature interval participates in one uniqueness check. A valid 121-byte matrix frame owns the 97-byte prefix at the same byte offset; the prefix is not a separate fixed frame. A valid 97-byte pairwise-orthogonal or repeated-normal fixed frame is one candidate at its byte offset. A compact candidate whose byte range overlaps a valid matrix or fixed candidate is nested in that frame and is not an independent candidate. Distinct non-nested frames at different offsets leave the reference plane unresolved.

A valid 121-byte angular frame whose byte range overlaps a valid matrix or fixed frame is nested in that frame and is not an independent candidate.

An offset reference-plane object may store two ordered frame records using the same or different frame layouts. The first frame is the constructed plane and the second is its support plane. Their normals are codirectional, and the support origin equals the constructed origin plus `D1` times the constructed normal. A support frame that does not identify another reference-plane object denotes the coincident planar face. When the second frame is omitted, the same translation recovers the support plane from the constructed frame.

The `moConstraintMidPlaneRefplaneData_c` class declaration is followed by an eight-byte body prefix, an f64 geometric tolerance, an f64 signed plane distance in metres, and an xyz unit normal as three f64 values. The body prefix is opaque to plane construction and may be zero or nonzero. The resulting plane satisfies `normal · position = distance`. The record does not store an independent in-plane axis. A coordinate frame chosen to represent this plane is not the profile orientation; an attached profile uses the in-plane basis and origin from its component reference-plane placement.

A reference-axis data class stores an 88-byte axis frame immediately after its class name. `moPlaneInterAxisData_c`, `moSurfaceAxisData_c`, and `moTwoPtsAxisData_c` use this layout. Two endpoint xyz triples occupy offsets `+0` through `+40` as little-endian f64 values in metres. Two finite f64 scalar fields occupy `+48` and `+56`. A unit direction xyz triple occupies `+64` through `+80`; it is codirectional with the endpoint delta. The frame belongs to the reference-axis feature interval containing the data-class declaration. The canonical axis origin is the closest point on the endpoint line to the model origin, expressed in millimetres.

A zero-origin angular reference-plane frame stores redundant normal z and y components as two f64 values, followed by byte `1`, an unaligned xyz unit u-axis, an xyz unit normal, and an xyz unit v-axis. The three vectors begin 17, 41, and 65 bytes after the first redundant component and are pairwise orthogonal. Normal x is zero; the redundant components equal normal z and y. Twenty-four zero bytes and f64 value `1` terminate the 121-byte frame.

A compact reference-plane frame stores xyz origin coordinates in metres at offsets `+0`, `+8`, and `+16`, normal x and y at `+24` and `+32`, and the complete xyz unit u-axis at `+40`, `+48`, and `+56`. Byte `+64` is zero. V-axis x and y are unaligned f64 values at `+65` and `+73`, and byte `+81` is zero. The omitted v-axis z and normal z are the unique values that make u and v unit and orthogonal and make `normal = u × v` while preserving the stored normal components.

Legacy coordinate-frame groups without serialized plane-reference wrappers contain three consecutive `moRefPlane_c` history records followed by three consecutive `moRefAxis_c` records. All six history ordinals and source IDs are consecutive. The axes use ordered plane pairs `(0,1)`, `(0,2)`, and `(2,1)` in axis-record order. The three axis directions form the ordered right-handed relations `d0 = d2 × d1`, `d1 = d0 × d2`, and `d2 = d1 × d0`. If two stored axis lines are perpendicular and intersect, their intersection point and the applicable relation define the third axis line.

Each `PMISemanticDataDB` dimension uses `cadText` value `<dimension-name>@<feature-name>` to identify its owning history parameter. `dimItems` is a one-element array whose element is a `DimSemData` map. The binding is valid when the feature name is unique and all records for the same owner and dimension name encode one equivalent semantic dimension with the same subtype, value, precision, display text, and flags. `Linear`, `Diameter`, and `Radial` values are f64 metres. These values supply history dimensions when the Keywords record omits them; an explicit Keywords dimension has precedence. Precedence decides which dimension a parameter is, not which number it holds. Keywords stores a dimension as the text the document displays: a number in the document's length unit or in degrees, rounded to the display precision, with no unit, so `11` is 11 mm in one document and 10.9985 in in another. Where a `Linear`, `Ordinate`, `Diameter`, `Radial`, or `Angle` record and the Keywords text are the same value, the record supplies the parameter's value and kind: the length in millimetres or the angle in radians, to the record's full precision. They are the same value when the decoded Keywords number times 0.001, 0.0254, or 1 equals the record's metres to one part in 10⁹; when a real Keywords number equals an `Angle` record's radians; or when the number the text opens with, after a `<MOD-DIAM>` or `R` modifier, is the record's value in millimetres, inches, or degrees rounded to the decimal places the text shows. A fraction (`17/32"`, `2 3/4"`) is exact and has no rounding. A parenthesized note after the number is a comment: `<MOD-DIAM>3.7(72t gear)` displays 3.7. Text with an operator (`+`, `-`, `*`, `/`, `^`, `=` or `,`) anywhere else after the number, or with an exponent, is an expression or a raw scalar and is never such a display. A record the Keywords text contradicts leaves the parameter as Keywords gives it; a sheet-metal bend-allowance factor displayed `0.35` beside a `Linear` record of zero is one. The parameter's expression becomes the literal of that value (`279.4mm`, `<MOD-DIAM>12.7mm`, `R25.4mm`, `0.5rad`), keeping the diameter or radius modifier the Keywords text had; the Keywords text itself stays in the native history. The records are the document's, one per dimension, and Keywords, the record and the `ResolvedFeatures` scalar can all describe a configuration other than the one whose body is stored: in a part with several configurations a refined length is exact for what the document displays and is not thereby the active body's. A configuration's evaluated value has the kind of its parameter, so each configuration's value of a refined parameter is kept in step: a value equal to the parameter's displayed value becomes the refined value; a value already of the refined kind is the configuration's own `ResolvedFeatures` length or angle and is kept; a real or integer is that configuration's display, in the unit the parameter's display was in (millimetres, inches, metres, or degrees), and is converted; and when no single unit explains the parameter's display, as for a zero, such a number is removed.

A dimension with no `PMISemanticDataDB` record can still have its exact value in the equation manager of the active configuration's `Contents/Config-N` stream, which stores an f64 beside each dimension a line drives or reads. The stream is the one whose `N` is the active configuration's `SourceIndex` or Keywords `id`; when the active configuration carries neither, no stream is used. The stored value has no subtype: it is metres, radians, or a count. It refines the parameter under a stricter agreement than a record's, for a length only. The value used is the one stored where a line reads the dimension, when every line of the stream that stores one stores the same f64; lines that disagree give none. A dimension for which no reading line stores a value takes the value on the assigned side of the line that drives it, when every driving line stores the same one. The assigned side can hold an earlier input. The stored number is the parameter's length when the decoded Keywords number times exactly one of 0.001 and 0.0254 equals it to one part in 10⁹, or when the number the text opens with is the stored value in exactly one of millimetres and inches, rounded. Keywords drops trailing zeros, so `12` beside `15.88` is 12.00: the rounding allowed is half a unit in the finest decimal place of any parameter text of the document that reads as a number, or in the text's own last place when that is finer, which is what separates an earlier input of 12.3 mm from a display of `12`. A document that holds a full-precision literal or a raw scalar among its parameter texts therefore allows no rounding at all, and its rounded displays stay as Keywords gives them. A fraction is exact. The stored number is not a length when it equals the decoded number itself, which is how a count, a ratio, and radians are stored, or when both units explain it, as for a length near zero. A rounded display also has to exclude the other readings: when the text at its own places equally shows the stored number itself or the stored number in degrees, as `0.02` shows both 1/64 in and 0.0227°, the parameter is left as Keywords gives it unless Keywords already made it a length. A parameter Keywords made an angle is left alone. A parameter bound to a PMI record never takes the manager's value, whether or not its record agrees with its display. The expression and each configuration's value follow as for a record. A configuration other than the active one whose value equalled the displayed value therefore takes the active configuration's exact value. Its own stream's stored value is not consulted and usually differs: such a value is the active configuration's, typed, and is not that configuration's own.

The active configuration's equation-manager lines are parameter expressions where the file establishes them. A line is evaluated as SolidWorks evaluates it: every operand is a number in document units, a length in the document length unit, an angle in degrees, and a count or a global variable as itself; `round` rounds to the nearest whole number, and `int` truncates a positive number, except that a number within 256 machine epsilons of a whole number, relative to it, is that whole number, because an angle stored in radians reads in degrees a few ulps off (a stored 1° reads as 1.0000000000000022°, and `int(1 / "Min_count@Clipboard")` is 1 in the manager's stored sequence). The trigonometric functions take and give the unit the document's Angular Equation Units setting names, degrees or radians (DI-43). The document length unit is read from the manager: a driven length line that reads no dimension stores its value in metres beside an expression whose number is in document units, and the unit is their quotient, a millimetre, centimetre, metre, inch, or foot, when every such line gives the same one; a line whose number differs between two trial units holds a literal with its own unit and is not used. A global variable that one line assigns becomes a parameter of the equations node, with the line's right-hand side as its expression and the line's number as its value, which equals the number the manager stores where another line reads it; the global variables are ordered so that each follows the ones it reads, in time linear in the lines. A driven dimension's parameter takes the line's right-hand side as its expression, and the dimensions and global variables it reads as its dependencies, when the line's number in the parameter's kind equals the parameter's value: a length of that many document units, an angle of that many degrees, a count rounded to the nearest whole number with ties away from zero, a number within a billionth of a half counting as the tie (`"width"/2` with `"width"` 29 drives a count of 15). When the manager gives no document unit, the unit is the one the next paragraph reads from the active lines: the trial unit, millimetre or inch, in which a line of the active stream reproduces its parameter's value while the other does not, with that paragraph's conditions; the expression step applies the same rule, in a part with one stream too, where the next paragraph's step does not run. Without either, a line is taken only when it gives the same value in millimetres and in inches. A line is not taken when the part has no single equations node to own a global variable; when two lines assign its name; when its global variable's name, ignoring case, is already a parameter of an equations node or a name an existing expression reads; when it drives no parameter that the manager bound and that has a value and no dependencies; when it reads the dimension it drives; when a name it reads does not resolve to the parameter the manager bound (the resolution is exact in letter case, where the manager's is not); when a global variable it reads is not taken; when a dimension it reads has a decoded value other than the one the manager stores beside it, because a rounded display can reproduce a rounded result by coincidence; when it reads a length and no document unit is established and the two trial units disagree; when it calls a trigonometric function; when it has no value; or when its value is not the parameter's, or for a global variable not the number the manager stores. In a part with several configurations, the lines are read after each inactive configuration's snapshot holds the values its own stream establishes (the next paragraph), and three more conditions apply. A stream's slot is the number of its section. Every inactive configuration's stream that has lines must hold exactly one line that assigns what the line assigns, with the same right-hand side and the same names and bindings read. In each configuration whose snapshot holds values, the active one and one that shares a stream included, the configuration's value of the parameter must be of the parameter's kind; every dimension the line reads must have the value that the configuration's own stream line stores beside it; and the line, evaluated over the configuration's values (the active value where its snapshot has none), must give the configuration's value. A copy of the active value that the stream does not establish fails this wherever the line's inputs differ in that configuration. A global variable is not taken when an inactive configuration's snapshot holds values, because its value in that configuration is not read; a line that reads it then reads a name that does not resolve. An inactive configuration with no stream lines and no values does not stop a line, and `equation.configurations-without-stream` says how many there are and that the lines taken are not established for them. A line may read a dimension of any feature, a later one included, and a global variable a dimension of any feature: a neutral parameter's dependencies need only form an acyclic graph. Once every line is placed, a line whose driven parameter or global variable lies on a dependency cycle is not taken, and then every line that reads a global variable not taken, until none is left. The manager evaluates its lines in stored order, so a line that reads a dimension a later line drives reads the earlier value; the line stores that value beside the reading, and the condition that each dimension a line reads has the decoded value stored beside it refuses such a line wherever the two differ. Such a line's parameter keeps its literal. A parameter whose expression is an equation-manager line carries the property `equation length unit`, a key no Keywords attribute can have because it contains spaces, whose value is the document unit's symbol (`mm`, `cm`, `m`, `in`, `ft`) or `unresolved`.

Each configuration has its own equation manager, in the `Contents/Config-N` stream whose `N` is that configuration's `SourceIndex` or Keywords `id`, and Keywords displays the active configuration's dimensions only. An inactive configuration's value that equals the parameter's value, for a parameter that a line of its stream drives or reads, is replaced by the value the stream establishes, when that value differs from it. A line that drives a dimension and also reads it stores the dimension's input beside that reading, not its result. The stream establishes the f64 stored where a line that does not drive the dimension reads it, when every line that reads the dimension stores one and all store the same one; lines that disagree, or a reading line with no stored value, establish nothing. When no line other than its drivers reads the dimension and exactly one line drives it, the stream establishes that line evaluated once, as above, over the values stored beside its own references, each in its parameter's kind. When several lines drive the dimension and, in the stored order, every one after the first reads it, the values stored beside them show that the manager evaluates them in that order, each refining the value the one before gave, though the stored order is not an evaluation order in general: a line between two of them stores the earlier value beside the dimension. The stream then establishes the last of them evaluated once over the values stored beside its own references, provided every line after it that reads the dimension stores that value; the read side is not otherwise used. Several drivers that are not such a chain, or two at one position, are treated as above, which establishes a value only from the read side. The value on a driving line's assigned side is not used, because it can hold an earlier input. A stored number is a millimetre length of its metres, an angle of its radians, a real as itself, or a count when it is whole. A parameter is replaced only where it is anchored: the same rule applied to the active configuration's stream gives the parameter's value, the value is not a zero, which every unit gives alike and a display rounded to no places can show for a small value, and, when the value is a line's evaluation, every dimension read by a line that drives the parameter is anchored. The document length unit is the one the manager gives as above. When the manager gives none, it is the trial unit, millimetre or inch, in which a line of the active stream reproduces its parameter's value while the other unit does not, counting only a parameter that exactly one line drives and no line reads, and only when no such line names the other unit. A value that is not the parameter's, and a value of a parameter no line of the stream drives or reads, is kept. The decode reports how many values it keeps that a line of their stream drives or reads and that are not replaced or equal to the established value; with no active stream, or one with no lines, that is every such value.

Before that, an angle whose value in the active configuration's snapshot is a rounded display takes the value the active stream establishes for it, as above: Keywords displays an angle in degrees at the document's angular precision and drops trailing zeros, so the bearing's cage span reads `0°` where every stream stores 0.001°. The display must be that value at the finest number of decimal places any angle display of the document shows (a number with that many places, within half a unit in the last), and the stream must confirm the value: a line that does not drive the parameter reads it, or the first line that drives it, in stored order, stores it. A line stores the parameter's value from before the line runs, so the first driving line stores what the last rebuild left, and a later one an earlier line's output. The active stream is the one whose `N` is the active configuration's slot. A parameter with dependencies keeps its expression, and the parameter must hold the display or the established value. The parameter takes the value and its literal as its expression, and every configuration value equal to the display takes the value, which is the copy rule above.

A legacy-prefix 140-byte relation-display profile curve has native code `1`, profile locus `04 00 02 00`, role and state u16 `1` at marker +27 and +29, selector `00 00 80 bf 00 00 04 00` at marker +31, f64 `1` at marker +48, distinct u16 endpoint ordinals at marker +56 and +58, u32 `1` at marker +60, f64 `-1` at marker +64, and zero bytes from marker +72 through +119. Its header at marker +5 is eight `ff` bytes and its shared selector at marker +13 is `00 00 80 bf`. Its typed relation continuation stores a nonzero, non-null u16 at marker +120, a nonzero, non-null two-byte selector at marker +122, four zero bytes at marker +124, two distinct nonzero, non-null two-byte selector tags at marker +128, and `ff fe ff 02 44 00 31 00` at marker +132. The endpoint ordinals are zero-based positions in the complete feature-local marker roster. A selected relation marker makes the curve a display carrier; selected coordinate-bearing points are required before the same layout can define a neutral line.

### 2.1 SWIFT semantic PMI

A `SWIFT/ConfigN-SchemaM` stream contains one GDT-analysis part object. The
part object begins with the Pascal string `Entity`, the Pascal string
`PrizMetrik.GdtAnalysisSupport.GdtPart`, an assembly-name Pascal string, and a
u32 LE object version. A Pascal string is one u8 byte length followed by that
many UTF-8 bytes. The complete stream can contain other data before and after
the part object. Exactly one complete part object selects the semantic graph.

Each entity has this header and terminator:

| Field      | Encoding                  | Semantics                  |
| ---------- | ------------------------- | -------------------------- |
| marker     | Pascal string `Entity`    | Entity start               |
| class      | Pascal string             | Qualified runtime class    |
| assembly   | Pascal string             | Runtime assembly name      |
| version    | u32 LE                    | Serialization version      |
| sections   | section sequence          | Typed fields and relations |
| terminator | Pascal string `EndEntity` | Entity end                 |

Entity sections use these grammars:

| Start token      | Counted values                                                                                       | End token           |
| ---------------- | ---------------------------------------------------------------------------------------------------- | ------------------- |
| `Strings`        | u32 LE count; Pascal-string key and Pascal-string value                                              | `EndStrings`        |
| `Integers`       | u32 LE count; Pascal-string key and i32 LE value                                                     | `EndIntegers`       |
| `Doubles`        | u32 LE count; Pascal-string key and f64 LE value                                                     | `EndDoubles`        |
| `Features`       | u32 LE count; feature ID and type Pascal-string pairs; owned entity definitions follow the roster    | `EndFeatures`       |
| `Annotations`    | u32 LE count; annotation ID and type Pascal-string pairs; owned entity definitions follow the roster | `EndAnnotations`    |
| `RelatedObjects` | u32 LE count; relation name and type Pascal-string pairs; one entity for each pair                   | `EndRelatedObjects` |

The part `Features` section owns the semantic feature definitions. The part
`Annotations` section owns the semantic annotation definitions. Other feature
and annotation rosters reference these part-owned identities. A
`GdtAppliedDatumCollection` owns its ordered `GdtAppliedDatum` objects through
`SubAnnotationN` related objects. Each applied datum references one part-owned
`GdtDatum` annotation. The relation names `PrimaryDatums`, `SecondaryDatums`,
and `TertiaryDatums` set datum precedence 1, 2, and 3.

A `GdtPattern` owns its ordered members through the `SubFeatures`
`GdtAppliedFeatureCollection`. Each `SubFeatureN` related object is one
`GdtAppliedFeature` with one feature reference. An annotation that references
the pattern applies to these member feature identities in collection order.
Nested patterns apply the same rule recursively.

GDT-analysis double fields use millimetres for lengths and radians for angles.
`Tolerance` is the geometric-tolerance magnitude. `Nominal`, `LowerLimit`,
`UpperLimit`, `MinusTolerance`, and `PlusTolerance` are dimension values. A
signed `MinusTolerance` is already the lower deviation. A zero `Nominal` with
an absent or zero integer `Dimension` is an omitted nominal sentinel, not a
zero-length dimension. A nonzero `Dimension` retains zero as the dimension
value. `ToleranceLowerTier` is the lower segment of a composite surface-profile
tolerance. A zero `LowerLimit` or `UpperLimit` is an omitted limit sentinel and
does not replace a zero `MinusTolerance` or `PlusTolerance`. A nonzero limit
supplies the corresponding deviation relative to the recovered nominal when
the tolerance field is zero.

SWIFT rendered strings use bytes `ff fe ff`, a u8 UTF-16 code-unit count, and
that many UTF-16LE code units. In a rendered dimension string,
`<MOD-DIAM>` or `<HOLE-DEPTH>` followed by optional whitespace and a positive
decimal literal marks an explicit diameter or hole-depth nominal in the
document display length unit. The number of digits after the decimal point
equals `BlockToleranceDecimalPlaces` on the corresponding `GdtDiameter` or
`GdtDepth`.

An omitted `GdtDiameter.Nominal` is recoverable from the rendered literal and
the applied nominal geometry. Feature references traverse direct child feature
references and `SubFeatures` applied-feature collections recursively. A
`GdtCylinder` contributes twice `NomCylinder.R`; a `GdtSphere` contributes
twice `NomSphere.R`. Reachable diameter contributors agree when their absolute
difference is at most the greater of `1e-5` millimetres and `1e-9` times their
largest absolute value or one millimetre. The rendered literal binds when
rounding this geometric diameter in a SolidWorks display length unit to
`BlockToleranceDecimalPlaces` produces the literal. Conversion of every
matching literal and unit must produce one millimetre value; multiple values
leave the nominal absent.

When no rendered literal binds, the common diameter of all reachable
contributors supplies the evaluated nominal. This applies to direct features,
patterns, and compound holes. A binding rendered literal has precedence over
the unrounded evaluated geometry. A compound stack containing different
diameters remains unresolved; the decoder does not select one contributor by
position or magnitude. An empty `SubFeatures` collection contributes no
geometry. A `CadRef` whose `CadIdentifier` string is empty contributes no
topology identity or nominal geometry. A rendered literal cannot bind without
applied nominal geometry or a resolved native-history nominal.

An empty `GdtPattern` applied-feature graph may recover a diameter through the
native feature history only when its object name is `Hole PatternN`, the history
contains exactly one native `LPatternN` with one feature seed, and exactly one
later native Hole feature consumes that seed and carries a positive diameter.
The Hole diameter is the evaluated nominal and remains subject to the rendered
literal precedence rule. A missing, multiply named, multiply seeded, or
multiply consuming join leaves the nominal absent.

The non-empty `CadIdentifier` form is `<lane>:<suffix>`. The lane prefix is
local to the SWIFT schema lane. The active partition contributes one sequence
index from its `00 0e` bridge, `00 10` edge-use, and `00 12` vertex-use
records. Each sequence selects its record attribute, and that attribute binds
to the emitted primary face, edge, or vertex with the corresponding identity.
The sequence index is checked before any emitted-arena suffix index. A
sequence with no emitted or unique primary target is unresolved and blocks
arena fallback; a sequence shared by different primary targets is unresolved.
Only a suffix absent from the sequence index may use a unique body, edge, or
vertex identity suffix. Supporting geometry and boundary identities do not
bind directly. A missing, non-decimal, duplicate, or non-primary identity
retains the source `ShapeAspect` target.

A `GdtDiameter` on a multi-diameter counterbore pattern pairs with a
`GdtCounterBore` annotation whose non-cylinder feature-reference set equals the
diameter annotation's feature-reference set. The counterbore annotation's
direct cylinder defines the counterbore diameter. Contributors equal to that
diameter are removed from the diameter annotation's reachable geometry. At
least one contributor must be removed, and every remaining contributor must
agree. Their common value is the hole diameter. The rule does not depend on
cylinder order or relative magnitude.

An omitted `GdtDepth.Nominal` uses the same rendered-literal binding. Each
reachable `GdtCylinder` contributes the axial distance from its `NomBottom`
plane origin to its `NomTop` plane origin. `NomCylinder.I`, `J`, and `K` must
form a unit vector, and the complete top-to-bottom displacement must be
parallel to that vector. All reachable cylindrical contributors must agree. If
the annotation also references one direct operation-specific `GdtCylinder`,
that cylinder's axial distance supplies the nominal when no rendered literal
binds. A binding rendered literal has precedence because it carries the
displayed nominal rather than the unrounded evaluated geometry.

A `GdtDepth` with nonzero `IsThreadDepth` uses the positive `ThreadDepth` of
its direct `GdtCylinder`. The cylinder must have nonzero `IsThreaded`; ordinary
cylinder depth does not replace the named thread-depth field.

A counterbore-depth annotation can reference the counterbore bottom as a
direct `GdtPlane` instead of referencing the counterbore cylinder. It pairs
with a `GdtCounterBore` annotation whose non-operation feature-reference set is
identical. The counterbore annotation has one direct `GdtCylinder`, and the
depth annotation has one direct `GdtPlane`. The plane's `NomOrigin` equals the
cylinder's `NomBottom` origin, the origin lies on `NomPlane`, and the plane
normal is parallel to the cylinder axis. The cylinder's axial top-to-bottom
distance supplies the depth. Multiple valid sibling cylinders must agree.

An omitted `GdtDistanceBetween.Nominal` with `ComputeAnswerBy` zero,
`Direction` 4, `NormalTo` 1, and an identity `NominalTransform` uses its unit
`DirectionVector` and exactly two applied features. A `GdtPlane` or
`GdtIntersectPlane` contributes the projection of `NomPlane.X`, `Y`, and `Z`
when its unit normal is parallel to the direction. A direct `GdtCylinder` or
`GdtCone` contributes the projection of its nominal-axis origin when its unit
axis is perpendicular to the direction. A `GdtCompoundHole` recursively uses
its cylindrical and conical axes; all such axes must have one common
projection. Support planes do not define the compound-hole location. The
positive absolute difference between the two feature projections is the
location nominal.

When `FeatureFosUsage` and `OriginFeatureFosUsage` are both 2, a directional
distance can pair one `GdtCompoundClosedSlot3D` with one descendant
`GdtCylinder`. `NomClosedSlot.LongitudeI`, `LongitudeJ`, and `LongitudeK` form
a unit vector parallel to the measurement direction. The cylinder diameter
equals `NomClosedSlot.Width`, its unit axis is parallel to the slot normal, and
its origin lies on the slot longitude at `(Length - Width) / 2` from the slot
origin. Under these invariants, the positive `NomClosedSlot.Length` is the
location nominal.

An omitted `GdtWidth.Nominal` is the positive millimetre `Width` field of its
applied nominal slot geometry. `GdtCompoundWidth` uses
`NomCompoundWidth.Width`, and `GdtCompoundClosedSlot3D` uses
`NomClosedSlot.Width`. Feature references use the same recursive direct-child
and `SubFeatures` traversal. All reachable width contributors must agree.
`NomHeight` and `NomLength` are feature extents and do not define the width
dimension.

An omitted `GdtRadius.Nominal` is the positive millimetre radius of its
applied semantic geometry. `GdtFillet` uses its `Radius` field,
`GdtCylinder` uses `NomCylinder.R`, and `GdtSphere` uses `NomSphere.R`.
Feature references use the same recursive traversal, and all reachable radius
contributors must agree.

An omitted `GdtLength.Nominal` applied to a `GdtCompoundClosedSlot3D` is the
positive millimetre `NomClosedSlot.Length`. Feature references use the same
recursive traversal, and all reachable length contributors must agree.

Compound-hole size annotations reference the complete hole feature or pattern
and the operation-specific nominal geometry as separate direct features. An
omitted `GdtCounterBore.Nominal` is twice `NomCylinder.R` from its direct
`GdtCylinder` feature. It does not use cylinders reached through the companion
pattern. An omitted `GdtCounterSinkAngle.Nominal` is `NomCone.FullAngle` in
radians from its direct `GdtCone` feature.

An omitted `GdtCounterSinkDiameter.Nominal` is the top diameter of its direct
`GdtCone`. `NomCone.X`, `Y`, and `Z` define the cone apex, and `NomCone.I`,
`J`, and `K` define its unit axis. The `NomTop` plane origin lies on that axis,
and the plane normal is parallel to the axis. If the axial apex-to-plane
distance is `h` and `NomCone.FullAngle` is `a`, the diameter is
`2 h tan(a / 2)`. The axis and plane-normal vectors must be unit vectors, the
complete apex-to-plane displacement must be axial, and `a` must be positive
and less than pi.

The geometric-tolerance classes map by their suffix: `GdtStraightness`,
`GdtFlatness`, `GdtRoundness`, `GdtCircularity`, `GdtCylindricity`,
`GdtCoaxiality`, `GdtLineProfile`, `GdtSurfaceProfile`,
`GdtCompositeSurfaceProfile`, `GdtAngularity`, `GdtPerpendicularity`,
`GdtParallelism`, `GdtPosition`, `GdtConcentricity`, `GdtSymmetry`,
`GdtCircularRunout`, and `GdtTotalRunout`. Dimension classes include
`GdtDiameter`, `GdtRadius`, `GdtAngle`, `GdtAngleBetween`,
`GdtCounterSinkAngle`, `GdtDistanceBetween`, `GdtWidth`, `GdtLength`,
`GdtDepth`, `GdtCounterBore`, and `GdtCounterSinkDiameter`.

The integer field `Modifier` uses `0` for no material requirement, `1` for
maximum material requirement, and `2` for least material requirement. The same
values apply to `GdtAppliedDatum.Modifier`. `ProjectedZoneEnabled` selects the
millimetre value in `ProjectedZoneValue`. `IsMaxTolerance` selects the
millimetre value in `MaxTolerance`. `PerUnitAreaType` value `0` defines a
rectangular unit from `PerUnitAreaLength` and `PerUnitAreaWidth`; value `1`
defines a circular unit from `PerUnitAreaDiameter`.

### 2.2 Object stream class map

`Contents/Config-N` and `Contents/Config-N-ResolvedFeatures` are MFC `CArchive` serializations. The other SW Objects streams (`Contents/CnfgObjs`, `Contents/CMgr`, `Contents/Config-N-ModelHeader`, `Contents/Definition`) use the same class declaration. An object is written as a class tag followed by the object's body; the body's layout belongs to its class and carries no length, so a body cannot be skipped without knowing its class.

| Tag (little-endian u16)            | Meaning                                                                     |
| ---------------------------------- | --------------------------------------------------------------------------- |
| `ff ff`, u16 schema, u16 *n*, *n* ASCII bytes | new class: its schema number and run-time class name; an object of it follows |
| `0x8000 \| index` (index < `0x7fff`) | an object of the class at map `index`                                       |
| `0x7fff`, then u32 `0x80000000 \| index` | an object of the class at map `index`, in a map whose indices reach `0x7fff` |
| `00 00`                            | null object (MFC's form)                                      |

A `suObList` object is an MFC `CObList`: a u16 element count, then that many objects. The root class `moPart_c` has one object, the one its declaration opens.

A body field that refers to an object already written stores that object's map index as a little-endian u16 with bit 15 clear, or, for an index of `0x7fff` or above, as `ff 7f` followed by the index as a little-endian u32. The index can name an object of the `Config-N` stream from the `ResolvedFeatures` stream, because the two streams share one map. A field that refers to a later object holds that object, written in place.

Unicode strings inside a body are `ff fe ff`, a length byte, and that many UTF-16LE code units; a length byte of `ff` is followed by a u16 length.

Classes and objects share one map counter. A new-class declaration takes the next index for the class and the one after it for the object it opens; a class tag takes the next index for the object it opens. One map spans `Contents/Config-N` and `Contents/Config-N-ResolvedFeatures`: the `ResolvedFeatures` stream begins with a u32, the map count at which `Config-N` ended, and a u16, and its first declaration takes that count as its index. In a compound-file part, the `ResolvedFeatures` stream instead opens with a u16 and then a class declaration, and carries no map count. The u16 after the map count is the number of top-level objects in the stream minus two. A stream can end with four zero bytes after its last object, and an in-context part can write two zero bytes after a top-level `moProfileFeature_c` whose sketch plane is written in place; neither has an established owner (open item EV-24). `Config-N` starts its map after entries the stream does not write, so indices are not fixed. The `moNodeName_c` index is not fixed: an object name opens `04 80 ff fe ff`, `05 80 ff fe ff` and so on for the index the map holds. A map that places `moNodeName_c` at `0x46` also declares `moHeader_c`, `su_CStringArray`, `moLogs_c` and `moStamp_c` after `moPart_c` and before the part's own objects. A class declared in `Config-N` and instantiated in `ResolvedFeatures` is written there by its `Config-N` index.

---

### 2.3 Object classes

A configuration declares each class at most once across its `Contents/Config-N` and `Contents/Config-N-ResolvedFeatures` streams; a class one configuration declares in `Config-N` another can declare in `ResolvedFeatures`. The class of an object fixes its body layout (§2.2).

An object of a named class opens its body with its name object: a `moNodeName_c` object, written as that class's declaration on its first use in the stream and as its tag after, whose body is a Unicode `CString` holding the name. A named class's body bytes after the name object belong to the class. The name object's tag, string marker and length byte are the same for every named class whose names have the same length, so they do not identify the class.

After the name string, a named object's body holds a named-object header, at offsets from the end of the name string:

| Offset | Field |
| --- | --- |
| +0 | little-endian u32 |
| +4 | little-endian u16 family word, one Boolean-operation byte and one schema byte (§2) |
| +8 | little-endian u32 object identifier; `ff ff ff ff` when the object has none |
| +12 | little-endian u32 |
| +16 | Unicode `CString` |

The object identifier at name string +8 is the identifier that §2's object-name records read. A named `moExtFeature_c` object writes a second name object after its name string instead of the header.

A feature or folder object writes a feature-object block after the named-object header's string. The head of the block opens with a list of the block's *inputs*: a little-endian u32 `0`, a little-endian u16, a little-endian u16 count *n*, *n* object pointers (§2.2), and twelve zero bytes. The fields after the list are at the offsets below when *n* is zero; inputs move them by their length. The u16 at +4 is zero in every class but `moBaseBody_c`, where it can be nonzero. An extrusion feature's one input is a reference to the `moProfileFeature_c` sketch feature it extrudes, which is why the extrusion classes write two more bytes before +20. A `moDetailCabinet_c` cabinet's inputs are its notes folders (open item EV-15).

| Offset | Field |
| --- | --- |
| +0 | little-endian u32 `0` |
| +4 | little-endian u16 |
| +6 | little-endian u16 count *n*, then *n* object pointers |
| +8 | twelve zero bytes (after the *n* pointers) |
| +20 | little-endian u32 |
| +24 | little-endian u32 |
| +28 | one byte |
| +29 | little-endian u32 version number |
| +33 | little-endian u32 date, year × 1000 + day of year; zero when +29 is zero |
| +37 | little-endian u32 version number |
| +41 | little-endian f64 |
| +49 | little-endian u32 date, year × 1000 + day of year |
| +53 | little-endian u16 `0` |
| +55 | Unicode `CString` |

The tail of the block has offsets from the end of the +55 string:

| Offset | Field |
| --- | --- |
| +0 | `00 ff ff 00 00` |
| +5 | one byte, `01` or `03` |
| +6 | eight `ff` bytes |
| +14 | little-endian f32 `-1` |
| +18 | six little-endian u32 |
| +42 | little-endian u32 |
| +46 | FILETIME, its high u32 first and then its low u32, both little-endian |
| +54 | little-endian u16 `2` |
| +56 | little-endian u32 |
| +60 | little-endian u32 |
| +64 | little-endian u32, in a file whose `swVersion` is 11000 or above |

The block ends at tail +64, or at tail +68 in a file whose `swVersion` is 11000 or above. In such a file, a folder class (a class whose name ends in `Folder_c`) then writes a little-endian u32 `0`. The class's own bytes follow.

After the feature-object block and its u32 `0`, a folder class writes these bytes of its own:

| Class | Own bytes |
| --- | --- |
| `moDocsFolder_c`, `moCommentsFolder_c`, `moEqnFolder_c`, `moSensorFolder_c`, `moNotesAreaFtrFolder_c`, `moConfigTableFolder_c`, `moRefAxisFtrFolder_c`, `moMateReferenceGroupFolder_c`, `moFoldUpDownSketchFolder_c` | none |
| `moSurfaceBodyFolder_c`, `moFeatSurfaceBodyFolder_c`, `moFeatSolidBodyFolder_c` | twelve zero bytes |
| `moSolidBodyFolder_c` | twelve zero bytes, then a reference to an object already written, or nothing when the next object is written in place |
| `moInkMarkupFolder_c` | a reference to an object already written, or nothing when the next object is written in place |
| `moMaterialFolder_c` | a Unicode `CString`, then a little-endian u32 |
| `moHistoryFolder_c` | a little-endian u16 count, then that many `moHistoryFeatItemData_c` objects |
| `moFavoriteFolder_c` | little-endian u32 `0` and u32 `1` |
| `moSelectionSetFolder_c` | little-endian u16 `0` and u32 `1` |

The references that `moSolidBodyFolder_c` and `moInkMarkupFolder_c` store name the `Config-N` stream's `moEnvFolder_c` object.

A `moFtrFolder_c` feature folder writes, after its u32 `0`, a reference to the folder written before it, eight zero bytes, and little-endian u32 `1`, `1`, `0` and `1`. Most folders then write a reference to their own object, and end there. A folder can instead continue as open item EV-13 describes.

An imported feature tree (the inputs of a `moMBimport_c` import feature) brackets each group of features with a pair of folders: an opening folder and a `moFtrFolder_c` whose name ends with `___EndTag___`. The opening folder's link writes the end-tag folder in place as its first reference, then eight zero bytes, the u32 run and a reference to the end-tag folder; an opening folder of a class that otherwise writes no link (`moRefAxisFtrFolder_c`) writes this link too. The end-tag folder's own link is one reference, back to its opening folder. After such inputs, the block's zero run before its fixed fields is two bytes instead of twelve. The end-tag folder can be of another folder class, as a `moNotesAreaFtrFolder_c` pair (`Notes`, `Notes1___EndTag___`) in a detail cabinet. In a part that declares `swVersion` 17000, such an end-tag folder's link follows its block's tail at once, with no u32 of the block and no u32 `0` of the folder; the detail cabinet's block likewise writes no u32, and the rest of the stream follows the layout of files before 11000 (four bytes fewer in a reference plane's end and six rather than twelve after a plane's data). The cabinet's fields do not fit after the u32 and do fit without it.

After the feature-object block, a `moRefPlane_c` reference plane writes these bytes of its own, at offsets from the block end:

| Offset | Field |
| --- | --- |
| +0 | 34 zero bytes |
| +34 | little-endian u32 `1` |
| +38 | sixteen zero bytes |
| +54 | little-endian u32 `1` |
| +58 | six zero bytes |
| +64 | little-endian u32 |
| +68 | 44 zero bytes |
| +112 | little-endian f64 |
| +120 | little-endian f64 |
| +128 | little-endian f64 |
| +136 | little-endian u32 |
| +140 | ten zero bytes, or six in a file whose `swVersion` is below 13000 |

A `moRefAxis_c` reference axis writes the same bytes as a reference plane.

A `moOriginProfileFeature_c` object writes ten zero bytes of its own.

A `moHelix_c` helix writes little-endian u16 `0` and u16 `1` of its own.

After the feature-object block, a `moAmbientLight_c` ambient light writes these bytes of its own:

| Offset | Field |
| --- | --- |
| +0 | little-endian f64 |
| +8 | little-endian f64 |
| +16 | little-endian f64 |
| +24 | little-endian u32 |
| +28 | little-endian u32 |
| +32 | little-endian u32 |
| +36 | little-endian u32 |
| +40 | little-endian u32 |
| +44 | Unicode `CString`, the light's name |

After the name string, at offsets from its end:

| Offset | Field |
| --- | --- |
| +0 | little-endian u32 `0` |
| +4 | little-endian u32 |
| +8 | little-endian u32 `0` |
| +12 | little-endian f64 |
| +20 | little-endian u32 |
| +24 | little-endian f64 |
| +32 | little-endian u32 |

The light's bytes end at name string +36.

A `moDirectionLight_c` directional light writes the same bytes through name string +36. Three little-endian f64 follow at name string +36. A feature-object block, head and tail, follows at name string +60, and further bytes follow that block (open item EV-09).

After the feature-object block, most feature classes begin their own bytes with a *feature body head*. The classes that write one are the extrusion feature classes `moExtrusion_c`, `moICE_c`, `moRevolution_c`, `moRevCut_c`, `moCut_c` and `moSweepCut_c`; `moSweep_c`, `moBaseBody_c` and `moSavedExtBody_c`; the pattern classes `moCirPattern_c`, `moLPattern_c`, `moAPattern_c`, `moMirrorPattern_c` and `moSketchPattern_c`; `moMirrorSolid_c`, `moCombineBodies_c`, `moMoveCopyBody_c`, `moMoveFace_c`, `moScale_c`, `moThicken_c`, `moMoldPartingLine_c` and `moPLine_c`; the sheet-metal and weldment classes `moTemplateSheetMetal_c`, `moUiBend_c`, `moBending_c` and `moWeldCornerFeat_c`; the reference classes `moRefPlane_c`, `moRefAxis_c` and `moRefPoint_c`; and `Fillet_c` and `Chamfer_c`. The head's fields are, in order:

| Field |
| --- |
| little-endian u32 `0` and u16 `0` |
| little-endian u16 *h*, `1` or `0` |
| little-endian u16 `0` |
| little-endian u16 count *n*, then *n* objects: references to objects already written, or objects written in place (§2.2); *n* is zero when *h* is `0` |
| little-endian u32 *f*, `0` or `1` |
| when *f* is `1`: a little-endian u16 count, then that many objects |
| when *h* is `1`: little-endian u16 `0` |
| little-endian u32 count *m*, then *m* little-endian u32 |
| little-endian u32 *t*: `3`, `2` or `0` |
| little-endian u32 *g*, `0` or `1` |

When *g* is `1`, a little-endian u16 count from `1` to `3` follows, then that many objects, the first written in place. When *g* is `0`, a little-endian u32 *e* (`0` or `1`) and a little-endian u16 *x* (`0` or `1`) follow. When *x* is `0`, the head ends with little-endian u16 `1`, eighteen bytes (zero in every object but one), and little-endian u32 `1`. The bytes after *x* when it is `1`, and the meaning of *h*, *f*, *t*, *g*, *e*, *x* and the *m* values, are open item EV-10.

After *g* `1`, the objects are `moCompSolidBody_c` objects written in place, each beginning with a `moUnitComponent_c` object; in another form the first object is declared inline, followed by a `moUnitComponent_c` object as in a solid body. When *x* is `1`, a little-endian u16 equal to *m* follows, and then a `moPerBodyChooserData_c` object written in place. A `moBaseBody_c` or `moSavedExtBody_c` head is followed by a `moPMarkRecord_c` object written in place.

*t* is `3` in the extrusion feature classes, the fillet, `moMirrorSolid_c` and most of the classes that change existing bodies, and `0` in the reference classes, the base body, the saved body and most patterns.

A `moExtrusion_c` extrusion has *h* `1`, *f* `0`, *t* `3`, *g* `0`, *e* `1` and *x* `1`; a `moBoss_c` (an older extruded boss) is written as an extrusion. The objects of its first list are references to `moAtom_c` objects. A `moBaseBody_c` object has *h* `1`, *f* `0`, *t* `0`, *g* `0` and *x* `0`, and ends with the head.

The reference-plane layout below is the head with *h*, *n*, *f* and *m* `0` and *t*, *g*, *e* and *x* `0`, which fills the bytes from the block end through +58. The fillet and chamfer layout below is the same head with *t* at +20, except that a fillet or chamfer's u32 *f* at +12 holds *k* in its upper sixteen bits.

A face or edge selection is stored as a persistent identity: a `moFaceRef_c` or `moEdgeRef_c` object holding a list of *hops*, each an identity-representation object whose class name ends in `IdRep_c`. These objects are written as follows.

A `moFaceRef_c` or `moEdgeRef_c` object writes a 38-byte header: little-endian u32 `1`, u32 `0`, u32 count *c*, a four-byte selector, a little-endian u32 record identifier, the 16 bytes `7d c3 94 25 ad 49 b2 54 7d c3 94 25 ad 49 b2 54`, and u16 `0`. Then come *c* entries, each an object pointer (§2.2): an identity-representation object written in place, a reference to one already written, or a null pointer (u16 `0`). Eight zero bytes follow the entries. In a file whose `swVersion` is 14000 or above, a little-endian u32 version number and u32 `0` follow them.

An identity-representation object writes u16 `0`, then a `moFR_c` object written in place, then bytes of its own class:

| Class | Bytes after the `moFR_c` object |
| --- | --- |
| `moSimpleSurfIdRep_c`, `moFromSktEntSurfIdRep_c`, `moFilletSurfIdRep_c`, `moThinOffsetSurfIdRep_c`, `moSimpleEdgeIdRep_c` | a little-endian u32 |
| `moEndFaceSurfIdRep_c`, `moThinClosingSurfIdRep_c`, `moUnBendSurfIdRep_c`, `moBendEndSurfIdRep_c`, `moSegmLeftTrimEdgeIdRep_c`, `moSegmRightTrimEdgeIdRep_c` | two little-endian u32 |
| `moFromSktEnt3IntSurfIdRep_c`, `moEndFace3IntSurfIdRep_c` | three little-endian u32 |
| `moSurfaceIdRep_c`, `moIndirectSurfIdRep_c` | none |
| `moLPatternSurfIdRep_c`, `moCirPatternSurfIdRep_c`, `moCirPatternSurfIdRep2_c` | an object pointer to the hop it repeats, then little-endian u32 instance number and u32 `0` |
| `moMirPatternSurfIdRep_c` | an object pointer to the hop it mirrors |

The hop a pattern or mirror hop points to is written in place inside it, and so is not a separate entry of the list.

A `moFR_c` object names the feature that produced the entity. It writes an object pointer to a `moExtObject_c` object (written in place on its first use, and referenced after), a little-endian u32, and a little-endian u32 time stamp in seconds since 1970-01-01 UTC.

A `moExtObject_c` object names a document. It writes two object pointers to `moCStringHandle_c` objects (the document's path and its name), little-endian u16 `2`, a byte, a little-endian u32, three Unicode `CString` values, 18 bytes, a Unicode `CString` holding a configuration name, and 16 bytes. A `moCStringHandle_c` object writes one Unicode `CString`. The other `moExtObject_c` forms are open item EV-14.

After the feature-object block, a `Fillet_c` fillet and a `Chamfer_c` chamfer write these bytes of their own, or one of the forms of open item EV-12:

| Offset | Field |
| --- | --- |
| +0 | fourteen zero bytes |
| +14 | little-endian u16 *k* |
| +16 | four zero bytes |
| +20 | little-endian u32, `3` in a fillet and `0` in a chamfer |
| +24 | ten zero bytes |
| +34 | little-endian u32 `1` |
| +38 | sixteen zero bytes |
| +54 | little-endian u32 `1` |
| +58 | little-endian u32, written only when *k* is not zero |

The fields that follow have offsets from the end of the +54 u32, or of the +58 u32 when it is written:

| Offset | Field |
| --- | --- |
| +0 | little-endian u16 `0` |
| +2 | little-endian u32 |
| +6 | little-endian u32, `0` or `1` |
| +10 | little-endian u32 |
| +14 | sixteen zero bytes |
| +30 | little-endian u32 |
| +34 | thirty zero bytes |
| +64 | little-endian u32 |

The object's bytes end at +68 of the second table.

A `moProfileFeature_c` sketch feature writes ten zero bytes of its own, then its `sgSketch` object: written in place, or a reference to one already written (§2.2).

The parameter classes `moLengthParameter_c`, `moAngleParameter_c`, `moIntegerParameter_c` and `moScalerParameter_c` write no feature-object block. Their objects are §2's named scalars: the 22-byte scalar header is the named-object header and a little-endian u16 `0`.

A `moLengthParameter_c`, `moAngleParameter_c` or `moScalerParameter_c` object writes its named-object header, little-endian u16 `0`, its little-endian f64 value, and a 35-byte trailer: three zero bytes, the little-endian u32 object identifier at trailer +3, and among later fields a little-endian u16 operand count at trailer +27 and a little-endian u32 role at trailer +29 (`0` driving, `1` display). That many operand objects follow the trailer; §2's 12-byte operand cells are these objects, each a class tag and a 10-byte handle body.

A `moDetailCabinet_c` object writes, after its feature-object block, little-endian u16 `0`, two little-endian f64 and ten zero bytes.

A `moDefaultRefPlnData_c` object, written after each default reference plane, holds six little-endian f64 (a point and a normal), a flag byte, nine further f64 when the flag is `1`, four f64, a byte, four f64 (the plane's display half-extents), six bytes, two little-endian i32, a byte, three f64, little-endian u32 `1` and twelve bytes: 169 bytes, or 241 with the flag set.

A `moHistoryFeatItemData_c` object writes little-endian u16 `0`, four little-endian u32 and a Unicode `CString`, then an object pointer to its `moCompFeature_c` object. A `moCompFeature_c` object writes a `moUnitComponent_c` object in place, then a little-endian u32 feature number and a little-endian u32 time stamp, as `moFR_c` does. A `moCompFace_c`, `moCompEdge_c` or `moCompVertex_c` object writes a `moUnitComponent_c` object, one byte, and an object pointer to its `moFaceRef_c`, `moEdgeRef_c` or vertex reference; a `moCompSolidBody_c` object writes the pointer without the byte.

A `moUnitComponent_c` object writes little-endian u16 `2`, a little-endian u16 flag word, 39 bytes, sixteen `ff` bytes and 20 bytes (16 in files of `swVersion` below 13000): 79 bytes. In files before `swVersion` 11000 the sixteen `ff` bytes and the bytes after them are absent: 43 bytes. A `moCompFeature_c` can refer back to a component already written instead of writing one. Most objects then write a little-endian u32 version number; an object in a file of `swVersion` 14000 or 15000 can omit it. The version number is a multiple of 1000 from 9000 to 40000, and the field that follows can hold any other value.

#### Configuration objects

`Contents/Config-N` holds one `moPart_c` object, which ends at the stream's last byte. Every named object here and in the history opens with a *named head*: an object pointer to its `moNodeName_c` (null for an unnamed object), a little-endian u32, a little-endian u16 family, a byte, a schema byte (`0x40` or `0xc0`), a little-endian u32 object identifier, a little-endian u32 and a Unicode `CString`. A `moNodeName_c` object writes one `CString`.

A `moPart_c` object writes, in order:

| Field |
| --- |
| little-endian u32, u32 `0`, the part's u32 version, a named head, a little-endian u32 |
| object pointers to `moVisualProperties_c`, `moUnitsTable_c`, `moUnitComponent_c`, `gcCurvatureObject_c` and `moTransRefPlaneData_c` objects |
| sixteen zero bytes, little-endian f64 density, little-endian f64 tolerance, little-endian u32 `2`, a little-endian u16 flag; when set, a `CString` and a `moFaceRef_c` object |
| a marker block, an object pointer to `gcXhatch_c`, one to `moMaterial_c` |
| one record per body (the material's body count): an object pointer to a `moCompSolidBody_c` or `moCompSurfaceBody_c`, 28 bytes, a little-endian u32 flag and, when it is set, a `moVisualProperties_c` body written in place |
| twenty zero bytes, little-endian u32, u32, u16, the next object identifier as u32, a u32 count and that many `moAtom_c` objects (named head and 64 bytes) |
| a little-endian u32 version, u32 `10000 + k`, a u32 holding *k* and flags, and *k* records: u32 identifier, u32 type, u32, i32 `-1`, u32 *f*; twelve bytes when *f* is `1`; when *f* is `0`, a u32 *g* and then an object when *g* is set, a u32 when not |
| a `moRelMgr_c` object, a `moEnvFolder_c` object, a null pointer, a `suObList`, and `moView_c` objects while one follows |
| a byte, u32 `50`, u32 `108`, a `moFeatColorTab_c` object and a `moSketchBlockMgr_c` object |
| optionally 29 bytes of thread data written without a tag, a `moCThreadRefMgr_c` and a `moPrtExplViewManager_c` object |
| with the view manager, eight zero bytes, eight `ff` bytes and 26 bytes; without it, 51 zero bytes; then the next object identifier as u32 and eight zero bytes |

The *marker block* writes `aaaaaaaa bbbbbbbb`. Before `swVersion` 12000 it continues with a fixed 74 bytes (10000) or 82 (11000). Otherwise it writes a little-endian u32 *a*; when *a* is `1`, u32 `0x18`, u32 `0x10`, u32 `1`, a u32 count and that many points of three f64; then 78 bytes (12000) or 82 (14000 and above), the next object identifier as u32, from 19000 a u32 count *k*, *k* eight-byte entries and a u32, and u16 `0`.

A `moVisualProperties_c` body writes u32 colour, u32, u32 colour, two `CString` values (the second the material), six f64 and a u32 flag. With the flag set: two f64, 88 bytes, a `CString`, six bytes, two f64 and six u32 (five before 15000); with it clear, five u32. Then a `CString`, a u32 and a u32 appearance flag. With an appearance: u32 count, u32, the appearance name, 81 bytes, the texture path, twelve bytes, a `CString`, 76 bytes; from 13000 a `CString` and u32; from 13000 up to 19000 three `CString` values and 92 bytes. Then a `CString`, a u32 and, except in 13000–14999, a second u32; a `CString`, two u32, a `CString` and twenty bytes. From 17000 a u32 follows; when it is non-zero, 192 bytes, five `CString` values, four bytes, two `CString` values, 28 bytes, two `CString` values, eight bytes and three `CString` values.

A `moUnitsTable_c` writes a little-endian u16 count (16 or 17) and that many unit objects. A unit writes a 62-byte base: u32 kind, u32, f64 value, u32 decimal places, u32, f64, four u32, f64, u32 and u16. `moAngleUserUnits_c`, `moUnitSysUnits_c`, `moForceUnits_c`, `moPowerUnits_c`, `moEnergyUnits_c` and `moTimeUnits_c` add a u16; `moLengthUserUnits_c` adds a u16 and an object pointer to a dual length unit or null; `moPercentUnits_c` adds a u32; `moDensityUnits_c` adds u16, u16, a flag byte, a length-unit pointer when the flag is set, u16 and u32. The other unit classes add nothing.

The part's `moUnitComponent_c` (its *part-header form*) writes u16 `2`, a byte, a u32 count and that many face-appearance entries (a u32 flag, a visual-properties body when set, a `moCompFace_c` pointer, a u32, a `moExtObject_c` pointer and three u32), then u32 `0` and a zero byte. The long form (sixteen `ff` bytes at +43) is the one the history writes.

| Class | Layout |
| --- | --- |
| `gcCurvatureObject_c` | five f64 and a u64 FILETIME |
| `moTransRefPlaneData_c` | six u32 (`1`, `1`, two colours, `0x5f`, `0`), a flag byte, ten f64 (a box) when set, u32 `0`, a u32 count and that many (u32 key, object pointer) pairs |
| `gcXhatch_c` | a `CString` name, three f64, four u32 |
| `moMaterial_c` | a `moDensityParameter_c` pointer, a byte, a `uoModelData_c` pointer or null, u32 body count, u32, and a visual-properties body when the count is non-zero |
| `moDensityParameter_c` | a named head (null name), u16, f64 |
| `uoModelData_c` | six zero bytes |
| `moView_c` | a `CString`, a flag byte, nine f64 when set, 119 bytes (115 before 15000), u16 |
| `moAnnotationView_c` | the `moView_c` layout without its final u16, then 50 bytes |
| `moFeatColorTab_c` | u16 count (`0` in every stream), four u32: eighteen bytes |
| `moPMarkRecord_c` (in `Config-N`) | u32 *X* (`1`–`3`), a further u32 when *X* ≥ 2, a u32 count *n*, *n* six-byte entries, u32 *f*, a feature written in place (named head and feature block) when *f* is `1`, u16 and 30 bytes |
| `moCThreadRefMgr_c` | 15, 19 or 23 zero bytes (14000, 15000, 16000 and above), u32, u32 *b*, u32 *n*; only *b* = *n* = `0` has been read |
| `moPrtExplViewManager_c` | a feature head and feature block |
| light (in `moEnvFolder_c`) | three f64, twenty bytes, a `CString`, twelve bytes, f64, u32, f64, u32; a directional light adds three f64 and eight bytes |

A `moSketchBlockMgr_c` writes a named head (null name) and feature block, twelve more bytes of a 16-byte identifier, u32, u32 identifier, u32, u16, a `suObList`, u32, u32, a u32 count *k*, *k* identifiers, 30 bytes and a `moPMarkRecord_c` pointer (a null pointer is followed by the record's body in place), a `suObList`, a u32 and two `CString` values.

A `moRelMgr_c` writes a u16 relation count and that many `moRelation_c` objects, a u16, and the document's detailing settings in place: a head (`CString`, 24 or 32 bytes, the standard name, 205 bytes, `CString`, u32, `CString`), unit names (48 or 52 bytes, a u32 count and that many `CString` values), a u32, 108 bytes (before 15000) or 200, from 16000 a u16 count and that many `CString` values, a font (`CString`, 64 bytes and, from 17000, a u32 weight), a header block, and ten dimension styles. **The remainder of the detailing settings is read by a per-version template of gap lengths and `CString` positions, not by a decoded grammar** (open item EV-19). The manager ends with 0, 8, 12, 16 or 20 bytes by version, a u16 count and that many parameter objects (named head, u16 `0`, f64, a byte).

#### Sketch objects

A sketch is an `sgSketch` object. Its geometry is a sequence of inline *records*, not separate objects; separate *handle* objects (`sgPointHandle`, `sgLineHandle`, `sgArcHandle`, `sgEntHandle`) point at records by slot. Every record opens with a 64-byte head:

| Offset | Field |
| --- | --- |
| +0 | an attribute record: `ff ff`, a little-endian u16 record schema version (`1f 00` or `07 00`), a slot byte, little-endian i32 `-1`, two little-endian u16 (usually `ff ff`), and little-endian f32 `-1` |
| +17 | little-endian u32 kind |
| +21 | little-endian u16 `0` |
| +23 | two little-endian u16, the locus (`04 00 02 00` profile, `05 00 01 00` geometry) |
| +27 | little-endian u16 role |
| +29 | little-endian u16 state |
| +31 | little-endian f32 `-1` |
| +35 | an object pointer: null, or an `sgExtEnt_c` object for a record projected from outside the sketch |
| +37 | little-endian u16 selector |
| +39 | nine bytes |
| +48 | little-endian f64 `1` |
| +56 | eight zero bytes |

In files before `swVersion` 11000 the eight zero bytes are absent: the coordinate mask or the curve's first fields stand at +56. Offsets after +35 move by the length of an object written there. §2's "marker" offsets for sketch records count from the start of this head in these layouts.

A *point record* continues with a little-endian u16 coordinate mask at +64, f64 *x* and *y*, and f64 *z* when bit 4 of the mask is clear (`1e 00` and `1f 00` write two coordinates, `0e 00` three). Then come two little-endian u16, three object pointers (the handles of the curves that meet at the point, then null pointers), little-endian i32 `-2`, 36 zero bytes, a little-endian u32, a little-endian u16 and a little-endian u32 record identifier. In some records a small little-endian u32 (`3`, `8` or `10`) stands in place of the `-2`, and the 42 bytes that follow hold four little-endian f64 and ten bytes.

A *text record* has a curve record's fields up to its f64 `-1` and then, in place of the curve type, a `moDimText_c` object (below). A *curve record* continues with two little-endian u16 endpoint indices at +64, a little-endian u32, little-endian f64 `-1` at +72, and a little-endian u16 curve type at +80: `0` for a line, `1` for an arc (type `0xffff` has the arc's layout; its meaning is open), `0x40` for a style spline. A style spline (`swVersion` 19000) writes u16 `0`, a u16 (`0xffff`), a `cptsSplineList_c` of its control points' handles, eight zero bytes, two f64 `-1000`, 60 zero bytes, u16 `1`, twelve zero bytes, i32 `-1`, u32 `0`, u32 `1` and six zero bytes before the common ending. A record of type `0xffff` whose next u16 is also `0xffff` is a partial ellipse: u16 `0xffff`, the centre index and four axis-point indices, and a list of them ends with 20, 22 or 24 zero bytes. An arc then writes a little-endian u32 and four little-endian i32 (`-2`). An ellipse also has curve type `1` but writes little-endian u16 `0`, the u16 index of its centre point and four u16 indices of its axis points. All three end with a little-endian u16, a little-endian u32 and a little-endian u32 record identifier: 92 bytes for a line, 112 for an arc, 104 for an ellipse. Records of schema version `07 00` have the same layout as version `1f 00`.

An `sgSketch` object writes, in order:

| Field |
| --- |
| little-endian u32 count *N*, u16 `0`, u16, u16 `0`, little-endian u32 |
| *N* point records |
| little-endian u32, then curve records for as long as one follows (lines and arcs) |
| further curve lists: a 12-byte header (little-endian u32 `0` or `0x10000`, a little-endian u32, and a little-endian u32 flag `1`) before each later run of curve records, or a 16-byte header (little-endian u16 `0`, a little-endian u16, then the 12-byte form) before a run of ellipses; or a little-endian u32 alone (as before the first list), or two zero bytes alone (before a list of text records), or — after a list ending in a line from an external reference (5 parts) — eight zero bytes, u32 `0x10000`, a u32 (`0x80000`, `0x800000`), a u32 (`4`, `7`) and u32 `1`; then zero bytes, 36 of them in the sketches examined when the last curve list holds lines, 22 when it holds arcs and 20 or 24 when it holds ellipses (open item EV-16) |
| three dimension groups, each a little-endian u16 count and that many dimension objects; after offset dimensions a group's entries can be relation entries, as in the relation groups (2 designs) |
| eight relation groups, each a little-endian u16 count and that many entries; an entry is an object (a dimension, for example the `sgOffsetDim` objects of an offset) when its first word is a class tag or declaration, a polygon or sketch-pattern entry (below), and an inline relation otherwise. A group that holds polygon or pattern entries is followed by a little-endian u16 count *m* and *m* two-operand relations, each followed by 20 bytes. Some sketches write fewer groups (seven; three in a block definition's sketch): no group count has bit 15 set, so a class tag or declaration where a count would be ends the groups |
| an object pointer to a `suObList` (the sketch's dimension displays) |
| the in-context block: little-endian u32 `0`, a little-endian u16 group count *n*, a little-endian u16 size *k* of the first group, *n* in-context groups (below) — or, when *n* is `0` and *k* is not, chains alone — and little-endian u32 `0`; twelve zero bytes in a sketch with no external references |
| when the sketch holds offset entities: `00 00 02 00` and an `sgPGMProfileData_c` object |
| a settings block: 76 bytes (78 in files of `swVersion` 19000 and above, except 76 after offset chains in three parts), a Unicode `CString`, 42 bytes, a little-endian u32 flag word, a little-endian u32, a little-endian u32 count and that many `moExtensionLineIndicator_c` objects (an object pointer and a little-endian u16 each), and 24 zero bytes: 78 bytes when the count is zero |
| an object pointer to a second `suObList`, holding the sketch's `moSketchRegion_c` regions |
| 38 zero bytes when the second list is empty; when the sketch holds slots, eighteen zero bytes and then its slot map (and after the map, or after the zero bytes alone in a sketch without slots, a little-endian u32, a little-endian u32 count and that many objects: `moSketchChain_c` objects or handles): an MFC `CMap` written as a little-endian u32 next free key, a little-endian u32 count and that many pairs of a little-endian u32 key and an `sgSlot_c` object, followed by zero bytes. An `sgSlot_c` writes a little-endian u32, a little-endian u32 identifier (its key plus one), a 72-byte record head (above; a little-endian u32 at +17, `0` to `3`, not tied to the slot's kind) and six handle objects. The six-handle form holds from `swVersion` 11000; a file of 10000 differs |
| little-endian u32 `3` (or `7`); a flag byte, `0` when a compressed body (below) follows in place of it; little-endian u32 `0`; a little-endian u16 count *c* and *c* face or edge references; when *c* is non-zero, little-endian u32 `4`, u32 `16`, u32 `1`, a u32 count and that many u32 identifiers; little-endian u32 `0` (absent after a reference list when the leading u32 is `7`); u32 `100000`, u16 `0`, — or u32 `100000` + *G* (the u32 `0` before it is also absent after a reference list whose leading u32 is `3`), a little-endian u16 group count *G* and *G* groups, each a `moOneContour_c` object, a little-endian u16 count and that many `moBBoxCenterData_c` objects (here u32 `1`, four f64, u32 `1`, a u16 identifier, a u16 and eighteen bytes), a u16 and a u32 identifier, the identifiers being `100000` to `100000` + *G* − 1 in some order — an object pointer (null, or a handle), and an object pointer to the `moCompRefPlane_c` object that names the sketch plane |

A relation writes eight zero bytes, little-endian i32 `-1`, sixteen zero bytes (seventeen in files of `swVersion` 18000 and above), a little-endian u32 relation type, a little-endian u32, a little-endian u16 (`0`, or `2` in some) and a little-endian u32 (`0xfffe`, or a small index `0`, `1`, `2`, … in type `47`), then its operand handles, written in place. A relation with two operands then writes six zero bytes, a little-endian u16 (`0`, or `3` in some), two `ff ff ff ff` and 24 zero bytes.

A *polygon* or *circular pattern* entry (relation group 7) writes a little-endian u32 identifier, a little-endian u32 count *n* (sides, or instances), ten bytes, a little-endian u16 written twice (*n* for a polygon; for a circular pattern a count that is not always *n*), the centre as three little-endian f64, two line handles, a little-endian u16, the handle of the construction circle, a little-endian u32, the handle of the centre point, u16 `1` and u32 `0`; a placeholder handle (slot `0x777f`, i32 `-2`) is followed by u32 `0`. A polygon then writes u16 `0`; a pattern writes its instance-count dimension, an `sgCircularPattCntDim`. A *linear pattern* writes one entry per direction (relation group 5): a little-endian u32 identifier, a little-endian u32 instance total, eighteen bytes holding the direction's count *n1* and the other's *n2* twice, the direction as three little-endian f64 (a unit vector), two line handles (each placeholder followed by u32 `0`), a little-endian u16 (`1` with real handles), eight zero bytes — or, for a direction taken from a line, u32 `1` (`01 00 01 00` in some), a `moLineRef_w`, its handle block (absent when the reference is a sketch entity's, whose tail holds it), u32 `0` (eight zero bytes in some), seven f64 and seven zero bytes (three or one in some; and in some a zero byte, a second `moLineRef_w` with its block, u32 `0` and seven f64 come first) — its `sgLinearPattCntDim`, the spacing as a little-endian f64 and u16 `1`. The relations after such a group bind the members: for a polygon, *m* relations of type `57` between consecutive sides, each followed by 20 bytes (u16 `0`, the previous, next and own vertex index as u16, u32 `1`, u32 `1`, u32 `0`). Types `4` and `5` take one line operand and type `9` two operands. A relation with three operands (type `13`, symmetric) then writes six zero bytes. A two-operand relation on a spline (type `9`, flags `0x10002`) writes, after its operands, little-endian u32 `0` and a `suObArray` of two entries, an `sgDimEntityHelpData_c` (a little-endian f64, a curve parameter, then u32 `0`) and a null pointer, before its suffix. A `swVersion` 19000 two-operand suffix ends with one more i32 `-1`.

A handle object (`sgPointHandle`, `sgLineHandle`, `sgArcHandle`, `sgEllipseHandle`, `sgTextHandle`) writes a little-endian u16 slot, little-endian i32 `-1` (`-2` in a polygon's placeholder handles; a small index, with u32 `0` after it, for an entity of a block instance) and a little-endian u32: ten bytes. An `sgEntHandle` writes a little-endian u16, two little-endian u32 and u16 `0`. An `sgSlotHandle` writes a handle's ten bytes and then a second twelve-byte cell (a little-endian u16 selector, a little-endian u16 point index, i32 `-1`, u32 `0`) that is not an object and takes no index. An `sgPntPntDist` or `sgCircleDim` dimension writes little-endian u32 `0`, its parameter object in place (whose operands are the dimension's handles), and 34 bytes, or 32 when a class tag follows at +32 (36 after some placeholder entity handles). The last dimension of a group ends where the next group's non-zero count and a dimension's class tag follow, which can be at +32 or +34. A parameter's operand count can exceed the handles written: one `sgPntLineDist` counts 3 operands and writes two handles; a pattern count writes its operands, a back-reference and a null. An `sgValG3Dim` (a three-entity dimension) writes little-endian u32 `0`, its parameter object (two handle operands), the third handle and a little-endian u16 (`1` or `3`). When the parameter's operands already hold three handles, the third handle is not written again; and the last such dimension of a group has no u16, the next group's count following it instead. An `sgCircularPattCntDim` or `sgLinearPattCntDim` writes little-endian u32 `0` and a `moIntegerParameter_c`: the parameter layout with a little-endian u32 value in place of the f64. An `sgOffsetDim` or `sgSkOffsetDim` dimension writes little-endian u32 `0` and its parameter object, whose operands are written as little-endian u16 values (the dimension's index within its relation group, then `1`) rather than handle objects; no bytes follow. A parameter object's trailer (§ *Dimension display objects*) is 35 bytes, with the operand count at +27; in files of `swVersion` 18000 and above it is 36 bytes with the count at +28. A `suObList` object is an MFC `CObList`: a little-endian u16 count and that many object pointers.

An `sgExtEnt_c` object writes an object pointer to a `moSketchExtRef_w` object, which writes an object pointer to a `moCompSketchEntHandle_c` object. That object writes a component (a `moUnitComponent_c` or `moContextChainComponent_c` object), a little-endian u32 feature number, a little-endian u32 time stamp, a handle object, a little-endian u32, u16 `0` and an object pointer. When that pointer is null, 58 bytes follow (holding three i32 `-12345` at +20 and a version number at +36, and ending with twelve zero bytes, i32 `-1` and u16 `0`), or 54 in older files, which end with fourteen zero bytes after the version; when it is a second context component, twelve zero bytes, i32 `-1` and u16 `0` follow (fourteen zero bytes in older files). Then come an object pointer to a `moPointBackedUpData_c` object, and, when six zero bytes and a string marker follow, those six bytes and a Unicode `CString` naming the external entity (for example `Point1@Origin`). A `moPointBackedUpData_c` object writes three little-endian f64.

A `moCompRefPlane_c` object writes a `moUnitComponent_c` object, a little-endian u32 feature number and a little-endian u32 time stamp. When the feature number is `-1` the plane is a `moRefPlane_c` feature written in place (its name pointer null), followed by its plane-data object, and the object ends there. A plane reached from a sketch's external entity writes, after the time stamp, u16 `0` and the tail of a `moCompSketchEntHandle_c` instead. Otherwise it writes little-endian u16 `0`, the plane's little-endian u16 feature-object identifier, u16 `0` and a flag byte; twelve little-endian f64 (a transform) when the flag is `1` and three when it is `0`; little-endian f64 `1`; and a trailer: one or three zero bytes, a little-endian u32 (`4` or `3`), a little-endian i32, u32 `0`, a little-endian u32 identifier, a little-endian u32, a little-endian u16 count and that many identity hops of the face the plane lies on, a little-endian u16 count and that many identity hops of that face's boundary edges, and four zero bytes (31 bytes when both counts are zero; §2 describes its start). The object is 70 bytes after the time stamp without the transform and 142 with it when the counts are zero. A plane in an assembly's context (`swVersion` 17000) writes, after the identifier and u16 `0`, little-endian u16 `1` and a `moContextChainComponent_c` (form C, below), four f64 (a box), six f64 (a point and a normal) and its frame twice: the flag byte, f64 values and f64 `1`; then 31 zero bytes, a byte `1` and 15 zero bytes; then the second frame and the trailer. A hop's producer pointer can write the producing feature in place, so a sketch on a face of an earlier feature holds that feature, and the features it references, inside its plane reference.

A `moDimText_c` (sketch text, `swVersion` 17000) writes the text as a Unicode `CString`, its position (three little-endian f64), little-endian u32 `1` and a `utCharFormat_c`. The `utCharFormat_c` writes the font name (`CString`); eight eight-byte slots: the height as an f64, or f64 `-1` and the point size as a little-endian u32 in the next slot, then a spacing f64 and a width factor `1` among the others; a little-endian u32 (`0` or `1`, or a u32 code followed by u32 `1`); four f64; and the text's outlines: a little-endian u32 loop count *n* (three for a `B`), *n* little-endian u32 segment counts summing to *S*, *S* little-endian u32 segment kinds (`1` or `2`), *S* little-endian u32 point counts summing to *P* (a segment's end points are counted in each segment), and *P* points of three f64. The `moDimText_c` then writes little-endian u32, u32 and a character count *m*; when *m* is non-zero, *m* character positions (three f64), *m* little-endian u32 loop counts (in reverse character order), eight zero bytes and *m* directions (three f64); two f64 (`1e-6`), 18 zero bytes, f64 `1` and little-endian u32 `0`. A text whose u32 after its position is `3` has a null format pointer, and the format's flag, four f64 and the outlines follow in place. Text on a path then writes a `suObArray` of the path's curve references (whose handle tails write u16 `1` where others write u16 `0`), six f64, nine zero bytes and a compressed body (the path's geometry). Both forms end with the text again (`CString`), little-endian u32 `1` and u32 `1`. A list of text records is preceded by two zero bytes in place of a curve list's header.

A `moSketchRegion_c` region writes an object pointer to a `suObList` of the handles of its boundary curves; when 30 zero bytes do not follow, the region ends there. Otherwise it writes 30 zero bytes, two little-endian u32, and one or more `moSketchChain_c` objects written in place. A `moSketchChain_c` chain writes a little-endian u16 count *n*, *n* little-endian u32 curve identifiers, a little-endian u16 count *k* and *k* little-endian u32 (*k* is `1`, `2` or `3`; `2` and `3` in a sketch holding text), six little-endian u32 (`6`, `-1`, `0`, `0`, `5`, `5`), a Unicode `CString`, a little-endian u16 count (`0` or `2`) and that many handles, a little-endian u32 and twenty zero bytes.

An *in-context group* records sketch geometry converted from another document in an assembly. It writes *k* `moSketchExtRef_w` objects, *k* little-endian u32 identifiers, *k* handle lists (a little-endian u16 count and that many sketch handle objects), a little-endian f64 (an offset distance) and a little-endian u16, a little-endian u32 support type (`1` a face, `2` a loop), the support reference (a `moSingleFaceRef_w` or `moLoopRef_w` object), `00 00` for a face or little-endian u32 `2` and u32 `0` for a loop; then, in an in-context part, little-endian u16 `1`, a `moContextChainComponent_c` object, twelve zero bytes and i32 `-1` (twelve zero bytes alone after a component written at version 14000), or otherwise eight zero bytes and the handle block of a `moCompSketchEntHandle_c` up to its i32 `-1` (three values in place of its three i32 `-12345` in older files); for a face, four little-endian f64; then a little-endian u32 count and that many compressed bodies; for a face, u16 `0` when bodies were written and two little-endian u32; then one or more chains. A chain writes a little-endian u32 count *c*, *c* little-endian u32 identifiers, a little-endian u32 (`0`, `1` or *c*), a little-endian u32 (the chain's index in its block) and six little-endian f64, and is followed by a little-endian u16: `1` when another chain follows, otherwise the next group's size, `0` after the last group. A group's size can be `0` before the last group: an empty group writes ten zero bytes, a little-endian u32 support type, `00 00 01 00 01 00` and its chains, which omit the u32 after their identifiers. A non-zero word after the last group introduces one more chain list before offset-entity data. A group with support type `0` (no support; an offset group, with a non-zero offset distance) writes u16 `0` and then its chains. An offset group's chain can have no identifiers: u32 `0`, then the group's size, u32 `1` and u32 index. A chain whose index is `0` and whose sixth f64 after an extra f64 is followed by u16 `2` and an `sgPGMProfileData_c` carries offset entities (the u32 after its identifiers can differ from `0`, `1` and the count): an f64 offset distance precedes its six f64, and u16 `2` and the offset profile data (`sgPGMProfileData_c`) follow it in place of its u16 and of the settings' u32 `0` and `00 00 02 00`. A *compressed body* writes a zero byte, a little-endian u32 *A*, a zero byte, a little-endian u32 *B* = *A* − 5 and *B* bytes: a 16-byte identifier (`23 1d d5 71 da 81 48 a2 a8 58 98 b2 1b 89 ef 99`), the little-endian u32 uncompressed and compressed lengths, a zlib stream that inflates to a Parasolid transmit file, and eight bytes.

A `moSketchExtRef_w`, `moSingleFaceRef_w`, `moLoopRef_w`, `moLineRef_w`, `moCircleRef_w`, `moCylinderRef_w` or `moSurfaceRef_w` object writes an object pointer to the referenced model entity (`moCompEdge_c`, `moCompFace_c`, `moCompLoop_c`, `moCompSketchEntHandle_c`). For a reference that keeps a backed-up copy, 58 zero bytes and the tail of a `moCompSketchEntHandle_c` follow (from its little-endian u32 after the handle): in an in-context part the component pointer is the `moContextChainComponent_c` the copy was taken through, followed by twelve zero bytes, i32 `-1` and u16 `0`. Then comes the backed-up geometry: a `moLineBackedUpData_c` object (two `moPointBackedUpData_c` objects and a little-endian f64) or a `moArcBackedUpData_c` object (two points, a little-endian f64, little-endian u32 `1`, a centre point and a little-endian f64). When six zero bytes and a string marker follow, those bytes and a Unicode `CString` naming the entity end the object. `moCompLoop_c` has the layout of `moCompEdge_c`, and `moLoopRef_c` that of `moEdgeRef_c`.

The relation groups' meaning, the fields this section leaves unnamed, the compact `ff ff 07 00 01` record generation, and the 4 zero bytes some sketches write after the relation groups are open item EV-16.

#### In-context references

A part edited in the context of an assembly refers to other documents through context components. A `moContextChainComponent_c` object stands where a `moUnitComponent_c` may: it writes little-endian u16 `2`, an object pointer to its `moReferenceChain_c`, an object pointer to a component, the bytes `00 00 04 00 01 00 00 00` (`05` in place of `04` in some), a little-endian u32 flag word (`0` or `0x4000`) and a little-endian u32 identifier. When the next u32 repeats the identifier (form A), it writes that u32, a little-endian u32, 24 bytes (mostly zero), sixteen `ff` bytes, 20 bytes and the optional version number. In form C (`swVersion` 17000) it writes one transform block, ten zero bytes, four little-endian u32, the version number, twelve zero bytes and little-endian i32 `-1`, and ends there. Otherwise (form B) it writes two transform blocks, six little-endian u32 (the first two `0`, or a pair such as `0x2007, 0` or `0, 3`) and the optional version number. A transform block writes `01 00`, a flag byte, twelve little-endian f64 when the flag is `1` and three when it is `0`, little-endian f64 `1` and a zero byte.

A `moReferenceChain_c` object writes a little-endian u16 (`0` or `1`) and an object pointer to a `moExtFeatureList_c`, which writes a little-endian u32 count and that many `moExtFeature_c` objects. A `moExtFeature_c` object writes object pointers to the `moCStringHandle_c` objects holding the referenced document's path and title, three bytes (`03 00 00`), a little-endian u32 time stamp, three Unicode `CString` values, a zero byte, a `CString`, i32 `-1`, a zero byte, a little-endian u32, an object pointer to the `moCStringHandle_c` holding the component instance name, u16 `0` and a little-endian u32 (usually `-1`). The `moExtFeatureList_c` holding them writes, after its features, a little-endian u32, the configuration name as a Unicode `CString` and a little-endian u32 (`0`, or `-1` when the name is empty); a list holds one feature per assembly level. A `moExtObject_c` object writes 12 bytes after its configuration name in files of `swVersion` below 17000 and 16 from 17000.

A `moFaceRefPlnData_c` object has the layout of `moDefaultRefPlnData_c` with these differences: the three f64 after the two u32 and the byte are absent; the u32 that follows is `1` or `5`; and after the twelve final bytes, an object pointer follows (a `moCompFace_c`, or a `moCompRefPlane_c` when the u32 is `5`). A copy of the face's geometry can follow, after either the in-context sequence below or a handle-style block (u32 `0`, u32 `0`, u32 `1`, three i32 `-12345`, the version, i32 `-1`). When the geometry has no trailer of the form below (files below `swVersion` 15000, and some later), it is the plane as a mirror pattern's plane writes it: a box (f64 `-1`, `1`, `1`, `-1`), a point and a normal, a flag byte, three or twelve f64, f64 `1` and zero bytes up to the next object. Otherwise its layout is not decoded; it ends with a fixed trailer: a transform block's f64 `1`, one or three zero bytes, little-endian u32 `4` (`2` or `3` in some), i32 `-1`, u32 `0`, a little-endian u32 identifier, a little-endian u32 and either six zero bytes (eight, or four before an object at `swVersion` 11000) or, as in the plane trailer, a little-endian u16 count and that many hops, a little-endian u16 count and that many hops, and four zero bytes (open item EV-18). A `moAbsoluteRefPlnData_c` (a thread's plane) has the layout of `moDefaultRefPlnData_c` without the three f64 before the u32 `1`; `moFixedRefPlnData_c` has the same layout (a hole wizard's sketch plane, written in place with the frame and short trailer). A plane defined by references (`moConstraintCoincPlaneRefplaneData_c`, `moConstraintMidPlaneRefplaneData_c` and the other `moConstraint…RefplaneData_c` classes) has the same layout without those three f64, a u32 that is not `1` (for example `29`), and after the twelve final bytes a `moConstraintRefplaneInterfaceData_c`. That object writes three object pointers, one per reference slot: a geometry object (`moRefPlaneCoincidentGeom_c`, `moRefPlaneMidPlaneGeom_c`, `moRefPlanePerpendicularGeom_c`, …) or null. A geometry object writes the reference (a `moPlaneRef_w`, or null); when the reference's entity is a face, its handle block and the plane as a mirror pattern's plane writes it (a box, a point and a normal, a flag byte, three or twelve f64, f64 `1` and 21 zero bytes) follow, and a `moCompRefPlane_c` entity has that form itself. Then come u32 `0` after a reference, two little-endian f64 and 24 zero bytes; a line reference whose slot ends at once writes none of them and the next geometry object follows. In an in-context part, `00 00`, little-endian u16 `1`, a context component, twelve zero bytes, i32 `-1` and the face's geometry follow: six f64 (a box), an f64, three f64 (a normal), a transform block without its leading `01 00` (a flag byte, twelve or three f64, f64 `1`), a flag byte and three f64, and further fields of varying length (open item EV-18).

`moCompVertex_c` has the layout of `moCompEdge_c`, `moVertexRef_c` that of `moEdgeRef_c`, and `moTempAxisRef_w` that of `moSketchExtRef_w`.

A `moOneContour_c` writes a little-endian u16 count and that many edge references. An edge reference whose first u32 is `2` has a 36-byte header and holds a `moVertexRef_c` before its counted hops. A `moVertexRef_c` writes a 38-byte header (i32 `-1` twice, a little-endian u32 count, a u32, u32 `0`, two f64 and u16 `0`), its hops, eight zero bytes, the optional version number and u32 `0`, then a little-endian u32 count and that many points of three f64 (a u32 `2` can stand in place of the first four of the eight zero bytes). A vertex reference from a sketch (`moCompVertex_c`) is followed by six zero bytes, the 58-byte block (54 in older files; and in some older files the three i32 `-12345` of a handle block are other values) of a `moCompSketchEntHandle_c`, the backed-up point and an optional name; in an in-context part, by u32 `0`, a `moContextChainComponent_c`, twelve zero bytes, i32 `-1`, u16 `0`, the backed-up point and the name. A `moContourChooser_c` with several contours, each with its own boxes, writes a little-endian u16 count *G* and *G* groups of a `moOneContour_c`, a u16 count and that many boxes, then the fields below once. With no contours it writes u16 `0`, u16 `1`, a null box, a u16 identifier, two little-endian u32, a u16, a `suObArray` of `moSketchChain_c` objects and the two hop lists; or the fields below without the u32 that follows a box. Otherwise it writes a little-endian u16 count and that many `moOneContour_c` objects, then a little-endian u16 count and that many entries of a `moBBoxCenterData_c` (here u32 `1`, four f64, u32 `1`, a u16 identifier, u16 `1`, fourteen zero bytes and a u32), a little-endian u32, a u16 identifier, two little-endian u32, three little-endian u16, a u16 count and that many face hops, and a u16 count and that many edge hops. In both forms the second and third of the three u16 can be replaced by a `suObArray` of `moSketchChain_c` objects, as in the form with no contours. A box whose u16 after the identifier is `2` writes two more zero bytes after its u32. A plane trailer's u32 type can be `2`, and the form with the u32 can end with two, six or eight zero bytes before the next object; the short trailer of an in-place plane can be twelve zero bytes in place of its last three u32 (an edge flange's or hole wizard's sketch). A plane reference's trailer, and the trailer of an in-context face geometry, can omit the u32 after the identifier; it then ends with two zero bytes rather than four. When a `moContourChooser_c` follows the identifier, the trailer writes a little-endian u16 (`1` or `6`), the chooser, a little-endian u16 count and that many hops, and two zero bytes.

A dimension parameter's trailer opens with a zero byte and an object pointer to its tolerance (null when it has none). A `moDimTolerance_c` writes a little-endian u32 kind, a u32, two f64 (the limits), a group of a byte `1`, three f64 and two `CString` values, a byte, u32 `1`, a byte `1`, three f64, six zero bytes, three `CString` values and u32 `0`.

A `moTempAxisRef_w` temporary axis writes an object pointer to the face it comes from (in older files the zero runs after the version, or after a context component, are twenty bytes with no i32 `-1`); then `00 00`, little-endian u16 `1`, an object pointer (a context component, or null); after a context component twelve zero bytes and i32 `-1`; after null, u32 `0`, a little-endian u32 identifier, twelve zero bytes, three i32 `-12345`, u32 `0`, the optional version number, twelve zero bytes and i32 `-1`; then eight zero bytes, seven little-endian f64 (a point, a scalar and a unit direction), 59 zero bytes (7 in files of `swVersion` 14000) and a Unicode `CString`.

A `moCompEdge_c` written in a fillet's edge list is followed by 62 zero bytes (the 58 zero bytes and the zero u32 that open an empty backed-up tail). A `moCompFace_c` or `moCompEdge_c` whose component is an in-context one of form A writes a `CString` (`NONE`) after its byte. A mark record of the id-range form whose hops are such faces writes u16 `0` and the face's GUID (`CString`) after each, and then sixteen zero bytes and a second hop list and reference list before its six zero bytes. An `edgeRadiusObject_c` can end after its edge's 66 zero bytes when the next radius follows, and its byte `1` is `0` in another.

A `Fillet_c` fillet writes a feature, 58 bytes, and a `moPMarkRecord_c` in its *id-range* form: a little-endian u32, a little-endian u32 identifier, u32 `1`, a second identifier, sixteen zero bytes, a little-endian u32, 30 zero bytes (16 in some), a little-endian u32 count and that many identity hops, a little-endian u16 count and that many edge references, and six zero bytes — or, in place of the six bytes, a `suObList` of edge references (a chamfer's; in files of `swVersion` 12000 there are no six bytes). Then come `suObList` objects (the first holds the filleted edges), little-endian u32 `1`, u32 `0`, a `suObList`, u16 `0` and a `suObList` of `edgeRadiusObject_c` objects. An `edgeRadiusObject_c` writes its radius display dimension, a `moCompEdge_c`, 66 zero bytes, a byte `1`, a `suObList`, 58 bytes (two f64 `0.5` among them), a little-endian u32 count and that many component objects. Then come edge and face objects (`moCompEdge_c`, `moCompFace_c`), an edge preceded by a little-endian u16 (`0`, `2` or `16`) and sometimes followed by its 62-byte tail; a run of zero bytes and eight `ff` bytes; eight zero bytes; a little-endian u32 count *K* and *K* entries of two little-endian u32 (the second a flag), the radius as a little-endian f64 and a `moCompEdge_c` with its 62-byte tail; and a constant 40 bytes (u32 `6`, u16 `0`, u32 `0x5230`, `0x19`, `0x5b2d`, `0x58b7`, `0x59d1`, fourteen zero bytes). A face fillet writes a `moCompFace_c` and u16 `0` between its `suObList` objects, `faceRadiusObject_c` radii, little-endian u32 `0` and a u32 identifier in place of the eight zero bytes, and faces in place of the entries' edges. A `faceRadiusObject_c` writes its radius display dimension, a `moCompFace_c`, u16 `0`, a byte `1`, a `suObList`, 58 bytes (two f64 `0.5` among them), a little-endian u32 count *n* and *n* triples of a `moCompFace_c`, a null pointer and a `moCompEdge_c`. In fillet identity hops, the u16 before the `moFR_c` can be non-zero. A `moIndirectSurfIdRep_c` hop wraps one child hop, as the mirror-pattern hop does.

Offset entities: an `sgPGMProfileData_c` object writes a little-endian u16 count and that many `sgPGMLoopOffsetData_c` loops, a little-endian u16 (`0x245`), the offset distance as a little-endian f64, zero bytes (eight, or 22), and a little-endian u32 count *n* with *n* little-endian u32 loop identifiers (those of the next profile). When *n* is `0` the profile ends; otherwise a little-endian u32, u32 `0`, the next offset distance as an f64, six f64 (two points), u16 `2` and the next `sgPGMProfileData_c` written in place follow. An `sgPGMLoopOffsetData_c` writes two little-endian u32 identifiers and a little-endian u16 count and that many `sgPGMLoopOffsetEdgeData_c` objects (one little-endian u32 each). A sketch with several offsets thus writes `00 00 02 00` and a chain of profiles.

A `moContextUnitComponent_c` (a component in another configuration or document) stands where a `moUnitComponent_c` may. It writes little-endian u16 `2`, a little-endian u16, little-endian u32 `1`, the configuration name as a Unicode `CString`, 44 bytes, sixteen `ff` bytes, 20 bytes and the optional version number. A `moSolidRef_w` writes one object pointer (a `moCompSolidBody_c`). A `moSavedExtBody_c` saved body writes a feature, a feature body head, a `moPMarkRecord_c`, sixteen zero bytes, a `moSolidRef_w`, a little-endian u32 count and that many u32 identifiers, i32 `-1`, u32 `0`, four little-endian u32, a transform block, ten zero bytes, four little-endian u32, the optional version number, twelve zero bytes, i32 `-1`, u32 `0`, a little-endian u32 count and that many compressed bodies. Then come u16 `1`, an object pointer, a rotation (nine f64), u32 `1`, a translation (three f64), u32 `0`, f64 `1`, 55 zero bytes, the configuration name as a Unicode `CString` and u32 `0` (four more zero bytes when the stream ends there). A `moDerivedSurfIdRep_c` identity hop wraps one child hop and ends with a little-endian u32.

A `moMBimport_c` import feature writes a feature (whose inputs are the imported tree's folders, below), a feature body head, a `moPMarkRecord_c`, the import's name and source path as Unicode `CString` values, little-endian u32 `1`, a little-endian u32 body count *n* and *n* pairs of a little-endian u32 body identifier and a `mbBodyImportIDMaps_c` object; then a little-endian u32, u16 `4`, u16 `1`, u16 `0`, a FILETIME, the source format and schema as `CString` values (`STEP`, `AP214`), u32 `0`, a little-endian u32 and u32 `0`. A `mbBodyImportIDMaps_c` object writes a little-endian u32 count and that many entries of a `CString` (`MISSING_PERSISTENT_ID`) and two little-endian u32.

#### Dimension display objects

A feature or sketch dimension is shown by a display object, `moDisplayDistanceDim_c`, `moDisplayRadialDim_c`, `moDisplayAngularDim_c`, `moDisplayDiameterDim_c` or `moDisplayRevolveDim_c`. In files of `swVersion` 14000 and above, each writes 151 bytes, a little-endian u32 count and that many little-endian u32 object identifiers, 261 bytes, a Unicode `CString`, and 132 bytes; in older files the object is 404 bytes long with no count entries and no string. It then writes an object pointer to its dimension handle and one to its `moFavoriteHandle_c`. The 122 bytes once read after a `moDisplayRevolveDim_c`'s favourite are that handle's view block.

A parameter shown by a feature dimension (under a `ParallelPlaneDistanceDim_c` or `AngleDim_c`) writes, after its value, a zero byte and its `moDimTolerance_c` when it has a tolerance (some parts), or else a 23-byte trailer (three zero bytes, the little-endian u32 object identifier, sixteen zero bytes) and then a placement block: a prefix of 84 bytes plus 82 bytes for each of a number of items, four Unicode `CString` values, 262 bytes (234 for a `moScalerParameter_c` or `moIntegerParameter_c`), a `CString`, 40 bytes and a `CString`. The prefix's item count is open item EV-17. The dimension objects under a `moFeatureDimHandle_c` can be `ParallelPlaneDistanceDim_c`, `AngleDim_c`, `moScalerDim_c`, `ThreeDRadiusDim_c`, `ThreeDdiameterDim_c` and `moNumberDim_c` (integer); each writes little-endian u32 `1` and an object pointer to its parameter.

A `moFeatureDimHandle_c` object writes 42 bytes, sixteen `ff` bytes, 20 bytes, the optional version number (as in `moUnitComponent_c`), 14 bytes, a zero byte, little-endian i32 `-1` and six zero bytes (103 bytes, or 107 with the version), then an object pointer to the dimension object it shows (for example a `ParallelPlaneDistanceDim_c`, which writes little-endian u32 `1` and an object pointer to its parameter). When the dimension object is new but its parameter is a back-reference, the parameter's post-value data and placement block follow the dimension object (sheet metal). In both back-reference forms the placement gap is 234 bytes for a count (an integer parameter) and 262 otherwise. When that pointer is a back-reference to a dimension written before, the handle itself carries the dimension's placement block: six bytes, four `CString` values, 262 bytes, a `CString`, 40 bytes and a `CString` (files of `swVersion` 12000). A sketch dimension handle (`moSkDimHandleValG2_c`, `moSkDimHandleRadial_c`, `moSkDimHandleOffset_c`) writes the same 103- or 107-byte prefix, 12 bytes, four Unicode `CString` values and 310 bytes; `moSkDimHandleLinearPattCnt_c` and `moSkDimHandleCircularPattCnt_c` write 282 in place of the 310. `moDisplayScalarDim_c`, `moDisplayCircularPattCntDim_c` and `moDisplayLinearPattCntDim_c` have the display-object layout.

A `moFavoriteHandle_c` object writes 206 bytes, four Unicode `CString` values, 92 bytes, two `CString` values, 12 bytes, a `CString`, 40 bytes, a `CString`, a little-endian u32 flag word and 50 bytes: 436 bytes when its strings are empty. When bit 7 of the flag word is set, 52 bytes, a `CString` and 18 bytes follow (14 in files below `swVersion` 16000). When bit 16 is set, the 50 bytes are 85: a byte `1`, ten little-endian f64 and a little-endian u32; with bit 0 also set, little-endian u32 `0` and a `CString` follow. Other tails: none after the last `CString` for a pattern-count dimension's handle, or when an object follows at once; u16 `0` when an object follows it (a circular pattern's); 28 bytes (u32 values `0`, `0`, `1`, `0`, `1`, twelve zero bytes) before a reference object; a flag and 46 bytes below `swVersion` 15000 when neither bit 7 nor bit 16 is set (`0x200`); and, before an extrusion's 76-byte block, 54 zero bytes, 50 in files of `swVersion` 14000 and 52 (open item EV-20); 52 rather than 50 before a sketch's in-context block, told by what follows: a header read without the two bytes gives group counts or a first chain count that is not a count. When bits 11 and 15 of the flag word are both set (`0x388886`, `0x388896`, `0x1b88886`, `0x1b88896`), the handle ends with a view block: little-endian u32 (`0` or `1`), nine little-endian f64 (two directions and a point), eight zero bytes, a flag byte, three or twelve little-endian f64 (twelve when the flag is `1`), f64 `1`, a zero byte and little-endian u32 `1` — 122 or 194 bytes. No handle without both bits writes it.

#### Extrusion feature objects

After its feature body head (*x* `1`), an extrusion feature writes a little-endian u16 and then one or more per-body units. A unit is a `moPerBodyChooserData_c` object (a little-endian u16 count, that many object pointers to face references, and the bytes `00 00 01 00`) followed by a `moBBoxCenterData_c` object; a `moPerBodyChooserDataWithFileName_c` has the same layout, and its box writes, after its flag word, u16 `0`, a GUID `CString` and u32 `0`. A `moBBoxCenterData_c` object writes little-endian u32 `1`, four little-endian f64, two little-endian u32 `0`, a little-endian u32 flag, a further u32 `0` when that flag is `1`, little-endian u16 `0`, a little-endian u16 identifier, a little-endian u16 continuation flag, a little-endian u32, the identifier again when the continuation flag is `1`, and eight or twenty bytes of little-endian u32 values `0`, `1` or `0x10001`; another unit follows when the last of those values is `0x10001`. A six-byte tail of little-endian u32 `1` and u16 `2` precedes a `moCompSolidBody_c` declaration (`swVersion` 16000). In an *id-list* box (`swVersion` 14000 to 17000) a little-endian u16 count *n*, *n* little-endian u32 identifiers `100000`+*k* and u32 `0` stand in place of the flag word, its u32 and the u16 `0`; the fields from the identifier on follow, and the closing bytes are ten bytes of `0` and `1` when no unit follows. A `moPMarkRecord_c` object and a `moEndSpec_c` object follow the units. Then come the display objects of the feature's dimensions, 76 bytes (`00 00 01 00 00 01 00 00`, 56 zero bytes, little-endian u32 `1` — `0` before a thread — and eight zero bytes), a `moFromEndSpec_c` object, an object pointer (null, a `moThinSpec_c` object in a thin extrusion, or a feature written in place) and a second object pointer: null, or the next feature written in place (`moICE_c`, `moSweepThread_c` or `moScale_c`); both are absent when the extrusion is the stream's last object. A `moThinSpec_c` object writes two dimension display objects, a little-endian u32, the wall thickness as a little-endian f64, a little-endian f64, eight bytes, a third dimension display object and twelve zero bytes.

A `moEndSpec_c` object writes sixteen bytes and a little-endian u32 end type at +16, then a little-endian u32: 24 bytes for types `0`, `5`, `6` and `9`. Types `1` and `2` write two more bytes and, for a second direction, a 76-byte block of the extrusion's kind (recognised when another such block follows it). Type `4` (up to a surface) continues with little-endian u32 `0`, a byte `1`, a little-endian u16 count, and that many face references, each followed by the in-context support sequence of a sketch's face group (`00 00`, u16 `1`, a context component, twelve zero bytes, i32 `-1`, four f64, a count and compressed bodies — or, outside an assembly context, the face form of the handle block, four f64 and the count), then five bytes (three in files of `swVersion` 15000). Type `3` (up to a vertex) continues with u32 `0`, four zero bytes and a `moEndPointRef_w`; in an in-context part that reference writes zero bytes, u16 `1`, a context component, twelve zero bytes, an optional i32 `-1`, the point (three f64), the fraction along the edge as an f64 and u16 `0`. An extruded cut can write, in place of its `moPMarkRecord_c`, 64 zero bytes before its `moEndSpec_c`; its bounding-box record then ends with 20 bytes of u32 values `0` and `1`. The extrusion's 76-byte block is 78 bytes in some parts, with two more zero bytes after its first four. Its two bytes at +40, zero in the plain block, are a pointer slot: a drafted extrusion of `swVersion` 14000 to 19000 writes there the draft angle's display dimension (a `moDisplayAngularDim_c`), with u32 `1` at +16, and the block's last 34 bytes (22 zero bytes, u32 `1`, eight zero bytes) follow the dimension. The extrusion's 76-byte block then writes a non-zero byte at +1 and the offset from the surface as a little-endian f64 at +8. A `moFromEndSpec_c` object writes twenty zero bytes, a little-endian u32 flag in files of `swVersion` 16000 and above, a little-endian f64 offset and a little-endian i32: 32 or 36 bytes. A start other than the sketch plane replaces the first zero bytes: type `5` (offset) writes u32 `5`, the offset's display dimension, a u32 and twelve zero bytes; type `1` (from a face) writes u32 `1`, a `moSingleFaceRef_w`, its handle block, four f64, u32 `0`, two u32 and the twenty zero bytes (sixteen in some); the fields above follow. A face of another part of an assembly (`swVersion` 17000) writes, after the `moSingleFaceRef_w`, the in-context support sequence of a sketch's face group (`00 00`, u16 `1`, a context component, twelve zero bytes, i32 `-1`, four f64, a count and compressed bodies), u16 `0`, two u32 and sixteen zero bytes.

An extrusion that ends at a face writes its 76-byte block in two parts: the first five bytes, `00 00 01 01 00` (byte 3 set), then a `moSingleFaceRef_w`, its handle block, four f64 (after u16 `0` in files of `swVersion` 15000), u32 `0` and two little-endian u32 identifiers, then the block's remaining 71 bytes (`01 00 00`, the f64 offset, 48 zero bytes, u32 `1` and eight zero bytes in files of `swVersion` 14000, 15000 and 17000). A reference's handle block can end with twelve zero bytes and no i32 `-1` in files of `swVersion` 14000 and 15000.

A `moPMarkRecord_c` object writes a little-endian u32 and a little-endian u32 form flag. In the flag-`0` form a further little-endian u32 can precede the object (a record with X `5`); after the object and its u16, further pairs of a GUID `CString` and a face object can follow, and a last GUID `CString`. The object pointer and u16 after the 32 zero bytes and the u32 are, when the next word is not a class token, a little-endian u32 count *n*, *n* eight-byte entries (a little-endian u32 Unix time stamp and a u32; or, when that u32 is `-1`, 24-byte entries: the time stamp, i32 `-1`, u32 `1`, `2`, `0`, `1`) and, when *n* is non-zero, little-endian u32 `0`; a count of zero writes the same four zero bytes as a null pointer and u16 `0`. With flag `1` it writes a little-endian u32 count *n*, *n* six-byte entries, eight zero bytes, little-endian u32 `20`, u32 `0`, u32 `20` and sixteen zero bytes. With flag `0` it writes 32 bytes — twelve bytes (zero, or a u32 time stamp first, `swVersion` 15000), then two lists each of a little-endian u32 count, that many pairs of u32 (body identifiers: a body mirror's) and u32 `0`, then u32 `0`; zero when both lists are empty — a little-endian u32, an object pointer, a little-endian u16 and a little-endian u32 *g*: fourteen zero bytes follow when *g* is `0`; when *g* is `1`, a little-endian u32, a Unicode `CString` holding a GUID, fourteen zero bytes and, when a string follows, an empty `CString`, little-endian u32 `1` and a `CString` naming the body (a base body's import name).

A `Chamfer_c` chamfer writes a feature, 58 bytes and a `moPMarkRecord_c` (id-range form), `suObList` objects and the display objects of its dimensions. In files of `swVersion` 15000 and 16000, three zero bytes, `0a 01`, the chamfered edge (`moCompEdge_c`), 66 zero bytes, u32 `1`, a 40-byte block of the fillet's form (u32 `6`, u16 `0`, five u32 values, fourteen zero bytes; `0x58b6` and `0x59cf` where the fillet writes `0x58b7` and `0x59d1`), u32 `1`, `00 01` and, in some, four zero bytes follow; files of `swVersion` 14000 and 16000 can write `00 00 01 0a 01` for the first five bytes, and files of 17000 and 19000 write the edge without them. A chamfer of `swVersion` 17000 can also open with a face or edge, u16 `0`, a `suObList` and its display objects before the five bytes and the edge. Other chamfer forms are open item EV-17.

A revolve (`moRevolution_c`, `moRevCut_c`) shares the extrusion's head up to its `moPMarkRecord_c`. Then come nine zero bytes (with u32 `1` at +5 in some files, whose `moRevEndSpec_c` also holds a u32 `6` at +8 of its 24 zero bytes), a `suObList` (empty), a little-endian u32 (`3` or `5`), a little-endian u32, the axis reference (a `moLineRef_w`), u32 `0` in files of `swVersion` 16000 and above, seven f64 (a point on the axis, its direction and a value) and a zero byte, then a `moRevEndSpec_c` (a little-endian u32 end type, 24 zero bytes, two f64 (`0.01`) and eight zero bytes), the display objects of its dimensions, and six zero bytes (seven from `swVersion` 18000, and in some `swVersion` 17000 revolves; when an object follows at once the run is seven and not six). It writes no 76-byte block, `moFromEndSpec_c` or trailing pointers. The axis's handle tail writes u16 `1` where a sketch entity handle writes u16 `0`.

A `moHelix_c` helix or spiral (`swVersion` 12000) writes a feature, u16 `0`, a little-endian u16 count and that many identity hops (`moWireEdgeIdRep_c`, a hop with four bytes of its own), a 142-byte block (u32 `1` at +30 and +50 and u32 `5` at +132), a compressed body, u32 `6`, eighteen zero bytes, u32 `1`, five f64 (a radius, the pitch, the number of revolutions, a start angle and the height), zero bytes (41), the display objects of its dimensions and a zero byte. A favourite handle among those display objects can end with its flag word (`0` or `2`) when the next display object follows at once. In files of `swVersion` 17000 the block before the compressed body is 148 bytes, and the eighteen bytes after u32 `6` hold an f64 (π/2) at +6. A favourite handle whose flag has neither bit 7 nor bit 16 set can also end with six zero bytes, a `CString` and 28 bytes (u32 `0`, `0`, `1`, `1`, i32 `-1`, eight zero bytes) before the next feature (sheet-metal features).

A sweep cut (`moSweepCut_c`) shares the extrusion's head up to its `moPMarkRecord_c`, then writes u16 `1`, its path (a `moGeneralCurveRef_w`, which holds a `moCompProfile_c`), u16 `1`, a `moWireContact_c` (three f64, u32 `0`, u32 `1`, i32 `-1`, u16 `2` and three f64), the cross-section (a second `moGeneralCurveRef_w`, which holds a `moCompReferenceCurve_c`; read as a bare u16 and pointer, its object index is lost and every later class tag in the stream is misread), 105 bytes of options, a pointer to the guide curve (a back-reference to the helix) and 17 bytes. A `moCompProfile_c` or `moCompReferenceCurve_c` writes a component, a little-endian u32 feature number, a little-endian u32 time stamp, a handle block (two or three i32 `-12345`, u32 `0`, the version), zero bytes, f64 `1`, a flag byte, a compressed body when the flag is `1` (the path's sketch), eight `ff` bytes and six zero bytes. Other sweep forms are open.

Sheet metal (`swVersion` 15000 to 17000). A `moTemplateSheetMetal_c` feature writes the feature, a 122-byte block (u32 `3` at +20, bytes `1` at +34 and +54), the bend radius's display object, 15 bytes, the bend table's path as a Unicode `CString`, u32 `1`, a byte `1`, two display objects (the K-factor, whose placement gap is 262 bytes rather than a scalar's 234, and the thickness), u32 `1`, u32 `0`, four `CString` values, and two groups of bytes (46 and 4), each followed by a display object. A `moSMBaseFlange_c` base flange shares the extrusion's head up to its `moPMarkRecord_c`, then writes its display objects, 19 bytes, a `moCompSheetmetalFeat_c` (the layout of `moCompFeature_c`) and 75 bytes (74 at 16000, 71 at 15000: two flag bytes, u32 `1` twice and two f64 gaps among zero bytes). `moEdgeFlange_c`, `moTemplateFlatPattern_c` and the other sheet-metal features are open.

A cut-list folder (`moCutListFolder_c`, a child of `moSolidBodyFolder_c`) writes the feature, twelve zero bytes, a `suObList` of the bodies in the list, then items (u32 values with an i32 `-1` among them and zero bytes, 38 bytes, and a component: `moCompSolidBody_c` or `moCompFeature_c`), and u32 `1`, i32 `-1`, u32 `2`, 23 zero bytes, u32 `1`, u32 `1`. Its body folder then writes u32 `1`, six zero bytes and a `moCutListSortOptions_c` (a little-endian u32 and three u16) in place of its twelve zero bytes.

A `moSurfaceAxisData_c` (a reference axis through a cylindrical face) writes fourteen f64 (points, the axis direction and the radius), a `moCylinderRef_w` with its handle block and the cylinder's geometry. In a feature body head with *g* `1`, bodies after the first can be preceded by a little-endian u32 count *n*, *n* little-endian u32 identifiers, a u32 (`-1`, or a value) and eight zero bytes (a cut's feature scope); a mark record's flag *g* can be `2`, writing two pairs of a u32 identifier and a GUID `CString`.

A body mirror (`moMirrorSolid_c`) writes the feature, its body head and `moPMarkRecord_c` (whose lists hold the mirrored body identifiers), 60 bytes (u16 `0`, u32 `1`, u32 `1`, f64 `1` at +34), a little-endian u16 count of bodies, each a `moCompSolidBody_c` followed by u32 *n*, *n* identifiers, i32 `-1` and two u32; u32 `0`; the mirror plane (a `moPlaneRef_w` whose `moCompRefPlane_c` has the mirror pattern's plane form); u32 `0`; a `moMatrixIndexHelper_c` (u16, a `suUIntArray` — a u32 count and that many u32 — u32, a u16 count and that many result bodies); u32 *n*, *n* identifiers, i32 `-1`, eight zero bytes, u32 `2`, six zero bytes, `01 01` and thirteen zero bytes. A mirror of two bodies writes fourteen more bytes in its helper (open).

A sheet-metal flat pattern (`moTemplateFlatPattern_c`, and `moFlatPattern_c` after a `moSheetMetal_c`, in files of `swVersion` 16000) writes the feature, whose block holds its bend-lines sketch features (and, for `moFlatPattern_c`, a `moFoldUpDownSketchFolder_c` followed by a `moFoldUpDownSketchXformData_c` of sixteen zero bytes and a `moFoldUpDownFeatXformData_c` of eight zero bytes before the block's twelve zero bytes), then 122 bytes (as a sheet-metal feature's), a null pointer and a `moCompFeature_c`. A `moTemplateFlatPattern_c` then writes 269 bytes ending in u32 `4` four times, i32 `-1`, u32 `0`, u32 `1` and an identifier, and a `moPerBodySheetMetalDataNew_c` (u16 `0` and two u32). A `moFlatPattern_c` writes 17 bytes, a `moCompSheetmetalFeat_c`, sixteen zero bytes, its two fixed faces (`moCompFace_c`, u16 `0` between), 66 bytes (two f64 `0.005` among them), a `moCompFeature_c` and 168 bytes of f64 values that end the stream (open item EV-23). A `moSheetMetal_c` has the layout of `moTemplateSheetMetal_c`. The bend-lines sketch's point records can end with twelve zero bytes and u32 `2`, its list headers' first u32 can be `0x20000`, and its tail can hold u32 `1` and an empty list before the marker. A feature block's twelve zero bytes after its children can be u32 `0`, u32 `1`, u32 `0` in other classes too, and a block's child count includes folders written in place inside earlier children (a mirrored part's; the block ends at its tail).

A configuration table (`moNativeConfigurationTableFeature_c`, a child of `moConfigTableFolder_c`) writes the feature, an empty `CString`, twelve zero bytes and i32 `-1`. A `moDetailFolder_c` whose features date from version 4400 (some parts) writes, after its u32 `0`, zero bytes (28), a frame (a flag byte `1`, twelve f64, f64 `1`) and zero bytes to the next object (43); the `moDetailCabinet_c` that holds it writes u16 `1` and a u16 identifier in place of its u16 `0`. A part with no configuration 0 keeps its features in the `Contents/Config-N` streams of its lowest-numbered configuration (`Config-121`, `Config-8199`, ...). In some exported parts an empty `moHistoryFolder_c` and a `moSelectionSetFolder_c` write neither the block tail's u32 nor the folder's u32 `0` before their own bytes, and a `moDetailCabinet_c` ends with two zero bytes. The layout of a mirrored part (`moMirrorStock_c`) and of an add-in's custom feature (`moMacroFeature_c`) is open (open item EV-23).

A reference point (`moRefPoint_c`) writes the feature, 112 bytes, the point (three f64), a little-endian u32 type (`4`: a face's centre), ten zero bytes and its data object; a `moFaceCenterRefPointData_c` writes the face reference, its handle block (whose face form can have twelve zero bytes after the identifier and no i32 `-1`), a box (four f64) and about fifteen bytes to the next object. A `moCompRefPoint_c` has the layout of `moCompRefAxis_c`, with the point's three f64 as its geometry. A `moFacePtRefPlnData_c` (a plane through a point on a face) has the layout of `moPerLinePtRefPlnData_c` with a kind of `6` and ends with eight zero bytes and u32 `1` after its references. A move-face feature (`moMoveFace_c`) writes the feature, its body head, units and `moPMarkRecord_c`, two f64 (distances), fourteen bytes of u16 flags, a u16 count and that many faces (`moCompFace_c`, u16 `0` between them), u32 `0`, a `suObList`, its display objects and 260 bytes (a frame among them) to the next object or the stream's end. A `moLineAxisData_c` (an axis along a line) writes fourteen f64, the line's reference and an axis tail.

A sketch picture (`moSketchBitmap_c`) writes the feature and a `moSketchBitmapData_c`: six f64 (an origin, two pixel scales, an angle), the width and height in pixels (little-endian u32), u32 `0`, the picture's name (`CString`) and u32 `1`; references and a further record follow (open item EV-23). A `moBaseBody_c` that ends its stream has no final u32 `0`. A thin feature's specification can end with eight zero bytes before the next object (`swVersion` 18000). A plane referenced from a face's plane data can take the mirror form (a box, a point and a normal, the frame and 21 zero bytes after the handle block's i32 `-1`) outside a mirror pattern. A version-12000 handle block's version can be `0` (some parts).

A combine-bodies feature (`moCombineBodies_c`) writes the feature, its body head, units and `moPMarkRecord_c`; its target body (`moCompSolidBody_c`); a little-endian u32 count and that many body identifiers; a u32 (the operation); eight zero bytes; a little-endian u16 count and that many tool bodies (`moSolidRef_w`); a u32 count and that many identifiers; a u32; a handle block (zero bytes, an identifier, u32 `0`, i32 `-1`, u32 `0`, three i32 `-12345`, u32 `0`, the version, twelve zero bytes, i32 `-1`), eight zero bytes and u32 `1` (and a zero byte in some). A `moCompRefAxis_c` (a reference axis used by a sketch) writes its component, the feature number, a time stamp, bytes up to a handle block whose identifier precedes its three i32 `-12345` by twelve zero bytes (u32 `0`, the version, twelve zero bytes, i32 `-1` after them), and the axis geometry (105 bytes: a value, a point, a direction, two f64 `0.5`); the next object ends it (open item EV-23). A reference's handle block can be the in-context form (u16 `0`, u16 `1`, a context component, twelve zero bytes and i32 `-1`; a surface axis's cylinder). An axis tail whose reference already holds its handle block up to the i32 `-1` and four zero bytes continues with four zero bytes and its seven f64. A linear pattern can end with thirteen zero bytes. An entity's name in an old handle block (some parts) is preceded by two zero bytes.

A `moMoldSrfBodyFolder_c` (a mould's Cavity or Core surface bodies, a child of `moSurfaceBodyFolder_c`) writes the folder, twelve zero bytes and a little-endian u32 (`2`). In some exported parts the u32 that follows a feature block's tail, and a folder's u32 `0`, are absent when the next object follows at once; a `moFavoriteFolder_c` there writes its u32 `1` alone.

A reference surface (`moRefSurface_c`, a surface derived from faces; `swVersion` 17000) writes the feature, a little-endian u16 count and that many identity hops of its faces, a little-endian u16 count and that many hops of its edges (`moSimpleEdgeIdRep_c` among them), 62 bytes, its `moPMarkRecord_c`, and 44 bytes: fourteen zero bytes, little-endian u32 `4`, ten zero bytes, u32 `1`, u32 `1` and eight zero bytes.

A hole-wizard feature (`moHoleWzd_c` in files of `swVersion` 16000) writes the feature, whose block holds two profile features (the placement and profile sketches), and an extrusion-style body head whose block after *x* `0` is u16 `1`, fourteen zero bytes and u32 `1`. Then come a little-endian u16 count and that many bodies it cuts (`moCompSolidBody_c`), a little-endian u32 count and that many identifiers, four u32, its `moPMarkRecord_c`, and the hole's data: six zero bytes, the diameter as an f64, u32 `1`, two parameter names (`Hole Dia.`, `Hole Depth`), u32 `200`, u32 `1`, u32 `0`, the standard, the fastener type and the size as Unicode `CString` values (`ANSI Metric`, `Hex Bolt - ANSI B18.2.3.5M`, `M5`), five f64 (`0`, `1`, `0`, `0`, `0`), a `CString`, two u32, the description (`CBORE for M5 Hex Head Bolt`), a `CString`, two u32, a `CString`, u32 `0`, a little-endian u32 count *n* and *n* entries of a u32 index and an f64, 52 zero bytes, a `CString`, twelve zero bytes, u32 `1`, u32 `1`, i32 `-1`, eight zero bytes, two `CString` values, u32 `0`, an f64 (`0.01`), twelve zero bytes, a u32 (`0` or `1`) and ten zero bytes; a file of `swVersion` 14000 writes the u32 `1` before the twelve zero bytes. A `moWzdHoleSurfIdRep_c` identity hop has eight bytes of its own.

A sketch entity's name after its backed-up geometry can be preceded, instead of by six zero bytes, by u16 `0` and a `moSkIntersectInfo_c` (an intersection's record; 20 bytes). A `moEllipseBackedUpData_c` writes its start and end points (`moPointBackedUpData_c`), an f64, u32 `1`, and its centre and four axis points. A line record whose second endpoint index is `0xffff` and whose u32 is `-2` writes a u32 `0` more before its f64 `-1`, and the f64 of an arc record that is a full circle can hold a value other than `-1`. A spline record's u32 `1` can be `0` (with f64 `-1000`, `-1000` and `0` after it), and its end-handle list can be a `modifSplineList_c`. The u32 pair after the end handles can be `01 00 01 00` (a u16 `1` and u32 `1`) as well as u32 `1` or `0`. A spline can carry its geometry as a compressed Parasolid body in place of its weights: after the poles, u32 `0`, u32 `0`, u32 `2`, the parameter range as two f64, u32 `0` and u32 `1`. A compressed body follows after a zero run (24 or 56 bytes) and u16 `1`; the record's u16, u32 and u32 identifier follow the body. A spline of this form with no body ends after the u32 `1`, followed by those three fields. The last ellipse of a run can end after its u16 when a 16-byte list header follows, whose 12-byte part can open with `0x10000`. A relation entry's sixteen zero bytes can hold u16 `1` in their last two. A favourite handle's tail can be 30 bytes (zero bytes, f64 `-1`, i32 `-1`, zero bytes) before the next display object of a sheet-metal bend. A `moCompSilhEdge_c` (a silhouette edge) has the layout of `moCompEdge_c` followed by the `moFaceRef_c` objects of the faces it bounds, and `moSilhEdgeRef_c` has that of `moEdgeRef_c`. A `moCompSilhVertex_c` (a silhouette vertex) has the layout of `moCompSilhEdge_c`, and `moSilhVertexRef_c` that of `moEdgeRef_c`; a sketch's reference to it writes a little-endian u32 (`9`, `12`), two f64, a u32 (`1`, `2`), ten zero bytes, an identifier, twelve zero bytes, three i32 `-12345`, u32 `0`, the version, twelve zero bytes, i32 `-1`, u16 `0`, the backed-up point and a name. A sketch's external reference to a silhouette edge can also have no frame (some parts): after its u32 and two f64, zero bytes, an identifier, three u32, three i32 `-12345`, u32 `0`, the version, twelve zero bytes, i32 `-1`, ten zero bytes and the name. A sketch's external reference to a silhouette edge (`swVersion` 16000 and 19000) then writes a little-endian u32 (`9`, `10` or `12`), two f64 (π or `0`, then a tolerance), zero bytes (62), little-endian u32 `1`, a frame (a flag byte, twelve or three f64 and f64 `1`), a zero byte, and the tail of a `moCompSketchEntHandle_c` without its first u32. A `suObArray` is an MFC `CObArray`, written as a `suObList` is. A `moSketchBlockDef_c` block definition has the layout of a profile feature, followed after its `sgSketch` by the block's frame and plane trailer as in a `moCompRefPlane_c` (from the flag byte on); its sketch's records can include splines, whose record, after the poles, writes a u32 weight count (`0`), i32 `-1`, u32 `1`, u32 `1`, the pole count again, u32 `2`, two f64 (a parameter range), that many f64 (`1`), eight zero bytes, a u16 flag, an f64, a second f64 when the flag is `1`, and twelve zero bytes. A `moSketchBlockInst_c` (a block instance) is a feature whose block holds its `moSketchBlockDef_c` (which then ends with a 16-byte GUID, a u32 and eight zero bytes) and a `moDetailFolder_c`, and then, before the block's twelve zero bytes, the instance's placement data (288 bytes). After the block it writes u16 `0`, a 16-byte GUID, u16 `0`, u16 `1`, a `moAbsolutePoint_c` (three f64: the insertion point) and 160 bytes (the rotation angle, a scale `1`, u32 `1` and an identifier among them). A sketch's tail can hold, after its zero bytes, u32 `1`, an identifier and an `sgBlock` object holding the block's own geometry. The object writes u32 `0`, the instance identifier, the sketch-block instance record (marker prefix, f64 `1` at +48, eight zero bytes, nine f64 at +64 as the block's u, v and normal rows, the identity flags at +136, the origin at +144, the scale at +168, the mirror word at +176 and the centre, maximum and minimum box triples at +184, +208 and +232), one f64, u32 `0`, a count and that many 12-byte (block, entity, sketch entity) triples, u32 `1` and an `sgPointHandle` whose small entity index is its object index. Then comes an `sgEntHandleData_c` declaration: u32 `0`, a u32, 22-byte entries (a handle class token and 20 bytes of data) until a null token, a zero run, u32 `3` and either a compressed body or u32 `1`, seven bytes and u64 `100000`. An object written in place (a `moCompRefPlane_c`, a `moContextChainComponent_c` or a `moSketchBlockMgr_c`) follows, then a `moProfileFeature_c` named by the block's definition, which holds the block's `sgSketch`. One block of 720 triples does not follow the triple layout. A polygon or linear-pattern relation entry can have identifier `-1` (and, for a polygon, u32 `1` with the side count in the u16 pair; for a pattern, a null pointer in place of the count dimension); `ff ff` there is not a class declaration.

A profile feature can also reuse another profile's sketch: after its feature, u32 `0`, u16 `0`, a u32 (`3`), a back-reference to the source profile and a `moContourChooser_c` of the contours it takes, in place of the ten zero bytes and the `sgSketch`. A profile that refers to a sketch written in an earlier stream with two zero u32 after it also occurs at `swVersion` 19000. A revolve whose block head writes u32 `0`, u32 `1`, u32 `0` after its child (the fields that follow two bytes shorter than an extrusion's) writes zero bytes (64) in place of its `moPMarkRecord_c`. A `moPerLinePtRefPlnData_c` (a plane through a point normal to a line) has the face form of the plane data, then an end-point reference, u32 `0`, a point (three f64), u32 `0`, a line reference, u32 `0`, seven f64 and fifteen bytes; its references' handle tails write u16 `1` where others write u16 `0`. `sgSplineLengthDim` and `sgSplineTanMagnitudeDim` dimensions have the layout of `sgPntPntDist` (a point and a spline handle as operands), except a tangent-magnitude dimension whose operands are written as data (open item EV-17).

Files of `swVersion` 12000 differ in these places: a profile feature can refer to a sketch written in an earlier stream and then write u32 `1`, u32 `1` (or two zero u32), its `moSketchChain_c` objects and the sketch's tail from the marker on; a face reference has no version after its eight zero bytes but keeps the face's u32 `0`; the block of a `moCompSketchEntHandle_c` tail holds two i32 `-12345` and ends after 56 bytes with nothing after it, or with u16 `0` and the entity's name, or after 52 bytes (sixteen zero bytes after the version); a vertex projected into a sketch writes six zero bytes, a 58-byte block of that form (22 zero bytes after the version) and its name, with no backed-up point; an in-context group's support block is eight zero bytes, the identifier, twelve zero bytes, two i32 `-12345`, u32 `0`, the version and twelve zero bytes; a fillet writes ten zero bytes before its `suObList` objects, and a radius's edge is followed by a zero byte, the edge's u32 identifier and 64 zero bytes in place of 66 zero bytes; a display object writes 109 bytes in place of 261 before its `CString`; and a dimension handle's prefix is 99 bytes.

A feature pattern (`moLPattern_c`, `moCirPattern_c`, `moMirrorPattern_c`) writes the feature, its body head with *g* `1` and the extrusion's fields after it, and its `moPMarkRecord_c`. Then come a `suObList` of the seed features, an empty `suObList`, u32 `1`, u16 `0`, u16 `1` (`2` in some), three zero bytes, a `suObList`, a fixed block of 192 bytes (228 in a circular pattern; two groups of eight `ff` bytes, zero bytes and four `ff` bytes among them), a `suObList`, and eleven zero bytes (thirteen in a mirror pattern). A linear pattern then writes four display objects (`moDisplayDim_c` for each direction's count and `moDisplayDistanceDim_c` for its spacing), a u32, u16 `0`, the first direction's reference (a `moLineRef_w`) and its axis tail, the second direction's reference and tail or a null pointer, thirteen zero bytes, u32 `1`, u32 `1`, twelve zero bytes, two more display objects and nine zero bytes. An axis tail is the handle block of the edge form up to the version, twelve zero bytes, i32 `-1` and eight zero bytes (twenty zero bytes in files of `swVersion` 15000), seven f64 (a point, the direction and a value) and a zero byte. A mirror pattern then writes its plane, a `moCompRefPlane_c` whose handle block ends at its i32 `-1` and is followed by a box (four f64), a point and a normal (six f64), a flag byte, three or twelve f64, f64 `1` and 21 zero bytes . A circular pattern then writes u16 `0`, its axis (a `moLineRef_w`) and its axis tail, a byte (`0` or `1`), its display objects, the bytes `01 00 00 00 00 01 00 00 00` and seven zero bytes; in files of `swVersion` 17000 eight zero bytes and five f64 follow when no object does, or, as the last object of a stream, six f64 and four zero bytes.

A `moInkMarkupView_c` (in a `moInkMarkupFolder_c`) is a feature whose block holds its markup sketches; its block ends inside its name string, whose characters follow, then a flag byte, three or twelve f64 (the view's transform; twelve when the flag is `1`) and 101 bytes (f64 `1`, view fields, i32 `-1`, f64 `-1`, u32 `2002` among them, and a u32 identifier), and a `moDetailFolder_c`. A `moDetailFolder_c` is a folder whose block is followed by 104 bytes (zero but for an f64 `1`), a little-endian u32, the author as a Unicode `CString` and a u32 time stamp. A markup sketch's plane is a reference plane written in place with `moAbsoluteRefPlnData_c`, then u32 `0`, the frame (flag byte, three or twelve f64, f64 `1`) and a short trailer: the identifier's fields, u32, u32 `0`, u32 `1`; the sketch of a sheet-metal edge flange writes twelve zero bytes in place of the last three u32.

Files of `swVersion` 11000 differ further: a unit component has eight bytes after its sixteen `ff` bytes, plane data six bytes after its u32 `1`, a dimension handle's prefix is 91 bytes, a favourite handle with bit 7 writes twelve zero bytes after its last `CString`, a plane trailer can end after its u32 and two zero counts, and a `moCompSketchEntHandle_c` tail holds two i32 `-12345`, the version, eighteen zero bytes and the entity's name. The sketch settings of these files are open item EV-16.

A `moScale_c` feature writes the feature, its body head, units and `moPMarkRecord_c` as an extrusion does, then little-endian u32 `1`, three little-endian f64 scale factors (`25.4` each), u32 `0`, u16 `0`, u16 `1`, the scaled body (a `moCompSolidBody_c`, its face reference inside), a little-endian u32 count and that many identifiers, i32 `-1` and twelve zero bytes.

A `moSweepThread_c` thread feature (in files of `swVersion` 16000) writes a feature whose inputs are a reference plane, followed by its `moAbsoluteRefPlnData_c`, and the thread-profile `moProfileFeature_c`; the plane's feature-object block has eight zero bytes and little-endian u32 `2` where other blocks have twelve zero bytes. The body head has *g* `1`, and the extrusion's fields follow it (u32 count, identifiers, five u32, u16 *x*). Then come the `moPMarkRecord_c`; `CString`, i32 `-2`, `CString`, i32 `-2`, two `CString` values, 36 zero bytes and little-endian u32 `1`; the pitch as a little-endian f64; the size (`M6x1.0`) and the thread-profile library path as Unicode `CString` values; a `moCircleRef_w` to the edge the thread starts at; zero bytes, u32 `1`, u32 `0`, a u32 identifier, twelve zero bytes, three i32 `-12345`, u32 `0`, the version, twelve zero bytes and i32 `-1`; two f64, a zero byte, eight `ff` bytes and six zero bytes; a point and an axis (six f64) and a length (f64); u32 `1`, u32 `0`; an optional `moSingleFaceRef_w`, which is followed by ten zero bytes, a u32 identifier, u32 `0`, i32 `-1`, u32 `0`, the `-12345` block with the version, four f64, u32 `0` and two u32; then 40 bytes, u32 `1`, u32 `1` and 26 zero bytes (in files of `swVersion` 17000 the same 74 bytes hold eight zero bytes, u32 `1`, u32 `0`, u32 `1` and 54 zero bytes). A sequence of objects ends the feature: display dimensions (each with its `moFeatureDimHandle_c` and favourite), and `moCylinderRef_w` and `moSurfaceRef_w` references. Each reference is followed by a handle block of the edge form or the face form above, then its geometry: a cylinder's eleven f64 and u16 `0`, a surface's four f64, a little-endian u32 (`0` or `1`) and nine f64. The fields this paragraph leaves unnamed are open item EV-21.

The bytes of an extrusion after its end specification, the bounding-box forms of other lengths, and the ICE forms that write a body chooser by value are open item EV-17.


The table lists the classes whose bodies this specification describes, the stream that declares them, and whether they are named. *Either* means that a configuration declares the class in one of the two streams. *Varies* means that the class's objects are named in some bodies and not in others.

| Class | Declared in | Named | Body |
| --- | --- | --- | --- |
| `cptsSplineList_c` | either | no | §2 |
| `edgeRadiusObject_c` | either | no | §2 |
| `faceRadiusObject_c` | `ResolvedFeatures` | no | §2 |
| `Fillet_c` | either | yes | §2 |
| `mo3DProfileFeature_c` | `ResolvedFeatures` | yes | §2 |
| `moAbsolutePoint_c` | `ResolvedFeatures` | no | §2 |
| `moAngleParameter_c` | either | yes | §2 |
| `moAttribute_c` | `ResolvedFeatures` | yes | §2 |
| `moBaseBody_c` | `ResolvedFeatures` | yes | §2 |
| `moBBoxCenterData_c` | `ResolvedFeatures` | no | §8 |
| `moBoss_c` | `ResolvedFeatures` | yes | §2 |
| `moCirPattern_c` | `ResolvedFeatures` | yes | §2 |
| `moCirPatternSurfIdRep_c` | either | no | §2 |
| `moCombineBodies_c` | `ResolvedFeatures` | yes | §2 |
| `moCommentsFolder_c` | `ResolvedFeatures` | yes | §2 |
| `moCompEdge_c` | either | no | §2 |
| `moCompFace_c` | either | no | §2 |
| `moCompFeature_c` | `ResolvedFeatures` | no | §8 |
| `moCompProfile_c` | `ResolvedFeatures` | no | §2 |
| `moCompReferenceCurve_c` | `ResolvedFeatures` | no | §2 |
| `moCompRefAxis_c` | `ResolvedFeatures` | no | §2 |
| `moCompRefPoint_c` | `ResolvedFeatures` | no | §2 |
| `moCompRefPlane_c` | either | no | §2 |
| `moCompSheetmetalFeat_c` | `ResolvedFeatures` | no | §2 |
| `moCompSilhVertex_c` | `ResolvedFeatures` | no | §2 |
| `moCompSurfaceBody_c` | either | no | §2 |
| `moConfigCommentsFolder_c` | `ResolvedFeatures` | yes | §2 |
| `moConstraintCoincCurveRefplaneData_c` | `ResolvedFeatures` | no | §2 |
| `moConstraintCoincLineAtAnglePlaneRefplaneData_c` | `ResolvedFeatures` | no | §2 |
| `moConstraintCoincLineParallelPlaneRefplaneData_c` | `ResolvedFeatures` | no | §2 |
| `moConstraintCoincLinePerpPlaneRefplaneData_c` | `ResolvedFeatures` | no | §2 |
| `moConstraintCoincPlaneRefplaneData_c` | `ResolvedFeatures` | no | §2 |
| `moConstraintCoincTwoLinesRefplaneData_c` | `ResolvedFeatures` | no | §2 |
| `moConstraintMidPlaneRefplaneData_c` | `ResolvedFeatures` | no | §2 |
| `moConstraintPerpPlnTanOneCylinderRefplaneData_c` | `ResolvedFeatures` | no | §2 |
| `moConstraintRefplaneInterfaceData_c` | `ResolvedFeatures` | no | §2 |
| `moConstSurfRef_w` | `ResolvedFeatures` | no | §2 |
| `moCosmeticThread_c` | either | yes | §2 |
| `moCut_c` | `ResolvedFeatures` | yes | §2 |
| `moCutListFolder_c` | `ResolvedFeatures` | yes | §2 |
| `moCutListSortOptions_c` | `ResolvedFeatures` | no | §2 |
| `moCylinderRef_w` | either | no | §2 |
| `moDefaultRefPlnData_c` | `ResolvedFeatures` | no | §2, §8 |
| `moDeleteBody_c` | `ResolvedFeatures` | varies | §2 |
| `moDeleteBodyData_c` | `ResolvedFeatures` | no | §2 |
| `moDerivedCosmeticThread_c` | `ResolvedFeatures` | yes | §2 |
| `moDetailCabinet_c` | `ResolvedFeatures` | yes | §2 |
| `modifSplineList_c` | `ResolvedFeatures` | no | §2 |
| `moDimRefWrapper_c` | either | no | §2 |
| `moDirectionSpec_c` | `ResolvedFeatures` | no | §2 |
| `moDisplayDistanceDim_c` | either | no | §2 |
| `moDimText_c` | `ResolvedFeatures` | no | §2 |
| `moDisplayRevolveDim_c` | `ResolvedFeatures` | no | §2 |
| `moDocsFolder_c` | `ResolvedFeatures` | yes | §2 |
| `moEdgeDim_c` | `ResolvedFeatures` | no | §2 |
| `moEdgeRef_c` | either | no | §2 |
| `moEllipseBackedUpData_c` | `ResolvedFeatures` | no | §2 |
| `moEndPointRef_w` | `ResolvedFeatures` | no | §2 |
| `moEndSpec_c` | `ResolvedFeatures` | no | §2 |
| `moExtrusion_c` | `ResolvedFeatures` | yes | §2 |
| `moFaceCenterRefPointData_c` | `ResolvedFeatures` | no | §2 |
| `moFacePtRefPlnData_c` | `ResolvedFeatures` | no | §2 |
| `moFaceRef_c` | either | no | §2 |
| `moFaceRefPlnData_c` | `ResolvedFeatures` | no | §2 |
| `moFeatureDimHandle_c` | either | no | §2 |
| `moFixedRefPlnData_c` | `ResolvedFeatures` | no | §2 |
| `moGeneralCurveRef_w` | either | no | §2 |
| `moHeader_c` | either | no | §2.2 |
| `moFlatPattern_c` | `ResolvedFeatures` | yes | §2 |
| `moFoldUpDownFeatXformData_c` | `ResolvedFeatures` | no | §2 |
| `moFoldUpDownSketchXformData_c` | `ResolvedFeatures` | no | §2 |
| `moHelix_c` | `ResolvedFeatures` | yes | §2 |
| `moHoleWzd_c` | `ResolvedFeatures` | yes | §2 |
| `moICE_c` | `ResolvedFeatures` | yes | §2 |
| `moIntegerParameter_c` | either | yes | §2 |
| `moLengthParameter_c` | either | yes | §2 |
| `moLengthUserUnits_c` | `Config-N` | no | §8 |
| `moLineAxisData_c` | `ResolvedFeatures` | no | §2 |
| `moLineRef_w` | either | no | §2 |
| `moLogs_c` | either | no | §2.2 |
| `moLPattern_c` | `ResolvedFeatures` | yes | §2 |
| `moMateCoincident` | `ResolvedFeatures` | yes | §11.3 |
| `moMateConcentric` | `ResolvedFeatures` | yes | §11.3 |
| `moMatrixIndexHelper_c` | `ResolvedFeatures` | no | §2 |
| `moMirrorPattern_c` | `ResolvedFeatures` | varies | §2 |
| `moMirrorSolid_c` | `ResolvedFeatures` | yes | §2 |
| `modifSplineList_c` | `ResolvedFeatures` | no | §2 |
| `moMoldSrfBodyFolder_c` | `ResolvedFeatures` | yes | §2 |
| `moMoveCopyBody_c` | `ResolvedFeatures` | yes | §2 |
| `moMoveCopyBodyData_c` | `ResolvedFeatures` | no | §2 |
| `moMoveFace_c` | `ResolvedFeatures` | varies | §2 |
| `moNodeName_c` | `Config-N` | no | §2.2 |
| `moNumberDim_c` | either | no | §2 |
| `moOriginProfileFeature_c` | `ResolvedFeatures` | yes | §2 |
| `moNativeConfigurationTableFeature_c` | `ResolvedFeatures` | yes | §2 |
| `moPart_c` | `Config-N` | no | §2.2, §8 |
| `moPerBodySheetMetalDataNew_c` | `ResolvedFeatures` | no | §2 |
| `moPerLinePtRefPlnData_c` | `ResolvedFeatures` | no | §2 |
| `moPlanarSurface_c` | `ResolvedFeatures` | yes | §2 |
| `moPlanarSurfIdKeeper_c` | `ResolvedFeatures` | no | §2 |
| `moPlaneInterAxisData_c` | `ResolvedFeatures` | no | §2 |
| `moPlaneRef_w` | `ResolvedFeatures` | no | §2 |
| `moPLine_c` | `ResolvedFeatures` | yes | §2 |
| `moPLineProject_c` | `ResolvedFeatures` | no | §2 |
| `moPLineProjIdRep_c` | either | no | §2 |
| `moPLineSurfIdRep_c` | either | no | §2 |
| `moPointRef_w` | `ResolvedFeatures` | no | §2 |
| `moProfileFeature_c` | either | yes | §2 |
| `moRefAxis_c` | `ResolvedFeatures` | yes | §2 |
| `moRefPlane_c` | `ResolvedFeatures` | yes | §2 |
| `moRefPlaneAngleGeom_c` | `ResolvedFeatures` | no | §2 |
| `moRefPlaneCoincidentGeom_c` | `ResolvedFeatures` | no | §2 |
| `moRefPlaneMidPlaneGeom_c` | `ResolvedFeatures` | no | §2 |
| `moRefPlaneParallelGeom_c` | `ResolvedFeatures` | no | §2 |
| `moRefPlanePerpendicularGeom_c` | `ResolvedFeatures` | no | §2 |
| `moRefPlaneTangentGeom_c` | `ResolvedFeatures` | no | §2 |
| `moRefPoint_c` | `ResolvedFeatures` | yes | §2 |
| `moRefSurface_c` | `ResolvedFeatures` | yes | §2 |
| `moRelation_c` | either | no | §2 |
| `moRelDimension_c` | either | no | §2 |
| `moRelEquationSide_c` | either | no | §2 |
| `moRelMgr_c` | `Config-N` | no | §2, §2.2 |
| `moRevCut_c` | `ResolvedFeatures` | yes | §2 |
| `moRevolution_c` | either | yes | §2 |
| `moSheetMetal_c` | `ResolvedFeatures` | yes | §2 |
| `moSilhVertexRef_c` | `ResolvedFeatures` | no | §2 |
| `moSimpleEdgeIdRep_c` | `ResolvedFeatures` | no | §2 |
| `moSingleFaceRef_w` | either | no | §2, §5 |
| `moSketchBitmap_c` | `ResolvedFeatures` | varies | §2 |
| `moSketchBitmapData_c` | `ResolvedFeatures` | no | §2 |
| `moSketchBlockInst_c` | `ResolvedFeatures` | varies | §2 |
| `moSketchChain_c` | either | no | §2 |
| `moSketchRegion_c` | either | no | §2 |
| `moSkIntersectInfo_c` | `ResolvedFeatures` | no | §2 |
| `moSMBaseFlange_c` | `ResolvedFeatures` | yes | §2 |
| `moSolidBodyFolder_c` | `ResolvedFeatures` | yes | §2 |
| `moSolidRef_w` | `ResolvedFeatures` | no | §2 |
| `moStamp_c` | either | no | §2.2 |
| `moSurfaceAxisData_c` | `ResolvedFeatures` | no | §2 |
| `moSurfaceBodyFolder_c` | `ResolvedFeatures` | yes | §2 |
| `moSurfCut_c` | `ResolvedFeatures` | yes | §2 |
| `moSweep_c` | `ResolvedFeatures` | yes | §2 |
| `moSweepCut_c` | `ResolvedFeatures` | yes | §2 |
| `moSweepRefSurface_c` | `ResolvedFeatures` | yes | §2 |
| `moTempAxisRef_w` | either | no | §2 |
| `moTemplateSheetMetal_c` | `ResolvedFeatures` | yes | §2 |
| `moThicken_c` | `ResolvedFeatures` | yes | §2 |
| `moTransRefPlaneData_c` | `Config-N` | no | §8 |
| `moVertDim_c` | `ResolvedFeatures` | no | §2 |
| `moTemplateFlatPattern_c` | `ResolvedFeatures` | yes | §2 |
| `moVisualProperties_c` | `Config-N` | no | §8 |
| `moWireContact_c` | `ResolvedFeatures` | no | §2 |
| `moWireEdgeIdRep_c` | `ResolvedFeatures` | no | §2 |
| `moWzdHoleSurfIdRep_c` | `ResolvedFeatures` | no | §2 |
| `ParallelPlaneDistanceDim_c` | either | no | §2 |
| `sg3DPlaneHandle` | `ResolvedFeatures` | no | §2 |
| `sgAnglDim` | either | no | §2 |
| `sgArcHandle` | either | no | §2 |
| `sgCircleDim` | either | no | §2 |
| `sgDimEntityHelpData_c` | `ResolvedFeatures` | no | §2 |
| `sgEntHandle` | either | no | §2 |
| `sgLineHandle` | either | no | §2 |
| `sgLLDist` | either | no | §2 |
| `sgPGMLoopOffsetData_c` | `ResolvedFeatures` | no | §2 |
| `sgPGMLoopOffsetEdgeData_c` | `ResolvedFeatures` | no | §2 |
| `sgPGMProfileData_c` | `ResolvedFeatures` | no | §2 |
| `sgPntPntDist` | either | no | §2 |
| `sgPntPntHorDist` | either | no | §2 |
| `sgPntPntVertDist` | either | no | §2 |
| `sgPointHandle` | either | no | §2 |
| `sgSameCircleDim` | `ResolvedFeatures` | no | §2 |
| `sgSketch` | either | no | §2 |
| `sgSlot_c` | `ResolvedFeatures` | no | §2 |
| `sgSlotHandle` | `ResolvedFeatures` | no | §2 |
| `sgSplineLengthDim` | `ResolvedFeatures` | no | §2 |
| `sgSplineTanMagnitudeDim` | `ResolvedFeatures` | no | §2 |
| `sgTextHandle` | `ResolvedFeatures` | no | §2 |
| `su_CStringArray` | either | no | §2.2 |
| `suObList` | `Config-N` | no | §2, §2.2 |
| `suUIntArray` | `ResolvedFeatures` | no | §2 |
| `utCharFormat_c` | `ResolvedFeatures` | no | §2 |
| `VarFillet_c` | `ResolvedFeatures` | yes | §2 |

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

An attribute id is **not** globally unique. A **site** is one validated outer block (identified by its marker offset) or one named compound stream. Partition and deltas streams in the same outer block share a site namespace; streams in different outer blocks or compound streams are distinct sites.

An untyped surface or curve belongs to the retained outer block or compound stream that contains its site. Merging sites qualifies model entity identities but does not change this document-global source-record identity.

The Features manifest is the namespaced `swSolidWorks` XML document in
`swXmlContents/Features` or the compound-document `Features` stream. Element
names use their local names with the SolidWorks namespace URI
`http://www.solidworks.com/sw2003/schema`. Its
`swConfigurationList/swConfiguration` rows identify configurations by decimal
`swID`. Exactly one row with `swMostRecentConfiguration="YES"` identifies the
active configuration; its `swID` selects `Config-<swID>-Partition`. The row's
`swModelRef` points to the corresponding `swModel` `id`, and the linked
`swModel` supplies the configuration name. `swConfigurationId` on that model
has the same value as `swID`.

Selection precedence is a valid `SourceIndex=N` on the active
Keywords configuration, then the unique manifest row's `swID`, then a sole non-ghost
partition. A `Deltas`, `GhostPartition`, or `ResolvedFeatures` section is not
a partition substitute. A manifest with zero or several `YES` rows supplies no
index. Partition indices need not be dense. When the manifest selects a
source-less Keywords configuration by name, the selected `swID` establishes
its partition binding. With one `swModel`, `swConfigurationName` supplies the
same active name used by the first rule; with several model rows, the linked
manifest row is authoritative. If no index is established, multiple partition
sites do not establish active geometry identity.

Compact analytic records, `00 11` coedges, `00 12` vertex-uses, and `00 1d` points use `(site_id, attr)` identity because their attributes can repeat across sites. Bridges (`00 0e`), loop heads (`00 0f`), and edge-uses (`00 10`) carry globally unique attributes, but their references to site-scoped families remain in the referring record's site. Partition and deltas records in one site share an attribute namespace.

### 3.3 Feature-local bodies

`Config-<N>-FeatureBodies/LocalBodies` holds the bodies an Imported feature owns. It starts with a
`u32` LE count, which equals the stream count, i.e. one body per feature.

Per feature, an MFC CString name (`ff fe ff`, a length byte, UTF-16LE), followed by one of two
framings:

| form | after the name |
|---|---|
| solid Imported, SW 2012 and later | `u32 0`, `u32` body count, then per body `00×5`, `u32 L`, `u8 0` |
| pre-2012 CFB, and `Surface-Imported` | `00×5`, `u32 L`, `u8 0`; no body count |

In both forms `L` is the chained section's `chain_len + 5`, and the chained Parasolid section (§3)
follows; it carries one plain transmit stream.

A plain stream's description is `: TRANSMIT FILE created by modeller …`, naming neither
partition nor deltas.

A part whose only geometry is imported stores its bodies here, and leaves
`Contents/Config-<N>-Partition` an empty partition/deltas model.

**The bodies are independent.** Each stream reuses the same small attribute ids; each stream is its
own site, never a partition with its deltas. A partition that carries geometry
is the result of the features that follow the import, and its LocalBodies are then that
import's input, not the part.

**Tolerance slots.** A plain stream can store real values in the FACE, EDGE and vertex-use
tolerance slots (§4): faces `1e-8` or `0.0`, edges and vertices `2e-8` to `2.8e-4` m. It can
instead store the null-real sentinel, as every partition does.

A plain stream frames its records on the null-real sentinel, on `0.0`, or on a value in
`[1e-12, 1e-2]` m. A two-byte big-endian integer pair read as one f64 is a NORMAL value near
1e-302, below that range.

Partition and deltas streams frame records only on the sentinel.

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
| `00 0c` | XT BODY ownership node        |    variable | first/edit or tagged |
| `00 0d` | XT SHELL ownership node       |    variable | first/edit or tagged |
| `00 0e` | bridge / face-use→surface link |          37 | at body +8  |
| `00 0f` | loop head                      |         ≥14 | none        |
| `00 10` | edge-use                       |          30 | at body +8  |
| `00 11` | oriented coedge                |          21 | none        |
| `00 12` | vertex-use                     |          26 | at body +16 |
| `00 13` | XT REGION ownership node       |    variable | first/edit or tagged |
| `00 1d` | world point                    |          38 | none        |

Magic-bearing records use `c2 bc 92 8f 99 6e 00 00`.

The `00 0c`, `00 0d`, `00 0e`, and `00 13` tags are Parasolid XT node types
BODY, SHELL, FACE, and REGION. Their node pointers use one big-endian u16 for
values `0` through `0x7ffe`. A larger pointer uses two big-endian u16 values:
the first has its high bit set and carries the low fifteen bits, and the
second carries the remaining high bits. The decoded value is
`(first & 0x7fff) | (second << 15)`. Values `0` and `1` are null-like
sentinels where a field permits them.

- **Bridge `00 0e`:** `attr` is u16 BE at body +0, `sequence` is u32 BE at body +2, and the ATTRIBUTE-chain head (`attributes_groups`) is u16 BE at body +6. `sequence` is the active-partition face identity used by SWIFT `CadIdentifier` suffixes; it is distinct from `attr`, and sequence values are unique within one partition stream. `refs[2]` = owning loop-head, `refs[4]` = primary surface carrier (compact analytic or `00 7c`), `marker` = face orientation versus the surface natural normal (`0x2b` forward / `0x2d` reversed), which the carrier's own marker orients (§7.1). `refs[0]` and `refs[1]` are the next and previous FACE pointers; `refs[3]` is the SHELL pointer. The five references are either adjacent big-endian u16 cells followed by the marker at body +26 or `[hi][lo][01]` cells followed by the marker at body +31.
- **Loop head `00 0f`:** `refs[1]` = first coedge, `refs[2]` = owning bridge, `refs[3]` = next sibling loop head.
- **Edge-use `00 10`:** `sequence` is u32 BE at body +2. Bare `refs[0]` = canonical forward coedge (`0x2b`) when the reference is not a sentinel, `refs[3]` = support curve (compact analytic or `00 86`), or the null `1` on a tolerant edge, whose geometry its fins carry (§7.6). The complete tripled form has the same seven post-magic pointer fields, including its canonical-fin and owner pointers.
- **Coedge `00 11`:** `refs[1]` owning loop, `refs[2]`/`refs[3]` reciprocal FIN links (forward/backward), `refs[4]` forward vertex-use, `refs[5]` next fin around the edge, `refs[6]` edge-use, `refs[7]` the fin's own curve, which a tolerant edge's geometry is read from (§7.6), `marker` sense vs canonical (`0x2b` forward, `0x2d` reversed).
- **Vertex-use `00 12` / point `00 1d`:** `00 12.sequence` is u32 BE at body +2 and `00 12.refs[4]` = point attr; a bare `00 1d` record has four references at body +6, reference 0 is its ATTRIBUTE-group head (`0` or `1` when null), and it stores xyz as three f64 BE at body +14, in metres. Attrs `0` and `1` are sentinels, not world points. A `[00 1d][attr]` adjacency-table entry supplies no POINT node ID, four-pointer block, or coordinate vector and is not a point record. Two byte positions can frame a point of one `attr`, and the later reading is kept, except that a reading with a non-zero coordinate below 1e-30 m does not replace one without: it is a misaligned reading, which on three parts left a vertex 13.8 to 1 498 mm from its edges.

The complete short-index bodies below exclude the type tag and optional
first-of-type `ff` byte. A tripled pointer is a big-endian u16 followed by
`01`. The complete FACE includes five pointers after its sense, ordered as
next-on-surface, previous-on-surface, next-front, previous-front, and
front-shell. The complete EDGE includes seven post-tolerance pointers:
fin, previous, next, curve, next-on-curve, previous-on-curve, and owner.
The complete VERTEX has five pointers before tolerance and its owner pointer
after tolerance. A bare POINT stores its transmit index at body +0 and node ID at body +2.
POINT has exactly four pointers before its coordinate vector.
Coordinate bytes do not extend that pointer list. Coordinates are finite
big-endian f64 values in metres, with no format magnitude limit. A deltas
POINT in the tripled form has no bare-pointer fallback.
Complete POINT frames include the optional `ff`, transmit index, node ID,
four pointer cells, and all coordinate bytes. Equal coordinate values do not
establish equal POINT frames. A partition POINT supplies the current value
for its transmit index; companion deltas retain historical versions.

`face_bare` has a 37-byte body: `face_bare.attr at +0` (u16), `face_bare.node_id at +2` (u32), `face_bare.attributes_head at +6` (u16), `face_bare.tolerance at +8` (bytes[8]), `face_bare.refs at +16` (u16[5]), `face_bare.sense at +26` (u8), `face_bare.surface_front_refs at +27` (u16[5]).

`face_tripled` has a 48-byte body: `face_tripled.attr at +0` (u16), `face_tripled.node_id at +2` (u32), `face_tripled.attributes_head at +6` (bytes[3]), `face_tripled.tolerance at +9` (bytes[8]), `face_tripled.refs at +17` (bytes[15]), `face_tripled.sense at +32` (u8), `face_tripled.surface_front_refs at +33` (bytes[15]).

A complete LOOP stores its node ID before its four pointers. Complete LOOP
frames include the optional `ff`, transmit index, node ID, and all four pointer
cells. Equal owning-FACE and first-FIN pointers do not establish equal LOOP
frames. A partition LOOP supplies the current record for its transmit index;
companion deltas retain historical versions. A FIN has no node ID. Its nine pointers are attributes_groups, loop, forward, backward,
vertex, other, edge, curve, and next_at_vx, followed by its sense.
Complete FIN frames include the optional `ff`, transmit index, all nine pointer
cells, and sense. Equal loop, vertex, and edge pointers do not establish equal
FIN frames. A partition FIN supplies the current record for its transmit index;
companion deltas retain historical versions.

`loop_bare` has a 14-byte body: `loop_bare.attr at +0` (u16), `loop_bare.node_id at +2` (u32), `loop_bare.refs at +6` (u16[4]).

`loop_tripled` has an 18-byte body: `loop_tripled.attr at +0` (u16), `loop_tripled.node_id at +2` (u32), `loop_tripled.refs at +6` (bytes[12]).

`fin_bare` has a 21-byte body: `fin_bare.attr at +0` (u16), `fin_bare.refs at +2` (u16[9]), `fin_bare.sense at +20` (u8).

`fin_tripled` has a 30-byte body: `fin_tripled.attr at +0` (u16), `fin_tripled.refs at +2` (bytes[27]), `fin_tripled.sense at +29` (u8).

`edge_bare` has a 30-byte body: `edge_bare.attr at +0` (u16), `edge_bare.node_id at +2` (u32), `edge_bare.attributes_head at +6` (u16), `edge_bare.tolerance at +8` (bytes[8]), `edge_bare.refs at +16` (u16[7]).

`edge_tripled` has a 38-byte body: `edge_tripled.attr at +0` (u16), `edge_tripled.node_id at +2` (u32), `edge_tripled.attributes_head at +6` (bytes[3]), `edge_tripled.tolerance at +9` (bytes[8]), `edge_tripled.refs at +17` (bytes[21]).

`vertex_bare` has a 26-byte body: `vertex_bare.attr at +0` (u16), `vertex_bare.node_id at +2` (u32), `vertex_bare.refs at +6` (u16[5]), `vertex_bare.tolerance at +16` (bytes[8]), `vertex_bare.owner at +24` (u16).

`vertex_tripled` has a 32-byte body: `vertex_tripled.attr at +0` (u16), `vertex_tripled.node_id at +2` (u32), `vertex_tripled.refs at +6` (bytes[15]), `vertex_tripled.tolerance at +21` (bytes[8]), `vertex_tripled.owner at +29` (bytes[3]).

`world_point_tripled` has a 42-byte body: `world_point_tripled.attr at +0` (u16), `world_point_tripled.node_id at +2` (u32), `world_point_tripled.refs at +6` (bytes[12]), `world_point_tripled.xyz at +18` (f64[3]).

A support surface belongs to a face through `face -> bridge -> bridge.refs[4] -> carrier`. Face and carrier attribute equality does not establish ownership. The carrier reference uses the bridge's site.

The fixed body offsets above hold only while every pointer in the record is
below `0x7fff`. A bare compact record stores its `attr` and every reference in
the same variable-width pointer form as the XT ownership nodes, and so does a
compact analytic carrier (§7.1). The `sequence`, the tolerance and the marker
keep their widths, and each wide pointer moves the fields after it by two
bytes. A LocalBodies stream can hold a loop head that begins `00 0f f2 7e 00 01`
(attr 62078), FACE records whose own attr is in the 4-byte form, and
narrow-attr FACE records carrying a pointer past `0x7ffe`. The deltas forms of
§4.3 keep their fixed `[hi][lo][01]` cells.

A record key is the u16 `attr`, so a pointer past `0xffff` has no key:
a compact record that holds one does not frame, and a typed node whose own
`attr` is past `0xffff` is neither framed nor counted. A framed FACE record
that no decoded face carries is the loss `topology.face-records-unread`.

### 4.1 XT ownership nodes

The first node of an XT type carries its schema-reconciliation data between
the type tag and its fields. A matching base-schema type uses one `ff` byte.
An edited first-of-type schema header has three bytes: u16 BE node type at `+0`
and u8 current effective-field count at `+2`. The field count precedes the
`C`, `D`, `I`, and `A` field-edit instructions ending in `Z`. Effective fields
are transmittable or variable-length fields. The base schema-13006 effective
field counts are BODY 23, SHELL 9, REGION 7, FACE 14, and WORLD 11. WORLD
(type 101) uses the same complete schema-program grammar. The program extent
ends at `Z`. Pointer-type words and field names inside that extent are field
descriptors, not topology or data records. `C` consumes one base
field and produces one current field; `D` consumes one base field; `I` produces
one current field before the base fields are exhausted; `A` produces one
current field after the base fields are exhausted. `Z` ends a complete edit
program when all base fields are consumed and the stated current field count
is produced.

An inserted or appended field contains a byte-length-prefixed field name,
u16 BE pointer class, and XT positive-integer element count. A zero pointer
class adds a byte-length-prefixed field type. An element count of one adds a
logical transmission flag. These field descriptors are part of the schema
program. Their bytes do not start topology or data nodes. The node type that
starts the program determines the class of the following value payload.
Later nodes use the type tag followed by the fields. The first BODY's
fields follow the `Z` terminator of its first-of-type edit sequence; later
BODY nodes start with `00 0c`.

A BODY payload follows `attr u16 BE` and `node_id u32 BE` directly. The first
BODY of an edited type follows the schema edit sequence's `Z` terminator; a
tagged BODY starts after `00 0c` and its optional `ff` byte. There is no
length/index frame between the prefix and the fields.

```
header_refs        u16 references[ schema-defined count ]
resolution_size    f64 BE
resolution_linear  f64 BE
body_links         u16 references[3]
state              u8              ; 1
owner              u16 reference
body_type          u8              ; 1 solid, 2 wire, 3 sheet, 6 general
nominal_state      u8
ownership_fields   schema-defined pointer and index-map fields
```

The header reference count and the placement of the ownership fields come
from the embedded schema. The ownership field sequence contains the shell,
boundary-surface, boundary-curve, boundary-point, region, edge, and vertex
references plus the index-map fields. The region head is selected by a unique
FACE → SHELL → REGION → BODY closure; a fixed ordinal or numeric chain is not
an ownership selector. A BODY is valid only when exactly one schema field
interpretation satisfies the complete field grammar, the resolution values
are finite, and `body_type` is one of the four defined values.

A SHELL has eight references:

```
[attribute_chain, body, next_shell_in_region, back_face_head,
 edge, vertex, region, front_face_head]
```

The seventh reference is the owning REGION. A FACE has an attribute-chain
reference, an eight-byte tolerance field, five references, and a sense marker:

```
[next_face, previous_face, loop, shell, surface]  sense
```

The fourth FACE reference is the owning SHELL. The tolerance field is a finite
nonnegative big-endian f64 or the stream's eight-byte tolerance sentinel
`c2 bc 92 8f 99 6e 00 00`. A REGION has five references followed by one type
byte:

```
[attribute_chain, body, next_region, previous_region, shell_head]  type
```

The type byte is `S` for a solid region or `V` for a void region. Each typed
ownership relation is accepted only when FACE → SHELL → REGION → BODY closes,
the SHELL and REGION links agree, and each BODY region chain has reciprocal
next/previous links terminating in a null-like value. The stored BODY
`body_type` supplies the neutral body kind; attribute-list shape and
geometric connectivity do not.


### 4.2 Stored edge direction

```
bare canonical coedge = same-site coedge with attr == 00 10.refs[0]   (attr > 1; marker 0x2b)
prefixed canonical coedge = the unique same-site coedge with refs[6] == edge-use attr and marker 0x2b
edge.end_vertex   = canonical.vertex_use → 00 12 → 00 1d
edge.start_vertex = partner coedge (same edge_use_attr).vertex_use → …
```

The bare `00 10.refs[0]` coedge anchors the stored edge direction. A prefixed deltas edge-use has no serialized anchor; its direction is resolved from the unique same-edge forward coedge. Sentinel attributes `0` and `1` do not reference topology records. A non-sentinel bare reference must resolve to a same-edge forward coedge. If the explicit reference is invalid, or the prefixed same-edge forward coedge is absent or non-unique, the edge direction is unresolved.

A coedge runs from the vertex-use of the previous coedge of its loop to its own `refs[4]` vertex-use. Followed through `refs[2]`, the coedges of a loop run counter-clockwise about the face normal: the face is on the left of each coedge, seen from the side the face normal points to.

The two coedges of an edge with two uses have opposite markers. Coedge markers do not constrain face senses: each bridge marker gives the sense of its own face (§4, §7.1).

### 4.3 Deltas encodings

Deltas streams re-encode records in prefixed/tripled forms (each ref stored as a `[hi][lo][01]` triple) or as `[disc][attr]` adjacency tables; the magic occurs at the family-specific position within the record window.

| Tag                    | Deltas form | Magic    | Anchor                                                                            |
| ---------------------- | ----------- | -------- | --------------------------------------------------------------------------------- |
| `00 0e`                | tripled     | body +9  | ATTRIBUTE-chain head triple before magic; five ref triples then marker after                     |
| `00 0f`                | tripled     | none     | four ref triples at body +6                                                       |
| `00 10`                | prefixed    | body +9  | post-magic cell 2 = curve carrier; direction from unique same-edge forward coedge |
| `00 11`                | tripled     | none     | slot4 vuse, slot5 twin, slot6 edge-use                                            |
| `00 12`                | prefixed    | body +21 | refs-before-magic slot 4 = point attr                                             |
| `00 1d`                | prefixed    | none     | xyz after `[hi][lo][01]*` run                                                     |
| `00 1e/1f/20/32/33/35` | prefixed    | none     | f64 block after `2b`/`2d` marker                                                  |

Post-magic `00 10` reference cells appear as `[01][hi][lo]` or `[hi][lo][01]` triples. Partition and deltas streams in the same outer block share a site namespace. Prefixed and tripled references encode the same u16 attribute values as bare references.

The post-magic cell order has no discriminator byte. A valid `00 10` interpretation has a zero or a curve-carrier attribute in `refs[3]`; a valid `00 11` interpretation has a loop attribute in `refs[1]`, a same-loop coedge in `refs[3]`, a vertex-use attribute in `refs[4]`, and an edge-use attribute in `refs[6]`. The frame is the unique interpretation that satisfies these reference invariants. If more than one interpretation satisfies them, the record has no selected topology interpretation.

A deltas stream groups its records into change sets. Each change set carries a **change roster**:

```
00 01 00 01  ( attr u16 BE  class u16 BE )*  00 01
```

Roster entries name same-site nodes by attribute and node class, mixing topology, geometry-carrier, and entity classes. Roster membership records that a node belongs to the change set; it does not determine whether the node persists in the final state — a roster names retained, rewritten, and superseded nodes alike. A rostered node with no stored record in any same-site stream has no payload; references to it resolve to nothing.

A deltas change set can re-create subordinate topology and geometry records under new attributes. Partition records are the base set for one site. A deltas record with an identity already present in the partition does not replace the partition topology, point coordinate, or carrier record. A deltas record with an absent identity fills that missing subordinate record. The change roster carries no relation to a superseded partition face.

Partition topology is the base set for one site. Typed BODY, SHELL, REGION,
and FACE nodes are combined before body membership is validated. A deltas
node with an identity present in the partition does not replace the partition
node. When the partition bridge table is nonempty, a deltas bridge with an
absent identity is retained only when its FACE transmit index is
named by the validated typed FACE set. Without valid typed ownership, no
deltas bridge is added to a nonempty partition bridge table.

An empty partition bridge table adopts all deltas bridges when valid typed
ownership is absent. With valid typed ownership, it adopts only deltas bridges
selected by FACE transmit index. This adoption fills an empty
table; it does not replace a partition face. In every branch, missing loop,
edge-use, coedge, vertex-use, and point identities are filled from deltas.
Existing partition point coordinates remain unchanged.

A compact FACE bridge and a change-roster cell contain no old-face-to-new-face
relation. A different bridge identity does not select a partition face for
replacement. Sequence equality, attribute-chain changes, reference overlap, geometry,
record order, and roster adjacency do not establish partition-face replacement.

The FACE `attributes_groups` pointer names the head of its ATTRIBUTE chain.
An ATTRIBUTE names its owning FACE through its first pointer slot. The head
is metadata identity; it does not select a FACE, assign it to a BODY, or
identify another FACE use. Each FACE transmit index supplies a distinct face
identity. A LOOP belongs to the FACE named by its owning-FACE pointer.

## 5. Entity records and face families

Top-level entity families include `00 4f` KEY nodes, `00 50` ATTRIB_DEF nodes,
`00 51` ATTRIBUTE nodes, `00 52` INT_VALUES nodes, `00 53` REAL_VALUES
nodes, and `00 54` CHAR_VALUES nodes. A `00 51` header is
`flags u32 BE`, `attr u16 BE`, `seq u32 BE`, and `disc u16 BE`.

Every node id and pointer in these records — a bare ATTRIBUTE's `attr`, `disc`
and slots, a KEY, ATTRIB_DEF or INT_VALUES node id, and a REAL_VALUES `attr` —
takes the variable-width pointer form of §4: past `0x7ffe` it is 4 bytes, and
each wide field moves the fields after it by two bytes. The counts, `seq` and
the values keep their widths, so the u16 offsets in this section hold only
while every pointer is below `0x7fff`. A REAL_VALUES record can carry a 4-byte `attr` (`00 53 00 00 00 03 bc bf 00 01`,
attr 48 319), following an ATTRIBUTE whose last slots are wide. The
prefixed (deltas) ATTRIBUTE keeps its fixed header cells and `[01][hi][lo]`
slots.

The low byte `flo` of `flags` is the ATTRIBUTE value count. In a bare
ATTRIBUTE, the five pointer slots precede the `flo` value slots, so the
complete slot count is `5 + flo`. A bare ATTRIBUTE is framed only when
`1 <= flo <= 0x20`; `00 51` and `00 52` byte values inside its slots are
data. An optional `ff` byte occurs between the first `00 51` type tag and
the first-of-type fields when the base schema fields are unchanged. A
prefixed ATTRIBUTE carries `[01][hi][lo]` slots followed by a zero
terminator.

The header fields have these meanings:

- `attr` is the file-local transmit index.
- `seq` is the XT node id.
- `disc` is the file-local pointer to the ATTRIBUTE's `ATTRIB_DEF` node.
  It is not a class, layout, or body discriminator.
- The slots are
  `[owner, previous, next, next_of_type, previous_of_type, value...]`.
  The first slot names the owner node; the next two walk that owner's
  ATTRIBUTE list; the last two walk the list for one definition.

A stream declares named attribute families through KEY and ATTRIB_DEF nodes.
A `00 4f` KEY node carries a big-endian u32 byte length, a u16 transmit index,
and that many printable ASCII bytes. The ATTRIB_DEF `identifier` pointer
selects that KEY in the same stream. Adjacency does not bind a name to a
definition. The instance's `disc` resolves to the definition by transmit
index. Low-index system definitions can have no KEY name. Conflicting KEY
names or definition identities do not supply a named family.

A definition is named by its `identifier`; one whose `identifier` names no
framed KEY takes the KEY immediately before it. The KEY need not follow its
definition: `HK 0810-2RS` stores `DEF 15 (identifier 18)`, `DEF 17`,
`KEY 18 SDL/TYSA_COLOUR`, `DEF 19`, so the record after a KEY is the next
definition but one.

An unchanged base-schema ATTRIB_DEF with short references has a 38-byte body prefix.
The prefix follows the `00 50` tag and its optional first-of-type `ff` byte.
field_count is the number of trailing field-type bytes.
node_index identifies the definition.
next_definition links the definition chain.
identifier names the KEY containing the family name.
type_id is the native numeric attribute type.
actions contains eight event-action bytes.
field_names is the field-name reference.
legal_owners contains fourteen Boolean owner-class fields.
The complete record includes every declared field-type byte. Field codes
`1`, `2`, `3`, and `5` denote integer, real, character, and vector fields.
The family name does not determine the field types.

An ATTRIBUTE with short references has a 22-byte prefix. Offsets begin after
the `00 51` tag and its optional unchanged-schema `ff` marker. The value count is u32 BE at `+0`; the transmit index is u16 BE at `+4`; the sequence is u32 BE at `+6`; and the definition pointer is u16 BE at `+10`. Five u16 BE
references follow: owner at `+12`, previous at `+14`, next at `+16`,
next_of_type at `+18`, and previous_of_type at `+20`. The schema-ordered value
references start at `+22`. The supported short-reference form has one value
reference per definition field and a value count equal to that field count.
An extended reference does not use these short-reference offsets.

The `FACE_ID_2001` definition has field codes `[9, 9, 1, 1]`.
Its first integer field supplies the persistent face list. The
`LAST_BODY_MODIFYING_FEATURE_ID` definition has field codes `[9, 1]`.
Its integer field supplies the last modifying feature source identifier.
An owner-chain reference or pointer field does not supply either integer
payload. A different field schema does not select these consumers.

A `00 51` ATTRIBUTE instance whose `seq` is below `0x1_0000` (the zero u16 at
body +6 with narrow pointers) carries its definition node id in `disc` and its
owner attribute in its first slot; its remaining slots follow. A `00 52` INT_VALUES node carries a
big-endian u32 count, a u16 node id, and that many big-endian u32 values.
Names, definitions, lists, and instances must fit their node framing. A
definition or list identity with conflicting payloads is not selected. The
supported named attribute families include `FACE_ID_2001`,
`LAST_BODY_MODIFYING_FEATURE_ID`, and `SDL/TYSA_COLOUR`; their consumers use
the definition name and the owner relation. An `SDL/TYSA_COLOUR` instance
whose final value slot is null-like frames the inline `00 53` color record at
the exact node boundary. A non-null final value slot names the linked color
through the same owner-local reference roster. In a bare stream it also names
the `00 53` node of that id wherever the stream frames it: the color need not
follow its ATTRIBUTE, and a stream can place every color away from it, while
each color is named by exactly one ATTRIBUTE's value slot. A node id framed with two different RGB
values names no color. The prefixed (deltas) form carries versions of a node
and keeps the owner-local roster only. An unnamed system definition uses the
same structural forms when its color record is valid; no numeric definition
id selects that family.

The FACE `attributes_groups` pointer joins the FACE to its ATTRIBUTE-chain
head. Each ATTRIBUTE owner pointer joins that ATTRIBUTE to its owning FACE.
The ATTRIBUTE-chain head does not supply the FACE identity. Typed FACE
ownership remains the face-to-shell relation in §4.

A face-color record is an inline child of a framed `00 53` link record:

```
00 53
[ff]?
flags      u32 BE       ; low byte = 3
attr       u16 BE       ; color identity within the site
red        f64 BE       ; finite, 0.0 through 1.0
green      f64 BE       ; finite, 0.0 through 1.0
blue       f64 BE       ; finite, 0.0 through 1.0
```

The first color record begins at the exact end of the preceding `00 51`
bare slot frame or prefixed terminated slot frame, or anywhere a bare
ATTRIBUTE's value slot names it (above). With a narrow `attr` its length is 32 bytes
without `ff` and 33 bytes with `ff`. Its exact end can frame another
`00 53`; one link can therefore own a contiguous color-record run. A color
in a bare stream is on the node its ATTRIBUTE's owner slot names, and binds
only when that node is a framed FACE the decode emits and the current link
version is unique; an owner that is an edge or a body binds no face color. A deltas ATTRIBUTE absent
from the partition is superseded history, so a deltas color's owner slot is
not read: a face can have a partition attribute chain with
no color and a deltas version of that chain with one, and the partition can hold
ATTRIBUTEs newer than every deltas color. An ATTRIBUTE with no owner slot read
binds the face whose bridge owner field names it. A face can own several color
ATTRIBUTEs, because a partition and its deltas store different ATTRIBUTE nodes
for one face; the one with the greatest `seq` is current. Equal current colors on one face bind it
once. Two current colors that differ on one face (at equal `seq`), conflicting
current records for one face, or conflicting RGB values for one color
attribute, select no color binding. A framed FACE (a compact bridge or a typed
FACE record) that the decode does not emit, and whose current color is
therefore not transferred, is the loss `appearance.face-color-not-transferred`,
counted once per face.

A color's `attr` is local to its stream. A stream's identities are qualified
by its site; a color's identity is qualified the same way, so two streams'
colors with one `attr` stay distinct.

Partition and deltas ATTRIBUTE records share the site namespace. A current
record is the one with the greatest sequence; a deltas record wins an
equal-sequence tie. A deltas record with an identity absent from the
partition adds that identity.

`FACE_ID_2001` instances hang on face bridge records and reference one
integer list of four through seven values whose value 3 is zero. A four-word
form `[36, source, timestamp, 0]` identifies a feature-scoped set of current
FACE owners. It has no feature-local face selector and cannot select one
face when multiple owners carry it. Local-bearing forms have five through
seven values. Value 2 is the producer timestamp. Value 1 is
the native object id of the history feature that produced the face and value
4 is that producer's feature-local face identity. Values 5 through the end
are optional signed path fields; a field with bit pattern `0xffffffff` is
written as decimal `-1`. The complete persistent identity is value 1,
value 4, and this ordered optional tail. Equality includes the tail length
and every field in order. An empty tail differs from a tail containing zero.
A complete reference selects a face only when its complete identity has one
distinct current target.

A single-face component selection that supplies only the source and local
identity selects a face only when that pair has one distinct current target.
Document, configuration, and mirror-plane selections use this rule. Earlier
component-path entries do not supply an implicit persistent-identity tail.
A generated-hole surface reference instead names all current faces with its
source and local identity. Cylinder type and bore radius restrict that set;
coincident axes coalesce. A feature whose produced faces survive is a
producer of every body owning one of them.

`ATOM_ID_2001` has definition field codes `[9, 1]`. Its first integer field
carries one nonzero body identity scalar. The scalar and its complete current
BODY owner relation establish body identity; face identity uses `FACE_ID_2001`.

`LAST_BODY_MODIFYING_FEATURE_ID` instances hang on BODY attributes and
resolve one `00 52` integer-list payload containing one native feature source
identifier. The source identifier selects the Keywords modeling-feature
record with the same `id`. A history ordinal does not select this record.
The body attribute names `sldprt:brep:body#<attr>`. Zero, `0xffffffff`, a
missing or non-scalar value, and a conflicting or ambiguous source identity
leave the body-history relation unresolved. A resolved relation adds that
body to the named feature's output bodies without fabricating a body or
replacing an explicit body scope.

The VERTEX owner is a non-null `SHELL_OR_BODY` reference: BODY for a non-acorn vertex, SHELL for an acorn vertex. Its FIN reference may be null. EDGE ownership uses the same union: BODY for a non-wire edge and SHELL for a wire edge. An owner transmit index is admitted when exactly one typed ownership class claims it. A typed node candidate whose reference fields leave the u16 transmit-index space lies inside another record and claims no class. VERTEX tolerance is a double; the null-double sentinel (`-3.14158e13`) denotes an accurate vertex with tolerance half the modeller linear resolution.

An INT_VALUES node has a six-byte prefix: its element count is u32 BE at body +0
and its transmit index is u16 BE at body +4. Exactly that many u32 BE values
follow at body +6. The count determines the complete payload extent; values
that contain node-tag byte patterns remain integer data. The extent ends at
the end of the stream or at the zero byte that begins the next node's tag. A
`00 52` pair whose stated extent ends anywhere else lies inside another
record's fields and frames no INT_VALUES node.

## 6. Body records

Body membership is the typed XT ownership graph. The `00 0c` BODY node
supplies the body identity, the stored `body_type`, the shell head, and the
region-chain head. Each `00 0e` FACE node supplies its owning shell in
`refs[3]`. Each `00 0d` SHELL supplies its owning REGION in `refs[6]`, and
each `00 13` REGION supplies its owning BODY in `refs[1]`.

A face belongs to a body only when this closure is complete:

```
FACE.refs[3] → SHELL.refs[6] → REGION.refs[1] → BODY
```

The BODY region-chain head is the first REGION attribute or a null-like
predecessor sentinel. In the sentinel form, exactly one REGION has
`previous_region` equal to the stored head. Follow `next_region` and require
the following REGION's `previous_region` to equal the current REGION. The
chain ends at a null-like value and may not repeat an attribute. A SHELL's
region and body links must agree with the REGION closure. A shell with a
non-null-like body reference must name that same BODY; a null-like shell body
reference is valid for a void-region shell.

The BODY `body_type` byte is the body kind: `1` solid, `2` wire, `3` sheet,
and `6` general. The kind is not inferred from an ATTRIBUTE list, a chain
shape, a region disc, shell position, face count, or geometric connectivity.
A typed graph with a duplicate node identity, an unknown body type, an invalid
pointer, or an incomplete reciprocal closure selects no body relation for
the affected graph.

Partition and deltas streams share the typed node identity space for one
site. Partition nodes are authoritative for an identity present in both
streams; deltas nodes add identities absent from the partition. Typed graph
validation runs after the streams are combined, so a subordinate node can
complete a closure without replacing an existing partition node.

A cylindrical face spanning a complete angular period stores two loops. Each
loop contains one coedge whose edge is closed and whose carrier is a circle
coaxial with the cylinder. The stored topology omits the longitudinal seam.
Its endpoints are the two circle points
`center - ref_direction * radius`, where `ref_direction` is the cylinder
carrier's reference direction. The neutral topology joins these endpoints
with one line edge, inserts two oppositely oriented radial coedges for that
edge, and combines the two circular coedges and two seam coedges into one
loop.

## 7. Geometry carriers

All length fields are metres. Directions, normals, axes, reference directions, knots, and weights are dimensionless.

### 7.1 Compact analytic records

Stream-scope support-geometry carriers encoding untrimmed surface/curve placement. Generic layout:

```
00 TT  [ff]?  attr u16 BE  ordinal u32 BE  refs u16 BE[5]  marker u8 (0x2b|0x2d)  values f64 BE[n]
```

The partition form uses the five adjacent u16 references shown above; its marker is at `hdr + 16`, where `hdr` is the attr offset after `00 TT` and the optional `ff`. The attr and the references are pointers (§4): past `0x7ffe` each takes the 4-byte form, and the marker moves two bytes later for each one. A deltas compact analytic record uses five `[hi][lo][01]` reference triples instead:

```
00 TT [ff]? attr u16 BE ordinal u32 BE refs [hi][lo][01][5] marker u8 (0x2b|0x2d) values f64 BE[n]
```

Its marker is at `hdr + 21`, and the values begin one byte after the marker in both forms. The five triple terminators are `01`. All scalar payload values are finite; the format imposes no coordinate-magnitude cutoff. A carrier is selected only when exactly one of the two framings satisfies these byte invariants and the analytic geometry invariants below.

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

The line direction is unit length. Every axis, normal, and reference direction is unit length; each axis or normal is orthogonal to its paired reference direction. The cone fields satisfy `sin² + cos² = 1`. A torus major radius is nonzero and its minor radius is positive. A negative major radius reverses the native surface parameter frame; neutral geometry uses its absolute magnitude and reverses the face sense. The minor radius can equal or exceed the major-radius magnitude (a spindle torus).

Tags `00 43` and `00 44` use the same compact header. The orientation marker is followed by a nonzero profile-curve attribute in the pointer form (§4). Tag `00 43` then stores a unit translation direction as three f64 values. It defines the ruled surface `R(u,v) = C(u) + v·D`, where `C` is the selected profile curve and `D` is the stored direction. Tag `00 44` instead stores an axis point and unit axis direction as six f64 values, then the two points at which the profile meets the axis as six f64 values and the profile's parameter at each as two more, low parameter first, then a unit direction square to the axis as three f64 values and one more f64. Each meeting is stored or absent on its own: an absent one holds the absent-value sentinel `-3.14158e13` in its point and its parameter, a profile that does not meet the axis holds it in all eight, and a line or B-curve profile that meets the axis at one end stores that end only. The record defines the surface obtained by revolving `C` through `2π` about that axis; the two parameters, when both are stored, are the ends of the part of `C` between its meetings with the axis. The orientation marker is `0x2b` on a spun record and is not read. A circle or ellipse profile is an exact rational quadratic curve, whole or over its stored interval (below); a NURBS profile retains its degree, knots, poles, weights, and closure in the surface profile direction.

A `00 44` record whose profile is a circle about a point of the axis, in a plane containing the axis, and whose two stored parameters are half a turn apart with the first on the axis, is a sphere: its centre and radius are the circle's and its polar axis is the spin axis. Revolving the whole circle would cover that sphere twice, with the surface normal (profile tangent × direction of revolution, `A × (C - Z)`) pointing out of the sphere on one covering and into it on the other; the stored interval names the covering the record means. The sphere's zero azimuth is the direction of the profile at the middle of the interval, taken square to the axis, and the face sense is reversed against the sphere's outward normal when the normal there points into the sphere. A circle whose centre is more than 1e-9 of its radius off the axis is not this case, and neither is a record that does not store both parameters, or whose parameters differ from half a turn by more than 1e-9, or whose first parameter is more than 1e-9 rad off the axis; a circle that starts on the axis has the axis in its plane. Any other circle or ellipse profile with both parameters stored is revolved over that part of it only: the conic from the low parameter to the high one in its own direction, an exact rational quadratic arc cut into equal spans of at most a quarter turn. An ellipse about one of its own axes is then a spheroid covered once, with the surface normal on one side throughout; the whole ellipse revolved would cover it twice. An interval longer than one turn is not applied. For a NURBS profile the stored interval is read and not applied: the whole profile is revolved.

Surface support for an intersection curve or a spline-on-surface edge uses the same bridge-referenced carrier grammar. The bridge's `refs[4]` selects the ordinary compact analytic or B-spline surface carrier; neither relation introduces a separate surface-record family. The relation describes the edge curve and its pcurve, not a replacement surface carrier.

Compact records omit trim intervals, loop membership, and vertex points. Typed topology records contain those relations.

The marker of a compact line, circle, or ellipse gives the direction of a stored edge on that curve relative to the curve parameter. The stored edge direction is the direction of its canonical forward coedge. Marker `0x2b` means that the stored edge runs in the direction of increasing curve parameter. Marker `0x2d` means that the stored edge runs against it. The neutral edge runs in the direction of increasing curve parameter. When the marker is `0x2d`, the neutral edge reverses its canonical vertices and every incident coedge sense. A bounded-curve wrapper range (§7.2) sets the direction of its edge instead of the marker.

The orientation marker of a compact surface record (`00 32` through `00 36`) orients the surface normal against the natural normal `∂u × ∂v` of the neutral surface: `0x2b` keeps it and `0x2d` reverses it. The neutral face sense composes three reversals: a `0x2d` bridge marker (§4), a `0x2d` surface marker, and a negative torus major radius. Each one reverses the face sense once.

### 7.2 B-spline and list carriers

A bridge's `refs[4]` can point to a `00 7c` **surface-use wrapper** (a first-class carrier for list/NURBS surfaces) instead of a compact analytic surface. Ownership check: `00 7c.refs[1] == owning 00 0e bridge attr`. The wrapper's `child0` → `00 7e` **surface descriptor**. After the optional marker, the descriptor body is 42 bytes. Offsets below are relative to the descriptor attribute (`+0`): `+0` is the descriptor attribute (`u16`); `+2` is the `u_periodic` logical byte; `+3` is the `v_periodic` logical byte; `+4` and `+6` are `u_degree` and `v_degree` (`u16`); `+8` and `+12` are `u_pole_count` and `v_pole_count` (`u32`); `+16` and `+17` are the `u_knot_type` and `v_knot_type` bytes; `+18` and `+22` are `u_distinct_knot_count` and `v_distinct_knot_count` (`u32`); `+26` is the `rational` logical byte; `+27` is the `u_closed` logical byte and `+28` is the `v_closed` logical byte; `+29` is the `surface_form` byte; `+30` is `vertex_dim` (`u16`); and `+32..+41` are the five terminal u16 BE refs `[control_grid, u_mult, v_mult, u_knot, v_knot]`. Logical bytes are `0` or `1`. `vertex_dim` is `3` for non-rational control data and `4` for rational homogeneous control data, matching `rational`. `u_pole_count` and `v_pole_count` exceed their degrees. The referenced control array has `u_pole_count * v_pole_count * vertex_dim` values. Each multiplicity sum is `pole_count + degree + 1`, and each unique-knot array has its declared distinct count. A knot array can contain trailing physical entries after the descriptor's distinct-knot count when every matching trailing multiplicity is zero. Those entries contribute no knots, and their f64 bits are ignored. `child1` → `00 7d` **scale node** (for a curved surface, a diagonal parameter-space scale; both `1.0` = identity).

B-spline **array records** are reached by attr reference, not inline:

```
00 2d  marker [ff?]  value_count u32 BE  attr u16 BE  f64[value_count] BE   ; poles / homogeneous control grid
00 7f  marker [ff?]  count       u32 BE  attr u16 BE  u16[count]  BE        ; knot multiplicities
00 80  marker [ff?]  count       u32 BE  attr u16 BE  f64[count]  BE        ; unique knot values
00 count:u8 attr u16 BE values[count]                                      ; compact referenced array
```

The compact form has a nonzero count. The descriptor reference role selects its element width: control-grid and unique-knot references select f64 BE values, and multiplicity references select u16 BE values. A compact control array count equals `u_pole_count * v_pole_count * vertex_dim`. A compact knot and multiplicity pair has one common physical count at least as large as the declared distinct-knot count. Multiplicities inside the declared prefix are nonzero, and every trailing multiplicity is zero.

For rational arrays (`dimension == 4`) `00 2d` and compact control arrays store `[x*w, y*w, z*w, w]` per pole. Surface control grids use native index `u*n_v + v`. A trailing zero-multiplicity array entry is not part of the knot vector; its paired f64 slot is physically present but ignored.

Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier, whose body references a `00 88` **curve descriptor** (attr, degree, control_count, dimension, knot_count, subtype, flags, then control/multiplicity/knot array attrs). Adjacent `00 87`/`00 b8`/`00 a3` are 3D prolog/wrapper records, not 2D UV pcurves.

An edge's support curve can instead point to a `00 85` **bounded-curve wrapper**. After the compact header and orientation marker, the wrapper stores the source curve attr, start xyz, end xyz, and the source parameter interval as eight f64 BE values. The optional `ff` byte after the tag shifts the compact header by one byte. The stored endpoint coordinates are the referenced source curve at the two interval parameters to within the linear resolution, `1e-8` m, and a wrapper whose two ends each lie within that distance of the source retains the source curve's geometry and bounds its use. A wrapper whose source is a surface-intersection carrier follows the rule in §7.3 instead.

A bounded wrapper whose endpoint evaluations match its vertices has its two source parameters as the edge's parameter range. A line source stores signed distance parameters in metres along its unit direction. The neutral line uses millimetres and multiplies these parameters by 1000. Other analytic parameters and NURBS parameters retain their stored values, except that a circle or ellipse interval may be stored on any equivalent angular branch, such as `[-1.47, 0]` for an arc ending at the seam from below; the neutral interval moves by whole turns to start within `[0, 2π)`. The wrapper interval follows source-curve orientation. The neutral interval is ascending. An end matches a vertex when it lies within that vertex's bound: the linear resolution, or the vertex's stored tolerance when it is a tolerant vertex (§7.6) and stores more. When the endpoint evaluations match the edge's ordered vertices in the opposite order, the neutral edge reverses its canonical vertices and every incident coedge sense; an edge shorter than its vertices' bounds matches both ways and takes the order whose worse end is nearer, and a closed edge, whose one vertex matches both ways equally, keeps its stored order. If neither order matches, the neutral edge has no parameter range.

An edge whose support curve is a compact circle or ellipse record without a bounded wrapper obtains its range from its vertex positions and the record's orientation marker. `θ(p)` is the carrier angle of position `p`: the angle from the reference direction toward `axis × reference`, after the two in-plane coordinates are divided by the major and minor radii. The marker is the sense of the curve against its edge: traversed from its stored start vertex to its stored end vertex, the edge runs in the positive carrier direction for marker `0x2b` and in the negative carrier direction for marker `0x2d`. Marker `0x2b` gives the range `[θ(start), θ(start) + ((θ(end) − θ(start)) mod 2π)]`, and the neutral edge keeps the stored vertex order. Marker `0x2d` gives the range `[θ(end), θ(end) + ((θ(start) − θ(end)) mod 2π)]`, and the neutral edge reverses its canonical vertices and every incident coedge sense. A closed edge spans `[θ(v), θ(v) + 2π]` from its vertex `v`; for a closed circle with sentinel vertex uses, `v` is its derived vertex, which is the seam endpoint on a complete cylindrical face (§5). The marker selects the direction of a closed edge by the same rule. Every closed circle or ellipse edge whose range spans one turn starts it at `θ(v)`, also after a seam moves its derived vertex, and a range moved this way is derived. The range is unresolved when a vertex lies further from the carrier than its bound (the linear resolution, or its stored tolerance when it is a tolerant vertex and stores more), when the two vertices of an open edge coincide, or when compact records with one attribute and identical geometry carry different markers.

A closed edge with sentinel vertex uses whose support curve is not a compact conic derives its vertex from the carrier's own closure point: the position at which the curve begins and ends, which the carrier states exactly. For a surface-intersection carrier (§7.3) that position is the ring terminator the composite names in both endpoint slots, so the derived vertex is exact and needs no inversion. The edge spans the carrier's whole parameter domain. The derived vertex is unresolved when the carrier's first and last positions differ, and a near-closing carrier is left unresolved rather than closed onto either end.

An unwrapped NURBS edge obtains its range from its two vertex positions only when each position has exactly one parameter in the complete nonzero-knot-span domain within numerical roundoff. Multiple parameter values for either endpoint leave the range unresolved. One approach of the carrier to a vertex is one parameter: points of the carrier that are all within roundoff of the vertex, with the carrier between them also within it, are the same parameter. The carrier can extend past both vertices. The neutral interval is ascending; when the start vertex's parameter is greater than the end vertex's, the neutral edge reverses its canonical vertices and every incident coedge sense. This rule is for an exact NURBS carrier; an edge carried by a surface-intersection chart, which declares its own accuracy, takes its range by the rule in §7.3 instead.

A sketch entity projected from an edge spans the edge carrier between the edge vertices. A NURBS entity is the carrier clamped to the edge parameter interval. The interval is the stored edge range. Without a stored range, the interval runs from the start vertex parameter to the end vertex parameter, each found on the carrier within the largest of the edge tolerance, the vertex tolerances, and `1e-2` mm. The first and last control points of a clamped entity are the projected edge vertices. An edge with two distinct vertices and no interval that the carrier can represent projects as the chord between the vertices. An edge with one vertex, or on a periodic carrier, keeps the complete carrier when it has no such interval. An end of a non-periodic complete carrier within `1e-2` mm of its projected vertex moves to that vertex. When the start vertex parameter is greater than the end vertex parameter, the entity endpoint references swap and the profile use reverses. An arc starts at the projected start vertex and sweeps by the edge parameter interval. A bounded ellipse takes the edge parameter interval as its bounds. The sweep and the bounds change sign when the carrier frame projects onto the sketch plane with opposite handedness.

The `00 2d`, `00 7f`, and `00 80` arrays carry control nets and knot data. A model-space B-curve's descriptor has `dimension` 3, or 4 with homogeneous poles. The one two-dimensional control net is an SP_CURVE's `b_curve` (§7.6), whose descriptor has `dimension` 2, or 3 with the `rational` logical set; `dimension` alone does not tell a rational parameter-space curve from a polynomial model-space one.

Planar pcurves are the exact inverse of the edge carrier in the support plane frame. Lines remain lines. Coplanar circles and ellipses remain analytic circles and ellipses with the same angular parameter; an edge axis opposite the plane normal reverses the parameter-plane rotation.

An elliptical edge on a cylindrical face has a polar-harmonic pcurve. Its radial-plane coefficients determine cylinder azimuth with `atan2`; its axial coefficients preserve the ellipse carrier's angular parameter. The radial harmonic has constant magnitude equal to the cylinder radius.

A coaxial circle on a circular cylinder or cone is a constant-axial-coordinate pcurve. A coaxial circle on a torus is a constant-minor-angle pcurve. A coaxial circle on a sphere is a constant-latitude pcurve, its latitude the arcsine of the circle centre's axial offset divided by the sphere radius. The azimuth origin is the circle reference direction expressed in the surface frame; the azimuth parameter direction is positive when the circle and surface axes agree and negative when they oppose.

A spherical pole-closing edge collapses to a pole. For a face of one loop of three circle coedges it is added at `center + radius·axis` or `center - radius·axis`, whichever is an existing boundary vertex of the patch within the linear resolution, 1e-5 mm, the first of the two when both are; the seam does not add a point or vertex, a patch with no vertex on a pole has no pole-closing edge, and neither has one whose pole vertex is not the end of exactly one coedge of the loop. Its spatial carrier is degenerate at the pole, and it follows, in the loop, the coedge that arrives at the pole. Its pcurve is the line `v = π/2` or `v = -π/2` over the parameter interval `[0, 1]`, from the azimuth at which the arriving coedge's pcurve ends to the azimuth at which the leaving coedge's pcurve begins or a whole turn from it, in the direction the loop runs: a loop has its face on its left, so on a forward face it runs towards lesser azimuth at `v = π/2` and towards greater at `v = -π/2`, and on a reversed face the other way. A great-circle pcurve is a line on its circle's own parameter `t`. Its phase moves by whole turns so that its latitude at the middle of the edge's range lies within half a turn of the equator. When that latitude lies within the poles, the pcurve is `v = phase + t` at the circle's longitude. Beyond a pole it is `v = ±π - phase - t` at the opposite longitude, because the point `(u, v)` is the point `(u + π, π - v)` of the sphere's chart, so its latitude stays within the poles over the range. A pole end has the azimuth of its line, and the pole is the one the sign of its latitude names. Every parameter value maps to the same pole vertex. A pole-closing edge whose arriving or leaving coedge has no pcurve, or whose two azimuths are equal, has none. A face of another shape with a corner on a pole gets no pole-closing edge. A stored fourth edge with no curve in a loop of three circles is a pole-closing edge whichever pole its vertex is stored at, and is placed at `center + radius·axis`.

A NURBS edge is isoparametric when it equals the exact surface isocurve at either endpoint of one active parameter domain. The edge and isocurve have the same degree, complete knot vector, periodic state, control points, and rational weight vector. The pcurve fixes that endpoint parameter and varies in the other surface parameter.

A NURBS surface axis that has degree one, two poles, a clamped knot vector, and equal weights at corresponding endpoint poles is affine. A NURBS edge with the other axis's degree, knot vector, periodic state, and weight vector is an interior isocurve when one common interpolation factor reproduces all its control points from the corresponding endpoint-pole pairs. That factor uniquely fixes the affine surface parameter. A degenerate affine axis or matches in both parameter axes leave the pcurve unresolved.

A quadratic rational NURBS edge on a cylinder has a polar-NURBS pcurve when every Bézier span satisfies the homogeneous polynomial identity `X² + Y² = R²W²` in the cylinder radial frame. Its axial control channel is the projection of the same spatial poles onto the cylinder axis. The pcurve shares the edge curve's degree, knots, weights, and parameter; its stored range is the interval whose evaluated endpoints coincide with the edge vertices.

A NURBS surface is ruled in one parameter when that parameter axis has degree one, two poles, a clamped knot vector, and equal weights at corresponding endpoint poles. A spatial line coincident with a ruling at a fixed value of the other parameter has an affine pcurve. The fixed parameter is the common endpoint-isocurve parameter. The varying parameter is the spatial line parameter projected onto the evaluated ruling vector.

The fixed parameter is selected only when the ruling inverse has one parameter candidate across the complete active knot domain. Both parameter axes are tested. Exact boundary matches are candidates alongside interior rulings. Multiple parameters or matches in both axes leave the pcurve unresolved; residual magnitude does not select one candidate.

An extended isoparametric NURBS edge carries the isocurve of the underlying surface before the face domain was re-clamped. Its curve domain contains the face's active domain in the running axis. The decoder inserts each running-axis face-domain endpoint into the curve's homogeneous knot representation until multiplicity `degree + 1`, extracts the middle segment, and accepts it only when its degree, knot vector, poles, and weights equal the exact surface isocurve at one fixed parameter. Fixed-parameter candidates are both active-domain boundaries and the unique interior value obtained by seeded inversion of an interior edge point. Both fixed axes are tested; exactly one axis and value is required. The resulting pcurve is the fixed-coordinate line with the edge's vertex parameter range.

A degree-one, non-rational NURBS edge with a list-curve knot vector is a boundary cache when its edge-range endpoints invert to the bound surface. Each pole is inverted with the preceding UV as the branch seed. The pcurve keeps the carrier degree, knots, and parameter, and its fit tolerance is the maximum distance between the surface-evaluated pcurve and the stored edge curve over every nonzero knot span. This family is accepted at its carrier error; no stricter fixed exactness floor or subdivision applies. An edge that declares its own accuracy is the exception: a chart edge (§7.3) takes the cache only when that error is within its `chordal_error`.

Before either construction, the edge curve is evaluated over its vertex parameter range and its endpoints are tested against the bound surface. A range whose endpoints are off the surface is an upstream face, loop, or surface-association defect: the pcurve is withheld and the topology defect is reported. A full curve-domain endpoint test is not used for an extended isocurve because its carrier intentionally extends beyond the face domain.

### 7.3 Surface-intersection curve carriers

An edge's `00 10.refs[3]` can point to either intersection carrier for a curve defined by the intersection of two support surfaces. The compact composite has this shape:

```
00 26  attr u16 BE  ordinal u32 BE  refs u16 BE[5]  marker u8 (0x2b|0x2d)
       payload u16 BE[6] = [support0, support1, chart, term_start, term_end, uv]
```

The intersection-data form replaces the two-byte composite tag with `00 01 5a`; the attribute, ordinal, five header references, marker, and six payload references have the same layout and semantics.

`support0` and `support1` reference the two intersected surface records. The remaining payload references resolve to three witness records:

- **`00 28` chart** — the solved point cache: `count u32 BE, attr u16 BE, base_parameter f64 BE, base_scale f64 BE, chart_count u32 BE, chordal_error f64 BE`, one further f64, then two absent-value sentinels `-3.14158e13` at body +36 and +44, then `count` point entries at body +52. An entry is either 88 bytes (point xyz, then a finite nonzero tangent at entry +56) or a bare 24-byte point. The chart has no stride discriminator. `chart_count == count`; `base_scale` is nonzero; `chordal_error` is positive. Chart points lie exactly on both support surfaces. The parameter at point `k+1` is the parameter at point `k` plus the chord length times `base_scale`, starting from `base_parameter`. The referenced terminators select the unique entry width with the least total endpoint displacement. Equal displacement leaves the chart unresolved. The optional support-UV record does not select the chart width.
- **`00 29` terminator** — an exact curve endpoint: `count u32 BE (1|2), attr u16 BE`, a kind label, then the endpoint xyz as f64 BE. The label is one kind character `L` (limit), `H` (ring), or `T` (terminator), optionally followed by a second character `?`, `F`, or `S`. A ring composite names one `H` terminator in both endpoint slots and its chart closes onto itself. The terminator, not the approximate chart endpoint, is the exact curve end.
- **`00 cc` support-UV values** — `count u32 BE, attr u16 BE, width u8 (2|3|4)`, then `count` f64 BE values, `width` per chart point. A width-4 row is `[support0.u, support0.v, support1.u, support1.v]` and is co-parameterized with the corresponding chart point. The value count is `width × n` for `n` chart points, or `width × (n + 1)` when the curve crosses a periodic seam of a support surface and the extra row restates the endpoint in the wrapped parameterization. Plane `u` and `v` values and cylinder or cone axial `v` values are lengths in metres. Angular and NURBS-knot parameters are dimensionless. A composite can reference a UV slot with no stored `00 cc` record; the chart and terminators alone define the solved curve.

Terminator and support-UV bodies also occur inline after their field labels: `term_use` followed by `00 00 00 01 01 63 43 5a`, and `values` followed by `00 00 00 02 01 66 01`, each directly preceding the same body layout.

The chart is a solved cache: the exact curve is the surface–surface intersection, and the stored chart polyline reproduces it to within `chordal_error`. The referenced terminators replace the approximate chart endpoints and are the exact ends of the intersection curve. They are an edge's endpoints when they reproduce its vertex positions; a composite whose terminators lie past an edge's vertices carries more of the intersection than that edge, and the edge is a sub-arc of the terminated curve. An absent or structurally inconsistent optional support-UV record does not invalidate the model-space curve. A unique complete width-4 UV cache with exactly `n` rows forms two degree-one NURBS pcurves over the chart's chord-length parameter knots. Length-valued surface parameters convert to millimetres. When that UV cache is unavailable, the inverse of each chart point on an analytic or positive-weight NURBS support forms the pcurve. Each periodic analytic parameter and each NURBS inverse selects the branch nearest to its preceding value. The exact inverse of each terminator replaces its endpoint UV on the same branch. A pcurve attaches only when all surface-evaluated controls reproduce the endpoint-corrected chart within `chordal_error`, its two surface-evaluated endpoints reproduce the edge vertices, and the face surface selects exactly one of the two support slots. For an analytic support, a second-derivative bound keeps every mapped segment within `chordal_error` of its model-space chord. For a NURBS support, each parameter segment splits at every surface knot; its positive-weight rational Bézier residual control hull bounds the complete mapped segment against the model-space chord within `chordal_error`. The pcurve fit tolerance is `chordal_error` converted to millimetres.

An edge's support curve can instead be a `00 85` bounded-curve wrapper (§7.2) whose source is an intersection carrier. The wrapper's two stored parameters are in the chart's parameter and its two stored points are exact points of the intersection. A stored point between two chart points lies off their chord by up to `chordal_error`, and its stored parameter is near that of its foot on the chord but is not that parameter. A stored point that is one of the chart's ends does not always carry that end's chord-length parameter either, so a stored parameter can lie outside the chart's domain. That excess is not bounded by `chordal_error`, and no field bounds it. A wrapper is admitted when each stored point lies within `chordal_error` of the chart at its stored parameter, that parameter held to the chart's domain. The source is the chart the wrapper names. The neutral curve is then the chart between the two parameters: a degree-one curve whose first and last poles are the stored points at the stored parameters, and whose other poles are the chart points with a parameter strictly inside both the stored interval and the chart's domain. A chart point within the linear resolution of a stored point is that end, and not a pole beside it: a stored end that is a chart point carries a parameter that differs from the chart's in the eleventh place. It is a solved cache like the chart, on the chart's two supports, and its fit tolerance and the edge's tolerance are `chordal_error`. The stored interval is the edge's parameter range by the rule of §7.2. The chart's support-UV rows belong to the chart's own points, so the wrapper's pcurves are the inverse construction below. A wrapper with a zero-width interval, or with no part of its interval inside the chart's domain, bounds no curve. Neither does a ring wrapper, whose interval runs once round a closed chart from an interior parameter, unless that parameter is within `chordal_error` of the seam: its high end, held to the domain, is the chart's seam and not the stored point. An edge whose vertex lies outside its bound of the stored end keeps this curve and has no range, and its curve then ends off that vertex. A wrapper is read only where no other carrier has its attribute.

An edge whose support curve is an intersection carrier itself, with no wrapper, obtains its range by inverting each of its two vertex positions onto the chart. The inversion admits a departure of `chordal_error`, not numerical roundoff, because the chart reproduces the exact curve only to that bound and a vertex on the exact curve need not lie within roundoff of the chart. Each vertex selects the interval of consecutive chart segments that passes within `chordal_error` of it, and the parameter of that interval's closest approach is the vertex's parameter. Two separated such intervals, or none, leave the range unresolved, as do two equal parameters; a closed chart's first and last segments are one interval. The neutral edge keeps its stored vertex order when the start vertex's parameter is the lower of the two, and reverses its canonical vertices and every incident coedge sense otherwise. The edge's tolerance is `chordal_error` converted to millimetres.

### 7.4 Constant-radius rolling-ball surfaces

A `00 38` surface carrier defines a circular rolling-ball blend between two support surfaces:

```
00 38 [ff]? attr u16 BE  ordinal u32 BE  refs u16 BE[5]
             marker u8 (0x2b|0x2d)
             selector u8 (45|52)  support0 u16 BE  support1 u16 BE  spine u16 BE
             offset0 f64 BE  offset1 f64 BE  side0 f64 BE  side1 f64 BE
```

`support0` and `support1` each name either a support-surface carrier directly or a support-pair record. **The record present at that reference determines the form, not `selector`.** A reference is a support pair when a support-pair record carries that attribute and a direct support surface when a surface carrier does; neither leaves the blend unresolved. The two forms are disjoint, because a support-pair record is itself a `00 38` record and a surface carrier is not. `selector` distinguishes the two serialized layouts and does not constrain either reference; the field offsets of `support0`, `support1`, and `spine` are the same for both values.

A support-pair record has zero `offset0` and `offset1`; its first two references name candidate surfaces and its third reference names their intersection curve. The blend's support on that side is the pair member the spine lies at the rolling-ball radius from. A member is taken when, at every exact spine station, it lies within a quarter of the radius of that offset and the other member lies beyond half of it; only where no member qualifies does the adjacent member apply, keeping the record's stated side.

For a blend record, `spine` names the center/spine curve that selects the blend branch. The spine is the locus of rolling-ball centers: it lies at the common offset magnitude from each support, on the side each support's resolved sense selects, and it is therefore not a curve on either support and not their intersection curve. `abs(offset0) == abs(offset1) > 0`; their common magnitude is the constant rolling-ball radius. A sign difference between the offsets reverses the second support relative to the first. Each `side` value is exactly `+1` or `-1`; a negative value independently reverses that support's side. Lengths are metres.

The side a direct support's offset and `side` value select is stated against that carrier's ORIENTED normal (§7.1), not the neutral surface's natural normal: a `0x2d` compact support carrier reverses the stated side once more, exactly as it reverses a face's sense. A `0x2b` carrier under a `0x2d` bridge keeps the stated side, so the carrier's marker decides. For a support-pair reference, the stated side is the PAIR's, and no field read here maps it onto the chosen member: a pair member chosen from the spine takes its side from the spine as well, on whichever side of the member the spine lies.

The record stores no surface. The blend surface is the envelope of the ball along the spine: at each spine station, the circular arc of the ball radius about the centre between the two contact points, the closest points of the two supports to the centre. Its natural normal points away from the ball centre, and a face's sense is stated against that normal. A spine that is a chart (§7.3) of the centre locus lies near the locus, not on it. The exact centre at a chart point is the point of the chart's normal plane at the ball radius from both supports. Where the two contacts meet, the section is one point.

A variable-radius blend result uses a `00 7c` B-spline surface-use wrapper and the solved NURBS carrier defined in §7.2. The B-rep stores the result surface, not a second procedural blend record or its radius law. The feature-history record carries the variable-radius design intent.

### 7.5 Offset surfaces

A `00 3c` surface carrier defines an exact signed normal offset from one support surface:

```
00 3c [ff]? attr u16 BE  ordinal u32 BE  refs u16 BE[5]
             marker u8 (0x2b|0x2d)
             discriminator u8 ('V'|'I'|'U')  true_offset u8 (0|1)
             support u16 BE  distance f64 BE
```

The compact partition record is 31 bytes from the `00 3c` tag through `distance`. The deltas form stores each common-header reference and `support` as `[hi][lo][01]`. A deltas status frame can continue after `distance`; that continuation is not part of the surface carrier.

`support` is a non-null reference to an analytic, B-spline, or typed procedural surface carrier. `distance` is a finite signed length in metres and has no magnitude limit. `discriminator` and `true_offset` classify the serialized construction and do not change its point equation. For support point `S(u,v)` with partial derivatives `S_u` and `S_v`, the offset surface is

```
O(u,v) = S(u,v) + distance * unit(S_u(u,v) × S_v(u,v)).
```

The offset surface retains the support parameterization. Nested offset carriers resolve recursively. A cyclic support-reference graph is invalid.

### 7.6 Tolerant edges and surface-parameter curves

An edge-use whose support-curve slot holds the null `1` is a tolerant edge. Its tolerance slot holds a value rather than the null-real sentinel, and its geometry is carried by its fins: each `00 11` coedge names its own curve in `refs[7]`. Every fin's curve of a tolerant edge is null or a `00 85` bounded curve (§7.2) whose source is a `00 89` SP_CURVE on the fin's face's support surface, and every such edge-use stores a valued tolerance. An ordinary edge's own curve slot can also name an SP_CURVE, bounded or bare.

```
00 89 [ff]? attr  node_id u32  refs[5]  marker u8 (0x2b|0x2d)
            surface  b_curve  original  tolerance_to_original f64
```

The header has the bounded curve's layout. `surface` names the support-surface carrier and `b_curve` a `00 86` B-curve whose `00 88` descriptor is two-dimensional: `dimension` 2 with poles `u v`, or `dimension` 3 with the `rational` logical set and poles `u·w v·w w`. The poles are surface parameters in the support's stored units: a plane's two parameters and a cylinder's or cone's axial parameter are lengths in metres, and angles and B-surface parameters are dimensionless. `original` is null or names a model-space curve. Every pointer uses the XT form (§4).

The bounded curve's stored start and end points are the support evaluated through the B-curve at its two stored parameters, to within `1e-5` mm. The two fins of one edge carry different SP_CURVEs, one on each face, which agree with each other only to about the edge's tolerance. An edge's vertices can lie further off, by up to the vertex's own stored tolerance.

A tolerant edge's neutral curve is the parametric curve on the support of its canonical fin's bounded SP_CURVE, else its partner fin's, over the bounded interval, and the edge runs from that interval's low end. A tolerant ring edge, with no vertex, seams at the low end. The curve is lifted from a fin to the edge, so it is derived rather than stored, and the edge carries its stored tolerance. A vertex whose bare vertex-use stores a value greater than zero in its tolerance slot is a tolerant vertex and carries that value, converted to millimetres, as its tolerance; the tripled vertex-use form is read with none.

An ordinary edge whose own curve slot names an SP_CURVE directly, with no `00 85` between them, has no stored end points and no interval for it. Its curve is the support evaluated through the B-curve over the B-curve's whole knot domain, from knot `degree` to knot `count - degree - 1`: the edge's two vertices are the support's points at the two ends of the domain. Such an SP_CURVE holds the parametric curve on the support over the whole domain when the two vertices of every edge that names it lie within 1e-5 mm of the domain's two ends in exactly one order. It is derived like the tolerant edge's, with the cache below over that domain; the edge runs from the domain's low end and, being an ordinary edge, carries no tolerance. An SP_CURVE whose support no emitted face carries stays opaque, though such an edge still runs from the domain's low end.

An SP_CURVE is not admitted, and its edges keep an opaque curve, when an edge that names it has no vertex pair, has an end further than 1e-5 mm from its vertex, or has ends that fit in both orders (one vertex on a closed curve, which leaves the direction undetermined), or when a tolerant edge reaches it through a fin, whose vertices are promised only to the edge's tolerance. 

The neutral curve carries a solved cache: a cubic model-space B-curve in the SP_CURVE's own parameter over the bounded interval, fitted to the support evaluated through the B-curve with a bound of 1e-5 mm. It is fitted, not stored. The fit is a cubic Hermite span between consecutive distinct knots of the two-dimensional B-curve inside the interval (knots within a billionth of the interval of each other or of its ends are one). A span is halved until its departure at seven parameters is within a quarter of the bound, and, on a B-surface support, until it crosses at most one knot line of the support in each direction: a B-surface is one polynomial piece between knot lines, and a span over several can hold detail between the seven parameters. The cache passes through the support at both ends of the interval and at every span end. A span end takes a triple knot in the cache when sharing one derivative across it would move a pole by more than 1e-7 mm, and a double knot otherwise. The cache-fit tolerance is 1e-5 mm. The bound is tested, not proved. A curve has no cache when its support does not evaluate somewhere on the interval, when halving reaches 4 096 spans, or when a span a billionth of the interval wide is still outside the bound.

## 8. Auxiliary lanes

- **DisplayLists tessellation** uses a 6-descriptor table: List A strip lengths, Positions/Normals f32 metres, and Lists B/C/D. Each descriptor is `item_size u32 LE`, `kind u32 LE`, `flags u32 LE`, `count u32 LE`, then `item_size * count` data bytes. All six descriptors have `flags = 2`. Their `(item_size, kind)` pairs are `(4, 8)`, `(12, 100)`, `(12, 100)`, `(4, 8)`, `(4, 8)`, and `(1, 8)`. Let `N = len(ListA)`, `C = sum(ListA)`, and `E = sum(2*ListA[i] - 2)`. The Positions count is `C`; the Normals count is zero or `C`; `ListC[i] = 2*ListA[i] - 2`; and `TriCount = C - 2*N`. Lists B and D are both empty or both have `E` entries. List B contains finite f32 values. List D contains byte values.

  A table ends at the end of its sixth descriptor payload. The following framed metadata belongs to that table until the next validated table starts.

  A `uoBodyPropInfo_c` class declaration separates body-property groups inside the face-table sequence. Face descriptor tables continue after that declaration without another `uoTempFaceTessData_c` declaration. A following `uoTempFaceTessData_c` declaration, or the end of the `DisplayLists` payload when there is no following declaration, ends the sequence.

  A `uoTempFaceTessData_c` class token has two table-header forms. Offsets are from the byte after the class token. Both forms start with the first table's triangle count as u32 LE at `+0` and strip count as u32 LE at `+4`. These values equal `C - 2*N` and `N` for that table. In the compact form, the first descriptor starts at `+8`. The extended form has u32 LE values `1`, `0`, `0`, and one nonzero token at `+8`, `+12`, `+16`, and `+20`, followed by 16 zero bytes at `+24`; its first descriptor starts at `+40`. The fixed cells and nonzero token select the extended form. A header that does not satisfy the complete extended grammar uses the compact offset and must frame a valid descriptor table there.

  Each extended table can carry the same 40-byte header immediately before its first descriptor without another class declaration. Its triangle and strip counts agree with that table. Let `M` be the exact sixth-descriptor payload end. The extended-form metadata has Boolean u32 LE cells `A` at `M+0x5c`, `B` at `M+0x68`, and `C` at `M+0x6c`; u32 LE `1` at `M+0x60`; an independent u32 word at `M+0x64`; and a four-f32 LE tuple at `M+0x70`. All cells through `M+0x80` belong to the same bounded metadata interval, before the following table header.

  The supported extended form words are:

  | A | B | C | header word |
  |---|---|---|-------------|
  | 0 | 0 | 0 | `0x00201296` |
  | 0 | 0 | 1 | `0x00201a96` |
  | 1 | 0 | 1 | `0x00201b96` |
  | 0 | 1 | 1 | `0x00201e96` |

  For these four triples, the word equals `0x00201296 | (A << 8) | (B << 10) | (C << 11)`. Other triples have no defined word. The independent word at `M+0x64` does not contribute. `A=1` requires one repeated 12-byte stored normal pattern; `A=0` requires more than one pattern. This classification requires a nonempty normal channel and compares bytes, including signed-zero bits. `B=0` requires sixteen zero tuple bytes. `B=1` requires a positive finite scalar and a finite unit three-vector within f32 rounding; the unit-length validation tolerance is `1e-6`. These cells do not identify a surface family, face sense, or winding.

  The mapping requires the complete six-descriptor table, header shape and counts, bounded metadata cells, supported Boolean triple, normal classification, tuple constraints, and agreement between the stored and calculated word. A missing or contradictory field, unsupported triple, or unknown word supplies no known form. The structurally framed table and its channels remain intact. A writer can regenerate a supported word from its three stored flags; the base word is not a default for other states.

  An exact-support table has every position on its owning B-rep face support surface. The f32 coordinate precision limits this incidence test. When exactly one B-rep face has an analytic support incident to all table positions, that face owns the table and its body is the face's topological owner.

  A NURBS support uses the same incidence rule with a constructive parameter witness. For every table position, a bounded surface inversion must return a parameter pair whose forward-evaluated surface point is within the greater of the face tolerance and the display quantization tolerance. When exactly one NURBS face admits every table position, that face owns the table. A nearest-support result without a bounded within-tolerance witness does not establish ownership.

  A table may instead be a chordal support cache on an analytic surface. The normal-supported path requires a finite source normal at every position and an analytic surface normal at every corresponding position. The absolute normal dot product is at least `1 - 1e-4`. The candidate's measured support deflection is the maximum analytic-surface residual over the table positions. The candidate with the least deflection is unique after the display quantization tolerance; a tie remains unresolved. A complete bounded-face relation is applied only among candidates tied at that least deflection. When finite source normals are present but no candidate has normal agreement, a unique complete bounded-face relation plus the same analytic residual witness can certify an analytic candidate; an unbounded geometric coincidence cannot. The selected cache keeps its measured deflection as `chordal_deflection`; this value is not replaced with a fixed accuracy floor.

  Coincident planar supports are distinguished by a complete bounded-face relation. This relation consists of one ordered simple loop whose edges are lines or bounded ellipse arcs, one one-edge closed circular outer loop, or one ordered simple outer loop with zero or more one-edge closed circular inner loops. An ellipse edge is the endpoint-constrained shorter arc of its carrier; an ambiguous half or full arc does not establish the relation. Every table position is inside or on the outer boundary and outside or on each circular inner loop. An indexed triangle does not cross a circular inner loop. An edge between two circle-boundary positions can be the chordal approximation of that loop. When exactly one coincident face contains the complete table under these rules, that face owns the table. An incomplete boundary relation does not establish ownership; a validated inner circular loop can still reject a candidate when a table position lies inside it.

  Coincident cylindrical supports are distinguished by a complete axial and angular boundary relation. The face has one loop whose boundary edges are axial lines or support-radius circles. The axial interval is the minimum and maximum projection of every boundary vertex onto the cylinder axis. The angular interval is the shortest circular interval containing every boundary vertex; a single seam angle denotes the full circumference. Every table position must lie within both intervals. When exactly one coincident face contains the complete table under this rule, that face owns the table. A face with a different boundary grammar does not eliminate a coincident candidate.

  Coincident conical supports use the same complete axial and angular boundary relation. The face has one loop whose boundary edges are axial lines, degree-one non-periodic two-pole NURBS generatrices, or support cross-section circles when `ratio = 1` and aligned ellipses for other positive ratios. Let `a` be the unit cone axis, `r` the support reference direction projected orthogonal to `a`, `t = a × r`, `q = (P - origin) · a`, and `s = tan(half_angle)`. The axial interval is the minimum and maximum `q` over the boundary vertices. The angular coordinate is `atan2((P - origin) · t / ratio, (P - origin) · r)`; the angular interval is the shortest circular interval containing every boundary vertex. The local cross-section radius is `abs(radius + q*s)`. Every table position must lie within the axial interval and angular interval, using the display quantization tolerance scaled by that local radius. When exactly one coincident face contains the complete table under this rule, that face owns the table. A face with a different boundary grammar does not eliminate a coincident candidate.

  A direct DisplayLists visual-property instance begins with its stream's `moVisualProperties_c` class-reference tag, a u16 LE value with bit 15 set. Its packed `0x00BBGGRR` colour is u32 LE at `+2`. Bytes `ff fe ff` at `+18` and a u8 UTF-16 code-unit count at `+21` frame its material name at `+22`. This is a tokenless `moVisualProperties_c` payload.

  The tag's low 15 bits are the class's index in the stream's archive map, which numbers each class and each object in the order the stream first stores it. The index therefore depends on what the stream stores before the `moVisualProperties_c` class declaration, among it the `uiLineFontMgr_c` line-font count (u16 LE after that class name) and the lights; `32 80`, `33 80`, `34 80` and `38 80` all occur. A stream's tag is the one value from `01 80` to `fe 80` that follows the `moVisualProperties_c` class declaration and precedes a visual-property payload whose two packed colours have a zero high byte, whose u32 at `+4` is zero and whose two framed names are present. A stream with no such declaration, or with two such values, states no direct instance. A wider high byte is not read as a tag: the bytes after `00 00 80 bf`, an f32 -1.0, can frame as a payload.

  A visual-property instance in the framed property slot after a validated `uoTempFaceTessData_c` table assigns a face-local appearance. A visual-property instance in a following `uoBodyPropInfo_c` interval assigns a body default to the structurally preceding table group.

  A DisplayFace metadata slot can contain duplicated framed UTF-16LE strings. Their frame is `ff fe ff`, a u8 code-unit count, then the UTF-16LE payload. A payload of the form `<SurfIdRep class>,<feature source ID>,<local surface ID>[,<signed path field>...],` is a persistent surface reference when the class begins with `mo` and ends with `SurfIdRep_c`. The terminal comma is part of the framed text convention. A signed path field is a decimal signed 32-bit value compared by its u32 bit pattern; decimal `-1` therefore denotes `0xffffffff`. When every trailing field is signed numeric, the complete identity is the source ID, local surface ID, and ordered trailing fields. All complete references in one slot must agree on that identity, which joins the slot to the surviving face whose `FACE_ID_2001` carries the same values; the join is valid only when it selects one face and that face has one body. A reference with an opaque trailing field remains a source-level reference for appearance matching but cannot establish face ownership. A missing, conflicting, or multiply resolved complete identity leaves face ownership unresolved. The feature source ID alone remains sufficient for feature-appearance matching only when all references agree on that source ID.

  A `ThirdPtyStore/VisualStates` appearance instance begins with an unconstrained u32 LE object prelude, the feature source ID, the feature timestamp, u32 LE values `1`, `1`, and `2`, bytes `07 80 01 00 00 00`, and marker `09 80`. The packed `0x00BBGGRR` colour follows the marker. The source and timestamp select a feature assignment only through one Keywords feature, one neutral model feature, and one feature-input object. Current `FACE_ID_2001` owners with that source and timestamp establish the generated-face scope, including four-word feature-scoped owners. A matching DisplayFace persistent surface source ID binds a validated feature appearance to the table. Effective appearance precedence is part, body, feature, then face. Equal-rank conflicting assignments supply no unique effective appearance; an unresolved higher-rank relation does not supply a lower-rank fallback.
  A framed string with a u8 code-unit count of `ff` stores its count in the following u16 LE; the payload then begins after that u16.

  A visual-property payload begins at its packed colour. Offsets in this paragraph are from that colour. A zero u32 LE is at `+4` and a second packed colour is at `+8`. Two framed strings begin at `+12`: the display name, then the material name. Either can be empty. The appearance name is the material name when it is not blank, and the display name otherwise. Six f64 LE optical scalars follow the material name, then a u32 LE texture flag. Flag `1` introduces a texture block: 104 bytes, the framed texture image path, then 22 bytes. Flag `0` states no texture block. Five or six u32 LE state cells follow, then an empty framed string, 16 bytes, the framed appearance short name, 81 bytes, the framed appearance file path, 12 bytes, and the framed surface-finish map path. The state-cell count is the first of five and six for which every later frame of this run is present. An empty path states no image. The texture image path feeds the appearance colour; the surface-finish map path feeds its surface relief.

  A `moDecalProperties_c` instance is an 84-byte body, the framed decal preset path, and the framed decal image path. The preset path ends with `.p2d`. The first instance follows the class declaration. A later instance follows a class-reference tag, a u16 LE value with bit 15 set. The body stores the decal horizontal location as f64 LE at `+8`, the decal vertical location as f64 LE at `+16`, the decal rotation in degrees as f64 LE at `+24`, the decal width as f64 LE at `+32`, and the decal height as f64 LE at `+40`.

  A DisplayLists decal record stores its framed image path. Offsets in this paragraph are from the end of that path. An extent pair is two f64 LE values at `+6` and `+14`. Two empty framed strings begin at `+22`. A nonzero u32 LE face-entry count is at `+315`, and that many 32-byte decal face entries follow it. A decal face entry stores the face identifier as u32 LE at `+0`, 16 zero bytes at `+4`, the extent pair as two f32 LE at `+20` and `+24`, and a zero u32 LE at `+28`. Each entry's f32 pair, widened to f64, equals the record's extent pair. A record that does not satisfy this complete grammar states no face list.

  The trailer of a face table stores that table's face identifier as u32 LE after its visual-property object slot; two zero bytes precede the identifier. A decal face identifier joins the one face table whose trailer stores it. An identifier that no trailer stores, or that several trailers store, joins no table. A DisplayLists decal record and a `moDecalProperties_c` instance whose image paths are equal without ASCII case are one decal when each path occurs once among the records and once among the instances.
- A DisplayLists scene-class declaration scopes its following scene objects. The byte sequence `00 00 00 00 00 00 30 40 00 00 00 00` followed by a nonzero u32 LE source identifier binds that object class to the Keywords record with the same source identifier. Anonymous object counts do not identify Keywords records.
- **Materials / metadata** live in SW Objects blocks: `moVisualProperties_c` contains appearance definitions with material names and RGB `0x00BBGGRR` values; names use UTF-16LE. A class-token occurrence does not establish an appearance assignment. A document metadata record starts with its ASCII class token. Offsets below are from the end of that token. Scalar fields are little-endian.

| token                   | offset | type      | field                               |
| ----------------------- | ------ | --------- | ----------------------------------- |
| `moBBoxCenterData_c`    | +0     | u32       | `1`                                 |
|                         | +4     | f64[3]    | bounding-box center xyz, metres     |
|                         | +28    | f64       | maximum radius, metres              |
| `moDefaultRefPlnData_c` | +0     | f64[3]    | datum origin xyz, metres            |
|                         | +24    | f64[6]    | datum frame, dimensionless          |
| `moTransRefPlaneData_c` | +0 | bytes[24] | opaque serializer prologue |
| | +24 | u8 | extent presence: 0 or 1 |
| | +25 | f64[3] | stored enclosure center xyz, metres; present branch |
| | +49 | f64[3] | axis-aligned upper xyz, metres; present branch |
| | +73 | f64[3] | axis-aligned lower xyz, metres; present branch |
| | +97 | f64 | stored enclosing-sphere radius, metres; present branch |
| `moPart_c`              | +0     | u32       | part identifier                     |
|                         | +4     | u32       | `0`                                 |
|                         | +8     | u32       | part version                        |
|                         | +12    | u8        | `0`                                 |
| `moConfigurationMgr_c`  | +66    | u32       | configuration-manager minor version |
|                         | +107   | u8        | configuration state count           |
|                         | +117   | u64       | FILETIME timestamp                  |
|                         | +125   | —         | record end                          |
| `moLengthUserUnits_c`   | +0     | bytes[3]  | `ff fe ff`                          |
|                         | +3     | u8        | UTF-16 code-unit count `n`          |
|                         | +4     | bytes[2n] | linear-unit name, UTF-16LE          |

The direct `moTransRefPlaneData_c` declaration has the six-byte prefix `ff ff 01 00 15 00` before its 21-byte ASCII token. The body has a 25-byte fixed prologue. Presence zero omits all ten enclosure doubles; presence one includes exactly 80 bytes at body +25. The 24 opaque bytes precede the presence byte without a fixed `ff` gap. Stored center and radius are independent fields; they are not derived from the two corners. The following class continuation is separate from this optional enclosure.

- **Per-face appearance** uses the inline `00 53` child defined in §5.
- **Embedded images** live in the `UserTextures` storage. `UserTextures/TextureNameMap` is a u32 LE count followed by that many framed strings. String `n` is the source path of `UserTextures/Texture_n`, whose payload is the image file. A texture path names an embedded image when the two paths are equal without ASCII case, or when the texture path begins with `<SystemTexture>` and its remaining components are the final components of the source path.
- **XML / UnQLite** carry OPC parts, document/feature metadata, unit metadata (`SW_UnitsLinear=0` = millimetres), and MessagePack UI data: auxiliary, not the exact B-rep.

---


`ThirdPtyStore/VisualStates` and the inflated payload of
`ThirdPtyStore/VisualStates__ZLB` carry the same semantic appearance stream.
One `visStateKeyData_c` interval carries a framed UTF-16LE key formed from a
configuration name and a nonempty display-state name separated by `_`.
A configuration name can appear inside angle brackets. A unique exact name
prefix selects its configuration. An absent name match supplies no configuration
scope. An inactive configuration does not
supply the active geometry's appearance scope. Multiple keys or multiple
matching configuration names supply no unique active scope.

A part-default definition follows one of these class sequences:
`moCompHolder_c`, `moUnitComponent_c`, `visStateVisualPropertiesAttribute_c`,
`moVisualProperties_c`; the same sequence with `visStateVisualStateAttribute_c`
between the attribute and definition; or `moUnitComponent_c`,
`visStateVisualPropertiesAttribute_c`, `visStateVisualStateAttribute_c`,
`moCompHolder_c`, `moVisualProperties_c`. The property-attribute body is
`01 00 00 00` or `01 00 00 00 00 00`. Class order establishes the relation;
a fixed distance between class tokens does not. A definition alone supplies
no part scope. An empty material name does not invalidate the stored colour.

A `VisualStates` body-component identity prefix is 17 bytes relative to the
`moCompSolidBody_c` body: a non-null high-bit u16 LE class token at `+0`,
seven zero bytes at `+2..+8`, and equal nonzero u32 LE body identity scalars
at `+9` and `+13`. The complete scalar resolves through a unique current
BODY `ATOM_ID_2001` owner. A conflicting or missing owner supplies no body
appearance scope.


## 9. Units

- **Length fields are metres.** Model-space coordinates, radii, and length-bearing values in compact analytic records, `00 1d` points, and B-spline control grids are stored in metres.
- Coordinates and radii convert from metres to millimetres by a factor of `1000`. Rational B-spline poles require dehomogenization (`[x*w, y*w, z*w, w] -> xyz`) before conversion.
- Directions, normals, axes, reference directions, knot values, and weights are dimensionless.
- `SW_UnitsLinear=0` denotes millimetre document units. It does not change stored coordinate values.
- Model coordinates use the world frame. No per-file rotation, translation, or scale field applies to these coordinates.

---

## 10. Inline record framing

Bare inline `00 51` ATTRIBUTE subrecords use the stored value count: their
slot count is `5 + flo`, where `flo` is the low byte of `flags`. The `disc`
field points to the ATTRIBUTE definition and does not select the framing.
`00 51` and `00 52` byte occurrences inside slots do not delimit records.

For a prefixed subrecord, `body[14] == 0x01`, each slot is `[01][hi][lo]`, and the first byte after the slot run is `00`. The zero byte self-delimits the slot run, and `end = pos + 14 + 3*slot_count + 1`, where `slot_count` is the number of complete triples. For a bare subrecord, `end = pos + 14 + 2*(5 + flo)`; the stored `flo` value is required to find the end.

---

## 11. Assembly documents

An `.sldasm` assembly document uses the container of §1. A document is an assembly when it has a `Contents/Config-N-MatesList` section or a `swXmlContents/COMPINSTANCETREE` section. A part document has neither.

### 11.1 Component instance tree

`swXmlContents/COMPINSTANCETREE` is a UTF-8 `swSolidWorks` XML document in the namespace `http://www.solidworks.com/sw2003/schema`. A compound-file envelope (§1) can store this section, a mate list (§11.3) or an embedded document (§11.4) as a wrapped stream whose name ends in `__ZLB`; the name without that suffix identifies the section.

- `swHeader/swFile` lists documents. `id` is the file identity, `swDocType` is `ASSEMBLY` or `PART`, and `swPath` is the absolute path that the saving system used. The first `swFile` is the assembly document itself.
- `swModelList/swModel` is one model of one document in one configuration. `id` is the model identity, `swFileRef` is the `swFile` identity and `swConfigurationName` is the configuration name. The `swName` of a sub-assembly model is the name of the top assembly, not of the sub-assembly; the document name comes from the `swFile` path.
- The `swReference` children of an assembly model are its placed component instances. `id` is the instance identity and `swModelRef` is the model of the component. A component whose model's `swFile` has `swDocType` `ASSEMBLY` is a sub-assembly. A model with `swReference` children is an assembly model. `swName` and `swReferenceNumber` make the instance name `<swName>-<swReferenceNumber>`. `swSuppressed`, `swHidden`, `swFlexible`, `swExcludeFromBOM`, and `swIsVirtualComponent` are `YES` or `NO`.
- `swConfigurationList/swConfiguration` lists the configurations of the assembly document. Exactly one has `swMostRecentConfiguration="YES"`; its `swModelRef` is the root model of the active configuration. Other configurations can have other root models, for example the generated `<name>-_flexible<n>` configuration of a flexible sub-assembly.

The `id` values of `swModel` and `swReference` elements are decimal numbers, distinct within the document. Models form a directed acyclic graph: one sub-assembly model can have more than one `swReference`. The placed-instance tree has one occurrence for each path from the root model.

### 11.2 Instance placement

`swTransform` is 16 decimal numbers: a 4x4 matrix `M` in row-major order for row vectors, `p' = p M`. Rows 0 to 2 hold the rotation, with a fourth column of `0`. Row 3 holds the translation in metres, then `1`. The column-vector rotation is the transpose of the upper-left 3x3 block. The translation converts to millimetres by a factor of `1000` (§9).

The placement is relative to the assembly model that contains the `swReference`. The world placement of an occurrence is the product of the placements on its path from the root model.

### 11.3 Mate lists

`Contents/Config-N-MatesList` holds the mates of configuration `N` as `SWObjects` records (§2). Mate classes have the prefix `moMate` (`moMateCoincident`, `moMateConcentric`, `moMateDistanceDim_c`). A mate operand names its component instance as `<instance name>@<assembly name>` and its face by a `SurfIdRep` string, the form of a part-feature face reference. A section without an `moMate` class token holds no mate.

### 11.4 Embedded components

`ImportedComp/<name>` sections, and sections whose names end in `.sldprt` or `.sldasm` (ASCII case-insensitive), are complete embedded documents of the container of §1. An embedded document can be an assembly. The Parasolid streams of an embedded document belong to that document, not to the assembly document.
