# Autodesk Protein asset package: Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/protein.md`](../layouts/protein.md), generated from `docs/layouts/protein.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. `cargo test -p cadmpeg-layout-tables` proves the two agree.

A Protein package is a ZIP archive of XML schemas and paged instance-property streams. Fusion `.f3d` containers carry nested `.protein` members. Inventor IPT/IAM documents wrap one Protein ZIP in a compound `Protein` stream whose four-byte length prefix is specified in [`inventor.md`](inventor.md) §7.

## 1. Package

The package is a ZIP archive. A schema entry is a file whose path begins with `Schemas/` or contains `/Schemas/` and ends with `Schema.xml`. Every schema declares its identifier in the root `UID` element. Duplicate ZIP entry names and duplicate schema identifiers are malformed.

Schema XML, ZIP entry order, and schema inheritance are a text grammar over named members. They have no fixed byte offsets. §8 gives the members of the package of a Fusion Base Feature design.

## 2. Instance stream header

`InstanceProperties.bin` and `DefinitionIteratorProperties.bin` start with a 16-byte stream header. The first header word is the little-endian u32 page size and equals `0x88`. Bytes 4 through 11 hold two little-endian i32 words, each `-1`. Bytes 12 through 15 hold the little-endian u32 `0`. Open item DR-904 in [`f3d-open-items.md`](f3d-open-items.md) holds the role of these three words.

The bytes after the header are an exact multiple of the page size. A header without at least one complete page is malformed.

## 3. Pages

Each page is 136 bytes. Bytes 0 through 7 are the page header. Bytes 8 through 135 are the 128-byte page body. Record extents come from page framing; scanning the concatenated payload for a record-start marker does not recover them.

The page header holds three fields. Bytes 0 through 3 are the little-endian i32 index of the next page of the same record, or `-1` on the last page of a record. Bytes 4 through 5 are the little-endian u16 count of record bytes in the page body. Bytes 6 through 7 are the little-endian u16 record-start flag: `1` on the first page of a record and `0` on every other page. Page indices count from `0` at the first page after the stream header. Every page of a record except its last page is full, with count `128`. The three page kinds are the three header forms:

| Page         | Marker                      | Contribution                                                                                                      |
| ------------ | --------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Record start | `80 00 01 00` at bytes 4..8 | Opens a logical record and contributes the complete body at bytes 8..136.                                         |
| Continuation | `80 00 00 00` at bytes 4..8 | Extends the current record with its complete body.                                                                |
| Terminal     | `ff ff ff ff` at bytes 0..4 | Closes the current record, or carries one complete record when no record is open; bytes 8..`8 + used` contribute, where `used` is the little-endian `u16` at bytes 4..6. |

A record-start page or continuation page stores its four-byte marker at offset 4: the count `128` and the record-start flag. Bytes 0 through 3 hold the next-page index.

A terminal page stores `ff ff ff ff` at offset 0 and the used payload length as a little-endian u16 at offset 4. A page with `ff ff ff ff` at bytes 0 through 3 is a terminal page at every used length. The used length is `128` when the record ends at the end of the page body. Bytes 4 through 7 of that page equal the continuation marker, or the record-start marker when the page is the only page of its record. Bytes 6 through 7 hold the record-start flag: `1` when the terminal page is the only page of its record, and `0` when it closes a record that a record-start page opened. `used` is at most 128. The body bytes after the first `used` bytes carry no record data.

The pages of a record are consecutive: each next-page index is the index of the page that follows it. The records of a stream are consecutive and in table-of-contents order (§6). A stream has no page outside a record.

A continuation page requires an open record. A terminal page without an open record carries one complete record whose opening record marker is implicit. A stream ending with an open record is valid: the last open record is complete. A record-start page also ends the record before it, and so does a terminal page whose record-start flag is `1`.

## 4. Instance logical record

A valid logical record begins with four length-prefixed UTF-8 strings: schema identifier, asset GUID, base asset identifier, and `AssetLibID`. The value block that follows is the remaining members of the schema inheritance closure. Property order, carriers, connection blocks, and omitted inherited members are schema-driven and have no fixed byte offsets.

## 5. Definition-iterator logical record

A `DefinitionIteratorProperties.bin` logical record is the concatenation of the page contributions of §3, and its first byte is the first byte of the body of its first page. It has this sequence:

1. length-prefixed UTF-8 schema identifier, followed by a Boolean flag;
2. length-prefixed UTF-8 asset identifier;
3. length-prefixed UTF-8 base asset identifier;
4. little-endian u32 format version;
5. version-specific metadata strings;
6. little-endian u32 tag count and that many length-prefixed UTF-8 tags;
7. little-endian u32 preview-path count and that many length-prefixed UTF-8 paths.

The Boolean flag does not select the metadata layout. Format version `0` stores description only. Version `1` stores group and description. Version `2` stores category, group, and description. Version `3` stores category, group, subgroup, and description. The record has no further typed members after the last preview path. If page framing contributes bytes through the end of a start or continuation page, the remaining bytes are zero padding.

A definition stream can repeat one asset identifier under multiple schema identifiers. The pair `(asset identifier, schema identifier)` identifies a catalog entry. Base asset identifier, category, group, subgroup, description, tags, and preview paths are descriptive metadata and can differ between repeats. Open item DR-907 in [`f3d-open-items.md`](f3d-open-items.md) holds the join from an instance record to its definition record.

## 6. Table of contents

`AssetData/AssetTableOfContents.bin` lists the instance records of the package. It is a sequence of entries that ends at the end of the member. It has no header and no entry count. An entry holds, in this order:

1. u32-length-prefixed ASCII instance identifier;
2. u32-length-prefixed ASCII category;
3. little-endian u32 index of the first page of the entry's record in `DefinitionIteratorProperties.bin`;
4. little-endian u32 index of the first page of the entry's record in `InstanceProperties.bin`;
5. u32-length-prefixed ASCII schema identifier.

The instance identifier is the asset GUID of the instance record (§4), and the schema identifier is its schema identifier. The category is the `type` value of that schema (§8). The entries are in ascending byte order of the instance identifier. Each stream holds one record per entry, in entry order, so the page indices of the entries ascend.

Entries have variable length and no fixed byte offsets.

## 7. Stream version table

`AssetData/StreamVersionTable` is ASCII text. It holds one line per item, in this order: `AssetTableOfContents:3.1`, `DefinitionIteratorProperties:3.2`, `InstanceProperties:3.2`, and `AssetLibVersion:1.0`. Each line except the last ends with `,` and LF. The last line ends with LF.

## 8. Members of a Fusion Base Feature package

The `.protein` member of a Fusion Base Feature design (§9) is a package ZIP with one asset folder. The folder name `{F}` is an uppercase GUID of 36 characters. The package holds 23 members, in this ZIP order:

| Member | Content |
|---|---|
| `{F}/AssetData/DefinitionIteratorProperties.bin` | paged stream (§2, §3, §5) |
| `{F}/AssetData/InstanceProperties.bin` | paged stream (§2, §3, §4) |
| `{F}/Schemas/CommonSchema.xml` | schema document |
| `{F}/Schemas/GenericSchema.xml` | schema document |
| `{F}/Schemas/PhysMatSchema.xml` | schema document |
| `{F}/Schemas/PrismCommonSchema.xml` | schema document |
| `{F}/Schemas/PrismOpaqueSchema.xml` | schema document |
| `{F}/Schemas/RenderMaterialSchema.xml` | schema document |
| `{F}/Schemas/StructuralAdvancedSchema.xml` | schema document |
| `{F}/Schemas/StructuralCommonSchema.xml` | schema document |
| `{F}/Schemas/StructuralPlasticSchema.xml` | schema document |
| `{F}/AssetData/AssetTableOfContents.bin` | table of contents (§6) |
| `{F}/AssetData/StreamVersionTable` | stream version table (§7) |
| `{F}/AssetData/cnx.xml` | link list of the `AssetData` folder |
| `{F}/Schemas/cnx.xml` | link list of the `Schemas` folder |
| `{F}/cnx.xml` | link list of the asset folder |
| `cnx.xml` | link list of the package root |
| `[Content_Types].xml` | content types |
| `autodesk-design-package.xml` | package format |
| `{F}/AssetData/core.xml` | node record of the `AssetData` folder |
| `{F}/Schemas/core.xml` | node record of the `Schemas` folder |
| `{F}/core.xml` | node record of the asset folder |
| `core.xml` | node record of the package |

Every member is DEFLATE-compressed (method `8`), with general-purpose flags `0x0806`, version made by `20` (host `0`), version needed `20`, no extra field, no comment, and internal and external attributes `0`. Every member stores the same DOS date and time. The central directory lists the members in local-header order, and the end-of-central-directory record has no comment.

The text members are UTF-8 XML on one line. Each starts with the declaration `<?xml version="1.0" encoding="utf-8" ?>`, with a space before `?>`, and the root element follows with no whitespace. Three values fill them: the folder GUID `{F}`, the package identifier `{P}` (an uppercase GUID), and the package time `{T}` in the form `YYYY-MM-DDThh:mm:ssZ`. Every `{T}` in a package is the same value, and the DOS time of the members is `{T}` truncated to an even second. The members have these root elements:

- `cnx.xml`: `<root version="1" name="root"><group name="default"><link name="{F}"></link></group></root>`.
- `{F}/cnx.xml`: the same root and group with the two links `<link name="AssetData"></link><link name="Schemas"></link>`.
- `{F}/AssetData/cnx.xml`: the same root and group with one link per binary member, in the order `AssetTableOfContents`, `DefinitionIteratorProperties`, `InstanceProperties`, `StreamVersionTable`. A link is `<link name="{name}" class="schema.autodesk.data.1.resource"><time name="created">{T}</time><bool name="interop%20content">0</bool><uri name="name">{file}</uri><string name="type">application/octet-stream</string></link>`, where `{file}` is the member file name (`{name}.bin`, or `StreamVersionTable`).
- `{F}/Schemas/cnx.xml`: the same link form for the nine schema documents in member order, with `{name}` the schema identifier, `{file}` `{name}.xml`, and type `application/xml`.
- `[Content_Types].xml`: `<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">` with the three elements `<Default Extension="xml" ContentType="application/xml"/>`, `<Default Extension="" ContentType="application/octet-stream"/>`, and `<Default Extension="bin" ContentType="application/octet-stream"/>`.
- `autodesk-design-package.xml`: `<formats><format>http://schema.autodesk.com/design-package/2009</format></formats>`.
- `core.xml`: `<root version="1" name="root"><group name="content"><node name="asset" class="schema.autodesk.package.1.asset">` with the children `<time name="created">{T}</time>`, `<uuid name="identifier">{P}</uuid>`, `<string name="last saved"></string>`, `<bool name="read only">0</bool>`, `<time name="updated">{T}</time>`, and `<uint32 name="updates">1</uint32>`, then `</node></group></root>`.
- `{F}/core.xml`: the same node form with the children `<time name="created">{T}</time>`, `<uint32 name="embed option">49</uint32>`, `<uuid name="identifier">{F}</uuid>`, `<string name="name">{F}</string>`, `<time name="updated">{T}</time>`, and `<uint32 name="updates">1</uint32>`.
- `{F}/AssetData/core.xml` and `{F}/Schemas/core.xml`: the same node form with the children `<uuid name="identifier">{P}</uuid>`, `<time name="created">{T}</time>`, `<string name="name">{folder}</string>`, `<time name="updated">{T}</time>`, and `<uint32 name="updates">1</uint32>`, where `{folder}` is `AssetData` or `Schemas`.

A schema document declares one schema (§1). The nine schemas form these inheritance chains:

| Schema | Base | `type` | `SchemaVersion` |
|---|---|---|---|
| `CommonSchema` | | | `4` |
| `RenderMaterialSchema` | `CommonSchema` | | `5` |
| `GenericSchema` | `RenderMaterialSchema` | `materialappearance` | `6` |
| `PrismCommonSchema` | `CommonSchema` | | `6` |
| `PrismOpaqueSchema` | `PrismCommonSchema` | `materialappearance` | `9` |
| `PhysMatSchema` | `CommonSchema` | `physicalmaterial` | `5` |
| `StructuralCommonSchema` | `CommonSchema` | `Structural` | `6` |
| `StructuralAdvancedSchema` | `StructuralCommonSchema` | `Structural` | `7` |
| `StructuralPlasticSchema` | `StructuralAdvancedSchema` | `Structural` | `7` |

Open item DR-905 in [`f3d-open-items.md`](f3d-open-items.md) holds the text of the schema documents. Open item DR-908 holds the member lists of other Fusion packages.

## 9. Package of a Fusion Base Feature design

The Protein package of a Fusion Base Feature design holds one physical material, its two aspects, and one appearance pair per appearance that the design assigns. The tables of this section give the first form of the appearance pair, whose twin has base asset identifier `PrismOpaque` and whose colour is a field. The second form, in which the twin is a copy of the appearance aspect, and what selects it are DR-906. With one appearance, the package has five instance records:

| Instance | Schema | Category | Base asset identifier | `AssetLibID` | Definition asset identifier |
|---|---|---|---|---|---|
| physical material | `PhysMatSchema` | `physicalmaterial` | `PrismMaterial-022` | `C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C` | `PrismMaterial-022` |
| structural aspect | `StructuralPlasticSchema` | `Structural` | `PrismMaterial-022_physmat_aspects_1:StructuralPlastic-004` | `C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C` | `StructuralPlastic-004` |
| appearance aspect | `PrismOpaqueSchema` | `materialappearance` | `PrismMaterial-022_physmat_aspects:Prism-374` | `C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C` | `Prism-374` |
| appearance | `GenericSchema` | `materialappearance` | `Generic` | `314DE259-5443-4621-BFBD-1730C6CC9AE9` | `Generic` |
| appearance twin | `PrismOpaqueSchema` | `materialappearance` | `PrismOpaque` | `BA5EE55E-9982-449B-9D66-9F036540E140` | `Opaque(R,G,B)` |

The instance identifier of each instance except the twin is an uppercase GUID of 36 characters. The identifier of the twin is the identifier of its appearance followed by `_Post2015`. The physical material's `physmat_aspects` connection names the appearance aspect and then the structural aspect.

The Design stream joins to the package through instance identifiers, written there as UTF-16 text. The Protein asset manager (Design record 1, [`f3d.md`](f3d.md) §3.1) stores the physical-material identifier and the identifier of each appearance, and names the package by its member basename `ProteinAsset.<uuid>.protein`, with `<uuid>` a lowercase GUID. The component aggregate stores the physical-material identifier. Each body-reference record stores the physical-material identifier, the identifier of the body's appearance, and the identifier of the appearance of each face that it assigns. The aspects and the twins occur only in the package. The basename GUID, the folder GUID `{F}`, and the package identifier `{P}` are three distinct GUIDs.

An appearance and its twin carry one colour `(r, g, b, 1)`: `generic_diffuse` of the appearance and `opaque_albedo` of the twin. Each channel is a value `n / 255` rounded to binary32 and widened to binary64. The definition of the twin has the asset identifier `Opaque(R,G,B)`, the decimal 8-bit channel values. In the first form, the appearance of the body has the colour `(246, 246, 243)`. In a package whose body appearance has the second form of open item DR-906, an appearance that a per-face assignment adds, and its twin, have the colour of that face. Their other values are the values in the tables of the body's appearance and twin below.

The table-of-contents entry of each instance names one definition record, with these values. Format version `2` stores category, group, and description; version `1` stores group and description (§5).

| Definition | Flag | Base asset identifier | Version | Metadata | Tags | Preview paths |
|---|---|---|---|---|---|---|
| `PrismMaterial-022` | `0` | `PrismMaterial-022` | `2` | `Plastic`; `advanced physical material`; `Acrylonitrile butadiene styrene` | `physical`, `Prism-374`, `ABS - white glossy`, `Plastic`, `Translucent`, `materials`, `opaque`, `StructuralPlastic-004`, `Acrylonitrile Butadiene Styrene, Molded`, `ABS`, `structural`, `CarbonBasic-003`, `Acrylonitrile Butadiene Styrene, Molded, Plastic and vinyl material`, `plastic`, `white`, `smooth`, `AppearancePlasticVinyl`, `StructuralPlastic`, `Carbon` | `Mats<s>PrismOpaque<s>Presets<s>t_Prism-374.png`, with the separator field `<s>` |
| `StructuralPlastic-004` | `0` | `StructuralPlastic-004` | `2` | `Plastic/Thermoplastic`; `Default`; `Acrylonitrile Butadiene Styrene, Molded` | `ABS`, `structural`, `Plastic` | one empty path |
| `Prism-374` | `0` | `Prism-374` | `2` | `Plastic`; `Default`; `ABS - white glossy` | `Plastic`, `Translucent`, `materials`, `opaque` | `Mats/PrismOpaque/Presets/t_Prism-374.png` |
| `Generic` | `0` | `Generic` | `2` | `Default`; `Miscellaneous`; `Generic material.` | `materials`, `generic` | `Mats/Generic/generic.png` |
| `Opaque(R,G,B)` | `1` | an uppercase GUID | `1` | `Default`; `Prism opaque material.` | `materials`, `opaque` | none |

The tables below give each instance record's value block (§4) in serialization order. "Declared by" names the schema of the inheritance closure that declares the property. A connectable property follows its value with a connection carrier; "connection: none" is the byte `0`.

Physical material (value block 203 bytes):

| Property | Declared by | Carrier | Value |
|---|---|---|---|
| `ExchangeGUID` | `CommonSchema` | Uuid | empty |
| `physmat_Comments` | `PhysMatSchema` | String | empty |
| `physmat_Cost` | `PhysMatSchema` | String | empty |
| `physmat_Keynote` | `PhysMatSchema` | String | empty |
| `physmat_Keywords` | `PhysMatSchema` | String | empty |
| `physmat_Label` | `PhysMatSchema` | String | empty |
| `physmat_Manufacturer` | `PhysMatSchema` | String | empty |
| `physmat_Mark` | `PhysMatSchema` | String | empty |
| `physmat_Model` | `PhysMatSchema` | String | empty |
| `physmat_Type` | `PhysMatSchema` | Choice | 0 |
| `physmat_URL` | `PhysMatSchema` | String | empty |
| `physmat_aspects` | `PhysMatSchema` | Reference | connection: the appearance aspect, the structural aspect |
| `physmat_class` | `PhysMatSchema` | String | empty |
| `physmat_classification` | `PhysMatSchema` | String | `Autodesk.Material Classifications.Plastic.Thermoplastics` |
| `physmat_deleted_aspects` | `PhysMatSchema` | Reference | connection: none |
| `revision` | `CommonSchema` | Integer | 1 |
| `version` | `CommonSchema` | Integer | 4 |

Structural aspect (value block 553 bytes):

| Property | Declared by | Carrier | Value |
|---|---|---|---|
| `ExchangeGUID` | `CommonSchema` | Uuid | empty |
| `common_Shared_Asset` | `StructuralCommonSchema` | Choice | 0 |
| `revision` | `CommonSchema` | Integer | 1 |
| `structural_Behavior` | `StructuralAdvancedSchema` | Choice | 0 |
| `structural_Damping_coefficient` | `StructuralCommonSchema` | Float | 0.0, tag `0x1001` |
| `structural_Density` | `StructuralCommonSchema` | Float | 1060.0, tag `0x11001` |
| `structural_ExternalMaterialID` | `StructuralCommonSchema` | String | `2:MATERIAL:MI_InventorSustainability:85087E0D-D221-475D-ABB3-22B9D6A96369:82BEAFC6-1AC0-4E4C-BDA9-C5733B45AE03:0` |
| `structural_Minimum_tensile_strength` | `StructuralAdvancedSchema` | Float | 29600.0, tag `0x21008` |
| `structural_Minimum_yield_stress` | `StructuralAdvancedSchema` | Float | 20000.0, tag `0x21008` |
| `structural_Poisson_ratio` | `StructuralAdvancedSchema` | Float | 0.38, tag `0x1001` |
| `structural_Poisson_ratio_12` | `StructuralAdvancedSchema` | Float | 0.38, tag `0x1001` |
| `structural_Poisson_ratio_23` | `StructuralAdvancedSchema` | Float | 0.38, tag `0x1001` |
| `structural_Poisson_ratio_X` | `StructuralAdvancedSchema` | Float | 0.38, tag `0x1001` |
| `structural_Poisson_ratio_Y` | `StructuralAdvancedSchema` | Float | 0.38, tag `0x1001` |
| `structural_Poisson_ratio_Z` | `StructuralAdvancedSchema` | Float | 0.38, tag `0x1001` |
| `structural_Shear_modulus` | `StructuralAdvancedSchema` | Float | 805000.0, tag `0x21008` |
| `structural_Shear_modulus_12` | `StructuralAdvancedSchema` | Float | 805000.0, tag `0x21008` |
| `structural_Shear_modulus_X` | `StructuralAdvancedSchema` | Float | 805000.0, tag `0x21008` |
| `structural_Shear_modulus_Y` | `StructuralAdvancedSchema` | Float | 805000.0, tag `0x21008` |
| `structural_Shear_modulus_Z` | `StructuralAdvancedSchema` | Float | 805000.0, tag `0x21008` |
| `structural_Specific_heat` | `StructuralCommonSchema` | Float | 1.5, tag `0x46001` |
| `structural_Thermal_conductivity` | `StructuralCommonSchema` | Float | 0.16, tag `0x45001` |
| `structural_Thermal_conductivity_X` | `StructuralAdvancedSchema` | Float | 0.16, tag `0x45001` |
| `structural_Thermal_conductivity_Y` | `StructuralAdvancedSchema` | Float | 0.16, tag `0x45001` |
| `structural_Thermal_conductivity_Z` | `StructuralAdvancedSchema` | Float | 0.16, tag `0x45001` |
| `structural_Thermal_expansion_coefficient` | `StructuralAdvancedSchema` | Float | 85.7, tag `0x43003` |
| `structural_Thermal_expansion_coefficient_1` | `StructuralAdvancedSchema` | Float | 85.7, tag `0x43003` |
| `structural_Thermal_expansion_coefficient_2` | `StructuralAdvancedSchema` | Float | 85.7, tag `0x43003` |
| `structural_Thermal_expansion_coefficient_X` | `StructuralAdvancedSchema` | Float | 85.7, tag `0x43003` |
| `structural_Thermal_expansion_coefficient_Y` | `StructuralAdvancedSchema` | Float | 85.7, tag `0x43003` |
| `structural_Thermal_expansion_coefficient_Z` | `StructuralAdvancedSchema` | Float | 85.7, tag `0x43003` |
| `structural_Young_modulus` | `StructuralAdvancedSchema` | Float | 2240000.0, tag `0x21008` |
| `structural_Young_modulus_1` | `StructuralAdvancedSchema` | Float | 2240000.0, tag `0x21008` |
| `structural_Young_modulus_2` | `StructuralAdvancedSchema` | Float | 2240000.0, tag `0x21008` |
| `structural_Young_modulus_X` | `StructuralAdvancedSchema` | Float | 2240000.0, tag `0x21008` |
| `structural_Young_modulus_Y` | `StructuralAdvancedSchema` | Float | 2240000.0, tag `0x21008` |
| `structural_Young_modulus_Z` | `StructuralAdvancedSchema` | Float | 2240000.0, tag `0x21008` |
| `structural_source` | `StructuralAdvancedSchema` | String | `Autodesk` |
| `structural_sourceURL` | `StructuralAdvancedSchema` | String | empty |
| `structural_subclass` | `StructuralCommonSchema` | String | `Thermoplastic` |
| `version` | `CommonSchema` | Integer | 4 |

Appearance aspect (value block 288 bytes):

| Property | Declared by | Carrier | Value |
|---|---|---|---|
| `ExchangeGUID` | `CommonSchema` | Uuid | empty |
| `interior_model` | `PrismCommonSchema` | Integer | 0 |
| `opaque_albedo` | `PrismOpaqueSchema` | Color | (0.963976232744, 0.963976232744, 0.954687171886, 1.0); connection: none |
| `opaque_albedo_colorspace` | `PrismOpaqueSchema` | Choice | 1 |
| `opaque_albedo_urn` | `PrismOpaqueSchema` | String | empty |
| `opaque_emission` | `PrismOpaqueSchema` | Boolean | false |
| `opaque_f0` | `PrismOpaqueSchema` | Float | 0.0529; connection: none |
| `opaque_f0_urn` | `PrismOpaqueSchema` | String | empty |
| `opaque_luminance` | `PrismOpaqueSchema` | Float | 0.0, tag `0x37001` |
| `opaque_luminance_modifier` | `PrismOpaqueSchema` | Color | (1.0, 1.0, 1.0, 1.0); connection: none |
| `opaque_luminance_modifier_colorspace` | `PrismOpaqueSchema` | Choice | 1 |
| `opaque_luminance_modifier_urn` | `PrismOpaqueSchema` | String | empty |
| `opaque_mfp` | `PrismOpaqueSchema` | Distance | 0.75, tag `0x200e` |
| `opaque_mfp_modifier` | `PrismOpaqueSchema` | Color | (1.0, 0.666666666667, 0.133333333333, 1.0); connection: none |
| `opaque_mfp_modifier_colorspace` | `PrismOpaqueSchema` | Choice | 1 |
| `opaque_mfp_modifier_urn` | `PrismOpaqueSchema` | String | empty |
| `opaque_translucency` | `PrismOpaqueSchema` | Boolean | true |
| `revision` | `CommonSchema` | Integer | 1 |
| `surface_albedo` | `PrismCommonSchema` | Color | (1.0, 1.0, 1.0, 1.0); connection: none |
| `surface_albedo_colorspace` | `PrismCommonSchema` | Choice | 1 |
| `surface_albedo_urn` | `PrismCommonSchema` | String | empty |
| `surface_anisotropy` | `PrismCommonSchema` | Float | 0.0; connection: none |
| `surface_anisotropy_urn` | `PrismCommonSchema` | String | empty |
| `surface_cutout` | `PrismCommonSchema` | Reference | connection: none |
| `surface_cutout_urn` | `PrismCommonSchema` | String | empty |
| `surface_ndf_type` | `PrismCommonSchema` | Choice | 1 |
| `surface_normal` | `PrismCommonSchema` | Reference | connection: none |
| `surface_normal_urn` | `PrismCommonSchema` | String | empty |
| `surface_rotation` | `PrismCommonSchema` | Float | 0.0; connection: none |
| `surface_rotation_urn` | `PrismCommonSchema` | String | empty |
| `surface_roughness` | `PrismCommonSchema` | Float | 0.264575131106; connection: none |
| `surface_roughness_urn` | `PrismCommonSchema` | String | empty |
| `swatch` | `PrismCommonSchema` | String | `Swatch-Torus` |
| `version` | `CommonSchema` | Integer | 1 |

Appearance of the body (value block 326 bytes):

| Property | Declared by | Carrier | Value |
|---|---|---|---|
| `ExchangeGUID` | `CommonSchema` | Uuid | empty |
| `color_by_object` | `GenericSchema` | Boolean | false |
| `common_Shared_Asset` | `RenderMaterialSchema` | Choice | 0 |
| `common_Tint_color` | `RenderMaterialSchema` | Color | (0.315, 0.315, 0.315, 1.0) |
| `common_Tint_color_colorspace` | `RenderMaterialSchema` | Choice | 2 |
| `common_Tint_toggle` | `RenderMaterialSchema` | Boolean | false |
| `generic_ao_details` | `GenericSchema` | Boolean | true |
| `generic_ao_distance` | `GenericSchema` | Float | 4.0 |
| `generic_ao_on` | `GenericSchema` | Boolean | false |
| `generic_ao_samples` | `GenericSchema` | Integer | 16 |
| `generic_backface_cull` | `GenericSchema` | Boolean | false |
| `generic_bump_amount` | `GenericSchema` | Float | 0.3; connection: none |
| `generic_bump_map` | `GenericSchema` | Color | (0.0, 0.0, 0.0, 1.0); connection: none |
| `generic_cutout_opacity` | `GenericSchema` | Float | 1.0; connection: none |
| `generic_diffuse` | `GenericSchema` | Color | (0.9647058844566345, 0.9647058844566345, 0.9529411792755127, 1.0); connection: none |
| `generic_diffuse_colorspace` | `GenericSchema` | Choice | 2 |
| `generic_diffuse_image_fade` | `GenericSchema` | Float | 1.0 |
| `generic_glossiness` | `GenericSchema` | Float | 1.0; connection: none |
| `generic_is_metal` | `GenericSchema` | Boolean | false |
| `generic_refl_depth` | `GenericSchema` | Integer | 0 |
| `generic_reflection_glossy_samples` | `GenericSchema` | Integer | 12 |
| `generic_reflectivity_at_0deg` | `GenericSchema` | Float | 0.02; connection: none |
| `generic_reflectivity_at_90deg` | `GenericSchema` | Float | 0.0; connection: none |
| `generic_refr_depth` | `GenericSchema` | Integer | -1 |
| `generic_refraction_glossy_samples` | `GenericSchema` | Integer | 12 |
| `generic_refraction_index` | `GenericSchema` | Float | 1.4 |
| `generic_refraction_translucency_weight` | `GenericSchema` | Float | 0.5; connection: none |
| `generic_roundcorners_allow_different_materials` | `GenericSchema` | Boolean | false |
| `generic_roundcorners_radius` | `GenericSchema` | Float | 0.0; connection: none |
| `generic_self_illum_color_temperature` | `GenericSchema` | Float | 0.0 |
| `generic_self_illum_filter_map` | `GenericSchema` | Color | (1.0, 1.0, 1.0, 1.0); connection: none |
| `generic_self_illum_filter_map_colorspace` | `GenericSchema` | Choice | 2 |
| `generic_self_illum_luminance` | `GenericSchema` | Float | 0.0 |
| `generic_transparency` | `GenericSchema` | Float | 0.0; connection: none |
| `generic_transparency_image_fade` | `GenericSchema` | Float | 0.0 |
| `mode` | `GenericSchema` | Integer | 4 |
| `revision` | `CommonSchema` | Integer | 1 |
| `swatch` | `RenderMaterialSchema` | String | `Swatch-Torus` |
| `version` | `CommonSchema` | Integer | 4 |

Appearance twin of the body (value block 288 bytes):

| Property | Declared by | Carrier | Value |
|---|---|---|---|
| `ExchangeGUID` | `CommonSchema` | Uuid | empty |
| `interior_model` | `PrismCommonSchema` | Integer | 0 |
| `opaque_albedo` | `PrismOpaqueSchema` | Color | (0.9647058844566345, 0.9647058844566345, 0.9529411792755127, 1.0); connection: none |
| `opaque_albedo_colorspace` | `PrismOpaqueSchema` | Choice | 0 |
| `opaque_albedo_urn` | `PrismOpaqueSchema` | String | empty |
| `opaque_emission` | `PrismOpaqueSchema` | Boolean | false |
| `opaque_f0` | `PrismOpaqueSchema` | Float | 0.02; connection: none |
| `opaque_f0_urn` | `PrismOpaqueSchema` | String | empty |
| `opaque_luminance` | `PrismOpaqueSchema` | Float | 0.0, tag `0x37001` |
| `opaque_luminance_modifier` | `PrismOpaqueSchema` | Color | (1.0, 1.0, 1.0, 1.0); connection: none |
| `opaque_luminance_modifier_colorspace` | `PrismOpaqueSchema` | Choice | 0 |
| `opaque_luminance_modifier_urn` | `PrismOpaqueSchema` | String | empty |
| `opaque_mfp` | `PrismOpaqueSchema` | Distance | 0.5, tag `0x200e` |
| `opaque_mfp_modifier` | `PrismOpaqueSchema` | Color | (1.0, 1.0, 1.0, 1.0); connection: none |
| `opaque_mfp_modifier_colorspace` | `PrismOpaqueSchema` | Choice | 0 |
| `opaque_mfp_modifier_urn` | `PrismOpaqueSchema` | String | empty |
| `opaque_translucency` | `PrismOpaqueSchema` | Boolean | false |
| `revision` | `CommonSchema` | Integer | 1 |
| `surface_albedo` | `PrismCommonSchema` | Color | (1.0, 1.0, 1.0, 1.0); connection: none |
| `surface_albedo_colorspace` | `PrismCommonSchema` | Choice | 0 |
| `surface_albedo_urn` | `PrismCommonSchema` | String | empty |
| `surface_anisotropy` | `PrismCommonSchema` | Float | 0.0; connection: none |
| `surface_anisotropy_urn` | `PrismCommonSchema` | String | empty |
| `surface_cutout` | `PrismCommonSchema` | Reference | connection: none |
| `surface_cutout_urn` | `PrismCommonSchema` | String | empty |
| `surface_ndf_type` | `PrismCommonSchema` | Choice | 1 |
| `surface_normal` | `PrismCommonSchema` | Reference | connection: none |
| `surface_normal_urn` | `PrismCommonSchema` | String | empty |
| `surface_rotation` | `PrismCommonSchema` | Float | 0.0; connection: none |
| `surface_rotation_urn` | `PrismCommonSchema` | String | empty |
| `surface_roughness` | `PrismCommonSchema` | Float | 0.2; connection: none |
| `surface_roughness_urn` | `PrismCommonSchema` | String | empty |
| `swatch` | `PrismCommonSchema` | String | `Swatch-Torus` |
| `version` | `CommonSchema` | Integer | 4 |

A design with several bodies and one appearance has the same package; only the reference counts of the Protein asset manager change. The GUIDs (basename, folder, package identifier, instance identifiers, and the base asset identifier of each `Opaque(R,G,B)` definition), the package time, and the colours of the per-face appearances vary between packages. The table-of-contents order follows from the instance identifiers, and the page indices follow from that order and the record lengths. Open item DR-906 in [`f3d-open-items.md`](f3d-open-items.md) holds a second form of the body's appearance, and open item DR-909 the values of the separator `<s>` of the `PrismMaterial-022` preview path.
