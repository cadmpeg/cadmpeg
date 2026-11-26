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
([`parasolid.md`](parasolid.md) §4). The description names the modeller
version, for example `: TRANSMIT FILE created by modeller version 3401176`. The
schema token has the form `SCH_<modeller>_<schema>_<base>`, for example
`SCH_3401176_34101_13006`. Lengths are metres.

### 3.2 ACIS kernel bytes

The kernel bytes are one interned ACIS binary save stream
([`asm.md`](asm.md) §2.4). It starts with the magic `ACIS BinaryFile` and ends
with the `End-of-ACIS-data` identifier. The save-format word is `3200`, and the
header product string names `IronCAD`. Lengths are centimetres (`asm.md` §4).

## 4. Units

The `Document Data` stream names the scene units as UTF-16LE strings: a length
unit (`Millimeters`), an angle unit (`Degrees`), and a mass unit (`Kilograms`).
Parasolid kernel lengths are in metres.
