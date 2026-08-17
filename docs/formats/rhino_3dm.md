# Rhino 3DM format

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/rhino.md`](../layouts/rhino.md), generated from `docs/layouts/rhino.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. `cargo test -p cadmpeg --test layout_tables` proves the two agree.

## 1. Archive bands

Rhino 3DM is a little-endian chunk stream. Archive versions select these
container grammars:

| Archive version | Band              | Chunk value width | Container grammar |
| --------------: | ----------------- | ----------------: | ----------------- |
|               1 | V1                |           4 bytes | flat chunks       |
|               2 | V2                |           4 bytes | table sequence    |
|               3 | V3                |           4 bytes | table sequence    |
|               4 | V4                |           4 bytes | table sequence    |
|               5 | legacy V5 grammar |           4 bytes | table sequence    |
|              50 | V5                |           8 bytes | table sequence    |
|              60 | V6                |           8 bytes | table sequence    |
|              70 | V7                |           8 bytes | table sequence    |
|              80 | V8                |           8 bytes | table sequence    |
|              90 | V9                |           8 bytes | table sequence    |

The archive version is the decimal value in the header. Version `5` and version
`50` are distinct. Any positive decimal version fitting the eight-byte header
field is syntactically valid.

V1 uses a flat-chunk grammar and may omit the end marker. V2 and later use the
table sequence below and require an end-of-file chunk.

## 2. Header

The 32-byte start section begins at the first matching magic sequence within
the first 33554432 bytes. A leading application block can precede it. Relative
to the start section, the bytes are:

```text
bytes 0..23   ASCII "3D Geometry File Format "
bytes 24..31  right-justified decimal archive version
```

The version field contains leading ASCII spaces followed by one or more ASCII
decimal digits. Canonical forms include:

```text
3D Geometry File Format        1
3D Geometry File Format        5
3D Geometry File Format       50
3D Geometry File Format       80
3D Geometry File Format       90
```

The first post-header chunk is a long comment chunk with typecode `0x00000001`.
The comment's declared boundary, not a text terminator, determines its extent.

## 3. Primitive encodings

All numeric values are little-endian.

| Primitive                           | Encoding                                               |
| ----------------------------------- | ------------------------------------------------------ |
| `u8`, `i8`, `char`                  | one byte                                               |
| `u16`, `i16`                        | two bytes                                              |
| `u32`, `i32`, `unsigned int`, `int` | four bytes                                             |
| `f32`, `float`                      | IEEE-754 binary32, four bytes                          |
| `f64`, `double`                     | IEEE-754 binary64, eight bytes                         |
| `bool`                              | one byte, `0x00` false or `0x01` true                  |
| `ON_3dPoint`                        | three `f64` values, x/y/z                              |
| `ON_3dVector`                       | three `f64` values, x/y/z                              |
| `ON_Interval`                       | two `f64` values, lower/upper                          |
| `ON_BoundingBox`                    | minimum point followed by maximum point                |
| `ON_Xform`                          | sixteen `f64` matrix entries in row-major memory order |
| `ON_ComponentIndex`                 | `i32 component_type`, `i32 component_index`            |
| `ON_UUID`                           | mixed-endian GUID described below                      |

An array written by the archive array helpers is `i32 count` followed by
`count` consecutive elements. Negative counts are invalid. Counts are checked
against the containing bound before allocation.

### 3.1 Colors

`ON_Color` is four direct bytes in red, green, blue, alpha order. It does not
use numeric endian conversion. Its alpha byte is transparency: 0 is opaque and
255 is fully transparent. `ON_4fColor` is four little-endian `f32` values in
red, green, blue, alpha order, with conventional opacity alpha.

### 3.2 Plane, circle, and arc

An `ON_Plane` is 128 bytes:

```text
origin: x y z                         3 × f64
xaxis:  x y z                         3 × f64
yaxis:  x y z                         3 × f64
zaxis:  x y z                         3 × f64
plane equation: x y z d               4 × f64
```

The plane equation is serialized and is not reconstructed from the axes.

An `ON_Circle` is:

```text
ON_Plane plane                         128 bytes
f64 radius                               8 bytes
ON_3dPoint point_at_zero                24 bytes
ON_3dPoint point_at_half_pi             24 bytes
ON_3dPoint point_at_pi                 24 bytes
```

An `ON_Arc` appends `ON_Interval angle` to the circle. The three circle
consistency points are on the wire in every payload using `ON_Circle`.

### 3.3 UUIDs

The wire form of a canonical UUID
`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` is:

```text
Data1: u32 little-endian
Data2: u16 little-endian
Data3: u16 little-endian
Data4: eight bytes unchanged
```

For example:

```text
canonical: 4ED7D4DD-E947-11D3-BFE5-0010830122F0
wire:      DD D4 D7 4E 47 E9 D3 11 BF E5 00 10 83 01 22 F0
```

## 4. Chunk framing

Every chunk begins with a little-endian `u32 typecode`.

```text
TCODE_SHORT = 0x80000000
TCODE_CRC   = 0x00008000
```

The typecode category masks are:

```text
legacy geometry  0x00010000
object           0x00020000
geometry         0x00100000
annotation       0x00200000
display          0x00400000
render           0x00800000
interface        0x02000000
tolerance        0x08000000
table            0x10000000
table record     0x20000000
user             0x40000000
```

The short bit alone selects short framing. Other typecode bits select identity
and category and do not change the framing rule.

The value field width is selected by the archive version:

```text
archive version < 50   i32
archive version >= 50  i64
```

A long chunk has the short bit clear. Its value is the complete number of
bytes after the value field, including a trailing checksum when present:

```text
u32 typecode
i32/i64 declared_length
declared_length bytes
```

A short chunk has the short bit set. Its value field is the complete payload;
there is no body and no checksum:

```text
u32 typecode
i32/i64 value
```

A negative value on a typecode without the short bit selects a bodyless chunk.
Arithmetic overflow and an end beyond the containing bound are framing
failures. A V1 typecode zero is the legacy long-chunk form.

### 4.1 Checksums

For V2 and later, a long chunk with `TCODE_CRC` set ends with a four-byte
little-endian CRC32. The declared length includes those four bytes. CRC32 covers
the bytes written directly in the chunk body and excludes the chunk header,
stored CRC, and every complete nested chunk. A leaf chunk therefore checksums
all body bytes before its CRC. A container resumes its CRC after each nested
chunk and checksums only the intervening direct fields.

For V1, CRC16 is selected by the legacy chunk cases: legacy geometry chunks,
`TCODE_SUMMARY`, and the V1 class-UUID chunk. The V1 class-UUID checksum is
CRC16; the corresponding V2+ checksum is CRC32. CRC16 is not selected merely
by applying the V2 `TCODE_CRC` interpretation.

CRC16 is non-reflected CRC-CCITT:

```text
polynomial = 0x1021
initial seed for V1 chunks = 1
index = (crc >> 8) & 0xff
crc = ((crc << 8) ^ table[index] ^ byte) & 0xffff
```

The stored CRC16 is little-endian. Test vectors are:

```text
CRC16(seed=0, empty)        = 0x0000
CRC16(seed=1, empty)        = 0x0001
CRC16(seed=0, "123456789")  = 0xbeef
```

CRC32 is reflected IEEE/zlib CRC32 with initial value zero:

```text
CRC32(empty)       = 0x00000000
CRC32("123456789") = 0xcbf43926
```

A checksum mismatch does not change the declared framing boundary. A missing
checksum or invalid boundary makes the chunk structurally invalid.

### 4.2 Bounded cursors

Every long chunk creates a child bound:

```text
body_start = cursor after declared_length
declared_end = body_start + declared_length
crc_start = declared_end - checksum_width
body_end = crc_start
```

The chunk body cannot extend beyond `body_end`. The checksum occupies the bytes
from `crc_start` to `declared_end`. Child payload bytes cannot overlap a parent
trailer.

A reader consumes the fields defined for the payload major version. A later
minor version can append fields before the bounded end. Unread suffix bytes are
skipped at the bounded end. Array and element readers that define major version
1 accept every nonnegative minor version. A reader may reject a minor version
only when the containing archive grammar defines a writer-band ceiling for that
payload; this is a grammar admission rule, not a trailing-byte rule.

For a direct payload, the owning long chunk is the bounded end. An anonymous
child owns its own suffix boundary before its parent continues. A tagged stream
ends at its explicit terminator; an unknown item ID does not supply a generic
value width. These boundaries apply independently to nested payloads, so a
suffix in one child cannot consume the fields of its parent.
Every typed field is owned by exactly one such bounded payload. An unknown
suffix has no inferred value width until a producer-defined field grammar
assigns one.

The first outer properties table and the first outer settings table are read.
Within those tables, a repeated singleton record replaces the preceding value;
the last successfully read occurrence owns the field. A failed occurrence does
not replace a previously read value.

Stored Boolean fields use one byte. `0x00` is false and `0x01` is true. When
the writer version is unavailable or predates openNURBS 6.0.2017-08-24, any
nonzero byte is true. At or after that writer version, a Boolean byte other
than `0x00` or `0x01` is malformed. The strict threshold is encoded writer
version `2348836140`; the legacy `YYYYMMDDn` form uses `201708240`. A raw
character field uses its byte value and does not use this Boolean rule.

Presence and enumeration fields that define a separate numeric grammar retain
that grammar. An enumeration value outside its defined set is retained in
native source data, uses the field's documented neutral fallback, and emits a
typed degradation loss. It does not discard the containing record.

A stored geometry item count is bounded by its containing payload and the byte
width of one item. The format does not impose a separate 65536-item limit.

Redundant count and size fields have a defined repair boundary. A mesh optional
channel with a negative count, a count different from the mesh vertex count, or
a decompressed size different from its declared size is dropped while the base
mesh is retained. A mesh double-precision vertex channel with a count different
from the float vertex count is dropped and the float vertices remain
authoritative. A point-cloud normal, color, or scalar channel with a nonzero
count different from the point count is consumed and dropped while the points
remain. An embedded history SubD edge chain whose edge-ID and orientation
counts do not match its stored chain count retains the chain with both
dependent arrays empty. An optional Brep region topology with a face-side count
different from twice the face count is dropped while the Brep remains. Each
repair emits a `container.redundant-field-repaired` loss.

Brep vertex, edge, trim, loop, face, region face-side, and region positional
index fields are redundant. The serialized array position is authoritative when
one differs, and the stored positional value is retained only in native bytes.
Each affected array emits a `container.redundant-field-repaired` loss.

Counts and indices that control byte framing, required NURBS arithmetic, or core
topology references are admission invariants. A mismatch in those fields
rejects the affected record or causes its documented carrier fallback; it is
not repaired as an optional channel.

## 5. Versions and end of file

A packed payload version is one byte:

```text
major = version >> 4
minor = version & 0x0f
```

An anonymous payload version is two little-endian `i32` values:

```text
i32 major
i32 minor
```

These forms are not interchangeable.

`TCODE_ENDOFFILE = 0x00007fff` is a long, unchecksummed chunk. Its declared
length is at least the file-size field width:

```text
archive version < 50   length = 4, u32 file_size
archive version >= 50  length = 8, u64 file_size
```

The stored size includes the start section, all preceding archive chunks, the
EOF typecode, the EOF value field, and the file-size field. It is informational
and has no CRC. V1 may omit EOF; interior legacy `ENDOFFILE_GOO` markers are not
document termination.

## 6. Typecode registry

### 6.1 Tables

| Meaning                   |     Typecode |
| ------------------------- | -----------: |
| comment block             | `0x00000001` |
| end of file               | `0x00007fff` |
| end of file goo           | `0x00007ffe` |
| end of table              | `0xffffffff` |
| material table            | `0x10000010` |
| layer table               | `0x10000011` |
| light table               | `0x10000012` |
| object table              | `0x10000013` |
| properties table          | `0x10000014` |
| settings table            | `0x10000015` |
| bitmap table              | `0x10000016` |
| user table                | `0x10000017` |
| group table               | `0x10000018` |
| font table                | `0x10000019` |
| dimstyle table            | `0x10000020` |
| instance-definition table | `0x10000021` |
| hatch-pattern table       | `0x10000022` |
| linetype table            | `0x10000023` |
| obsolete layerset table   | `0x10000024` |
| texture-mapping table     | `0x10000025` |
| history-record table      | `0x10000026` |

### 6.2 Records and object framing

| Meaning                    |     Typecode |
| -------------------------- | -----------: |
| bitmap record              | `0x20008090` |
| material record            | `0x20008040` |
| layer record               | `0x20008050` |
| light record               | `0x20008060` |
| light record attributes    | `0x02008061` |
| light attributes userdata  | `0x02000062` |
| light record end           | `0x8200006f` |
| group record               | `0x20008073` |
| font record                | `0x20008074` |
| dimstyle record            | `0x20008075` |
| instance-definition record | `0x20008076` |
| hatch-pattern record       | `0x20008077` |
| linetype record            | `0x20008078` |
| obsolete layerset record   | `0x20008079` |
| texture-mapping record     | `0x2000807a` |
| history-record record      | `0x2000807b` |
| object record              | `0x20008070` |
| object record type         | `0x82000071` |
| object attributes          | `0x02008072` |
| attribute userdata         | `0x02000073` |
| object history             | `0x02008074` |
| history header             | `0x02008075` |
| history data               | `0x02008076` |
| object record end          | `0x8200007f` |
| user-table UUID             | `0x20008080` |
| user-table record header    | `0x20008082` |
| user record                 | `0x20000081` |

### 6.3 Properties, settings, and user chunks

| Meaning                   |     Typecode |
| ------------------------- | -----------: |
| revision history          | `0x20008021` |
| notes                     | `0x20008022` |
| preview image             | `0x20008023` |
| application               | `0x20008024` |
| compressed preview        | `0x20008025` |
| writer version            | `0xa0000026` |
| as-file-name              | `0x20008027` |
| units and tolerances      | `0x20008031` |
| render mesh settings      | `0x20008032` |
| analysis mesh settings    | `0x20008033` |
| annotation settings       | `0x20008034` |
| named construction planes | `0x20008035` |
| named views               | `0x20008036` |
| views                     | `0x20008037` |

The writer-version property is the openNURBS packed version `0xa0000026`.
Version-gated readers use this stored value independently of the archive
version.
| current layer             | `0xa0000038` |
| current material          | `0x20008039` |
| current color             | `0x2000803a` |
| current wire density      | `0xa000003c` |
| render settings           | `0x2000803d` |
| grid defaults             | `0x2000803f` |
| model URL                 | `0x20008131` |
| current font              | `0xa0000132` |
| current dimstyle          | `0xa0000133` |
| settings attributes       | `0x20008134` |
| plugin list               | `0x20008135` |
| render userdata           | `0x20008136` |
| anonymous chunk           | `0x40008000` |
| UTF-8 string chunk        | `0x40008001` |
| model attributes chunk    | `0x40008002` |
| dictionary                | `0x40008010` |
| dictionary ID             | `0x40008011` |
| dictionary entry          | `0x40008012` |
| dictionary end            | `0xc0000013` |
| XDATA                     | `0x40000001` |

The compressed-preview record (`0x20008025`) body is:

```
i32 biSize
i32 biWidth
i32 biHeight
i16 biPlanes
i16 biBitCount
i32 biCompression
i32 biSizeImage
i32 biXPelsPerMeter
i32 biYPelsPerMeter
i32 biClrUsed
i32 biClrImportant
u32 compressed-buffer size
if size > 0:
  u32 CRC32 of the uncompressed buffer
  u8 compression method
  if method = 0: size direct bytes
  if method = 1: anonymous long chunk containing deflate bytes
```

The palette color count is `biClrUsed` when nonzero; otherwise it is 2, 16,
or 256 for `biBitCount` 1, 4, or 8, and zero for other bit counts. The
compressed-buffer size is the palette bytes plus `biSizeImage` for a
contiguous bitmap. For a non-contiguous bitmap, the first buffer is the
palette and a second buffer of `biSizeImage` bytes follows. A zero-size
buffer has only its size field; a non-contiguous bitmap can then continue with
the second image buffer. Method 1 stores deflate bytes in the anonymous child;
the anonymous child is a complete CRC-bearing chunk and its bytes are excluded
from the preview record's direct CRC. The direct CRC covers the bitmap header,
buffer sizes, uncompressed-buffer CRCs, method bytes, and method-0 bytes.

### 6.4 Class wrapper chunks

| Meaning         |     Typecode |
| --------------- | -----------: |
| class wrapper   | `0x00027ffa` |
| class userdata  | `0x00027ffd` |
| userdata header | `0x0002fff9` |
| class UUID      | `0x0002fffb` |
| class data      | `0x0002fffc` |
| class end       | `0x80027fff` |

The class-data body is owned by the class grammar. It is not a flat sequence
of child chunks: direct fields can occur before, between, and after complete
nested chunks. A class reader consumes each nested chunk at the field that
owns it and validates the declared boundary there. A class wrapper scanner
must not apply one flat child-chunk or checksum range to the complete
class-data body. The complete wrapper order is the class UUID chunk, the
class-data chunk, zero or more class-userdata chunks, and the class-end chunk.
After the class reader consumes its known fields, unread bytes through the
class-data boundary are skipped. A class UUID selects the class grammar; it
does not supply a common grammar for classes outside the built-in registry.

### 6.5 Object-type filter bitfield

`TCODE_OBJECT_RECORD_TYPE` is a short chunk whose value is a `u32` bitfield.
The values below are the defined bits:

|    Bit value | Meaning                  | Model object                       |
| -----------: | ------------------------ | ---------------------------------- |
| `0x00000000` | unknown                  | no declared type                   |
| `0x00000001` | point                    | `ON_Point`                         |
| `0x00000002` | point set                | point cloud or point grid          |
| `0x00000004` | curve                    | `ON_Curve`                         |
| `0x00000008` | surface                  | `ON_Surface`                       |
| `0x00000010` | Brep                     | `ON_Brep`                          |
| `0x00000020` | mesh                     | `ON_Mesh`                          |
| `0x00000040` | layer                    | `ON_Layer`                         |
| `0x00000080` | material                 | `ON_Material`                      |
| `0x00000100` | light                    | `ON_Light`                         |
| `0x00000200` | annotation               | annotation object                  |
| `0x00000400` | userdata                 | userdata object                    |
| `0x00000800` | instance definition      | `ON_InstanceDefinition`            |
| `0x00001000` | instance reference       | `ON_InstanceRef`                   |
| `0x00002000` | text dot                 | `ON_TextDot`                       |
| `0x00004000` | grip                     | selection filter, not a model type |
| `0x00008000` | detail                   | detail view                        |
| `0x00010000` | hatch                    | `ON_Hatch`                         |
| `0x00020000` | morph control            | `ON_MorphControl`                  |
| `0x00040000` | SubD                     | `ON_SubD` and SubD references      |
| `0x00080000` | loop                     | Brep loop                          |
| `0x00100000` | Brep vertex filter       | selection filter                   |
| `0x00200000` | polysurface filter       | selection filter                   |
| `0x00400000` | edge filter              | selection filter                   |
| `0x00800000` | polyedge filter          | selection filter                   |
| `0x01000000` | mesh vertex filter       | mesh/SubD component filter         |
| `0x02000000` | mesh edge filter         | mesh/SubD component filter         |
| `0x04000000` | mesh face filter         | mesh/SubD component filter         |
| `0x07000000` | mesh component reference | mesh/SubD component reference      |
| `0x08000000` | cage                     | NURBS cage                         |
| `0x10000000` | phantom                  | phantom object                     |
| `0x20000000` | clipping plane           | clipping-plane object              |
| `0x40000000` | extrusion                | `ON_Extrusion`                     |
| `0xffffffff` | any                      | all bits                           |

The value may contain multiple bits. A zero filter selects all objects;
otherwise an object is selected when its nonzero type value has any bit in
common with the filter:
`(object_type & filter) != 0`. Filter-only bits are valid in a filter but do
not identify standalone model records. A zero object-type value denotes an
unknown type.

## 7. Tables and object records

The normal V2+ table sequence is:

1. comment/start;
2. properties;
3. settings;
4. bitmap;
5. texture mapping;
6. material;
7. linetype;
8. layer;
9. group;
10. font;
11. dimstyle;
12. light;
13. hatch pattern;
14. instance definitions;
15. objects;
16. history records;
17. zero or more user tables;
18. EOF.

Optional tables may be absent. A table is a bounded table chunk containing
record chunks. A short `TCODE_ENDOFTABLE` with value zero normally terminates
the records. If the marker is absent, the table chunk boundary terminates the
records and scanning emits a warning. A present marker must be the final table
child. Every record is contained within the table bound.

An object record is:

```text
OBJECT_RECORD long chunk
  OBJECT_RECORD_TYPE short chunk
  OPENNURBS_CLASS long chunk
    OPENNURBS_CLASS_UUID long chunk
      UUID (16 bytes)
      CRC body (4 bytes in V2+)
    OPENNURBS_CLASS_DATA long chunk
      class payload
    zero or more CLASS_USERDATA chunks
    OPENNURBS_CLASS_END short chunk, value zero
  optional OBJECT_RECORD_ATTRIBUTES long chunk
  optional OBJECT_RECORD_ATTRIBUTES_USERDATA long chunk
  optional OBJECT_RECORD_HISTORY long chunk
  OBJECT_RECORD_END short chunk
```

The object type is a category bitfield, not a class identity. The UUID chunk
has declared body length 20 in V2+: sixteen UUID bytes and four checksum bytes.
The checksum is finalized by chunk handling, not interpreted as class payload.
The class-data checksum is likewise selected by its enclosing typecode. The
class wrapper length includes all child chunk headers, values, bodies, and
checksums.

### 7.1 History records

Each `TCODE_HISTORYRECORD_RECORD` contains one class wrapper for class UUID
`ECD0FD2F-2088-49DC-9641-9CF7A28FFA6B`. Its class-data payload is an anonymous
chunk with major version 1. The writer emits minor 1 before archive 60 and
minor 2 from archive 60. Minor version 1 adds the record type; minor version 2
adds the copy-on-replace flag:

```text
anonymous version 1.minor
ON_UUID record_id
i32 command_version
ON_UUID command_id
ON_UuidList descendants
ON_UuidList antecedents
anonymous version 1.minor values
  i32 value_count
  value_count × history value anonymous chunk
if minor >= 1: i32 record_type
if minor >= 2: bool copy_on_replace
```

An `ON_UuidList` is an anonymous major-1 chunk with a nonnegative minor
version. It contains an archive array of UUIDs; fields after that array are
skipped at the chunk boundary. Descendant order is serialized order.
Antecedents identify input objects and descendants identify output objects. A
descendant UUID belongs to the record that lists it, but the wire format does
not require that UUID to occur in only one record and does not carry a producer
selector for an antecedent. Multiple records can therefore produce the same
descendant UUID without a unique history dependency in the bytes.
`record_type` is 0 for update history parameters and 1 for feature parameters.

The values wrapper and every history value are independent anonymous chunks.
The writer emits version 1.0 for both. Each history value is an anonymous
major-1 chunk:

```text
i32 value_type
i32 value_id
type-specific payload
```

The fixed-layout value-type numbers are:

| Value type | Payload                                      |
| ---------: | -------------------------------------------- |
|          0 | no payload                                   |
|          1 | archive array of one-byte booleans           |
|          2 | archive array of `i32`                       |
|          3 | archive array of `f64`                       |
|          4 | archive array of four-byte `ON_Color` values |
|          5 | archive array of `ON_3dPoint`                |
|          6 | archive array of `ON_3dVector`               |
|          7 | archive array of `ON_Xform`                  |
|          8 | archive array of UTF-16 strings              |
|          9 | archive array of object references           |
|         10 | geometry-value anonymous chunk               |
|         11 | archive array of UUIDs                       |
|         12 | reserved; no value implementation            |
|         13 | polyedge-value anonymous chunk               |
|         14 | SubD-edge-chain-value anonymous chunk        |

Every value is independently bounded by its anonymous chunk. The next value or
record suffix begins at that chunk's declared end.

An object reference is an anonymous major-1 chunk. Minor 1 adds the first two
evaluation intervals, minor 2 adds the third, and minor 3 adds object snap
mode. Later minors append fields and leave an unread bounded suffix:

```text
ON_UUID object_id
ON_ComponentIndex component
i32 geometry_type
ON_3dPoint selection_point
i32 evaluation_type
ON_ComponentIndex evaluation_component
4 × f64 evaluation_parameter
array of instance-reference path items
if minor >= 1: 2 × ON_Interval evaluation_interval
if minor >= 2: ON_Interval third_evaluation_interval
if minor >= 3: i32 object_snap_mode
```

An evaluation interval gated by the minor version is absent when its field is
not stored. It is not the interval `[0,0]`.

An instance-reference path item is an anonymous major-1 chunk:

```text
ON_UUID instance_reference_id
ON_Xform instance_transform
ON_UUID instance_definition_id
i32 definition_geometry_index
if minor >= 1:
  ON_ComponentIndex component
  object-evaluation anonymous major-1 chunk
```

A geometry value is an anonymous major-1 chunk containing an `i32` count
followed by that many polymorphic class wrappers. Every non-null wrapper
contains its geometry class UUID, class-data payload, zero or more class
userdata items, and a class-end marker. A null wrapper contains only a nil
class UUID child. When a wrapper contains `ON_Mesh` or `ON_Extrusion`, its
class-userdata payload follows the owning-class rules in sections 7.2.3 and
16, respectively; the geometry value reader applies those rules to the
embedded owner.

A polyedge value is an anonymous major-1 chunk containing an `i32` count
followed by that many polyedge anonymous major-1 chunks:

```text
i32 segment_count
segment_count × curve-proxy-history anonymous chunk
archive array of f64 polyedge_parameters
i32 evaluation_mode
```

A curve-proxy-history chunk is anonymous major 1. Minor 1 adds the edge and
trim domains:

```text
object reference
bool reversed
ON_Interval full_real_curve_domain
ON_Interval sub_real_curve_domain
ON_Interval proxy_curve_domain
if minor >= 1:
  ON_Interval segment_edge_domain
  ON_Interval segment_trim_domain
```

A SubD edge-chain value is an anonymous major-1 minor-1-or-later chunk
containing an `i32` count followed by that many edge-chain anonymous major-1
minor-1-or-later chunks:

```text
ON_UUID persistent_subd_id
u32 edge_count
archive array of u32 persistent_edge_ids
archive array of u8 persistent_edge_orientations
```

Both archive-array counts must equal `edge_count`. Orientations are 0 for
forward and 1 for reversed traversal.

The object-evaluation chunk contains `i32 evaluation_type`, an
`ON_ComponentIndex`, four `f64` evaluation parameters, and three
`ON_Interval` values. Path items are ordered from the selected instance
reference through nested definitions to the referenced definition geometry.

### 7.2 Class userdata

A class userdata chunk begins with a packed version byte.

An object accepts one userdata item for each item UUID. A duplicate item UUID
is rejected by attachment, so the first serialized item owns the object state.
Attached built-in extension readers that select by class UUID use the first
serialized matching item; later matching items remain bounded source records and
do not replace it. An obsolete extension that is consumed by `DeleteAfterRead`
is not attached and follows the class-specific rule for that side effect.

Major `1` fields:

```text
userdata class UUID
userdata item UUID
i32 copy count
ON_Xform userdata transform
```

Major `2` uses a userdata-header child chunk:

```text
userdata class UUID
userdata item UUID
i32 copy count
ON_Xform userdata transform
UUID application ID                  minor >= 1
bool last-saved-as-goo               minor >= 2
i32 userdata archive version         minor >= 2
i32 userdata writer version          minor >= 2
```

The header has the checksum selected by its typecode. An anonymous child
contains the userdata payload. Older userdata without archive-version fields
uses the containing archive version below 50 and archive version 5 with
four-byte chunk lengths at 50 and later. The anonymous child is always bounded.
The generic userdata header is version `2.2`. Its version 2.1 prefix adds the
application UUID, and version 2.2 adds the last-saved-as-goo flag, userdata
archive version, and userdata writer version. The reader admits only major 1
and major 2; another major has no header grammar.
The userdata-header reader consumes the fields defined by its minor version and
skips a later bounded suffix. The userdata payload is owned by the userdata
class and is skipped at its anonymous-child boundary when no typed reader owns
that class.

#### 7.2.1 `ON_UserStringList`

The built-in `ON_UserStringList` class uses class UUID and item UUID
`CE28DE29-F4C5-4FAA-A50A-C3A6849B6329`. Its application UUID is
`17B3ECDA-17BA-4E45-9E67-A2B8D9BE520D`. Its userdata payload is the body of
the outer anonymous userdata child from section 7.2. The body contains one
anonymous major-1, nonnegative-minor list child:

```text
i32 anonymous major = 1
i32 anonymous minor
i32 user-string count
count × anonymous user-string entry
```

Each entry is an anonymous major-1, nonnegative-minor child:

```text
i32 anonymous major = 1
i32 anonymous minor
UTF-16 key
UTF-16 value
```

The list and entry readers consume the known fields and skip bytes through
their own anonymous boundaries. The count is nonnegative and every entry is
bounded by its child chunk. The writer emits major 1, minor 0 for the list and
each entry.

`ON_Object::SetUserString` rejects an empty key. A nonempty value updates the
first existing key using case-insensitive ordinal comparison and preserves
that entry's key and position. A null or empty value removes the first
matching entry. A new nonempty key appends an entry. The serialized list keeps
the resulting order.

#### 7.2.2 `ON_OBSOLETE_V5_TextExtra`

The built-in `ON_OBSOLETE_V5_TextExtra` class uses class UUID and item UUID
`D90490A5-DB86-49F8-BDA1-9080B1F4E976`. Its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`. Its userdata payload is the body of
the outer anonymous userdata child from section 7.2. The body contains one
anonymous major-1, minor-0 child:

```text
UUID parent text
bool draw text mask
i32 mask color source
4 × u8 mask color (red, green, blue, alpha)
f64 border offset factor
```

The parent UUID is nil when no parent text identity is assigned. Mask color
source `0` selects the viewport background and source `1` selects the stored
mask color. The setter writes every other source value as `0`; the reader
retains the stored `i32`. The border offset factor is dimensionless. For a V5
text object, the mask border extends each side of the tight text rectangle by
the factor multiplied by the text height. The reader consumes the known
fields through the anonymous boundary and skips later minor-version suffix
bytes. The OpenNURBS 5 application UUID retains this userdata when a V6 model
is saved as V5. The V4 model-save compatibility filter excludes it; the
class-wrapper grammar itself remains valid for any archive band.

CADIR maps a matching class-userdata item on a V5 text object to the
annotation's `v5_text_extra` native value. A nil parent UUID becomes null. The
mask color remains four RGBA bytes and the border offset remains a
dimensionless factor; neither is unit-scaled. A malformed recognized payload
retains the annotation record, omits `v5_text_extra`, and emits an
`annotation.userdata-dropped` decode loss.

#### 7.2.3 `ON_V5_MeshDoubleVertices`

The built-in `ON_V5_MeshDoubleVertices` class uses class UUID and item UUID
`17F24E75-21BE-4A7B-9F3D-7F85225247E3`. Its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`. Its userdata payload is the body of
the outer anonymous userdata child from section 7.2. The body contains an
anonymous major-1, minor-0 child:

```text
i32 single-precision vertex count
i32 double-precision vertex count
u32 single-precision vertex CRC
u32 double-precision vertex CRC
i32 serialized double-vertex count
serialized double-vertex count × ON_3dPoint
```

The writer attaches this item to a nonempty mesh only for archive version 50
when the single- and double-precision vertex arrays are synchronized. The
writer validates both counts and both CRCs before writing. The stored counts
and CRCs are producer-validity fields; the read-side transfer uses the actual
serialized array count and the owner mesh's float vertices. If that count
equals the owner vertex count and every serialized f64 coordinate casts
exactly to the corresponding f32 coordinate, the double array becomes the
mesh's source-precision vertex array. Otherwise the double array is discarded
and the float array remains authoritative. A mesh double array already loaded
from the class-data payload has precedence over this userdata item. Later
minor bytes are skipped at the anonymous boundary.

CADIR maps an accepted array to tessellation vertices before unit scaling. A
rejected or malformed recognized item retains the mesh and its float vertices
and emits `container.redundant-field-repaired`.

#### 7.2.4 `ON_V4V5_MeshNgonUserData`

The built-in `ON_V4V5_MeshNgonUserData` class uses class UUID and item UUID
`31F55AA3-71FB-49F5-A975-757584D937FF`. Its application UUID is
`17B3ECDA-17BA-4E45-9E67-A2B8D9BE520D`. Its userdata payload is the body of
the outer anonymous userdata child from section 7.2. The body contains an
anonymous major-1 child. Writers emit minor 1:

```text
i32 n-gon record count
repeat n-gon record count:
  i32 corner count N
  if N > 0:
    N × i32 mesh vertex index
    N × i32 mesh face index
minor >= 1: i32 mesh face count, i32 mesh vertex count
```

Each positive writer corner count is at least 3 and at most 100000. The vertex
indices identify the ordered n-gon boundary. The face indices identify the
mesh faces that fill that boundary; unused trailing positions are `-1` and
follow only other `-1` positions. A nonpositive corner count contributes no
record. The reader skips later bytes through the anonymous child boundary.

The list is a V4/V5 compatibility carrier. `ON_Mesh::V4V5_ModifyNgonList`
attaches it. The critical archive filter permits this `ON_opennurbs4_id`
userdata in archive versions 4 and 5; an empty userdata filter serializes all
attached userdata, so an explicitly attached list can also remain in a later
archive band. Archive version 60 and later writes the separate major-3 mesh
n-gon chunk from section 19.3 when modern n-gons exist. The reader accepts the
legacy list in every archive band supported by the class wrapper.

When both stored validation counts are zero, the reader validates every
vertex index against the owning mesh and every face index against the owning
face table, allowing only a trailing `-1` face suffix. When either count is
nonzero, the list is admitted only when both counts equal the owning mesh
counts; the class reader does not repeat index validation in this branch. A
count mismatch or failed old-form index validation discards the list. The
serialized record count is not the admitted list count when a nonpositive
corner count is skipped.

CADIR decision: neutral tessellation carries the mesh faces and their derived
triangles but has no n-gon grouping field. A valid legacy list therefore
contributes its admitted record count to the existing
`mesh.ngon-grouping-dropped` loss; it does not alter the face triangles. An
invalid or malformed list contributes no grouping count and leaves the mesh
admissible.

#### 7.2.5 `ON_V5_BrepRegionTopologyUserData`

The built-in `ON_V5_BrepRegionTopologyUserData` class uses class UUID and item
UUID `7FE23D63-E536-43F1-98E2-C807A2625AFF`. Its application UUID is
`17B3ECDA-17BA-4E45-9E67-A2B8D9BE520D`. Its userdata payload is one anonymous
major-1, minor-0 `ON_BrepRegionTopology` chunk:

```text
anonymous version 1.0
  face-side array
  region array
```

Each array is an anonymous major-1, minor-0 chunk containing an `i32` count.
For archive versions below 60, each element is a raw anonymous major-1,
minor-0 chunk. For archive version 60 and later, each element is a polymorphic
object wrapper whose class-data payload contains the corresponding anonymous
major-1, minor-0 face-side or region record.

A face-side record is an anonymous major-1, minor-0 chunk containing:

```text
i32 face-side index
i32 region index
i32 face index
i32 surface-normal direction
```

A region record is an anonymous major-1, minor-0 chunk containing:

```text
i32 region index
i32 region type
i32 face-side count
face-side count × i32 face-side index
ON_BoundingBox
```

The bounding box contains the minimum and maximum points as six `f64` values.
Region type `0` is the infinite region and type `1` is a bounded region. The
region topology has exactly `2 * face_count` face sides. Side positions `2*f`
and `2*f+1` identify face `f` and carry directions `+1` and `-1`. There is
exactly one infinite region; region membership is reciprocal, and a face side
is listed at most once in a region. A side with region index `-1` is unassigned.

An `ON_Brep` writer emits packed Brep version 3.2 for archive versions 4 and
5. For archive version 50, it temporarily attaches this userdata item when a
region topology exists, the Brep has at least one face, and the topology has
exactly twice as many face sides as faces. The item is deleted after writing.
Archive version 40 does not automatically attach this carrier. A later
archive can contain the item when it remains attached and userdata is
serialized; the array element form then follows the containing archive band.

When reading, the userdata reader creates the region topology with the owning
Brep. `DeleteAfterRead` installs it only when the Brep has no region topology
already loaded. A loaded inline topology therefore takes precedence; a V5
class-userdata topology supplies the same region fields when no inline value
exists.

CADIR decision: a recognized valid item populates the Brep's existing region
face-side and region carriers. It does not create a second neutral topology
representation. A structurally unreadable or semantically invalid optional
item is discarded, the Brep remains admissible, and the decode report emits
`container.redundant-field-repaired` with the diagnostic cause. A checksum
mismatch follows the recoverable warning policy in §4.1.

#### 7.2.6 `ON_SubDMeshProxyUserData`

The built-in `ON_SubDMeshProxyUserData` class uses class UUID and item UUID
`2868B9CD-28AE-4EA7-8073-BD390B3E97C8`. Its application UUID is
`7B0B585D-7A31-45D0-925E-BDD7DDF3E4E3` (`ON_opennurbs6_id`). The class-userdata
wrapper is the major-2, minor-2 form from this section. Its payload is an
anonymous major-1 chunk with a positive minor; the writer emits minor 1:

```text
anonymous version 1.1
bool proxy data is valid
if false: no further proxy fields
if true:
  ON_SubD compatibility payload
  i32 parent mesh face count
  i32 parent mesh vertex count
  anonymous SHA-1 version 1.0: 20 raw digest bytes
  anonymous SHA-1 version 1.0: 20 raw digest bytes
```

The embedded compatibility payload is the `ON_SubD` payload from section 17:
one `u8 has_subdimple` followed, when the value is 1, by one bounded
anonymous SubDimple chunk. The `has_subdimple` byte and that nested chunk
boundary delimit the embedded SubD; no additional length field surrounds it.
Each SHA-1 record contains `i32 major = 1`, `i32 minor = 0`, and 20 digest
bytes. The first digest is over the parent `ON_Mesh::m_F` array and the second
is over its `m_V` array. The source arrays are raw memory: each mesh face is
four `i32` values (16 bytes), and each float vertex is three `f32` values (12
bytes), in native byte order. A V5 double-precision vertex userdata item does
not replace `m_V` and does not change these proxy hashes.

The runtime-object conversion gate is archive version 60: below 60, a runtime
`ON_SubD` is written as an `ON_Mesh` control-net proxy; at 60 and later, the
direct `ON_SubD` class is written. V3 through V5 object writers also emit the
section 7.2.6 proxy item. V1 and V2 still write the mesh proxy class, but their
object writers suppress all class-userdata items, so they do not contain this
proxy item. The reader accepts a positive proxy-payload minor. A proxy item is
valid only when its userdata transform is the identity, its embedded SubD
pointer exists, its stored face count is positive, its stored vertex count is
greater than 2, its stored hashes are not empty-content hashes, and the stored
counts and hashes equal the current parent mesh arrays. `SubDFromMeshProxy`
then returns the embedded SubD and removes the proxy userdata item.

CADIR decision: a valid proxy on a top-level `ON_Mesh` transfers the embedded
level-zero SubD surface and suppresses the proxy mesh tessellation. A proxy on
a nested display or history mesh is not promoted because that mesh is a cache
carrier, not the owning runtime object. A false validity flag, a nonidentity
userdata transform, a count or hash mismatch, a malformed payload, or an
embedded payload without a neutral control cage leaves the parent mesh as the
admitted tessellation and does not create a second SubD entity.

#### 7.2.7 `ON_OBSOLETE_IDefAlternativePathUserData`

The built-in `ON_OBSOLETE_IDefAlternativePathUserData` class uses class and
item UUID `F42D9671-21EB-4692-9B9A-BC3507FF28F5`. Its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4` (`ON_opennurbs5_id`). It is a V4/V5
linked-instance-definition compatibility carrier. Its payload is the body of
the outer anonymous userdata child from this section. The body contains one
anonymous major-1 child:

```text
anonymous version 1.0
UTF-16 alternate path
bool alternate path is relative
```

The class reader requires major version 1, reads the UTF-16 path and Boolean,
and skips later bytes at the anonymous boundary. The path is trimmed at both
ends before it is applied. An empty trimmed path has no effect.

For a V5 instance definition, the class-data path is initially the full path.
When the class-data relative-path Boolean is true, that path occupies the
relative slot instead and the full slot is empty. A linked type with an empty
class-data path is converted to static before class userdata is applied; this
carrier cannot restore the linked type. On a linked definition, a relative
carrier fills the relative slot only when that slot is empty. A full-path
carrier fills the full slot only when that slot is empty and preserves the
existing relative path and content hash. The same slot rule applies to a
structured file reference when this carrier is present alongside one.

CADIR stores both path strings in the definition's external-reference record.
For the legacy V5 path slots, the class-data relative Boolean and a
successfully applied relative carrier set `relative_path_preferred`; a full
carrier does not clear an existing relative path or preference. A structured
V5 file-reference has no source preference bit, so its paths transfer with
`relative_path_preferred = false`; the carrier does not change that bit. A
recognized, well-framed carrier whose bounded payload is malformed is
discarded and the linked definition remains admitted.

#### 7.2.8 Obsolete layer-settings userdata

`ON_OBSOLETE_IDefLayerSettingsUserData` uses class and item UUID
`11EE2C1F-F90D-4C6A-A7CD-EC8532E1E32D`. `ON_OBSOLETE_LayerSettingsUserData`
uses class and item UUID `BFB63C09-4BC7-4727-89BB-7CC754118200`. Both use the
OpenNURBS 5 application UUID
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`.

Both classes inherit `ON_Internal_ObsoleteUserData`. Its reader requires one
anonymous child at the class-userdata payload boundary and skips the child
without reading fields. The two derived classes add no payload members. Their
archive policy is false and their delete-after-read policy is true: they are
V5 compatibility records that are never written by the current producer and
are discarded immediately after a successful read. The layer writer's current
per-viewport state is a separate `ON__LayerExtensions` userdata item.

CADIR assigns no neutral field to either obsolete class. A well-framed
class-userdata item is consumed and discarded; the owning layer or instance
definition remains admitted and its typed state is unchanged. The decoder does
not interpret the obsolete child bytes.

#### 7.2.9 `ON_OBSOLETE_CCustomMeshUserData`

`ON_OBSOLETE_CCustomMeshUserData` uses class and item UUID
`69F27695-3011-4FBA-82C1-E529F25B5FD9`. Its constructor leaves the application
UUID nil. It inherits `ON_UserData::Archive() == false` and has no `Write`
override, so the current producer does not emit this class. It also inherits
the default `DeleteAfterRead() == false`; the object-attribute reader performs
the compatibility conversion and deletes the temporary userdata explicitly.

The class-userdata header and outer anonymous child use the framing in section
7.2. The outer anonymous child is the class payload boundary. Unlike userdata
classes whose readers open another child, this class reads its fields directly
from that outer anonymous body:

```text
i32 legacy value                         ignored
bool custom mesh settings are in use
direct custom render-mesh body
```

The direct custom render-mesh body is the `ON_MeshParameters` grammar defined
for the settings-attributes record. Its packed major version is `1`; the
reader consumes the fields gated by its minor version, including the version-
1.5 `ON_SubDDisplayParameters` child, and leaves later bytes to the outer
anonymous boundary. The Boolean uses the userdata header's writer-version
strictness rule from section 4.2.

`ReadObjectUserDataAnonymousChunk` consumes the outer anonymous header before
calling this class's `Read`. After a successful read of the attributes
userdata stream, the object reader calls `SetCustomSettingsEnabled` with the
legacy in-use Boolean and then `SetCustomRenderMeshParameters`. That setter
copies the mesh parameters, forces custom settings true, and forces compute
curvature false. The first matching class/item UUID owns the conversion.

CADIR stores the converted wire fields in the owning native object
presentation's optional `custom_render_mesh` record. It does not create a
second geometry or userdata identity. A malformed recognized carrier is
discarded at its bounded payload, the object attributes remain admitted, and
the decoder records the bounded diagnostic; a later duplicate does not replace
the first matching item.

#### 7.2.10 `ON_PerObjectMeshParameters`

`ON_PerObjectMeshParameters` is an archived object-attributes userdata class.
Its class and item UUID are both
`B5628CA9-82C4-4CAE-9883-487B3E4AB28B`. Its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4` (`ON_opennurbs5_id`). It is attached to
the `ON_3dmObjectAttributes` owner, not to the object's geometry class.

The generic userdata payload child from section 7.2 contains one class-owned
anonymous long chunk. That class-owned chunk has an ordinary anonymous chunk
version followed by one bounded anonymous long child:

```text
anonymous long chunk
  i32 major = 1
  i32 minor (writer emits 0)
  anonymous long child, positive declared length
    packed ON_MeshParameters version 1.5
```

The `ON_MeshParameters` body uses the grammar in section 20.4. Its fields are
bounded by the nested child; the mesh reader consumes the known minor-gated
prefix and skips its suffix at that child boundary. The class-owned outer
chunk and the generic payload child likewise skip their direct suffixes at
their own boundaries. The class-owned reader requires outer major `1` and does
not use the outer minor.

After reading the nested mesh parameters, the class forces `custom_settings`
to true and `compute_curvature` to false. The parsed
`custom_settings_enabled` value remains effective. CADIR stores the resulting
wire fields in the owning native object presentation's optional
`custom_render_mesh` record and does not create another geometry or userdata
identity. A malformed recognized class payload is discarded at its bounded
userdata item, the object attributes remain admitted, and the decoder records
the bounded diagnostic. Duplicate selection follows section 7.2.

#### 7.2.11 `ON_AnnotationTextFormula`

`ON_AnnotationTextFormula` is runtime userdata on a legacy V5 annotation. Its
class and item UUID are both
`699FCC42-62D4-488C-9109-F1B7A37CE926`, and its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4` (`ON_opennurbs5_id`). The class does
not archive: it inherits `ON_UserData::Archive() == false` and supplies no
`Write` or `Read` override. `ON_OBSOLETE_V5_Annotation::SetTextFormula` may
attach it for runtime access, but the generic userdata writer excludes it
because `WriteToArchive` is false. No class-userdata wrapper or payload exists
for this UUID.

The formula itself is not userdata bytes. In the legacy common annotation
chunk, the minor-2 field in section 18.3 is the direct UTF-16 text formula.
Reading and writing that field updates the runtime userdata through
`SetTextFormula`; CADIR uses the existing annotation text mapping and creates
no separate userdata record.

#### 7.2.12 `ON_DisplacementUserData`

`ON_DisplacementUserData` uses class UUID
`B8C04604-B4EF-43B7-8C26-1AFB8F1C54EB`, item UUID
`8224A7C4-5590-4AC4-A32C-DE85DC2FFDAE`, and application UUID
`F293DE5C-D1FF-467A-9BD1-CAC8EC4B2E6B`. It is attached to the
`ON_3dmObjectAttributes` mesh-modifier owner. The model writer collects each
mesh-modifier userdata item from that owner, so the item remains in the
object-attributes userdata stream even though its XML is combined with the
other mesh-modifier nodes when the owner is queried.

The payload is the body of the anonymous userdata child from section 7.2:

```text
i32 XML userdata version
if version = 1: UTF-16 XML string
if version = 2: i32 UTF-8 byte count; raw UTF-8 XML bytes
```

The writer emits version 2. The reader accepts versions 1 and 2 and skips
remaining bytes at the anonymous payload boundary. Version 1 uses the archive
UTF-16 string grammar from section 7.3. Version 2 uses a byte count and does
not require an archive string terminator.

The XML document has root `xml` and a direct child named
`new-displacement-object-data`. Parameter and property names are matched
case-insensitively. A parameter with no `type` property is absent to the
parameter reader. Unknown child elements are ignored. Missing parameters use
the class getter defaults below; a nil UUID is no texture:

| XML child | Type | Meaning | Missing value |
| --- | --- | --- | ---: |
| `on` | bool | Enables displacement | `false` |
| `texture` | UUID | Texture used to compute displacement | nil UUID |
| `channel` | int | Texture mapping channel | `0` |
| `black-point` | double | Displacement amount at texture black | `0.0` |
| `white-point` | double | Displacement amount at texture white | `1.0` |
| `sweep-pitch` | int | Initial subdivision density; lower values produce higher resolution | `1000` |
| `refine-steps` | int | Number of refinement passes | `1` |
| `refine-sensitivity` | double | Contrast sensitivity used to split edges during refinement | `0.5` |
| `face-count-limit-enabled` | bool | Enables post-process face reduction | `false` |
| `face-count-limit` | int | Target face count for that reduction | `10000` |
| `post-weld-angle` | double | Maximum adjacent-face normal angle welded together, in degrees | `40.0` |
| `mesh-memory-limit` | int | Displacement mesh memory limit, in megabytes | `512` |
| `fairing-enabled` | bool | Enables fairing | `false` |
| `fairing-amount` | int | Number of fairing steps | `4` |
| `sub-object-count` | int | Serialized sub-object count parameter | absent |
| `sweep-res-formula` | int | Sweep-resolution formula: `0` default, `1` absolute-tolerance-dependent | archive-dependent |

The `sub-object-count` parameter does not delimit the sub-item sequence. The
reader enumerates every direct `sub` child in document order. Each `sub` child
overrides the top-level displacement parameters for one polysurface or SubD
face:

```text
sub-index       int       component face index; >= 0 selects the face
sub-on          bool      displacement override
sub-texture     UUID      texture override; nil means no texture
sub-channel     int       mapping-channel override
sub-black-point double    black-point override
sub-white-point double    white-point override
```

Missing sub-item values are `-1`, `false`, nil UUID, `0`, `0.0`, and `1.0` in
that order. For archive versions below 60, a missing `sweep-res-formula` is
materialized as `1` (`AbsoluteToleranceDependent`) by the userdata reader. For
archive version 60 and later, the missing value is the enum default `0`.
The class's public enum defines no other formula values; an unrecognized
serialized integer remains an integer field.

CADIR stores the recognized item under the owning object presentation's
`mesh_modifiers.displacement` native value. It does not create a mesh or a
second object identity. The first serialized matching class/item/application
triple owns the typed value. A malformed recognized payload leaves the object
attributes and geometry admitted, omits this native value, and retains the
bounded userdata record for opaque fidelity handling.

#### 7.2.13 `ON_EdgeSofteningUserData`

`ON_EdgeSofteningUserData` uses class UUID
`CB5EB395-BF1B-4112-8F2F-F728FCE8169C`, item UUID
`8CBE6160-5CBD-4B4D-8CD2-7CE0A7C8C2D8`, and application UUID
`F293DE5C-D1FF-467A-9BD1-CAC8EC4B2E6B`. It is attached to the
`ON_3dmObjectAttributes` mesh-modifier owner and uses the same XML userdata
payload framing as section 7.2.12. The writer emits XML userdata version 2;
the reader accepts versions 1 and 2 and skips remaining bytes at the bounded
userdata payload boundary.

The XML document has root `xml` and a direct child named
`edge-softening-object-data`. Parameter and property names are matched
case-insensitively. A parameter with no `type` property is absent to the
parameter reader. Unknown child elements are ignored. Missing parameters use
the class getter defaults below:

| XML child | Type | Meaning | Missing value |
| --- | --- | --- | ---: |
| `on` | bool | Enables edge softening | `false` |
| `softening` | double | Softening radius | `0.1` |
| `chamfer` | bool | Chamfers softened edges | `false` |
| `unweld` | bool | Leaves softened edges faceted (`Faceted`) | `false` |
| `force-softening` | bool | Softens edges despite an excessive radius | `false` |
| `edge-threshold` | double | Adjacent-face angle threshold, in degrees | `5.0` |

CADIR stores the recognized item under the owning object presentation's
`mesh_modifiers.edge_softening` native value. The native `faceted` field is
the typed form of the XML `unweld` parameter. It does not create geometry or
a second object identity. The first serialized matching
class/item/application triple owns the typed value. A malformed recognized
payload leaves the object attributes and geometry admitted, omits this native
value, and retains the bounded userdata record for opaque fidelity handling.

#### 7.2.14 `ON_ThickeningUserData`

`ON_ThickeningUserData` uses class UUID
`AA03D9C3-4CCF-4431-A06E-25F38CF3913F`, item UUID
`6AA7CCC3-2721-410F-AA56-E8AB4F3ECE67`, and application UUID
`F293DE5C-D1FF-467A-9BD1-CAC8EC4B2E6B`. It is attached to the
`ON_3dmObjectAttributes` mesh-modifier owner and uses the same XML userdata
payload framing as section 7.2.12. The writer emits XML userdata version 2;
the reader accepts versions 1 and 2 and skips remaining bytes at the bounded
userdata payload boundary.

The XML document has root `xml` and a direct child named
`thickening-object-data`. Parameter and property names are matched
case-insensitively. A parameter with no `type` property is absent to the
parameter reader. Unknown child elements are ignored. Missing parameters use
the class getter defaults below:

| XML child | Type | Meaning | Missing value |
| --- | --- | --- | ---: |
| `on` | bool | Enables thickening | `false` |
| `solid` | bool | Adds side walls to make an open mesh solid | `true` |
| `both-sides` | bool | Thickens on both sides of the original surface | `false` |
| `offset-only` | bool | Produces only the offset surface | `false` |
| `distance` | double | Thickening distance | `0.1` |

CADIR stores the recognized item under the owning object presentation's
`mesh_modifiers.thickening` native value. It does not create geometry or a
second object identity. The first serialized matching class/item/application
triple owns the typed value. A malformed recognized payload leaves the object
attributes and geometry admitted, omits this native value, and retains the
bounded userdata record for opaque fidelity handling.

#### 7.2.15 `ON_CurvePipingUserData`

`ON_CurvePipingUserData` uses class UUID
`2D5AFEA9-F458-4079-992F-C2D405D9383B`, item UUID
`2B1A758E-7CB1-45AB-A5BF-DFCD6D3D136D`, and application UUID
`F293DE5C-D1FF-467A-9BD1-CAC8EC4B2E6B`. It is attached to the
`ON_3dmObjectAttributes` mesh-modifier owner and uses the same XML userdata
payload framing as section 7.2.12. The writer emits XML userdata version 2;
the reader accepts versions 1 and 2 and skips remaining bytes at the bounded
userdata payload boundary.

The XML document has root `xml` and a direct child named
`curve-piping-object-data`. Parameter and property names are matched
case-insensitively. A parameter with no `type` property is absent to the
parameter reader. Unknown child elements are ignored. Missing parameters use
the source getter defaults below:

| XML child | Type | Meaning | Missing value |
| --- | --- | --- | ---: |
| `on` | bool | Enables curve piping | `false` |
| `radius` | double | Pipe radius | `1.0` |
| `segments` | int | Number of pipe segments | `16` |
| `weld` | bool | Welds the pipe; native `faceted` is its inverse | `true` |
| `accuracy` | int | Pipe accuracy setting | `50` |
| `cap-type` | string | Cap mode: `none`, `flat`, `box`, or `dome` | `none` |

The source default writer emits `cap-type` as `dome`. The public getter maps
an absent or unrecognized cap string to `none`; the typed reader applies the
same mapping. CADIR stores the recognized item under the owning object
presentation's `mesh_modifiers.curve_piping` native value, with `faceted` as
the inverse of XML `weld` and `cap_type` as the canonical lower-case cap name.
It does not create geometry or a second object identity. The first serialized
matching class/item/application triple owns the typed value. A malformed
recognized payload leaves the object attributes and geometry admitted, omits
this native value, and retains the bounded userdata record for opaque fidelity
handling.

#### 7.2.16 `ON_ShutLiningUserData`

`ON_ShutLiningUserData` uses class UUID
`429DCD06-5643-4254-BDE8-C0557F8FD083`, item UUID
`07506EBE-1D69-4345-9F0D-2B9AA1906EEF`, and application UUID
`F293DE5C-D1FF-467A-9BD1-CAC8EC4B2E6B`. It is attached to the
`ON_3dmObjectAttributes` mesh-modifier owner and uses the same XML userdata
payload framing as section 7.2.12. The writer emits XML userdata version 2;
the reader accepts versions 1 and 2 and skips remaining bytes at the bounded
userdata payload boundary.

The XML document has root `xml` and a direct child named
`shut-lining-object-data`. The four modifier parameters use the typed XML
parameter grammar. A parameter with no `type` property is absent to this
reader. Unknown child elements are ignored. Missing parameters use the source
getter defaults below:

| XML child | Type | Meaning | Missing value |
| --- | --- | --- | ---: |
| `on` | bool | Enables shut lining | `false` |
| `faceted` | bool | Uses faceted shut-line processing | `false` |
| `auto-update` | bool | Updates shut lining automatically | `false` |
| `force-update` | bool | Forces shut-line updates | `false` |

The default userdata initializer writes all four typed fields with their false
values. An otherwise empty `ON_ShutLining` modifier may serialize only the
`shut-lining-object-data` root; omitted fields read as the same defaults.

The direct `curve` children are an ordered sequence. Each curve reads its
fields from the child node's default property, so these field elements do not
need a `type` attribute. Curve field names are matched case-insensitively.
Missing fields use these source getter defaults:

| Curve child | Type | Meaning | Missing value |
| --- | --- | --- | ---: |
| `uuid` | UUID | Source curve identity; nil UUID means no identity | nil UUID |
| `enabled` | bool | Enables this curve's shut line | `false` |
| `radius` | double | Shut-line radius | `1.0` |
| `profile` | int | Shut-line profile | `0` |
| `pull` | bool | Pulls the curve to the surface | `false` |
| `is-bump` | bool | Makes the curve a bump instead of a dent | `false` |

The modifier writer's `AddCurve` path first leaves an empty `curve` child in
the base node and `AddChildXML` then appends one serialized copy for every
managed curve. Consequently, a write with N managed curves contains N empty
curve elements followed by N populated copies. The reader retains every direct
curve element, including empty entries, in serialized order.

CADIR stores the recognized item under the owning object presentation's
`mesh_modifiers.shut_lining` native value, including the scalar fields and
ordered curve records. A nil curve UUID becomes null. The item does not create
geometry or a second object identity. The first serialized matching
class/item/application triple owns the typed value. A malformed recognized
payload leaves the object attributes and geometry admitted, omits this native
value, and retains the bounded userdata record for opaque fidelity handling.

#### 7.2.17 `ON_PhysicallyBasedMaterialUserData`

`ON_PhysicallyBasedMaterialUserData` uses class UUID and item UUID
`5694E1AC-40E6-44F4-9CA9-3B6D0E8C4440` and application UUID
`7B0B585D-7A31-45D0-925E-BDD7DDF3E4E3`. It is attached to an `ON_Material`
class wrapper. The generic userdata payload is an outer anonymous child whose
body contains the class-owned inner anonymous chunk.

The inner chunk has major version `1` and minor version `1` or `2`. Its body
has this bounded prefix:

```text
f32 base_color[4]                         // ON_4fColor RGBA
i32 brdf                                  // 0 GGX, 1 Ward
f64 subsurface
f32 subsurface_scattering_color[4]        // ON_4fColor RGBA
f64 subsurface_scattering_radius
f64 metallic
f64 specular
f64 specular_tint
f64 roughness
f64 anisotropic
f64 anisotropic_rotation
f64 sheen
f64 sheen_tint
f64 clearcoat
f64 clearcoat_roughness
f64 opacity_ior
f64 opacity
f64 opacity_roughness
f32 emission[4]                           // ON_4fColor RGBA
if minor >= 2:
  f64 alpha
```

`ON_4fColor` components are little-endian binary32 values in red, green,
blue, alpha order. Its alpha is opacity: `1.0` is opaque. The version-1
reader leaves `alpha` at its source default `1.0`; the version-2 reader
reads the final double. Both readers skip bytes remaining before the inner
chunk boundary.

The default parameters are base color unset, GGX, subsurface `0`, white
subsurface-scattering color, subsurface-scattering radius `0`, metallic
`0`, specular `0.5`, specular tint `0`, roughness `1`, anisotropic `0`,
anisotropic rotation `0`, sheen `0`, sheen tint `0`, clearcoat `0`,
clearcoat roughness `0`, opacity IOR `1.52`, opacity `1`, opacity roughness
`0`, black emission, and alpha `1`.
An unset base color is four `-1.234321e+38` binary32 components; the decoder
preserves these color components as written.

The first serialized userdata item with the class and item UUID above whose
application UUID is absent or is the OpenNURBS 6 application UUID owns the
typed value. A different present application UUID is not recognized. CADIR
stores the decoded fields under
`native.rhino.materials[].physically_based`, including the inner minor
version as `version`. A malformed recognized payload leaves the material
record admitted, omits `physically_based`, and retains the bounded userdata
record for opaque fidelity handling.

#### 7.2.18 `ON_RdkUserData`

`ON_RdkUserData` uses class UUID and item UUID
`AFA82772-1525-43DD-A63C-C84AC5806911` and
`B63ED079-CF67-416C-800D-22023AE1BE21`, with application UUID
`16592D58-4A2F-401D-BF5E-3B87741C1B1B`. It can be attached to any
`ON_Object`. Its `Read` and `Write` methods delegate to
`ON_XMLUserData` and add no bytes to the class-owned payload.

Inside the generic class-userdata anonymous child, the class-owned payload is:

```text
i32 XML userdata version
if version == 1:
  archive UTF-16 XML string
else if version <= 2:
  i32 UTF-8 byte count including the terminator
  byte count raw UTF-8 bytes, including a terminating 0x00
```

The writer emits XML userdata version `2`. The reader rejects a version above
`2`; version `1` selects the legacy archive UTF-16 string, and every other
admitted version selects the UTF-8 branch. The UTF-8 length is the number of
bytes written by the conversion, including the terminating NUL. The XML root
serialized by `ON_XMLRootNode` is `<xml>`. The RDK convention names its direct
data child `render-content-manager-data`; the child elements and properties
below it are callback-owned XML and have no common field grammar in this
carrier.

The writer archives the userdata only when the XML root has at least one child.
On a non-material parent, the source reader keeps the userdata attached. On an
`ON_Material` parent, the source reader treats RDK material userdata as
obsolete: it sets the material plug-in ID to the universal render engine,
reads `render-content-manager-data/material/@instance-id`, transfers that UUID
to the material RDK instance-ID member, and deletes the userdata.

The V4/V5 compatibility writer uses the same class and item UUIDs for an
internal `ON_RdkMaterialInstanceIdObsoleteUserData` carrier. It is written
when the material RDK instance-ID is non-nil and the archive version is at
most 50. Its class-owned payload is a direct, non-terminated XML form:

```text
i32 version = 2
i32 byte_count                    // 0 through 1024
byte_count raw UTF-8 XML bytes    // no terminating 0x00
```

The XML root is `xml`, its direct data child is
`render-content-manager-data`, and that element has a direct `material` child
whose `instance-id` attribute is the UUID transferred to the material. The
reader accepts a bounded suffix after the counted bytes. The V6 material
writer stores the same UUID in its class-data minor-5 UUID field and does not
write this compatibility carrier. The universal render-engine UUID is
`99999999-9999-9999-9999-999999999999`.

The direct, non-terminated form is the compatibility carrier. A generic
`ON_RdkUserData` payload uses the version-2 UTF-8 form above and includes its
terminating `0x00`; its callback-owned XML is not assigned a neutral field
grammar by CADIR. For a material, the Rhino codec transfers only the
compatibility form to `rdk_instance_uuid`, gives it precedence over the
class-data UUID, and sets `plugin_uuid` to the universal render-engine UUID.
This is a CADIR decision that keeps callback-owned RDK XML opaque while
preserving the source-defined material compatibility field.

CADIR does not promote callback-owned RDK XML to typed native fields. The Rhino
codec retains the complete containing object record for opaque source fidelity.
For materials, the typed `rdk_instance_uuid` field comes from the material
class-data record unless the compatibility carrier above supplies it.

#### 7.2.19 `MappingCRCCache`

`MappingCRCCache` uses class UUID and item UUID
`5A4971F3-AA73-493C-A385-2F7EB4288989`. Its application UUID is
`ON_opennurbs_id`; the current OpenNURBS source defines that value as
`50EDE5C9-1487-4B4C-B3AA-6840B460E3CF`. The userdata is attached to the
custom mapping primitive by `ON_TextureMapping::SetCustomMappingPrimitive`,
not to the `ON_TextureMapping` record. `ON_TextureMapping::Write` writes the
primitive as a complete polymorphic object after the mapping name, so the
cache is a class-userdata child of that primitive's class wrapper.

Inside the generic class-userdata anonymous child, the class-owned payload is:

```text
i32 version = 1
i32 mapping_crc
```

The anonymous child has the normal archive-version chunk length and checksum.
The writer emits version `1`. The reader requires version `1` and reads the
signed `i32` checksum. The cache's default member value is `-1` before a
primitive checksum is assigned.

`mapping_crc` is the primitive checksum used by `ON_TextureMapping::MappingCRC`;
it is not the file-chunk checksum and it is not the aggregate texture-mapping
CRC. The primitive checksum starts with state `0x12341234`. For an `ON_Mesh`,
the source applies `ON_Mesh::DataCRC` to that state, then includes the texture
coordinate array when present, then includes the bytes of
`ON_3dPoint::UnsetPoint`. For an `ON_Brep` or `ON_Surface`, it applies that
object's `DataCRC` to the same state. Other primitive classes leave the state
at `0x12341234`. `SetCustomMappingPrimitive` stores the resulting value in the
cache. If `MappingCRC` reads a primitive without a cache, it computes the same
value, attaches a cache, and uses that value.

The Rhino codec parses the complete primitive class wrapper and exposes its
class UUID in `native.rhino.texture_mappings[].primitive_class_uuid`; it skips
the nested `MappingCRCCache` userdata and does not add `mapping_crc` to the
native mapping record. CADIR treats this value as recomputable source cache
state, not an independently authored mapping property.

#### 7.2.20 `CTtMappingMeshInfoUserData` and `CTtRenderMeshInfoUserData`

The derived mesh-correspondence carriers use these class and item UUIDs:

| userdata class | class UUID | item UUID | application UUID |
| --- | --- | --- | --- |
| `CTtMappingMeshInfoUserData` | `1706ADC5-52BF-4BE2-8402-4501EB2AE675` | `1706ADC5-52BF-4BE2-8402-4501EB2AE675` | `ON_opennurbs_id` (`50EDE5C9-1487-4B4C-B3AA-6840B460E3CF`) |
| `CTtRenderMeshInfoUserData` | `4960A046-8201-4F0F-8F22-FCB6F91C765D` | `4960A046-8201-4F0F-8F22-FCB6F91C765D` | `ON_opennurbs_id` (`50EDE5C9-1487-4B4C-B3AA-6840B460E3CF`) |

Each carrier is userdata on an `ON_Mesh`. Its class-owned payload is the
bounded anonymous child of the generic class-userdata wrapper. Both writers
emit version 1; both readers require version 1 and leave any remaining bytes
inside the anonymous child for its bounded end.

Both payloads start with the same geometry fingerprint:

```text
i32 topology CRC
5 × ON_3dPoint point weighted-average hash
5 × ON_3dPoint edge weighted-average hash
```

Each `ON_3dPoint` is three f64 values, so the fingerprint occupies 244 bytes.
The fingerprint matches only when the topology CRC is equal and all five
point-hash and all five edge-hash points, after the other mesh's transform,
are within the supplied distance tolerance. A transform updates the point and
edge hashes and does not change the topology CRC.

`CTtMappingMeshInfoUserData` appends this payload:

```text
i32 version = 1
geometry fingerprint
i32 face-source ID count
count × i32 source face ID
```

Entry `f` associates mapping-mesh face `f` with its source face ID. The reader
builds a reverse index only for nonnegative source IDs. Negative IDs remain in
the ordered face-source array and are not addressable through that index.
The mapping closest-point path queries this index with the source face ID
stored by the render-mesh carrier.

`CTtRenderMeshInfoUserData` appends this payload:

```text
i32 version = 1
geometry fingerprint
i32 source face ID
```

The default source face ID is `ON_UNSET_INT_INDEX` (`-2147483647`). With the
default zero fingerprint and an empty mapping face-source array, the mapping
class payload is 252 bytes before its anonymous-child checksum; its child
chunk declares 256 bytes. The render class payload has the same 252-byte
length and child-chunk size.

The closest-point mapper compares the mapping and render fingerprints with a
distance tolerance of `0.001`, then uses the render source face ID to select
the mapping-mesh faces. These carriers therefore describe recomputable mesh
correspondence state, not authored geometry, material, or texture-mapping
properties. The Rhino codec consumes their bounded class-userdata wrappers,
retains the containing source record, and does not create native cache fields
for either carrier.

### 7.3 Strings

UTF-8 strings use a fixed four-byte unsigned element count:

```text
u32 byte_count_including_NUL
byte_count_including_NUL raw bytes
```

Empty strings use count zero. Nonempty strings contain UTF-8 bytes followed by
`0x00`; the count is a byte count.

UTF-16 strings use:

```text
u32 code_unit_count_including_NUL
code_unit_count_including_NUL UTF-16LE code units
```

Surrogate pairs count as two code units. An empty string has count zero and no
code units. A nonempty string ends with a zero code unit. The archive `size_t`
destination type does not change either file count.

## 8. Properties, settings, units, and layers

### 8.1 Properties

Properties strings are UTF-16.

Revision history:

```text
packed version 1.0
UTF-16 created-by
8 × i32 UTC time: sec,min,hour,mday,mon,year,wday,yday
UTF-16 last-edited-by
8 × i32 UTC time in the same order
i32 revision count
```

Notes:

```text
packed version 1.0 or 1.1
i32 HTML flag
UTF-16 notes
i32 visible flag
i32 left
i32 top
i32 right
i32 bottom
bool locked                         version >= 1.1
```

Application:

```text
packed version
UTF-16 application name
UTF-16 application URL
UTF-16 application details
```

The writer-version property is a short value in
`TCODE_PROPERTIES_OPENNURBS_VERSION`. The preview records are bounded binary
payloads. The revision-history writer emits version `1.0`; its reader requires
major version 1 for the typed prefix, then the containing property record
boundary consumes the remainder. The notes writer emits version `1.1`; its
reader requires major version 1, reads `locked` at minor 1 and later, and then
ends at the containing boundary. The application writer emits version `1.0`;
its reader reads the three strings after the packed version without a major or
minor gate and then ends at the containing boundary. Later direct suffix bytes
are not typed fields.

These three payloads are direct prefixes of their length-bounded property
records. The packed version selects fields within the prefix; it does not create
a child boundary or assign a grammar to bytes after the prefix.

### 8.2 Units and tolerances

The units/tolerances structure begins with an ordinary `i32` structure version,
not a packed chunk version:

```
i32 structure version
i32 unit system
f64 absolute tolerance
f64 angle tolerance
f64 relative tolerance
i32 distance display mode                 version >= 101
i32 distance display precision            version >= 101
f64 meters per unit                       version >= 102
UTF-16 custom unit name                   version >= 102
```

The writer emits structure version 102. The reader accepts every structure
version from 100 through 199; fields introduced at versions 101 and 102 are
present when their gates are met, and later bytes remain within the containing
`TCODE_SETTINGS_UNITSANDTOLS` record boundary. Unit values are:

| Value | Unit               |
| ----: | ------------------ |
|     0 | none               |
|     1 | microns            |
|     2 | millimeters        |
|     3 | centimeters        |
|     4 | meters             |
|     5 | kilometers         |
|     6 | microinches        |
|     7 | mils               |
|     8 | inches             |
|     9 | feet               |
|    10 | miles              |
|    11 | custom             |
|    12 | angstroms          |
|    13 | nanometers         |
|    14 | decimeters         |
|    15 | dekameters         |
|    16 | hectometers        |
|    17 | megameters         |
|    18 | gigameters         |
|    19 | yards              |
|    20 | printer points     |
|    21 | printer picas      |
|    22 | nautical miles     |
|    23 | astronomical units |
|    24 | light years        |
|    25 | parsecs            |
|   255 | unset              |

Unit system `none` retains native coordinates at scale 1.0 and supplies no
millimetre binding. Unit system `unset` supplies no coordinate scale.

Unit values 23, 24, and 25 scale one stored unit to
`149597870000000`, `9460730472580800000`, and `30856775800000000000`
millimeters, respectively.

In V2 settings, the units body is direct data in its own long record. The
ordinary settings-record boundary, not structure version 100–199, consumes
later direct bytes.

The legacy V1 structure is:

```text
i32 version
i32 unit system
f64 absolute tolerance
f64 relative tolerance
f64 angle tolerance
```

V1 geometry is a flat sequence. The object reader dispatches these direct
record typecodes:

| Record | Typecode | Payload boundary |
| --- | ---: | --- |
| `TCODE_RH_POINT` | `0x00100001` | one point and optional attribute data in the same chunk |
| `TCODE_MESH_OBJECT` | `0x00100015` | one `TCODE_COMPRESSED_MESH_GEOMETRY` child and optional attribute data |
| `TCODE_LEGACY_SHL` | `0x00010003` | one legacy shell wrapper |
| `TCODE_LEGACY_FAC` | `0x00010004` | one legacy face wrapper |
| `TCODE_LEGACY_CRV` | `0x00010008` | one legacy curve wrapper |
| `TCODE_TEXT_BLOCK` | `0x00200004` | one V1 text annotation record |
| `TCODE_ANNOTATION_LEADER` | `0x00200005` | one V1 leader annotation record |
| `TCODE_LINEAR_DIMENSION` | `0x00200006` | one V1 linear-dimension record |
| `TCODE_ANGULAR_DIMENSION` | `0x00200007` | one V1 angular-dimension record |
| `TCODE_RADIAL_DIMENSION` | `0x00200008` | one V1 radial-dimension record |
| `TCODE_RHINOIO_OBJECT_NURBS_CURVE` | `0x00020008` | one pre-class NURBS curve record |
| `TCODE_RHINOIO_OBJECT_NURBS_SURFACE` | `0x00020009` | one pre-class NURBS surface record |
| `TCODE_RHINOIO_OBJECT_BREP` | `0x0002000b` | one pre-class NURBS Brep record |

The five annotation and three pre-class NURBS records are complete bounded
records. They are not split into legacy geometry children. A direct typecode
outside this dispatch is skipped as one complete chunk. The containing direct
chunk is the recovery boundary when a dispatched payload is malformed; a
partial child is not promoted to a typed record.

The annotation payloads start with an `i32` version:

```text
TCODE_TEXT_BLOCK, version 1 or 2:
  i32 type flag
  3 × 3 × f64 entity plane (origin, X axis, Y axis)
  i32 byte count, byte count bytes user text
  i32 flags
  i32 by-object flag
  i32 byte count, byte count bytes face name
  i32 face weight
  f64 text height
  if version == 1: 2 × f64 extra values

TCODE_ANNOTATION_LEADER, version 1:
  i32 type flag
  3 × 3 × f64 entity plane
  i32 flags
  i32 by-object flag
  i32 point count
  point count × 3 × f64 points

TCODE_LINEAR_DIMENSION, version 1:
  i32 annotation type
  3 × 3 × f64 entity plane
  11 × 3 × f64 definition points
  i32 byte count, byte count bytes user text
  i32 byte count, byte count bytes default text
  i32 user-positioned-text flag
  i32 flags
  i32 by-object flag

TCODE_ANGULAR_DIMENSION, version 1:
  i32 annotation type
  3 × 3 × f64 entity plane
  f64 angle
  f64 radius
  4 × f64 extension distances
  5 × 3 × f64 definition points
  i32 byte count, byte count bytes user text
  i32 byte count, byte count bytes default text
  i32 user-positioned-text flag
  i32 flags
  i32 by-object flag

TCODE_RADIAL_DIMENSION, version 1:
  i32 annotation type
  3 × 3 × f64 entity plane
  5 × 3 × f64 definition points
  i32 byte count, byte count bytes user text
  i32 byte count, byte count bytes default text
  i32 user-positioned-text flag
  i32 flags
  i32 by-object flag
```

The annotation byte-count strings are not NUL-terminated in the payload. Any
following V1 attribute or material chunks remain inside the containing direct
chunk and follow the object-specific fields.

Each pre-class NURBS curve or surface record contains one
`TCODE_RHINOIO_OBJECT_DATA` child. Its wire version is `100` or `101` after
clearing bit `0x100`. The curve child stores:

```text
i32 wire version
i32 dimension (at least 1)
i32 rational form (0 or 1)
i32 order (at least 2)
i32 control-point count (at least order)
i32 flag (0)
(order + control-point count - 2) × f64 knots
control-point count × (dimension + rational form) × f64 control values
```

The surface child stores the same wire version, dimension, rational form, and
flag, followed by U order, V order, U control-point count, and V control-point
count. It then stores the U knot vector, the V knot vector, and the U-major
control lattice:

```text
i32 wire version
i32 dimension (at least 1)
i32 rational form (0 or 1)
i32 U order (at least 2)
i32 V order (at least 2)
i32 U control-point count (at least U order)
i32 V control-point count (at least V order)
i32 flag (0)
(U order + U control-point count - 2) × f64 U knots
(V order + V control-point count - 2) × f64 V knots
U control-point count × V control-point count
  × (dimension + rational form) × f64 control values
```

The pre-class Brep record contains one `TCODE_RHINOIO_OBJECT_DATA` child with
wire version `100` or `101`, then the following arrays:

```text
i32 2D-curve count (at least 1)
2D-curve count × {
  i32 segment count (at least 1)
  segment count × TCODE_RHINOIO_OBJECT_NURBS_CURVE
}
i32 3D-curve count (at least 1)
3D-curve count × {
  i32 segment count (at least 1)
  segment count × TCODE_RHINOIO_OBJECT_NURBS_CURVE
}
i32 surface count (at least 1)
surface count × TCODE_RHINOIO_OBJECT_NURBS_SURFACE
i32 vertex count
vertex count × {
  i32 vertex index
  3 × f64 point
  i32 edge-index count, edge-index count × i32 edge indices
  f64 tolerance
}
i32 edge count
edge count × {
  i32 edge index
  i32 3D-curve index
  2 × f64 proxy domain
  2 × i32 vertex indices
  i32 trim-index count, trim-index count × i32 trim indices
  f64 tolerance
}
i32 trim count
trim count × {
  i32 trim index
  i32 2D-curve index
  2 × f64 proxy domain
  i32 edge index
  2 × i32 vertex indices
  i32 reversed-3D flag
  i32 trim type (1 boundary, 2 mated, 3 seam, 4 singular)
  i32 legacy isocurve flag
  i32 loop index
  2 × f64 tolerances
  2 × 3 × f64 old trim points
  f64 2D tolerance
  f64 3D tolerance
}
i32 loop count
loop count × {
  i32 loop index
  i32 trim-index count, trim-index count × i32 trim indices
  i32 loop type (1 outer, 2 inner, 3 slit)
  i32 face index
}
i32 face count
face count × {
  i32 face index
  i32 loop-index count, loop-index count × i32 loop indices
  i32 surface index
  i32 reversed flag
}
2 × 3 × f64 bounding-box points
```

In this direct pre-class Brep record, the vertex table and the vertex-index
fields on edges and trims identify shared vertices by their stored source
references. The legacy `TCODE_LEGACY_FAC` and `TCODE_LEGACY_SHL` records have
no vertex table; their trim-loop adjacency and seam or mate permutations define
the topology, and the reader derives vertex positions from incident edge
endpoints.

`TCODE_RH_POINT` begins with three `f64` coordinates. Attribute chunks follow
the coordinates inside the same bounded chunk.

`TCODE_LEGACY_CRV` (`0x00010008`) contains attribute chunks followed by one
`TCODE_LEGACY_CRVSTUFF` (`0x00010108`). Curve stuff stores dimension `u8`,
closure `u8`, segment count `u16`, two dimension-wide bounding-box points, and
that many `TCODE_LEGACY_SPL` children. Each spline contains
`TCODE_LEGACY_SPLSTUFF` (`0x00010109`):

```text
u8 dimension (2 or 3)
u8 rational form (0 nonrational, 1 euclidean, 2 homogeneous)
u8 order
u16 control-point count
u8 closure (0 open, 1 closed, 2 periodic)
u8 legacy form
2 × dimension × f64 bounding box
if order > 2: u8 clamped-end mask
compressed openNURBS knot vector
control-point count × (dimension + rational) × f64 control points
```

The clamped-end mask supplies omitted repeated end knots. The neutral knot
vector restores the two superfluous end knots. A rational control point stores
one weight after its coordinates.

`TCODE_LEGACY_FAC` (`0x00010004`) contains one
`TCODE_LEGACY_FACSTUFF` (`0x00010104`). Face stuff stores:

```text
i32 reversed-surface flag
i32 legacy face type
i32 (2 × boundary count + outer-boundary-present)
6 × f64 model-space bounding box
i32 seam-trim count
seam-trim count × u16 seam permutation
TCODE_LEGACY_SRF
boundary count × TCODE_LEGACY_BND
```

`TCODE_LEGACY_SRF` contains `TCODE_LEGACY_SRFSTUFF` (`0x00010107`).
Surface stuff stores dimension and form bytes, two degree bytes, two `u16`
pole-count deltas, two rational-form bytes, two closure bytes, two singularity
bytes, a dimension-wide bounding box, the U and V openNURBS knot vectors, and
the U-major control lattice. Each order is its stored degree plus one. Each
pole count is `order - 1 + stored delta`. Rational form 1 stores Euclidean
coordinates followed by weight. Rational form 2 stores homogeneous
coordinates followed by weight. The reader processes the U rational-form byte
then the V byte into one mode: zero leaves the current mode unchanged, and a
nonzero form replaces it. Therefore a nonzero V form is authoritative; when V
is zero, a nonzero U form is authoritative; when both are zero, the surface is
non-rational.

Each `TCODE_LEGACY_BND` contains `TCODE_LEGACY_BNDSTUFF` (`0x00010105`):

```text
i32 trim count
i32 boundary type (-1 slit, 0 outer, 1 inner)
4 × f64 parameter-space bounding box
trim count × TCODE_LEGACY_TRM
```

Each `TCODE_LEGACY_TRM` contains `TCODE_LEGACY_TRMSTUFF` (`0x00010106`):

```text
u8 edge/mate/seam flags
i32 edge-reversal flag
i32 legacy continuity
i32 legacy monotonicity
f64 model-space tolerance
f64 parameter-space tolerance
TCODE_LEGACY_CRV parameter-space curve
if edge-present: TCODE_LEGACY_CRV model-space edge curve
```

Flag bit 0 marks an explicit model-space edge curve. Bit 1 marks a seam on the
same face. Bit 2 marks a mated trim on another face. The face seam permutation
and shell permutation pair trim records. A pair shares one edge only when
exactly one trim stores a model-space edge curve; the curve-less trim aliases
that edge. If both trims store model-space edge curves, both records remain as
separate edges. If neither trim stores one, neither trim has a model-space edge
curve.

V1 Brep vertices are identified by trim-loop adjacency and the shared-edge
permutations. The endpoint coordinates do not merge separate topological
vertices. A vertex position is the arithmetic mean of its incident
model-space edge endpoints, and its tolerance is the maximum incident edge
tolerance.

`TCODE_LEGACY_SHL` (`0x00010003`) contains one
`TCODE_LEGACY_SHLSTUFF` (`0x00010103`):

```text
i32 outer-shell flag
i32 face count
6 × f64 model-space bounding box
i32 shared-trim count
shared-trim count × u16 shared-trim permutation
face count × TCODE_LEGACY_FAC
```

The shell permutation pairs mated trims on different faces. A paired trim that
does not store a model-space curve uses the edge curve stored by its mate.

`TCODE_MESH_OBJECT` (`0x00100015`) contains
`TCODE_COMPRESSED_MESH_GEOMETRY` (`0x00100017`). The geometry body stores four
`i32` values for point count, face count, normal presence, and texture-coordinate
presence; a six-`f64` bounding box; three `u16` quantized coordinates per
vertex; four `u16` indices per face when the point count is below 65535 and
four `i32` indices otherwise; optional three signed bytes per normal; and
optional two `u16` texture coordinates per vertex. Quantized coordinate 0 is
the box minimum and 65535 is the box maximum.

V1 legacy wrapper/stuff pairs share the wrapper's trailing CRC16. The final
stuff child ends at the wrapper end and does not add a second CRC16.

V2 uses the table and polymorphic class-record grammar in sections 7 through
17. All chunk values use four bytes. The object class wrapper contains the
class UUID, one class-data chunk, zero or more class-userdata chunks, and the
class-end short chunk. The class UUID selects the class payload grammar. A
class wrapper always uses that boundary, but `WriteObject` can select a
compatibility class before writing it. In V2, ordinary curves and surfaces are
written as NURBS classes, extrusions are written as Breps or surfaces, modern
annotations are converted to obsolete V2 annotation classes, text dots are
written as V2 text-dot classes, and SubD is written as a mesh proxy. The
class-data fields and version are therefore those of the selected serialized
class, not necessarily those of the runtime object or of a later archive that
stores that runtime class. V1 and V2 object writers do not write class userdata.
V2 changes the outer chunk width while the producer's compatibility conversion
selects the class identity and class-data grammar.

For standard units, the enum determines the scale. For custom units,
`meters-per-unit` and the custom name determine the scale and label.

### 8.3 Layer records

Layer version is packed `1.minor`. Current records use minor 15.

Base fields:

```
i32 obsolete mode
i32 archive layer index
i32 IGES level
i32 render-material referenced index
i32 obsolete model index
ON_Color layer color
i16 obsolete line style
i16 obsolete line style index
f64 obsolete thickness
f64 obsolete scale
UTF-16 layer name
```

Gated fields:

```
minor >= 1:  bool visible
minor >= 2:  linetype referenced index i32
minor >= 3:  plot color, plot weight f64
minor >= 4:  bool locked
minor >= 5:  layer UUID
minor >= 6:  parent UUID, bool expanded
minor >= 7:  [rendering attributes](#84-rendering-attributes)
minor >= 8:  display-material UUID
minor == 9:   two obsolete u8 style fields
minor >= 10:  tagged extension stream
```

Layer extension item gates are:

```
minor >= 10: item 28, no-clipping-planes bool and UUID list
minor >= 11: item 29 hatch-pattern index, item 30 scale, item 31 rotation
minor >= 12: item 32 section fill rule
minor >= 13: item 33 embedded linetype
minor >= 14: item 34 visible in new details
minor >= 15: item 35 embedded section style, item 36 obsolete clipping type,
              item 37 UTF-16 description
```

The writer emits non-default extension IDs in strictly increasing order. The
extension stream is item byte, payload, next item byte, terminated by item
zero. The reader applies the item gates through the same ascending cascade. If
an ID is lower than or equal to the last consumed ID, below its minor gate, or
greater than 37, only that ID byte is consumed; its value has no generic width
and the remaining bytes through the layer class-data boundary remain untyped.
The reader does not require a terminator after that ID. Bytes after item zero
are bounded suffix bytes, not another extension item. Layer visibility and lock
state are independent. Item 37 contains a UTF-16 string using the standard string
grammar. The layer description is normalized by trimming leading and trailing
code points in these ranges: U+0001–U+0020, U+007F–U+00A0, U+2000–U+200B,
U+200E–U+200F, U+2028–U+202F, and U+2066–U+2069. U+1680, U+205F, and U+3000
are retained. The writer omits an empty normalized description. The reader
applies the same normalization; an empty result is the default.

Item 35 contains a direct `ON_SectionStyle` anonymous child. The child uses
version `1.1`, writes the binary-archive model-component attributes, and then
uses this item stream through `u8` item zero:

```
item 1:  u8 background-fill mode
item 2:  two ON_Color background-fill colors
item 3:  bool section-boundary visible
item 4:  two ON_Color section-boundary colors
item 5:  f64 boundary-width scale
item 6:  u8 section-fill rule
item 7:  i32 hatch-pattern index
item 8:  f64 hatch scale
item 9:  f64 hatch rotation in radians
item 10: two ON_Color hatch colors
item 11: direct ON_Linetype child
```

The writer emits only non-default section-style values and emits the direct
child at item 11 when a boundary linetype exists. A reader consumes known item
values and stops at the child boundary when a later item is encountered. The
current writer does not emit the compatibility items 28 through 33 or item 36;
it emits item 34 when the per-viewport visibility default for new detail views
is changed, item 35 when a custom section style exists, and item 37 when the
layer description is nonempty. Item 28's
`no-clipping-planes` value is followed by an `ON_UuidList`; item 29 through 32
are the legacy section-hatch and section-fill values; item 33 is a direct
linetype child; item 36 is an obsolete clipping-type Boolean; and item 37 is
the UTF-16 description string.

CADIR transfers a non-default IGES level as the optional `iges_level` field of
the owning native layer record; the source default `-1` is omitted. Item 34 is
transferred as the optional `visible_in_new_details` field. The obsolete mode,
model index, line-style fields, thickness, scale, and item 36 have no neutral
field. The compatibility section-hatch values, item 33 direct linetype, and
item 35 section-style child have no second neutral resource identity: the
codec validates their bounded source grammar and retains the complete owning
layer record through source fidelity without projecting them into a separate
CADIR object. Item 28 is transferred as `clipping_planes_enabled` after
inverting its source `no-clipping-planes` value. Item 37 is transferred as the
optional native layer `description` field after this normalization; an empty
result is omitted.

#### 8.3.1 Layer per-viewport userdata

`ON__LayerExtensions` is class-owned userdata on `ON_Layer`. Its class UUID and
item UUID are `3E4904E6-E930-4FBC-AA42-EBD407AEFE3B`; its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`. Its payload is the outer anonymous
userdata child from section 7.2:

```text
anonymous version 1.0
i32 per-viewport entry count
count × anonymous per-viewport entry
```

The source writer emits each entry as anonymous version 1.2:

```text
u32 settings mask
if mask & 1:  ON_UUID viewport ID
if mask & 2:  ON_Color per-viewport layer color
if mask & 4:  ON_Color per-viewport plot color
if mask & 8:  f64 per-viewport plot weight in millimeters
if mask & 16: u8 visible (`1` on, `2` off)
if entry minor >= 1 and mask & 16: u8 compatibility visible value
if entry minor >= 2 and mask & 32: u8 persistent visibility (`1` or `2`)
```

The fixed mask values are viewport ID `1`, layer color `2`, plot color `4`,
plot weight `8`, visible `16`, and persistent visibility `32`. A source entry
has an effective mask only when its viewport ID is non-nil and at least one
override is effective. `ON_UNSET_COLOR` means use the layer color or plot
color. A plot weight is effective for a finite value `>= 0.0` or `-1.0`;
`0.0` selects the application default pen and `-1.0` suppresses plotting.
The first visible byte is the effective visibility. Minor 1 writes that byte
twice so a minor-0 reader can consume the first value; the minor-1 reader
consumes the second as its compatibility persistent value. Minor 2 adds the
explicit persistent-visibility byte. A root layer clears persistent
visibility after reading an entry. Entries with no effective mask are removed,
and the reader sorts the remaining entries by viewport ID, effective mask,
visible value, persistent-visibility value, color, plot color, and plot weight.
An empty extension is not archived.

CADIR stores the source-normalized entries in the owning layer's
`per_viewport_settings` array with the viewport UUID, effective mask, RGBA
colors, plot weight, and raw visibility values. A malformed recognized payload
is omitted from that typed array while the layer record is retained with a
decode diagnostic.

The archive layer index is the object-reference key. If two layer records use
one archive index, component registration keeps the original index on the
first record and assigns a distinct unused index to each later record.
References to the original index therefore resolve to the first record.

### 8.4 Rendering attributes

Layer records and object attributes use different rendering-attributes classes.
Both outer records are long `TCODE_ANONYMOUS_CHUNK` records with a CRC32-selected
typecode. A layer uses `ON_RenderingAttributes`:

```text
i32 anonymous major = 1
i32 anonymous minor = 0
i32 material-reference count
count × anonymous material-reference chunk
```

The layer reader requires major 1 and reads the material-reference array. The
minor value does not gate another known field; later bytes before the outer
chunk end are a bounded suffix. The material-reference count is nonnegative.

An object uses `ON_ObjectRenderingAttributes`. The writer emits minor 3. Its
known prefix is:

```text
i32 anonymous major = 1
i32 anonymous minor
i32 material-reference count
count × anonymous material-reference chunk
i32 mapping-reference count
count × anonymous mapping-reference chunk
minor >= 2: bool casts shadows
minor >= 2: bool receives shadows
minor >= 3: bool advanced texture preview
```

The object reader requires major 1 and minor at least 1. Minor 1 ends after
the two arrays; minor 2 appends the two shadow flags; minor 3 appends the
advanced-texture-preview flag. A later minor keeps this prefix and leaves the
remaining bytes before the outer chunk end as a bounded suffix. Each `bool` is
one byte. Both array counts are nonnegative.

Each material reference is a long `TCODE_ANONYMOUS_CHUNK` with this payload:

```text
i32 anonymous major = 1
i32 anonymous minor
UUID plug-in ID                         16 bytes
UUID front-face material ID             16 bytes
i32 obsolete mapping-channel count
obsolete mapping-channel array
minor >= 1:
  UUID back-face material ID            16 bytes
  u8 material source
  u8 reserved[3]
```

The current material-reference writer emits minor 1 and a zero obsolete
mapping-channel count. A minor 0 record ends after the empty obsolete array.
Minor 1 and later append the back-face material UUID, one-byte material-source
selector, and three reserved bytes in that order. Later material-reference
bytes are a bounded suffix after the fields selected by the minor. The
material count and nested chunk boundaries cannot exceed the containing
rendering-attributes chunk.

The obsolete mapping-channel array contains anonymous major-1 mapping-channel
chunks. The reader consumes each complete child and discards the obsolete
array; a valid non-empty array does not prevent the material reference from
being admitted. Each child has its own boundary before the back-face fields.

An object mapping reference is a long anonymous chunk with this payload:

```text
i32 anonymous major = 1
i32 anonymous minor = 0 when written
UUID plug-in ID                         16 bytes
i32 mapping-channel count
count × anonymous mapping-channel chunk
```

The reader requires major 1 and reads the channel array; a later minor leaves
its remaining bytes as a bounded suffix. A mapping channel is a long
anonymous chunk:

```text
i32 anonymous major = 1
i32 anonymous minor = 1 when written
i32 mapping-channel ID
UUID mapping ID                          16 bytes
minor >= 1: ON_Xform, 16 × f64
```

The mapping-channel reader requires major 1. Minor 0 ends after the mapping
ID; minor 1 and later include the 4×4 object transform, followed by any
bounded suffix bytes. All mapping counts are nonnegative. Every child chunk
has its own CRC and ends within its containing rendering-attributes chunk.

CADIR transfers the layer material-reference array to
`native.rhino.layers[].rendering_materials`. It transfers the object
material-reference array to
`native.rhino.object_presentation[].rendering_materials`, the mapping-reference
array to `rendering_mappings`, and each mapping channel to `channels`. A
mapping reference has `plugin_uuid`; a channel has `mapping_channel_id`,
`mapping_uuid`, and, for channel minor at least 1, `object_transform`. The
transform is the sixteen source doubles in row-major 4×4 order. Nil UUIDs are
retained as the UUID string.

CADIR decision: object `casts_shadows` and `receives_shadows` are emitted only
when their source byte is false; an omitted field has the source default true.
`advanced_texture_preview` is emitted only when its source byte is true; an
omitted field has the source default false. A rendering-attributes child with
an older minor that does not carry a field leaves that field omitted. The
source byte is retained even though the OpenNURBS reader does not enable
advanced texture preview from it.

## 9. Object attributes

### 9.1 V3 and V4 fixed attributes

The payload begins with packed version `1.minor`:

```
UUID object ID
i32 layer referenced index
i32 render-material referenced index
ON_Color object color
i16 obsolete line style
i16 obsolete line style index
f64 obsolete thickness
f64 obsolete scale
i32 wire density
u8 object mode
u8 color source
u8 linetype source
u8 material source
UTF-16 name
UTF-16 URL
```

Gates:

```
minor >= 1: i32 group count, group referenced indexes
minor >= 2: bool visible
minor >= 3: i32 display-material count, UUID viewport/display-material pairs
minor >= 4: i32 decoration, plot-color source, plot color,
             plot-weight source, plot-weight f64
minor >= 5: i32 linetype referenced index
minor >= 6: u8 active space, explicit display-material UUID pairs
minor >= 7: [rendering attributes](#84-rendering-attributes)
```

Archives below version 5 write this fixed payload. The V5 and later writer
uses the tagged payload below. The reader selects the tagged V5 reader when
the recorded OpenNURBS writer version is at least `200712190`; an older
OpenNURBS writer uses this fixed payload.

Defaults are normal object mode, visible unless hidden, layer color/linetype/
material/plot color/plot weight sources, model space, wire density 1, and plot
weight 0.0.

The color source values are 0 layer color, 1 object color, 2 unresolved
material color, and 3 parent color for an instance-definition member or layer
color for another object. Other values retain the raw selector, leave neutral
color unset, and emit a typed degradation loss.

The low nibble of object mode is 0 normal, 1 hidden, 2 locked, or 3 instance-
definition member. Higher bits do not change the mode. Before the explicit
visibility field exists, mode 1 selects invisible and all other modes select
visible.

### 9.2 V5 through V8 tagged attributes

The payload begins with packed version `2.minor`, object UUID, and layer
referenced index. The item stream is:

```
u8 item ID
item payload
u8 next item ID
...
u8 0
```

Item payloads:

|  ID | Payload                                      |
| --: | -------------------------------------------- |
|   1 | UTF-16 name                                  |
|   2 | UTF-16 URL                                   |
|   3 | linetype referenced index `i32`              |
|   4 | material referenced index `i32`              |
|   5 | [rendering attributes](#84-rendering-attributes) |
|   6 | object `ON_Color`                            |
|   7 | plot `ON_Color`                              |
|   8 | plot weight `f64` in millimeters             |
|   9 | object decoration `u8`                       |
|  10 | wire density `i32`                           |
|  11 | visibility `bool`                            |
|  12 | object mode `u8`                             |
|  13 | color source `u8`                            |
|  14 | plot-color source `u8`                       |
|  15 | plot-weight source `u8`                      |
|  16 | material source `u8`                         |
|  17 | linetype source `u8`                         |
|  18 | `i32` group count and referenced indexes     |
|  19 | active-space `u8`                            |
|  20 | viewport UUID                                |
|  21 | `i32` display-material count and UUID pairs  |
|  22 | display order `i32`                          |
|  23 | obsolete line-cap source `u8`                |
|  24 | obsolete line-cap style `u8`                 |
|  25 | obsolete line-join source `u8`               |
|  26 | obsolete line-join style `u8`                |
|  27 | obsolete clip-participation source `u8`      |
|  28 | clipping proof `bool` and `ON_UuidList`      |
|  29 | section-attributes source `u8`               |
|  30 | hatch-pattern referenced index `i32`         |
|  31 | section-hatch scale `f64`                    |
|  32 | section-hatch rotation `f64`                 |
|  33 | linetype-pattern scale `f64`                 |
|  34 | hatch-background `ON_Color`                  |
|  35 | hatch-boundary-visible `bool`                |
|  36 | object-frame `ON_Xform`                      |
|  37 | section-fill rule `u8`                       |
|  38 | embedded linetype object                     |
|  39 | embedded section-style object                |
|  40 | clipping-plane label style `u8`              |
|  41 | obsolete selective-clipping-list type `bool` |
|  42 | detail-background-visible `bool`             |

Introduction gates:

```
minor 0: items 1..21
minor 1: item 22
minor 2: items 23..26
minor 3: items 27..28
minor 4: items 29..32
minor 5: item 33
minor 6: items 34..35
minor 7: no new item
minor 8: item 36
minor 9: item 37
minor 10: item 38
minor 11: item 39
minor 12: item 40
minor 13: items 41..42
```

Item 42 is a Boolean. Detail backgrounds are transparent by default; true
requests the detail's display-mode background settings. The writer emits item
42 only when the value is true. The packed minor is a four-bit value. A minor
greater than 13 is a future minor. Its known prefix uses the item grammars
above. The reader applies these gates through the source's ascending cascade.
After a typed item, a lower or equal item ID, a known item before its minor
gate, or an item ID outside 1 through 42 consumes only that one-byte ID and
stops typed parsing. The item's value and all following bytes remain untyped
until the containing `TCODE_OBJECT_RECORD_ATTRIBUTES` boundary. Such a stop
does not require a terminator. Item zero terminates the stream normally; bytes
after that terminator are bounded suffix bytes and are not another tagged item
stream.

Default values are empty strings, unset indexes, default rendering attributes,
unset colors, plot weight 0.0, decoration none, wire density 1, visible true,
normal mode, layer selectors, empty groups, model space, nil viewport, empty
display-material list, display order 0, linetype scale 1.0, hatch boundary
hidden, detail background transparent, and default frame/label style. CADIR
stores a true item-42 value as the optional `detail_background_visible` field
of the owning `object_presentation` record and omits false, which is the source
default.

Present tagged items occur in ascending item-ID order. The terminator follows
the last item.

The effective display state is object visibility combined with layer visibility.
Each color, material, linetype, plot color, and plot weight uses the object
value only when its selector selects the object; otherwise it uses the layer or
document value.

### 9.3 Attribute userdata

`TCODE_OBJECT_RECORD_ATTRIBUTES_USERDATA` is a long chunk after the object
attributes chunk. Its body is the class-userdata stream from section 7.2 and
ends with a short zero `TCODE_OPENNURBS_CLASS_END` marker. Each userdata item
is a long `TCODE_OPENNURBS_CLASS_USERDATA` chunk. The reader stops at the class
end marker and skips bounded suffix bytes through the containing record.

The OpenNURBS object-attributes reader invokes the same class-userdata reader
on the attributes object. After a successful stream read, it removes the
first user-string entry whose key is `$temp_object$`, using the same
case-insensitive ordinal comparison as `ON_UserStringList::SetUserString`.

CADIR keeps object user strings and object-attributes user strings as separate
ordered native arrays. It selects the first serialized `ON_UserStringList`
item with matching class and item UUID in each owner. It applies the
`$temp_object$` removal only to the attributes array; it does not merge the
two arrays or remove that key from geometry userdata.

The `ON_OBSOLETE_CCustomMeshUserData` item is the other typed attributes
carrier. Its direct outer-anonymous body is converted as specified in section
7.2.9; it is not parsed as a nested anonymous payload.

The `ON_PerObjectMeshParameters` item is a typed attributes carrier. Its
generic payload contains the class-owned nested anonymous chunks specified in
section 7.2.10, and the resulting mesh parameters are retained under the
owning native object presentation.

The `ON_DisplacementUserData` item is a typed attributes carrier. Its XML
payload and displacement/sub-item fields are specified in section 7.2.12; the
resulting modifier is retained under the same object presentation without
changing the transferred object geometry.

The `ON_EdgeSofteningUserData` item is a typed attributes carrier. Its XML
payload and edge-softening fields are specified in section 7.2.13; the
resulting modifier is retained under the same object presentation without
changing the transferred object geometry.

The `ON_ThickeningUserData` item is a typed attributes carrier. Its XML
payload and thickening fields are specified in section 7.2.14; the resulting
modifier is retained under the same object presentation without changing the
transferred object geometry.

The `ON_CurvePipingUserData` item is a typed attributes carrier. Its XML
payload and curve-piping fields are specified in section 7.2.15; the resulting
modifier is retained under the same object presentation without changing the
transferred object geometry.

The `ON_ShutLiningUserData` item is a typed attributes carrier. Its XML payload
and scalar/curve fields are specified in section 7.2.16; the resulting modifier
is retained under the same object presentation without changing the transferred
object geometry.

## 10. Compressed buffers

A nonzero compressed buffer is:

```
u32 uncompressed size
u32 CRC32 of uncompressed bytes
u8 method
body
```

The size is always four bytes and is bounded by `UINT32_MAX`. A zero size ends
the buffer immediately; no CRC, method, or body follows.

```
method 0: stored bytes, exactly uncompressed size
method 1: one anonymous long chunk whose body is a complete zlib stream
```

The outer CRC covers the uncompressed bytes. The anonymous method-1 chunk has
its own chunk CRC after the compressed stream. The chunk declaration provides
the compressed-input boundary; the zlib stream consumes its entire chunk body.
Inflated output has exactly the declared size. Unknown methods, wrong method-1
chunk type, zlib failure, truncation, trailing compressed bytes, and outer CRC
failure make the buffer invalid.

## 11. Class UUID registry

| Class                    | UUID                                   |
| ------------------------ | -------------------------------------- |
| `ON_Geometry`            | `4ED7D4DA-E947-11D3-BFE5-0010830122F0` |
| `ON_CurveProxy`          | `4ED7D4D9-E947-11D3-BFE5-0010830122F0` |
| `ON_CurveOnSurface`      | `4ED7D4D8-E947-11D3-BFE5-0010830122F0` |
| `ON_NurbsCurve`          | `4ED7D4DD-E947-11D3-BFE5-0010830122F0` |
| `ON_LineCurve`           | `4ED7D4DB-E947-11D3-BFE5-0010830122F0` |
| `ON_ArcCurve`            | `CF33BE2A-09B4-11D4-BFFB-0010830122F0` |
| `ON_PolylineCurve`       | `4ED7D4E6-E947-11D3-BFE5-0010830122F0` |
| `ON_PolyCurve`           | `4ED7D4E0-E947-11D3-BFE5-0010830122F0` |
| `ON_PolyEdgeCurve`       | `39FF3DD3-FE0F-4807-9D59-185F0D73C0E4` |
| `ON_PolyEdgeSegment`     | `42F47A87-5B1B-4E31-AB87-4639D78325D6` |
| `ON_NurbsSurface`        | `4ED7D4DE-E947-11D3-BFE5-0010830122F0` |
| `ON_PlaneSurface`        | `4ED7D4DF-E947-11D3-BFE5-0010830122F0` |
| `ON_RevSurface`          | `A16220D3-163B-11D4-8000-0010830122F0` |
| `ON_SumSurface`          | `C4CD5359-446D-4690-9FF5-29059732472B` |
| `ON_Mesh`                | `4ED7D4E4-E947-11D3-BFE5-0010830122F0` |
| `ON_Brep`                | `60B5DBC5-E660-11D3-BFE4-0010830122F0` |
| `ON_Extrusion`           | `36F53175-72B8-4D47-BF1F-B4E6FC24F4B9` |
| `ON_SubD`                | `F09BA4D9-455B-42C3-BA3B-E6CCACEF853B` |
| `ON_Point`               | `C3101A1D-F157-11D3-BFE7-0010830122F0` |
| `ON_PointCloud`          | `2488F347-F8FA-11D3-BFEC-0010830122F0` |
| `ON_PointGrid`           | `4ED7D4E5-E947-11D3-BFE5-0010830122F0` |
| `ON_EmbeddedBitmap`      | `772E6FC1-B17B-4FC4-8F54-5FDA511D76D2` |
| `ON_WindowsBitmap`       | `390465EB-3721-11D4-800B-0010830122F0` |
| `ON_WindowsBitmapEx`     | `203AFC17-BCC9-44FB-A07B-7F5C31BD5ED9` |
| `ON_Hatch`               | `0559733B-5332-49D1-A936-0532AC76ADE5` |
| `ON_DetailView`          | `C8C66EFA-B3CB-4E00-9440-2AD66203379E` |
| `ON_NurbsCage`           | `06936AFB-3D3C-41AC-BF70-C9319FA480A1` |
| `ON_MorphControl`        | `D379E6D8-7C31-4407-A913-E3B7040D034A` |
| `ON_Centermark`          | `D46767BA-7E8F-4D9D-9A92-66050219A5B9` |
| `ON_Layer`               | `95809813-E985-11D3-BFE5-0010830122F0` |
| `ON_InstanceDefinition`  | `26F8BFF6-2618-417F-A158-153D64A94989` |
| `ON_InstanceRef`         | `F9CFB638-B9D4-4340-87E3-C56E7865D96A` |
| `ON_3dmObjectAttributes` | `A828C015-09F5-477C-8665-F0482F5D6996` |
| `ON_DimStyle`             | `67AA51A5-791D-4BEC-8AED-D23B462B6F87` |
| `ON_V5x_DimStyle`         | `81BD83D5-7120-41C4-9A57-C449336FF12C` |

These registered legacy identities use the current class payload layout:

| Payload family     | Alias UUIDs                                                                                             |
| ------------------ | ------------------------------------------------------------------------------------------------------- |
| NURBS curve        | `5EAF1119-0B51-11D4-BFFE-0010830122F0`, `76A709D5-1550-11D4-8000-0010830122F0`                          |
| NURBS surface      | `4760C817-0BE3-11D4-BFFE-0010830122F0`, `FA4FD4B5-1613-11D4-8000-0010830122F0`                          |
| polycurve          | `EF638317-154B-11D4-8000-0010830122F0`                                                                  |
| Brep               | `0705FDEF-3E2A-11D4-800E-0010830122F0`, `2D4CFEDB-3E2A-11D4-800E-0010830122F0`, `F06FC243-A32A-4608-9DD8-A7D2C4CE2A36` |
| revolution surface | `0A8401B6-4D34-4B99-8615-1B4E723DC4E5`                                                                  |

Alias identity does not add a payload prefix or suffix. It participates in
the same curve/surface base-class checks used by polymorphic Brep arrays.
`ON_Circle` and `ON_Arc` are value types; their object wrapper is
`ON_ArcCurve`.

`ON_CurveProxy`, `ON_SurfaceProxy`, `ON_OffsetSurface`, `ON_PointGrid`,
`ON_MeshComponentRef`, and `ON_SubDComponentRef` are runtime reference or
cache classes and have no valid persistent class-data payload. A
`ON_PolyEdgeSegment` is the archive-bearing proxy-derived exception and uses
the payload below.

`ON_PointGrid::Write` and `ON_PointGrid::Read` return false. An in-memory point
grid therefore has no `rhino_3dm` object-class payload and is not a persistent
format construct.

## 12. Curves and points

For archive versions 4, 50, and 60, `ON_BinaryArchive::WriteObject` writes
these point and curve classes through their class `Write` methods. Its curve
compatibility translation to an `ON_NurbsCurve` applies only to archive
versions 1 and 2. The payloads below are therefore the direct class-data
payloads used by the V4, V5, and V6 object tables.

### 12.1 Point

Packed version `1.0`; major 1 is accepted. The payload is:

```
u8 version
ON_3dPoint point
```

The point coordinates are document-length values. CADIR transfers one neutral
point and one free vertex with the same source object identity, and converts
the three coordinates to millimetres. There is no point parameter or
dimension field.

### 12.2 Point cloud

Packed version `1.2`; major 1 is accepted. Fields:

```
u8 version
i32 point count
point count × ON_3dPoint
ON_Plane plane
ON_BoundingBox bounding box
i32 flags
minor >= 1:
  i32 normal count
  normal count × ON_3dVector
  i32 color count
  color count × ON_Color
minor >= 2:
  i32 value count
  value count × f64
```

Optional counts are nonnegative and bounded by the containing payload. A
source writer emits zero or the point count for a channel. Flags bit 0 means
ordered points; bit 1 means the plane is set. Writers emit version 1.2. A
major-1 reader uses the minor gates above and skips a later bounded suffix.

The point array is the source point sequence. The plane origin and bounding-box
endpoints are document-length values. Plane axes and point normals are
dimensionless vectors. Point colors are direct RGBA bytes; the alpha byte is
transparency. Point values are scalar intensity values. `ON_PointCloud::IsValid`
requires only a positive point count. The source `HasPointNormals`,
`HasPointColors`, and `HasPointValues` accessors report a channel only when its
count equals the point count; a source reader can still consume a bounded
nonmatching array, which then has no channel meaning. The runtime hidden-point
array is not serialized.

CADIR transfers one neutral point and one free vertex for each stored point, in
source order, and converts the point coordinates to millimetres. CADIR assigns
no neutral field to the plane, cached bounds, flags, normals, colors, or scalar
values. The complete object record remains linked through the Rhino native
unknown record and source-fidelity bytes, which are authoritative for those
fields. A nonzero optional-channel count that differs from the point count is
consumed and dropped with `container.redundant-field-repaired`; a zero count
means that channel is absent. Point-cloud runtime hidden flags are absent after
readback because they have no wire representation.

### 12.3 Line curve

Packed version `1.0`; major 1 is accepted:

```
u8 version
ON_Line from/to points
ON_Interval domain
i32 dimension
```

`ON_Line` is two `ON_3dPoint` values in `from`, then `to` order. The line
domain is two f64 values. `dimension` is serialized without fallback. A
source-valid line has distinct endpoints and a nondecreasing domain; the
source `SetDomain` mutator accepts only a strictly increasing domain. CADIR's
typed admission requires finite distinct endpoints, a finite strictly
increasing domain, and dimension 2 or 3. A source-writable equal-domain line
is retained as a native object record when it fails that CADIR gate.

CADIR transfers a line as a degree-one NURBS curve with control points
`from`, `to` and knots `[t0,t0,t1,t1]`. Endpoint coordinates are converted to
millimetres; domain parameters and `dimension` are not converted.

### 12.4 Arc curve

Packed version `1.0`; major 1 is accepted:

```
u8 version
ON_Circle circle
ON_Interval angle
ON_Interval curve domain
i32 dimension
```

The serialized `ON_Circle` is:

```text
ON_Plane
  ON_3dPoint origin
  ON_3dVector x axis
  ON_3dVector y axis
  ON_3dVector z axis
  ON_PlaneEquation x, y, z, d
f64 radius
ON_3dPoint point at angle 0
ON_3dPoint point at angle π/2
ON_3dPoint point at angle π
```

The three points are consistency values written by `ON_BinaryArchive` from
the circle at the stated angles. The plane origin, radius, consistency
points, and plane-equation `d` are document-length values; plane axes and the
first three plane-equation coefficients are dimensionless. The angle and
curve-domain intervals are parameters and are not converted. The source
reader accepts major 1 and normalizes a dimension other than 2 or 3 to 3.

Invalid dimensions are normalized to 3 by the payload rule. Radius and both
intervals must be valid.

A full circle uses the analytic representation when its angle and curve domain
are `[0, 2π]` and the x-axis norm differs from one by less than `1e-10`.
Otherwise the transfer uses the rational quadratic representation.

### 12.5 Polyline curve

Packed version `1.0`; major 1 is accepted:

```
u8 version
i32 point count
point count × ON_3dPoint
i32 parameter count
parameter count × f64
i32 dimension
```

The point count is at least two, parameter count equals point count, and
parameters are finite and strictly increasing. The degree-one NURBS knot vector
for parameters `t[0..n)` is:

```
[t0, t0, t1, ..., t[n-2], t[n-1], t[n-1]]
```

Polyline points are document-length values; parameters and `dimension` are
not converted. CADIR's typed admission requires dimension 2 or 3. CADIR
transfers the point sequence as one degree-one NURBS curve with the knot
vector above.

### 12.6 Polycurve

A packed version byte precedes this bounded layout:

```
u8 version
i32 segment count
i32 reserved
i32 reserved
ON_BoundingBox reserved bounds
i32 parameter count
parameter count × f64 segment parameters
segment count × polymorphic ON_Curve
```

The two reserved `i32` values are zero in the source writer. The reserved
`ON_BoundingBox` is written and read but has no defined procedural meaning;
the source writer uses a default/unset value. `WriteArray(m_t)` supplies the
parameter count and values. Parameter count is segment count plus one.
Segment parameters are finite and strictly increasing. Each child is a
polymorphic `ON_Curve` class wrapper, in child order. The source validity
check also requires every child to be valid and of the same dimension, rejects
a closed child in a multi-child polycurve, and rejects gaps between adjacent
children.

`ON_PolyCurve::Read` consumes this layout without testing the packed major or
minor; its source writer emits version 1.0. This reader behavior is specific to
`ON_PolyCurve` and does not establish a future-major field contract. `ON_Brep`
legacy C2 and C3 arrays call the same reader for their direct polycurve
payloads.

CADIR typed admission is limited to source-defined major 1 polycurve payloads
for both top-level and legacy C2/C3 decoding. A payload with another major is
not assigned the current layout by CADIR; its containing object remains a
retained native record with an unsupported-version loss. This is an admission
decision, not a claim that the OpenNURBS reader rejects the bytes.

CADIR emits one compound procedural-curve record for the polycurve and one
neutral carrier curve for each child. Child geometry is converted to
millimetres; child and polycurve parameters, the reserved values, and the
reserved bounds are not converted. The parent retains the source object
record. A bounded child gap is represented by the midpoint join repair and
emits its geometry warning; a malformed child or parameter array does not
enter the typed compound record.

Conversion of a polycurve to one NURBS curve first maps each segment to its
polycurve parameter interval. Each segment is endpoint-clamped. Segments of
lower degree are elevated to the maximum segment degree. At each join, the last
control point of the accumulated curve and the first control point of the next
segment move to their midpoint. The first control point of the next segment is
then omitted. The appended knots are translated by the difference between the
accumulated end parameter and the next segment start parameter. The internal
join has `degree` equal knots. The result has this identity:

```text
knot count = control-point count + degree + 1
```

#### 12.6.1 Persistent polyedge references

`ON_PolyEdgeCurve` uses the polycurve payload with every child class equal to
`ON_PolyEdgeSegment`. A segment is an anonymous version 1.0 chunk:

```text
UUID referenced object (16 wire-order bytes)
i32 component type
i32 component index
f64 edge-domain minimum
f64 edge-domain maximum
f64 trim-domain minimum
f64 trim-domain maximum
u8 proxy reversed (0 or 1)
f64 polyedge-segment-domain minimum
f64 polyedge-segment-domain maximum
f64 referenced-curve-domain minimum
f64 referenced-curve-domain maximum
```

The segment and parameter counts obey the polycurve invariants. All domains
are finite. The object UUID and component index persist the source curve,
Brep edge, or Brep trim selection; the reversal and domain fields define its
orientation and parameter mapping inside the polyedge.
When `ON_PolyEdgeSegment::Create` receives an ordinary source curve rather
than a Brep edge or trim, the edge and trim domains remain empty intervals:
both endpoints are the finite `ON_UNSET_VALUE` sentinel
`-1.23432101234321e+308`. Those sentinel pairs mean that no Brep edge or trim
subdomain is selected; they are not increasing domains.

The packed curve readers consume their known prefixes and skip unread bytes
before the containing class-data boundary. A later minor does not change the
field order of the known prefix.

### 12.7 Curve on surface

`ON_CurveOnSurface` has no version prefix. Its bounded class payload is:

```text
polymorphic two-dimensional ON_Curve
i32 model-curve-present
if present: polymorphic model-space ON_Curve
polymorphic ON_Surface
```

The presence value is zero or one. The first curve is in support-surface
parameter space and remains unscaled. The optional model curve and support
surface use document length conversion. All three child objects must derive
from their declared curve or surface families. The model curve is the exact
stored solved carrier when present; the parameter curve and support surface
retain the construction relationship independently.
The containing class-data reader skips bytes after the three known child
objects and their presence field before its bounded end.

## 13. NURBS curves and surfaces

### 13.1 NURBS curve

Packed version `1.0` before archive 60 and `1.1` at archive 60 and later.
Major 1 is accepted. Minor 1 adds a trailing SubD-friendly boolean tag.

```
u8 version
i32 dimension
i32 rational flag
i32 order
i32 CV count
i32 reserved
i32 reserved
ON_BoundingBox reserved bounds
i32 stored knot count
stored knot count × f64
i32 stored CV count
stored CV count × (dimension + rational) f64
minor >= 1: bool SubD-friendly tag
```

Require order at least 2, CV count at least order, stored CV count equal to CV
count, and stored knot count `order + CV count - 2`. Knots are finite and
nondecreasing. The native domain is:

```
domain.min = K[order - 2]
domain.max = K[CV count - 1]
```

Rational CVs are homogeneous `[xw,yw,zw,w]`; Euclidean points are
`[xw/w,yw/w,zw/w]`. Weights are finite and nonzero. Periodicity is derived
from the reconstructed knot vector, not serialized as a boolean.
After the known minor-gated fields, the reader skips any suffix before the
bounded class-data end.

CADIR transfers an admitted curve of dimension 2 or 3 to a neutral NURBS
curve. A dimension-2 pole receives a zero third coordinate. Pole coordinates
are converted to millimetres; degree, the reconstructed knot vector,
parameter domain, weights, and periodicity are unchanged. The reserved
bounding box is framing data, not a curve definition, and remains available
only through the retained native record.

The stored vector omits two endpoint knots. Let `o=order`, `n=CV count`,
`m=o+n-2`, and `K[0..m)` be stored knots. The full vector has `o+n` entries:

```
F[0] = start
F[i+1] = K[i]                 for 0 <= i < m
F[m+1] = end
```

```
start = K[0]
if o > 2 and n >= 2*o-2 and n >= 6 and K[0] < K[o-2]:
    start = K[0] - (K[n-o+1] - K[n-o])

end = K[m-1]
if o > 2 and n >= 2*o-2 and n >= 6 and K[n-1] < K[m-1]:
    end = K[m-1] + (K[o+1] - K[o])
```

For `o=3`, `n=6`, `K=[0,0,0,1,2,3,3]`, the full vector is
`[0,0,0,0,1,2,3,3,3]`. For `K=[0,1,2,3,5,6,7]`, it is
`[-2,0,1,2,3,5,6,7,9]`. Endpoint clamping must not be imposed.

### 13.2 NURBS surface

Packed version `1.0`; major 1 is accepted:

```
u8 version
i32 dimension
i32 rational flag
i32 U order
i32 V order
i32 U CV count
i32 V CV count
i32 reserved
i32 reserved
ON_BoundingBox reserved bounds
i32 U stored knot count
U stored knots
i32 V stored knot count
V stored knots
i32 stored CV count
stored CV count × (dimension + rational) f64
```

The U and V stored knot counts are `order + CV count - 2`; stored CV count is
`U count * V count`. Reconstruct each knot vector independently using the
curve rule. The wire iteration is:

```
for i in 0..U_count:
  for j in 0..V_count:
    CV(i,j)
```

The flat index is `i * V_count + j`. Rational surface CVs use the same
homogeneous conversion. Periodicity in each direction is derived from its knot
vector.
The reader consumes the known major-1 prefix and skips any suffix before the
bounded class-data end.

OpenNURBS accepts a positive source dimension. `CADIR decision:` typed surface
admission accepts dimensions 2 and 3; a dimension-2 pole receives a zero third
coordinate. Pole coordinates are converted to millimetres. Orders, counts,
knot vectors, parameter domains, weights, and periodicity are unchanged.
The legacy class UUIDs `4760C817-0BE3-11D4-BFFE-0010830122F0` and
`FA4FD4B5-1613-11D4-8000-0010830122F0` select this same payload grammar.

### 13.3 Plane surface

Packed version has major 1 and a nonnegative minor:

```
u8 version
ON_Plane plane
ON_Interval U domain
ON_Interval V domain
minor >= 1:
  ON_Interval U extents
  ON_Interval V extents
```

Version 1.0 uses domains as extents. Domains and extents are independent; the
domain controls parameterization. Every interval is finite and strictly
increasing. For a parameter `u` and domain `D = [D0,D1]`, the plane coordinate
is `E0 + (u-D0) × (E1-E0) / (D1-D0)`, where `E = [E0,E1]` is the matching
extent. The same rule applies to `v`. The evaluated point is the plane origin
plus the mapped U coordinate times the plane X axis plus the mapped V
coordinate times the plane Y axis. A reader consumes the known prefix for the
minor and skips remaining bytes before the bounded payload end.

`CADIR decision:` a transferred plane surface uses the neutral infinite-plane
carrier with the source origin converted to millimetres, the source Z axis as
normal, and the source X axis as `u_axis`. The source equation is validated
but is not a second neutral field. Domains and extents remain native
parameterization data; for a Brep they are used by the pcurve parameter map,
and for a free surface they remain in the retained native record because the
neutral plane carrier has no finite-rectangle fields.

### 13.4 Clipping-plane surface

Class UUID `DBC5A584-CE3F-4170-98A8-497069CA5C36` contains an anonymous
version 1 chunk with a nonnegative minor. Its first child is an anonymous chunk
containing a plane-surface payload. Its second child is the clipping-plane
record:

```text
anonymous version 1.minor
  anonymous plane-surface carrier
  clipping-plane anonymous chunk
```

The clipping-plane chunk has major version 1 and a nonnegative minor:

```text
ON_UUID first_viewport_id
ON_UUID plane_id
ON_Plane plane
bool enabled
if minor >= 1: ON_UuidList viewport_ids
if minor >= 2: f64 depth
if minor >= 4: bool depth_enabled
if minor >= 5: ordered participation items followed by u8 zero
```

Minor 0 uses `first_viewport_id` as the viewport list. In later minors that
field is retained for layout compatibility and `viewport_ids` is the complete
list. Minor 2 depth uses the original distance interpretation. Minor 3 changes
the interpretation without changing its wire type. Before minor 4, a
nonnegative depth other than the unset positive value enables depth clipping;
minor 4 carries the explicit flag.

Minor 5 participation items are ordered and optional:

| Item | Payload                                      |
| ---: | -------------------------------------------- |
|   10 | `i32 count`, `count` referenced object UUIDs |
|   11 | `i32 count`, `count` referenced layer `i32`s |
|   12 | `bool is_exclusion_list`                     |
|   13 | `bool participation_lists_enabled`           |
|    0 | terminator                                   |

Each item can occur at most once. Present items occur in ascending item order.
Item codes 14 and above terminate the known participation stream; the
remaining bytes are skipped at the bounded chunk end.

`CADIR decision:` a transferred clipping-plane surface uses the first child
plane surface as its neutral plane carrier. The clipping-plane child is
parsed and validated, but its viewport IDs, plane ID, enabled flag, clipping
plane, depth controls, and participation lists remain native-record fields;
the neutral surface has no clipping-control fields. The clipping plane may
have a different origin from the first child plane; the first child still
defines the transferred surface geometry.

### 13.5 Revolution surface

Writers emit packed version `2.0`; the version byte is
`(major << 4) | minor`, and majors 1 and 2 are accepted. The presence field is
a one-byte `char`; transpose is an `i32`:

```
u8 version
ON_Line axis
ON_Interval angle
major >= 2: ON_Interval surface parameter interval
ON_BoundingBox bounds
i32 transposed
char profile present
if present: polymorphic ON_Curve profile
```

Major 1 defaults the surface parameter interval to the angular interval. A
present profile is a curve. The axis endpoints and profile geometric values
are document lengths. The angle interval is in radians. The surface parameter
interval maps its endpoints to the angle interval endpoints and is not a
length. `transposed = 0` makes angle the U parameter and the profile the V
parameter; `transposed = 1` swaps those directions. A profile control point on
the revolution axis produces one exact axis control point at every angular
control position.

The source writer emits presence `1` when `m_curve` is non-null and `0`
otherwise. A zero is readable as an object payload, but it is not a valid
`ON_RevSurface`: source validity requires a non-null valid three-dimensional
profile. CADIR rejects a zero presence flag for typed transfer and retains the
bounded source record. A present profile must have an exact NURBS
representation for the decoder to construct the solved NURBS carrier; the
profile remains the procedural surface's directrix child.

`bounds` is the six-f64 cached `ON_RevSurface::m_bbox` value. Its coordinates
are document lengths. CADIR consumes and bounds this field but does not place
the cache in the procedural definition; it reconstructs the solved carrier
from the axis, intervals, transpose flag, and directrix. The known fields are
followed by a bounded suffix that the reader skips.

### 13.6 Sum surface

Writers emit packed version `1.0`, where the byte is `(major << 4) | minor`.
Major 1 is accepted:

```
u8 version
ON_3dVector basepoint
ON_BoundingBox bounds
polymorphic ON_Curve first
polymorphic ON_Curve second
```

`basepoint` is a document-length translation vector. `bounds` is the six-f64
cached `ON_SumSurface::m_bbox` value; its coordinates are document lengths and
the decoder consumes it as bounded framing input. It is not part of the
procedural definition. The two curve slots are ordered: slot 0 supplies U and
slot 1 supplies V. Curve geometric coordinates are document lengths; curve
parameter values, knot values, and rational weights do not receive document
unit conversion.

The exact surface is `S(u,v)=basepoint+C0(u)+C1(v)`. For child homogeneous
poles `H0=(wP,w)` and `H1=(vQ,v)`, the surface weight is `wv` and the
homogeneous point is `v(wP)+w(vQ)+wv*basepoint`. U inherits the first curve;
V inherits the second.
Source validity requires both child pointers to name valid three-dimensional
curves and requires a valid basepoint. A nil child is nevertheless a readable
archive object because the polymorphic object slot can contain a nil UUID; it
is source-invalid, and CADIR retains the bounded source record instead of
admitting a typed sum. Typed transfer additionally requires both children to
have an exact NURBS representation. The solved carrier uses the ordered child
domains and knot vectors and the product-weight formula above. The reader
consumes the known prefix and skips any suffix before the bounded class-data
end.

## 14. Mesh

`ON_Mesh` begins with a packed version byte in its class-data payload. Major 1
is uncompressed, major 3 is compressed, and major 2 has no defined payload
layout. The common prefix is:

```
u8 version
i32 vertex count
i32 face count
2 × ON_Interval packed texture domain
2 × ON_Interval surface domain
2 × f64 surface scale
float vertex bounds[6]
float normal bounds[6]
float texture bounds[4]
i32 closed state
u8 mesh-parameters present
if present: bounded anonymous mesh-parameters chunk
4 × (u8 curvature-stat present, optional bounded chunk)
face array
```

Closed state is `-1` unknown, `0` open, `1` closed, `2` obsolete closed;
other values are unknown. Face index width is explicitly serialized:

```
i32 index width
face count × four indices
```

The stored width is 1, 2, or 4. Writers select 1 when vertex count is below
256, 2 when it is below 65536, and 4 otherwise. Readers use the stored width.
Indices are little-endian unsigned values. A triangle is `[v0,v1,v2,v2]`; a
quad is `[v0,v1,v2,v3]`. Neutral triangulation splits a quad along its shorter
geometric diagonal; equal diagonals select `0-2`. A repeated quad vertex is
removed before triangulation.
Quad topology is not preserved in the neutral tessellation: every four-vertex
face is transferred as two triangles, and the transfer is derived.

Major 1 follows the face array with raw counted arrays:

```
ON_3fPoint vertices
ON_3fVector normals
ON_2fPoint texture coordinates
ON_SurfaceCurvature curvature
ON_Color colors
minor >= 2: i32 packed texture rotation
```

Major 3 follows the face array with five compressed buffers for vertices,
normals, texture coordinates, curvature, and colors. Nonzero sizes must equal
the expected channel byte count. Minor gates are:

```
minor >= 2: i32 packed texture rotation
minor >= 3: texture-mapping UUID, compressed surface parameters (2×f64/vertex)
minor >= 4 and writer version >= 200606010:
  anonymous mapping tag
  minor >= 5: manifold, oriented, solid bytes
  minor >= 6: ngon-present byte and optional ngon chunk
  minor >= 7: double-vertex-present byte and optional double-vertex chunk
  minor >= 8: serialized vertex bounding box
```

The mapping tag is an anonymous major-1 chunk. Minor 1 adds the mapping type:
mapping UUID, `i32` CRC, sixteen transform doubles, and for minor at least 1 a
`u32` mapping type. Ngon records contain a
`u32` count followed by each boundary vertex count, face count, vertex indices,
and face indices. Double vertices contain a `u32` count and, when nonzero, a
compressed `3*f64` channel. A valid double channel has exactly the declared
mesh vertex count and finite values.

Archive version 50 mesh objects can also carry the class-userdata item in
section 7.2.3. It is used only when the class-data payload did not provide a
double-precision channel. Its accepted coordinates are the serialized f64
values whose casts equal the float vertex channel; rejected userdata leaves
the float channel authoritative.

Mesh objects in every archive band can carry the class-userdata item in
section 7.2.4 when it remains attached. Its admitted record count reports
source n-gon grouping; the mesh face table remains the neutral tessellation
input. When both this item and the inline major-3 n-gon chunk are present, the
inline count is authoritative for the neutral grouping loss.

A top-level mesh object written from a runtime SubD in an archive below 60 can
also carry the section 7.2.6 proxy item. Its parent-array counts and raw-array
SHA-1 values are checked before the mesh is admitted as a SubD; a failed check
leaves the ordinary mesh tessellation authoritative.

## 15. Brep

`ON_Brep` major 2 uses the historical trimmed-face payload in section 15.0.
`ON_Brep` major 3 uses payload version 3.minor. Minor 1 adds mesh-side
chunks, minor 2 adds `is_solid`, and minor 3 adds region topology. Later
minors append fields before the bounded end:

```
packed version
C2 polymorphic curve array
C3 polymorphic curve array
surface polymorphic array
vertex raw array
edge raw array
trim raw array
loop raw array
face raw array
ON_BoundingBox
minor >= 1: render-mesh side chunk, analysis-mesh side chunk
minor >= 2: i32 is_solid
minor >= 3: anonymous region-topology chunk
```

### 15.0 Legacy major 2

The class UUID remains `ON_Brep`; the class-data payload starts with packed
version `2.minor`. The reader requires positive face, edge, loop, and trim
counts, then reads the following fields:

```text
i32 face count
i32 edge count
i32 loop count
i32 trim count
i32 outer flag
ON_BoundingBox Brep bounds
trim count × direct ON_PolyCurve C2 payloads
edge count × direct ON_PolyCurve C3 payloads
face count × direct ON_NurbsSurface payloads
face count × {
  i32 legacy face index
  i32 obsolete material index
  i32 reversed-surface flag
  i32 legacy face-type flag
  ON_BoundingBox face bounds
  i32 loop count
  loop count × {
    i32 legacy loop index
    i32 boundary type (-1 slit, 0 outer, 1 inner)
    4 × f64 parameter-space bounds
    i32 trim count
    trim count × {
      i32 legacy trim index
      i32 legacy twin index
      u8 managed-edge flag
      i32 edge index
      i32 reversed-3D flag
      i32 legacy continuity flag
      i32 legacy monotonicity flag
      f64 legacy 3D tolerance
      f64 legacy 2D tolerance
    }
  }
}
face count × u8 render-mesh-present flag and optional ON_Mesh class
minor >= 1: face count × u8 analysis-mesh-present flag and optional ON_Mesh class
```

The C2 and C3 values are direct `ON_PolyCurve` payloads, not polymorphic
class wrappers. The surface values are direct `ON_NurbsSurface` payloads.
The source reader assigns C2 slot `trim index` to each trim. An invalid edge
index becomes `-1`; a true managed-edge flag with an invalid index rejects the
payload. The legacy twin, continuity, and monotonicity values are consumed as
source-only fields; the source reader then derives trim ISO and trim-type
flags from the loaded surface, loop, edge, and trim topology. The positional
edge index and the ordered loop rings therefore control typed reconstruction.
Major-2 has no serialized ISO value. CADIR does not infer the source-only ISO
cache; its internal raw value is `not-iso`, and the neutral pcurve use leaves
`isoparametric` unset.

Legacy boundary type maps to loop type 3 for `-1`, 1 for `0`, 2 for `1`, and
0 for another value. The trim type is derived as singular when the trim has no
edge, boundary when its edge has one trim, seam when its edge has another trim
in the same loop, and mated otherwise. The legacy 2D tolerance becomes both
public trim tolerances; the legacy 3D tolerance is the edge tolerance maximum.
Each vertex tolerance is the maximum incident edge tolerance and the distance
from the averaged vertex position to its incident edge endpoint.

Major-2 has no serialized vertex table. The source reader establishes vertex
identity by walking each directed loop ring and by joining both uses of an
edge, with `reversed-3D` selecting which trim endpoint corresponds to each
edge endpoint. It then averages the incident C3 endpoints for the vertex
position. The decoder applies this same endpoint-equivalence rule. An edge
with no trim uses its C3 endpoints as independent vertex positions.

The face, loop, and trim bounds are cached source boxes. The source reader
fills missing trim and loop boxes from the C2 curves after topology loading;
they do not replace the ordered topology or the C3-derived vertex positions.
The render and analysis mesh sides are optional caches. A malformed or wrong-
class present mesh is discarded while the Brep topology remains transferable.
The outer flag is a legacy solid hint: the source reader sets its runtime
`m_is_solid` cache to outward-solid only when the flag is 1 and the completed
Brep passes `IsSolid()`. CADIR does not promote that conditional cache to a
neutral field; major-2 body kind uses the validated topology rule in section
15.5 and retains the original bytes.

Polymorphic C2/C3/surface arrays are anonymous major-1, then `i32 count`
and for each slot an `i32 present` flag followed by one polymorphic object when
present is 1. Zero denotes a positional null slot. Vertices, edges, trims,
loops, and faces are raw anonymous major-1 arrays with a packed `1.minor` byte
and inline records. Face array version is 1.1 before archive 70 and 1.2 at
archive 70+;
minor 1 adds one UUID per face and minor 2 adds a presence byte and one color
per face.

### 15.1 Vertex

```
i32 vertex index
ON_3dPoint point
i32 edge count
edge count × i32 edge index
f64 tolerance
```

An empty edge list is valid. It records a native vertex with no edge
incidence. A singular or point-on-surface trim can still name that vertex;
those trims use edge index `-1` and identical endpoint indexes. The vertex
record has no shell-membership field.

### 15.2 Edge

```
i32 edge index
i32 C3 index
i32 proxy reversed
ON_Interval proxy domain
i32 vertex index[2]
i32 trim count
trim count × i32 trim index
f64 tolerance
archive >= 3 and writer version >= 200206180:
  ON_Interval edge domain
```

Without the final domain, edge domain equals proxy domain. Proxy reversal is
an `i32` flag. Both intervals are increasing. A nonzero proxy-reversal flag
reverses the proxy curve before the edge domain is assigned.

### 15.3 Trim

Common fields:

```
i32 trim index
i32 C2 index
ON_Interval proxy domain
i32 edge index
i32 vertex index[2]
i32 reversed 3D
i32 trim type
i32 ISO
i32 loop index
f64 tolerance[2]
```

One trim stores exactly one parameter-space C2 reference and one proxy domain.
The native trim record has no repeated C2-use list or alternate parameter-space
curve slot.

CADIR writer decision: a neutral coedge with more than one ordered pcurve use
is not representable by this trim record. The Rhino writer rejects that
coedge before output; it does not select the first use or discard the other
carriers, ranges, or tolerances.

Each stored tolerance is a finite nonnegative value or an explicit unset
sentinel. The unset sentinels are `-1.23432101234321e308` and
`+1.23432101234321e308`.

When archive version is at least 3 and writer version is at least 200206180:

```
ON_Interval trim domain
u8 proxy reversed
u8 reserved[7]
u8 reserved[24]
```

Otherwise two legacy `ON_3dPoint` placeholders are read. Both branches append
legacy 2D and 3D tolerance doubles.

Trim types are 0 unknown, 1 boundary, 2 mated, 3 seam, 4 singular, 5
curve-on-surface, 6 point-on-surface, and 7 slit/reserved. ISO values are 0
not-iso, 1 interior U, 2 interior V, 3 west, 4 south, 5 east, and 6 north.
Values outside the defined sets are unknown. Singular and point-on-surface
trims use edge index -1 and identical endpoint vertices.

A boundary trim is the only use of its edge. A mated trim has one edge mate in
a different loop. A seam trim has one edge mate in the same loop.

The source `ON_BrepTrim::Read` switch admits wire trim values 0 through 4 and
leaves values 5, 6, and 7 as runtime `unknown`; `ON_BrepLoop::Read` likewise
admits loop values 0 through 3 and leaves 4 and 5 as runtime `unknown`.
Those reader switches do not change the wire meanings above. CADIR reads the
wire values directly. A curve-on-surface loop (type 4) must contain exactly
one curve-on-surface trim (type 5) and may be open or closed. A point-on-
surface loop (type 5) must contain exactly one point-on-surface trim (type 6).
The point trim has no edge or C2 reference; its runtime surface-parameter
point box is not written by `ON_BrepTrim::Write`, so CADIR transfers the
serialized coincident 3D vertex and does not invent a pcurve. A slit loop
(type 3) is a closed directed ring. Trim value 7 is reserved and source-
invalid; CADIR retains its raw enumeration only when the remaining typed
references and ring invariants are admissible.

### 15.4 Loop and face

Loop:

```
i32 loop index
i32 trim count
trim count × i32 trim index
i32 loop type
i32 face index
```

Loop types are 0 unknown, 1 outer, 2 inner, 3 slit, 4 curve-on-surface, and 5
point-on-surface. Face:

```
i32 face index
i32 loop count
loop count × i32 loop index
i32 surface index
i32 reversed surface
i32 face material channel
```

An explicit neutral outer or inner boundary role selects loop type 1 or 2.
When the role is unspecified, the first loop of a face is outer and the other
loops are inner.

The face array uses packed version 1.1 below archive 70 and 1.2 at archive 70
and later. Version 1.1 appends one UUID per face. Version 1.2 then appends a
color-presence byte and, when nonzero, one `ON_Color` per face.

The `reversed surface` field states whether the face normal is opposite to the
surface normal. A region face-side direction identifies the side that contains
the region and does not replace the face orientation.

Negative material channels map to zero. A vertex, edge, trim, loop, or face
array position is authoritative and replaces a disagreeing stored positional
index. References must be in range and non-null where required. Standard
outer, inner, and slit loop rings must be finite, endpoint-continuous, and
closed. Procedural loop types 4 and 5 use the single-trim rules above instead
of the closed-ring test.

### 15.5 Mesh sides, solid state, and regions

For Brep minor at least 1, render and analysis side chunks each contain one
byte per face; nonzero is followed by a polymorphic object which must be an
`ON_Mesh`. The mesh object wrapper can carry the archive-50
`ON_V5_MeshDoubleVertices` userdata item from section 7.2.3; the same actual
count and exact f64-to-f32 cast checks apply to that nested mesh. These are
cache channels and do not alter Brep topology.
Any archive band can carry the `ON_V4V5_MeshNgonUserData` item from section
7.2.4 on the nested mesh when it remains attached. Its admitted grouping count
contributes the same neutral `mesh.ngon-grouping-dropped` loss; it does not
alter Brep topology.
If a present slot has the wrong class or its bounded mesh payload cannot be
parsed, the slot is discarded independently and the decode report emits
`brep.mesh-cache-degraded` with the diagnostic cause. The Brep remains
admissible when its analytic topology is valid.

For minor at least 2, the writer copies the Brep `m_is_solid` cache to an
`i32` without deriving it. Its OpenNURBS runtime meanings are 0 unset, 1 solid
with outward normals, 2 solid with inward normals, and 3 not solid. The source
reader resets a value below 0 or at least 3 to 0. It also resets the value to 0
when the archive OpenNURBS writer version is before 2 October 2002. Thus 3 is a
source-writable not-solid cache value, not a stable source-read value.

CADIR decision: the parser retains the serialized `i32` in native fidelity and
reports an enumeration degradation for values outside 0 through 2. Neutral
body kind treats 1 and 2 as solid. It treats 0, a pre-2 October 2002 value, and
an invalid value as unset and derives the result from the validated Brep. A
Brep is closed when it has at least one face and every edge has exactly two
trim uses. A closed Brep is solid; another Brep is a sheet. The outward and
inward orientation distinction remains native fidelity because neutral
`BodyKind` has no orientation field.

For archive version 50, minor 2 stores region topology through the temporary
`ON_V5_BrepRegionTopologyUserData` item in section 7.2.5 when the topology
passes the writer's face-side count gate. Archive version 40 has no automatic
region-topology carrier. For minor at least 3, the region wrapper is anonymous
major-1, followed by a presence byte and, when present, a major-1
region-topology object. The inline object and the section 7.2.5 item use the
same face-side and region arrays: before archive 60, arrays contain raw
anonymous element chunks; at archive 60 and later, arrays contain polymorphic
objects.
When the optional region topology fails its face-side or element invariants, the
complete optional region topology is discarded and the decode report emits
`container.redundant-field-repaired` with the diagnostic cause; the Brep remains
admissible.

CADIR decision: when an inline minor-3 topology does not assign exactly one
bounded region to each face, neutral shell grouping falls back to edge
incidence and the decode report emits
`Brep 3.3 region topology was not representable; incidence-derived shells used`.
The native region records remain retained.

Face side:

```
i32 face-side index
i32 region index
i32 face index
i32 surface-normal direction (+1 or -1)
```

Region:

```
i32 region index
i32 region type (0 infinite, 1 bounded)
i32 face-side count
face-side count × i32 face-side index
ON_BoundingBox
```

There are exactly `2 * face_count` face sides. Positions `2*f` and `2*f+1`
correspond to face `f` with directions +1 and -1. There is exactly one
infinite region; region membership is reciprocal, face sides are not duplicated
within a region, and unassigned sides use region index -1.

Every reference, domain, reversal, ring, and region satisfies the invariants
above. For minor 3, serialized region membership agrees with face-edge
incidence.

## 16. Extrusion

`ON_Extrusion` uses an anonymous chunk version `(i32 major, i32 minor)`.
Writers emit versions 1.0 through 1.3. A reader accepts major 1 with any
nonnegative minor, consumes the fields gated by the known minor values, and
skips a later suffix before the bounded class-data end. The common fields are:

```
polymorphic ON_Curve profile
ON_Line path
ON_Interval trim interval
ON_3dVector up
bool miter-normal-present[2]
ON_3dVector miter-normal[2]
ON_Interval path domain
bool transposed
```

For archive versions below 60, the source writer emits minor 2; for archive
version 60 and later it emits minor 3. `ON_Extrusion` is a V5 class and is not
written as that class in V4: a capped or multi-profile value is translated to
`ON_Brep`, and an uncapped single-profile value is translated to
`ON_SumSurface` or `ON_NurbsSurface`.

Source validity requires a positive profile count and a profile object, a
finite nonzero path, `0 <= trim[0] < trim[1] <= 1`, a unit `up` perpendicular
to the path, and unit present miter normals whose local-Z component is greater
than `1/64`. A nil profile wrapper is readable, but it yields profile count
zero and is source-invalid. CADIR retains that bounded source record instead
of admitting typed extrusion geometry. A typed extrusion also requires the
profile to have an exact NURBS representation and to lie in the profile plane.

Miter vectors are serialized even when their presence flags are false. Minor
1 appends `i32 profile count`. Minor 2 appends bottom and top cap booleans.
Minor 3 appends an anonymous mesh-cache chunk. The complete 1.3 order is the
common fields, profile count, two caps, and mesh cache. The cache is anonymous
version 1.0. It contains zero or more entries, each marked by `u8 1` and
followed by an anonymous version-1.0 entry containing a mesh UUID and a
polymorphic `ON_Mesh` wrapper; `u8 0` terminates the entry sequence. The mesh
wrapper's class-userdata stream is part of that nested mesh and uses the
owning-mesh rules in section 7.2.3.

For a minor-2 writer that saves display meshes, the mesh cache is instead a
temporary class-userdata item. Its class UUID and item UUID are
`A8130A3E-E4F3-4CB0-BB8A-F10A473912D0`, and its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`. The body of its anonymous userdata
child contains exactly three polymorphic object wrappers in this order:

```text
render mesh object
analysis mesh object
null object
```

The first two objects are null or `ON_Mesh`; a non-null wrong class rejects the
optional cache. The third object is consumed and discarded. A null object
wrapper is an `OPENNURBS_CLASS` long chunk containing only a class-UUID child
whose UUID is nil; it has no class-data or class-end child. A non-null nested
mesh carries the section 7.2.3 userdata item, and its accepted double vertices
are transferred to the mesh tessellation before document-unit scaling. The
cache reader consumes these three slots and skips any remaining bytes through
the bounded userdata payload end. The cache is display data and does not alter
analytic extrusion geometry. A malformed cache is discarded while the
extrusion remains admissible.

A present miter normal is unitized. The miter applies only when the unitized
local Z component is greater than `1/64`. A normal that cannot be unitized or
does not exceed this threshold selects the flat cap transform.

Profile and path coordinates are document lengths and receive document-unit
conversion. `up` and miter normals are directions and do not scale. Trim and
path-domain values, profile knots and rational weights, transpose, and cap
flags do not scale. The trimmed path endpoints define the two cap origins;
the lateral carrier uses the path-domain interval and the ordered profile
coordinates. The profile remains the directrix child of each lateral.

For minor below 1, profile count defaults to one when a profile exists and
zero otherwise. For minor below 2, closed outer profiles default both caps to
true; otherwise both defaults are false. The mesh cache is display data and is
not analytic extrusion geometry.

A multiple-profile extrusion stores a polycurve whose segment count is the
profile count. Profile segments are closed and contain one outer segment
followed by inner segments in a common profile plane. The profile and every
polycurve segment are two-dimensional curve payloads in extrusion profile
coordinates. Profile coordinates use document length conversion before the
profile frame places them at the trimmed path endpoints.

The profile orientation is the sign of its oriented area in the profile plane.
For two evaluated coordinates `a` and `b`, point coincidence means
`abs(a-b) <= 2^-32` or `abs(a-b) <= (abs(a)+abs(b))*2^-42`. A nonperiodic
boundary is closed when its endpoints are coincident and neither the one-third
nor two-thirds parameter point is coincident with either endpoint. A periodic
NURBS boundary is closed when its knot vector is periodic and its duplicated
degree-many control points are pairwise coincident. Analytic and compound
profile curves use their exact NURBS representation for these tests.

The oriented-area accumulator visits every nonempty curve span. It uses one
sample per span for degree at most 1, four samples per span for degree 2 or 3
(doubling the count until the total is at least 17), and the degree as the
sample count for degree at least 4. Samples are taken at fractions `j/n` for
`j = 0 .. n-1`, followed by the final endpoint. For each consecutive pair
`(x0,y0)` and `(x1,y1)`, it adds
`(x0-x1)*(y0+y1)` and then divides the sum by two. Compound curves sum the
area of each segment in order. Positive area is the outer orientation and
negative area is the inner orientation. Zero area has orientation zero. A
single open or zero-area outer boundary is valid only without caps. Multiple
boundaries must be closed and must have the outer-then-inner orientation
sequence above.

## 17. SubD

`ON_SubD` begins with a one-byte SubDimple presence flag. Zero is empty; one is
followed by an anonymous SubDimple chunk. SubDimple uses anonymous major 1.
Minor 0 is the base payload; later minors append fields. V5/V6 use minor 0;
V7/V8 use minor 4.

For archive versions below 60, `WriteObject` serializes a runtime SubD as an
`ON_Mesh` control-net proxy. V3 through V5 include the section 7.2.6 userdata
item; V1 and V2 suppress class-userdata items. For archive version 60 and
later, `WriteObject` serializes the runtime object as this direct `ON_SubD`
class. The embedded SubD in a proxy uses this same payload grammar.

SubDimple fields:

```
u32 level count
u32 obsolete maximum vertex ID
u32 obsolete maximum edge ID
u32 obsolete maximum face ID
ON_BoundingBox obsolete global bounds
level count × level chunk
minor >= 1:
  u8 obsolete texture-domain type
  mapping tag
minor >= 2: symmetry record
minor >= 3: u64 legacy geometry serial
minor >= 4:
  bool symmetric
  UUID face-packing ID
  bool synchronize packing hash serials
  face-packing topology hash record
```

The symmetry record stores a type byte. Type 2 is rotate symmetry and type 113
is its prototype form. The nested transform chunk stores the rotation axis as
two `ON_3dPoint` values. For nested minor version at least 2, type 2 follows the
axis with four ignored `f64` values. Writers store four quiet NaNs. Type 113
omits these values.

Each level is anonymous version 1.1:

```
u16 level index
u8 4, u8 4, u8 4
ON_BoundingBox control-net bounds
u32 p0, p1, p2, p3 archive-ID partitions
vertices [p0,p1)
edges [p1,p2)
faces [p2,p3)
u8 render-mesh-present
```

Archive IDs are contiguous, one-based, partitioned vertex/edge/face, and
records occur in archive-ID order. Level zero is the control cage.

A pointer in a vertex, edge, face, or saved-limit-point field is `u32 archive
ID` followed by `u8 flags`. The field determines the component type, and bit 0
is the only serialized flag. Archive ID zero is null and has flags zero. Edge
and face direction bits reverse traversal. Generic component pointers in
size-tagged additions use bits 1 and 2 for type: `0x2` vertex, `0x4` edge, and
`0x6` face.

Each component base has archive ID, component ID, subdivision level, then
pre-V7 saved point/vector fields or V7+ size-tagged additions. Vertex records
contain tag, 3D control point, incident edge/face counts, saved limit points,
edge pointers, face pointers, and a V5/V6 zero end marker. Tags are 0 unset,
1 smooth, 2 crease, 3 corner, and 4 dart.

Edge records contain tag, face count, two sector coefficients, start
sharpness, two vertex pointers, face pointers, the pre-V7 zero marker, and in
V8 an optional eight-byte end sharpness addition. V5 through V7 map scalar
sharpness to both endpoints; V8 stores `[start,end]`. Edge tags are 0
unset, 1 smooth, 2 crease, and 4 smooth-X.

Face records contain level-zero ancestor ID, obsolete parent ID, directed edge
count and edge pointers, then pre-V7 zero marker or V7+ additions including
packing rectangle, material channel, color, pack ID, custom texture points, and
end marker 255. Face rings have at least three uses, valid edges, endpoint
continuity, and closure.

## 18. Instance definitions and references

Instance-definition records are in the instance-definition table. Archive
versions through 50 use packed major version 1, minor 6. An archive-60 legacy
record may use packed minor 7; archive versions 60 and later otherwise use an
anonymous major-1 V6 payload. The packed field order is:

```
definition UUID
member object UUID array
name
description
URL
URL tag
bounding box
u32 definition type
linked-file path
minor >= 1: linked checksum
minor >= 2: unit system
minor >= 3: meters per unit and relative-path bool
minor >= 4: unit-system detail
minor >= 5: nested linked-definition depth
minor >= 6: linked component appearance
minor >= 7: file-reference presence and record
```

The unit-system detail replaces the earlier unit fields. A version-1.7 reader
skips all bytes after the optional file-reference record as an abandoned tail.

For a linked definition, the legacy relative-path Boolean makes the
serialized path the relative path; otherwise it is the full path. A present
`ON_OBSOLETE_IDefAlternativePathUserData` item follows the bounded update rule
in section 7.2.7 after the V5 class-data prefix.

V6 through V8 use anonymous major-1 payloads:

```
model-component attributes
u32 definition type
unit-system detail
description
URL
URL tag
bounding box
bool member UUID list present
if present: member UUID array
bool linked type
if linked: anonymous linked-type major-1 payload
```

The linked-type chunk contains file reference, nested depth, linked appearance,
reference-component-settings presence, and optional settings. Static and
linked-and-embedded definitions carry member UUIDs; linked external definitions
normally do not.

`ON_InstanceRef` writers emit packed version 1.0. Its reader requires major
version 1 and does not assign meaning to the packed minor:

```
definition UUID
ON_Xform transform
ON_BoundingBox bounds
```

Definition membership comes from the ordered definition UUID array, not object
attributes. The reference payload carries one transform and one bounding box.
The known prefix ends at the enclosing instance-reference class-data boundary;
later bytes remain bounded there.
The transform applies to the definition as a whole. When transfer of one
member emits more than one typed geometry carrier, the same transform is applied
to every carrier emitted from that member, including a body carrier and any
point, curve, surface, mesh, or subdivision carrier. The wire format has no
per-carrier transform or carrier-selection field.

### 18.1 Modern dimensions

The linear, angular, and radial dimension class payloads use an anonymous
version 1.0 family chunk. Its first child is an anonymous common-dimension
chunk with major version 1 and minor version 0 or 1:

```text
anonymous common dimension version 1.minor
  annotation
  UTF-16 user text
  f64 obsolete text rotation
  bool use default text point
  ON_2dPoint user text point
  bool flip first arrow
  bool flip second arrow
  i32 arrow fit
  UUID detail measured
  f64 distance scale
  if minor >= 1: i32 text fit
family fields
```

Arrow fit 0 is automatic, 1 forces arrows inside, and 2 forces arrows outside.
The neutral arrow-position values are 0, 1, and -1 respectively.

The annotation is an anonymous chunk with major version 1 and minor versions
0 through 4:

```text
anonymous annotation version 1.minor
  anonymous text-content version 1.0
  UUID dimension style
  ON_Plane plane
  if minor >= 1: i32 annotation type
  if minor >= 2:
    anonymous override version 1.1
      bool override present
      if present: class wrapper for the override dimension style
  if minor >= 3: ON_2dVector horizontal direction
  if minor >= 4: bool allow text scaling
```

Annotation versions before 3 use horizontal direction `(1,0)`. Annotation
versions before 4 allow text scaling. The text-content body is a UTF-16 rich
text string, obsolete plane, rectangle width `f64`, rotation `f64`, horizontal
alignment `i32`, vertical alignment `i32`, obsolete text height `f64`, and
wrap `bool`, in that order.

The text-content, annotation, common-dimension, and family chunks are
independent boundaries. Each reader consumes its known prefix and ends its
own anonymous child. Bytes after a known prefix remain bounded by that child;
bytes after the family chunk remain direct bytes in the enclosing
`TCODE_OPENNURBS_CLASS_DATA` chunk. They are not fields of the next dimension
family.

Linear family fields are definition point and dimension-line point as two
`ON_2dPoint` values. Annotation types 1 and 5 select linear dimensions. The
measurement is `abs(definition_point.x) * distance_scale`.

Angular family fields are two `ON_2dVector` directions, two `f64` extension
offsets, and an `ON_2dPoint` dimension-line point. Annotation types 2 and 11
select angular dimensions. The measured sweep is the counterclockwise angle
from the first direction to the second direction. The dimension-line point does
not select the measured arc.

Radial family fields are radius point and dimension-line point as two
`ON_2dPoint` values. Annotation type 3 selects diameter and type 4 selects
radius. The measurement is the radius-point magnitude times distance scale,
and diameter multiplies that result by two.

Ordinate family fields are:

```text
i32 measured direction
ON_2dPoint definition point
ON_2dPoint leader point
f64 first kink offset
f64 second kink offset
```

Annotation type 6 selects ordinate. Measured direction 1 measures plane x and
2 measures plane y. Zero infers x when the absolute leader displacement in x
does not exceed its displacement in y, and otherwise infers y. Measurement is
the absolute selected definition-point coordinate times distance scale.

`ON_Centermark` uses the same anonymous family and common-dimension chunks.
Annotation type 8 selects center mark. Its family suffix is one nonnegative
`f64` radius. The radius uses document length conversion. Center marks have no
measured dimension value; the radius controls the persisted mark geometry.

All points and distance-valued fields use document length conversion.
Directions, angles, and distance scale are unscaled. Coordinates, scales, and
computed measurements are finite; distance scale is positive.

The legacy V5 linear, angular, and radial dimension classes use an anonymous
version 1.0 family chunk. Linear and radial families contain one common legacy
annotation child. Angular dimensions append an `f64` angle and `f64` radius.
The common annotation is anonymous version 1.0 through 1.3:

```text
i32 annotation type
i32 text display mode
ON_Plane plane
archive array of ON_2dPoint construction points
UTF-16 displayed text
i32 user-positioned-text flag
i32 initial archive dimension-style index
f64 text height
i32 justification
if minor >= 1: bool allow model-space text scaling
if minor >= 2: UTF-16 text formula
if minor >= 3:
  i32 archive text-style index
  i32 archive dimension-style index
```

Linear annotation type 1 is rotated and type 2 is aligned. Its five points are
the first extension endpoint, first arrow, second extension endpoint, second
arrow, and user text point. The first extension endpoint becomes the defining
plane origin. The second endpoint and arrow midpoint define the dimension and
dimension-line points relative to that origin. Both types measure the absolute
plane-x difference. The neutral annotation types are rotated 5 and aligned 1.

Angular annotation type 3 has four archived points, but its geometry comes from
the stored angle and radius. The first direction is `(1,0)`, the second is
`(cos(angle),sin(angle))`, and the dimension-line point is the radius at half
the angle. The stored angle is the measurement. Its neutral annotation type is
2.

Radial annotation type 4 is diameter and type 5 is radius. Its four points are
center, arrow, tail, and knee. The center becomes the defining plane origin;
arrow and tail become relative radius and dimension-line points. Their neutral
annotation types are 3 and 4 respectively.

When the common annotation has no scaling field, model-space text scaling is
false. Minor 3 selects the dimension-style index from the dimension-style slot
for dimensions. For text, it selects the text-style slot first, then the
initial slot, then the dimension-style slot. A text annotation with zero
justification becomes top-left, and its plane origin moves by one text height
along the plane y axis. V5 text, leader, and ordinate annotation types map to
neutral types 9, 10, and 6.

The legacy V5 ordinate class has an anonymous version 1.0 or 1.1 family chunk.
Its first child is an anonymous version 1.0 wrapper containing the common
legacy annotation. The suffix is:

```text
i32 measured direction
if family minor >= 1:
  f64 first kink offset
  f64 second kink offset
```

Legacy annotation type 8 selects ordinate and has definition and leader points
in that order. Direction 0 measures x, 1 measures y, and -1 uses the same
leader-displacement inference rule as a modern unset direction. The plane
origin is the ordinate reference and measurement is the absolute selected
definition-point coordinate.

Legacy linear, radial, and ordinate dimensions may carry userdata class and
item UUID `8AD5B9FC-0D5C-47FB-ADFD-74C28B6F661E`. Its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`. The userdata payload is the body of
the outer anonymous userdata child from section 7.2. The body contains one
class-owned anonymous major-1 child. The writer emits minor 2. Its payload is:

```text
anonymous version 1.2
UUID dimension using this extension
i32 forced arrow position (-1 outside, 0 automatic, 1 inside)
i32 text rectangle count
if count = 7: 28 × i32 rectangle coordinates
if minor >= 1: f64 distance scale
if minor >= 2: UUID detail measured
```

The extension UUID is nil when no dimension identity is assigned. The rectangle
count is zero or seven; seven selects seven four-integer display rectangles.
Distance scale is finite and positive. It is the page-space distance divided by
the measured model-space distance for a detail-view dimension and multiplies
the model-space measurement for display. Detail measured identifies the detail
view; nil means that the dimension does not measure detail model space. Minor 0
omits distance scale and detail measured, whose defaults are `1.0` and nil.
The model-space base point stored by the V5 extension object is runtime-only and
has no serialized field.

The V5 dimension-extension reader ends its class-owned anonymous version-1
child. Bytes after that child remain in the enclosing anonymous
`TCODE_OPENNURBS_CLASS_USERDATA` payload. The angular extension uses the same
boundary rule: its class-owned anonymous version-1.0 child contains the two
extension-line origin offsets, and later bytes remain at the enclosing userdata
boundary.

Saving a V6 annotation to archive versions 3 through 50 converts it to a V5
linear, radial, or ordinate annotation and serializes this carrier. Saving a
V6 annotation to archive version 60 and later writes the direct V6 annotation
and does not create this carrier. Dimension plane origins, plane equation
offsets, construction points,
angular radius, kink offsets, and text height use document length conversion.
Style indices, flags, directions, stored angles, and distance scale remain
unscaled. CADIR maps forced arrow position, distance scale, and detail measured
to the dimension. The extension UUID and display rectangles have no neutral
fields and remain in the retained source record. When duplicate records for an
attached built-in dimension extension occur, the first serialized matching
record owns the extension state.

### 18.2 Hatches

`ON_Hatch` uses packed version 1.1 before archive 60 and packed version 1.2 in
archive 60 and later:

```text
packed version 1.minor
ON_Plane hatch plane
f64 pattern scale
f64 pattern rotation
i32 referenced hatch-pattern archive index
i32 loop count
loop count × hatch loop
if minor >= 2: ON_2dPoint basepoint
```

Each hatch loop uses packed version 1.1:

```text
packed version 1.1
i32 loop type
class wrapper containing one plane-space curve
```

Loop type 0 is outer and type 1 is inner. The loop curve is a two-dimensional
curve in hatch-plane coordinates. Model-space loop point `p=(x,y,0)` is
`plane.origin + x*plane.xaxis + y*plane.yaxis`. Plane origin, loop coordinates,
and basepoint coordinates use document length conversion. Plane axes, pattern
scale, and pattern rotation are unscaled. Pattern scale is finite and positive;
pattern rotation and every geometric coordinate are finite. Loop count is
nonnegative and every loop object derives from the curve family.

In archive 50, a nonzero base point for a hatch below minor 2 is in
`ON_OBSOLETE_V5_HatchExtra` userdata class and item UUID
`3FF7007C-3D04-463F-84E3-132ACEB91062`. Its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`. The userdata payload is the body of
the outer anonymous userdata child from section 7.2. The body contains one
class-owned anonymous major-1, minor-0 child:

```text
anonymous version 1.0
UUID ignored_id
ON_2dPoint basepoint
```

`ON_Hatch::Write` attaches this temporary carrier only for archive version 50
when the minor-1 hatch has a valid nonzero base point. Archive versions below
50 do not receive this automatic carrier; archive version 60 and later write
the base point in the inline minor-2 hatch field instead. The obsolete hatch
extension is consumed after reading: each valid matching record applies its
base point in serialized order, so the last valid record owns the hatch base
point, and no such extension remains attached to the hatch. CADIR maps the
consumed base point to the hatch basepoint and retains no separate userdata
field.

For archive versions below 50, a nonzero base point has no serialized carrier;
the CADIR basepoint is therefore `[0,0]`. For every accepted hatch object,
CADIR creates one native hatch feature and one linked neutral curve for each
ordered loop. The feature retains the pattern archive index, pattern scale,
pattern rotation, and converted basepoint as native parameters. The hatch
pattern is not converted into a neutral filled region; the source object record
remains retained for native fidelity and this boundary emits the
`hatch.fill-not-transferred` warning.

Archive version 60 and later may attach `ON_GradientColorData` class userdata
to an `ON_Hatch`. The userdata class UUID and item UUID are
`0C1AD613-4EFA-4F47-A147-4D79D77FCB0C`. Its application UUID is
`7B0B585D-7A31-45D0-925E-BDD7DDF3E4E3`. The class userdata header and its
outer anonymous child use section 7.2. The outer anonymous body contains one
long `TCODE_ANONYMOUS_CHUNK` (`0x40008000`) major-1 chunk:

```text
anonymous gradient-data version 1.0
i32 gradient type
ON_3dPoint gradient start
ON_3dPoint gradient end
f64 repeat
i32 color-stop count
color-stop count × color stop
```

Gradient type values are 0 none, 1 linear, 2 radial, 3 disabled linear, and
4 disabled radial. A color stop is a long `TCODE_ANONYMOUS_CHUNK`
(`0x40008000`) major-1 chunk whose body is:

```text
ON_Color RGBA bytes
f64 stop position
```

`ON_Color` is four raw bytes in red, green, blue, alpha order. The source
writer emits the color-stop count as a nonnegative `i32`; the codec admits a
nonnegative count only when every bounded stop fits the userdata payload and
the allocation cap. The class reader requires major version 1, accepts only
gradient types 0 through 4, consumes each stop at its child boundary, and
closes the gradient-data chunk at its own boundary. Gradient start and end
coordinates use document length conversion. Repeat and stop positions are
unitless.

The CADIR decision is to expose a recognized gradient userdata item as the
hatch feature's native `gradient` parameter. Its JSON value contains the
gradient type name and numeric value, scaled start and end points, repeat, and
ordered RGBA stops. The hatch fill is still not rendered as neutral fill
geometry; the source object record remains retained for native fidelity.

### 18.3 Detail views

`ON_DetailView` writers emit an outer anonymous chunk packed version 1.1. The
reader requires major version 1 and admits nonnegative minors. Its two child
wrappers are anonymous chunks packed version 1.0:

```text
anonymous detail version 1.minor
  anonymous view-state version 1.0
    ON_3dmView payload
  anonymous boundary version 1.0
    raw ON_NurbsCurve payload
  if minor >= 1: f64 page-per-model ratio
```

At outer minor 0 the page-per-model ratio is absent and has value zero. At
minor 1 and later the ratio follows the boundary child. Bytes after the known
prefix remain bounded by the enclosing anonymous chunk.

The child declarations independently bound extensible view state and boundary
geometry. Each child reader consumes its known prefix and skips remaining bytes
before that child's bounded end. The boundary NURBS curve uses the ordinary
two-dimensional NURBS curve layout without a class wrapper. Its control points
are page-layout coordinates in millimeters and are not multiplied by the model
document scale. The page-per-model ratio is finite, nonnegative, and
dimensionless; detail minor 0 defaults it to zero.

CADIR creates one native `detail_view` feature and one linked neutral curve for
each accepted detail object. The feature retains the page-per-model ratio and
the boundary link. It retains the bounded `ON_3dmView` payload as `view_bytes`
and `view_sha256` native properties; the view state is not decoded into the
neutral `views` arena. This is a CADIR transfer boundary and emits
`detail.view-not-transferred`; the source detail object remains retained for
native fidelity.

### 18.4 NURBS cages

`ON_NurbsCage` writers emit anonymous version 1.0. A reader accepts major 1
with any nonnegative minor and skips a later suffix before the bounded
class-data end:

```text
i32 dimension
i32 rational flag
i32 order[3]
i32 control_count[3]
(order[0] + control_count[0] - 2) × f64 U knots
(order[1] + control_count[1] - 2) × f64 V knots
(order[2] + control_count[2] - 2) × f64 W knots
for u in 0..control_count[0]:
  for v in 0..control_count[1]:
    for w in 0..control_count[2]:
      (dimension + rational) × f64 control value
```

Dimension is positive and at most 10000. The rational flag is zero or one.
Each order is at least two, and each control count is at least its order.
Knots are finite and nondecreasing independently in all three directions.
Nonrational control values are Euclidean coordinates. Rational control values
are homogeneous coordinates followed by a finite nonzero weight; Euclidean
coordinates divide by that weight. Coordinates use document length conversion.
Knot values and weights are unscaled.

When a cage is nested in a morph-control end chunk, the cage's anonymous
chunk is its own boundary. A cage suffix ends at that child boundary and does
not consume the enclosing morph-control fields.

CADIR creates one native `nurbs_cage` feature for each accepted cage. Its
parameters retain dimension, rational form, orders, and control counts; its
native properties retain the three knot vectors, converted control points, and
rational weights. Knot values and weights remain unscaled. The cage is not
converted into a neutral lattice, so the source object remains retained for
native fidelity and this boundary emits `cage.lattice-not-transferred`.

### 18.5 Morph controls

`ON_MorphControl::Write` emits anonymous version 2.1. Its version-2 reader
accepts every nonnegative minor, consumes the fields below, and skips later
minor bytes at the outer anonymous boundary. Version 1 is the legacy cage
form; its reader also consumes the known prefix through the outer boundary.
Only major versions 1 and 2 have a payload grammar. Other major versions are
rejected.

The modern payload is:

```text
i32 variant
anonymous start-control version 1.0
anonymous end-control version 1.0
anonymous captive-UUID-list version 1.0
anonymous localizer-list version 1.0
if minor >= 1:
  f64 tolerance
  bool quick preview
  bool preserve structure
```

Variant 1 stores a raw `ON_NurbsCurve` in each control chunk. Variant 2 stores
a raw `ON_NurbsSurface` in each control chunk. Variant 3 stores an `ON_Xform`
in the start chunk and one complete anonymous `ON_NurbsCage` in the end chunk.
The UUID list is an anonymous major-1 chunk containing an archive array of
UUIDs. `ON_UuidList::Write` sorts the active UUIDs before writing; the reader
also normalizes the list after reading. The localizer list is an anonymous
major-1 chunk containing `i32 count` followed by that many localizer chunks.

Each localizer is anonymous version 1.0:

```text
i32 type
ON_3dPoint point
ON_3dVector vector
ON_Interval distances
anonymous optional-curve version 1.0
  bool present
  if present: raw ON_NurbsCurve
anonymous optional-surface version 1.0
  bool present
  if present: raw ON_NurbsSurface
```

Localizer types are 0 none, 1 sphere, 2 plane, 3 cylinder, 4 curve, 5 surface,
and 6 distance. The OpenNURBS reader maps an unrecognized type value to type
0. Control points, localizer points, localizer distance intervals, transform
translation coefficients, and tolerance use document length conversion.
Vectors, transform linear coefficients, knots, parameters, and weights are
unscaled. Tolerance is finite and nonnegative.

Legacy morph-control major version 1 is the cage variant. Its field order is a
complete NURBS cage, captive UUID list, and start `ON_Xform`. It has no
localizers and defaults tolerance and both option flags to zero or false.

CADIR creates one native `morph_control` feature. Its parameters retain the
variant, sorted captive UUIDs, tolerance, quick-preview flag, preserve-
structure flag, and any resolved captive-object links. Variant 1 retains the
start and end NURBS curves; variant 2 retains the start and end NURBS surfaces;
variant 3 retains the start transform and end cage. Each localizer retains its
type, point, vector, distance interval, and optional curve or surface. The
deformation is not applied to captive objects: the source morph object remains
retained for native fidelity and emits `morph.deformation-not-applied`.

### 18.6 Persistent polyedge references

CADIR does not create neutral geometry for an `ON_PolyEdgeCurve`. It creates
one native `polyedge_reference` feature. Its `construction` property retains
the polycurve parameter array and, in segment order, each referenced object
UUID, component type and index, edge and trim domains, proxy-reversal byte,
polyedge segment domain, and referenced-curve domain. A uniquely resolved
segment UUID becomes `segment_N_object` in the feature parameters and links
the feature to that source object record. A missing or ambiguous UUID leaves
that parameter absent and emits the corresponding reference loss.

CADIR retains the polyedge source record and emits
`polyedge.references-not-resolved` because the construction is not a neutral
binding, even when every segment UUID resolves to a source record. The
polyedge parameter array and all segment domains remain numeric construction
values; CADIR does not apply document-length scaling to them. Referenced
carrier geometry is scaled by its own geometry rule.

## 19. Exact gates and invariants

This section collects exact version gates, field widths, and invariants for
built-in payload families.

### 19.1 Point and simple-curve gate table

| Class              | Framing     | Written version | Accepted major/minor                                       | Required invariants                                                                       |
| ------------------ | ----------- | --------------- | ---------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `ON_Point`         | packed byte | 1.0             | major 1; minor ignored                                     | three finite coordinates                                                                  |
| `ON_PointCloud`    | packed byte | 1.2             | major 1; minor 0/1/2 gates arrays                          | positive point count; finite points; bounded nonnegative channel counts                  |
| `ON_LineCurve`     | packed byte | 1.0             | major 1; minor ignored                                     | finite distinct endpoints; increasing domain; dimension 2 or 3                            |
| `ON_ArcCurve`      | packed byte | 1.0             | major 1; minor ignored                                     | positive radius; finite plane; increasing angle and curve domains                         |
| `ON_PolylineCurve` | packed byte | 1.0             | major 1; minor ignored                                     | at least two points; parameter count equals point count; strict parameter increase        |
| `ON_PolyCurve`     | packed byte | 1.0             | source reader fixed layout; CADIR admits major 1            | positive segment count; parameter count is segment count plus one; every child is a curve |

`ON_LineCurve` and `ON_PolylineCurve` serialize their `i32 dimension` without
normalizing invalid values. `ON_ArcCurve` normalizes a dimension other than 2
or 3 to 3. `ON_Point` has no dimension field.

The point-cloud optional arrays are exactly:

```text
minor 0: points, plane, bounding box, flags
minor 1: i32 normal_count, normal_count × ON_3dVector,
         i32 color_count, color_count × ON_Color
minor 2: i32 value_count, value_count × f64
```

The point-cloud `flags` bits are bit 0 ordered stream and bit 1 plane set.
Point-cloud point count is positive. Optional array counts are zero or equal
to the point count.

A degree-one knot vector corresponding to a bounded line is
`[t0,t0,t1,t1]`. For polyline parameters `t[0..n)`, the corresponding knot
vector is `[t0,t0,t1,...,t[n-2],t[n-1],t[n-1]]`. A polycurve has
`segment_parameters.len() == child_count + 1`.

### 19.2 NURBS acceptance table

| Class   | Version gate                             | Counts                                                                   | Domain                                                |
| ------- | ---------------------------------------- | ------------------------------------------------------------------------ | ----------------------------------------------------- |
| curve   | packed 1.0 before archive 60; 1.1 at 60+ | `stored_knots = order + cv_count - 2`; stored CV count equals `cv_count` | `[K[order-2], K[cv_count-1]]`                         |
| surface | packed 1.0                               | U/V stored counts `order + count - 2`; CV count `u_count*v_count`        | each direction uses its own interior stored endpoints |

Order is at least 2, CV count is at least order, dimensions are positive,
knots are finite and nondecreasing, and every rational weight is finite and
nonzero. The SubD-friendly curve tag is one byte. Periodicity is derived from
the reconstructed knot vectors.

For rational curves and surfaces, each wire CV has `dimension+1` doubles:
`[xw,yw,zw,w]`; the Euclidean pole is `[xw/w,yw/w,zw/w]`. Surface flat wire
order is `i * v_count + j`.

### 19.3 Mesh channel and minor table

The mesh version is a packed byte. Writer bands are:

| Archive band | Written mesh version |
| ------------ | -------------------- |
| 50           | 3.5                  |
| 60, 70, 80   | 3.8                  |

Archive band 50 admits major-3 mesh minors through 5. A larger minor in that
band is a writer-band mismatch. In an admitting archive band, a larger minor
uses the monotonic gates above and leaves later fields as a bounded suffix.

Major 1 uses raw arrays. Major 2 has no defined payload layout. Major 3 uses
five compressed buffers after the face array. Every buffer is present in
sequence, including a zero-size absent channel.

| Channel             | Element encoding                      | Expected nonzero uncompressed size |
| ------------------- | ------------------------------------- | ---------------------------------: |
| vertices            | `vertex_count × 3 × f32`              |                `vertex_count * 12` |
| normals             | `vertex_count × 3 × f32`              |                `vertex_count * 12` |
| texture coordinates | `vertex_count × 2 × f32`              |                 `vertex_count * 8` |
| curvature           | `vertex_count × 2 × f64`              |                `vertex_count * 16` |
| colors              | `vertex_count × 4` direct color bytes |                 `vertex_count * 4` |
| surface parameters  | `vertex_count × 2 × f64`              |                `vertex_count * 16` |

The first five channels use the compressed-buffer protocol. A nonzero channel
whose size differs from its expected size is invalid. Vertex and face count are
nonnegative; each face index is less than vertex count. The explicit
index-width field is 1, 2, or 4 and matches the selection from vertex count.

Major 3 gates are monotonic:

```text
minor >= 2: i32 packed texture rotation
minor >= 3: UUID texture mapping ID, compressed surface parameters
minor >= 4 and writer version >= 200606010: mapping-tag chunk
minor >= 5: u8 manifold, u8 oriented, u8 solid
minor >= 6: u8 ngon-present, optional ngon chunk
minor >= 7: u8 double-vertices-present, optional double-vertex chunk
minor >= 8: ON_BoundingBox vertex bounds
```

The three minor-5 values are tri-state bytes: zero means unset, one means
false, and two means true. Other values are invalid. They are not ordinary
booleans.

The mapping tag is an anonymous major-1 chunk. Minor 1 adds the mapping type:

```
UUID mapping ID
i32 mapping CRC
16 × f64 mesh transform
minor >= 1: u32 mapping type
```

The ngon chunk is anonymous major-1:

```
u32 ngon_count
repeat ngon_count:
  u32 boundary_vertex_count
  if boundary_vertex_count != 0:
    u32 face_count
    boundary_vertex_count × u32 vertex index
    face_count × u32 mesh-face index
```

Each vertex index is less than mesh vertex count and each mesh-face index is
less than mesh face count. The double-vertex chunk is anonymous major 1:

```
u32 double_vertex_count
if double_vertex_count != 0:
  compressed buffer of double_vertex_count × 3 × f64
```

The double-vertex count equals mesh vertex count and every value is finite.
Vertex and face indices are in range.

Archive versions 4 and 5 do not have the inline minor-6 n-gon chunk. Their
legacy n-gon grouping, when present, is the section 7.2.4 class-userdata item.
That class-userdata item can also persist in a later archive when it remains
attached and all userdata is serialized.

### 19.4 Brep framing and version table

The Brep class-data payload starts with packed version `3.minor`. C2, C3, and
surface arrays are anonymous version 1.0 wrappers:

```
i32 count
repeat count:
  i32 present
  if present == 1: polymorphic object
```

`present == 0` is a positional null slot. No other presence value is valid.
The slot remains addressable by its array index. A topology record referencing
a null slot is invalid.

Vertices, edges, trims, loops, and faces are separate anonymous version 1.0
raw-array wrappers:

```
packed version byte 0x10
i32 count
count × inline record
```

They have no per-record version byte. The face wrapper uses packed 1.1 before
archive 70 and packed 1.2 at archive 70+. The suffixes are:

```
face-array minor >= 1: count × UUID face IDs
face-array minor >= 2:
  u8 per-face-color-present
  if nonzero: count × ON_Color
```

For Brep minor at least 1, each mesh-side wrapper is anonymous and starts with
exactly `face_count` entries. There is no version byte before face zero:

```
u8 present
if nonzero: polymorphic object
```

The first wrapper is render mesh and the second analysis mesh. A nonzero entry
must contain `ON_Mesh`; wrong-type entries are discarded independently. Its
class wrapper can contain the section 7.2.3 V5 mesh double-vertex userdata;
that userdata is decoded against the nested mesh's float vertex channel.
The same wrapper can contain section 7.2.4 legacy mesh n-gon userdata in any
archive band; its validated record count is reported as dropped neutral
grouping when no inline n-gon count takes precedence.

For Brep minor 2 in archive version 50, the region-topology item in section
7.2.5 is a class-userdata child of the Brep class wrapper. Its payload is one
anonymous version 1.0 region-topology object, followed by the class-userdata
and class-end framing. The automatic item is present only when the Brep has a
region topology, at least one face, and exactly `2 * face_count` face sides.
For Brep minor 3, the region wrapper is anonymous version 1.1, contains a
one-byte region-topology-present flag, and then one anonymous version 1.0
region-topology object when present. In both carriers, each face-side and
region array is anonymous version 1.0 with `i32 count`, has no per-entry
presence integer, and uses raw anonymous version 1.0 elements before archive
60 or polymorphic objects at archive 60+. The face-side count is exactly
`2 * face_count`; side positions `2f` and `2f+1` carry directions +1 and -1
for face `f`. Before archive 60, an element is the record chunk itself; at
archive 60 and later, the polymorphic object's class-data payload contains
that record chunk. A loaded inline topology takes precedence over the userdata
carrier.

Element records use the widths already specified in §15. Their cross-record
invariants are exact: positional indexes equal record indexes; C2/C3/surface
references are in-range and non-null; endpoint, incidence, edge/trim/loop/
face back-references agree; domains are finite increasing intervals or explicit
unset values; tolerances are finite nonnegative values or explicit unset
sentinels; singular and point-on-surface trims use edge -1 and identical
endpoints; loop rings are directed, continuous, and closed.

### 19.5 SubD record tables

The SubDimple anonymous chunk is major 1. Minor 0 is the base payload; later
minors append fields. The outer `ON_SubD` byte is `has_subdimple`: 0 means no
following payload and 1 means one SubDimple chunk.

The runtime-object write gate is archive version 60: below 60 the serialized
class is `ON_Mesh`; V3 through V5 carry the SubD in section 7.2.6 class
userdata, while V1 and V2 have no class-userdata stream. At 60 and later the
serialized class is `ON_SubD`. Proxy admission requires the identity
transform, positive face count, vertex count greater than 2, non-empty-content
SHA-1 values, and exact parent-mesh face/float-vertex counts and SHA-1 values
defined in section 7.2.6.

SubDimple field order:

```
u32 level_count
u32 obsolete_max_vertex_id
u32 obsolete_max_edge_id
u32 obsolete_max_face_id
ON_BoundingBox obsolete_global_bounds
level_count × anonymous level chunk
minor >= 1: u8 obsolete_texture_domain_type
minor >= 1: mapping tag
minor >= 2: symmetry record
minor >= 3: u64 legacy geometry serial
minor >= 4: bool subd_is_symmetric
minor >= 4: UUID face_packing_id
minor >= 4: bool sync_face_packing_hash_serials
minor >= 4: face-packing topology hash record
```

The symmetry record is an anonymous major-1 chunk with version at least 1:

```text
i32 major = 1
i32 version >= 1
u8 symmetry_type
if symmetry_type is 1 through 5:
  u32 inversion_order
  u32 cyclic_order
  UUID symmetry_id
  anonymous symmetry-transform chunk
  if version >= 2: u8 coordinate_system
  if version >= 3: u64 symmetric-object content serial
  if version >= 4: topology hash record, geometry hash record
```

Symmetry types are 0 unset, 1 reflection, 2 rotation, 3 alternating
reflection-and-rotation, 4 inversion, and 5 cyclic. Type 113 is the legacy
rotation prototype and uses the rotation grammar. Any other type is retained in
native source data, maps to `Unset`, and causes the reader to skip the remainder
of the bounded symmetry chunk before continuing the containing SubD record.
Coordinate systems are 0 unset, 1 object, and 2 world. Any other coordinate
value is retained in native source data, maps to `Unset`, and does not prevent
the reader from consuming later versioned fields. Each unknown value emits a
`container.enumeration-value-degraded` loss.

Every level is anonymous version 1.1:

```
u16 level_index
u8 algorithm_0 = 4
u8 algorithm_1 = 4
u8 algorithm_2 = 4
ON_3dPoint controlnet_min
ON_3dPoint controlnet_max
u32 archive_id_partition[4]
vertices for [p0,p1)
edges for [p1,p2)
faces for [p2,p3)
u8 render_mesh_present
```

The partition array is `[first_vertex, first_edge, first_face, one_past_face]`.
Each record's embedded archive ID must equal the loop's expected ID. Archive
IDs are reconstructed into the vertex, edge, and face maps before pointer
resolution. Level zero is the control cage. Higher levels are bounded,
validated for framing, and may be discarded after consumption.

Every component base is:

```
u32 archive_id
u32 component_id
u16 subdivision_level
```

For archive versions below 70, the base suffix is:

```
u8 saved_point_size
if nonzero: 3 × f64 saved_point
u8 deprecated_vector_size
if nonzero: 3 × f64 deprecated_vector
```

The writer uses saved-point size 4 or zero and deprecated-vector size zero.
For archive versions 70+, each base has the following size-tagged additions:

```
u8 size_24; if nonzero: 24 bytes deprecated displacement
u8 size_4;  if nonzero: u32 group ID
u8 size_5;  if nonzero: u32 archive ID + u8 pointer flags
u8 255 end of additions
```

Each size tag accepts zero, its defined size, 254 for a bounded anonymous
future addition, or 255 to terminate the addition sequence. A fixed-size
unknown addition is consumed as exactly that many bytes. A 254 addition is
consumed as one anonymous chunk. Any other size is invalid.

Vertex record:

```
component base
u8 vertex_tag
3 × f64 control point
u16 edge_count
u16 face_count
u8 saved_limit_point_present
if present:
  u32 limit_point_count
  repeat:
    3 × f64 limit point
    3 × f64 tangent 1
    3 × f64 tangent 2
    3 × f64 normal
    u32 face archive ID + u8 pointer flags
u16 serialized_edge_count
edge_count × (u32 archive ID + u8 flags)
u16 serialized_face_count
face_count × (u32 archive ID + u8 flags)
archive < 70: u8 end marker = 0
archive >= 70: component additions
```

The serialized edge and face counts must equal their preceding counts. Vertex
tags are 0 unset, 1 smooth, 2 crease, 3 corner, and 4 dart.

Edge record:

```
component base
u8 edge_tag
u16 face_count
2 × f64 sector coefficient
f64 sharpness_start
u16 serialized_vertex_count = 2
2 × (u32 archive ID + u8 flags)
u16 serialized_face_count
face_count × (u32 archive ID + u8 flags)
archive < 70: u8 end marker = 0
archive >= 80:
  u8 end_sharpness_size
  if 255: no end sharpness; end = start
  if 8: f64 sharpness_end
archive >= 70: component additions
```

Edge tags are 0 unset, 1 smooth, 2 crease, and 4 smooth-X. Pointer type bits
are 0x2 vertex, 0x4 edge, and 0x6 face; bit 0 is direction. A null pointer has
archive ID zero. Edge and face directions reverse traversal; vertex direction
is reserved.

Face record:

```
component base
u32 level_zero_face_id
u32 obsolete_parent_face_id
u16 edge_count
u16 serialized_edge_count
edge_count × (u32 archive ID + u8 flags)
archive < 70: u8 end marker = 0
archive >= 70:
  u8 size_34; if nonzero:
    u8 obsolete_texture_coordinate_type
    u8 packing_rotation_index
    2 × f64 rectangle origin
    2 × f64 rectangle size
  u8 size_4; if nonzero: u32 material channel index
  u8 size_4; if nonzero: ON_Color per-face color
  u8 size_4; if nonzero: u32 pack ID
  u8 size_4; if nonzero:
    u32 ten_point_chunk_count
    ten_point_chunk_count × (u8 size_240 + 10 × ON_3dPoint)
    optional u8 size_(remainder) + remainder × ON_3dPoint
  u8 255 end marker
```

The custom texture-point count must equal `edge_count / 10` for full ten-point
chunks, with the final remainder equal to `edge_count % 10`. A face ring has at
least three directed uses, valid edge pointers, and endpoint continuity.

### 19.6 Instance-definition exact tables

The instance-definition table record contains the class payload. Archive 50
uses packed version 1.6. Archive 60 accepts the legacy packed version 1.7
form and the anonymous V6 form; archives 70 and 80 use the anonymous V6 form.

V5 packed field order:

```
u8 packed version = 0x16 or 0x17
UUID definition ID
i32 UUID-array count
count × UUID member object ID
UTF-16 name
UTF-16 description
UTF-16 URL
UTF-16 URL tag
ON_BoundingBox
u32 definition type
UTF-16 linked full path
minor >= 1: checksum record
minor >= 2: u32 unit-system enum
minor >= 3: f64 meters per unit, bool legacy relative-path
minor >= 4: units/tolerances detail record
minor >= 5: i32 nested linked-definition depth
minor >= 6: u32 linked-component appearance
minor >= 7:
  bool file-reference-present
  if true: file-reference record
```

The packed V5 reader requires major version 1. It reads each field through the
minor-1.6 appearance field. At minor 1.7 it reads the file-reference presence
byte and, when set, the file-reference record; the writer then emits one
obsolete linked-layer-settings Boolean. The reader does not assign that final
Boolean a field meaning and returns after the file-reference record, so the
obsolete Boolean and any later bytes are an abandoned suffix at the enclosing
class-data boundary.

The V5 legacy path fields use the full-path slot when the relative-path
Boolean is false and the relative-path slot when it is true. The optional
class-userdata carrier can fill the other slot without replacing a nonempty
slot.

The anonymous V6 writer emits outer and linked-type chunks at minor 0. Their
readers require major version 1 and consume the fixed prefix without a minor
gate. The linked-type child has the same major-version rule and bounded suffix
behavior. Model-component attributes, unit-system
detail, file-reference, content-hash, SHA-1, and referenced-component-settings
children are each bounded anonymous major-1 chunks. Model-component attributes
consume status bytes for model serials, UUID, component type, index, and name;
the instance-definition writer selects only index, UUID, and name. Unit-system
detail consumes a unit enum, meters-per-unit value, and custom-unit name.
File-reference minor 1 adds the embedded-file UUID; content-hash and SHA-1
writers use minor 0. Each reader closes its own chunk after its known prefix,
so later-minor direct bytes remain at that child boundary.

Referenced-component settings has an outer anonymous major-1 chunk containing
a presence Boolean. When present, the Boolean is followed by an anonymous
major-1 implementation chunk containing two layer-object arrays and a parent
layer presence Boolean with an optional layer object. The outer reader closes
after that implementation child; the implementation reader closes after its
known arrays and optional parent layer.

The V5 member array is empty for linked definitions and contains member UUIDs
for static and linked-and-embedded definitions. Definition type values are 0
or 1 static, 2 linked-and-embedded, 3 linked, and `0xffffffff` unset. A
missing or empty linked path converts a non-unset linked type to static. For a
linked definition, appearance defaults to active below archive 50 and reference
at archive 50 and later when no valid appearance is stored.

V6–V8 anonymous field order:

```
anonymous version major=1, minor=0 or later
model-component attributes: index, UUID, name
u32 definition type
units/tolerances detail record
UTF-16 description
UTF-16 URL
UTF-16 URL tag
ON_BoundingBox
bool member-UUID-array-present
if true: i32 UUID-array count, count × UUID
bool linked-type
if true:
  anonymous linked-type major=1, minor=0 or later
  file-reference record
  i32 nested linked-definition depth
  u32 linked-component appearance
  bool reference-component-settings-present
  if true: referenced-component-settings record
```

`ON_InstanceRef` is packed major version 1. The writer emits minor 0 and the
reader ignores the minor after consuming the fields below. Later bytes remain
bounded by the enclosing class-data record:

```
u8 version = 0x10
UUID definition ID
16 × f64 transform entries
ON_BoundingBox
```

The transform and definition UUID identify the reference. Definition
membership comes from the member UUID array, not object attributes.

## 20. Product, presentation, and complete-record semantics

### 20.1 External file identity

A file reference is an anonymous major-1 chunk. Minor 1 adds the embedded-file
component:

```
UTF-16 full path
UTF-16 relative path
anonymous content-hash major-1 chunk:
  u64 referenced byte count
  u64 hash acquisition time
  u64 content modification time
  anonymous SHA-1 major-1 chunk: 20 digest bytes
  anonymous SHA-1 major-1 chunk: 20 digest bytes
u32 path status
minor >= 1: UUID embedded-file component
```

The file-reference reader requires major version 1, reads the embedded-file
UUID at minor 1 and later, and closes the bounded chunk after the path-status
field or embedded UUID. The content-hash child and both SHA-1 children require
major version 1 and have fixed prefixes; later direct bytes remain at their
respective child boundaries.

The first SHA-1 identifies the normalized name and the second identifies the
content. A linked instance definition and a texture image reference use this
same structure. Linked definitions preserve their structure when the archive
contains no local member geometry.

### 20.2 Materials, textures, and mappings

Archives 2 and 3 store material class data as a direct packed major-1
payload. The material writer emits minor 1. The direct payload is:

```
packed version 1.minor
ON_Color ambient
ON_Color diffuse
ON_Color emission
ON_Color specular
f64 shine
f64 transparency
4 × u8 obsolete shadow and wire flags
ON_Color obsolete wire color
i16 obsolete line-style pattern
i16 obsolete pattern index
f64 obsolete thickness
f64 obsolete scale
UTF-16 bitmap path
i32 bitmap mode
i32 obsolete bitmap index
UTF-16 bump path
i32 bump mode
i32 obsolete bump index
f64 bump scale
UTF-16 environment-map path
i32 environment-map mode
i32 obsolete environment-map index
i32 material archive index
UUID material plug-in
UTF-16 obsolete Flamingo library
UTF-16 material name
minor >= 1: UUID material ID
minor >= 1: ON_Color reflection
minor >= 1: ON_Color transparent
minor >= 1: f64 index of refraction
```

The direct version has major 1. Minor 0 ends after the material name; minor 1
adds the material ID, reflection color, transparent color, and index of
refraction. A minor at least 1 reads the same known prefix. Bytes after the
known prefix remain at the containing class-data boundary. When the minor is
0, the source material defaults the two omitted colors to `ON_Color::White`
and the index of refraction to `1.0`. CADIR does not fabricate the source
material ID: the source UUID is absent and the material record key uses the
owning table-record offset.

The three path groups are direct fields, not an anonymous texture array. An
empty path creates no texture. A nonempty path creates one texture with type
`1` for bitmap, `2` for bump, or `86` for environment map. Mode `2` is
`decal_texture`; every other stored mode is `modulate_texture`. The three
obsolete indices are consumed and have no typed meaning. The bump texture
uses the interval `[0, bump scale]`; the other two texture types use
`[0,1]`.

CADIR decision: a V2/V3 path texture has no serialized texture UUID, child
boundary, file reference, or linear-workflow flag. Its native texture record
therefore has no source UUID, uses the owning material record offset as its
source offset, and uses these `ON_Texture::Default` values for absent fields:
mapping channel `1`, enabled `true`, linear minification and magnification
filters (`1`), repeat U/V/W wraps (`0`), identity UVW transform,
`ON_UNSET_COLOR` border and transparent colors, no transparency-texture UUID,
alpha blend `[1,1,1,0,0]`, black RGB blend constant, RGB blend
`[1,1,0,0]`, blend order `0`, and no file reference or linear-workflow value.
The direct material does not serialize reflectivity, shareability, lighting,
Fresnel, glossiness, RDK, or diffuse-alpha fields; CADIR transfers source
defaults for the booleans and reflectivity and leaves the optional fields
absent.

A modern material is an anonymous major-1, nonnegative-minor chunk followed by
model-component attributes. An early archive-60 component-attribute child uses
anonymous type
`0x40008000` and a version-1.0 body:

```
u32 presence mask
if mask & 0x01: UUID component ID
if mask & 0x02: UUID parent ID
if mask & 0x04: i32 archive index
if mask & 0x08: UTF-16 name
if mask & 0x10:
  u32 component-status mask
  u32 component-status value
```

The component-status mask uses bit 0 for locked and bit 1 for hidden. The
reader tests those known bits and ignores other presence-mask and
component-status-mask bits. Later component attributes use `0x40008002` and a
version-1.0 body with five independent status bytes in model-serial, component
UUID, component type, archive index, and name order. Status 0 reads no value, 1
is followed by the value, and 2 clears the value. The corresponding values are
three `u32` model-serial numbers, a UUID, a `u32` component type, an `i32`
archive index, and a UTF-16 string. Any other status value is treated as no
value by the reader; the writer emits only 0, 1, and 2. Both component-attribute
readers close their bounded child after the known body, so later direct bytes
remain at the containing material or resource boundary. The remaining material
fields are six colors, index of refraction, reflectivity, shine, transparency,
an anonymous texture array, material-channel pairs, shareable and lighting
flags, Fresnel controls, reflection and refraction glossiness, an RDK instance
UUID, and the diffuse-texture alpha switch.
Before writer version 1 December 2009, transparent RGB `(128,128,128)` is the
obsolete default and the diffuse color replaces the complete transparent
color.

The legacy V4 material's inner anonymous chunk also has major 1 and a
nonnegative minor. Minor 1 adds the obsolete library string; minor 2 adds
material channels; minor 3 adds shareable and lighting flags; minor 4 adds
Fresnel fields; minor 5 adds the RDK UUID; and minor 6 adds the diffuse-alpha
switch. Material readers consume the known prefix and skip remaining bytes
before the bounded end.

When component attributes omit the archive index, the in-memory index is
`ON_UNSET_INT_INDEX` (`-2147483647`). Negative index `-1` identifies a live
system component and is not an absence marker.

Each texture-array element is an `ON_Texture` class wrapper. Its anonymous
major-1, nonnegative-minor payload is:

```
UUID texture ID
u32 mapping channel
UTF-16 legacy image path
bool enabled
u32 texture type, mode, minification filter, magnification filter
3 × u32 U/V/W wrap mode
16 × f64 UVW transform
ON_Color border, ON_Color transparent
UUID transparency texture
2 × f64 bump interval
5 × f64 alpha blend constant and coefficients
ON_Color RGB blend constant
4 × f64 RGB blend coefficients
i32 blend order
minor >= 1: file reference
minor >= 2: bool treat as linear
```

Texture transforms and blend coefficients are dimensionless. Texture readers
consume the known prefix for the minor and skip remaining bytes before the
bounded end. The texture-array wrapper is also an anonymous major-1,
nonnegative-minor chunk; its count and complete class-wrapped elements are the
known prefix, followed by a bounded suffix. Texture mappings store mapping and
projection enums, primitive and UVW transforms, a primitive class object,
texture-space enum, and capped flag. Class userdata belongs to the nested
primitive object; the `MappingCRCCache` and mesh-correspondence cache payloads
are defined in sections 7.2.19 and 7.2.20.
Material channels bind a UUID to an integer channel.

The texture writer emits anonymous minor 0 for archives below version 60,
minor 1 for versions 60 through 69, and minor 2 for version 70 and later. The
texture reader requires major 1, adds the file-reference child at minor 1 and
the linear-treatment Boolean at minor 2, and closes the texture child before
the texture-array reader resumes.

The texture-mapping payload is an anonymous major-1 chunk. The writer emits
minor 1:

```text
UUID mapping ID
u32 mapping type
u32 projection
16 × f64 primitive transform
16 × f64 UVW transform
UTF-16 mapping name
ON_Object primitive class wrapper
minor >= 1: u32 texture space, bool capped
```

The primitive class wrapper ends before the texture-space and capped fields.
The mapping reader requires major 1, reads those fields at minor 1 and later,
and leaves later bytes at the mapping-child boundary.

### 20.3 Drafting resources and annotations

Linetypes store model-component identity, ordered length/type segments, cap and
join styles, width and width units, taper points, and the model-distance flag.
Segment lengths and widths with model units are length values. Hatch patterns
store identity, fill type, description, and hatch lines. Hatch-line base,
offset, and dash values are lengths; angle is radians.

The hatch-pattern class UUID is
`064E7C91-35F6-4734-A446-79FF7CD659E1`. A hatch-pattern table record contains
one class wrapper and no record-specific child after it:

```text
HATCH_PATTERN_RECORD long
  OPENNURBS_CLASS long
    OPENNURBS_CLASS_UUID long: hatch-pattern class UUID and CRC
    OPENNURBS_CLASS_DATA long: hatch-pattern payload
    zero or more CLASS_USERDATA chunks
    OPENNURBS_CLASS_END short, value 0
```

For archives below version 60, the class-data payload is:

```text
packed version 1.2
i32 archive hatch-pattern index
u32 fill type                                  // 0 solid, 1 lines
UTF-16 hatch-pattern name
UTF-16 description
if fill type == 1:
  i32 hatch-line count
  count × hatch-line V5 payload
UUID hatch-pattern ID
```

Each V5 hatch-line payload is:

```text
packed version 1.1
f64 angle in radians
2 × f64 base point
2 × f64 offset vector
i32 dash count
count × f64 dash length
```

For archive versions 60 through 80, the class-data payload contains one
anonymous major-1, minor-0 chunk. Its body is model-component attributes
restricted to ID, archive index, and name, followed by the fill type, UTF-16
description, and an anonymous line-list chunk. The line-list body is an `i32`
count followed by complete anonymous hatch-line chunks. Each modern line chunk
is anonymous major 1, minor 0 and contains the same angle, base, offset, and
dash fields as the V5 line payload. The modern hatch writer emits this branch;
the reader also admits the V5 branch for archive version 60 files whose
OpenNURBS writer version selects the legacy compatibility path. All counts are
nonnegative and every anonymous child is bounded independently. A line angle
is in radians. Base coordinates, offsets, and signed dash lengths are lengths;
positive dashes draw and negative dashes leave gaps.

The hatch-line writer emits the packed 1.1 payload below archive version 60 and
the anonymous 1.0 line child at archive version 60 and later. The hatch-pattern
writer emits the packed 1.2 payload below archive version 60, the anonymous
1.0 pattern child for archive versions 60 through 89, and the same anonymous
pattern prefix with the archive-90 tail at archive version 90 and later. The
line child ends before the next line-list byte, the line-list child ends before
the pattern unit-system byte or archive-90 tail, and the pattern child ends at
its enclosing class-data boundary.

Archive version 90 retains that anonymous major-1, minor-0 body and
append these fields after the line-list chunk:

```text
u8 pattern unit-system code
bool always model distances
```

The pattern unit-system code uses the table in section 8.2. `none` means the
hatch lines are always model-distance values; another code defines the unit
system in which the pattern lines are specified. `always model distances` true
displays hatch-line lengths and widths in model distances. False interprets
them as page-layout or printed-output lengths and widths. These two fields are
part of the archive-90 class-data grammar, not an untyped suffix of the
archive-80 grammar.

The writer emits packed version 1.2 below archive 60 and anonymous version 1.0
for archive versions 60 through 80. Archive version 90 uses the same anonymous
prefix followed by the two fields above. A missing or nil pattern
UUID is not a source identity; CADIR keys that record by its source record
offset and leaves `source_uuid` unset.

For archive version 90, the native `hatch_patterns` record includes optional
`pattern_unit_system` and `always_model_distances` fields. The former is the
serialized unit-system code; the latter is the serialized Boolean. These
fields are absent for earlier archive versions.

The V2 compatibility annotation classes use these class UUIDs:

| class | UUID |
| --- | --- |
| `ON_OBSOLETE_V2_TextDot` | `8BD94E19-59E1-11D4-8018-0010830122F0` |
| `ON_OBSOLETE_V2_AnnotationArrow` | `8BD94E1A-59E1-11D4-8018-0010830122F0` |

Their class-data payloads are direct packed version 1.0 records. The reader
requires major version 1 and does not gate the known prefix on the minor
version. The text-dot payload is:

```text
packed version 1.minor
ON_3dPoint center
UTF-16 text
```

The annotation-arrow payload is:

```text
packed version 1.minor
ON_3dPoint tail
ON_3dPoint head
```

The text-dot string uses the UTF-16 archive grammar in section 7.3. The
class-data chunk ends after the known prefix; later bytes remain at that
`OPENNURBS_CLASS_DATA` boundary. The text-dot center and arrow endpoints use
document length conversion. The source arrow is annotation display geometry,
not a neutral curve.

CADIR decision: V2 text dots enter `native.rhino.text_dots`. Because the V2
payload has no modern height, secondary-text, font, or display fields, those
fields use neutral values: height `0`, empty secondary text and font, and
false display flags. V2 annotation arrows enter
`native.rhino.annotation_arrows` with their scaled tail and head points and
source links. CADIR does not create a neutral curve or semantic annotation
for an arrow because the V2 class carries only its display endpoints.

The remaining V2 annotation classes use these class UUIDs:

| class | UUID | payload role |
| --- | --- | --- |
| `ON_OBSOLETE_V2_Annotation` | `ABAF5873-4145-11D4-800F-0010830122F0` | virtual base |
| `ON_OBSOLETE_V2_DimLinear` | `5DE6B20D-486B-11D4-8014-0010830122F0` | linear or aligned dimension |
| `ON_OBSOLETE_V2_DimRadial` | `5DE6B20E-486B-11D4-8014-0010830122F0` | radius or diameter dimension |
| `ON_OBSOLETE_V2_DimAngular` | `5DE6B20F-486B-11D4-8014-0010830122F0` | angular dimension |
| `ON_OBSOLETE_V2_TextObject` | `5DE6B210-486B-11D4-8014-0010830122F0` | text |
| `ON_OBSOLETE_V2_Leader` | `5DE6B211-486B-11D4-8014-0010830122F0` | leader |

The class-data payload for each class is a direct packed version `1.minor`
prefix. The reader requires major `1` and leaves an unread minor at the
class-data boundary. The shared base prefix is:

```text
packed version 1.minor
u32 annotation type
ON_Plane plane
i32 point count
point count × ON_2dPoint
UTF-16 user text
UTF-16 default text
i32 user-positioned-text flag
```

`ON_Plane` contains an origin, x axis, y axis, z axis, and plane equation as
`16 × f64`. Each `ON_2dPoint` contains two `f64` values. The producer writes a
nonnegative point count. The annotation type enum values are `0` nothing, `1`
linear, `2` aligned, `3` angular, `4` diameter, `5` radius, `6` leader, `7`
text block, and `8` ordinate. The flag is false for zero and true for every
nonzero value. The two strings use the UTF-16 archive string grammar in
section 7.3. The source reader rejects a plane origin or point coordinate
whose absolute raw value is greater than `1.0e150`.

The concrete payloads append these fields after the shared prefix:

```text
ON_OBSOLETE_V2_DimLinear: no fields
ON_OBSOLETE_V2_DimRadial: no fields
ON_OBSOLETE_V2_DimAngular:
  f64 angle in radians
  f64 radius in model length units
ON_OBSOLETE_V2_TextObject:
  UTF-16 face name
  i32 Windows font weight
  f64 text height in model length units
ON_OBSOLETE_V2_Leader: no fields
```

The angular reader requires both stored values to be positive and no greater
than `1.0e150`. The text reader accepts a signed height whose absolute value
is no greater than `1.0e150`. The base class is virtual and has no concrete
object admission; a direct base-class record is retained as a native
annotation when its type is known. Types `0` and `8` have no concrete V2 class
in this family and remain native when carried by a direct base record. All
fields after a concrete prefix belong to `OPENNURBS_CLASS_DATA` and are not
interpreted by the class reader.

The source conversion selects `user text` when it is nonempty and otherwise
selects `default text`, then trims Unicode whitespace and control characters
from both ends. Plane origins and all point coordinates are document lengths.
For linear and radial dimensions, conversion moves the first point to the
plane origin and expresses the remaining points relative to that origin. A
linear dimension uses points 0, 1, 2, and 3 as the first extension endpoint,
first arrow tip, second extension endpoint, and second arrow tip; point 4 is
the optional user-positioned text point. Its numeric distance is the length of
point 1 minus point 3. A radial dimension uses point 0 as center, point 1 as
the radius point, point 2 as the dimension-line point, and point 3 as the
optional text point. Its numeric distance is the center-to-point-1 length,
doubled for type 4. An angular dimension keeps its plane origin. Points 0 and
1 are its stored direction vectors and point 2 is its optional text point; its
stored angle is radians and its stored radius is a model length. A leader uses
every point in order. The text object has no point-role interpretation.

The V2-to-V5 source conversion applies additional point-count gates after it
copies the common fields: linear and aligned dimensions require exactly five
points after truncating extras; angular dimensions retain at most three points
and require at least two; leaders require at least two points; radial
dimensions and text objects have no source point-count gate. A failed minimum
gate clears the converted point array. These conversion gates are separate
from the CADIR admission decision below.

CADIR decision: V2 linear, radial, and angular dimensions enter the semantic
annotation arena. Linear and radial admission requires the points needed for
the roles above; angular admission requires two direction points. A record
that does not provide those roles remains retained and does not receive a
typed semantic definition. The semantic measurement for linear and radial
families is the source numeric distance after document-unit conversion. The
semantic angular measurement is the stored angle in radians. Its degree
conversion is retained as `v2_numeric_value_degrees`; this is the
stored V2 value, not the possibly recomputed angle from the V2-to-V5
conversion path.
`v2_points`, `v2_default_text`, and the angular `v2_angle_radians` and
`v2_radius` parameters preserve the V2 fields that have no common neutral
field. CADIR uses the explicit stored angular values for its neutral
dimension-line point and retains the source direction vectors unchanged; the
source conversion may recompute an angular plane, angle, and radius from valid
direction vectors. Linear text uses the default semantic text location
because the V2-to-V5 conversion does not enable user positioning for that
family. Radial and angular text use their optional points only when the
serialized flag is true.

CADIR decision: V2 text objects, leaders, and direct base annotations enter
`native.rhino.annotations`. Their `rich_text` is the selected and trimmed
text; the raw user text, raw default text, face name, font weight, text height,
user-positioned flag, and leader points remain in the native record. V2 text
height is converted as a document length. A direct base annotation with a
type 6 or 7 receives the native `leader` or `text` kind; all other base types,
including recognized dimension types, use native kind `annotation` and retain
the numeric `annotation_type`. An unrecognized type also remains a native
`annotation` record with its raw signed `i32` value; CADIR does not apply the
source reader's fallback to `dtNothing`. No V2 text or leader is fabricated as
a modern dimension or curve.

Group and light records use packed major-1 versions. The group class UUID is
`721D9F97-3645-44C4-8BE6-B2CF697D25CE`. A group table record is:

```text
GROUP_RECORD long
  OPENNURBS_CLASS long
    OPENNURBS_CLASS_UUID long: group class UUID and CRC
    OPENNURBS_CLASS_DATA long: group payload
    zero or more CLASS_USERDATA chunks
    OPENNURBS_CLASS_END short, value 0
```

The group class-data payload is:

```text
packed version 1.minor
i32 archive group index
UTF-16 group name
if minor >= 1: UUID group ID
```

The writer emits packed version 1.1. The reader requires major 1 and skips
fields after its known prefix at the class-data boundary. The group record has
no record-end child; its CRC-bearing long-chunk boundary contains the class
wrapper. Group membership is stored on object attributes as archive group
indexes, not in the group class data. CADIR links an object to the unique group
with each listed index. Duplicate group indexes produce no links for that index
and a `presentation.record-dropped` loss. A missing or nil serialized group ID
is not a source identity; CADIR keys that record by its archive index and leaves
`source_uuid` unset.

The light class UUID is
`85A08513-F383-11D3-BFE7-0010830122F0`. A light table record is:

```text
LIGHT_RECORD long
  OPENNURBS_CLASS long
    OPENNURBS_CLASS_UUID long: UUID and CRC
    OPENNURBS_CLASS_DATA long: light payload
    zero or more CLASS_USERDATA chunks
    OPENNURBS_CLASS_END short, value 0
  optional LIGHT_RECORD_ATTRIBUTES long: ON_3dmObjectAttributes body
  optional LIGHT_RECORD_ATTRIBUTES_USERDATA long:
    zero or more CLASS_USERDATA chunks
    OPENNURBS_CLASS_END short, value 0
  LIGHT_RECORD_END short, value 0
```

The light class-data payload is:

```text
packed version 1.minor
i32 enabled                                      // nonzero means enabled
i32 style
f64 intensity
f64 watts
ON_Color ambient                                 // 4 bytes
ON_Color diffuse                                 // 4 bytes
ON_Color specular                                // 4 bytes
3 × f64 direction
3 × f64 location
f64 spot angle in degrees
f64 spot exponent
3 × f64 attenuation
f64 shadow intensity
i32 archive light index
UUID light ID
UTF-16 light name
if minor >= 1: 3 × f64 length, 3 × f64 width
if minor >= 2: f64 hotspot
```

The class writer emits version 1.2. `style` values are 0 unknown, 4 camera
directional, 5 camera point, 6 camera spot, 7 world directional, 8 world
point, 9 world spot, 10 ambient, 11 world linear, and 12 world rectangular.
Location is ignored for directional and ambient lights. Direction is ignored
for point and ambient lights. Length is used only by linear and rectangular
lights; width is used only by rectangular lights. Location, length, and width
use document length units. Direction and attenuation are dimensionless. The
attenuation factor at distance `d` is
`1 / (attenuation[0] + d * attenuation[1] + d² * attenuation[2])` for styles
that use attenuation. Intensity 0 is off, 1 is 100 percent, values above 1
are permitted for high-dynamic-range renderers, and watts 0 means that fixture
power is unused. Shadow intensity 0 disables shadow casting and 1 produces a
full black shadow.

The spot angle is stored in degrees and ranges from 0 through 90. The spot
exponent ranges from 0 through 128, with 0 uniform and 128 highly focused.
The hotspot field ranges from 0 through 1. `ON_UNSET_VALUE`, encoded as
`-1.23432101234321e+308`, selects the exponent interface instead of an
explicit hotspot. In that mode the effective hotspot is derived from the
stored exponent and angle by the OpenNURBS spotlight relation
`cos(h × angle)^exponent = 0.7071067811865475`; an explicit hotspot selects
the hotspot interface. For minor 0 and 1, the file has no hotspot field: the
reader derives `clamp(1 - exponent / 128, 0, 1)` and clears the stored
exponent. The native light record preserves the raw angle, exponent, and
hotspot fields, including the sentinel.

The light reader requires major 1 and leaves bytes after the known
minor-gated prefix at the `OPENNURBS_CLASS_DATA` boundary. The class-data
boundary ends before class userdata and the class-end marker.

`LIGHT_RECORD_ATTRIBUTES` is an optional CRC-bearing long child after the
class wrapper. Its body is the `ON_3dmObjectAttributes` payload from section
9: archives below version 5 use the fixed version-1 grammar, and V5 and later
use the version-2 tagged grammar selected by the OpenNURBS writer version.
The attribute-child CRC covers its direct body bytes and excludes the complete
rendering-attributes child described in section 8.4. The light record accepts
at most one attributes child, followed by at most one
`LIGHT_RECORD_ATTRIBUTES_USERDATA` long child, and ends with the short zero
`LIGHT_RECORD_END` child. The userdata body is the attribute-userdata stream
from section 9.3 and ends with the short zero class-end marker.

CADIR decision: table-light attributes belong to the light component. When
the attributes child is valid, `native.rhino.lights[].attributes` contains a
nested object-attribute projection. Its `source_offset` is the attributes
child header offset and its `source_uuid` is the serialized attribute object
UUID. The projection contains `name`, `url`, `layer_index`, `material_index`,
`linetype_index`, `color`, `visible`, `object_mode`, `decoration`,
`wire_density`, the color/linetype/material/plot source selectors, plot color
and weight, group indexes, display-material pairs, active-space and viewport
selectors, display order, clipping state and UUIDs, hatch and linetype
settings, detail-background state, section-fill and clipping-label values,
rendering material and mapping references, shadow and texture-preview flags,
geometry and attribute user strings, custom render-mesh settings, and mesh
modifiers. The projection uses the same field defaults and userdata ownership
rules as `native.rhino.object_presentation`; it does not create an object
record or a second light identity. A missing attributes child omits the
nested projection. A malformed attributes child leaves the typed light class
data admitted, omits the projection, and records an object-attributes
degradation; malformed recognized attribute-userdata carriers leave the light
attributes admitted and record a bounded diagnostic.

The same class-data payload is used when `ON_Light` appears in an object
record; the object record then uses the common object-attributes and
object-record-end children instead of the light-table children.

The linetype class UUID is
`26F10A24-7D13-4F05-8FDA-8E364DAF8EA6`. A linetype table record contains one
class wrapper and no record-specific child after it:

```text
LINETYPE_RECORD long
  OPENNURBS_CLASS long
    OPENNURBS_CLASS_UUID long: linetype class UUID and CRC
    OPENNURBS_CLASS_DATA long: linetype payload
    zero or more CLASS_USERDATA chunks
    OPENNURBS_CLASS_END short, value 0
```

For archives below version 60, the class-data payload is anonymous version
1.1:

```text
anonymous version 1.1
i32 archive linetype index
UTF-16 linetype name
i32 segment count
count × {
  f64 segment length
  u32 segment type tag
}
minor >= 1: UUID linetype ID
```

The writer emits minor 1. The segment type tag is 0 for a line, 1 for a
space, and `0xffffffff` for the unset type. Other tags are underlying segment
enum values. Segment lengths are the `ON_LinetypeSegment::m_length` values.

For archives version 60 and later, the class-data payload is anonymous version
2.3:

```text
anonymous version 2.3
MODEL_ATTRIBUTES long, version 1.0
i32 segment count
count × {
  f64 segment length
  u32 segment type tag
}
extension item stream
```

The model-attributes body has five independent status/value pairs in this
order: model serial number, component UUID, component type, archive index, and
name. Status 0 has no value, status 1 is followed by the value, and status 2
clears the value. The corresponding values are three `u32` serial numbers, a
UUID, a `u32` component type, an `i32` archive index, and a UTF-16 string. The
linetype writer uses model-attributes chunk type `0x40008002`; its normal
attribute filter leaves model serial and component type absent and writes the
UUID, archive index, and name statuses.

The modern extension stream is a sequence of an item byte, its value, and the
next item byte. The writer emits items in ascending order and then item zero:

```text
minor >= 1: item 1: u8 line cap style
minor >= 1: item 2: u8 line join style
minor >= 2: item 3: f64 width
minor >= 2: item 4: u8 width unit-system code
minor >= 2: item 5: i32 taper count, count × { f64 x, f64 y }
minor >= 3: item 6: bool always model distances
item 0: extension terminator
```

The cap styles are round `0`, flat `1`, and square `2`. The join styles are
round `0`, miter `1`, and bevel `2`. The default values are round cap, round
join, width `1.0`, width units `none` (`0`), no taper points, and false for
always model distances. The writer omits items 1 and 2 at their defaults, item
3 when width differs from `1.0` by no more than `ON_EPSILON`, item 4 when the
unit is `none`, item 5 when the taper is empty, and item 6 when the flag is
false. The width unit-system code uses the table in section 8.2: `none` means
pixels, `unset` (`255`) means the document unit system, and another code names
that explicit unit system. Taper `x` is the fraction along the curve and taper
`y` is the width at that fraction.

The writer emits non-default extension items in strictly increasing code order
and then writes code `0`. The reader applies item gates from the minor version
through the same ascending cascade, accepts future modern minor values after
the known prefix, and closes the anonymous chunk at its boundary. A code lower
than or equal to the last consumed code is consumed only as an ID and its
value remains an untyped suffix. An item code greater than 6 is a future
extension: its ID is consumed, but its unlength-prefixed value and all later
bytes remain a bounded suffix. The reader does not require a terminator after
an out-of-order or future ID. The anonymous class-data reader accepts major 1
and major 2 only.

`ON_LinetypeSegment::m_length` is in millimeters on printed output. A legacy
major-1 record has no model-distance flag and therefore carries print-millimeter
segment lengths. In a modern record, false `always model distances` has the
same print/layout interpretation. True `always model distances` interprets
segment lengths in document units; CADIR transfers those values to
`segments[].length_millimeters` by multiplying by the document's millimeters-
per-unit scale. CADIR retains `width`, `width_units`, and taper ordinates as
stored values with their unit selector; it does not apply that segment scale to
them. A nil linetype UUID is not a source identity; CADIR keys that record by
its source record offset and leaves `source_uuid` unset.

Text styles use the legacy packed font format when the archive version is below
60 or the OpenNURBS writer version is earlier than 6.0.2015-09-23. Later
archives use an anonymous major-1 text-style record. Its minor-1 prefix is:

```text
anonymous version 1.1
model-component attributes
bool font-description-present
if present: UTF-16 font description
bool font-present
if present: anonymous font record
UUID text-style ID
UTF-16 text-style name
```

The text-style reader accepts minor values greater than 1 by consuming this
prefix and leaving the remaining bytes to the text-style chunk boundary. Minor
0 omits the ID and name.
The modern font child is consumed to its own anonymous boundary before the
text-style reader consumes the text-style ID and name; a font suffix cannot
consume those outer fields.

The modern font child is an anonymous major-1 record. The current writer uses
minor 6; each field added at a later minor is present when the font minor is at
least that value:

```text
anonymous font version 1.minor
u32 font characteristics
UTF-8 string chunk Windows LOGFONT name
UTF-16 PostScript name
if minor >= 1: UTF-16 obsolete font description
if minor >= 2: i32 Windows LOGFONT weight; f64 Apple weight trait
if minor >= 3: f64 point size; bool obsolete LOGFONT block
  if true: 4 × u8 obsolete values; 4 × i32 obsolete values
if minor >= 4: UTF-16 family name
if minor >= 5:
  UTF-16 locale name
  UTF-16 localized PostScript name
  UTF-16 English PostScript name
  UTF-16 localized Windows LOGFONT name
  UTF-16 English Windows LOGFONT name
  UTF-16 localized family name
  UTF-16 English family name
  UTF-16 localized face name
  UTF-16 English face name
  anonymous packed version 1.0 PANOSE record: 10 × u8
if minor >= 6: u8 rich-text quartet member
```

The UTF-8 string chunk has format byte 0 for an empty value or format byte 1
followed by the remaining UTF-8 bytes. It has no string count or terminator;
the chunk boundary supplies its length. Font point size is not a model length.
The modern font reader accepts later minor values, consumes the known prefix,
and leaves the remaining bytes to the font chunk boundary.

Archives below version 60 use class UUID `81BD83D5-7120-41C4-9A57-C449336FF12C`
(`ON_V5x_DimStyle`) for dimension-style table records. Its class-data body is
not anonymous and begins with packed version `1.5`, with the major in the high
nibble and the minor in the low nibble:

```text
u8 packed version
i32 referenced dimension-style index
UTF-16 name
5 × f64 model lengths: extension-line extension, extension-line offset,
  arrow size, center-mark size, text gap
u32 obsolete text-display mode
i32 arrow type
i32 angular units
i32 length format
i32 angle format
i32 length resolution
i32 angle resolution
i32 legacy text-style index
if minor >= 1: f64 model text height
if minor >= 2:
  f64 length factor
  UTF-16 prefix
  UTF-16 suffix
  bool alternate dimensions enabled
  f64 alternate length factor
  i32 alternate length format
  i32 alternate length resolution
  i32 alternate angle format
  i32 alternate angle resolution
  UTF-16 alternate prefix
  UTF-16 alternate suffix
  i32 unused value
if minor >= 3: UUID dimension-style ID
if minor >= 4: f64 model dimension-line extension
if minor >= 5:
  f64 model leader arrow size
  i32 leader arrow type
  bool suppress extension line 1
  bool suppress extension line 2
```

The writer uses defaults for fields omitted by a minor gate: text height 1,
length factor 1, alternate dimensions disabled, alternate length factor 1,
alternate formats 0, alternate resolutions 2, empty strings, dimension-line
extension 0, leader arrow size 1, leader arrow type 0, and both suppression
flags false. The class writer multiplies the model-length fields by the
dimension scale for archive versions below 5; the V5 archive writes the model
values directly. The class UUID is followed by the ordinary class-data and
class-end framing. A V5 or V50 writer also attaches class userdata with class
and item UUID `513FDE53-7284-4065-8601-06CEA8B28D6F`, application UUID
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`, and an anonymous version 1.3 payload
written by `ON_DimStyleExtra`:

```text
outer anonymous userdata child
  anonymous version 1.3
    UUID parent dimension style
    i32 valid-field count
    count × u8 valid-field values
    i32 tolerance format
    i32 tolerance resolution
    f64 upper tolerance
    f64 lower tolerance
    f64 tolerance text-height scale
    f64 baseline spacing
    if minor >= 1:
      bool draw legacy text mask
      i32 legacy mask color source
      4 × u8 legacy mask color
    if minor >= 2:
      f64 dimension scale
      i32 dimension-scale source
    if minor >= 3: UUID source dimension style
```

The extra writer multiplies baseline spacing by dimension scale for archive
versions below 5 and writes the dimension scale as 1; it writes both values
directly for V5. `ON_DimStyleExtra::DeleteAfterRead` applies the parent UUID,
valid-field bits, tolerance fields, mask fields, dimension scale, and source
dimension-style UUID to the V5 style. It does not copy baseline spacing into
the V5 style. The V5 reader retains the extra as a nested native source record;
its valid-field bytes, tolerance values, mask bytes, dimension scale, and
source UUID are not inferred from the packed class-data prefix.
When the mask gate is absent, its source default color is white with raw bytes
`[255,255,255,0]`; absent dimension-scale fields default to `1.0` and source
`0`.

If the packed ID is absent or nil, the CADIR decision is to use the table
record offset for the native record identity and leave `source_uuid` absent;
the decoder does not fabricate the UUID that openNURBS creates at read time.

Dimension-style records use an anonymous major-1 chunk. OpenNURBS writers
through version 8 emit minor 9; the version-9 writer emits minor 11. The
common minor-0 prefix after model-component attributes is:

```text
7 × f64 model lengths: extension-line extension, extension-line offset,
  arrow size, leader arrow size, center-mark size, text gap, text height
u32 obsolete text-display mode
u32 angle format
u32 obsolete length format
i32 angle resolution
i32 length resolution
i32 text-style index
f64 length factor
bool alternate dimensions enabled
f64 alternate length factor
u32 obsolete alternate length format
i32 alternate length resolution
4 × UTF-16 prefix, suffix, alternate prefix, alternate suffix
f64 dimension-line extension
bool suppress extension line 1
bool suppress extension line 2
UUID parent dimension style
u32 legacy field-override parent count
bool field-override array present
if present: bool array field overrides
u32 tolerance format
i32 tolerance resolution
3 × f64 upper tolerance, lower tolerance, tolerance text-height scale
f64 baseline spacing
bool draw legacy text mask
u32 legacy mask fill type
color legacy mask color
f64 dimension scale
i32 dimension-scale source
UUID source dimension style
4 × u8 line-color source
4 × color line colors
4 × u8 line-plot-color source
4 × color line plot colors
2 × u8 plot-weight source
2 × f64 plot weights
f64 fixed extension length
bool fixed extension length enabled
f64 text rotation
i32 alternate tolerance resolution
f64 tolerance text-height fraction
2 × bool suppress arrow 1, suppress arrow 2
i32 text-move leader mode
i32 arc-length symbol
f64 stack text-height fraction
u32 stack format
3 × f64 alternate rounding, rounding, angular rounding
4 × u32 alternate zero suppression, obsolete tolerance zero suppression,
  zero suppression, angular zero suppression
bool alternate text below main text
3 × u32 arrow type 1, arrow type 2, leader arrow type
3 × UUID arrow block IDs
```

The text-style index is the V5 referenced index in V1–V5 archives and is the
unset `i32` value in V6 and later archives. A `color` is four channel bytes.
The three child chunks and later fields are appended in minor gates:

```text
minor >= 1:
  u32 obsolete leader content type
  2 × u32 obsolete text and leader vertical alignment
  2 × u32 leader content angle style and leader curve type
  f64 leader content angle
  bool leader has landing
  f64 leader landing length
  2 × u32 obsolete text and leader horizontal alignment
  2 × bool draw forward, signed ordinate
  anonymous scale-value child
  u32 dimension-style unit system
minor >= 2: anonymous font-characteristics child
minor >= 3: anonymous text-mask child
minor >= 4: 12 × u32 text locations, alignments, orientations, and angle styles;
  bool text underlined
minor >= 5: 2 × u32 obsolete primary and alternate dimension unit systems
minor >= 6: 2 × u32 primary and alternate dimension length-display modes
minor >= 7: u32 center-mark style
minor >= 8: bool force dimension line; u32 text fit; u32 arrow fit
minor >= 9: u32 decimal separator
minor >= 10: bool use kerning
minor >= 11: f64 line-space scale
```

The scale-value, font-characteristics, and text-mask values are anonymous
child chunks and are bounded independently. A major-1 reader consumes the
minor-0 prefix and each gate through minor 11. `use kerning` enables kerning
when glyph placement is computed. `line-space scale` is the dimensionless
multiplier applied to line spacing. For a minor greater than 11, the reader
consumes this known prefix and leaves later bytes to the containing anonymous
chunk boundary. A non-major-1 chunk is rejected.

Sizes, baseline spacing, fixed extension length, leader landing length, and
plot weights are length values. Scale factors, rotations, fractions, rounding
values, colors, enums, and override bits are not scaled.

Section-style anonymous records use major 1. Minor 1 is the current writer
version; later minor values retain the known model-component prefix and
extension item grammars. The writer emits non-default extension items in
strictly increasing code order, then writes code `0`. The reader consumes one
item at a time through the same ascending cascade. Code `0` ends the stream;
a code lower than or equal to the last consumed code is consumed only as an
ID and its value remains an untyped suffix. A code greater than 11 is a future
extension: its ID is consumed, but its value has no generic width and remains
bounded by the anonymous chunk. The reader does not require a terminator after
an out-of-order or future ID because the cascade has ended at that ID.

Bounded font, text-style, dimension-style, hatch, rendering-attribute,
texture-mapping, material, group, and light readers consume their known
major-1 prefixes and skip later suffix bytes before the containing bound.
Tagged object-attribute and layer streams retain their one-byte item grammar;
an unknown item has no generic value width. Writer-band ceilings and explicit
terminators remain part of those grammars.

Modern `ON_Text` class data is an anonymous version-1.0 child containing the
common annotation structure. Modern `ON_Leader` class data is an anonymous
version-1.1 child containing that structure followed by an archive array of
leader `ON_2dPoint` values. The annotation and text-content children close
independently; later bytes remain at their child boundaries, and bytes after
the text or leader child remain at the enclosing `TCODE_OPENNURBS_CLASS_DATA`
boundary. V5 text and leader classes contain outer anonymous version 1.0 and
the common V5 annotation chunk described in section 18.

The text-dot class UUID is
`74198302-CDF4-4F95-9609-6D684F22AB37`. Its class-data payload is direct
class data, not an anonymous child:

```text
packed version 1.minor
ON_3dPoint center point
i32 height in points
UTF-16 primary text
UTF-16 font face
i32 display bits
if minor >= 1: UTF-16 secondary text
```

The packed version is one byte: the high nibble is the major and the low
nibble is the minor. The writer emits version `1.0` for archive versions below
60 and version `1.1` for archive version 60 and later. The reader requires
major 1. Display bit `0x01` means always on top, `0x02` means transparent,
`0x04` means bold, and `0x08` means italic. Other display bits have no defined
meaning and are ignored. A major-1 reader consumes the known prefix selected
by the minor and skips remaining bytes at the bounded class-data end. Therefore
V4 and V5.0 text dots have no serialized secondary-text field and read with an
empty secondary string; V6 and later text dots serialize that field.

CADIR maps the class data to one `native.rhino.text_dots` record. Center
coordinates are model-space lengths and are converted to millimeters using the
document unit scale. Height in points, both UTF-16 strings, font face, and the
four display flags are not scaled. The record links to its object record and
does not create a neutral model point or other geometry carrier.

In archive versions 2 through 4, the V5 text and leader class-data payload has
no outer anonymous chunk. It begins with packed version 1.0 and stores the
common fields through text height directly. The direct form omits justification,
model-space text scaling, text formula, and separate style indices. Its user
text equals the displayed text and model-space text scaling is false.

V5 annotations use world X as the reference horizontal vector. Its plane-space
direction is `(dot(world-X, plane-X), dot(world-X, plane-Y))`. V5 angular
dimensions store the two extension-line origin offsets in
`ON_AngularDimension2Extra` userdata UUID
`A68B151F-C778-4A6E-BCB4-23DDD1835677`; its class UUID and item UUID are the
same. Its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`. OpenNURBS writes its generic
class-userdata header as version 2.2 with copy count 1 and the identity
transform, followed by an anonymous payload version 1.0 and two model-length
`f64` values in order: first extension-line origin offset, second
extension-line origin offset. The OpenNURBS 5 application UUID is used so a
V6 save-as-V5 retains this item; V4 does not write it. When the userdata is
absent, both offsets are `-1.0`. A negative offset disables the override.

V5 text objects may carry `ON_OBSOLETE_V5_TextExtra` userdata as defined in
section 7.2.2. Its mask settings are text-object state, not a neutral
annotation field. During V5-to-modern conversion, OpenNURBS applies the
factor multiplied by the V5 text height as the modern mask border and maps
color source `0` to viewport background and source `1` to the stored color.

The V5 text-style record stores packed version 1.2, the archive index, a
UTF-16 description, 64 fixed `u16` Windows LOGFONT face-name slots, and, for
minor 1 and later, separate `i32` font weight, `i32` italic, and obsolete
`f64` line-feed ratio fields. Minor 2 and later append the text-style UUID. It
does not store the modern mixed-radix font-characteristics word. Its
description becomes the PostScript name only when it is nonempty, is not
`Default`, and the archive runtime is Apple or the writer version is later
than 23 February 2018.

The font and dimension-style tables are a compatibility pair. The writer
opens `TCODE_FONT_TABLE` for every archive version. For an archive below
version 60 it writes one font record for each archived dimension style, using
that style's font characteristics and copying its name and archive index into
the compatibility `ON_TextStyle`; the table is then closed before the
dimension-style table is written. For archive version 60 and later the font
table is empty. The dimension-style writer instead writes the unset `i32`
text-style index and, at minor 2 and later, writes the same font-characteristics
payload as an anonymous `ON_Font` child. The dimension-style reader loads the
legacy font table before reading dimension styles and resolves the legacy
index from that table; its modern reader reads the bounded font child.

CADIR decision: legacy font-table records are typed `text_styles` records.
The modern font child remains a bounded child of its owning `dimension_styles`
record under `controls.font_characteristics`, represented by its source offset,
byte length, and SHA-256. CADIR does not create a second text-style identity
for this embedded resource. The child uses the font grammar above, and its
complete source record remains available through native-record fidelity.

### 20.4 Views and document presentation

A version-1 settings stream is a flat sequence after the 32-byte file header
and comment. It has no settings-table wrapper. The version-1 writer emits a
long `TCODE_UNIT_AND_TOLERANCES` record with this body:

```text
i32 structure version = 1
i32 length-unit code
f64 absolute tolerance
f64 relative tolerance
f64 angle tolerance in radians
```

Length-unit codes are 0 none, 1 microns, 2 millimeters, 3 centimeters,
4 meters, 5 kilometers, 6 microinches, 7 mils, 8 inches, 9 feet, and 10
miles. The version-1 reader also accepts these legacy presentation records:

```text
long TCODE_NAMED_CPLANE
  long TCODE_NAME: i32 ASCII byte count, ASCII name bytes
  long TCODE_CPLANE:
    ON_Point origin
    ON_Vector x axis
    ON_Vector y axis
    f64 grid spacing
    i32 grid line count
    i32 thick-line frequency
  short TCODE_ENDOFTABLE = 0

long TCODE_NAMED_VIEW
  long TCODE_NAME: i32 ASCII byte count, ASCII name bytes
  long TCODE_CPLANE: as above
  long TCODE_VIEW:
    i32 projection
    i32 valid flag
    ON_Point target
    f64 angle 1
    f64 angle 2
    f64 angle 3
    f64 view size
    f64 camera distance
  short TCODE_SHOWGRID
  short TCODE_SHOWGRIDAXES
  short TCODE_SHOWWORLDAXES
  short TCODE_ENDOFTABLE = 0

long TCODE_VIEWPORT
  long TCODE_CPLANE: as above
  long TCODE_VIEW: as above
  short TCODE_SHOWGRID
  short TCODE_SHOWGRIDAXES
  short TCODE_SHOWWORLDAXES
  long TCODE_SNAPSIZE: f64 legacy snap size
  long TCODE_NEAR_CLIP_PLANE: f64 legacy near-clip distance
  long TCODE_HIDE_TRACE: empty legacy hide-trace marker
  long TCODE_VIEWPORT_POSITION: 4 × f64 window bounds
  long TCODE_VIEWPORT_TRACEINFO:
    ON_Point image origin
    ON_Vector image x axis
    ON_Vector image y axis
    f64 image width
    f64 image height
    i32 ASCII byte count, ASCII image path bytes
  long TCODE_VIEWPORT_WALLPAPER:
    i32 ASCII byte count, ASCII wallpaper path bytes
  short TCODE_MAXIMIZED_VIEWPORT
  short TCODE_VIEWPORT_V1_DISPLAYMODE
  short TCODE_ENDOFTABLE = 0
```

The legacy presentation child order is not significant. Unknown complete
children are skipped before the end marker. A missing or malformed nested end
marker fails that presentation record. `TCODE_VIEWPORT_TRACEINFO` and
`TCODE_VIEWPORT_WALLPAPER` contain their path payload directly; the path is
not a nested `TCODE_NAME` chunk. A V1 file may omit the EOF marker after the
flat settings stream.

CADIR decision: the V1 decoder transfers the unit/tolerance record to neutral
tolerances, retains complete V1 named-construction-plane, named-view, and
viewport records as opaque source records, and reports
`presentation.record-dropped` for each. It does not place these legacy records
in the modern `views` or `construction_planes` arenas. The top-level short
`TCODE_ENDOFTABLE` is structural and is not retained.

A view record is an ordered child-chunk list terminated by `TCODE_ENDOFTABLE`.
The `TCODE_VIEW_CPLANE` child has this body:

```text
u8 packed version = 1.minor
ON_Point origin: 3 × f64
ON_Vector x axis: 3 × f64
ON_Vector y axis: 3 × f64
ON_Vector z axis: 3 × f64
ON_PlaneEquation x, y, z, d: 4 × f64
f64 grid spacing
f64 snap spacing
i32 grid line count
i32 thick-line frequency
UTF-16 name
if minor >= 1: bool depth-buffer flag
```

The construction-plane writer emits packed version 1.1 in every archive band.
The reader requires major version 1, initializes the depth-buffer flag to true,
reads the flag for minor 1 and later, and skips remaining bytes at the child
boundary. Plane origins, the plane-equation `d` value, grid spacing, and snap
spacing are document lengths; plane axes and the equation normal are
dimensionless. The viewport child stores packed version 1.0 through 1.5,
validity flags, projection, camera location and frame vectors, six frustum
coordinates, six integer port bounds, viewport UUID, five camera/frustum lock
flags, target point, camera-frame validity, and three dimensionless view-scale
values. Camera locations, targets, and frustum coordinates are length values.
Camera axes and view scale are not scaled.

The viewport writer emits packed version 1.5 in every archive. The reader
requires major version 1 for typed viewport fields and reads the viewport UUID
at minor 1, the five camera and frustum locks at minor 2, the target point at
minor 3, camera-frame validity at minor 4, and the three view-scale values at
minor 5. Remaining bytes are skipped at the `TCODE_VIEW_VIEWPORT` child
boundary. CADIR decision: a viewport with another major remains in the
retained view child without typed viewport fields; no future-major field is
inferred from the packed prefix.

A saved or active view list has this body:

```text
i32 count
repeat count times:
  long TCODE_VIEW_RECORD
    ordered view-child stream
```

The named-construction-plane list has the same count framing, with one long
`TCODE_VIEW_CPLANE` child per count. The named-view and active-view lists use
long `TCODE_VIEW_RECORD` children. The list records are CRC-bearing; their CRC
covers the count and any direct suffix bytes and excludes each complete child
chunk. Each child has its own CRC and bounded body. The readers require
the defined child typecode for every counted child; an unexpected typecode or
short framing fails that list read.

CADIR rejects a negative count or a count above 65536 before it frames any
child. This is a CADIR admission bound, not a change to the on-disk `i32`
count field.

A view record writes these children in order when their archive-version gates
are met:

```text
long  TCODE_VIEW_VIEWPORT
long  TCODE_VIEW_VIEWPORT_USERDATA       (archive >= 4 and userdata exists)
long  TCODE_VIEW_CPLANE
long  TCODE_VIEW_TARGET
short TCODE_VIEW_V3_DISPLAYMODE
long  TCODE_VIEW_POSITION
short TCODE_VIEW_SHOWCONGRID
short TCODE_VIEW_SHOWCONAXES
short TCODE_VIEW_SHOWWORLDAXES
long  TCODE_VIEW_NAME
long  TCODE_VIEW_TRACEIMAGE
long  TCODE_VIEW_WALLPAPER
long  TCODE_VIEW_WALLPAPER_V3             (archive >= 3)
long  TCODE_VIEW_ATTRIBUTES               (archive >= 4)
short TCODE_ENDOFTABLE = 0
```

`TCODE_VIEW_POSITION` is a long child. Its body is:

```text
packed version 1.0 below archive version 5; 1.1 at archive version 5 and later
i32 maximized flag
f64 normalized window left
f64 normalized window right
f64 normalized window top
f64 normalized window bottom
if minor >= 1: u8 floating viewport monitor count
```

The writer emits the version shown for the archive band. The reader initializes
the position to `maximized = false`, bounds `[0,1,0,1]`, and floating viewport
`0`. A major other than 1 leaves those defaults and consumes no versioned
fields. For major 1, a nonzero maximized value is true; every minor at least 1
reads the floating-viewport byte. Each horizontal and vertical pair is repaired
in order: swap its endpoints when the lower value is greater than the upper
value, clamp the lower value below 0 to 0, clamp the upper value at or above 1
to 1, and replace the pair with `[0,1]` when the lower value is not less than
the upper value. Remaining bytes are skipped at the `TCODE_VIEW_POSITION`
boundary. CADIR stores the packed version, repaired bounds, maximized flag, and
floating-viewport byte in the native view's `window_position` value.

The view reader accepts these children in any order, skips unknown complete
chunks, and stops typed-child decoding at the short zero-valued
`TCODE_ENDOFTABLE`. Bytes after that marker remain an untyped suffix through
the `TCODE_VIEW_RECORD` boundary. The marker is required; a missing or
non-short/nonzero marker fails the view read. A view enters the typed `views`
arena only after its known children and marker parse successfully. If a framed
view record fails child parsing, it is omitted from the typed arena and emits a
`presentation.record-dropped` loss; no synthetic identity, visibility, or child
record is created. If a counted view child has the wrong type or the list
cannot frame a later child, parsing stops at that bounded failure and emits the
same loss for that record boundary. CADIR decision: a malformed
named-construction-plane list is omitted as a whole and emits the same loss.

View-attributes packed versions 1.1 through 1.9 add view type; page dimensions;
display-mode UUID; anonymous page settings; projection lock; an array of
versioned clipping-plane equations, UUIDs, enabled flags, and depths; named-view
UUID; construction-Z-axis flag; focal-blur values; rendering pixel size; and
section behavior. Page sizes and margins are millimeters already. At outer minor
2 and later, the display-mode UUID is followed by an anonymous page-settings
child:

```text
i32 page-settings major = 1
i32 page-settings minor >= 0
i32 page number
f64 width in millimeters
f64 height in millimeters
f64 left margin in millimeters
f64 right margin in millimeters
f64 top margin in millimeters
f64 bottom margin in millimeters
UTF-16 printer name
```

The page-settings writer emits version 1.0. Its reader requires major version 1,
accepts every nonnegative minor, and skips later bytes at the anonymous child
boundary. The nested view-attribute clipping-plane record has major version 1.
Minor 0 has no depth;
minor 1 and 2 add a depth whose legacy enabled state is true only for a
nonnegative value other than `1.234321e38`; minor 3 and every later minor store
an explicit depth-enabled flag. A bounded reader consumes this known prefix and
skips any suffix before the clipping record's bounded end. A standalone
clipping-plane object's separate record uses the minor-0-through-5 grammar in
section 13.4, including the minor-5 participation items.

Each known view child consumes its bounded known prefix and skips a suffix
before that child boundary. Unknown child chunks before the end marker are
skipped as complete bounded chunks. `TCODE_ENDOFTABLE` terminates the typed
view-child stream; later bytes remain bounded suffix data.

The view-attributes writer emits packed version 1.9 whenever the child is
written, which is for archive version 4 and later. Its reader consumes the
known fields at the minor gates 1 through 9 and skips later bytes at the
`TCODE_VIEW_ATTRIBUTES` boundary. The anonymous page-settings child and each
anonymous clipping-plane child close before the next direct field; a suffix in
one child cannot become a field of the enclosing attributes record.

`TCODE_VIEW_TRACEIMAGE` contains packed version `1.3` below archive version 60
and `1.4` at archive version 60 and later:

```text
packed version 1.minor
UTF-16 legacy image path
f64 image width
f64 image height
ON_Plane image plane
if minor >= 1: bool grayscale
if minor >= 2: bool hidden
if minor >= 3: bool filtered
if minor >= 4: anonymous file-reference child
```

The image width and height, plane origin, and plane-equation offset use document
length units; plane axes are dimensionless. The file reference is the section
20.1 anonymous major-1 child. A major other than 1 is rejected. Remaining bytes
are skipped at the trace-image child boundary.

The trace-image writer emits packed version 1.3 below archive version 60 and
1.4 at archive version 60 and later. The wallpaper V3 writer emits packed
version 1.1 below archive version 60 and 1.2 at archive version 60 and later.
The readers consume the file-reference child only at the corresponding minor
gate and leave later bytes at the enclosing `TCODE_VIEW_TRACEIMAGE` or
`TCODE_VIEW_WALLPAPER_V3` boundary.

`TCODE_VIEW_WALLPAPER` is the legacy long child containing only a UTF-16 path.
`TCODE_VIEW_WALLPAPER_V3` contains packed version `1.1` below archive version
60 and `1.2` at archive version 60 and later:

```text
packed version 1.minor
UTF-16 legacy wallpaper path
bool grayscale
if minor >= 1: bool hidden
if minor >= 2: anonymous file-reference child
```

The V3 wallpaper child overrides the legacy wallpaper path and its display
flags. Its reader requires major version 1 and skips remaining bytes at the V3
child boundary. The legacy child supplies path data only; its default flags are
grayscale true and hidden false until the V3 child is read. Trace images and
wallpaper retain their file-reference state in the native view record.

`ON_WindowsBitmap` class data has no version prefix. `ON_WindowsBitmapEx` starts
with packed version `1.minor` and a UTF-16 file path. Its writer emits `1.0`;
its reader accepts every minor with major `1`. The common Windows bitmap header
is exactly 40 bytes:

```
i32 header size
i32 width in pixels
i32 height in pixels
u16 planes
u16 bits per pixel
i32 compression
i32 image byte count
i32 horizontal pixels per meter
i32 vertical pixels per meter
i32 colors used
i32 important colors
```

The writer emits header size `40`. Let `C` be `colors used` when nonzero; when
it is zero, `C` is `2` for 1-bit pixels, `16` for 4-bit pixels, `256` for
8-bit pixels, and `0` otherwise. The palette byte count is `4*C`, and the
image byte count is the nonnegative image byte-count field.

The class writers emit §10 compressed buffers. The archive-version-1
`ON_WindowsBitmap` reader additionally accepts raw palette bytes followed by
raw image bytes. Except for that legacy reader branch, the
`ON_WindowsBitmap` and `ON_WindowsBitmapEx` readers use §10 compressed buffers.
`ON_WindowsBitmap` writes the compressed-buffer body without a version prefix.
`ON_WindowsBitmapEx` writes packed version 1.0, its UTF-16 file path, and that
same compressed body. `ON_EmbeddedBitmap` writes packed version 1.1, method 1,
the compressed buffer, and the component UUID and name; its reader also admits
the minor-0 prefix without those identity fields.
The first buffer declares either `4*C + image byte count` and contains the
combined palette and image, or `4*C` and contains the palette alone. When it
declares the palette alone and the image byte count is nonzero, a second buffer
follows and declares exactly the image byte count. A zero image byte count ends
the sequence after the first buffer. After the known buffers, remaining bytes
belong to the bounded class-data suffix.

`ON_EmbeddedBitmap` class data uses packed version `1.minor`. Its common prefix
is:

```
UTF-16 file path
u32 image CRC32
i32 image compression method
```

Method 0 stores `u32 byte count` followed by that many raw image bytes. Method 1
stores a compressed buffer using §10. The writer emits method 1. Minor 0 ends
after the buffer. Minor 1 and later append the component UUID and UTF-16 name.
The reader accepts major 1, applies those minor gates, and leaves any remaining
bytes before the bounded class-data end as a suffix.

The `TCODE_SETTINGS_RENDERMESH` and `TCODE_SETTINGS_ANALYSISMESH` records
each contain one direct custom render-mesh body with the grammar defined below
for `TCODE_SETTINGS_ATTRIBUTES`. Their writers emit packed version `1.5`.
The enclosing CRC-bearing settings record is the boundary, so a major-1 reader
applies all known minor gates, including the anonymous version-1.3 SubD child,
and skips later bytes at that outer boundary. The render-mesh body controls
automatic render tessellation; the analysis-mesh body controls automatic
analysis tessellation. They are separate records even though their wire
grammars are identical.

For these two records, the outer CRC covers the bytes written directly by the
mesh body. It excludes the complete anonymous SubD-display child, including
that child's header and CRC. Bytes appended after the child before the outer
record CRC are direct suffix bytes and are included in the outer CRC.

Global annotation settings are the direct body of
`TCODE_SETTINGS_ANNOTATION`. The body starts with a packed one-byte version
`1.minor` (major in the high nibble, minor in the low nibble):

```text
u8 packed version = 1.minor
7 × f64: dimension scale, text height, extension-line extension,
  extension-line offset, arrow length, arrow width, center mark
u32 dimension unit system
i32 arrow type
i32 angular units
i32 length format
i32 angle format
i32 obsolete text alignment
i32 resolution
UTF-16 font face
if minor >= 1: f64 world-view text scale, u8 annotation-scaling flag
if minor >= 2: f64 world-view hatch scale, u8 hatch-scaling flag
if minor >= 3: u8 model-space scaling flag, u8 layout-space scaling flag
if minor >= 4: bool use dimension layer, ON_UUID dimension-layer identity
```

The writer emits minor 2 when the archive version is below 60 and minor 4 when
the archive version is 60 or later. It writes `1.0` for dimension scale even
when the in-memory dimension-scale value differs. Major version 1 is the
reader's admitted family. Dimension units use the values in section 8.2. The
six base dimensions after dimension scale are document-length values; CADIR
transfers them to millimeters with the model unit scale. Dimension scale and
the world-view and hatch scale values are dimensionless. The stored arrow type
is the `ON_Arrowhead` enum ordinal minus 2, so 0 is a solid triangle. Angular
units are 0 degrees and 1 radians. Length-format value 2 selects feet-and-
inches; all other values select model-unit display. Angle-format values are 0
decimal degrees, 1 degrees-minutes-seconds, 2 radians, and 3 gradians.
Resolution is interpreted according to the selected length format. The
dimension-layer identity is used only when its flag is true; a nil identity
selects the current layer.

When a minor version omits a field, OpenNURBS initializes its runtime value
from the archive band: pre-V5 files disable annotation, model-space, layout-
space, and hatch scaling; V5 and later files enable annotation, model-space,
and layout-space scaling and disable hatch scaling. A minor-1 annotation flag
also sets the layout-space flag; a minor-3 layout-space flag then replaces that
value. `CADIR decision:` omitted fields are `null` in the native
annotation-settings record rather than synthesized from the archive-band
runtime default.

Grid defaults are the direct body of the CRC-bearing long
`TCODE_SETTINGS_GRID_DEFAULTS` record:

```text
u8 packed version = 1.minor
f64 grid spacing
f64 snap spacing
i32 grid line count
i32 thick-line frequency
i32 show-grid flag
i32 show-grid-axes flag
i32 show-world-axes flag
```

The grid-default writer emits packed version 1.0 and the reader requires major
version 1; the minor does not gate any field. The default values are grid
spacing 1.0, snap spacing 1.0, grid-line count 70, thick-line frequency 5, and
all three visibility flags true. Grid spacing and snap spacing are document
lengths; CADIR transfers them to millimeters with the document model-unit
scale. A thick-line frequency of 0 disables thick lines, 1 makes every line
thick, and `N >= 2` makes every Nth line thick. The three visibility flags are
nonzero-true `i32` values.

Both bodies are contained directly by their length-bounded top-level settings
records. After the known fields, the outer `TCODE_SETTINGS_ANNOTATION` or
`TCODE_SETTINGS_GRID_DEFAULTS` boundary consumes any remaining suffix.

The `TCODE_SETTINGS_RENDER` record contains one render-settings body. When
`ON_3dmRenderSettings::Write` writes archive version 50 or earlier, it uses the
legacy body. For archive version 60 it uses the legacy body when the recorded
OpenNURBS writer version is earlier than `6.0.2013.11.05`; later V6 writers and
all V7 and V8 writers use the modern body.

The legacy body is a direct sequence. Its first field is an `i32` version in
the inclusive range 100 through 199:

```text
i32 version
i32 custom image size flag
i32 image width in pixels
i32 image height in pixels
ON_Color ambient light
i32 background style
ON_Color background top color
UTF-16 background bitmap path
9 × i32 flags: hidden lights, depth cue, flat shade, backfaces,
  points, curves, isoparams, mesh edges, annotations
i32 antialias style
i32 shadow-map style
i32 shadow-map width in pixels
i32 shadow-map height in pixels
f64 shadow-map offset
if version >= 101: f64 image DPI, i32 image unit system
if version >= 102: ON_Color background bottom color
if version >= 103: bool scale background to fit
```

The legacy flags are nonzero-true `i32` values. The version-103 fit flag is a
one-byte `bool`. The legacy writer stores backfaces as `1` for archive versions
below 3 and otherwise stores the backfaces setting. Background style values are
0 solid color, 1 wallpaper image, 2 gradient, and 3 environment. Antialias
style values are 0 none, 1 normal, 2 medium, and 3 best. Shadow-map style values
are 0 none, 1 normal, and 2 best. The image unit system uses the values in
section 8.2.

The modern body is an anonymous long chunk with two direct `i32` version
fields. Its major version is 1 and its minor version is nonnegative. The known
prefix is:

```text
i32 major version = 1
i32 minor version
bool custom image size
i32 image width in pixels
i32 image height in pixels
f64 image DPI
i32 image unit system
ON_Color ambient light
i32 background style
ON_Color background top color
ON_Color background bottom color
UTF-16 background bitmap path
11 × bool flags: hidden lights, depth cue, flat shade, backfaces,
  points, curves, isoparams, mesh edges, annotations,
  scale background to fit, transparent background
i32 antialias style
i32 shadow-map style
i32 shadow-map width in pixels
i32 shadow-map height in pixels
f64 shadow-map offset
if minor >= 1:
  i32 focal-blur mode
  f64 focal-blur distance
  f64 focal-blur aperture
  f64 focal-blur jitter
  i32 focal-blur sample count
if minor >= 2:
  i32 rendering source
  UTF-16 specific viewport name
  UTF-16 named-view name
  UTF-16 snapshot name
if minor >= 3: bool force viewport aspect ratio
```

Modern booleans are one-byte values. Focal-blur mode values are 0 none, 1
automatic, and 2 manual; these five minor-1 values are compatibility data and
are not used by the OpenNURBS render-settings state after reading. Rendering
source values are 0 active viewport, 1 specific viewport, 2 named view, and 3
snapshot. If custom image size and force viewport aspect ratio are both true,
the image height is derived from the selected viewport aspect ratio rather than
the stored height.

The modern reader requires major version 1 and the archive chunk reader admits
only nonnegative minor versions. A legacy reader rejects versions outside
100–199. A reader consumes only the fields admitted by these gates. Remaining
bytes in a modern body end at the anonymous chunk boundary; remaining bytes in
a legacy body end at the containing `TCODE_SETTINGS_RENDER` record boundary.
The modern `TCODE_SETTINGS_RENDER` record CRC excludes the complete anonymous
render-settings child, including its header and CRC. A direct suffix after
that child is included. The legacy record has no nested body child, so its CRC
covers the direct legacy body.

When the archive version is at least 60 and the render-settings object has
writable userdata, the writer places `TCODE_SETTINGS_RENDER_USERDATA`
immediately after `TCODE_SETTINGS_RENDER`. The record is a CRC-bearing long
chunk. Its body is the class-userdata stream from section 7.2, followed by a
short zero `TCODE_OPENNURBS_CLASS_END` marker. Each item is a long
`TCODE_OPENNURBS_CLASS_USERDATA` chunk. The writer emits userdata version 2.2,
the version-2 header child, and one bounded anonymous payload child. A reader
invokes this stream only after it has successfully read the preceding render
settings record; otherwise it skips the record. Unknown nonzero child chunks
are skipped. The class-end marker stops the stream, and any remaining bytes
through the containing record boundary are a suffix. The anonymous payload is
owned by the userdata class and has no common field grammar.

The outer CRC excludes every complete child chunk through and including the
class-end marker. Bytes after the class-end marker are direct suffix bytes and
are included in the outer CRC. The current writer emits no direct bytes in
this record, so its outer CRC is the CRC of an empty byte sequence.

The current-selector settings records have fixed direct prefixes. The current
layer, wire-density, font, and dimstyle records are short chunks whose value
uses the archive's short-value width. The reader admits current-layer, font,
and dimstyle values from `-1` through `INT32_MAX`; it admits wire-density
values from `-2` through `INT32_MAX`. The writer substitutes zero for an unset
layer, font, or dimstyle index and writes the current wire density directly.

The current-material record is a CRC-bearing long chunk. Its known body prefix
is:

```text
i32 current material index
i32 current material source
```

The writer emits `-1` for an unset material index. The source reader consumes
the signed `i32` without an additional index range gate. Material-source
ordinals are 0 layer, 1 object, and 3 parent.

The current-color record is a CRC-bearing long chunk. Its known body prefix
is:

```text
u8 red
u8 green
u8 blue
u8 alpha
i32 current color source
```

Color-source ordinals are 0 layer, 1 object, 2 material, and 3 parent. These
two long readers consume the known eight-byte prefix and let the enclosing
settings-record boundary skip later direct suffix bytes. Their CRC covers the
complete direct body, including any such suffix. The selectors have no
version prefix; bytes not in the fixed prefixes are bounded suffix data.

The historical settings record `0x2000803e` is a CRC-bearing long chunk. When
its declared length is 28, its body is 24 obsolete bytes followed by the
four-byte CRC. The version-2 reader consumes those 24 bytes only for that
declared length; otherwise it skips the bounded record without assigning a
payload grammar. The current writer never emits this record. CADIR retains a
present record in the bounded `setting_records` arena without assigning typed
fields.

The `TCODE_SETTINGS_PLUGINLIST` record is a CRC-bearing long settings record.
The writer emits it first in the settings stream only for archive versions 4
and later, and only when at least one plugin reference is present. Its known
body is:

```text
u8 packed version = 1.minor
i32 plugin-reference count
count × TCODE_ANONYMOUS_CHUNK plugin-reference record
```

The writer emits outer version `1.0`. The reader requires major version 1 and
accepts every nonnegative minor. The count is nonnegative. A plugin reference
is an anonymous CRC-bearing long chunk with this body:

```text
i32 anonymous major = 1
i32 anonymous minor
ON_UUID plugin identity
i32 plugin-type enum ordinal
UTF-16 plugin name
UTF-16 plugin version
UTF-16 plugin executable filename
if minor >= 1:
  UTF-16 developer organization
  UTF-16 developer address
  UTF-16 developer country
  UTF-16 developer phone
  UTF-16 developer email
  UTF-16 developer website
  UTF-16 developer update URL
  UTF-16 developer fax
if minor >= 2:
  i32 plugin platform
  i32 plugin SDK version
  i32 plugin SDK service release
```

The outer CRC covers the packed version, count, and any direct suffix bytes.
It excludes each complete anonymous plugin-reference chunk, including its
header and trailing CRC.

The plugin-reference writer emits version `1.2`. Its reader requires major
version 1 and accepts every nonnegative minor. The plugin identity identifies
the application plugin whose userdata may be present in the file. The
plugin-type field is the Rhino plugin-type ordinal. Platform values are 0
unknown, 1 C++, and 2 .NET. The SDK fields are the version and service-release
components used by the plugin SDK. The filename is the executable filename;
the remaining strings are developer contact fields. Each reference ends at
its anonymous chunk boundary, and the list ends at the outer
`TCODE_SETTINGS_PLUGINLIST` boundary, so later minor suffixes are skipped at
their respective boundaries.

The `TCODE_SETTINGS_ATTRIBUTES` record is a CRC-bearing long settings record.
Its writer emits packed version `1.7`. The known direct body is:

```text
u8 packed version = 1.minor
f64 linetype display scale
ON_Color current plot color
i32 current plot-color source
i32 V5 current line-pattern index, or -1
i32 current linetype source
if minor >= 1:
  TCODE_ANONYMOUS_CHUNK version 1.0
    direct page-space units-and-tolerances body from §8.2
if minor >= 2: ON_UUID active view
if minor >= 3:
  ON_Point model basepoint
  TCODE_ANONYMOUS_CHUNK version 1.2 earth-anchor body
if minor >= 4: bool save texture bitmaps in file
if minor >= 5: TCODE_ANONYMOUS_CHUNK version 1.0 IO-settings body
if minor >= 6: direct custom render-mesh body
if minor >= 7:
  ON_UUID current layer
  ON_UUID current render material
  ON_UUID current line pattern
  ON_UUID current text style
  ON_UUID current dimension style
  ON_UUID current hatch pattern
```

Plot-color source values are 0 layer, 1 object, 2 display color, and 3
parent. Linetype source values are 0 layer, 1 object, and 3 parent. The
parent values fall back to the layer when no parent exists.

The page-units anonymous child is a boundary wrapper; its body starts with the
ordinary `i32` units structure version from §8.2. The earth-anchor body is:

```text
i32 anonymous major = 1
i32 anonymous minor
f64 latitude in degrees
f64 longitude in degrees
f64 elevation in meters
ON_Point model point
ON_Vector model north
ON_Vector model east
if minor >= 1:
  i32 legacy elevation-reference enum
  ON_UUID anchor identity
  UTF-16 name
  UTF-16 description
  UTF-16 URL
  UTF-16 URL tag
if minor >= 2: i32 earth-coordinate-system enum
```

The earth-anchor writer emits version `1.2`. The legacy elevation-reference
values are 0 ground level, 1 mean sea level, and 2 center of earth. The current
earth-coordinate-system values are 0 unset, 1 ground level, 2 mean sea level,
3 center of earth, 5 WGS1984, and 6 EGM2008. Unknown values retain their
numeric value.

The IO-settings child is:

```text
i32 anonymous major = 1
i32 anonymous minor
bool save texture bitmaps in file
i32 linked-instance-definition update policy
```

The writer emits version `1.0`. Policy values are 1 prompt, 2 always update,
and 3 never update. In archive versions at least 5, a read value of 0 is
normalized to 1.

The direct custom render-mesh body uses packed version `1.minor`; the writer
emits `1.5`:

```text
u8 packed version
i32 compute-curvature flag
i32 simple-planes flag
i32 refine flag
i32 jagged-seams flag
i32 obsolete weld field
f64 tolerance
f64 minimum edge length
f64 maximum edge length
f64 grid aspect ratio
i32 minimum grid count
i32 maximum grid count
f64 grid angle in radians
f64 grid amplification
f64 refine angle in radians
f64 obsolete combine angle
i32 face type
if minor >= 1: i32 texture range
if minor >= 2: bool custom settings, f64 relative tolerance
if minor >= 3: u8 mesher selector
if minor >= 4: bool custom settings enabled
if minor >= 5: TCODE_ANONYMOUS_CHUNK version 1.3 SubD-display body
```

The face-type values are 0 mixed triangles and quads, 1 all triangles, and 2
all quads. Texture range 1 is unpacked normalized space and 2 is packed
scaled normalized space. Mesher 0 is slow and 1 is fast. The SubD-display
body stores:

```text
i32 anonymous major = 1
i32 anonymous minor
i32 adaptive display density
i32 SubD component location
if minor >= 2: bool display density is absolute
if minor >= 3: bool compute curvature
```

SubD component locations are 0 unset, 1 control net, and 2 limit surface.
The custom render-mesh record is direct, so it has no boundary separate from
the containing settings-attributes body. Typed admission therefore ends at
writer version `1.5`; a later custom-mesh minor cannot be skipped before the
version-1.7 current-component UUIDs without assigning a field boundary.

The settings-attributes reader requires major version 1, applies the gates
above, and consumes any remaining bytes at the outer
`TCODE_SETTINGS_ATTRIBUTES` boundary. The outer record is admitted in the
settings table for archive versions 4 and later. Its CRC covers every direct
body byte, including the direct custom-render-mesh fields, the six version-1.7
UUIDs, and any direct suffix. It excludes the complete page-units,
earth-anchor, IO-settings, and SubD-display anonymous child chunks.

### 20.5 Byte partition and opaque identity

The 32-byte archive header is typed data. Every long chunk consists of a
structural header, bounded body, and optional structural checksum. Short chunks
are structural header/value pairs. Table and class end markers and the EOF
record are structural. Bodies decoded by the preceding sections are typed.
Every remaining complete table, property, setting, object, userdata, and class
record is one named opaque record identified by its typecode and, when present,
class UUID, userdata class UUID, userdata item UUID, plug-in UUID, object UUID,
or archive offset. These categories partition the archive byte range without
gaps or overlap. An opaque record's identity includes its exact byte length and
SHA-256; retained bytes, when present, cover the complete record.

### 20.6 Extension and version boundaries

The class wrapper contains a class UUID chunk, a class-data chunk, optional
class-userdata chunks, and a class-end chunk. The class-data chunk contains the
fields written by the selected class. An unregistered class UUID has no typed
payload contract because no class reader supplies its field grammar. Its
complete object record is opaque, and the class UUID identifies the class
payload within that record. A class-userdata item with an unregistered class
UUID, item UUID, or plug-in UUID remains part of the complete containing object
record. A dictionary inside that item remains part of the same bounded userdata
payload.

CADIR typed-admission decision: the Rhino codec's typed class registry contains
only the built-in class UUIDs defined by this specification. It admits no
third-party class-data payload, plug-in dictionary ID, or direct plug-in user
record as typed data. An application or plug-in UUID carried by a recognized
built-in userdata carrier does not change that carrier's class ownership. All
unregistered class wrappers, non-standard dictionaries, and direct plug-in
records remain complete opaque records with their source identity and bytes.

CADIR decision: a major version not defined by this specification never enters
typed decoding, even when its class or record UUID is registered. The complete
containing record remains retained, and the codec does not apply a known-major
prefix to a later major. A future major can enter typed decoding only after its
field grammar and neutral admission rule are added to this specification.

CADIR decision: a minor suffix not defined by this specification remains
bounded source bytes in its containing payload and receives no typed field or
neutral value. The complete containing record remains retained. A later minor
can enter typed decoding only after a producer-defined field order and neutral
admission rule are added to this specification.

CADIR decision: the archive container grammar is admitted only for the archive
versions listed in section 1. A syntactically valid positive archive version
outside that list is header-only: inspection reports the header and decoding is
refused. The codec does not infer a table sequence or apply a supported archive
grammar to that version. A later archive version can enter typed decoding only
after its container grammar, table boundaries, and typed admission rules are
added to this specification.

`ON_ArchivableDictionary` has dictionary UUID
`21EE7933-1E2D-4047-869E-6BDBF986EA11`. Its structure is:

```text
TCODE_DICTIONARY, major 1 minor 0
  TCODE_DICTIONARY_ID, major 1 minor 0
    ON_UUID dictionary ID
    i32 dictionary version
    UTF-16 dictionary name
  repeated TCODE_DICTIONARY_ENTRY
    i32 entry type
    UTF-16 entry name
    entry value
  TCODE_DICTIONARY_END
```

The dictionary ID child, each entry, and the dictionary end marker are bounded
chunks. Array values use an `i32` element count followed by their elements;
nested dictionaries use entry type 44 and recurse through this grammar.
Dictionary entry types are stable:

| Value | Type codes |
| --- | --- |
| undefined, Boolean, UInt8, Int8, Int16, UInt16, Int32, UInt32, Int64, Float, Double, UUID, UTF-16 string | 0-12 |
| Boolean[], UInt8[], Int8[], Int16[], Int32[], Float[], Double[], UUID[], UTF-16 string[] | 13-21 |
| Color, Point2i, Point2f, Rect4i, Rect4f, Size2i, Size2f, Font | 22-29 |
| Interval, Point2d, Point3d, Point4d, Vector2d, Vector3d, BoundingBox, Ray3d, PlaneEquation, Xform, Plane, Line, Point3f, Vector3f | 30-43 |
| nested dictionary, obsolete object, MeshParameters, Geometry | 44-47 |

The primitive and geometric values use the encodings in §3. An obsolete or
unsupported entry type is skipped at the entry boundary. A dictionary UUID
other than the standard ID, and the field semantics of a plug-in dictionary,
remain owned by that plug-in.

A direct user-table record has this framing:

```text
TCODE_USER_TABLE_UUID
  ON_UUID plug-in ID
  optional TCODE_USER_TABLE_RECORD_HEADER, major 1 minor 0
    bool last-saved-as-goo
    i32 goo archive version
    i32 goo writer version
TCODE_USER_RECORD
  arbitrary plug-in-owned bytes
TCODE_ENDOFTABLE
```

The `TCODE_USER_RECORD` body is one bounded record. The UUID and optional
header identify the producer context; they do not define an inner plug-in
grammar. A direct record in the user table is opaque when no built-in record
type owns its payload. Its table typecode, record typecode, archive offset, byte
length, and SHA-256 identify the record. `TCODE_USER_TABLE_UUID` is
CRC-bearing: its CRC covers the 16-byte plug-in UUID and any direct bytes, but
excludes the complete optional record-header child. The record-header child
has its own CRC. `TCODE_USER_RECORD` is a long non-CRC chunk.

V5+ object attributes use a major-2 tagged stream. After the UUID and layer
reference, each item is a one-byte ID followed directly by the value grammar
for that ID; there is no per-item length. ID zero terminates the stream. The
known IDs and their values are the fields defined in §9.2. If a later minor
contains an ID outside the known set, the ID is the only byte whose width is
known. The reader stops at that ID and does not guess its value width; the
remaining bytes through the containing attributes chunk boundary are not
reinterpreted as typed fields.

Layer records use the same one-byte ID convention for their post-version
extensions. A later layer ID outside the known set has no generic value
grammar or skip length; the reader consumes only that ID and leaves the
remaining bounded layer payload untyped. An out-of-order or gate-inadmissible
ID has the same boundary rule. A later major payload version or later minor
suffix uses the same rule when its fields are not defined: only its containing
chunk boundary is available for preservation.

The settings table has a different boundary rule. Each top-level settings item
is a length-bounded chunk. An unknown top-level typecode is skipped by ending
that chunk, without assigning a payload grammar. The counted named-view,
active-view, and named-construction-plane lists require each child to have its
defined child typecode and long framing; an unexpected child type is a read
failure, not a generically skippable future item.

Opaque records do not select neutral fields or partial typed state. Retained
bytes, when present, cover the complete record boundary.

Typed transfer is atomic per object record. A class UUID selects a payload
grammar, but it does not by itself admit the object. All positional slots,
cross-record references, finite-value gates, and topology invariants required
by that grammar must pass. If one required invariant fails, the complete
object record remains opaque and no partial typed topology is committed.
