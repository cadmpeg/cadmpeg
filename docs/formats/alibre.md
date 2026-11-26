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

The model unit is the centimetre: the header `scale` is `10.0` millimetres per unit, and a part's STEP export states the same unit as `SI_UNIT(.CENTI.,.METRE.)`. The shared ACIS binary reader reads every model-space length as centimetres and multiplies it by 10 to obtain millimetres; the decoder does not take the factor from the header. A header that states any other scale is charged as `alibre/header.scale-unverified` (blocking), because the lengths would then be off by the ratio. The resolved tolerance `resabs` is multiplied by 10 in the same way.

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

## 5. Assemblies (`.AD_ASM`)

An Alibre assembly is a compound file of the same kind as a part. It holds no `SAB_DATA` stream and no geometry: it places component files, each a part or another assembly, and the components hold the bodies.

### 5.1 Streams

| Stream | Content |
| --- | --- |
| `STEP_DATA` | The assembly as STEP Part 21 text in the Alibre schema (§5.2). |
| `CONSTITUENTS` | The component references as framed text records (§5.3). |
| `PROP_DATA` | Document properties. |
| `VERSION` | An XML element with the repository identity, followed by a path and identifiers. |
| `GLOBAL_PARAMETER_DATA_CACHE` | 20 bytes of zero. |
| `LIGHT_CONFIGURATION_DATA` | 48 bytes. |

The decoder detects an assembly from the streams `CONSTITUENTS` and `STEP_DATA` and the absence of `SAB_DATA`, and reports the dialect `alibre:cfb-assembly`, admitted, with no kernel layer.

### 5.2 Assembly entities in `STEP_DATA`

The stream follows §4. Lengths are in centimetres, the API unit of Alibre; the decoder multiplies them by 10 and states millimetres. Attribute positions count from 0.

| Entity | Attributes |
| --- | --- |
| `DESIGNASSEMBLY` | 1: the origin datums; 18: `PRODUCT`; 20: the list of `DESIGNINCLUSION`, one for each placed occurrence, in placement order; 21 to 23: lists that are empty when the assembly has no relation; 25: the list of `DESIGNASSEMBLYCONFIGURATION`; 26: the active configuration. |
| `PRODUCT` | 1: the product name. |
| `DESIGNINCLUSION` | 1: the list of `DESIGNMEMBER`; 2: the component path with forward slashes and no extension; 3: `REPOSITORYVERSIONREFERENCE`; 5: `CARTESIAN_TRANSFORMATION_OPERATOR_3D`; 7: a logical that is true for an anchored occurrence; 9: the occurrence GUID, distinct for every inclusion; 11: a `PROFILE` that lists nothing; 14: the name of the component configuration that the occurrence uses. Attribute 0 holds the empty list; attributes 4, 6, 8, and 13 hold null; attribute 10 holds false; attribute 12 holds 0. |
| `REPOSITORYVERSIONREFERENCE` | 1: the drive letter; 2: the path without extension; 3: the GUID of the component document. |
| `DESIGNMEMBER` | 3: a logical that is true for a hidden member; 5: the member GUID; 6: the GUID of the member of the subassembly that this member names, or null for a member of the inclusion itself. |
| `CARTESIAN_TRANSFORMATION_OPERATOR_3D` | 3: `DIRECTION` of the image of the x axis; 4: `DIRECTION` of the image of the y axis; 5: `CARTESIAN_POINT` of the local origin; 6: the scale; 7: `DIRECTION` of the image of the z axis. Attributes 0 to 2 are null. |
| `DESIGNASSEMBLYCONFIGURATION` | 0: the configuration name; 8: the list of suppressed `DESIGNINCLUSION`; 9: the list of hidden `DESIGNMEMBER`. |

The operator maps a point `p` of the component to `origin + p.x * a1 + p.y * a2 + p.z * a3`, where `a1` is attribute 3 (the image of the local x axis), `a2` is attribute 4 (the image of the local y axis), and `a3` is attribute 7 (the image of the local z axis). The three images are the columns of the rotation that acts on a column vector. The row-major matrix that the Alibre API returns from `WorldTransform.Array()` acts on a row vector (`point' = point * M`) and holds the translation in its last row, so its row `i` is the image of local axis `i`. A +90 degree turn about +Z at (40, 0, 0) mm stores `a1` = (0, 1, 0), `a2` = (-1, 0, 0), and `a3` = (0, 0, 1): the local x axis goes to world +y. A +90 degree turn about +Y stores `a1` = (0, 0, -1) and `a3` = (1, 0, 0): the local z axis goes to world +x. A +30 degree turn about +X stores `a2` = (0, 0.866025, 0.5) and `a3` = (0, -0.5, 0.866025). A unit-length image carries a rounding error of about 1e-16, as in `6.12303176911189E-17` for cos 90 degrees.

The decoder states a placement only when the operator is a rigid motion: the scale is 1, the three images have length 1 and are orthogonal, and `a3` is the cross product of `a1` and `a2`, all to 1e-6. A scaled, sheared, or mirrored operator, or a value that is not finite, gives an occurrence with no transform and the loss `assembly.placement-unreadable`. The occurrence name that Alibre shows, `<component><n>`, follows from the component name and the order of the inclusions of that component.

The configuration name `Config<1>` of an inclusion names a configuration of the component, not of the assembly.

### 5.3 The `CONSTITUENTS` stream

The stream holds one record for each distinct component. A record carries the repository element `<Repository Guid="…" Name="C" V3Name="" Type="File"/>`, a colon, the component path without extension, a bar, the component document GUID, and integers separated by bars, and then a second copy of the same reference. The records repeat what the `REPOSITORYVERSIONREFERENCE` entities state. The decoder does not read the stream. The framing bytes before each record are not decoded (ALB-10).

### 5.4 Decode

The decoder reads one occurrence for each `DESIGNINCLUSION`, in list order:

- The occurrence places the product definition of the component. Within one assembly, the inclusions of one component file and one document GUID place one definition, and the decoder reads the file once for that assembly. A file that an assembly places at two nesting levels is decoded once for each level.
- The transform is the rotation and translation of the operator, in millimetres (§5.2).
- The occurrence is suppressed when the active configuration lists its inclusion. It is not visible when one of its own members is hidden by the member flag or by the active configuration. An inclusion with no member of its own gives the occurrence no visibility value.
- An anchored inclusion becomes a grounded joint at the occurrence.
- The decoder reads the `Default Units` string of the workspace (`DESIGNWORKSPACE` attribute 0, a `PROFILE` of `STRINGVALUEPAIR` entities; the decoder accepts `cm`) and the origin datums (`DESIGNASSEMBLY` attribute 1): each point that they reach must be the origin. Occurrences are placed in the frame of the assembly, whose datums are that origin.
- The decoder merges a component with the tolerances of the assembly. A component part states its own header tolerance in millimetres, and the assembly keeps the default 1e-6 mm; the decoder reports a difference.
- The occurrence carries the GUID of its inclusion (attribute 9) as its native reference, `alibre:model:inclusion#<GUID>` (`alibre:<key>@model:inclusion#<GUID>` in a subassembly whose file has the definition key `<key>`). An inclusion that states none, an attribute 11 that is not a `PROFILE` or is a `PROFILE` with content, and every other attribute that the occurrence does not carry are `assembly.inclusion-field-unmapped`. The GUID of a member (`DESIGNMEMBER` attribute 5) is not carried; `assembly.member-guid-unmapped` says so once for the assembly.
- The occurrence name is the file name of the path, an index starting at 1 for each component, in angle brackets.

The decoder resolves a component by the file name of its path, in the directory of the assembly, through the companion file source of the decode context. It tries the extensions `AD_PRT`, `AD_ASM`, `ad_prt`, and `ad_asm`, in that order. The drive and the directories of the stored path are not read. The file name must be a bare name: a name that is empty, `.` or `..`, longer than 255 octets, or that holds `%`, `:`, a control character, or a separator, is not looked up, because a companion file source can read `%XX` as an escape and a drive or a separator as a path that leaves the directory of the assembly. The inclusion is then external with `assembly.member-unavailable`. When no extension opens, the loss gives the reason of the attempt that says most (a refusal before a plain not-found), and names the extensions tried. The decoder compares the document GUID of the inclusion (§5.2) with the GUID that follows the first bar in the `VERSION` stream of the file it opened. A file of another document stays unmerged: the inclusion is external with `assembly.member-document-mismatch`. The decoder keeps one definition for each file name and document GUID, so two inclusions of one name that reference two documents place two definitions. A file that holds the same bytes as the assembly being decoded is a cycle. A part decodes as §3 states, and a subassembly decodes by this section, recursively. Each component file has a definition key: `m<n>` for the n-th file, counted from 0, that an assembly merges, and `<placing key>/m<n>` for a file that only a subassembly with the key `<placing key>` places, so two files of the decode have two different keys. The `File` definition of a component that the assembly being decoded places is `alibre:xref:product-definition#<key>`; that of a component that only a subassembly places is a record of the subassembly, `alibre:<placing key>@xref:product-definition#<key>`. Each other record of a component file has the id that it has when the file is decoded alone, with `<key>@` at the head of its scope, and each source stream of the file has the name `alibre:<key>@<stream>`, so two files do not share an identity. A component that is not available, does not decode, lies past the nesting limit, or is a file that is already being decoded stays an external product definition with the path and the document GUID of the inclusion.

A decode reports these losses, each with the code `alibre/<code>`:

| Code | Meaning |
| --- | --- |
| `assembly.member-unavailable` | A component file is not beside the assembly, or its stored file name is not a bare name. |
| `assembly.member-document-mismatch` | The file beside the assembly is a different document than the inclusion references. |
| `assembly.member-undecoded` | A component file is available and does not decode. |
| `assembly.member-cycle` | A component file contains the assembly that places it. |
| `assembly.expansion-truncated` | A subassembly lies past the nesting limit, or occurrences lie past the occurrence limit. |
| `assembly.member-design-unmerged` | A component carries design records that the merge leaves out. |
| `assembly.placement-unreadable` | An inclusion has no readable rigid placement. One loss counts all inclusions and lists the first 16 with the reason. |
| `assembly.unit-unverified` | The `Default Units` of the workspace profile are not `cm`, or are not stated. The decoder still reads lengths in centimetres. |
| `assembly.datums-unread` | The origin datums at attribute 1 of `DESIGNASSEMBLY` are absent, reach no point, or reach a point off the origin. |
| `assembly.member-tolerance-unmerged` | A component states document tolerances that differ from the tolerances of the assembly. The assembly keeps its own. |
| `assembly.member-guid-unmapped` | Members state a GUID that the occurrence does not carry. One loss counts the members and the inclusions. |
| `assembly.inclusion-field-unmapped` | An inclusion states an attribute that the occurrence does not carry (no GUID at attribute 9, an attribute 11 that is no empty `PROFILE`, or another non-default value), or an attribute past the fifteen of the schema. One loss counts all inclusions. |
| `assembly.nested-member-state-unread` | An inclusion hides a component below its subassembly. One loss counts all inclusions. |
| `assembly.record-unreadable` | An inclusion, a member, or a configuration does not read as its entity, the list of configurations is absent, or the active configuration is not one of the configurations that read. The decode leaves the record out and goes on. |
| `assembly.configuration-unmapped` | An occurrence names a component configuration other than `Config<1>`, or the assembly holds a configuration besides the active one. The occurrences carry the state of the active configuration only. One configuration of the default name loses nothing and reports nothing. |
| `assembly.member-dialect-unverified` | A component file declares an ACIS save format outside the verified bands, as `alibre/source.kernel-dialect-unverified` states for a part. The assembly has no kernel layer of its own. |
| `assembly.entities-unread` | The stream holds an entity name that the reader does not know, or the relation lists of the assembly are not empty. |
| `source.streams-undecoded` | `CONSTITUENTS`, `PROP_DATA`, `VERSION`, and the two cache streams carry no decoded content. |

The decoder does not read assembly constraints, the configurations other than the active one, the configuration of the component that an inclusion uses, or flexible subassemblies (ALB-09 to ALB-12).
