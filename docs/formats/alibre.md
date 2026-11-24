# Alibre Design part (`.AD_PRT`): Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

An Alibre Design part file is a Microsoft Compound File (CFB). The part body is an ACIS binary stream in the stream `SAB_DATA`. The decoder reads the part geometry from that stream through the shared ASM/ACIS decoders described in [`asm.md`](asm.md). Unknown fields are in [`alibre-open-items.md`](alibre-open-items.md).

## 1. Container

The file is a CFB version 3 file with the signature `D0 CF 11 E0 A1 B1 1A E1`. The directory has a flat root: every entry is a stream, and no storage entry exists below the root. The streams carry no OLE property set.

| Stream | Content |
| --- | --- |
| `SAB_DATA` | The part body as an ACIS binary stream (§2). |
| `STEP_DATA` | The Alibre feature model as STEP Part 21 text (§4). |
| `PROP_DATA` | Document properties. |
| `VERSION` | An XML element with the repository identity, followed by a path and identifiers. |
| `AUTOGRAPH_DATA` | Text records of the form `#<n>{<values>}`. |
| `DESIGN_PARAMETER_DATA` | A short binary record. |
| `HOLE_OPERATIONS_DATA` | A short binary record. |
| `EXTERNAL_THREAD_OPERATIONS_DATA` | A short binary record. |
| `CUSTOM_ITEM_PROP_DATA` | A short binary record. |
| `STANDARD_DESIGN_PROP_DATA` | A short binary record. |
| `GLOBAL_PARAMETER_DATA_CACHE` | A short binary record. |
| `LIGHT_CONFIGURATION_DATA` | A short binary record. |

A part holds the streams `SAB_DATA` and `STEP_DATA`. An assembly file holds `STEP_DATA` and a `CONSTITUENTS` stream and holds no `SAB_DATA` stream. The decoder detects a part from the two stream names and reads `SAB_DATA` for the body and `STEP_DATA` for the feature history.

## 2. The `SAB_DATA` stream

The stream is an ACIS binary stream as [`asm.md`](asm.md) §1 defines for the `ACIS BinaryFile` magic. The four header words are the save-format version (`3300` for ACIS 33.0), the record count, the entity count, and the flags. The three header strings are the product family, the product version string, and the save date. The three header doubles are `scale`, `resabs`, and `resnor`.

### 2.1 Length unit

The model unit is the centimetre, and the header `scale` is `10.0`, in millimetres per unit. The decoder multiplies every model-space length by 10 to obtain millimetres, as it does for every ACIS binary stream. The resolved tolerance `resabs` is multiplied by 10 in the same way.

### 2.2 Key string

A true flag (`0x0a`) and one short string (`0x07`, one length byte, the bytes) follow the three header doubles. The string is an opaque key. The record stream starts after it.

### 2.3 Identifier table

An identifier in a record name chain or in a payload carries a table suffix. The first use of a name is `name%<u32>`: the name, the byte `0x25`, and a little-endian `u32` that is the table index. A later use is `%<u32>` with an empty name and the index of the first use. The suffix is part of the identifier length byte. The four index bytes can have any value, and they need not be valid UTF-8 (the index 128 is the bytes `80 00 00 00`). Only the name before the `%` must be UTF-8. The decoder resolves every identifier to its name before it frames the record.

A payload string can end with `%<digits>` as ASCII text. The decoder keeps such a string unchanged.

### 2.4 Entity base

Every topology and geometry record starts with four fields: the attribute reference, the entity id integer, one further integer, and the pattern reference. The decoder removes the further integer, which gives the three-field base of [`asm.md`](asm.md) §5.1 and §5.2, and reads the class fields that follow at the offsets of that section. A `body` record stores one integer between the pattern reference and the first-lump reference, and the decoder removes that integer too. The decoder does not change an attribute record, which is a record whose name ends in `attrib` (`name_attrib-gen-attrib`, `TimeStamp-Alibre-attrib`, `StateID-Alibre-attrib`, `Autograph-Alibre-attrib`, `tag-sys-attrib`); it keeps its stored form. The decoder does not interpret the further integer and the `body` integer. When it removes either one, it reports `alibre/source.record-extensions-unprojected` once for the stream, with the number of records it changed.

The `coedge` record stores no integer between the loop reference and the pcurve reference, and the `vertex` record stores no endpoint-index integer. These two forms are the forms of [`asm.md`](asm.md) §7.4, which the decoders of `coedge_pcurve_ref` and `vertex_point_ref` accept.

### 2.5 Records

The stream holds the records `body`, `lump`, `shell`, `face`, `loop`, `coedge`, `edge`, `vertex`, `point`, and the surface and curve classes `plane`, `cone`, `torus`, `straight`, `ellipse`, `intcurve`, `pcurve`, and `spline` with its subtypes. Attribute records of the classes `name_attrib`, `TimeStamp`, `StateID`, `Autograph`, and `tag-sys` hang from entities through the attribute reference. The name chains of these records are `name_attrib-gen-attrib`, `TimeStamp-Alibre-attrib`, `StateID-Alibre-attrib`, `Autograph-Alibre-attrib`, and `tag-sys-attrib`. The `name_attrib-gen-attrib` record of the body holds the string `alibrePartClosedBody%5`. This string is a payload string and not an identifier, so it keeps its `%5` ending (§2.3).

An `edge` record stores the continuity string `unknown` or `tangent`. A side face of a lofted solid rests on a `spline` surface whose subtype is `skinsur`; the record ends with a solved `nubs` surface block, which the decoder reads as the face carrier. The two cap faces of the loft rest on `plane` surfaces. The loft has four `intcurve` edge curves, and its coedges reference 16 `pcurve` records. The decoder reads none of them as a pcurve entity and reports `alibre/geometry.pcurve-undecoded` (ALB-01).

## 3. Decode

The decoder reports the primary dialect `alibre:cfb-acis-binary` and the kernel dialect `acis:save-format-binary-other`. The save format 33 is outside the verified Spatial ACIS bands, so the kernel layer is unverified and decode reports `alibre/source.kernel-dialect-unverified`. Every stream other than `SAB_DATA` and `STEP_DATA` is reported by `alibre/source.streams-undecoded`.

A `SAB_DATA` stream without the ACIS binary magic or without a record-stream boundary classifies as `alibre:unknown`, and decode refuses it as an unsupported dialect.

## 4. The `STEP_DATA` stream

The stream is STEP Part 21 text with the schema names `ALIBRE_SCHEMA` and `CONFIG_CONTROL_DESIGN`. It describes the feature model and holds no faces. The `DATA` section holds simple entity instances `#n=NAME(arguments);`. A parameter is `$` (null), `*` (omitted), an integer, a real, a quoted string, an enumeration or logical `.X.`, a reference `#n`, a parenthesized list, or a typed parameter `NAME(arguments)`. Attribute positions below count from 0. The model unit is the centimetre, and an angle is in radians.

The entity families are:

- Product structure: `PRODUCT`, `PRODUCT_DEFINITION`, `DESIGNPARTCONFIGURATION`, `DESIGNWORKSPACE`, `DESIGNPART`.
- Feature history: `DESIGNSTATE`, `DESIGNPARTSTATECONTEXT`, the operation entities of §4.2, and the end-condition entities of §4.5.
- Sketches: `DESIGNSKETCH`, `BLOCKDEFINITION`, `BLOCKPRIMITIVE`, `LINE`, `CIRCLE`, `NODEUV`, `REFERENCEPOINTUV`, `REFERENCEAXISUV`, `REFERENCECIRCLEUV`, `REFERENCEDEPENDENTUV`, `CONSTRAINTPAIRUV`, `CONSTRAINTCONTEXTUV`.
- Parameters and dimensions: `DOUBLEPARAMETER`, `ANGLEPARAMETER`, `ANGLEUV`, `PARAMETERTABLE`, `MEASUREMENT`, `LINEARDIMENSION`, `DESIGNDIMENSION`, `DIMENSIONANNOTATION`.
- Document profile: `PROFILE` with `INTEGERVALUEPAIR`, `REALVALUEPAIR`, `STRINGVALUEPAIR`, `BOOLEANVALUEPAIR`, `MEASUREMENTFORMATPAIR`, `FONTDESCRIPTIONPAIR`, and `HATCHPATTERNPAIR` entities.

### 4.1 Geometry entities

| Entity | Attributes |
|---|---|
| `CARTESIAN_POINT` | 1: list of 2 or 3 coordinates. |
| `DIRECTION` | 1: list of 2 or 3 components. |
| `VECTOR` | 1: `DIRECTION`; 2: length. |
| `AXIS1_PLACEMENT` | 1: `CARTESIAN_POINT` origin; 2: `DIRECTION` axis. |
| `AXIS2_PLACEMENT_3D` | 1: origin; 2: `DIRECTION` of the z axis; 3: `DIRECTION` of the x axis. |
| `AXIS2_PLACEMENT_2D` | 1: 2D origin. |
| `PLANE` | 1: `AXIS2_PLACEMENT_3D`. |
| `LINE` | 1: 2D start point; 2: `VECTOR` whose direction is a 2D unit direction. |
| `CIRCLE` | 1: `AXIS2_PLACEMENT_2D` of the centre; 2: radius. |
| `NODEUV` | 1: 2D point of a free sketch point. |

The y axis of a placement is the cross product of the z axis and the x axis. A `LINE` ends at its start point plus the direction scaled by the vector length. Sketch coordinates are in the frame of the sketch plane.

### 4.2 States and operations

`DESIGNPARTSTATECONTEXT` attribute 0 lists the `DESIGNSTATE` entities in feature order, and attribute 1 references the last state. A `DESIGNSTATE` holds nine attributes: the name (0), the index (1), the operation entity (2), a logical (3), and attributes 4 to 8, whose meaning ALB-06 records as unknown. The decoder reads attributes 0 to 2. The index is 0 for the states of the origin, the principal axes, the principal planes, and sketches. A state of a modeling feature or a user datum holds its 1-based position among such states. The decoder does not read the index: the feature order is the order of the state list.

| Operation entity | Attributes |
|---|---|
| `DESIGNPOINTOPERATION` | 1: `DESIGNPRIMARYPOINT` (name 1, `CARTESIAN_POINT` 2). |
| `DESIGNAXISOPERATION` | 1: `DESIGNPRIMARYAXIS` (name 1, `AXIS1_PLACEMENT` 2). |
| `DESIGNPLANEOPERATION` | 1: `DESIGNPRIMARYPLANE` (name 1, `PLANE` 2) or `DESIGNASSOCIATEDPLANE` (name 1, `PLANE` 2, `OFFSETPLANEPRODUCER` 3). |
| `DEFINESKETCHOPERATION` | 1: `DESIGNSKETCH`. |
| `EXTRUDESKETCHOPERATION` | 0: list of `DESIGNDIMENSION`; 2: `DESIGNSKETCH`; 3: `BLINDENDCONDITION`. |
| `REVOLVESKETCHOPERATION` | 0: list of `DESIGNDIMENSION`; 2: `DESIGNSKETCH`; 3: `OPERATIONTARGETPROXY` of a `DESIGNPRIMARYAXIS`; 7: `AXIS1_PLACEMENT` of the revolution axis; 8: `BLINDANGLEENDCONDITION`. |
| `FILLETOPERATION` | 1: list of `OPERATIONTARGETPROXY`; 2: radius `DOUBLEPARAMETER`. |
| `HOLEOPERATION` | 2: `DESIGNSKETCH` whose point primitives are the hole centres; 3: `SIMPLEHOLE`. |
| `LOFTOPERATION` | 2: ordered list of the `DESIGNSKETCH` entities of the sections. |

`OFFSETPLANEPRODUCER` holds an `OPERATIONTARGETPROXY` of the base plane (0) and the offset `DOUBLEPARAMETER` (1). The plane of an associated plane lies at the offset along the normal of the base plane. `OPERATIONTARGETPROXY` attribute 1 references the target: a `DESIGNPRIMARYPLANE`, a `DESIGNASSOCIATEDPLANE`, a `DESIGNPRIMARYAXIS`, or a `TOPOLOGYSIGNATURE`.

### 4.3 Sketches

`DESIGNSKETCH` holds the name (0), the `OPERATIONTARGETPROXY` of the support (1), the `BLOCKDEFINITION` (2), and the `PLANE` of the sketch frame (3). A support that references a plane datum places the sketch on that plane. A support that references a `TOPOLOGYSIGNATURE` places the sketch on a face of the body.

`BLOCKDEFINITION` holds the `BLOCKPRIMITIVE` list (1), the `CONSTRAINTCONTEXTUV` (8), the x axis reference (9), and the y axis reference (10), each a `REFERENCEAXISUV` with the first logical true. `BLOCKPRIMITIVE` holds the list of reference entities of the primitive (0), the geometry entity (1), and the ordinal (3).

The reference entities of a primitive are:

- `REFERENCEPOINTUV`: logical (0) true for the sketch origin; point (2); list of `REFERENCEDEPENDENTUV` (3).
- `REFERENCEAXISUV`: logical (0) true for a sketch axis; point (2); `VECTOR` (3). A sketch axis starts at the unit point of the axis and ends at the origin.
- `REFERENCECIRCLEUV`: logical (0); `CIRCLE` (2); list of `REFERENCEDEPENDENTUV` (3).

`REFERENCEDEPENDENTUV` holds a geometry entity (0) and an index (1). Index 0 of a `LINE` is its start point, index 1 is its end point, and index 0 of a `CIRCLE` is its centre. A `REFERENCEPOINTUV` that depends on a `LINE` or `CIRCLE` bounds that primitive with the role of the index.

`CONSTRAINTPAIRUV` holds a pair of reference entities (0), the context (1), and a kind (2). The kinds are:

| Kind | Pair | Meaning |
|---|---|---|
| 6 | two `REFERENCEPOINTUV` | The two points coincide. |
| 6 | `REFERENCEAXISUV` of a line, `REFERENCEPOINTUV` of an end point of that line | The point is an end point of the line. |
| 2 | `REFERENCECIRCLEUV`, `REFERENCEPOINTUV` of its centre | The point is the centre of the circle. |
| 4 | sketch axis, `REFERENCEAXISUV` of a line | The line is parallel to the sketch axis. |

### 4.4 Parameters

`DOUBLEPARAMETER` holds the name (0), a GUID or null (2), and the value (11) in centimetres. `ANGLEPARAMETER` holds the name (0), a GUID or null (2), and the `ANGLEUV` of the value (11), whose attribute 0 is the angle in radians. `PARAMETERTABLE` attribute 0 lists the parameters of the part. A parameter name starts with `D` for a distance, `R` for a radius, and `A` for an angle.

### 4.5 End conditions and holes

| Entity | Attributes |
|---|---|
| `BLINDENDCONDITION` | 2: logical, false for the direction of the sketch normal; 3: depth `DOUBLEPARAMETER`. |
| `BLINDANGLEENDCONDITION` | 1: angle `ANGLEPARAMETER`. |
| `THROUGHALLENDCONDITION` | The hole continues through the body. |
| `SIMPLEHOLE` | 1: list with the diameter `DOUBLEPARAMETER`; 2: end condition. |

An extrusion starts on the sketch plane and ends at the depth along the sketch normal. A revolution turns the sketch about the revolution axis through the angle. A loft joins its sections in list order. A fillet applies the radius to the edges that its targets name.

### 4.6 Decode

The decoder reads one feature for each state in state order and maps it to the design layer of the IR: the origin, axes, and planes to datums, an associated plane to an offset plane, a sketch to a placed sketch with its line, circle, and point entities, and the extrusion, revolution, fillet, hole, and loft to their operations. A parameter is a design parameter in millimetres or radians, and an operation refers to it by dimension. The decoder converts centimetres to millimetres. A hole takes every point primitive of its sketch as a centre. A parameter GUID is the source setting `alibre:guid` of the design parameter. The decode reports `alibre/history.edge-selection-unresolved` for the topology signatures of a fillet, `alibre/history.face-selection-unresolved` for a sketch on a face, `alibre/history.dimension-annotation-unmapped` for the dimension list of an extrusion or revolution, `alibre/history.direction-flag-assumed` for the logical of a blind end condition, and the other `alibre/history.*` codes for records that no feature maps. The remaining entities of the document profile are in `alibre/history.presentation-unmapped`. A `STEP_DATA` stream that does not read as Part 21 or as a feature history, or whose history does not project, leaves the body intact and reports `alibre/history.stream-unread`; a resource limit still refuses the decode.
