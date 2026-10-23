# Creo View `.pvz` package (PVS structure and OL geometry): Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

This specification covers the PTC Creo View and ProductView `.pvz` package, its `.pvs` product-structure member, and its `.ol` geometry members. openpvz-rs by Calum Knott, released under the MIT license, is prior open-source work on this format.

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/creo_view_pvz.md`](../layouts/creo_view_pvz.md), generated from `docs/layouts/creo_view_pvz.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. Unknown parts are in [`creo_view_pvz-open-items.md`](creo_view_pvz-open-items.md).

## 1. Package

### 1.1 Container

A `.pvz` file is a ZIP archive with the framing in [`zip.md`](zip.md). Members use the stored (0) or deflate (8) compression method. The archive contains exactly one member whose name ends in `.pvs`. That member is the structure member. A PVS file and an OL file can also occur as loose files outside a package; their byte layout is the same as the member layout.

A member name in the structure member refers to a ZIP member by exact byte equality of the UTF-8 name. Names can contain spaces, non-ASCII characters, and brackets. A reader resolves them byte for byte; it does not apply glob or case folding.

### 1.2 Member roles

| Member name suffix | Role | Defined in |
| ------------------ | ---- | ---------- |
| `.pvs` | Product structure, string pool, and property sets. | §3 |
| `.ol` | Geometry of one component definition: appearances, tessellation, edge tessellation, and exact B-rep. | §4–§8 |
| `.pvi` | Illustration (figure) document. UTF-8 XML with root `galaxy_3di:figure`. | open item |
| `.xml`, `.xlf` | Illustration standards, SBOM, validation report, and XLIFF localisation documents. UTF-8 XML. | open item |
| `.etb`, `.ast`, `.png`, `.gif` | Auxiliary illustration data and images. | open item |

Geometry elements (§3.8) name the `.ol` members. Illustration elements (§3.12) name the `.pvi` members.

## 2. Primitive encodings

### 2.1 Byte order and scalars

All multi-byte integers and reals are little-endian. `f32` is IEEE-754 binary32. `f64` is IEEE-754 binary64. `u32` and `i32` are 32-bit integers.

### 2.2 ULEB128

A ULEB128 value stores seven bits per byte, least significant group first. Bit 7 of a byte is set when another byte follows. The structure member uses ULEB128 for counts, string lengths, definition indexes, and string atoms.

### 2.3 String atoms

A string atom is one ULEB128 code followed by zero or more bytes.

| Code parity | Meaning | Following bytes |
| ----------- | ------- | --------------- |
| even | Inline UTF-8 text of `code / 2` bytes. | `code / 2` bytes of UTF-8 |
| odd | Reference to string-pool entry `(code - 1) / 2` (§3.5). | none |

Code 0 is the empty inline string. A reference atom is valid only when the PVS member has a string pool and the index is less than the pool count. A PVS member without a string pool uses inline atoms only.

## 3. PVS structure member

### 3.1 Header

The PVS member is binary when it begins with the six ASCII bytes `PV-PVS`. The binary header is 18 bytes.

| Offset | Width | Field | Meaning |
| -----: | ----: | ----- | ------- |
| 0 | 10 | `signature` | ASCII `PV-PVS` followed by four ASCII decimal digits: `0301`, `0302`, `0303`, `0304`, or `0305`. |
| 10 | 4 | `first_stream_offset` | `u32`. File offset of the first stream payload. |
| 14 | 4 | `stream_count` | `u32`. Number of stream directory entries. |

The four signature digits do not change the container, element, or atom grammar of §3.1 to §3.13. All five signatures use this layout.

### 3.2 Stream directory

The stream directory starts at offset 18 and holds `stream_count` entries without padding.

| Offset | Width | Field | Meaning |
| -----: | ----: | ----- | ------- |
| 0 | 1 | `stream_type` | Stream type (§3.3). |
| 1 | 1 | `codec` | Stream encoding. Value 1 is GZIP. |
| 2 | 1 | `name_length` | Byte length of `name`. |
| 3 | `name_length` | `name` | Stream name bytes. Zero length is valid. |
| 3 + `name_length` | 4 | `stored_length` | `u32`. Stored byte length of the stream payload. |

The directory ends exactly at `first_stream_offset`. Stream payloads follow in directory order with no gap. The first payload starts at `first_stream_offset`, each next payload starts where the preceding payload ends, and the last payload ends at the end of the file. A codec value other than 1 is not defined.

### 3.3 Stream payloads and types

Each stream payload with codec 1 is one GZIP member ([RFC 1952](https://www.rfc-editor.org/rfc/rfc1952)). The expanded bytes form exactly one element (§3.4) whose family byte equals the stream type.

| `stream_type` | Name | Content |
| ------------: | ---- | ------- |
| 1 | String pool | Shared strings for reference atoms (§3.5). At most one per file. |
| 2 | Structure | Component definitions and occurrence hierarchy (§3.6). Exactly one per file. |
| 3 | Property sets | Named property stream (§3.13). Zero or more per file; the stream name distinguishes them. The empty name is the general property stream. |

### 3.4 Element grammar

All expanded PVS streams use one element grammar.

```text
element  = family:u8 descriptor:u8 [length] [type] payload[length] [children]
children = element* 00 00
```

The descriptor byte selects the optional parts.

| Descriptor | Length field | Type field | Children |
| ---------: | ------------ | ---------- | -------- |
| `0x02` | `u8` | none | no |
| `0x04` | `u16` | none | no |
| `0x12` | `u8` | `u8` | no |
| `0x22` | `u8` | `u16` | no |
| `0x80` | none (length 0) | none | yes |
| `0x82` | `u8` | none | yes |
| `0x92` | `u8` | `u8` | yes |

Bit 7 of the descriptor selects a child list. Bits 4 and 5 give the width in bytes of the type field: 0, 1, or 2. Bits 0 to 3 give the width of the length field: 0 selects none, 2 selects one byte, and 4 selects two bytes. The length counts only the payload bytes. It excludes the family, descriptor, length, and type bytes and the child list. A child list is a sequence of elements closed by the two bytes `00 00`. The two bytes `00 00` at an element start position close the enclosing child list; they are not an element. Other descriptor values are not defined.

### 3.5 String pool stream

The string-pool stream is one element with family 1 and descriptor `0x02` or `0x04`. Its payload is:

```text
count:uleb128 { length:uleb128 utf8[length] }*count
```

The entries fill the payload exactly. Entry indexes start at 0 in stored order.

### 3.6 Structure stream

The structure stream is one element with family 2 and descriptor `0x80`. Its children are, in stored order, appearance-palette elements (family `0x2e`, §3.12) and component definitions (family `0x20`). The zero-based position of a definition among the family-`0x20` children of this root is its definition index. Child occurrences (§3.9) and property sets (§3.13) refer to definitions by this index.

A definition element has descriptor `0x02`, `0x82`, or `0x92`. A descriptor-`0x02` definition has no child list, and therefore no geometry and no child occurrences. The child list of a descriptor-`0x82` or `0x92` definition holds its geometry element (§3.8), its child occurrences (§3.9), and the auxiliary elements of §3.12, in any order.

### 3.7 Definition name

The payload of a definition element begins with a string atom that holds the definition name. The name is the source model name, for example `BRACKET.PRT` or `TOP.ASM`, or a free name for a sub-assembly, a simplified group, or an illustration root. The name can be empty.

The type byte of a descriptor-`0x92` definition selects the bytes that follow the name atom.

| Type | Bytes after the name atom | Role |
| ---: | ------------------------- | ---- |
| none (`0x02`, `0x82`) | none | Component definition. |
| `0x01` | 1 byte | Illustration root definition (§3.12). |
| `0x02` | 1 byte | Component definition. |
| `0x06` | 2 bytes | Component definition. |
| `0x1e` | 4 bytes | Component definition. |

The meaning of the bytes after the name atom is an open item. The payload length bounds them.

### 3.8 Geometry element

Family `0x23` attaches one OL member to its definition. A definition has at most one geometry element.

| Descriptor | Payload |
| ---------: | ------- |
| `0x12`, type `0x01` | Member-name atom, then six `f32` bounds: `min_x`, `min_y`, `min_z`, `max_x`, `max_y`, `max_z`. |
| `0x02` | Member-name atom only. |

The member-name atom names a ZIP member of the package (§1.1). The bounds are an axis-aligned box in the definition coordinate system, in the length unit of §8.21. The bounds are the box of the member geometry; the open items record the relation to the tessellation vertex bounds.

### 3.9 Child occurrence element

Family `0x21` places one occurrence of a definition inside its parent definition. Its descriptor is `0x12` or `0x92`, or `0x22` with a two-byte type. The low byte of the type is the placement kind. The high byte of a two-byte type is an open item.

```text
payload = definition_index:uleb128 placement feature_id:atom [extension]
```

`definition_index` is the definition index (§3.6) of the placed definition. It is less than the number of definitions. The low four bits of the placement kind select the placement encoding. The high four bits are flags; their meaning is an open item.

| Kind low nibble | Placement encoding | `f64` count |
| --------------: | ------------------ | ----------: |
| `0x8` | Identity. No values. | 0 |
| `0xa` | Translation `tx, ty, tz`. | 3 |
| `0xc` | Basis `xx, xy, xz, yx, yy, yz, zx, zy, zz`. | 9 |
| `0xe` | Translation, then basis. | 12 |

`feature_id` is a string atom that holds the source feature identifier of the occurrence. A descriptor-`0x12` or `0x92` occurrence ends exactly after `feature_id`. A descriptor-`0x22` occurrence carries `extension` bytes after `feature_id` up to the payload length; their grammar is an open item.

A descriptor-`0x92` occurrence has child elements of family `0x30` (descriptor `0x12` type `0x03`, or descriptor `0x02`). Each holds one byte. Its meaning is an open item.

### 3.10 Placement semantics

A placement maps the child definition coordinate system into the parent definition coordinate system. The nine basis values are three vectors in stored order: the child X axis `(xx, xy, xz)`, the child Y axis `(yx, yy, yz)`, and the child Z axis `(zx, zy, zz)`. Each vector is expressed in parent coordinates. The vectors are the columns of the rotation matrix:

```text
| px |   | xx yx zx |   | cx |   | tx |
| py | = | xy yy zy | · | cy | + | ty |
| pz |   | xz yz zz |   | cz |   | tz |
```

`c` is a point in child coordinates and `p` is the same point in parent coordinates. A reader that loads the nine values as rows of the matrix applies the inverse rotation. An encoding without a basis has the identity basis. An encoding without a translation has the zero translation. The three basis vectors are mutually perpendicular, have one common length `s`, and form a right-handed set. `s` = 1 gives a rigid placement; another `s` gives a rigid placement combined with the uniform scale `s`. Translations use the length unit of §8.21.

### 3.11 Hierarchy and root

The definitions and child occurrences form a directed acyclic graph. Exactly one definition is not the target of a child occurrence; it is the root. Several occurrences can target one definition, which then has several instances. A world transform of an occurrence path is the product of the placements from the root to the leaf.

When the package carries illustrations, the root is a definition of type `0x01` (§3.12), and one of its child occurrences places the top model definition.

### 3.12 Auxiliary structure elements

These elements occur in the structure stream. Their framing follows §3.4. The open items record the unknown fields.

| Family | Descriptor | Parent | Payload |
| -----: | ---------: | ------ | ------- |
| `0x2e` | `0x80` | structure root | Child list of `0x2f` elements. |
| `0x2f` | `0x12` | `0x2e` | Type `0x15`: ten `f32`. Type `0x05`: six `f32`. |
| `0x3b` | `0x12` type `0x01` | type-`0x01` definition | Two atoms: a `.pvi` member name, then an illustration display name. |
| `0x44` | `0x80` | type-`0x01` definition | Child list of `0x45` elements. |
| `0x45` | `0x02` | `0x44` | One string atom. |
| `0x3c` | `0x12` | component definition | Type `0x02`: two bytes. Type `0x06`: a count byte and string atoms. |
| `0x30` | `0x12` type `0x03`, or `0x02` | descriptor-`0x92` occurrence | One byte. |

### 3.13 Property streams

A property stream (type 3) is one element with family 3 and descriptor `0x80`. Its children are property sets of family `0x2a`. Each property set has a child list of properties of family `0x2b`.

A property is a descriptor-`0x02` element whose payload is two atoms: the property name, then the property value. A descriptor-`0x12` property with type `0x05` has three bytes after the two atoms; their meaning is an open item.

Each property set applies to one definition. A cursor starts at definition index −1. A descriptor-`0x80` set advances the cursor by 1. A descriptor-`0x92` set with type `0x01` has a ULEB128 payload `step` and advances the cursor by `step`. The set applies to the definition at the cursor after the advance. The cursor stays less than the number of definitions. A definition without a set in a stream has no properties in that stream.

### 3.14 XML encoding

A PVS file that begins with `<PV_FILE` is an XML document with root element `PV_FILE`, attribute `type="PVS"`, and a `version` attribute that holds the same four digits as the binary signature. It carries the same model in element form:

| XML element | Binary equivalent |
| ----------- | ----------------- |
| `section_index/internal_section@type` | Stream directory entry types. |
| `section_structure/component@name` | Definition (§3.6, §3.7), in definition-index order. |
| `component/shape_source@file_name`, `@bbox` | Geometry element (§3.8); `bbox` holds six comma-separated reals in §3.8 order. |
| `component/component_instance@index`, `@translation`, `@orientation`, `@id` | Child occurrence (§3.9); `orientation` holds the nine basis values in §3.10 order. |
| `component/illustration3d_source@file_name`, `@name` | Element `0x3b` (§3.12). |
| `section_properties/property_component_ref@index_offset` | Property set (§3.13); `index_offset` is `step`. |
| `property@name`, `@value` | Property (§3.13). |

The complete XML grammar is an open item.

## 4. OL geometry member

### 4.1 Header

| Offset | Width | Field | Meaning |
| -----: | ----: | ----- | ------- |
| 0 | 10 | `signature` | ASCII `DIV-OL08BL` or `DIV-OL09BL`. |
| 10 | 4 | `version` | `u32`. Value 1. |
| 14 | 4 | `directory_length` | `u32`. Equals `4 + 8 × entry_count`. |
| 18 | 4 | `entry_count` | `u32`. Number of directory entries, including the end sentinel. |

The two signatures share the layout and the stream grammar of §4 to §8.

### 4.2 Stream directory

The directory starts at offset 22 and holds `entry_count` entries of 8 bytes.

| Offset | Width | Field | Meaning |
| -----: | ----: | ----- | ------- |
| 0 | 4 | `stream_id` | `u32`. Stream identity (§4.5). |
| 4 | 4 | `offset` | `u32`. File offset of the stream payload. |

Entries are in ascending `stream_id` order. The last entry has `stream_id` `0x00ffffff` and an `offset` equal to the file length. It is the end sentinel and has no payload. The payload of entry `i` occupies `[offset[i], offset[i+1])`. Each payload is one GZIP member.

### 4.3 Preamble block

The first stream payload starts at offset `22 + 8 × entry_count`, or 19 bytes later. A 19-byte gap holds the preamble block:

| Offset | Width | Field | Value |
| -----: | ----: | ----- | ----- |
| 0 | 4 | `tag` | `u32` `0x010000d4`. |
| 4 | 4 | `word` | `u32` 2 or 3. |
| 8 | 1 | `byte_0` | `0x03`. |
| 9 | 1 | `generation` | `0x08` or `0x09`. |
| 10 | 1 | `byte_2` | `0x01` or `0x02`. |
| 11 | 8 | `reserved` | Zero bytes. |

The meaning of `word`, `byte_0`, `generation`, and `byte_2` is an open item. A reader locates the first stream through the directory; it does not need the preamble.

### 4.4 TLV framing

Each expanded stream payload is a sequence of TLV records that fills it exactly:

| Offset | Width | Field |
| -----: | ----: | ----- |
| 0 | 4 | `tag` (`u32`) |
| 4 | 4 | `length` (`u32`) |
| 8 | `length` | `payload` |

Tag values are local to a stream identity: one tag value has different meanings in different streams. A record with tag 0 and length 0 is a scope terminator.

### 4.5 Stream identities

| `stream_id` | Stream | Defined in |
| ----------: | ------ | ---------- |
| `0x01000000` | Attributes and appearances | §5 |
| `0x01000001` | Surface tessellation | §6 |
| `0x01000002` | Edge tessellation | §7 |
| `0x01000003` | Exact geometry, `f32` scalars | §8 |
| `0x01000004` | Exact geometry, `f64` scalars | §8 |
| `0x00ffffff` | End sentinel | §4.2 |

The attribute stream is always present. The other streams are optional. A member carries at most one exact-geometry stream.

The first record of every stream has tag `0x01000002` and a 4-byte payload. Its `u32` value is the stream index: the low byte of `stream_id` (0 to 4). In the exact-geometry stream this value is the exact version, 3 or 4 (§8.1).

## 5. Attribute stream

### 5.1 Appearance values

Tag 5 with a 56-byte payload holds 14 `f32` values:

| Index | Field |
| ----: | ----- |
| 0–2 | Base colour RGB. |
| 3–5 | Diffuse RGB. |
| 6–8 | Specular RGB. |
| 9 | Shininess exponent. |
| 10–12 | Emissive RGB. |
| 13 | Opacity, 1 for opaque. |

Opacity is in the range 0 to 1.

### 5.2 Appearance reference

Tag 7 holds an appearance reference:

| Offset | Width | Field |
| -----: | ----: | ----- |
| 0 | 4 | `name_length` (`u32`) |
| 4 | `name_length` | `name`: NUL-terminated bytes, padded with NUL to `name_length`. |
| 4 + `name_length` | 4 | `base_index` (`u32`) |
| 8 + `name_length` | 17 | `tail` |

The `tail` fields are an open item.

### 5.3 Appearance table

The tag-7 records, in stored order, form the appearance table. A table index is the zero-based position of a tag-7 record among the tag-7 records of the stream.

A tag-5 record immediately followed by a tag-7 record defines the appearance for that record's `base_index`. Each `base_index` has at most one definition. A tag-7 record without a preceding tag-5 record refers to the appearance that an earlier pair defined for its `base_index`.

A tessellation patch header (§6.2) stores a table index. The appearance of table entry `i` is the appearance defined for `base_index[i]`. The value `0xffffffff` means no appearance.

### 5.4 Other attribute records

The attribute stream also carries records with tags 3, 4, `0x2b`, `0xc8`, `0xcd`, `0x01000008`, `0x0100002c`, `0x19`, `0x1a`, `0x1b`, and others. Tag 3 holds a named list of identifiers, for example the source feature identifiers of the member. Tag `0x0100002c` maps a feature identifier to a feature name. These records are open items.

## 6. Surface tessellation stream

### 6.1 Record sequence

After the stream index record, the stream holds one group per tessellated patch. A group begins with a patch header (tag `0x0100000d`). It contains a vertex block (tag `0x01000016`) and the triangle primitives that follow that vertex block and index into it. A group can also contain the records of §6.5.

### 6.2 Patch header

Tag `0x0100000d`:

| Offset | Width | Field | Meaning |
| -----: | ----: | ----- | ------- |
| 0 | 4 | `appearance` | `u32`. Appearance table index (§5.3), or `0xffffffff`. |
| 4 | 4 | `face_ref` | `u32`. Exact face reference, or `0xffffffff`. |
| 8 | 1 | `flag` | 0 or 1. |
| 9 | 4 | `ref_count` | `u32`. |
| 13 | 4 × `ref_count` | `refs` | `u32` values. |

The payload length is `13 + 4 × ref_count`. The meaning of `face_ref`, `flag`, and `refs` is an open item.

### 6.3 Vertex block

Tag `0x01000016` holds a vertex array in the definition coordinate system:

| Offset | Width | Field |
| -----: | ----: | ----- |
| 0 | 1 | `layout` |
| 1 | 4 | `vertex_count` (`u32`) |
| 5 | stride × `vertex_count` | vertices |

Bit 0 of `layout` adds a normal; bit 2 adds a texture coordinate. Each vertex holds, in order, the position `x, y, z` (three `f32`), the unit normal `nx, ny, nz` (three `f32`) when bit 0 is set, and the texture coordinate `s, t` (two `f32`) when bit 2 is set.

| `layout` | Stride | Vertex fields |
| -------: | -----: | ------------- |
| 0 | 12 | position |
| 1 | 24 | position, normal |
| 5 | 32 | position, normal, texture coordinate |

A vertex block with `vertex_count` 0 has no vertices; primitives after it do not reference it.

### 6.4 Triangle primitives

A primitive record holds `u32` vertex indexes; its length is a multiple of 4. Each index is less than the `vertex_count` of the most recent vertex block.

| Tag | Primitive | Triangle `k` from indexes `i` |
| --: | --------- | ----------------------------- |
| `0x19` | Fan | `(i[0], i[k+1], i[k+2])` for `k = 0 … n−3` |
| `0x1a` | Strip | `(i[k], i[k+1], i[k+2])` for even `k`; `(i[k+1], i[k], i[k+2])` for odd `k`; `k = 0 … n−3` |
| `0x1b` | List | `(i[3k], i[3k+1], i[3k+2])`; `n` is a multiple of 3 |

### 6.5 Other tessellation records

| Tag | Length | Content |
| --: | -----: | ------- |
| `0x0100000b` | 0 | Stream scope marker. |
| `0x0100000c` | 4 or 8 | `u32` values. |
| `0x0a` | 12, 16, 20, or 24 | `u32` values. |
| `0x01000017` | 10 or 22 | Contains an `f32` at offset 4. |
| `0x01000018` | 11 | `u32`, `u32`, three bytes. |
| `0x1c`, `0x1d` | variable | Line primitives (§7.2). |
| `0x26`, `0x01000021`, `0x01000027` | variable | Unknown. |
| 0 | 0 | Scope terminator. |

These records are open items. A reader that needs only shaded geometry uses §6.2 to §6.4.

## 7. Edge tessellation stream

### 7.1 Edge index table

After the stream index record, tag `0x0100000f` holds a `u32` count `n` followed by `n` `u32` values. The meaning of the values is an open item.

### 7.2 Edge groups

Each edge group begins with an edge header (tag `0x01000012`, 12 bytes):

| Offset | Width | Field | Meaning |
| -----: | ----: | ----- | ------- |
| 0 | 4 | `edge_index` | `u32`. Index of the exact edge record (§8.15). |
| 4 | 4 | `word` | `u32`. |
| 8 | 4 | `value` | `f32`. |

The group continues with a vertex block (§6.3) with `layout` 0 and one line primitive that indexes into it. The group can also contain the records of §6.5. The vertex block holds sample points of exact edge `edge_index`, in the definition coordinate system; it contains the curve start point. Vertex order in the block need not be curve order; the line primitive gives the order.

| Tag | Primitive | Content |
| --: | --------- | ------- |
| `0x1c` | Line strip | `u32` indexes. Consecutive indexes form segments. |
| `0x1d` | Line segments | `u32` index pairs. Each pair forms one segment. |

The meaning of `word`, `value`, and the records of §6.5 in an edge group are open items.

## 8. Exact geometry stream

### 8.1 Selection, version, and precision

The exact-geometry stream has `stream_id` `0x01000003` or `0x01000004`. Its stream index record (§4.5) stores the exact version:

| `stream_id` | Exact version | Scalar type |
| ----------: | ------------: | ----------- |
| `0x01000003` | 3 | `f32` |
| `0x01000004` | 4 | `f64` |

In this section, `scalar` is the scalar type of the stream, and `w` is its width in bytes (4 or 8). Integers keep their width in both versions.

### 8.2 Record order

The stream is, in order:

```text
version-record
face-section     = { surface-record { loop | singular-use }* }*
loop             = loop-record coedge-record* terminator
edge-section     = edge-record*
pcurve-section   = pcurve-record*
```

| Tag | Record | Defined in |
| --: | ------ | ---------- |
| `0x13` | Supporting surface; starts a face | §8.3–§8.10 |
| `0x01000010` | Loop | §8.11 |
| `0x11` | Coedge | §8.12 |
| `0x01000012` | Singular edge use | §8.13 |
| 0 | Loop terminator | §8.14 |
| `0x14` | Edge curve | §8.15 |
| `0x15` | Pcurve | §8.16 |
| `0x0a` | 12-byte record in the face section | open item |

A face is one surface record and the loop, coedge, singular-use, and terminator records that follow it up to the next surface record or the first edge record. A face index, an edge index, and a pcurve index are zero-based ordinals of their records within their section. All coedge, singular-use, and edge-tessellation references to these indexes are less than the record count of the target section.

### 8.3 Surface record head

Every surface record begins with three bytes:

| Offset | Width | Field |
| -----: | ----: | ----- |
| 0 | 1 | `surface_type` |
| 1 | 1 | `flags` |
| 2 | 1 | `marker` |

| `surface_type` | Surface | Defined in |
| -------------: | ------- | ---------- |
| 1 | Cone | §8.6 |
| 2 | Cylinder | §8.5 |
| 4 | Rational B-spline | §8.9 |
| 5 | Plane | §8.4 |
| 8 | Sphere | §8.7 |
| 9 | Torus | §8.8 |

Bit 7 of `flags` (`0x80`) adds a u-range pair `u_min, u_max`. Bit 0 of `marker` adds a v-range pair `v_min, v_max`. The pairs are scalars; the u-range pair comes first. A pair that is absent leaves that direction unbounded by the record. The other `flags` bits are open items.

An analytic surface record (types 1, 2, 5, 8, 9) is:

```text
head(3) geometry:scalar[G] [u_min u_max] [v_min v_max] tail
tail = 00 | embedded-bspline
```

`G` is the geometry scalar count of the type. The tail is either the single byte `0x00` or an embedded B-spline representation (§8.10) that ends at the end of the payload.

Each analytic type has a native parameterisation `P(u, v)` given below. Angles are in radians. Vectors are not normalised: their lengths carry the radii and scales.

### 8.4 Plane

`G` = 12: `O[3]`, `U[3]`, `V[3]`, `N[3]`.

```text
P(u, v) = O + u·U + v·V
```

`N` is the plane normal. The native u and v are coefficients of `U` and `V`; they are lengths only when `U` and `V` are unit vectors.

### 8.5 Cylinder

`G` = 12: `O[3]`, `X[3]`, `Y[3]`, `A[3]`.

```text
P(u, v) = O + cos(u)·X + sin(u)·Y + v·A
```

`|X|` is the radius. `X` and `Y` are perpendicular and of equal length. u is the angle from `X` toward `Y`. v is a coefficient of `A`; it is a length only when `A` is a unit vector.

### 8.6 Cone

`G` = 14: `O[3]`, `X[3]`, `Y[3]`, `A[3]`, `a`, `b`.

```text
P(u, v) = O + v·A + (a + b·v)·(cos(u)·X + sin(u)·Y)
```

The radius at parameter v is `(a + b·v)·|X|`. The apex is at `v = −a / b`. u is the angle from `X` toward `Y`.

### 8.7 Sphere

`G` = 12: `C[3]`, `X[3]`, `Y[3]`, `Z[3]`.

```text
P(u, v) = C + cos(v)·(cos(u)·X + sin(u)·Y) + sin(v)·Z
```

`|X|` is the radius. u is the longitude from `X` toward `Y`. v is the latitude, with `v = π/2` at `C + Z`.

### 8.8 Torus

`G` = 21: `O[3]`, `M_c[3]`, `M_s[3]`, `axis[3]`, `m_c[3]`, `m_a[3]`, `m_s[3]`.

```text
r       = |m_c|
major   = cos(u)·M_c + sin(u)·M_s
minor   = cos(u)·m_c − r·sin(u)·m_s
P(u, v) = O + major + cos(v)·minor + sin(v)·m_a
```

`|M_c|` is the major radius and `r` is the minor radius. `axis` is the unit axis of revolution. `m_c` is the minor-circle radial vector at `u = 0`, `m_a` is the minor-circle axial vector, and `m_s` is a unit direction that rotates `m_c` with u. u is the major angle; v is the minor angle. The major radius can be zero or near zero; the formula then describes a sphere-like surface.

### 8.9 B-spline surface

| Offset | Width | Field |
| -----: | ----: | ----- |
| 0 | 1 | `surface_type` = 4 |
| 1 | 1 | `flags` |
| 2 | 1 | `marker` |
| 3 | 1 | `rational`: 0 or 1 |

The fixed head continues with the body:

```text
u_order:u32 u_knot_count:u32 u_knots:scalar[u_knot_count]
v_order:u32 v_knot_count:u32 v_knots:scalar[v_knot_count]
u_pole_count:u32 v_pole_count:u32
poles:scalar[u_pole_count × v_pole_count × 4]
[u_min u_max] [v_min v_max]
00
```

`u_pole_count = u_knot_count − u_order` and `v_pole_count = v_knot_count − v_order`. The order is the degree plus one. The knot vectors are full: each knot repeats once per multiplicity. Pole `(i, j)` is at position `i × v_pole_count + j` (u-major). Each pole is homogeneous `(w·x, w·y, w·z, w)`: the Euclidean pole is the first three values divided by the fourth, and the fourth is the weight. When `rational` is 0, every weight is 1.

The range pairs follow the rule of §8.3 and give the trimmed parameter domain of the face. The record ends with one `0x00` byte.

```text
P(u, v) = Σ_i Σ_j N_i(u) · N_j(v) · w_ij · p_ij  /  Σ_i Σ_j N_i(u) · N_j(v) · w_ij
```

### 8.10 Embedded B-spline representation

An analytic surface record can end with an embedded B-spline. The embedded B-spline is the §8.9 head with `surface_type` 4, `flags` 0, and `marker` 0, followed by the §8.9 body without range pairs and without the final `0x00` byte. A plane carries `rational` 0; the other analytic types carry `rational` 1.

The embedded B-spline describes the same surface as the analytic record in its own parameter domain. Secondary pcurves (§8.12) use this domain.

### 8.11 Loop record

Tag `0x01000010` has a 4-byte payload `marker` (`i32`). It starts a loop on the current face. The global loop index is the zero-based position of the loop among all loop records of the stream.

| `marker` | Meaning |
| -------: | ------- |
| −2 | The loop starts a new loop group on the current face. |
| ≥ 0 | The loop joins the loop group whose first loop has global index `marker`. That loop has marker −2, precedes this loop, and belongs to the same face. |

Other negative values are open items. A loop group need not be contiguous: loops of other groups of the same face can come between its members.

### 8.12 Coedge record

Tag `0x11` has a 13-byte payload:

| Offset | Width | Field | Meaning |
| -----: | ----: | ----- | ------- |
| 0 | 4 | `edge` | `u32`. Edge index (§8.15), or `0xffffffff`. |
| 4 | 1 | `orientation` | 0 or 1. |
| 5 | 4 | `pcurve` | `u32`. Native pcurve index, or `0xffffffff`. |
| 9 | 4 | `pcurve_secondary` | `u32`. Secondary pcurve index, or `0xffffffff`. |

The native pcurve lies in the native parameter domain of the face surface (§8.4–§8.9). The secondary pcurve lies in the domain of the embedded B-spline (§8.10) of an analytic face. A coedge on a B-spline face has a native pcurve and no secondary pcurve. A coedge on an analytic face has a native pcurve, a secondary pcurve, or both. Exact version 4 stores both on analytic faces.

A coedge with `edge` `0xffffffff` has no shared edge record. Its 3D geometry is the image of its pcurve on the face surface.

The `orientation` byte does not give the curve sense; §8.17 derives the sense. Its meaning is an open item.

### 8.13 Singular edge use

Tag `0x01000012` belongs to the current face:

| Offset | Width | Field |
| -----: | ----: | ----- |
| 0 | 4 | `edge` (`u32`) |
| 4 | `w` | `t_start` (`scalar`) |
| 4 + `w` | `w` | `t_end` (`scalar`) |

The record marks a topologically singular use of edge `edge` on the face: the use has zero length in model space, and both ends of the edge are one vertex. The referenced curve can be a point, a line, a circle, a B-spline, or a polyline; a non-point curve collapses within model tolerance.

### 8.14 Loop terminator

A tag-0 record with length 0 closes the current loop. A coedge record belongs to the loop opened by the most recent loop record that no terminator has closed.

### 8.15 Edge curve records

Tag `0x14` holds one 3D curve. Every edge curve record begins with `curve_type` (`u8`) and `flags` (`u8`), and every record except type 2 ends with one `0x00` byte. Bit 3 of `flags` (`0x08`) adds a parameter-range pair `t_start, t_end` to a conic. The other `flags` bits are open items.

| `curve_type` | Curve | Layout after the two head bytes |
| -----------: | ----- | ------------------------------- |
| 1 | Circle | `C[3]`, `X[3]`, `Y[3]` (scalars), [`t_start`, `t_end`], `00` |
| 2 | Composite | `count:u32`, `count × u32` edge indexes, `00` |
| 3 | Point | `P[3]`, `00` |
| 4 | Ellipse | `C[3]`, `X[3]`, `Y[3]`, [`t_start`, `t_end`], `00` |
| 5 | Line | `O[3]`, `D[3]`, `00` |
| 6 | Rational B-spline | `rational:u8`, `order:u32`, `knot_count:u32`, `knots:scalar[knot_count]`, `pole_count:u32`, `poles:scalar[pole_count × 4]`, `00` |
| 7 | Polyline | `count:u32`, `points:scalar[count × 3]`, `00` |

Parameterisations and natural parameter intervals:

| Type | `C(t)` | Interval |
| ---: | ------ | -------- |
| 1, 4 | `C + cos(t)·X + sin(t)·Y` | `[t_start, t_end]`; `[0, 2π]` without the range pair |
| 3 | `P` | one point |
| 5 | `O + t·D` | `[0, 1]` |
| 6 | Rational B-spline, homogeneous poles as in §8.9 | `[knots[order − 1], knots[knot_count − order]]` |
| 7 | Linear interpolation of `points` | from `points[0]` to `points[count − 1]` |

A circle has `|X| = |Y|` with `X ⊥ Y`. An ellipse has `X ⊥ Y` and unequal lengths. For type 6, `pole_count = knot_count − order`, and `rational` 0 means every weight is 1. The interval start is the curve start point; the interval end is the curve end point.

A type-2 record lists other edge records of the same section by index; each listed index is less than the edge count. The parameterisation of a composite curve is an open item.

### 8.16 Pcurve records

Tag `0x15` holds one 2D curve in a surface parameter domain. Every pcurve record begins with `curve_type` (`u8`) and `flags` (`u8`) and ends with one `0x00` byte. Bit 3 of `flags` adds a parameter-range pair. The other `flags` bits are open items.

| `curve_type` | Curve | Layout after the two head bytes |
| -----------: | ----- | ------------------------------- |
| 2 | Composite | `count:u32`, `count × u32` pcurve indexes, `00` |
| 4 | Ellipse | `C[2]`, `X[2]`, `Y[2]`, [`t_start`, `t_end`], `00` |
| 5 | Line | `O[2]`, `D[2]`, `00` |
| 6 | Rational B-spline | `rational:u8`, `order:u32`, `knot_count:u32`, `knots:scalar[knot_count]`, `pole_count:u32`, `poles:scalar[pole_count × 3]`, [`t_start`, `t_end`], `00` |
| 7 | Polyline | `count:u32`, `points:scalar[count × 2]`, `00` |

The parameterisations are the 2D forms of §8.15. A type-2 pcurve lists other pcurve records by index. A B-spline pcurve stores homogeneous poles `(w·u, w·v, w)`. A B-spline pcurve with flag `0x08` uses the range pair as its interval; otherwise its interval is the knot domain. The pcurve start point and end point are the images of the interval ends.

### 8.17 Coedge geometry and sense

The model stores each coedge twice: once as the edge curve, once as the pcurve image on the face surface. Let `S` be the face surface (native pcurve) or its embedded B-spline (secondary pcurve), `p0` and `p1` the pcurve start and end points, and `e0` and `e1` the edge curve start and end points. Then either `S(p0) = e0` and `S(p1) = e1`, or `S(p0) = e1` and `S(p1) = e0`, within the tolerance of §8.21.

The coedge sense is forward when the first condition holds and reversed when the second holds. A reader derives the sense from these endpoints. It does not take the sense from the `orientation` byte. For a closed edge curve both conditions hold; the sense then follows from the neighbouring coedges of the loop.

### 8.18 Loop traversal

The coedges of a loop form a closed chain: the end of each coedge, after its sense, meets the start of another coedge. The chain order is either the stored coedge order or its reverse. A reader selects the order whose consecutive pcurve images meet within tolerance.

### 8.19 Face regions and nesting

A loop group (§8.11) bounds one or more face regions on its face. Within a group, a loop is nested in another loop when it lies inside that loop in the parameter domain of the face. The nesting depth of a loop is the number of loops of the group that contain it. A loop of even depth is the outer boundary of a face region. A loop of odd depth is a hole of the region whose outer loop immediately contains it. A face with several loop groups, or with even-depth loops beyond depth 0, has several face regions.

On a periodic surface (cylinder, cone, sphere in u; torus in u and v) containment is tested modulo the period of 2π.

### 8.20 Vertices and shells

The stream has no vertex records and no shell records. A vertex is the common point of the coedge ends that meet in a loop chain (§8.18), and of both ends of a singular edge (§8.13). An edge is shared when coedges of two faces reference it.

A shell is a set of face regions connected through shared edges. A shell is closed when each non-singular edge in it has exactly two coedge uses; otherwise it is open.

### 8.21 Units and tolerances

Lengths in OL members and in PVS placements and bounds are in metres. Angles are in radians. The member stores no tolerance value. The endpoint agreement of §8.17 holds within 2×10⁻⁵ m. Exact version 3 stores `f32` scalars, so its geometry carries `f32` rounding.
