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

## 3. PSM object storage

### 3.1 Objects, clusters and space maps

The `PSM` streams store the document's object graph. An object identifier is
a little-endian `u32`. Its bits 13 and above select the cluster, and its low
13 bits are the serial in that cluster. Cluster 0 is the stream
`PSMcluster0`. The space map of cluster `K` is the stream
`PSMspacemap/0x%08x` named by `K << 13`, so cluster 0 has
`PSMspacemap/0x00000000`.

A space map starts with a 12-byte header. All fields are little-endian.

| Offset | Field |
| ---: | --- |
| 0 | ASCII `sseg` |
| 4 | `u16` item count `A` |
| 6 | `u16` count of items without a body |
| 8 | `u16` next free serial |
| 10 | `u16` free-list count `F` |

`F` `u16` free serials follow the header. `A` items follow them in
ascending identifier order and end at the stream end. An item is a `u16`
length `L` and `L` bytes:

| Item offset | Field |
| ---: | --- |
| 0 | `u16` length `L` |
| 2 | `u32` object offset in the cluster stream |
| 6 | `u32` object identifier |
| 10 | `u16` form word; bits 6 and above are the object's form |
| 12 | `u16` |
| 14 | `u16` |
| 16 | `u16` flags; bit `0x2000` selects the class rule in §3.3 |

An item with `L` = 16 has no body. A longer item carries a body of `L` − 16
bytes after the flags.

A cluster stream starts with the `u32` value `0x6C90F544` and a `u32` object
count equal to the space map's item count. The objects follow. An object
starts at its item's offset and ends at the next item's offset, or at the
stream end for the last item. An object starts with an 8-byte frame:

| Offset | Field |
| ---: | --- |
| 0 | `u16` `0x5151` |
| 2 | `u16` form, equal to the item's form word shifted right by 6 |
| 4 | `u32` object identifier, equal to the item's identifier |

### 3.2 Type table

`PSMtypetable` contains two consecutive sections. Each section has a 20-byte
header that starts with ASCII `ePyT`. The `u16` values at header offsets 10
and 14 are equal and give the section's record count.

The first section holds `N` named declarations, starting at stream offset 20.
A declaration has this layout:

| Offset | Field |
| ---: | --- |
| 0 | 16-byte class GUID |
| 16 | `u32` declaration index, from `0` through `N` − 1 |
| 20 | `u16` `1` |
| 22 | `u16` `0xFFFF` |
| 24 | `u32` metadata word |
| 28 | `u16` class-name byte count `n` |
| 30 | ASCII class name of `n` bytes, ending with one zero byte |
| 30 + `n` | `u16` byte count `m` of a second text field |
| 32 + `n` | second text field of `m` bytes |

The next declaration starts at offset 32 + `n` + `m`. The second section
starts after the last declaration. It holds `K` compact records of 26 bytes,
starting 20 bytes after its header: a 16-byte GUID and ten bytes, with a
`u16` value `V` at record offset 18. The second section ends at the stream
end.

### 3.3 Object class

The class of an object is a named declaration. Let `f` be the object's form
and `Q` the flags of its space-map item. The class is declaration `f` when
`Q` has bit `0x2000` set, when `f` ≥ `K`, or when compact record `f` has
`V` = `0xFFFF`. Otherwise the class is declaration `V` of compact record
`f`. Form numbers differ between documents; class names identify classes.

## 4. Feature order

### 4.1 Feature navigator object

One cluster-0 object holds the ordered feature list. Its frame is followed by
a `u32` flag word and the GUID `{44F66D50-4DD1-11CE-BF4E-08003601C72A}` at
object offset 12. The rest of the object has this sequence. All integers are
little-endian `u32` values.

| Field | Content |
| --- | --- |
| first list | count `a`, then `a` object identifiers |
| second list | count `b`, then `b` object identifiers |
| constant block | 147 bytes |
| navigator name | `31`, then ASCII `JMFeatureNavigatorPartModified` and one zero byte |
| constant block | 68 bytes |
| four words | `0`, `0`, `1`, `1` |
| owner | identifier of the `CLSID_JMPartOwner` object, or `0` in a document without features |
| last feature | identifier of the last listed feature |
| entry count | `M` |
| entries | `M` pairs of `0` and an object identifier |
| final word | |

One more `u32` can follow the final word. The object then ends. The first
three entries are the three base reference planes, of class
`ReferencePlaneObject`. The other `M` − 3 entries are the feature-tree
items in order. A feature is an object of class `CLSID_JMPart` or
`CLSID_JMGroup`. A feature-tree folder is an object of class
`CLSID_JMFeatureGroup`.

### 4.2 Feature kind

A `CLSID_JMPart` object stores its function name. The `u32` at object offset
132 is a byte count `L`, and `L` bytes follow it. A `u32` byte count `s`
follows them, then `s` bytes of ASCII function name ending with one zero
byte. The function names are:

| Function name | Feature |
| --- | --- |
| `JMProtrusionFunction` | extruded protrusion |
| `JMCutoutFunction` | extruded cutout |
| `JMRevolvedProtrusionFunction` | revolved protrusion |
| `JMMultiSweepProtrusionFunction` | swept protrusion |
| `JMLoftedProtrusionFunction` | lofted protrusion |
| `JMHoleFunction` | hole |
| `JMRoundFunction` | round |
| `JMChamferFunction` | chamfer |
| `JMThinwallAssocFunctionNew` | thin wall |
| `JMBaseTabFunction` | sheet-metal base tab |
| `JMFlangeFunction` | sheet-metal flange |

A `CLSID_JMGroup` feature is a mirror copy or a pattern.

## 5. Sketches

### 5.1 Profile objects

A profile is a group of cluster-0 objects with consecutive identifiers: a
`CLSID_JProfileSet` object `S`, the `CLSID_JProfile` object `S` + 1, and
the `CLSID_JProfile2d` object `S` + 2. A profile is stored only when a
feature consumes it. All lengths are metres and all angles are radians.

A `CLSID_JProfile` object stores the sketch frame:

| Offset | Field |
| ---: | --- |
| 26 | three `f64`: origin in model space |
| 50 | three `f64`: vector `A`, the plane normal |
| 74 | three `f64`: vector `B`, the local x axis |
| 127 | `u32` identifier of the `CLSID_JProfile2d` object |
| 292 | `u32` `0x10`, `u32` `0x80000000`, `u32` identifier of the plane |

The local y axis is `A` × `B`. A point (x, y) of the profile is the model
point origin + x·`B` + y·(`A` × `B`). The plane is a `ReferencePlaneObject`.
The origin, `A` and `B` of the profile equal those of its plane.

A `CLSID_JProfile2d` object lists the profile members. The `u32` at object
offset 8 is the member count `n`, and `n` `u32` member identifiers follow
it.
A member identifier `0xFFFFFFFF` names no object; the profile of a revolved
protrusion lists one.

### 5.2 Profile curves

A member of class `CLSID_JLine2d` is a line segment. The four `f64` at
object offsets 10, 18, 26 and 34 are x0, y0, x1 and y1 in the profile
frame: the segment runs from (x0, y0) to (x1, y1).

A member of class `CLSID_JCircle2d` is a full circle. The `f64` at object
offsets 10 and 18 are the centre x and y in the profile frame, and the
`f64` at offset 26 is the radius.

A member of class `CLSID_JArc2d` is a circular arc. The `f64` at object
offsets 10, 18 and 26 are the centre x, y and the radius. The `f64` at
offsets 35 and 43 are the start and end angles, measured from the local x
axis towards the local y axis. The arc runs from the start angle to the end
angle in that sense.

A member of class `CLSID_JKeyPoint2d` joins two curves and stores no
coordinate. Members of other classes are constraints, dimensions and other
profile items.

### 5.3 Feature profile

A `CLSID_JMPart` feature with function `JMProtrusionFunction`,
`JMCutoutFunction`, `JMRevolvedProtrusionFunction`,
`JMMultiSweepProtrusionFunction`, `JMLoftedProtrusionFunction` or
`JMBaseTabFunction` stores the identifier of its profile as the `u32` 72
bytes after the end of its function name (§4.2). For
`JMMultiSweepProtrusionFunction` that profile is the sweep path, and the
`u32` 104 bytes after the end of the function name is the identifier of the
section profile. A feature with function
`JMHoleFunction`, `JMFlangeFunction`, `JMBaseContourFlangeFunction` or
`JMFastPatternFunction` stores it 68 bytes after the end of its function
name. The identifier names a
`CLSID_JProfile` object, or a `CLSID_JPatternProfile` object for a pattern
profile. At most one feature names a profile.

## 6. Feature extents

### 6.1 Parameter slots

A `CLSID_JMPart` feature with function `JMProtrusionFunction`,
`JMCutoutFunction` or `JMRevolvedProtrusionFunction` stores its parameters
as `u32` object identifiers after the end of its function name (§4.2):

| Bytes after the name | Object |
| ---: | --- |
| 72 | the profile (§5.3) |
| 100 | the extent kind |
| 128 | the side |
| 184 | the depth of an extruded feature, or the axis of a revolved feature |
| 212 | the sweep angle of a revolved feature |

An identifier `0` names no object. A `Double Value Object` stores its value
as the `f64` at object offset 8. A `CLSID_JDim` stores its value as §7.1
states. Lengths are metres and angles are radians.

### 6.2 Extent kind and side

The extent kind is a `Double Value Object`. Value `13` is a finite extent
and value `16` is an extent through all material.

The side is a `Double Value Object`. Value `2` extends the feature along the
profile normal `A` (§5.1), value `1` extends it against the normal, and
value `3` extends it to both sides of the profile plane. For a revolved
feature, value `2` sweeps towards the profile normal and value `1` away
from it.

### 6.3 Depth

The depth of a finite extruded feature is a `CLSID_JDim` or a
`Double Value Object`. A one-sided feature extends by the depth from the
profile plane. A feature extended to both sides extends by half the depth to
each side. A feature through all material stores no depth.

### 6.4 Revolve axis and angle

The axis of a revolved feature is a `CLSID_JMRefAxis` object. Its byte at
object offset 12 is `3`. The three `f64` at offset 13 are a point on the
axis in model space, and the three `f64` at offset 37 are the unit axis
direction. The sweep angle is a `Double Value Object` or a `CLSID_JDim`.

## 7. Holes, rounds, chamfers and thin walls

### 7.1 Dimension values

A `CLSID_JDim` object stores the `u32` value `1`, then the identifier `T` of
the object it dimensions at offset `p`, then a `u32` type code. The `u32` at
`p` + 50 is `T` again, and the `f64` at `p` + 54 is the value. `p` is 148 in
the 459-byte and 466-byte layouts and 144 in the 455-byte layout.

### 7.2 Holes

A `CLSID_JMPart` feature with function `JMHoleFunction` stores these object
identifiers after the end of its function name:

| Bytes after the name | Object |
| ---: | --- |
| 68 | the profile (§5) |
| 96 | the `CLSID_JMHolePropData` object |
| 124 | the counterbore depth, a `CLSID_JDim`, or `0` |
| 152 | the countersink angle, a `CLSID_JDim`, or `0` |
| 208 | the extent kind (§6.2) |
| 236 | the side (§6.2) |
| 264 | the depth of a finite hole, a `CLSID_JDim`, or `0` |

Each `CLSID_JHole2d` member of the profile is one hole position. Its `f64`
values at object offsets 8 and 16 are the position x and y in the profile
frame (§5.1).

A `CLSID_JMHolePropData` object stores:

| Offset | Field |
| ---: | --- |
| 8 | `u32` hole type: `0` simple, `1` countersink, `2` counterbore, `3` tapped |
| 12 | `f64` hole diameter; the tap drill diameter of a tapped hole |
| 20 | `f64` counterbore or countersink diameter |
| 28 | `f64` counterbore depth |
| 36 | `f64` countersink angle, in degrees |
| 72 | `f64` hole depth |
| 1053 | `f64` thread minor diameter, in millimetres |
| 1061 | `f64` thread major diameter, in millimetres |
| 1069 | `u32` thread designation length `k`, in UTF-16 code units |
| 1073 | `k` UTF-16LE code units: the thread designation |

A hole of type `3` is threaded with the stored thread. A hole of another
type can keep thread values from an earlier type; they do not apply.

### 7.3 Rounds

A `CLSID_JMPart` feature with function `JMRoundFunction` holds `n` edge sets.
Its object identifiers after the end of its function name are:

| Bytes after the name | Object |
| ---: | --- |
| 72 + 20·k | the `CLSID_JMTopologyProxy` that selects the edges of set `k` |
| 84 + 20·n + 20·k | the kind of set `k`, a `Double Value Object` |
| 96 + 40·n + 20·k | the radius of set `k`, a `CLSID_JDim` |

`k` runs from `0` to `n` − 1. Set kind `1` is a constant radius. The count
`n` is the number of consecutive `CLSID_JMTopologyProxy` identifiers from
72 bytes after the name.

### 7.4 Chamfers

A `CLSID_JMPart` feature with function `JMChamferFunction` stores these
object identifiers after the end of its function name:

| Bytes after the name | Object |
| ---: | --- |
| 68 | the chamfer type, a `Double Value Object` |
| 96 | the `CLSID_JMTopologyProxy` of the reference face, or `0` |
| 128 | the `CLSID_JMTopologyProxy` of the edges |
| 156 | the first setback, a `CLSID_JDim` |
| 184 | the second setback or the angle, a `CLSID_JDim`, or `0` |

Chamfer type `41` has one setback on both faces. Type `43` has two setbacks;
the first setback is on the reference face. Type `42` has a setback on the
reference face and an angle from it.

### 7.5 Thin walls

A `CLSID_JMPart` feature with function `JMThinwallAssocFunctionNew` stores
these object identifiers after the end of its function name:

| Bytes after the name | Object |
| ---: | --- |
| 68 | the thin-wall type, a `Double Value Object` |
| 96 | the wall thickness, a `CLSID_JDim` |
| 124 | the `CLSID_JMTopologyProxy` of the open faces |

Type `4` hollows the body inward by the thickness, with the open faces
removed.

### 7.6 Edge selections

A `CLSID_JMTopologyProxy` object of 559 bytes selects one straight edge. Its
`f64` values at object offsets 121, 129 and 137 are the edge start point,
the `f64` values at offsets 145, 153 and 161 are the unit edge direction,
and the `f64` at offset 177 is the edge length. Lengths are metres.

A proxy of another size `S` stores the edge start point at offset `S` - 438.
The unit edge direction follows the start point after 24 bytes, and the edge
length follows it after 56 bytes. The 559-byte offsets above are the case
`S` = 559. A proxy of 563 bytes stores an 8-byte header extension. A proxy of
575 bytes stores two more `(u32 0, u32 n << 16)` pairs in its name list.

## 8. Patterns and mirrors

### 8.1 Pattern features

A pattern is a `CLSID_JMGroup` entry of the navigator (§4.1). The group
object stores:

| Offset | Field |
| ---: | --- |
| 106 | `u32` identifier of the `CLSID_JMPart` object with function `JMFastPatternFunction` |
| 337 | `u32` identifier of the seed feature |

The profile of the `JMFastPatternFunction` object (§5.3) has one
`CLSID_JPattern2d` member.

A `CLSID_JPattern2d` object holds the pattern parameters after the last
occurrence of the GUID `{0145EEC0-1602-11CE-B7EE-08003601E53B}` in it. With
`E` the offset after that GUID:

| Offset | Field |
| ---: | --- |
| `E` + 14 | `u32` pattern kind: `1` rectangular, `2` circular |
| `E` + 26 | `u32` first count |
| `E` + 30 | `u32` second count |
| `E` + 34 | `f64` first spacing, or the angle between instances |
| `E` + 42 | `f64` second spacing |

The `CLSID_JPattern2d` object references its sketch geometry by
`u32` triples of `0x10`, `0x80000000` and an object identifier. A
rectangular pattern references four `CLSID_JLine2d` objects that run round
the rectangle from the first instance to the last. The first line runs from
the seed position along the first direction. The second line runs along the
second direction. The first count and spacing apply along the first
direction, and the second count and spacing along the second direction.

A circular pattern references one `CLSID_JArc2d` object. Its centre is on
the pattern axis, which is normal to the profile plane. The first count is
the number of instances, and the angle between instances is the
`f64` at `E` + 34. The instances follow the seed counter-clockwise about
the profile normal `A` (§5.1).

All values are in profile coordinates, metres and radians.

### 8.2 Mirror features

A mirror is a `CLSID_JMGroup` entry of the navigator whose group object
stores:

| Offset | Field |
| ---: | --- |
| 92 | `u32` seed count `c` |
| 96 | `c` `u32` seed feature identifiers |
| 325 + 4·`c` | `u32` identifier of the mirror plane |

The mirror plane is a `ReferencePlaneObject`, the plane about which the seed
is mirrored. Its object stores three `f64` at offset 13, a point on the plane,
and three `f64` at offset 37, the unit plane normal. The point is in metres.

## 9. Assembly occurrences

### 9.1 Occurrence list

The cluster-0 object of class `References Collection Object` lists the
occurrences of an assembly document. The `u32` at object offset 16 is the
occurrence count `n`. `ceil(n / 8)` bytes follow it, then `n` `u32`
identifiers of `CLSID_JAsmReference` objects, one per occurrence.

### 9.2 Occurrence object

A `CLSID_JAsmReference` object stores:

| Offset | Field |
| ---: | --- |
| 16 | `u32` state word; bit `0x04` set states that the occurrence is suppressed |
| 113 | `u32` identifier of the `CLSID_JSite` object of the referenced document |
| 125 | nine `f64`: the rotation `R`, row-major |
| 197 | three `f64`: the translation `t`, in metres |
| 221 | `f64` `1` |

Row `i` of `R` is the direction of the referenced document's axis `i` in the
assembly. A point `p` of the referenced document, as a row vector, is at
`p` `R` + `t` in the assembly. The state word is `2` for an occurrence that
is not suppressed. Occurrences of one document name one `CLSID_JSite`
object.

### 9.3 Referenced document

A `CLSID_JSite` object with identifier `N` has its link data in the stream
`JSite<N>/\x01Ole`, where `<N>` is `N` in decimal. The stream has a 12-byte
header, then a file moniker at offset 12:

| Offset | Field |
| ---: | --- |
| 0 | the file moniker CLSID `{00000303-0000-0000-C000-000000000046}` |
| 16 | `u16` up-level count, `0` |
| 18 | `u32` byte count `n` |
| 22 | `n` bytes of ASCII drive-absolute path, ending with one zero byte |
| 22 + `n` | `u16` `0xFFFF` |
| 24 + `n` | `u16` `0xDEAD` |

A second file moniker of the same layout follows in the stream. Its up-level
count is `1` and its path is relative to the folder of the assembly
document.

## 10. Sheet metal

### 10.1 Variable table

The stream `CustomPropertyVariableInfo` lists the document variables that
the software defines. It starts with a `u32` record count `n` and a `u32`
version `1`. `n` records of 24 bytes follow:

| Offset | Field |
| ---: | --- |
| 0 | `u32` key |
| 4 | `u32` index |
| 8 | `u32` identifier of the cluster-0 `Double Value Object` that holds the variable |
| 12 | `u16` unit type: `1` length in metres, `16` density, `58` unitless |
| 14 | `u16` zero |
| 16 | `f64` value, equal to the value of the referenced object |

The key is `(i - 1) << 24 | 2` for index `i` 2 or 3, and `i << 24` for the
other indices. A part document (`.par`) lists indices 2 and 3. A sheet-metal
document (`.psm`) lists indices 2 to 13. An assembly document lists no
record. The indices are:

| Index | Variable |
| ---: | --- |
| 2 | density |
| 3 | accuracy |
| 4 | material thickness |
| 5 | bend radius |
| 6 | relief width |
| 7 | relief length |
| 8 | neutral factor |
| 9 | minimum arc length |
| 10 | deviational tolerance |
| 11 | bend deduction |
| 12 | bend allowance |
| 13 | bend method: `0` neutral factor, `1` bend deduction |

The referenced object of a variable also stores the variable name as
UTF-16 text. The neutral factor is the K-factor: the distance of the neutral
surface from the inside of a bend, as a fraction of the thickness.

### 10.2 Base tab

A `CLSID_JMPart` feature with function `JMBaseTabFunction` stores its
profile at the §5.3 position, the identifier of the material-thickness
variable object (§10.1) 100 bytes after the end of its function name, and a
side `Double Value Object` 128 bytes after the end of its function name.
The base tab is a flat sheet of the material thickness over the region of
its profile. Side `2` puts the material along the profile normal `A`
(§5.1) and side `1` against it.

### 10.3 Flange

A `CLSID_JMPart` feature with function `JMMultiFlangeFunction` stores these
object identifiers after the end of its function name (§4.2):

| Bytes after the name | Object |
| ---: | --- |
| 40 | the `CLSID_JMPart` feature that precedes this flange in the part |
| 72 | the `CLSID_JMTopologyProxy` edge selection (§7.6) |
| 100 | `CLSID_JDim` flange length |
| 128 | `CLSID_JDim` entered angle |
| 156 | `Double Value Object` material thickness, the document variable of §10.1 index 4 |
| 184 | `Double Value Object` extent kind |
| 212 | `Double Value Object` material side |
| 240 | `Double Value Object` side |
| 296 | `Double Value Object` inside bend radius |
| 324 | `Double Value Object` neutral factor |
| 380 | `Double Value Object` relief width |
| 408 | `Double Value Object` relief length |
| 440 | `Double Value Object` offset kind, `44` |
| 492 | `CLSID_JDim` partial width where the extent kind is `135`; offset where the extent kind is `138` |
| 496 | `Double Value Object` relief type, where the extent kind is `134` |
| 512 | `CLSID_JDim` width, only where the extent kind is `138` |
| 520 | `Double Value Object` relief type, where the extent kind is `135` |
| 540 | `Double Value Object` relief type, where the extent kind is `138` |
| 832 | `Double Value Object` bend method, the document variable of §10.1 index 13 |
| 860 | `Double Value Object` bend deduction, the document variable of §10.1 index 11 |
| 888 | `Double Value Object` bend allowance, the document variable of §10.1 index 12 |

The slots from offset 492 on are 24 bytes later where the extent kind is
`135` than where it is `134`, and 44 bytes later where the extent kind is
`138`. The slots at offsets 832, 860 and 888 are
those of the extent kind `134` layout. The feature holds one edge. A flange
that follows another flange in the part names that flange at offset 40.

The extent kind is `134` for the full length of the edge and `135` for a
width centred on the edge. The partial width is the stored whole-edge proxy
and a `CLSID_JDim` at offset 492; the file stores no offset from either end
of the edge. The side value `2` turns the flange towards the normal of the
base face and the side value `1` turns it against that normal. The material
side is `1` for material inside, `2` for material outside and `9` for bend
outside. The flange stores no thickness side. The relief type is `53` for no
relief, `51` for a square relief and `52` for a round relief.

The extent kind `138` is a width along the edge that starts at an offset from
one end of the edge. The `CLSID_JDim` at offset 492 stores the offset and the
`CLSID_JDim` at offset 512 stores the width. The stored edge proxy is the whole
edge.

The relief width and the relief length dimension the notch that the relief
cuts in the base at each end of the flange width. The notch lies outside the
flange width along the edge and cuts the full thickness. The relief width is
the extent of the notch along the edge, from the end of the flange width away
from the flange. The relief length is the extent of the notch across the edge,
from the line where the bend meets the base face, away from the edge, to the
far line of the notch. The notch also removes the bend and the base between
that line and the edge. A relief length of zero ends the notch at that line.
A square relief ends the notch at the far line. A round relief adds to the
notch a half-round end of diameter equal to the relief width, centred on the
far line and lying beyond it.

The length `CLSID_JDim` stores the flange length. The length runs from the
outer sharp, the intersection of the outer faces of the base and the flange,
to the top of the flange. The angle `CLSID_JDim` stores the entered angle
`a`; the flange turns through the angle pi - `a` from the plane of the face
that bounds the edge.

The values at offsets 296, 324, 380 and 408 are objects of the flange, each
bound to a document variable of §10.1 by one `Assoc subsystem Standard
Relation implementation` object of 161 bytes. The `u32` at object offset 99
is the identifier of the flange object. The `u32` at object offset 127 is
the identifier of the document variable object (§10.1). A value that has a
relation follows the document variable. A value that has no relation is set
on the flange and holds its own number. The `u16` at object offset 18 of the
objects at offsets 296, 324, 380 and 408 is `149` where a relation binds the
object and `157` where none does.

A `JMBendTable` object stores 362 bytes and one 93-byte record per bend. In
record `k`, the `f64` at object offset 374 + 93`k` is the inside bend radius
and the `f64` at object offset 382 + 93`k` is the bend angle pi - `a`.

### 10.4 Hem

A `CLSID_JMPart` feature with function `JMHemAssocFunction` stores these
object identifiers after the end of its function name (§4.2):

| Bytes after the name | Object |
| ---: | --- |
| 68 | the `CLSID_JMTopologyProxy` edge selection (§7.6) |
| 96 | `Double Value Object` form code |
| 124 | `Double Value Object` bend side code |
| 208 | `Double Value Object` inner bend radius |
| 236 | `Double Value Object` length of the straight part |
| 264 | `Double Value Object`, half the sheet thickness |
| 292 | `Double Value Object`, four times the sheet thickness |
| 376 | `Double Value Object`, the sheet thickness |
| 404 | `Double Value Object`, the sheet thickness |

The form code is `207` for a closed hem, `208` for an open hem, `209` for an
S-flange, `210` for a curl and `212` for a closed loop. Every form stores the
inner bend radius at offset 208 and the straight length at offset 236. The
inner bend radius is half the gap between the hem and the face. The
straight length is measured from the plane through the fold axis to the free
end. The entered hem length is the straight length plus the inner bend radius
plus the sheet thickness. Lengths are metres.

The bend side code at offset 124 is `1` when the bend lies on the material
outside, `2` when it lies on the material inside and `9` when it lies on the
bend outside. The slots at offsets 152 and 180 hold `44` in every form and
side. The value at offset 432 is `0.44`. The values at offsets 320, 348 and
488 are zero.

The edge proxy selects the free edge of the wall on the picked face of the
sheet. The side code does not change the edge proxy.

### 10.5 Base contour flange

A `CLSID_JMPart` feature with function `JMBaseContourFlangeFunction` stores
these object identifiers after the end of its function name:

| Bytes after the name | Object |
| ---: | --- |
| 68 | the `CLSID_JProfile` open profile (§5.3) |
| 124 | `Double Value Object`, `1` |
| 152 | `Double Value Object` extent kind, `13` for a finite extent (§6.2) |
| 180 | `Double Value Object`, `1` |
| 208 | `CLSID_JDim` extent distance along the profile normal |
| 236 | `Double Value Object` inside bend radius |
| 264 | `Double Value Object` neutral factor |

The extent distance is the distance from the profile plane. The slots at
offsets 124 and 180 each hold the side value of §6.2 or a value that no stored
feature separates from it.

### 10.6 Unfold and refold

A `CLSID_JMPart` feature with function `JMUnfoldFunction` stores a
`CLSID_JMTopologyProxy` face selection at 68 bytes after the end of its
function name and a second one at 96 bytes. A feature with function
`JMRefoldFunction` stores one `CLSID_JMTopologyProxy` at 68 bytes. The
selections are not edge selections: they are larger than 559 bytes and store
no edge line.

### 10.7 Close corner

A `CLSID_JMPart` feature with function `JMCloseCornerAssocFunction` stores
these object identifiers after the end of its function name:

| Bytes after the name | Object |
| ---: | --- |
| 68 | the `CLSID_JMTopologyProxy` selection of the corner |
| 124 | `Double Value Object` closure code |
| 152 | `Double Value Object` treatment code |
| 180 | `Double Value Object` relief size |
| 244 | `CLSID_JDim` corner gap |
| 272 | `Double Value Object`, `0` for closure `103` and `0.5` for closure `104` |
| 420 | `Double Value Object` that repeats the relief size |

The closure code is `103` for closed faces and `104` for overlapped faces.
The treatment code is `105` for no treatment, `106` for intersect, `107` for
a circular cutout, `220` for a U cutout, `221` for a V cutout, `222` for a
square cutout and `232` for a miter. The slots at offsets 300, 336, 364 and
392 hold `2`, `1`, `0` and pi / 4 in every close corner.

Lengths are metres.

## 11. Kernel stream map

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

## 12. Transmit envelope

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

## 13. Geometry

Primary partition node indices identify bodies, regions, shells, faces,
loops, fins, edges, vertices, and their carriers. References are local to
the partition. Topology ownership and carrier fields use the Parasolid
schema named in the binary header. Kernel model-space lengths are in metres.
Direction vectors are dimensionless. A line's distance
parameter has the model-space length unit. A body's `res_linear` field is
its positive linear resolution in metres. The part's linear and angular
tolerances follow [`parasolid.md`](parasolid.md) §4 over
the bodies of every primary partition.

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

A FACE whose `loop` pointer is null on a plane, cylinder, or cone states no
boundary. The face is stated partial with the loss
`topology.face-boundary-unstated`.

The decoder gives up a face that states no orientation. The loss of the face
lists as dropped its loops and their fins. The decoder gives up a loop that it
cannot read, such as a loop whose fins do not form a ring or a loop that uses
an edge that it cannot read. The loss of the loop lists as dropped its fins,
its face, and the other loops of that face with their fins.

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

## 14. Document version declarations

The root streams `Versions`, `DocVersion2`, and `BuildVersions` carry document
version declarations. These declarations and the kernel schema token occupy
separate streams. The kernel token declares the embedded node grammar.
