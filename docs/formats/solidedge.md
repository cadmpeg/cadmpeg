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

## 2. Kernel stream map

The `PARASOLID` storage contains binary kernel streams. A primary partition
has the path `PARASOLID/STREAM<n>.P_B`, where `<n>` is a decimal stream index.
Its paired delta stream has the path `PARASOLID/STREAM<n>.D_B`.
Each stream has its own transmit header and node-index space.

`JSitesList` also names document sites. Site storages use the name pattern
`JSite<n>`. Assembly sites carry reference and physical-property streams,
including `JAsmRefAttr` and `JAsmPhysicalPropertiesAttr`.

## 3. Transmit envelope

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

## 4. Geometry

Primary partition node indices identify bodies, regions, shells, faces,
loops, fins, edges, vertices, and their carriers. References are local to
the partition. Topology ownership and carrier fields use the Parasolid
schema named in the binary header. Kernel model-space lengths are in metres.
Direction vectors are dimensionless. A line's distance
parameter has the model-space length unit. A body's `res_linear` field is
its positive linear resolution in metres.

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
vertices of an edge lie on that intersection. The chart may stop short of a
vertex or run past it. Limit records carry a type character and a counted
point array. Types `L`, `H`, `T`, and `B` place their terminal point first in
that array.

## 5. Document version declarations

The root streams `Versions`, `DocVersion2`, and `BuildVersions` carry document
version declarations. These declarations and the kernel schema token occupy
separate streams. The kernel token declares the embedded node grammar.
