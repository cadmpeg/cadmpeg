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
| 22 | `u16` `0xFFFF` or `0x000E`; the class rule (§3.3) does not use it |
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

The final word does not depend on the kind of any feature. Let `n` be the
number of feature-tree items that are not feature-tree folders. A mirror copy
and a pattern each count once, as do a swept, lofted, revolved or sheet-metal
feature. A feature-tree folder adds an entry and does not change `n`. In a
document with no suppressed feature, no failed feature and no earlier
unsuppression, the word is `n` + 1. Each suppressed feature adds the number of
non-folder items that follow it in the feature-tree order, including a mirror
copy that failed because its source is suppressed. Each feature whose recompute
failed subtracts one. Unsuppressing a feature adds the number of non-folder items that follow it,
plus one, and does not restore the word: the word of a document with no
suppressed feature stays above `n` + 1 after an unsuppression. Suppressing the
same feature again adds the number of following non-folder items again. A SaveAs
sequence and a save in place after the same actions store the same word.
The word is not a feature count, and the reader does not use it.

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
| `JMBodyStateToggleAssocFunction` | conversion of a construction body to a design body |

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
bytes after the end of its function name (§4.2). A feature with function
`JMHoleFunction`, `JMFlangeFunction` or `JMFastPatternFunction` stores it 68
bytes after the end of its function name. The identifier names a
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
| 156 | the profile side |
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

The profile side is a `Double Value Object` of an extruded feature. Value `1`
is the left side of the profile and value `2` is the right side. The slot
names no object in the first feature of a document. The profile side does
not change the extent, the side or the geometry of the feature.

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

## 7. Holes, rounds and chamfers

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
72 bytes after the name. A round of the three edges that meet at one corner
holds three sets of kind `1`, one for each edge, each with its own radius.

Set kind `2` is a variable radius. In a round of one set of kind `2`, the
radius slot of the set holds `0`. The vertex proxies follow: `m` consecutive
`CLSID_JMTopologyProxy` identifiers, 20 bytes apart, start 168 bytes after the
name. The `m` radii follow: `m` `CLSID_JDim` identifiers, 20 bytes apart, start
12 bytes after the last vertex proxy. Vertex `j` and radius `j` form one
station. A vertex proxy (§7.5) holds an end point of the edge of the set. The
station is at the start of the edge when the vertex is the start point of the
edge, and at its end when the vertex is the end point.

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

Chamfer type `41` has one setback on both faces. Type `43` has two setbacks.
Type `42` has a setback and an angle.

### 7.5 Topology proxies

A `CLSID_JMTopologyProxy` object holds the geometry of the entity it selects,
in the part frame and in metres. The geometry class identifier is the first
16-byte sequence in the object whose last ten bytes are `CE 11 9A 5E 08 00 36
9B 41 02`. The geometry record starts at the byte after it. The first six
bytes of the identifier give the geometry class:

| First six bytes | Selected entity | Geometry record |
| --- | --- | --- |
| `74 0C 54 25 15 3A` | straight edge | `u8` `3`, `f64` start point (3), `f64` unit direction (3), `f64` `0`, `f64` length |
| `76 E3 FE 7C 16 3A` | planar face | `u8` `14`, `f64` point (3), `f64` unit normal (3), `f64` unit vector (3) |
| `72 C7 D4 1D 15 3A` | vertex | `f64` point (3) |
| `92 DA 66 66 16 3A` | circular edge | `u8` `15`, `f64` center (3), `f64` unit axis (3) |

The end point of a straight edge is the start point plus the length times the
direction. The unit normal of a planar face points out of the material.

### 7.6 Thin walls

A `CLSID_JMPart` feature with function `JMThinwallAssocFunctionNew` stores the
`CLSID_JMTopologyProxy` of the open face 124 bytes after the end of its
function name.

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
`f64` at `E` + 34.

All values are in profile coordinates, metres and radians.

### 8.2 Mirror features

A mirror is a `CLSID_JMGroup` entry of the navigator whose group object
stores:

| Offset | Field |
| ---: | --- |
| 92 | `u32` seed count `c` |
| 96 | `c` `u32` seed feature identifiers |
| 329 | for `c` = 1, `u32` identifier of the mirror plane |

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
| 14 | `u8` pattern role: `0x20` for the seed of an assembly pattern, `0x40` for a copy of one, `0` otherwise |
| 16 | `u32` state word: `2` for a grounded occurrence, `4` for an occurrence that is not grounded |
| 113 | `u32` identifier of the `CLSID_JSite` object of the referenced document |
| 125 | nine `f64`: the rotation `R`, row-major |
| 197 | three `f64`: the translation `t`, in metres |
| 221 | `f64` `1` |

Row `i` of `R` is the direction of the referenced document's axis `i` in the
assembly. A point `p` of the referenced document, as a row vector, is at
`p` `R` + `t` in the assembly. Occurrences of one document name one
`CLSID_JSite` object.

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

The ANSI path is the short (8.3) form of the path where a component of the
long path exceeds eight characters. A Unicode extension follows the word
`0xDEAD`:

| Offset | Field |
| ---: | --- |
| 0 | 20 reserved bytes |
| 20 | `u32` size `s` of the remaining extension, `0` when the moniker stores no Unicode path |
| 24 | `u32` byte count `m` of the Unicode path; `s` is `m` + 6 |
| 28 | `u16` key value |
| 30 | `m` bytes of UTF-16LE path |

The offsets count from the first byte after `0xDEAD`. The Unicode path is the
long form of the ANSI path. Its last character may be a zero code unit. A
reader states the Unicode path when the extension is present and the ANSI path
otherwise.

A second file moniker of the same layout follows in the stream. Its up-level
count is `1` and its path is relative to the folder of the assembly
document. It carries the Unicode extension by the same rule.

### 9.4 Occurrence structure list

The stream `Preview/Structure/IOT` lists the occurrences that take part in
the assembly. A 20-byte header holds a `u32` item count `n` at offset 16. `n`
items follow and end at the stream end. An item is an entry or a node. An
entry is 20 bytes:

| Offset | Field |
| ---: | --- |
| 0 | `u32` flags: `0x40000001` for a shown occurrence; bit `0x2` is set for a hidden occurrence |
| 4 | `u32` detail; bit `0x10000` is set where a property list follows the entry |
| 8 | `u32` identifier of the `CLSID_JAsmReference` object (§9.2) |
| 12 | `u32` `0x80000027` or `0x8000007E` |
| 16 | `u32` |

A property list is a `u32` count `p` and `p` pairs of two `u32`.
The second value of a pair is `0`.

An occurrence of a subassembly is a node. A node has a 12-byte head:

| Offset | Field |
| ---: | --- |
| 0 | `u32` `0x80000000` |
| 4 | `u32` detail; bit `0x10000` is set where a property list follows the head |
| 8 | `u32` identifier of the `CLSID_JAsmReference` object (§9.2) |

The property list, where present, and a `u32` member count `m` follow the
head. `m` items follow, each an entry or a node. The members state objects of
the referenced document. A node states no flags. A stream with only entries
that have no property list is 20 + 20·`n` bytes long.

An occurrence that the references collection (§9.1) lists and the stream
does not list among its `n` items is suppressed. Its `CLSID_JAsmReference`
object and the references collection are the same as for an active
occurrence. A hidden occurrence keeps its entry.

### 9.5 Assembly patterns

A pattern of an occurrence along a layout sketch is a `CLSID_JAsmPattern`
object. Its copies are `CLSID_JAsmReference` objects of the same document
(§9.2) that the references collection (§9.1) lists with the seed. A pattern
object stores:

| Offset | Field |
| ---: | --- |
| 16 | `u32` identifier of the seed `CLSID_JAsmReference` object |
| 20 | `u32` identifier of the `CLSID_JPattern2d` object of the layout sketch |

The `CLSID_JPattern2d` object states the counts, the spacings and the sketch
geometry as for a part pattern (§8.1), with `E` the offset 84 bytes after the
zero byte that ends the ASCII text `JPattern2dGeomsFunction` in the object.
The profile that lists the object as a member states the layout frame. The pattern object ends with the copy list:
a `u32` count `m`, one byte, `m` items and 12 bytes. An item is the `u32`
identifier of a `CLSID_JAsmPatternOcc` object, the byte `0x10`, the
identifier in decimal ASCII and a zero byte.

A `CLSID_JAsmPatternOcc` object stores:

| Offset | Field |
| ---: | --- |
| 12 | `u32` index of the copy along the second run |
| 16 | `u32` index of the copy along the first run; for a circular pattern, the index of the copy |
| 24 | `u32` identifier of the seed `CLSID_JAsmReference` object |
| 36 | `u32` identifier of the `CLSID_JAsmReference` object of the copy |

The copy object stores the placement of the copy as an occurrence does
(§9.2). The seed has index `0` in every run. A pattern can list a copy whose
indices repeat those of an earlier copy of the pattern, and the document can
hold a `CLSID_JAsmReference` object with role `0x40` that no
`CLSID_JAsmPatternOcc` object of a pattern names. The references collection
(§9.1) and the structure list (§9.4) list these objects too. Such an object
and the reference of a repeated copy are not occurrences. The occurrences of
a pattern are its seed and the references of its copies that are the first of
their index.

### 9.6 Mirrored occurrences

A mirrored component is an occurrence (§9.2) whose rotation has determinant
`+1`; the stored placement is not a reflection. A `CLSID_JAsmGroup` object
stores the identifier of that occurrence as a `u32` at object offset 17.
does not list is suppressed. Its `CLSID_JAsmReference` object and the
references collection are the same as for an active occurrence. A hidden
occurrence keeps its entry.

## 10. Kernel stream map

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

## 11. Transmit envelope

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

## 12. Geometry

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

### 12.1 Body role

A BODY owns one ATTRIBUTE whose definition is named `MONIKER/BODY_ID_ATTRIB`.
The `identifier` pointer of the ATTRIB_DEF node names a KEY node, and the
characters of that KEY node are the definition name. A CHAR_VALUES node in the
`fields` list of the ATTRIBUTE holds the role string. The role string is ASCII
`SE `, a build number, one space, `Type`, and a decimal role number. Role
number 86 marks a design body. Role number 96 marks a construction body.

The role number does not depend on the body type. A construction body is a
solid body (type 1) when its faces enclose a volume, a sheet body (type 3)
when they do not, and a wire body (type 2) when it holds only edges. Converting
a closed construction body to a design body changes the role number from 96 to
86 and leaves the body type 1.

The feature that creates a construction body is an object of class
`CLSID_JMConstruction`. The object stays in the document when the body is
converted to a design body, and a `JMBodyStateToggleAssocFunction` feature is
added. The class of the object does not state the role of the body.

## 13. Document version declarations

The root streams `Versions`, `DocVersion2`, and `BuildVersions` carry document
version declarations. These declarations and the kernel schema token occupy
separate streams. The kernel token declares the embedded node grammar.
