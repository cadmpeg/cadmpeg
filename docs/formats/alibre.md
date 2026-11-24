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

The model unit is the centimetre, and the header `scale` is `10.0`, in millimetres per unit. The decoder multiplies every model-space length by 10 to obtain millimetres, as it does for every ACIS binary stream. The resolved tolerance `resabs` is multiplied by 10 in the same way.

### 2.2 Key string

A true flag (`0x0a`) and one short string (`0x07`, one length byte, the bytes) follow the three header doubles. The string is an opaque key. The record stream starts after it.

### 2.3 Identifier table

An identifier in a record name chain or in a payload carries a table suffix. The first use of a name is `name%<u32>`: the name, the byte `0x25`, and a little-endian `u32` that is the table index. A later use is `%<u32>` with an empty name and the index of the first use. The suffix is part of the identifier length byte. The decoder resolves every identifier to its name before it frames the record.

A payload string can end with `%<digits>` as ASCII text. The decoder keeps such a string unchanged.

### 2.4 Entity base

Every topology and geometry record starts with four fields: the attribute reference, the entity id integer, one further integer, and the pattern reference. The decoder removes the further integer, which gives the three-field base of [`asm.md`](asm.md) §5.1 and §5.2, and reads the class fields that follow at the offsets of that section. A `body` record stores one integer between the pattern reference and the first-lump reference, and the decoder removes that integer too. Attribute records (`name_attrib`, `TimeStamp`, `StateID`, `Autograph`) keep their stored form.

The `coedge` record stores no integer between the loop reference and the pcurve reference, and the `vertex` record stores no endpoint-index integer. These two forms are the forms of [`asm.md`](asm.md) §7.4, which the decoders of `coedge_pcurve_ref` and `vertex_point_ref` accept.

### 2.5 Records

The stream holds the records `body`, `lump`, `shell`, `face`, `loop`, `coedge`, `edge`, `vertex`, `point`, and the surface and curve classes `plane`, `cone`, `torus`, `straight`, `ellipse`, and `spline` with its subtypes. Attribute records of the classes `name_attrib`, `TimeStamp`, `StateID`, `Autograph`, and `alibrePartClosedBody` hang from entities through the attribute reference. The `alibrePartClosedBody` name appears as the string of a `name_attrib` record.

An `edge` record stores the continuity string `unknown` or `tangent`. A face on a lofted solid rests on a `spline` surface whose subtype is `skinsur`; the record ends with a solved `nubs` surface block, which the decoder reads as the face carrier.

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

## 5. Assemblies (`.AD_ASM`)

An Alibre assembly is a compound file of the same kind as a part. It holds no `SAB_DATA` stream and no geometry: it places component files, each a part or another assembly, and the components hold the bodies.

### 5.1 Streams

| Stream | Content |
| --- | --- |
| `STEP_DATA` | The assembly as STEP Part 21 text in the Alibre schema (§5.2). |
| `CONSTITUENTS` | The component references as framed text records (§5.3). |
| `PROP_DATA` | Document properties. |
| `VERSION` | An XML element with the repository identity, followed by a path and identifiers. |
| `GLOBAL_PARAMETER_DATA_CACHE` | 20 bytes, all zero in the specimens. |
| `LIGHT_CONFIGURATION_DATA` | 48 bytes. |

The decoder detects an assembly from the streams `CONSTITUENTS` and `STEP_DATA` and the absence of `SAB_DATA`, and reports the dialect `alibre:cfb-assembly`, admitted, with no kernel layer.

### 5.2 Assembly entities in `STEP_DATA`

The stream follows §4. Lengths are in centimetres, the API unit of Alibre; the decoder multiplies them by 10 and states millimetres. Attribute positions count from 0.

| Entity | Attributes |
| --- | --- |
| `DESIGNASSEMBLY` | 1: the origin datums; 18: `PRODUCT`; 20: the list of `DESIGNINCLUSION`, one for each placed occurrence, in placement order; 21 to 23: lists that are empty when the assembly has no relation; 25: the list of `DESIGNASSEMBLYCONFIGURATION`; 26: the active configuration. |
| `PRODUCT` | 1: the product name. |
| `DESIGNINCLUSION` | 1: the list of `DESIGNMEMBER`; 2: the component path with forward slashes and no extension; 3: `REPOSITORYVERSIONREFERENCE`; 5: `CARTESIAN_TRANSFORMATION_OPERATOR_3D`; 7: a logical that is true for an anchored occurrence; 9: the occurrence GUID; 11: a `PROFILE`; 14: the name of the component configuration that the occurrence uses. Attributes 0, 4, 6, 8, 10, 12, and 13 hold the empty list, null, false, or 0 in every specimen. |
| `REPOSITORYVERSIONREFERENCE` | 1: the drive letter; 2: the path without extension; 3: the GUID of the component document. |
| `DESIGNMEMBER` | 3: a logical that is true for a hidden member; 5: the member GUID; 6: the GUID of the member of the subassembly that this member names, or null for a member of the inclusion itself. |
| `CARTESIAN_TRANSFORMATION_OPERATOR_3D` | 3: `DIRECTION` of the image of the x axis; 4: `DIRECTION` of the image of the y axis; 5: `CARTESIAN_POINT` of the local origin; 6: the scale; 7: `DIRECTION` of the image of the z axis. Attributes 0 to 2 are null. |
| `DESIGNASSEMBLYCONFIGURATION` | 0: the configuration name; 8: the list of suppressed `DESIGNINCLUSION`; 9: the list of hidden `DESIGNMEMBER`. |

The placement of an occurrence maps a point of the component by the three axis images, the scale, and the origin. The specimens place components with identity rotations. The occurrence name that Alibre shows, `asm_box<2>`, is not stored: it follows from the component name and the order of the inclusions of that component.

The configuration name `Config<1>` of an inclusion names a configuration of the component, not of the assembly.

### 5.3 The `CONSTITUENTS` stream

The stream holds one record for each distinct component. A record carries the repository element `<Repository Guid="…" Name="C" V3Name="" Type="File"/>`, a colon, the component path without extension, a bar, the component document GUID, and integers separated by bars, and then a second copy of the same reference. The records repeat what the `REPOSITORYVERSIONREFERENCE` entities state. The decoder does not read the stream. The framing bytes before each record are not decoded (ALB-10).

### 5.4 Decode

The decoder reads one occurrence for each `DESIGNINCLUSION`, in list order:

- The occurrence places the product definition of the component. Two inclusions of one component file place one definition, and the file is decoded once.
- The transform is the rotation, scale, and translation of the operator, in millimetres.
- The occurrence is suppressed when the active configuration lists its inclusion. It is not visible when one of its own members is hidden by the member flag or by the active configuration.
- An anchored inclusion becomes a grounded joint at the occurrence.
- The occurrence name is the file name of the path, an index starting at 1 for each component, in angle brackets.

The decoder resolves a component by the file name of its path, with the extension `AD_PRT` and then `AD_ASM`, in the directory of the assembly, through the companion file source of the decode context. The drive and the directories of the stored path are not read. A part decodes as §3 states, and a subassembly decodes by this section, recursively. The decoder places the identities of each component file under the scope `alibre:xref/m<n>/`, so that two files do not share an identity. A component that is not available, does not decode, lies past the nesting limit, or is a file that is already being decoded stays an external product definition with the path and the document GUID of the inclusion.

A decode reports these losses, each with the code `alibre/<code>`:

| Code | Meaning |
| --- | --- |
| `assembly.member-unavailable` | A component file is not beside the assembly. |
| `assembly.member-undecoded` | A component file is available and does not decode. |
| `assembly.member-cycle` | A component file contains the assembly that places it. |
| `assembly.expansion-truncated` | A subassembly lies past the nesting limit, or occurrences lie past the occurrence limit. |
| `assembly.member-design-unmerged` | A component carries design records that the merge leaves out. |
| `assembly.placement-unreadable` | An inclusion has no readable placement. |
| `assembly.inclusion-field-unmapped` | An inclusion states an attribute that the occurrence does not carry. |
| `assembly.nested-member-state-unread` | An inclusion hides a component below its subassembly. |
| `assembly.configuration-unmapped` | An occurrence names a component configuration, or the assembly holds more than one configuration. The occurrences carry the state of the active configuration only. |
| `assembly.member-dialect-unverified` | A component file declares an ACIS save format outside the verified bands, as `alibre/source.kernel-dialect-unverified` states for a part. The assembly has no kernel layer of its own. |
| `assembly.entities-unread` | The stream holds an entity name that the reader does not know, or the relation lists of the assembly are not empty. |
| `source.streams-undecoded` | `CONSTITUENTS`, `PROP_DATA`, `VERSION`, and the two cache streams carry no decoded content. |

The decoder does not read assembly constraints, the configurations other than the active one, the configuration of the component that an inclusion uses, or flexible subassemblies (ALB-09 to ALB-12).
