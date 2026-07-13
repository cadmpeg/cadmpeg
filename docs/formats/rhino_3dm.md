# Rhino 3DM format

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

The first 32 bytes are:

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

`ON_Color` is four direct color bytes written in memory order. It does not use
numeric endian conversion. `ON_4fColor` is four little-endian `f32` values in
red, green, blue, alpha order.

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

Bit `0x00004000` is reserved and is zero in valid typecodes.

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

Negative long lengths, arithmetic overflow, and an end beyond the containing
bound are framing failures. A V1 typecode zero may be accepted as the legacy
long-chunk form; this exception does not apply to V2 and later.

### 4.1 Checksums

For V2 and later, a long chunk with `TCODE_CRC` set ends with a four-byte
little-endian CRC32. The declared length includes those four bytes. CRC32 covers
all body bytes before the CRC and excludes the chunk header and stored CRC.

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
CRC16(seed=0, "123456789")  = 0x31c3
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
length is exactly the file-size field width:

```text
archive version < 50   length = 4, u32 file_size
archive version >= 50  length = 8, u64 file_size
```

The stored size includes the 32-byte header, all preceding chunks, the EOF
typecode, the EOF value field, and the file-size field. It has no CRC. In V2+
the stored size must equal the complete input length. V1 may omit EOF; interior
legacy `ENDOFFILE_GOO` markers are not document termination.

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
| object record type         | `0x82a00071` |
| object attributes          | `0x02008072` |
| attribute userdata         | `0x02000073` |
| object history             | `0x02008074` |
| history header             | `0x02008075` |
| history data               | `0x02008076` |
| object record end          | `0x82a0007f` |

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
record chunks followed by a short `TCODE_ENDOFTABLE` whose value is zero.
Every record is contained within the table bound.

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
anonymous version 1.0 values
  i32 value_count
  value_count × history value anonymous chunk
if minor >= 1: i32 record_type
if minor >= 2: bool copy_on_replace
```

An `ON_UuidList` is an anonymous version 1.0 chunk containing an archive array
of UUIDs. Descendant order is serialized order. Antecedents identify input
objects and descendants identify output objects. `record_type` is 0 for update
history parameters and 1 for feature parameters.

Each history value is an anonymous version 1.0 chunk:

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
|         11 | archive array of UUIDs                       |

Every value is independently bounded by its anonymous chunk. The next value or
record suffix begins at that chunk's declared end.

### 7.2 Class userdata

A class userdata chunk begins with a packed version byte.

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

Surrogate pairs count as two code units. Nonzero strings end with a zero code
unit. The archive `size_t` destination type does not change either file count.

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

The legacy V1 structure is:

```text
i32 version
i32 unit system
f64 absolute tolerance
f64 relative tolerance
f64 angle tolerance
```

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
|  28 | clipping proof `bool` and UUID list          |
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
method 1: zlib/DEFLATE bytes
```

The CRC covers the uncompressed bytes. Inflated output has exactly the declared
size. Unknown methods, zlib failure, truncation, and checksum failure make the
buffer invalid.

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
| `ON_Layer`               | `95809813-E985-11D3-BFE5-0010830122F0` |
| `ON_InstanceDefinition`  | `26F8BFF6-2618-417F-A158-153D64A94989` |
| `ON_InstanceRef`         | `F9CFB638-B9D4-4340-87E3-C56E7865D96A` |
| `ON_3dmObjectAttributes` | `A828C015-09F5-477C-8665-F0482F5D6996` |

The legacy `TL_RevSurface` UUID `0A8401B6-4D34-4B99-8615-1B4E723DC4E5` is an
accepted alias for the native revolution payload. `ON_Circle` and `ON_Arc` are
value types; their object wrapper is `ON_ArcCurve`.

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
nondecreasing. Each child is a curve.

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

Packed version `1.1`; major 1 is accepted:

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
domain controls parameterization.

### 13.4 Revolution surface

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
present profile is a curve.

### 13.5 Sum surface

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

Width is 1 when vertex count is below 256, 2 when below 65536, and 4
otherwise. Indices are little-endian unsigned values. A triangle is
`[v0,v1,v2,v2]`; a quad is `[v0,v1,v2,v3]`.

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
minor >= 4 and writer version >= 200606010: anonymous mapping tag
minor >= 5: manifold, oriented, solid bytes
minor >= 6: ngon-present byte and optional ngon chunk
minor >= 7: double-vertex-present byte and optional double-vertex chunk
minor >= 8: serialized vertex bounding box
```

The mapping tag is version 1.0 or 1.1: mapping UUID, `i32` CRC, sixteen
transform doubles, and for 1.1 a `u32` mapping type. Ngon records contain a
`u32` count followed by each boundary vertex count, face count, vertex indices,
and face indices. Double vertices contain a `u32` count and, when nonzero, a
compressed `3*f64` channel. A valid double channel has exactly the declared
mesh vertex count and finite values.

## 15. Brep

`ON_Brep` major 3 uses payload version 3.minor, with minors 0 through 3:

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

Polymorphic C2/C3/surface arrays are anonymous version 1.0, then `i32 count`
and for each slot an `i32 present` flag followed by one polymorphic object when
present is 1. Zero denotes a positional null slot. Vertices, edges, trims,
loops, and faces are raw anonymous version 1.0 arrays with a packed `1.0` byte
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
an `i32` flag.

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

Negative material channels map to zero. Every positional index must match its
record index; references must be in range and non-null where required. Trim
domains and loop rings must be finite, endpoint-continuous, and closed.

### 15.5 Mesh sides, solid state, and regions

For Brep minor at least 1, render and analysis side chunks each contain one
byte per face; nonzero is followed by a polymorphic object which must be an
`ON_Mesh`. These are cache channels and do not alter Brep topology.

For minor at least 2, `i32 is_solid` is 0 unset, 1 solid/outward, 2
solid/inward, and 3 not-solid. Other values become unset.

For minor 3, the region wrapper is anonymous version 1.0, followed by a
presence byte and, when present, a version-1.0 region-topology object. The
object contains a face-side array and region array, each with version 1.0 and
an `i32` count. Before archive 60, arrays contain raw anonymous element chunks;
at archive 60 and later, arrays contain polymorphic objects.

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

For minor below 1, profile count defaults to one when a profile exists and
zero otherwise. For minor below 2, closed outer profiles default both caps to
true; otherwise both defaults are false. The mesh cache is display data and is
not analytic extrusion geometry.

A multiple-profile extrusion stores a polycurve whose segment count is the
profile count. Profile segments are closed and contain one outer segment
followed by inner segments in a common profile plane.

## 17. SubD

`ON_SubD` begins with a one-byte SubDimple presence flag. Zero is empty; one is
followed by an anonymous SubDimple chunk. SubDimple uses anonymous major 1 and
minor 0 through 4. V5/V6 use minor 0; V7/V8 use minor 4.

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

A component pointer is `u32 archive ID` followed by `u8 flags`. Bit 0 is
direction; bits 1 and 2 encode type: `0x2` vertex, `0x4` edge, `0x6` face.
Archive ID zero is null. Edge and face direction bits reverse traversal.

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
use packed version 1 with writer minors 6 or 7. Their order is:

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

V6 through V8 use anonymous version 1.0:

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
if linked: anonymous linked-type version 1.0
```

The linked-type chunk contains file reference, nested depth, linked appearance,
reference-component-settings presence, and optional settings. Static and
linked-and-embedded definitions carry member UUIDs; linked external definitions
normally do not.

`ON_InstanceRef` uses packed version 1.0:

```
definition UUID
ON_Xform transform
ON_BoundingBox bounds
```

Definition membership comes from the definition UUID array, not object
attributes. The reference payload carries the transform and bounding box.

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

Major 3 gates are exact:

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

The mapping tag is an anonymous chunk with packed version 1.0 or 1.1:

```
UUID mapping ID
i32 mapping CRC
16 × f64 mesh transform
minor >= 1: u32 mapping type
```

The ngon chunk is anonymous version 1.0:

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

For Brep minor at least 1, each mesh-side wrapper is anonymous with packed
version byte `0x00`, followed by exactly `face_count` entries:

```
u8 present
if nonzero: polymorphic object
```

The first wrapper is render mesh and the second analysis mesh. A nonzero entry
must contain `ON_Mesh`; wrong-type entries are discarded independently.

For Brep minor 3, the region wrapper is anonymous version 1.0, contains a
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

### 19.5 SubD exact record tables

The SubDimple anonymous chunk is major 1, minor 0 through 4. The outer
`ON_SubD` byte is `has_subdimple`: 0 means no following payload and 1 means
one SubDimple chunk.

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
anonymous version major=1, minor=0
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
  anonymous linked-type version major=1, minor=0
  file-reference record
  i32 nested linked-definition depth
  u32 linked-component appearance
  bool reference-component-settings-present
  if true: referenced-component-settings record
```

`ON_InstanceRef` is packed version 1.0:

```
u8 version = 0x10
UUID definition ID
16 × f64 transform entries
ON_BoundingBox
```

The transform and definition UUID identify the reference. Definition
membership comes from the member UUID array, not object attributes.
