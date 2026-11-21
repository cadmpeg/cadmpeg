# Solid Edge compound documents

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

## 1. Container

Solid Edge `.par`, `.psm`, and `.asm` documents use Microsoft Compound File
Binary. The compound directory stores UTF-16LE entry names and the storage
hierarchy. A stream entry identifies its logical byte length and allocation
chain. Regular streams use the FAT. Small streams use the mini FAT and the
root storage's mini stream.

### 1.1 Document identity

A part, sheet-metal, or assembly document has the root entries `JSitesList`
and `PSMroots`. A family-of-assemblies document has the root entries
`FamilyInfo` and `FOAPublishData`. Entry names compare without regard to ASCII
case. A document of the first form has the root storage CLSID of a part, sheet-metal,
or assembly document, and a document of the second form has the family CLSID.
The root storage CLSID distinguishes the document kind:

| Kind | Extension | Root CLSID |
|---|---|---|
| Part | `.par` | `23c52e80-4698-11ce-b307-0800363a1e02` |
| Sheet metal | `.psm` | `dd8522e0-2375-11d0-ac05-080036fd1802` |
| Assembly | `.asm` | `00c6bf00-483b-11ce-951a-08003601be52` |
| Family of assemblies | `.asm` | `04d613a0-a322-40b5-a2a4-36ca0de6f5d9` |

The CLSID is written in the canonical GUID form; the directory stores its
first three fields little-endian.

## 2. Document settings

`PSMcluster0` stores the document settings in typed property sets. A part,
sheet-metal, or assembly document has one such stream at the root.

### 2.1 Property sets

A property set starts with a 16-byte header. All header fields are
little-endian.

| Offset | Field | Value |
| ---: | --- | --- |
| 0 | `u32` zero word | `0` |
| 4 | `u16` set tag | `0x5151` |
| 6 | `u16` set form | |
| 8 | `u32` set index | |
| 12 | `u32` record count `N` | |

The set form does not change the record grammar.

`N` records follow the header without padding. A record has this layout.
All integer fields are little-endian.

| Offset | Field |
| ---: | --- |
| 0 | `u32` name byte count `n` |
| 4 | name: `n` bytes of UTF-16LE, ending with one NUL code unit |
| 4 + `n` | `u32` record identifier |
| 8 + `n` | `u32` flag |
| 12 + `n` | `u32` value type |
| 16 + `n` | value |

`n` is even and includes the NUL code unit. The value type sets the value
width:

| Type | Value |
| ---: | --- |
| 1 | `u32` |
| 2 | `u32` |
| 4 | IEEE 754 binary64 |
| 6 | `u32`, `0` or `1` |

The next record starts immediately after the value. The set ends after its
`N`th record. The stream contains other object data between and after the
property sets, and that data can contain the zero word and set-tag bytes. A header is a
property set only when all `N` records parse to the end of the set.

### 2.2 Document units

The property set with index `2` stores the document unit settings as type-2
records. The record names identify the settings; the record order and the
record identifiers do not.

| Record name | Setting |
| --- | --- |
| `DefaultDistancePrimary` | Length unit code |
| `DefaultDistancePrecision` | Displayed decimal places of a length |
| `DefaultAnglePrimary` | Angle unit code |
| `DefaultAnglePrecision` | Displayed decimal places of an angle |

The length unit codes are:

| Code | Unit |
| ---: | --- |
| 59 | metre |
| 61 | millimetre |
| 62 | centimetre |
| 64 | inch |

The angle unit codes are:

| Code | Unit |
| ---: | --- |
| 77 | radian |
| 357 | degree |

The document units are display settings. They do not change Parasolid
coordinates, which are in metres for every unit setting.

`PSMcluster0` also stores the five style labels `ANSI (mm)`, `ANSI (cm)`,
`ANSI (m)`, `ANSI (in)`, and `ANSI (ft)` in every document. Each label is a
little-endian `u32` UTF-16 code-unit count followed by that many UTF-16LE
code units. The labels list the available styles and do not select the
document units.
