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

A part holds the streams `SAB_DATA` and `STEP_DATA`. An assembly file holds `STEP_DATA` and a `CONSTITUENTS` stream and holds no `SAB_DATA` stream. The decoder detects a part from the two stream names and reads only `SAB_DATA`.

## 2. The `SAB_DATA` stream

The stream is an ACIS binary stream as [`asm.md`](asm.md) §1 defines for the `ACIS BinaryFile` magic. The four header words are the save-format version (`3300` for ACIS 33.0), the record count, the entity count, and the flags. The three header strings are the product family, the product version string, and the save date. The three header doubles are `scale`, `resabs`, and `resnor`.

### 2.1 Length unit

The model unit is the centimetre, and the header `scale` is `10.0`, in millimetres per unit, in every committed part. The shared ACIS binary reader reads every model-space length as centimetres and multiplies it by 10 to obtain millimetres; the decoder does not take the factor from the header. A header that states any other scale is charged as `alibre/header.scale-unverified` (blocking), because the lengths would then be off by the ratio. The resolved tolerance `resabs` is multiplied by 10 in the same way.

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

An `edge` record stores the continuity string `unknown` or `tangent`. A side face of a lofted solid rests on a `spline` surface whose subtype is `skinsur`; the record ends with a solved `nubs` surface block, which the decoder reads as the face carrier. The two cap faces of the loft rest on `plane` surfaces. The edges between a lofted face and a cap face have `intcurve` curves, and the coedges of a lofted solid reference `pcurve` records. The decoder reads none of these as a pcurve entity and reports `alibre/geometry.pcurve-undecoded` with the count (ALB-01).

## 3. Decode

The decoder reports the primary dialect `alibre:cfb-acis-binary` and the kernel dialect `acis:save-format-binary-other`. The save format 33 is outside the verified Spatial ACIS bands, so the kernel layer is unverified and decode reports `alibre/source.kernel-dialect-unverified`. Every stream other than `SAB_DATA` is reported by `alibre/source.streams-undecoded`.

A `SAB_DATA` stream without the ACIS binary magic or without a record-stream boundary classifies as `alibre:unknown`, and decode refuses it as an unsupported dialect.

## 4. The `STEP_DATA` stream

The stream is STEP Part 21 text with the schema names `ALIBRE_SCHEMA` and `CONFIG_CONTROL_DESIGN`. It describes the feature model and holds no faces. The entity families are:

- Product structure: `PRODUCT`, `PRODUCT_DEFINITION`, `DESIGNPARTCONFIGURATION`, `DESIGNWORKSPACE`.
- Feature history: `DESIGNSTATE` (one for each origin, axis, plane, sketch, and feature, with an index), `DESIGNPARTSTATECONTEXT`, `EXTRUDESKETCHOPERATION`, `DEFINESKETCHOPERATION`, `BLINDENDCONDITION`.
- Sketches: `DESIGNSKETCH`, `BLOCKDEFINITION`, `BLOCKPRIMITIVE`, `LINE`, `REFERENCEPOINTUV`, `REFERENCEAXISUV`, `CONSTRAINTPAIRUV`, `CONSTRAINTCONTEXTUV`.
- Parameters and dimensions: `DOUBLEPARAMETER`, `PARAMETERTABLE`, `MEASUREMENT`, `LINEARDIMENSION`, `DESIGNDIMENSION`.
- Document profile: `PROFILE` with `INTEGERVALUEPAIR`, `REALVALUEPAIR`, `STRINGVALUEPAIR`, `BOOLEANVALUEPAIR`, and `MEASUREMENTFORMAT` entities.

The decoder does not read this stream.
