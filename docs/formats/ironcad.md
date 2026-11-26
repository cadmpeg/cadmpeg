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
| `Shape Contents` | frame | Scene browser names as UTF-16LE strings. |
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

A framed stream holds a 20-byte little-endian header and one zlib stream.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 4 | `magic` | `0x3de3de00`. |
| 4 | 4 | `block_size` | `0x40000`. The inflated size of the zlib stream. |
| 8 | 8 | `payload_length` | Number of payload bytes at the start of the inflated block. |
| 16 | 4 | `compressed_length` | Number of zlib bytes. Equal to the stream length minus 20. |

The zlib stream starts at offset 20 and ends at the stream end. It inflates to
exactly `block_size` bytes. The first `payload_length` bytes are the payload.
The remaining bytes of the block are zero. `payload_length` is at most
`block_size`.

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
| 4 | 4 | `marker` | `0x0009b401`. |
| 8 | 4 | `kernel` | `0x80000001` for ACIS, `0x80000002` for Parasolid. |
| 12 | 12 | `words` | Three `u32` words. Each is `1`. |
| 24 | 4 | `reserved` | `0`. |

The kernel bytes start at offset 28 and fill the remainder of the record.

The second `u32` of an item selects its kind. The value `0x0009b401` starts a
kernel record. A node length is less than 2³², so the second `u32` of a node is
zero.

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
`0` opens no record.

A **part record** and an **assembly record** continue after the name with a
property set. The property set starts with these 20 bytes:

```
02 00 00 00 4a 00 e1 3d 2d 92 cf 11 ac 2e 00 a0 24 58 1e 5c
```

A **feature record** continues after the name with `00 00 00 00 01 00`. The
feature records of a part come before its part record. An assembly record
comes after the part records of its parts, and no feature record comes
between it and the record before it.

The property set holds named properties. A property is the bytes `f4 ff`, a
`u32` name length in bytes, the NUL-terminated little-endian UTF-16 name, a
`u16` value type, and the value. Type `8` is a string: a `u32` byte length and
the NUL-terminated UTF-16 text. Type `5` is an f64; the value `-1e300` marks an
unset property. The properties include `PartName`, `Part Number`,
`Description`, `Classification`, `PartVolume`, `PartMass`, `MassDensity`,
`SizeboxHeight`, `SizeboxLength`, `SizeboxWidth`, `StartTaper`, `EndTaper`,
and `ModelGUID`. Values are SI: lengths are metres, masses are kilograms, and
`MassDensity` is kilograms per cubic metre.

The `ModelGUID` value is a brace-enclosed GUID string. The record matrix
starts 41 bytes after the end of that string.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 41 | `gap` | Bytes between the `ModelGUID` text and the matrix. |
| 41 | 128 | `matrix` | Sixteen little-endian f64 values in row order. |

Rows 1 to 3 of `matrix` hold a 3 × 3 block and the value `0`. Row 4 holds a
translation in metres and the value `1`. The matrix of a part record places
the part frame in the frame of its parent: a point `p` of the part frame lies
at `p · R + t`, where `R` is the 3 × 3 block and `t` the translation. The
parent of a part that an assembly holds is that assembly, and the matrix of an
assembly record places the assembly in the scene in the same form. A part at
the scene origin stores the identity matrix. A part turned by 30° about the
scene Z axis stores the rows `(cos 30°, sin 30°, 0)` and `(-sin 30°, cos 30°, 0)`
in the first two rows of `R`.

`Shape Data` holds one matrix of the same form for each feature. It maps a
point of the feature body to the part frame as `p · R + t`.
