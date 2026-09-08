# Autodesk Inventor IPT/IAM

This specification defines the byte semantics of the supported Autodesk Inventor IPT/IAM envelope. Multi-byte integers are little-endian unless a section states otherwise.

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/inventor.md`](../layouts/inventor.md), generated from `docs/layouts/inventor.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. `cargo test -p cadmpeg --test layout_tables` proves the two agree.

## 1. Compound container

An IPT or IAM document is a Compound File Binary container. The container follows the CFB sector, FAT, DIFAT, mini-FAT, directory, and sibling-tree rules. The directory contains the `RSeStorage` storage. A positive format identification also requires a structurally reached `RSeStorage/RSeSegInfo` stream or a stream at `RSeStorage/V<n>/RSeDb`, where `<n>` is one or more decimal digits.

The database storage-band number `<n>` and the database schema are independent values. An `M<token>` metadata stream and a `B<token>` bulk stream are one segment pair only when both are direct children of `RSeStorage` and their nonempty `<token>` suffixes are byte-equal.

## 2. RSe database and indexes

An `RSeDb` stream starts with a 16-byte database identifier, a u32 schema, an 8-byte creation-version tuple, a u64 creation FILETIME, an 8-byte save-version tuple, a u64 save FILETIME, and a u32-counted UTF-16LE note. A version tuple contains `revision`, `minor`, `major`, and five state bytes. Schema 31 selects the supported registry grammar. The stream ends after the note.

`RSeStorage/RSeSegInfo` starts with a u32 segment count. Each registry entry contains counted UTF-16LE display and type names, segment and revision identifiers, state words, an 8-byte version tuple, object records, and node records. A segment metadata identifier resolves to exactly one registry entry. The registry type name selects the segment family.

`RSeStorage/RSeDbRevisionInfo` starts with u32 version 3 and a u32 record count. Each record contains a 16-byte identifier, u32 flags, and a u16 kind. Kind `0xffff` is followed by a one-byte selector and an 8-byte value when the selector is nonzero or a 16-byte value when it is zero. Other kinds have no value payload. The stream ends after the declared records.

## 3. RSe metadata stream

An `M<token>` stream starts with a u32 byte count and that many UTF-8 marker bytes. The supported marker is `RSe Meta Stream Version 8`. It is followed by u16 version 8, eight u16 header values, a u32-counted UTF-16LE display name, a 16-byte segment identifier, three u32 state words, two u32-counted UTF-8 timestamp strings, and a one-byte body form. One exact zlib member occupies the remainder of the stream.

The inflated body starts with seven u16 values and contains 11 sections plus a terminal 16-byte identifier. Sections 1 through 4 use a forward counted frame:

```text
count u32
items[count]
span u32
```

`span` equals `4 + count * item_size`. Section 1 items are 4-byte block descriptors. Bit 31 states that the corresponding bulk block is stored; bits 0 through 30 give its payload length. Section 2 items are 10 bytes. Section 3 items are 28 bytes. Section 4 items are 28-byte type descriptors and the count does not exceed 256. A type descriptor contains a 16-byte type identifier and two `(u16, u32)` field pairs.

Sections 5 through 11 use backward framing. The eight bytes before a section payload contain a u32 span to the previous payload and a u32 discriminator. The section chain joins the section 4 footer exactly. Section 11 has a 72-byte payload. The 16-byte terminal identifier follows section 11.

## 4. RSe bulk stream and records

A `B<token>` stream starts with a 16-byte prefix and a u16 form. One exact zlib member occupies the remainder. The prefix and form are retained as envelope values; they do not select a segment by themselves.

Each stored section-1 block descriptor frames one record in block order. A record contains:

```text
selector u32
payload bytes[block.payload_len]
trailing_payload_len u32
versioned_trailer bytes[]
```

The selector low byte indexes the section 4 type table. `trailing_payload_len` is zero or equals `block.payload_len`. Segment major versions above 18 have an extended trailer. Its presence byte selects an empty trailer or a typed property and reference list. After all records, the expanded stream has a u32 `0xffffffff` marker and a retained trailer. The stream is exhausted exactly.

## 5. Active part kernel carrier

The active part kernel carrier is the sole record with type identifier `5c5945f6d5113313100060a6bba647b5` in the sole `PmBRep` segment. The record payload starts with u32 header state, u16 header kind, u32 header value, and u32 schema. The kernel bytes start at payload offset 14.

Segment major versions 15 through 22 use a 17-byte footer. Segment major versions 23 and later use an 18-byte footer with one additional zero byte. The footer contains a u32 selected key, a Boolean byte, an i32 delta state, the optional zero byte, a u32 history reference, and u32 `0xffffffff`. The footer ends at the record boundary.

Kernel bytes beginning with `ASM BinaryFile4` or `ASM BinaryFile8` use the ASM header and SAB grammar in [asm.md](asm.md). Kernel bytes beginning with `ACIS BinaryFile` use the 32-bit ACIS header and SAB grammar in [asm.md](asm.md) at every save format. Save-format majors 217 and 218 are the verified bands. A kernel signature validates the already typed record. It is not a record locator.

## 6. OLE properties and preview

Root streams that contain an OLE Property Set self-identify through their byte-order marker, section directory, and FMTID. Section offsets bound each property. Code-page properties select LPSTR decoding. LPWSTR values contain UTF-16LE code units. Scalar, vector, FILETIME, BLOB, and clipboard values retain their type code. A clipboard preview is emitted only when its payload has a recognized image signature.

## 7. Protein package

The root `Protein` stream starts with a u32 payload length. Zero length is the complete four-byte empty form. A nonzero length equals the exact remaining byte count, and the remaining bytes are one ZIP archive. ZIP entry names are unique and do not contain absolute, parent, current-directory, empty, NUL, or backslash path components.

The ZIP package, `InstanceProperties.bin` page framing, schemas, and typed property records are specified in [`protein.md`](protein.md). Asset definitions form a catalog. A topology assignment requires a separate typed assignment record.

## 8. External references

`UFRxDoc` schemas 11 through 15 start with a u16 schema and a u16 section-version count. The section-version table governs optional header fields. The schema-15 representation/model-state branch adds a u16 representation prefix. An assembly then stores two counted UTF-16LE representation strings. A part omits these two strings. Both document kinds continue with a two-u16 secondary LOD state, a counted UTF-16LE active model-state name, and a two-u16 model-state pair; they omit the older header-version-flags field. The model-state table precedes the external-reference table. Each model-state record contains a u8 prefix, counted UTF-16LE name, two-u16 state pair, u32 prefix count, u32 parameter count, the counted parameter records, and a 77-byte suffix. Each parameter record contains a counted UTF-16LE name, u8 tag, u16 kind, u16 state, counted UTF-16LE value, and u16 trailer.

The external-reference table contains counted UTF-16LE paths and names, state groups, 16-byte document and database identifiers, a u32 reference identifier, u32 occurrence count, u32 version, and u32 flags. Section-version entry 4 values of 2 or later add a zero-byte table terminator. Persisted paths remain unresolved. The codec does not open them.

The embedded-reference table follows the external-reference table. It starts with a u32 record count. Each record contains a u32 value, u64 FILETIME, u32 value, an additional u32 value when section-version entry 15 is 7 or later, another u32 value, counted UTF-16LE path, i32 library identifier, counted UTF-16LE library name, u16 state, counted UTF-16LE display name, and eight state bytes. Section-version entry 15 values of 6 or later add a zero-byte table terminator.

The occurrence table follows the embedded-reference table. It starts with a u32 count. A zero count is followed by one zero u32. Each nonempty record starts with:

```text
end_string_flag u32
file_reference_id u32
occurrence_id u32
header_value u32
title_form_or_count u32
```

For occurrence section versions before 28, title form 0 has no title, form 1 is followed by a separately counted UTF-16LE title, and other nonzero values are the title code-unit count. The header then contains five state bytes, one additional byte at version 20 or later, and another byte at version 21 or later. At version 28 or later, every nonzero title form is followed by a separately counted UTF-16LE title. The header then contains at most eight zero u16 values, u16 marker `0x2080`, and the u32 sequence 0, 1, 0.

Two property sections follow the header. Each starts with a u32 section value and u32 property count. A property contains a Boolean presence byte, u8 type tag, u32 value, the repeated type tag, the tag-selected value, and a u32 trailer. Settings follow as counted UTF-16LE name, 16-byte identifier, and counted UTF-8 value records. The export section contains ten state bytes, version-selected padding, and either an empty count, a `0x00ffffff` or `0xffffffff` sentinel followed by zero u32, or a bounded typed export table. Each occurrence record is bounded by exhaustion of these nested counts.

## 9. Assembly records

An `AmDc` record with type identifier `604d8790d011f8d10008cabc0663dc09` stores one assembly occurrence identity. Its payload contains:

```text
header_value u32
header_id u16
next_reference u32
flags u32
owner_reference u32
node_index u32
state i32[2]
relation_marker u32 = 0x30000002
relation_count u32 = 0
ordinal_key u32
related_marker u32 = 0x30000002
related_count u32
related_header u32[2] when related_count is nonzero
related_references u32[related_count]
child_reference u32
identity_mode u16 = 0x0200
occurrence_id u32
label counted UTF-16LE = "DCx"
trailer u16 = 1
```

The record ends after the trailer. Occurrence identifiers are document-local values and are not required to be dense.

An `AmGraphics` record with type identifier `a26371cad011b2d30008bfbb21eddc09` or `07d0d0b9d4112d5f6000f8830e73fcb0` stores an assembly placement. Its common payload prefix contains u32 zero, u16 header identifier, u32 owner reference, u32 attribute reference, u8 state, and a compact 4-by-4 transform. The transform can start with optional u32 `0x00000203`, followed by a u16 set mask and a u16 zero mask. The 16 matrix elements use row-major bit positions. A clear zero-mask bit stores an f64 when the set-mask bit is clear and represents `1` when the set-mask bit is set. A set zero-mask bit represents `0` when the set-mask bit is clear and `-1` when the set-mask bit is set. Stored f64 values are finite.

The active placement branch continues with u8 branch, u8 graphics state, u32 occurrence identifier, counted UTF-16LE label `"GRx"`, u16 invariant 1, u32 graphics index, u32 object reference, and the occurrence identifier again. Both occurrence-identifier fields are equal. The remaining branch-specific suffix is retained. A placement joins an occurrence through the exact occurrence identifier.

A `UFRxDoc` occurrence joins its external prototype through `file_reference_id` and joins the `AmDc` and `AmGraphics` records through `occurrence_id`. The external-reference occurrence count equals the number of joined `UFRxDoc` occurrences. Each occurrence in the current document is a root placement. A referenced assembly remains an unresolved external prototype; its internal occurrences are not inserted into the current document.

Inventor placement translations use centimetres. Neutral occurrence translations use millimetres, so the first three elements in matrix column 3 are multiplied by 10. The remaining matrix coefficients are unchanged. External-reference state bit `0x2000` marks its occurrence as suppressed. A suppressed occurrence has `visible = false`; when it has no graphics placement, its neutral transform is identity. An active occurrence without a finite affine graphics placement is not transferred and produces an assembly-placement loss.

## 10. Part appearance assignments

A `PmApp` record with type identifier `cdecfb11d1116b250008ebbb21eddc09` stores the document-default style references. For segment major versions above 19, its 55-byte payload contains:

```text
header_value u32
header_id u16
material_reference u32
rendering_style_reference u32
related_references u32[7]
state u8
terminal_reference u32
padding bytes[8] = 0
```

Record references use bit 31 as a reference qualifier and bits 0 through 30 as a one-based record index. The referenced RSe record ordinal is the index minus one. A zero index is null.

A `PmApp` record with type identifier `6fd85967d2113878600094b70b02ecb0` stores one rendering style. For segment major versions above 23, its fixed prefix contains:

```text
header_value u32
header_id u16
state u8
flags u16
padding u16 = 0
values u16[2]
default_state u32
value u32
name_reference u32
```

The prefix is followed by a u32-counted UTF-16LE name and a u32-counted UTF-16LE long name. Segment major versions above 16 then store a u16 style state, four u32-counted UTF-16LE strings, two u16 style values, and a 16-byte mixed-endian GUID. The four strings are the style label, Protein asset GUID, Protein material identifier, and Protein asset-library identifier. The remaining rendering fields are a retained suffix.

The document-default rendering style is the rendering-style record selected by the default-style record's one-based reference in the same `PmApp` segment. Its Protein asset GUID and asset-library identifier resolve one unique appearance catalog entry. That appearance is the document default for every transferred part body. A missing, null, cross-carrier, or ambiguous reference does not produce an appearance binding.

A `PmGraphics` record reference stores a one-based record index in bits 0 through 30. Bit 31 is retained as the reference qualifier. Qualified and unqualified references use the same index arithmetic. Every non-null reference resolves within the same `PmGraphics` segment.

A current `PmGraphics` list starts with u16 values `2, 0x3000` and a u32 item count. A nonempty list then contains two u32 metadata values followed by the counted items. A node-reference list stores one u32 record reference per item. An empty list ends after the count and has no metadata values.

A `PmGraphics` record with type identifier `a3e99451d2119b2860006ab72c39cdb0` stores one graphics face. For segment major versions above 14, its payload contains:

```text
header_value u32
header_id u16
flags u32
styles_reference u32
surface_reference u32
parent_reference u32
state u32
edge_references node-reference-list
visibility_state u8
bounds f64[6]
key u32
values u32[2]
```

The `key` is the graphics face's Design-join key. It joins the face to the unique transferred ASM face whose `face.chunk[1]` has the same nonnegative value. Graphics face records with keys absent from the transferred B-rep do not bind active topology.

A `PmGraphics` record with type identifier `0786eb48d2110c076000f99ac5361ab0` stores an object-style collection as one node-reference list. A graphics face's non-null `styles_reference` selects this record by its one-based reference in the same segment.

A `PmGraphics` record with type identifier `0f5648afd411c78d1000d58dc04a0ab5` stores a primary-color style. For segment major versions above 14, its 94-byte payload contains:

```text
header_value u32
controls u16[7]
color_header u8[2]
colors f32[4][4]
color_tail u16[2]
state u8
values u16[2]
terminal_state u8
```

The four colors are RGBA vectors. The second vector is the diffuse color. A style collection supplies a face override when it contains exactly one reference to a primary-color style. The direct-color appearance binds the joined neutral face. A face binding has precedence over the document-default body binding for that face. A missing or ambiguous face, style collection, or primary-color reference does not produce a face binding.

## 11. Part parameters

For segment major versions 15 through 22, a `PmDc` record with type identifier `264d8790d011f8d10008cabc0663dc09` stores one numeric design parameter. Its payload contains:

```text
header_value u32
header_id u16
next_reference u32
flags u32
context_reference u32
source_index u32
name counted UTF-16LE
name_value u32
unit_reference u32
formula_reference u32
nominal_value f64
model_value f64
tolerance u16
terminal_value i16
```

The payload ends after `terminal_value`. PmDc references store a one-based record index in bits 0 through 30. Bit 31 is retained as a qualifier and does not change the index arithmetic. Every non-null parameter, expression, or unit reference resolves within the same PmDc segment.

A numeric expression record starts with `header_value:u32`, `header_id:u16`, and `unit_reference:u32`. Type `047aa7f8d2118f09c0005a9a2378d04f` continues with `value:f64`, `value_type:u16`, and a current-version `value_state:u32`. Type `057aa7f8d2118f09c0005a9a2378d04f` continues with one parameter reference. Types `0c7aa7f8d2118f09c0005a9a2378d04f` and `0d7aa7f8d2118f09c0005a9a2378d04f` continue with one child expression reference and encode unary minus and power identity. Types `067aa7f8`, `077aa7f8`, `087aa7f8`, `097aa7f8`, `0a7aa7f8`, and `0b7aa7f8` with the same remaining 12 identifier bytes continue with two ordered child expression references and encode addition, subtraction, multiplication, division, modulo, and power respectively.

A unit definition record with type identifier `fd79a7f8d2118f09c0005a9a2378d04f` starts with `header_value:u32` and `header_id:u16`. It then stores numerator and denominator reference arrays, a Boolean visibility byte, and a derived-unit reference. Each reference array starts with u16 values `3, 0x3000` and a u32 count. A nonempty array then stores two u16 metadata values and the counted record references. An empty array ends after the count.

One base-unit record selected by a unit definition's sole numerator supplies its dimension and display scale. Base-unit records contain `header_value:u32`, `header_id:u16`, `magnitude:f64`, and `factor:f64`. The supported scalar base units are millimetres (`bc204162d2119b0b60006ab760fec3b0`), metres (`f579a7f8d2118f09c0005a9a2378d04f`), inches (`f679a7f8d2118f09c0005a9a2378d04f`), feet (`f779a7f8d2118f09c0005a9a2378d04f`), radians (`f2cd305cd2113f0d60006ab760fec3b0`), degrees (`f0cd305cd2113f0d60006ab760fec3b0`), degree-equivalent grad units (`f6cd305cd2113f0d60006ab760fec3b0`), and dimensionless values (`23009d5fd2118e09c0005a9a2378d04f`). A transferable scalar unit has one numerator, no denominator, and no derived-unit reference.

Length expression values and parameter model values use internal centimetres. Neutral evaluated lengths multiply the parameter model value by 10 to obtain millimetres. A displayed literal divides its internal value by the base-unit scale: 0.1 for millimetres, 100 for metres, 2.54 for inches, and 30.48 for feet. Angular model values use radians. A displayed degree literal divides its radian value by `pi/180`. Dimensionless values are unchanged.

A neutral parameter transfers only when its unit definition, base unit, and complete expression graph resolve uniquely in the same segment. Parameter-reference expression nodes supply ordered dependency identities. Cyclic, null, cross-segment, ambiguous, non-finite, compound-unit, power-identity, and unresolved expression graphs remain native.

## 12. Planar sketches

For segment major versions 15 through 22, PmDc content records start with this 22-byte header:

```text
header_value u32
header_id u16
next_reference u32
flags u32
context_reference u32
source_index u32
```

Type `114d8790d011f8d10008cabc0663dc09` is a planar sketch. The content header is followed by `state:i32`, `count_value:u32`, a type-8 entity-reference array, transform and direction references, and two u32 values. A trailing type-2 reference list is optional. The entity-reference array contains geometric entities, constraints, and helper records.

A type-8 reference array starts with u16 values `8, 0x3000` and a u32 count. A nonempty array then stores two u16 metadata values and the counted PmDc references. A type-2 reference list starts with u16 values `2, 0x3000` and a u32 count. A nonempty list then stores two u32 metadata values and the counted references.

Planar sketch entities add `entity_flags:u32` and `sketch_reference:u32` after the content header. The sketch reference identifies the owning planar sketch. The construction mask is `0x04080040`; an entity is construction geometry when any masked bit is set.

The planar entity types are:

- `35df52ced011d0d20008ccbc0663dc09`: point. It stores a two-f64 position, endpoint-of and center-of type-2 lists, and an optional u32 state plus type-2 association list.
- `3adf52ced011d0d20008ccbc0663dc09`: line. It stores a type-2 endpoint list, versioned auxiliary type-2 lists, and two-f64 origin and direction vectors.
- `3bdf52ced011d0d20008ccbc0663dc09`: circle. It stores a type-2 point list, versioned auxiliary type-2 lists, a center-point reference, a positive f64 radius, and a u8 state.
- `60d40745d111bee680006fb1e13554c7`: ellipse. It stores a type-2 point list, versioned auxiliary type-2 lists, a center-point reference, a two-f64 major-axis direction, positive f64 major and minor radii, and a u8 state.

Sketch coordinates and radii use internal centimetres. Neutral planar coordinates and lengths multiply these values by 10 to obtain millimetres. A line's ordered endpoint references define its bounded direction. A circle or ellipse forms a closed profile by itself. Non-construction line records form a profile loop when every endpoint in their connected component has degree two and the ordered traversal closes.

Type `184d8790d011f8d10008cabc0663dc09` stores a compressed 4-by-4 sketch transform after the content header. An optional u32 value `0x203` precedes two u16 masks. For matrix element bit `b`, a clear zero-mask bit and clear value-mask bit select one following f64. A clear zero-mask bit and set value-mask bit select positive one. A set zero-mask bit and clear value-mask bit select zero. Both bits set select negative one. Explicit values occur in row-major order. Translation values use internal centimetres.

Type `40df52ced011d0d20008ccbc0663dc09` stores a sketch direction. It adds `entity_flags:u32`, `parameter:f64`, an optional u32 extension, and a three-f64 direction vector after the content header. The transform's first column is the sketch u-axis. Its third column and the direction vector are the same oriented plane normal. The last matrix row is `[0, 0, 0, 1]`.

A planar constraint starts with the content header, `state:i32`, and a group reference. Segment major versions 15 and 16 store the parameter reference immediately after the group reference. Segment major versions 17 through 22 store two type-6 maps before the parameter reference. A type-6 map starts with u16 values `6, 0x3000` and a u32 count. A nonempty map then stores two u32 metadata values. The first map stores reference/f64 pairs. The second map stores reference/reference pairs.

Constraint types `944d8790`, `954d8790`, `964d8790`, `974d8790`, `984d8790`, and `994d8790` with the same remaining 12 identifier bytes as the planar-sketch type encode coincident, parallel, perpendicular, tangent, horizontal, and vertical relations. Coincident stores two entity references. Parallel and perpendicular store two entity references and a u16 orientation. Tangent stores two entity references and an optional u32 extension. Horizontal and vertical store one entity reference and a u8 state.

Types `00c0ac00d1115fe0800066b1e13554c7` and `40ff8336d1115fe0800066b1e13554c7` encode horizontal and vertical distance dimensions. Each stores two entity references, a replacement parameter reference, and four u32 values after the constraint header. Type `00b71b67d11168e0800066b1e13554c7` encodes a radius dimension and stores one u32 state, an entity reference, and four u32 values. Type `e096df74d11169e0800066b1e13554c7` encodes a diameter dimension and stores an auxiliary reference, an entity reference, and four u32 values. The constraint-header parameter drives radius and diameter. The replacement parameter drives horizontal and vertical distance.

Type `008c10e1d11102e680006db1e13554c7` binds a circular entity to its center-point entity. Type `d07d2c44d11189e680006fb1e13554c7` gives two circular entities equal radii. Each stores its two ordered entity references after the constraint header.

## 13. Feature records

For segment major versions 15 through 22, PmDc type `914d8790d011f8d10008cabc0663dc09` stores one feature record. Its fixed prefix contains the 22-byte content header, `state:i32`, `outline_value:u32`, and a type-2 property-reference list prefix. The ordered property references follow the list metadata. A final u32 value follows the reference list and ends the record.

PmDc type `24fd418fd211ac6e00082aab32a3dc09` stores an end-of-features record. Its 26-byte payload contains the 22-byte content header followed by `state:i32`.

Feature property records use the same 22-byte content header. The following enumeration records add `type_value:i16` and `value:u16` and then end:

- `28be9a72d111440900084eba32a3dc09`: part operation;
- `297d6392d1113cb9000831bd0663dc09`: extent;
- `117ccd43d2119658a00021803603c8c9`: hole form;
- `2788f278c54dd7be4313b3986039b52e`: fillet form;
- `7339fdce7a4e4011bee343897908ba92`: auxiliary feature enumeration.

Type `3200aa7dd2112b836000f3a89dccefb0` stores the chamfer form. It adds the same `type_value:i16` and `value:u16` fields, followed by a zero u32 terminal value.

The part-operation, extent, hole-form, fillet-form, and chamfer-form records use type values 5, 11, 3, 2, and 2 respectively.

Type `284d8790d011f8d10008cabc0663dc09` adds a counted UTF-16LE name, `name_value:u32`, and a Boolean byte. The Boolean byte is 0 or 1. Types `91739422d11107cf000835bd0663dc09`, `71f23ed8d2115094a00049803603c8c9`, `ae70680ed14a1e86d76248b03c2a96e1`, and `dae9481bd211dc2c00083eab1b14dc09` add one type-2 reference list. They identify a boundary patch, feature dimensions, an object collection, and constant-radius fillet edge sets respectively.

Type `dfd51dbbd1116e72000817bd0663dc09` adds a counted UTF-16LE name and three u32 values: the name value, nominal value, and model value. Type `474d8790d011f8d10008cabc0663dc09` adds one body reference. Type `3b2477a4d1118f96000826bd0663dc09` adds an entity-link reference and one u8 value. Type `2c9256724d4d6d709427fd964d84df16` adds transform, point, and value references.

A feature label record with type `2ba4482bd2115864600074b79b49ebb0` starts with this 26-byte linked-element header:

```text
header_value u32
header_id u16
values u32[2]
owner_reference u32
parent_reference u32
next_reference u32
```

The header is followed by `index:u32`, a type-2 participant-reference list, a counted UTF-16LE label, and a 16-byte class identifier. The owner reference identifies the labeled record. Each participant is a one-based record reference in the same PmDc segment. Other records with the same type identifier use different payload forms and do not satisfy the label grammar.

Type `154d8790d011f8d10008cabc0663dc09` is an entity-style link and also starts with the linked-element header. It then stores `value:u32`, `associative_id:u32`, and `entity_type:u32`. A profile-selection record selects an entity-style link through its entity-link reference.

Type `1641d6aad211db2c00083eab1b14dc09` stores one constant-radius fillet edge set. The content header is followed by edge-collection, radius, selection-mode, and continuity references. Type `514d8790d011f8d10008cabc0663dc09` stores an edge collection as a type-2 reference list after the content header. The members identify type `82695c37d111516b0008a1ba32a3dc09` edge-item records. An edge item stores a type-2 `u32` index-reference list after the content header, an `i32` when the list is not empty, and a final `u32`.

Type `4a374949d211001d00083bab1b14dc09` stores a fillet edge-selection enumeration. It adds `type_value:u32` and `value:u32` after the content header. Type value 4 and value 0 select explicitly listed edges.

Feature-label class identifiers `3111a90cd0118b83000819b00524dc09`, `dc15f7f1d1114205000830b00524dc09`, `3f7100f9d2118b6f6000f0a89dccefb0`, and `1a7d751fd2119c54a00020803603c8c9` identify extrusion, fillet, chamfer, and hole records respectively.

An extrusion uses property slots 0 through 7 for part operation, boundary patch, direction, reverse direction, forward length, taper angle, extent, and symmetric extent. Slot 23 repeats the boundary-patch reference. Slot 26 identifies the result object collection. Part-operation enumeration values 1, 2, 3, and 4 mean new body, cut, join, and intersection. Extent values 1, 4, and 5 mean fixed dimension, through next, and through all. A fixed dimension uses the slot-4 length. A boundary patch contains profile-selection references. The feature label's sole participant identifies the owning planar sketch.

A constant-radius edge fillet uses slot 0 for the fillet edge-set collection, slot 11 for the fillet form, and slot 15 for the result object collection. Fillet form value 0 is an edge fillet. Each edge set identifies its edge collection and length parameter. Selection type 4 with value 0 lists the exact edge items. The edge set also carries its continuity Boolean.

An equal-distance edge chamfer uses slot 0 for its edge collection, slot 2 for its length, slot 4 for the chamfer form, slot 5 for direction reversal, and slot 11 for the result object collection. Chamfer values 0, 1, and 2 mean equal distance, distance and angle, and two distances. The equal-distance branch does not consume an oriented support face.

A hole uses slots 0 through 6 for hole form, diameter, depth, entry diameter, entry depth, entry angle, and drill-point angle. Slots 8, 9, 16, 17, 21, and 24 identify its transform, extent, a direction, a Boolean, a placement, and the result object collection. Hole-form values 0, 1, 2, and 3 mean drilled, countersink, counterbore, and spotface. The placement transform translation uses internal centimetres. The direction record supplies the drilling direction. The placement record repeats the transform reference and retains its point and value references.

A result object collection contains one or more surface-body references. This ordered surface-body list is the feature's native result-body identity.

Types `44326720d211c51d60002aab01f31bb0` and `b5a9d9fad211053360002cab01f31bb0` store rectangular-pattern and mirror feature records. Both start with the generic 30-byte feature prefix through `outline_value`, followed by a type-2 property-reference list, `value:u32`, a type-2 participant-reference list, six property references, and `control:u8`.

For segment major versions 15 through 20, the rectangular-pattern suffix contains 20 more property references. For segment major versions 21 and 22, it contains 26 more property references. The mirror suffix contains five more property references, six u32 extension values in segment major versions 21 and 22, and two final property references. The record ends after the applicable suffix.

## 14. Document kind

`Pm*` segment families identify a part document. `Am*` segment families identify an assembly document. A document that contains both families has the distinct `mixed_part_assembly` kind. Property metadata can identify a part, assembly, drawing, or presentation only when segment-family evidence does not already identify the kind.
