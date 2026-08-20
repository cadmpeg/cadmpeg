<!-- Generated from docs/layouts/rhino.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `rhino` record layouts

Source of truth: [`docs/formats/rhino_3dm.md`](../../docs/formats/rhino_3dm.md).
Table source: `docs/layouts/rhino.toml`.

§3 states the global rule: all numeric values are little-endian. `ON_Color` and
the `ON_UUID` `Data4` run are the stated exceptions and are marked `n/a`.

Covers the file header (§2), the primitive width table (§3), the mixed-endian
UUID wire form (§3.3), chunk framing (§4), the end-of-file record (§5), the
class-UUID chunk body (§7), the compressed-buffer prologue (§10), and the SubD
component base (§19.5). Everything else in the specification is an ordered slot
layout: field order and widths with no stated offsets.

## Composite types

| Type | Bytes | Endianness | Meaning |
| ---- | ----: | ---------- | ------- |
| `on_uuid` | 16 | little | Mixed-endian GUID: `Data1` u32 LE, `Data2` u16 LE, `Data3` u16 LE, `Data4` eight bytes unchanged. |
| `on_3d_point` | 24 | little | Three little-endian f64 values, x/y/z. |
| `on_interval` | 16 | little | Two little-endian f64 values, lower then upper. |

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `0x80000000` | TCODE_SHORT | 0 B | flag bit: the chunk's value field is its payload and no body or checksum follows | §4 |
| `0x00008000` | TCODE_CRC | 0 B | flag bit: for V2 and later, the long chunk ends with a four-byte little-endian CRC32 | §4 |
| `0x00000001` | comment block | variable | the first post-header chunk | §6.1 |
| `0x00007fff` | end of file | variable | long, unchecksummed; declared length is the file-size field width | §6.1 |
| `0xffffffff` | end of table | variable | short chunk whose value is zero, closing a table | §6.1 |
| `0x10000013` | object table | variable | table chunk holding object records | §6.1 |
| `0x20008070` | object record | variable | one object record inside the object table | §6.2 |
| `0x0002fffb` | class UUID | variable | class-identity chunk; its checksum is forced on regardless of TCODE_CRC | §6.4 |
| `0x0002fffc` | class data | variable | class payload chunk | §6.4 |

## `file_header`

Spec §2 · layout: byte offsets · size: 32 B

The version field is right-justified decimal text, not a binary integer: leading ASCII spaces then at least one ASCII digit. Version `5` and version `50` are distinct.

Parsed by:
- `crates/cadmpeg-codec-rhino/src/chunks.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 24 | `magic` | `bytes[24]` | little | spec | The trailing space is part of the magic. · value `"3D Geometry File Format "` |
| 24 | 8 | `archive_version` | `bytes[8]` | little | spec | bytes 24..31 right-justified decimal archive version |

## `uuid_wire_form`

Spec §3.3 · layout: byte offsets · size: 16 B

The only mixed-endian primitive in the format. The worked example is canonical `4ED7D4DD-E947-11D3-BFE5-0010830122F0`, wire `DD D4 D7 4E 47 E9 D3 11 BF E5 00 10 83 01 22 F0`.

Parsed by:
- `crates/cadmpeg-codec-rhino/src/wire.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `data1` | `u32` | little | spec | Data1: u32 little-endian |
| 4 | 2 | `data2` | `u16` | little | spec | Data2: u16 little-endian |
| 6 | 2 | `data3` | `u16` | little | spec | Data3: u16 little-endian |
| 8 | 8 | `data4` | `bytes[8]` | unstated | spec | Not a number: the eight bytes are copied verbatim, so no byte order applies. |

## `long_chunk_header_v2`

Spec §4 · layout: byte offsets · size: 8 B

Archive versions below 50. The length word is `i32` below archive version 50 and `i64` from 50; the 8-byte total here is the below-50 form. `declared_length` bytes of body follow and include the trailing checksum when present.

Parsed by:
- `crates/cadmpeg-codec-rhino/src/chunks.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `typecode` | `u32` | little | spec | Every chunk begins with a little-endian `u32 typecode`. |
| 4 | 4 | `declared_length` | `i32` | little | spec | archive version < 50 i32 |

## `long_chunk_header_v50`

Spec §4 · layout: byte offsets · size: 12 B

Archive versions 50 and above widen the length word to `i64`.

Parsed by:
- `crates/cadmpeg-codec-rhino/src/chunks.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `typecode` | `u32` | little | spec | Every chunk begins with a little-endian `u32 typecode`. |
| 4 | 8 | `declared_length` | `i64` | little | spec | archive version >= 50 i64 |

## `endoffile_record_v50`

Spec §5 · layout: byte offsets · size: 20 B

`TCODE_ENDOFFILE = 0x00007fff` is a long, unchecksummed chunk whose declared length is exactly the file-size field width. The stored size includes the 32-byte header, all preceding chunks, the EOF typecode, the EOF value field, and the file-size field. Below archive version 50 the length and size words are four bytes each and the record is 12 bytes. The 20-byte total is derived from the three stated widths; the spec states no total.

Parsed by:
- `crates/cadmpeg-codec-rhino/src/chunks.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `typecode` | `u32` | little | spec | `TCODE_ENDOFFILE = 0x00007fff` is a long, unchecksummed chunk |
| 4 | 8 | `declared_length` | `i64` | little | spec | archive version >= 50 length = 8, u64 file_size |
| 12 | 8 | `file_size` | `u64` | little | spec | archive version >= 50 length = 8, u64 file_size |

## `class_uuid_chunk_body`

Spec §7 · layout: byte offsets · size: 20 B

One of the two places the specification states a record body size outright.

Parsed by:
- `crates/cadmpeg-codec-rhino/src/objects.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `class_uuid` | `on_uuid` | little | spec | sixteen UUID bytes and four checksum bytes |
| 16 | 4 | `crc32` | `u32` | little | spec | sixteen UUID bytes and four checksum bytes |

## `compressed_buffer_prologue`

Spec §10 · layout: byte offsets · size: 9 B

A zero size ends the buffer immediately: no CRC, method, or body follows, so the prologue collapses to its first four bytes. Method 0 stores the bytes verbatim; method 1 stores one anonymous long chunk whose body is a complete zlib stream.

Parsed by:
- `crates/cadmpeg-codec-rhino/src/chunks.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `uncompressed_size` | `u32` | little | spec | u32 uncompressed size |
| 4 | 4 | `crc32` | `u32` | little | spec | u32 CRC32 of uncompressed bytes |
| 8 | 1 | `method` | `u8` | little | spec | u8 method |

## `windows_bitmap_header`

Spec §20.4 · layout: byte offsets · size: 40 B

The class payload may prepend a packed version and UTF-16 path for ON_WindowsBitmapEx; this record is the common header that follows that optional prefix.

Parsed by:
- `crates/cadmpeg-codec-rhino/src/presentation.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_size` | `i32` | little | spec | i32 header size |
| 4 | 4 | `width` | `i32` | little | spec | i32 width in pixels |
| 8 | 4 | `height` | `i32` | little | spec | i32 height in pixels |
| 12 | 2 | `planes` | `u16` | little | spec | u16 planes |
| 14 | 2 | `bits_per_pixel` | `u16` | little | spec | u16 bits per pixel |
| 16 | 4 | `compression` | `i32` | little | spec | i32 compression |
| 20 | 4 | `image_byte_count` | `i32` | little | spec | i32 image byte count |
| 24 | 4 | `horizontal_pixels_per_meter` | `i32` | little | spec | i32 horizontal pixels per meter |
| 28 | 4 | `vertical_pixels_per_meter` | `i32` | little | spec | i32 vertical pixels per meter |
| 32 | 4 | `colors_used` | `i32` | little | spec | i32 colors used |
| 36 | 4 | `important_colors` | `i32` | little | spec | i32 important colors |

## `on_plane`

Spec §3.2 · layout: ordered slots (no stated byte offsets) · size: 128 B

The plane equation is serialized and is not reconstructed from the axes. Field order and widths are stated; byte offsets are not, but the four 24-byte frames plus the 32-byte equation close the stated 128-byte total.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `origin` | `f64[3]` | little | spec | origin: x y z 3 × f64 |
| 1 | `xaxis` | `f64[3]` | little | spec | xaxis: x y z 3 × f64 |
| 2 | `yaxis` | `f64[3]` | little | spec | yaxis: x y z 3 × f64 |
| 3 | `zaxis` | `f64[3]` | little | spec | zaxis: x y z 3 × f64 |
| 4 | `plane_equation` | `f64[4]` | little | spec | plane equation: x y z d 4 × f64 |

## `on_circle`

Spec §3.2 · layout: ordered slots (no stated byte offsets) · size: not stated

Per-field byte widths are stated; the total is not. An `ON_Arc` appends `ON_Interval angle` to the circle. The three consistency points are on the wire in every payload using `ON_Circle`.

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `plane` | `bytes[128]` | little | spec | ON_Plane plane 128 bytes |
| 1 | `radius` | `f64` | little | spec | f64 radius 8 bytes |
| 2 | `point_at_zero` | `on_3d_point` | little | spec | ON_3dPoint point_at_zero 24 bytes |
| 3 | `point_at_half_pi` | `on_3d_point` | little | spec | ON_3dPoint point_at_half_pi 24 bytes |
| 4 | `point_at_pi` | `on_3d_point` | little | spec | ON_3dPoint point_at_pi 24 bytes |

## `subd_component_base`

Spec §19.5 · layout: byte offsets · size: 10 B

The field list is stated in order; the 10-byte total follows from the three stated widths.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `archive_id` | `u32` | little | spec | u32 archive_id |
| 4 | 4 | `component_id` | `u32` | little | spec | u32 component_id |
| 8 | 2 | `subdivision_level` | `u16` | little | spec | u16 subdivision_level |

## `anonymous_version_prefix`

Spec §5 · layout: byte offsets · size: 8 B

The anonymous form. The packed form is one byte with `major = version >> 4` and `minor = version & 0x0f`. The two forms are not interchangeable.

Parsed by:
- `crates/cadmpeg-codec-rhino/src/extrusion.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `major` | `i32` | little | spec | i32 major |
| 4 | 4 | `minor` | `i32` | little | spec | i32 minor |

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| Object payload grammars (§7-§20) | §7 | Roughly sixty payload layouts. Each states a serialization order and per-field types but no byte offsets and no total size, because version gates and counted arrays move every later field. They are slot layouts, and only the two in §3.2 state a size the slots can be checked against. |
| Materials, linetypes, hatch patterns, fonts, dimstyles, and views (§20.2-§20.4) | §11 | The specification names the field sets for these classes in prose but gives no serialization order and no widths, so there is nothing to tabulate. |
