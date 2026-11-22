# Solid Edge compound documents

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

## 1. Container

Solid Edge `.par`, `.psm`, and `.asm` documents use Microsoft Compound File
Binary. The compound directory stores UTF-16LE entry names and the storage
hierarchy. A stream entry identifies its logical byte length and allocation
chain. Regular streams use the FAT. Small streams use the mini FAT and the
root storage's mini stream.

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

Chamfer type `41` has one setback on both faces. Type `43` has two setbacks.
Type `42` has a setback and an angle.

## 8. Patterns and mirrors

### 8.1 Pattern features

A pattern is a `CLSID_JMGroup` entry of the navigator (§4.1). The `u32` at
object offset 106 of the group object is the identifier of a `CLSID_JMPart`
object with function `JMFastPatternFunction`. The `u32` at object offset
337 is the identifier of the seed feature. The profile of the
`JMFastPatternFunction` object (§5.3) has one `CLSID_JPattern2d` member.

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

A mirror is a `CLSID_JMGroup` entry of the navigator whose `u32` at object
offset 92 is the seed count `c`. `c` seed feature identifiers follow from
offset 96. For `c` = 1, the `u32` at object offset 329 is the identifier of
the `ReferencePlaneObject` about which the seed is mirrored.

