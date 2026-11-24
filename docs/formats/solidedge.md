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
| `DefaultDistanceSecondary` | Second length unit code of a compound display |
| `DefaultDistanceTertiary` | Third length unit code of a compound display |
| `DefaultDistancePrecision` | Displayed decimal places of a length |
| `DefaultAnglePrimary` | Angle unit code |
| `DefaultAngleSecondary` | Second angle unit code of a compound display |
| `DefaultAngleTertiary` | Third angle unit code of a compound display |
| `DefaultAnglePrecision` | Displayed decimal places of an angle |

The primary code is the document unit. A secondary or tertiary code of
`345` selects no unit.

The length unit codes are below. An option that has abbreviated and full
display forms has one code for each form.

| Code | Unit |
| ---: | --- |
| 59 | metre |
| 60 | nanometre |
| 61 | millimetre |
| 62 | centimetre |
| 63 | kilometre |
| 64 | inch |
| 65 | foot |
| 66 | yard |
| 67 | mile |
| 68 | tenth |
| 69 | hundredth |
| 70 | thousandth |
| 71 | rod |
| 72 | pole |
| 74 | chain |
| 76 | furlong |
| 360 | foot, abbreviated |
| 361 | inch, abbreviated |

The angle unit codes are:

| Code | Unit |
| ---: | --- |
| 77 | radian |
| 78 | degree |
| 79 | minute of arc |
| 80 | second of arc |
| 81 | gradian |
| 357 | degree, abbreviated |
| 358 | minute of arc, abbreviated |
| 359 | second of arc, abbreviated |

A compound display adds smaller units to the primary unit. The
foot-and-inch display stores the foot code as primary and the inch code as
secondary, each in the same display form. The degree-minute-second display
stores the degree code as primary, the minute code as secondary, and the
second code as tertiary.

The document units are display settings. They do not change Parasolid
coordinates, which are in metres for every unit setting.

`PSMcluster0` also stores the five style labels `ANSI (mm)`, `ANSI (cm)`,
`ANSI (m)`, `ANSI (in)`, and `ANSI (ft)` in every document. Each label is a
little-endian `u32` UTF-16 code-unit count followed by that many UTF-16LE
code units. The labels list the available styles and do not select the
document units.

## 3. Kernel stream map

The `PARASOLID` storage contains binary kernel streams. A primary partition
has the path `PARASOLID/STREAM<n>.P_B`, where `<n>` is a stream index of one or
more ASCII decimal digits. Its paired delta stream has the path
`PARASOLID/STREAM<n>.D_B`. Path components compare without regard to ASCII
case. No other path is a primary partition: a stream below a deeper storage,
a leaf with a non-digit in the index, and a leaf with another extension are
not. Each stream has its own transmit header and node-index space.

`JSitesList` also names document sites. Site storages use the name pattern
`JSite<n>`. Assembly sites carry reference and physical-property streams,
including `JAsmRefAttr` and `JAsmPhysicalPropertiesAttr`.

## 4. Transmit envelope

A kernel stream starts with the Parasolid text header. Header lines end with
LF. Named text fields use `KEY=VALUE;`. The header has `**PART1;`, `**PART2;`,
and `**PART3;` sections. The final line starts with `**END_OF_HEADER` and has
star padding. The binary transmit payload starts immediately after that
line's LF. The text header has variable length.

The `FORMAT` declaration is `binary`. The primary partition's `GUISE` is
`transmit_partition`; the delta stream's `GUISE` is `transmit_deltas`.
`KEY` names the stream index as `STREAM<n>`, and `FILE` names the stream leaf.
`SCH` declares the kernel schema. `USFLD_SIZE` declares the user-field size.

The binary payload starts with `PS`. The description and schema token are
length-prefixed byte strings. Each length is an unsigned big-endian `u32`.
The description identifies `(partition)` or `(deltas)` and carries a modeller
version declaration. The schema token selects the Parasolid field grammar.
The binary header and node grammar are specified in
[`parasolid.md`](parasolid.md). An embedded-schema token names its base schema
and is followed by the maximum node type and the user-field size.

## 5. Geometry

Primary partition node indices identify bodies, regions, shells, faces,
loops, fins, edges, vertices, and their carriers. References are local to
the partition. Topology ownership and carrier fields use the Parasolid
schema named in the binary header. Kernel model-space lengths are in metres.
Direction vectors are dimensionless. A line's distance
parameter has the model-space length unit. A body's `res_linear` field is
its positive linear resolution in metres.

A REGION node has a `type` character, `S` for a solid region or `V` for a
void region, a `body` pointer, and a `shell` pointer that heads the region's
shells, chained through `SHELL.next`. A SHELL names its `region`, and its
`face` pointer heads its faces, chained through `FACE.next`. A void region that
owns no face has its boundary in the shell of the neighbouring region, and it
transfers no neutral region when its body also has a solid region or a region
that owns a face. A void region that owns a face transfers as its own region,
which holds those faces. A body that has no region records its shells through
the BODY `shell` pointer, which heads the chain of the body's shells. A BODY's
`ref_instance` is null in a primary partition: the partition holds the body in
its own coordinates.

An edge's `halfedge` names one fin of the edge's radial ring, which the fins
link through `other`. The named fin can lie outside every loop, with null
`backward`, `forward`, and `loop`. The fins of the ring that lie in a loop
carry the edge's topology.

A swept surface (type 67) is the linear extrusion of its `section` curve
along its unit `sweep` direction. The surface does not store the extent along
the sweep; the face loops bound it. A null `scale` makes the sweep parameter
the distance along the direction. Open item DR-1053 holds the role of a
stated `scale`.

An intersection record references two support surfaces, a chart, and start
and end limits. A chart carries a counted array of model-space points, a
base parameter, a nonzero parameter scale, and a repeated point count.
The parameter increment between successive points is their distance in
metres times the scale. `chordal_error` is a model-space length; the null
real denotes an unstated fit bound. The chart polyline approximates the
exact intersection of the two supports, which is the edge carrier. The
polyline can depart from the supports by more than the body's linear
resolution. Each vertex of an edge lies on that intersection within the
stated tolerances of the vertex and the edge, and so can lie off the polyline
by the polyline's departure. The chart may stop short of a vertex or run past
it. Limit records carry a type character and a counted
point array. Types `L`, `H`, `T`, and `B` place their terminal point first in
that array.

## 6. Document version declarations

The root streams `Versions`, `DocVersion2`, and `BuildVersions` carry document
version declarations. These declarations and the kernel schema token occupy
separate streams. The kernel token declares the embedded node grammar.
