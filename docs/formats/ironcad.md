# IronCAD scene files

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

## 1. Container

An IronCAD `.ics` scene file uses Microsoft Compound File Binary. All scene
streams are entries of the root storage. Entry names compare without regard to
ASCII case.

### 1.1 Document identity

The root storage CLSID is `0002c52b-0000-0000-c000-000000000046`. The CLSID is
written in the canonical GUID form; the directory stores its first three fields
little-endian. The root storage contains the streams `Brep Data`,
`Shape Data`, and `Document Data`. A compound file with this CLSID and these
three streams is an IronCAD scene.

### 1.2 Stream map

| Stream | Encoding | Content |
|---|---|---|
| `Brep Data` | frame (§2) | Kernel record tree (§3). |
| `Shape Data` | frame | Scene object graph. |
| `Shape Contents` | frame | Scene objects (§6). |
| `Shared Objects` | frame | Style, layer, and line-type names as UTF-16LE strings. |
| `Public Access` | frame | Part property names and values. |
| `Public Access Data` | frame | |
| `Image Data` | frame | |
| `Document Data` | raw | Scene path and unit names (§4). |
| `Preview` | raw | Windows bitmap file, starting with `BM`. |
| `HSF` | raw | HOOPS Stream File. |
| `View`, `Animation`, `MetaInfo`, `PMIDraftDb` | raw | |
| `HSF_LIB`, `Public Access Cache` | raw | Empty. |
| `SummaryInformation`, `DocumentSummaryInformation` | raw | OLE property sets. |

## 2. Compressed stream frame

A framed stream holds a 20-byte little-endian header and one or more zlib
blocks.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 4 | `magic` | `0x3de3de00`. |
| 4 | 4 | `block_size` | `0x40000`. The inflated size of each block. |
| 8 | 8 | `payload_length` | Number of payload bytes at the start of the inflated blocks. |
| 16 | 4 | `compressed_length` | Number of zlib bytes of the first block. |

The frame holds `⌈payload_length / block_size⌉` blocks, and at least one. The
first block is the `compressed_length` zlib bytes at offset 20. Each later
block is a little-endian `u32` compressed length followed by that many zlib
bytes. The blocks fill the stream exactly, and each block inflates to exactly
`block_size` bytes. The inflated blocks, in stored order, form one buffer. Its
first `payload_length` bytes are the payload, and the remaining bytes are
zero.

## 3. Kernel record tree

The `Brep Data` payload is one node.

A **node** starts with a little-endian `u64` length. The length counts the
length field itself, so an empty node has length 8. The node content is a
sequence of nodes and kernel records that fills the node exactly.

A **kernel record** starts with a little-endian `u32` length. The length counts
the record header and the kernel bytes, and excludes the length field.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 4 | `length` | Header and kernel byte count. |
| 4 | 4 | `marker` | Low byte `0x01`; the upper 24 bits vary between records. |
| 8 | 4 | `kernel` | `0x80000001` for ACIS, `0x80000002` for Parasolid. |
| 12 | 12 | `words` | Three `u32` words. Each is `1`. |
| 24 | 4 | `reserved` | `0`, `1`, or `2`. |

The kernel bytes start at offset 28 and fill the remainder of the record.

The second `u32` of an item selects its kind. A nonzero value starts a kernel
record. A node length is less than 2³², so the second `u32` of a node is zero.

The payload node is the scene root. A **pair** is a child node that holds a
kernel record at any depth, directly followed by a kernel record. The child
node of a pair belongs to the feature tree of its part: a pair that it holds at
any depth is a feature shape of that part, and a node that it holds is not an
assembly.

A node below the scene root that holds pairs at any depth is an **assembly**:
each pair that it holds is a part of the assembly, and each assembly node that
it holds is a member assembly. The pairs that the scene root holds are the
parts of the scene.

### 3.1 Parasolid kernel bytes

The kernel bytes are a native binary Parasolid transmit
([`parasolid.md`](parasolid.md) §5). The description names the modeller
version, for example `: TRANSMIT FILE created by modeller version 3401176`. The
schema token has the form `SCH_<modeller>_<schema>_<base>`, for example
`SCH_3401176_34101_13006`. Lengths are metres.

### 3.2 ACIS kernel bytes

The kernel bytes are one interned ACIS binary save stream
([`asm.md`](asm.md) §2.4). It starts with the magic `ACIS BinaryFile` and ends
with the `End-of-ACIS-data` identifier. The save-format word is `3200`, and the
header product string names `IronCAD`. Lengths are centimetres (`asm.md` §4).

## 4. Units

The `Document Data` stream stores the scene path, then four unit entries in
this order: a length entry, an angle entry, a second length entry, and a mass
entry. An entry is an f64 directly followed by three adjacent strings: the unit
name, the unit abbreviation, and a precision string. The strings use the
record-name form of §5. The first length entry, the angle entry, and the mass
entry state the scene units, and their f64 is the scale of the unit to the SI
unit. A scene in millimetres stores `0.001`, `Millimeters`, and `mm`, then
`π/180`, `Degrees`, and `deg`, then the second length entry, then `1.0` and
`Kilograms`. A scene in inches stores `0.0254` and `Inches` in the first length
entry. Parasolid kernel lengths are in metres.

## 5. Scene records

The `Public Access` stream is framed (§2). Its payload holds a sequence of
named records. A record name is the bytes `ff ff fe`, a `u8` count of UTF-16
code units, and that many little-endian UTF-16 code units. A marker with count
`0` opens a record with an empty name when the header of a version `1` or `2`
part copy record follows it, and no record otherwise.

The bytes `ff ff ff ff e1 ff` and a `u32` scene object identifier (§6) come
before the records of a scene object.

A **feature record** continues after the name with `00 00 00 00 01 00`.

A **scene object record** continues after the name with a 20-byte header.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 4 | `version` | Record version. |
| 4 | 4 | `class` | First field of the class GUID. |
| 8 | 12 | `class_tail` | `2d 92 cf 11 ac 2e 00 a0 24 58 1e 5c`. |

The class GUID is `{<class>-922D-11CF-AC2E-00A024581E5C}`. The header of a
class `0x3de1004a` record opens a property set; the version is `2` or `3`. A
class `0x3de1004a` record that follows a feature record, with no class
`0x3de1004a` record between them, is a **part record**. The feature records of
a part come before its part record. A class `0x3de1004a` record that follows
no such feature record is an **assembly record**. An assembly record comes
after the part records of its parts.

A class `0x3de1004b` record is a **part copy record**. A version `2` part copy
record holds the `ModelGUID` property, then a property set of class
`0x3de1004a`, then the matrix. A version `1` part copy record holds the
`ModelGUID` property and the matrix, and places one more occurrence of a part
or an assembly. Scene object records of other classes come between feature
records.

The kind of the scene object of a record (§6) selects whether the record
places a part or an assembly. A record places the body of the part result
record (§3) that ends at the `end` of its scene object. A part copy record
whose scene object ends at an empty node holds a **copy path** in its bytes
before its name: twelve zero bytes, the `u32` `1`, and the identifiers of the
scene object whose body the copy places and of its ancestors, from the root
down, each greater than `1`, then a `u32` `0` or `1`. The copy lies in the
assembly that holds that scene object.

The property set holds named properties. A property is the bytes `f4 ff`, a
`u32` name length in bytes, the NUL-terminated little-endian UTF-16 name, a
`u16` value type, and the value. Type `8` is a string: a `u32` byte length and
the NUL-terminated UTF-16 text. Type `5` is an f64; the value `-1e300` marks an
unset property. Type `3` is an i32. Type `0` holds no value. The properties include `PartName`, `Part Number`,
`Description`, `Classification`, `PartVolume`, `PartMass`, `MassDensity`,
`SizeboxHeight`, `SizeboxLength`, `SizeboxWidth`, `StartTaper`, `EndTaper`,
and `ModelGUID`. Values are SI: lengths are metres, masses are kilograms, and
`MassDensity` is kilograms per cubic metre.

The `ModelGUID` value is a brace-enclosed GUID string. The record matrix
follows the end of that string and a flag byte `1`. In a class `0x3de1004a`
record of version `2`, the flag is at a fixed offset.

In the **short form**, the flag directly follows 32 bytes.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 32 | `gap` | Bytes after the `ModelGUID` text. |
| 32 | 1 | `flag` | `1`. |
| 33 | 128 | `matrix` | Sixteen little-endian f64 values in row order. |

In the **long form**, two `u32` words come between those 32 bytes and the flag.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 32 | `gap` | Bytes after the `ModelGUID` text. |
| 32 | 8 | `words` | `0`, `1`. |
| 40 | 1 | `flag` | `1`. |
| 41 | 128 | `matrix` | Sixteen little-endian f64 values in row order. |

In a part record of version `3`, a property set of class `0x3de50020` comes
between the `ModelGUID` text and the flag. In a version `2` part copy record,
the part property set comes there.

Rows 1 to 3 of `matrix` hold a 3 × 3 block and the value `0`. Row 4 holds a
translation in metres and the value `1`. The matrix of a part record places
the part frame in the frame of its parent: a point `p` of the part frame lies
at `p · R + t`, where `R` is the 3 × 3 block and `t` the translation. The
parent of a part that an assembly holds is that assembly. The matrix of an
assembly record places the assembly in the frame of its parent in the same
form. The parent of an assembly that the scene root holds is the scene, and the
parent of a member assembly is the assembly that holds it. A part at
the scene origin stores the identity matrix. A part turned by 30° about the
scene Z axis stores the rows `(cos 30°, sin 30°, 0)` and `(-sin 30°, cos 30°, 0)`
in the first two rows of `R`.

`Shape Data` holds one matrix of the same form for each feature. It maps a
point of the feature body to the part frame as `p · R + t`.

## 6. Scene objects

The `Shape Contents` stream is framed (§2). Its payload holds the scene
objects in scene browser order: features, parts, assemblies, patterns, lights,
and views. The features of a part come before the part, and the members of an
assembly come before the assembly. A **scene object entry** is a 20-byte
header, the object name in the record name form of §5, a `u32` `0`, and a
sequence of 56-byte items that ends with a terminating item.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 4 | `version` | `0x250`, `0x253`, or `0x26d`. |
| 4 | 8 | `end` | Offset in the `Brep Data` payload where the kernel data of the object ends. |
| 12 | 4 | `reserved` | `0`. |
| 16 | 4 | `id` | Object identifier. |

An item has this form:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 4 | `tag` | `4`. |
| 4 | 8 | `handle` | All bytes `ff` in the terminating item. |
| 12 | 2 | `kind` | Item kind. |
| 14 | 2 | `count` | `1`. |
| 16 | 8 | `reserved` | `0`. |
| 24 | 8 | `data_offset` | Offset of the item data in the `Shape Data` payload. |
| 32 | 4 | `size_tag` | `16`. |
| 36 | 4 | `data_size` | Byte count of the item data. |
| 40 | 4 | `object_version` | The header `version`. |
| 44 | 8 | `object_end` | The header `end`. |
| 52 | 4 | `trailer` | `0`. |

An object that holds an item of kind `20` has a body: it is a part or a
feature. An object that holds an item of kind `25` and no item of kind `20` is
an assembly. The identifiers of the children of one parent differ from each
other.

The `end` of a part is the end of its part result record. A copy that places
the body of a part stored before it has the `end` of that part. A copy stored
before the part whose body it places, a pattern, and another object without
kernel data each add an empty node, and their `end` is the end of that node.
The `end` of an assembly is the end of its assembly node, which is also the
end of its last member. The `end` of a feature lies inside the child node of
the pair of its part, or is the end of that child node.
