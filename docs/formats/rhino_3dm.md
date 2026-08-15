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

### 6.4 Class wrapper chunks

| Meaning         |     Typecode |
| --------------- | -----------: |
| class wrapper   | `0x00027ffa` |
| class userdata  | `0x00027ffd` |
| userdata header | `0x0002fff9` |
| class UUID      | `0x0002fffb` |
| class data      | `0x0002fffc` |
| class end       | `0x82027fff` |

The class-data body is owned by the class grammar. It is not a flat sequence
of child chunks: direct fields can occur before, between, and after complete
nested chunks. A class reader consumes each nested chunk at the field that
owns it and validates the declared boundary there. A class wrapper scanner
must not apply one flat child-chunk or checksum range to the complete
class-data body.

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
chunk with major version 1. Minor version 1 adds the record type; minor version
2 adds the copy-on-replace flag:

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

Each history value is an anonymous major-1 chunk:

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
followed by that many polymorphic class wrappers. Every wrapper contains its
geometry class UUID and class-data payload.

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
payloads.

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

Defined structure versions are 100, 101, and 102. Unit values are:

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
geometry or annotation class uses the same versioned class-data fields in V2
as in a later archive carrying that class and payload version; V2 changes the
outer chunk width, not the class-data boundary or the class identity rule.

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
minor >= 15: item 35 embedded section style, item 36 obsolete clipping type
```

The extension stream is item byte, payload, next item byte, terminated by item
zero. Layer visibility and lock state are independent.

The archive layer index is the object-reference key. If two layer records use
one archive index, component registration keeps the original index on the
first record and assigns a distinct unused index to each later record.
References to the original index therefore resolve to the first record.

### 8.4 Rendering attributes

Rendering attributes are shared by layer records and object attributes. The
outer record is a long `TCODE_ANONYMOUS_CHUNK` with a CRC32-selected typecode.
Its payload is:

```text
i32 anonymous major = 1
i32 anonymous minor = 0
i32 material-reference count
count × anonymous material-reference chunk
```

The count is nonnegative. Each material reference is a long
`TCODE_ANONYMOUS_CHUNK` with this payload:

```text
i32 anonymous major = 1
i32 anonymous minor = 0 or 1
UUID plug-in ID                         16 bytes
UUID front-face material ID             16 bytes
i32 obsolete mapping-channel count
obsolete mapping-channel array
minor >= 1:
  UUID back-face material ID            16 bytes
  u8 material source
  u8 reserved[3]
```

The obsolete mapping-channel array contains exactly the declared number of
mapping-channel records. Its count is zero, so no mapping-channel bytes follow
the count. Material-reference minor 0 ends after the empty mapping array.
Minor 1 appends the back-face material UUID, one-byte material-source selector,
and three reserved bytes in that order. Both anonymous chunks end exactly
after their version-gated fields; their counts and nested chunk boundaries
cannot exceed the containing rendering-attributes chunk.

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
minor 13: item 41
```

Default values are empty strings, unset indexes, default rendering attributes,
unset colors, plot weight 0.0, decoration none, wire density 1, visible true,
normal mode, layer selectors, empty groups, model space, nil viewport, empty
display-material list, display order 0, linetype scale 1.0, hatch boundary
hidden, and default frame/label style.

Present tagged items occur in ascending item-ID order. The terminator follows
the last item.

The effective display state is object visibility combined with layer visibility.
Each color, material, linetype, plot color, and plot weight uses the object
value only when its selector selects the object; otherwise it uses the layer or
document value.

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
| `ON_Hatch`               | `0559733B-5332-49D1-A936-0532AC76ADE5` |
| `ON_DetailView`          | `C8C66EFA-B3CB-4E00-9440-2AD66203379E` |
| `ON_NurbsCage`           | `06936AFB-3D3C-41AC-BF70-C9319FA480A1` |
| `ON_MorphControl`        | `D379E6D8-7C31-4407-A913-E3B7040D034A` |
| `ON_Centermark`          | `D46767BA-7E8F-4D9D-9A92-66050219A5B9` |
| `ON_Layer`               | `95809813-E985-11D3-BFE5-0010830122F0` |
| `ON_InstanceDefinition`  | `26F8BFF6-2618-417F-A158-153D64A94989` |
| `ON_InstanceRef`         | `F9CFB638-B9D4-4340-87E3-C56E7865D96A` |
| `ON_3dmObjectAttributes` | `A828C015-09F5-477C-8665-F0482F5D6996` |

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

## 12. Curves and points

### 12.1 Point

Packed version `1.0`; major 1 is accepted. The payload is:

```
u8 version
ON_3dPoint point
```

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

Optional counts are zero or the point count. Flags bit 0 means ordered points;
bit 1 means the plane is set.

### 12.3 Line curve

Packed version `1.0`; major 1 is accepted:

```
u8 version
ON_Line from/to points
ON_Interval domain
i32 dimension
```

The line is bounded. `dimension` is serialized without fallback.

### 12.4 Arc curve

Packed version `1.0`; major 1 is accepted:

```
u8 version
ON_Circle circle
ON_Interval angle
ON_Interval curve domain
i32 dimension
```

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

Parameter count is segment count plus one. Segment parameters are finite and
strictly increasing. Each child is a curve.

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
UUID referenced object
ON_ComponentIndex referenced component
ON_Interval edge domain
ON_Interval trim domain
bool proxy reversed
ON_Interval polyedge segment domain
ON_Interval referenced curve domain
```

The segment and parameter counts obey the polycurve invariants. All domains
are finite. The object UUID and component index persist the source curve,
Brep edge, or Brep trim selection; the reversal and domain fields define its
orientation and parameter mapping inside the polyedge.

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
U-major CV sequence
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

### 13.5 Revolution surface

Packed version `2.0`; majors 1 and 2 are accepted. The presence field is a
one-byte `char`; transpose is an `i32`:

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
present profile is a curve. A profile control point on the revolution axis
produces one exact axis control point at every angular control position.

### 13.6 Sum surface

Packed version `1.0`:

```
u8 version
ON_3dVector basepoint
ON_BoundingBox bounds
polymorphic ON_Curve first
polymorphic ON_Curve second
```

The exact surface is `S(u,v)=basepoint+C0(u)+C1(v)`. For child homogeneous
poles `H0=(wP,w)` and `H1=(vQ,v)`, the surface weight is `wv` and the
homogeneous point is `v(wP)+w(vQ)+wv*basepoint`. U inherits the first curve;
V inherits the second.

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

## 15. Brep

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
index. References must be in range and non-null where required. Trim domains
and loop rings must be finite, endpoint-continuous, and closed.

### 15.5 Mesh sides, solid state, and regions

For Brep minor at least 1, render and analysis side chunks each contain one
byte per face; nonzero is followed by a polymorphic object which must be an
`ON_Mesh`. These are cache channels and do not alter Brep topology.
If a present slot has the wrong class or its bounded mesh payload cannot be
parsed, the slot is discarded independently and the decode report emits
`brep.mesh-cache-degraded` with the diagnostic cause. The Brep remains
admissible when its analytic topology is valid.

For minor at least 2, `i32 is_solid` is 0 unset, 1 solid/outward, 2
solid/inward, and 3 not-solid. Other values remain in native source data and
use the unset neutral fallback. An unset value requires the reader to derive
the solid state and orientation from the Brep.
For an archive writer version before 2 October 2002, the stored value is unset.
A Brep is closed when it has at least one face and every edge has exactly two
trim uses. A closed Brep is solid; another Brep is a sheet.

For minor at least 3, the region wrapper is anonymous major-1, followed by a
presence byte and, when present, a major-1 region-topology object. The object
contains a face-side array and region array, each with major 1 and
an `i32` count. Before archive 60, arrays contain raw anonymous element chunks;
at archive 60 and later, arrays contain polymorphic objects.
When the optional region topology fails its face-side or element invariants, the
complete optional region topology is discarded and the decode report emits
`container.redundant-field-repaired` with the diagnostic cause; the Brep remains
admissible.

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
Defined versions are 1.0 through 1.3. The common fields are:

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

Miter vectors are serialized even when their presence flags are false. Minor
1 appends `i32 profile count`. Minor 2 appends bottom and top cap booleans.
Minor 3 appends an anonymous mesh-cache chunk. The complete 1.3 order is the
common fields, profile count, two caps, and mesh cache.

A present miter normal is unitized. The miter applies only when the unitized
local Z component is greater than `1/64`. A normal that cannot be unitized or
does not exceed this threshold selects the flat cap transform.

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

Instance-definition records are in the instance-definition table. V5 payloads
use packed major version 1 with writer minor 6 or later. Their order is:

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

`ON_InstanceRef` uses packed major version 1:

```
definition UUID
ON_Xform transform
ON_BoundingBox bounds
```

Definition membership comes from the ordered definition UUID array, not object
attributes. The reference payload carries one transform and one bounding box.
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

Legacy dimensions may carry userdata class
`8AD5B9FC-0D5C-47FB-ADFD-74C28B6F661E`. Its anonymous version 1.0 through 1.2
payload is:

```text
UUID parent dimension style
i32 forced arrow position (-1 outside, 0 automatic, 1 inside)
i32 text rectangle count
if count = 7: 28 × i32 rectangle coordinates
if minor >= 1: f64 distance scale
if minor >= 2: UUID detail measured
```

The rectangle count is zero or seven. Distance scale is finite and positive.
Dimension plane origins, plane equation offsets, construction points, angular
radius, kink offsets, and text height use document length conversion. Style
indices, flags, directions, stored angles, and distance scale remain unscaled.
When duplicate records for an attached built-in dimension extension occur, the
first serialized matching record owns the extension state.

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
`ON_OBSOLETE_V5_HatchExtra` userdata class
`3FF7007C-3D04-463F-84E3-132ACEB91062`. Its payload is:

```text
anonymous version 1.minor
ON_UUID ignored_id
ON_2dPoint basepoint
```

The obsolete hatch extension is consumed after reading. Each valid matching
record applies its base point in serialized order, so the last valid record
owns the hatch base point; no such extension remains attached to the hatch.

### 18.3 Detail views

`ON_DetailView` uses an anonymous chunk with major version 1 and a nonnegative
minor:

```text
anonymous detail version 1.minor
  anonymous view-state version 1.minor
    ON_3dmView payload
  anonymous boundary version 1.minor
    raw ON_NurbsCurve payload
  if minor >= 1: f64 page-per-model ratio
```

The child declarations independently bound extensible view state and boundary
geometry. Each child reader consumes its known prefix and skips remaining bytes
before that child's bounded end. The boundary NURBS curve uses the ordinary
NURBS curve layout without a class wrapper. Its control points use document
length conversion. The page-per-model ratio is finite and nonnegative; detail
minor 0 defaults it to zero.

### 18.4 NURBS cages

`ON_NurbsCage` uses anonymous version 1.0:

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

### 18.5 Morph controls

Current `ON_MorphControl` payloads use anonymous version 2.0 or 2.1:

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
The UUID list is `i32 count` followed by that many UUID values. The localizer
list is `i32 count` followed by that many localizer chunks.

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
and 6 distance. Control points, localizer points, localizer distance intervals,
transform translation coefficients, and tolerance use document length
conversion. Vectors, transform linear coefficients, knots, parameters, and
weights are unscaled. Tolerance is finite and nonnegative.

Legacy morph-control major version 1 is the cage variant. Its field order is a
complete NURBS cage, captive UUID list, and start `ON_Xform`. It has no
localizers and defaults tolerance and both option flags to zero or false.

## 19. Exact gates and invariants

This section collects exact version gates, field widths, and invariants for
built-in payload families.

### 19.1 Point and simple-curve gate table

| Class              | Framing     | Written version | Accepted major/minor                                       | Required invariants                                                                       |
| ------------------ | ----------- | --------------- | ---------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `ON_Point`         | packed byte | 1.0             | major 1; minor ignored                                     | three finite coordinates                                                                  |
| `ON_PointCloud`    | packed byte | 1.2             | major 1; minor 0/1/2 gates arrays                          | nonnegative count; optional counts zero or point count                                    |
| `ON_LineCurve`     | packed byte | 1.0             | major 1; minor ignored                                     | finite distinct endpoints; increasing domain; dimension 2 or 3                            |
| `ON_ArcCurve`      | packed byte | 1.0             | major 1; minor ignored                                     | positive radius; finite plane; increasing angle and curve domains                         |
| `ON_PolylineCurve` | packed byte | 1.0             | major 1; minor ignored                                     | at least two points; parameter count equals point count; strict parameter increase        |
| `ON_PolyCurve`     | packed byte | 1.0             | version byte does not alter the bounded layout             | positive segment count; parameter count is segment count plus one; every child is a curve |

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
must contain `ON_Mesh`; wrong-type entries are discarded independently.

For Brep minor 3, the region wrapper is anonymous version 1.1, contains a
one-byte region-topology-present flag, and then one anonymous version 1.0
region-topology object when present. Its face-side and region arrays are each
anonymous version 1.0 with `i32 count`. Before archive 60, entries are raw
anonymous version 1.0 element chunks. At archive 60+, entries are polymorphic
objects. The arrays have no per-entry presence integer. The face-side count is
exactly `2 * face_count`; side positions `2f` and `2f+1` carry directions +1
and -1 for face `f`.

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
uses packed version 1.6. Archive 60 may use packed version 1.7 or the
anonymous V6 form; archives 70 and 80 use the anonymous V6 form.

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

`ON_InstanceRef` is packed major version 1. Minor 0 defines the fields below;
later minors append fields:

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

The first SHA-1 identifies the normalized name and the second identifies the
content. A linked instance definition and a texture image reference use this
same structure. Linked definitions preserve their structure when the archive
contains no local member geometry.

### 20.2 Materials, textures, and mappings

A modern material is an anonymous major-1, nonnegative-minor chunk followed by
model-component attributes. An early archive-60 component-attribute child uses
anonymous type
`0x40008000` and a `u32` presence mask: bits 0 through 4 gate UUID, parent UUID,
archive index, UTF-16 name, and two component-status integers. Later component
attributes use `0x40008002` and independent status bytes. The remaining
material fields are six colors, index of refraction, reflectivity, shine,
transparency, an anonymous texture array, material-channel pairs, shareable and
lighting flags, Fresnel controls, reflection and refraction glossiness, an RDK
instance UUID, and the diffuse-texture alpha switch.
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
texture-space enum, and capped flag. Material channels bind a UUID to an
integer channel.

### 20.3 Drafting resources and annotations

Linetypes store model-component identity, ordered length/type segments, cap and
join styles, width and width units, taper points, and the model-distance flag.
Segment lengths and widths with model units are length values. Hatch patterns
store identity, fill type, description, and hatch lines. Hatch-line base,
offset, and dash values are lengths; angle is radians.

Group and light records use packed major-1 versions. Group minor 0 stores the
index and name; minor 1 and later add the UUID. Light minor 0 stores the base
state; minor 1 and later add length and width; minor 2 and later add hotspot.
Linetype records use anonymous major 1 or 2 versions. Major 1 minor 0 stores
the index, name, and segments; minor 1 and later add the UUID. Major 2 minor 0
stores component attributes and segments; minor 1 and later add the ordered
extension items. Each reader consumes its known prefix and skips remaining
bytes before the bounded end. Unknown linetype extension codes at or above 7
terminate the known extension stream.

Text styles use the legacy packed font format through archive 50 and the
anonymous model-component form in later archives. Font state includes the raw
characteristics word, Windows and PostScript names, Windows and Apple weights,
point size, family and localized names, the ten-byte PANOSE classification,
and rich-text quartet member. Font point size is not a model length.

Dimension-style anonymous versions 1.0 through 1.9 store the common size,
format, resolution, prefix, suffix, alternate-unit, suppression, and parent
fields followed by field-override bits; tolerance values; baseline and text
mask state; scale and source style; display and plot colors, sources, and
weights; fixed extension length; text rotation; arrow suppression and custom
arrow UUIDs; leader curve, landing, content-angle, and alignment state; scale
value, font, and text-mask child chunks; text locations, alignments,
orientations, and angle styles; primary and alternate unit/display modes;
center-mark style; dimension-line, text-fit, and arrow-fit controls; and the
decimal separator. Sizes, baseline spacing, fixed extension length, leader
landing length, and plot weights are length values. Scale factors, rotations,
fractions, rounding values, colors, enums, and override bits are not scaled.

Modern text and leader objects contain the common annotation structure and an
ordered leader point array. V5 text and leader classes contain outer anonymous
version 1.0 and the common V5 annotation chunk described in section 18. Text
dots store packed version, model point, point height, primary and secondary
text, font face, and independent always-on-top, transparency, bold, and italic
bits.

In archive versions 2 through 4, the V5 text and leader class-data payload has
no outer anonymous chunk. It begins with packed version 1.0 and stores the
common fields through text height directly. The direct form omits justification,
model-space text scaling, text formula, and separate style indices. Its user
text equals the displayed text and model-space text scaling is false.

V5 annotations use world X as the reference horizontal vector. Its plane-space
direction is `(dot(world-X, plane-X), dot(world-X, plane-Y))`. V5 angular
dimensions store the two extension-line origin offsets in
`ON_AngularDimension2Extra` userdata UUID
`A68B151F-C778-4A6E-BCB4-23DDD1835677`. The userdata payload is anonymous
version 1.0 followed by two model-length `f64` values. When the userdata is
absent, both offsets are `-1.0`. A negative offset disables the override.

The V5 text-style record stores font weight and italic as separate `i32`
fields. Italic is 0 or 1. It does not store the modern mixed-radix font
characteristics word. Its description becomes the PostScript name only when
it is nonempty, is not `Default`, and the archive runtime is Apple or the
writer version is later than 23 February 2018.

### 20.4 Views and document presentation

A view record is an ordered child-chunk list terminated by `TCODE_ENDOFTABLE`.
The construction-plane child stores packed version 1.0 or 1.1, plane, grid and
snap spacing, grid counts, UTF-16 name, and depth-buffer flag. The viewport
child stores packed version 1.0 through 1.5, validity flags, projection, camera
location and frame vectors, six frustum coordinates, six integer port bounds,
viewport UUID, five camera/frustum lock flags, target point, camera-frame
validity, and three dimensionless view-scale values. Camera locations, targets,
frustum coordinates, construction-plane origins, and grid spacing are length
values. Camera axes and view scale are not scaled.

A saved or active view list has a bounded `i32` count followed by complete view
chunks. A view enters the typed `views` arena only after its child chunks and
end marker parse successfully. If a framed view record fails child parsing, it
is omitted from the typed arena and emits a
`presentation.record-dropped` loss; no synthetic identity, visibility, or child
record is created. If the list cannot frame a later child, parsing stops at the
bounded failure and emits the same loss for that record boundary.

View-attributes packed versions 1.1 through 1.9 add view type; page dimensions;
display-mode UUID; anonymous page settings; projection lock; an array of
versioned clipping-plane equations, UUIDs, enabled flags, and depths; named-view
UUID; construction-Z-axis flag; focal-blur values; rendering pixel size; and
section behavior. Page sizes and margins are millimeters already. The nested
view-attribute clipping-plane record has major version 1. Minor 0 has no depth;
minor 1 and 2 add a depth whose legacy enabled state is true only for a
nonnegative value other than `1.234321e38`; minor 3 and every later minor store
an explicit depth-enabled flag. A bounded reader consumes this known prefix and
skips any suffix before the clipping record's bounded end. A standalone
clipping-plane object's separate record uses the minor-0-through-5 grammar in
section 13.4, including the minor-5 participation items.

Trace images store path, width, height, plane, grayscale, hidden, filtered, and
file-reference state. Wallpaper stores path, grayscale, hidden, and file
reference. Windows bitmap classes store a Windows bitmap header followed by
one or two compressed palette/pixel buffers. `ON_WindowsBitmapEx` prefixes the
bitmap with packed version 1.0 and a UTF-16 file path. Embedded bitmaps store
component identity, file reference, compression method, uncompressed size, and
the image buffer.

Global annotation settings store drafting sizes, unit and format enums, font
face, text and hatch scales, model/layout scaling flags, and optional dimension
layer identity. Grid defaults store grid/snap spacing, line counts, and grid and
axis visibility. Render settings store image dimensions, DPI and units,
ambient and background state, geometry and lighting switches, antialias and
shadow settings, rendering source and view names, and viewport-aspect lock.

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

An unregistered class UUID has no typed payload contract. Its complete object
record is opaque, and the class UUID identifies the class payload within that
record. A class-userdata item with an unregistered class UUID, item UUID, or
plug-in UUID remains part of the complete containing object record. A dictionary
inside that item remains part of the same bounded userdata payload.

A direct record in the user table is opaque when no built-in record type owns its
payload. Its table typecode, record typecode, archive offset, byte length, and
SHA-256 identify the record.

Object attributes and layer extensions use tagged streams without a length for
each item. An item ID outside the defined set makes the complete containing
object or layer record opaque; the following bytes are not reinterpreted as a
new item stream. A later major payload version or a later minor suffix uses the
same rule when its fields are not defined: the complete bounded containing
record remains opaque.

Opaque records do not select neutral fields or partial typed state. Retained
bytes, when present, cover the complete record boundary.

Typed transfer is atomic per object record. A class UUID selects a payload
grammar, but it does not by itself admit the object. All positional slots,
cross-record references, finite-value gates, and topology invariants required
by that grammar must pass. If one required invariant fails, the complete
object record remains opaque and no partial typed topology is committed.
