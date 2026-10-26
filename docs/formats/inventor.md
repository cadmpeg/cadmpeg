# Autodesk Inventor IPT/IAM

This specification defines the byte semantics of the supported Autodesk Inventor IPT/IAM envelope. Multi-byte integers are little-endian unless a section states otherwise.

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/inventor.md`](../layouts/inventor.md), generated from `docs/layouts/inventor.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. `cargo test -p cadmpeg --test layout_tables` proves the two agree.

## 1. Compound container

An IPT or IAM document is a Compound File Binary container. The container follows the CFB sector, FAT, DIFAT, mini-FAT, directory, and sibling-tree rules. The directory contains the `RSeStorage` storage. A positive format identification also requires a structurally reached `RSeStorage/RSeSegInfo` stream or a stream at `RSeStorage/V<n>/RSeDb`, where `<n>` is one or more decimal digits.

The last DIFAT sector's next-sector link is ENDOFCHAIN (`0xfffffffe`). Some Inventor documents store FREESECT (`0xffffffff`) in that link instead. That link is a chain terminator only when the header DIFAT count equals `ceil((fat_count - 109) / (sector_size / 4 - 1))`, the walk visits exactly that many DIFAT sectors, and the walk collects exactly `fat_count` FAT sector identifiers. Such a document decodes and reports `inventor/container.noncanonical` ("CFB DIFAT chain ended with FREESECT instead of ENDOFCHAIN"). A FREESECT link under any other count, an empty DIFAT whose start is not ENDOFCHAIN, and a FREESECT terminator on any FAT chain are malformed.

The database storage-band number `<n>` and the database schema are independent values. An `M<token>` metadata stream and a `B<token>` bulk stream are one segment pair only when both are direct children of `RSeStorage` and their nonempty `<token>` suffixes are byte-equal.

## 2. RSe database and indexes

An `RSeDb` stream starts with a 16-byte database identifier, a u32 schema, an 8-byte creation-version tuple, a u64 creation FILETIME, an 8-byte save-version tuple, a u64 save FILETIME, and a u32-counted UTF-16LE note. A version tuple contains `revision`, `minor`, `major`, and five state bytes. Schema 31 selects the supported registry grammar. The stream ends after the note.

`RSeStorage/RSeSegInfo` starts with a u32 segment count. Each registry entry contains counted UTF-16LE display and type names, segment and revision identifiers, state words, an 8-byte version tuple, object records, and node records. The last object record's u32 node count stores the number of node records plus one; a stored zero, like an entry without object records, is followed by no node records. A segment metadata identifier resolves to exactly one registry entry. The registry type name selects the segment family.

`RSeStorage/RSeDbRevisionInfo` starts with u32 version 3 and a u32 record count. Each record contains a 16-byte identifier, u32 flags, and a u16 kind. Kind `0xffff` is followed by a one-byte selector and an 8-byte value when the selector is nonzero or a 16-byte value when it is zero. Other kinds have no value payload. The stream ends after the declared records.

## 3. RSe metadata stream

An `M<token>` stream starts with a u32 byte count and that many UTF-8 marker bytes. The supported marker is `RSe Meta Stream Version 8`. It is followed by u16 version 8, eight u16 header values, a u32-counted UTF-16LE display name, a 16-byte segment identifier, three u32 state words, two u32-counted UTF-8 timestamp strings, and a one-byte body form. The body form selects compression: method 1 is zlib and method 2 is Zstandard. Other methods are invalid. The compressed body occupies the remainder of the stream. Method 1 contains one complete zlib member with no trailing bytes. Method 2 contains one complete Zstandard frame with no trailing bytes. Concatenated frames and skippable frames are invalid. Truncated frames are invalid.

The inflated body starts with seven u16 values and contains 11 sections plus a terminal 16-byte identifier. Sections 1 through 4 use a forward counted frame:

```text
count u32
items[count]
span u32
```

`span` equals `4 + count * item_size`. Section 1 items are 4-byte block descriptors. Bit 31 states that the corresponding bulk block is stored; bits 0 through 30 give its payload length. Section 2 items are 10 bytes. Section 3 items are 28 bytes. Section 4 items are 28-byte type descriptors. A type descriptor contains a 16-byte type identifier and two `(u16, u32)` field pairs. The section 4 count is not limited to 256; descriptors at index 256 and later have no record selector that addresses them.

Sections 5 through 11 use backward framing. The eight bytes before a section payload contain a u32 span to the previous payload and a u32 discriminator. The section chain joins the section 4 footer exactly. Section 11 has a 72-byte payload. The 16-byte terminal identifier follows section 11.

## 4. RSe bulk stream and records

A `B<token>` stream starts with a 16-byte prefix and a u16 form. The low byte at offset 16 is the envelope value. The high byte at offset 17 is the compression method: 1 is zlib and 2 is Zstandard. Other methods are invalid. The compressed body starts at offset 18 and occupies the remainder. Its compression framing follows section 3. The prefix and full u16 form are retained as envelope values; they do not select a segment by themselves.

Each stored section-1 block descriptor frames one record in block order. A record contains:

```text
selector u32
payload bytes[block.payload_len]
trailing_payload_len u32
versioned_trailer bytes[]
```

The selector low byte indexes the section 4 type table, so a record selects one of the first 256 type descriptors. `trailing_payload_len` is zero or equals `block.payload_len`. Segment major versions above 18 have an extended trailer. Its presence byte selects an empty trailer or a typed property and reference list. After all records, the expanded stream has a u32 `0xffffffff` marker and a retained trailer. The stream is exhausted exactly.

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

`UFRxDoc` schemas 11 through 15 start with a u16 schema and a u16 section-version count. The section-version table governs optional header fields. The schema-15 representation/model-state branch adds a u16 representation prefix. An assembly then stores two counted UTF-16LE representation strings. A part omits these two strings. Both document kinds continue with a two-u16 secondary LOD state, a counted UTF-16LE active model-state name, and a two-u16 model-state pair; they omit the older header-version-flags field. Schema 15 then stores a u32 and a u16 header state, one more u32, and the u16 invariant 1. The model-state table precedes the external-reference table. Each model-state record contains a u8 prefix, counted UTF-16LE name, two-u16 state pair, u32 prefix count, u32 parameter count, the counted parameter records, and a suffix. When section-version entry 2 is 16 or earlier, the suffix has 77 bytes; when it is 18 or later, it has 81 bytes. Each parameter record contains a counted UTF-16LE name, u8 tag, u16 kind, u16 state, counted UTF-16LE value, and u16 trailer.

Schemas before 15 store level-of-detail records at the same position, after the u32 LOD count. Each record contains a u8 prefix, counted UTF-16LE name, two-u16 state pair, u32 member count, the members, and a zero-byte record terminator. A member contains a counted UTF-16LE occurrence key, a u8 kind, and a zero-byte member trailer. Kind 1 has no payload. Kind 2 inserts a two-u16 state pair and a counted UTF-16LE level-of-detail name of the referenced subassembly before the trailer. Other kinds are not implemented. The member keys are the decimal form of the occurrence key described below.

The external-reference table contains counted UTF-16LE paths and names, state groups, 16-byte document and database identifiers, a u32 reference identifier, u32 occurrence count, u32 version, and u32 flags. Section-version entry 4 values of 2 or later add a zero-byte table terminator. Persisted paths remain unresolved. The codec does not open them.

The embedded-reference table follows the external-reference table. It starts with a u32 record count. Each record contains a u32 value, u64 FILETIME, u32 value, an additional u32 value when section-version entry 15 is 7 or later, another u32 value, counted UTF-16LE path, i32 library identifier, counted UTF-16LE library name, u16 state, counted UTF-16LE display name, and eight state bytes. Section-version entry 15 values of 6 or later add a zero-byte table terminator.

The occurrence table follows the embedded-reference table. It starts with a u32 count and a reserved u32 that is 0. The records follow. Each record starts with:

```text
reference_id u32
occurrence_id u32
occurrence_key u32
header_value u32
title_count u32
```

`reference_id` equals the `reference_id` of the external reference that the occurrence instantiates. `occurrence_id` is the identifier of the occurrence's `AmDc` and `AmGraphics` records. `occurrence_key` is a dense per-document key that level-of-detail members name. `header_value` is the occurrence's instance number among the occurrences of the same component. For occurrence section versions before 28, `title_count` is the code-unit count of the UTF-16LE title that follows, and 0 stores no title. The header then contains five state bytes, one additional byte at version 20 or later, and another byte at version 21 or later. At version 28 or later, every nonzero `title_count` is followed by a separately counted UTF-16LE title. The header then contains at most eight zero u16 values, u16 marker `0x2080`, and the u32 sequence 0, 1, 0.

Two property sections follow the header. Each starts with a u32 section value and u32 property count. A property contains a Boolean presence byte, u8 type tag, u32 value, the repeated type tag, the tag-selected value, and a u32 trailer. In the first property section, tag `0x03` holds the 16-byte document identifier of the component the occurrence instantiates, and tag `0x05` holds the component file name without its directory and extension. In either property section, tag `0x2e` holds a 16-byte identifier. Settings follow as counted UTF-16LE name, 16-byte identifier, and counted UTF-8 value records. The export section contains ten state bytes, version-selected padding, and either an empty count, a `0x00ffffff` or `0xffffffff` sentinel followed by zero u32, or a bounded typed export table. Saves from 2025 onward store no padding after the ten state bytes; they end the export section with a padding byte and a u32. Each export entry contains a counted UTF-16LE name, the typed item list, eight state bytes, a u32 linked-name count, that many counted UTF-16LE names of other export entries, and, for saves from 2015 onward, one padding byte. Export item tag `0x1a` carries a 16-byte identifier. Each occurrence record is bounded by exhaustion of these nested counts.

## 9. Assembly records

An `AmDc` record with type identifier `604d8790d011f8d10008cabc0663dc09` stores one assembly occurrence identity. Its payload contains:

```text
header_value u32
header_id u16
next_reference u32
flags u32
flags_extension u32 = 0  from segment major version 28
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

The record ends after the trailer. Before segment major version 28 the fields from header_value through related_count occupy 50 bytes. From segment major version 28 the flags_extension field makes them occupy 54 bytes. Occurrence identifiers are document-local values and are not required to be dense.

An `AmGraphics` record with type identifier `a26371cad011b2d30008bfbb21eddc09` or `07d0d0b9d4112d5f6000f8830e73fcb0` stores an assembly placement. Its common payload prefix contains u32 zero, u16 header identifier, u32 owner reference, u32 attribute reference, u8 state, and a compact 4-by-4 transform. The transform can start with optional u32 `0x00000203`, followed by a u16 set mask and a u16 zero mask. The 16 matrix elements use row-major bit positions. A clear zero-mask bit stores an f64 when the set-mask bit is clear and represents `1` when the set-mask bit is set. A set zero-mask bit represents `0` when the set-mask bit is clear and `-1` when the set-mask bit is set. Stored f64 values are finite.

The active placement branch continues with u8 branch, u8 graphics state, u32 occurrence identifier, counted UTF-16LE label `"GRx"`, u16 invariant 1, u32 graphics index, u32 object reference, and the occurrence identifier again. Both occurrence-identifier fields are equal. The remaining branch-specific suffix is retained. A placement joins an occurrence through the exact occurrence identifier.

A `UFRxDoc` occurrence joins the `AmDc` and `AmGraphics` records through `occurrence_id`. It joins its external prototype through `reference_id`: the one external reference with the same `reference_id` is the prototype. Copies of one file keep its document identifier, so the document identifier alone does not select a copy. When the occurrence stores a first-section document identifier, the prototype must store the same document identifier. Otherwise, and when no reference or several references have the `reference_id`, the occurrence is unresolved. The external-reference occurrence count equals the number of joined `UFRxDoc` occurrences. Each occurrence in the current document is a root placement. A referenced assembly remains an unresolved external prototype; its internal occurrences are not inserted into the current document.

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

A `PmDc` record with type identifier `264d8790d011f8d10008cabc0663dc09` stores one numeric design parameter. In segment major versions 15 through 22 its payload contains:

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

The payload ends after `terminal_value`. The first six fields are the content header of section 12; later segment major versions extend it as that section states. PmDc references store a one-based record index in bits 0 through 30. Bit 31 is retained as a qualifier and does not change the index arithmetic. Every non-null parameter, expression, or unit reference resolves within the same PmDc segment.

A numeric expression record starts with `header_value:u32`, `header_id:u16`, and `unit_reference:u32`. Type `047aa7f8d2118f09c0005a9a2378d04f` continues with `value:f64`, `value_type:u16`, and a current-version `value_state:u32`. Type `057aa7f8d2118f09c0005a9a2378d04f` continues with one parameter reference. Types `0c7aa7f8d2118f09c0005a9a2378d04f` and `0d7aa7f8d2118f09c0005a9a2378d04f` continue with one child expression reference and encode unary minus and an explicit parenthesized group. A group's value is its child's value, and its text is the child's text in parentheses. Type `037aa7f8d2118f09c0005a9a2378d04f` encodes a function call. After the unit reference it stores a type-2 reference list of argument expressions and `function:u32`. Functions 1, 2, and 3 are the cosine, sine, and tangent of their one angle argument, written `cos(…)`, `sin(…)`, and `tan(…)`. Types `067aa7f8`, `077aa7f8`, `087aa7f8`, `097aa7f8`, `0a7aa7f8`, and `0b7aa7f8` with the same remaining 12 identifier bytes continue with two ordered child expression references and encode addition, subtraction, multiplication, division, modulo, and power respectively.

A unit definition record with type identifier `fd79a7f8d2118f09c0005a9a2378d04f` starts with `header_value:u32` and `header_id:u16`. It then stores numerator and denominator reference arrays, a Boolean visibility byte, and a derived-unit reference. Each reference array starts with u16 values `3, 0x3000` and a u32 count. A nonempty array then stores two u16 metadata values and the counted record references. An empty array ends after the count.

One base-unit record selected by a unit definition's sole numerator supplies its dimension and display scale. Base-unit records contain `header_value:u32`, `header_id:u16`, `magnitude:f64`, and `factor:f64`. The supported scalar base units are millimetres (`bc204162d2119b0b60006ab760fec3b0`), metres (`f579a7f8d2118f09c0005a9a2378d04f`), inches (`f679a7f8d2118f09c0005a9a2378d04f`), feet (`f779a7f8d2118f09c0005a9a2378d04f`), radians (`f2cd305cd2113f0d60006ab760fec3b0`), degrees (`f0cd305cd2113f0d60006ab760fec3b0`), degree-equivalent grad units (`f6cd305cd2113f0d60006ab760fec3b0`), dimensionless values (`23009d5fd2118e09c0005a9a2378d04f`), and unitless values (`22009d5fd2118e09c0005a9a2378d04f`). Unitless values are dimensionless and display the symbol `ul`. A transferable scalar unit has one numerator, no denominator, and no derived-unit reference.

Length expression values and parameter model values use internal centimetres. Neutral evaluated lengths multiply the parameter model value by 10 to obtain millimetres. A displayed literal divides its internal value by the base-unit scale: 0.1 for millimetres, 100 for metres, 2.54 for inches, and 30.48 for feet. Angular model values use radians. A displayed degree literal divides its radian value by `pi/180`. Dimensionless values are unchanged.

A neutral parameter transfers only when its unit definition, base unit, and complete expression graph resolve uniquely in the same segment. Parameter-reference expression nodes supply ordered dependency identities. Cyclic, null, cross-segment, ambiguous, non-finite, compound-unit, and unresolved expression graphs remain native.

A parameter's `context_reference` selects the parameter table that owns it. A parameter table is a PmDc record with type identifier `634d8790d011f8d10008cabc0663dc09`. Its payload contains:

```text
header_value u32
header_id u16
next_reference u32
flags u32
flags_extension u32      from segment major version 28
context_reference u32
members type-2 reference list
names type-6 name map
suffix bytes[]
```

A type-6 name map starts with u16 values `6, 0x3000` and a u32 count. A nonempty map then stores two u32 metadata values. Each entry is a counted UTF-16LE parameter name followed by one PmDc reference. The member list and the name map both contain every parameter of the table. The suffix is retained.

The table's `context_reference` selects the model root that owns the table. A null table context selects no model. A model root is a PmDc record with type identifier `164d8790d011f8d10008cabc0663dc09`. Its payload contains:

```text
header_value u32
header_id u16
next_reference u32
flags u32
flags_extension u32      from segment major version 28
identifier bytes[16]
name counted UTF-16LE
members type-2 reference list
values u32[15]
brep_segment counted UTF-16LE
scale f64
terminal_value u32
```

The payload ends after `terminal_value`. Value 3 is the PmDc reference to the root's parameter table. Value 9 is null or the same reference. `brep_segment` is the display name of the segment that stores the model's body. The document model root names the `PmBrepSegmentType` segment. The flat-pattern model root names the `MbBrepSegmentType` segment; the flat-pattern model and its `MbBrepSegmentType` and `MbGRxSegmentType` segments are present only when the part has a flat pattern.

Parameter ownership requires exactly one document model root in the segment, exactly one segment with the display name the root names, and a name-map entry that references the parameter record under the parameter's own name. Parameters in a table whose context is the document model root are document parameters. Parameters in the parameter table of a flat-pattern model root belong to that flat-pattern model, which is one native feature after every document feature. The flat-pattern table repeats the names of the document sheet-metal parameters. Parameters in a table with a null context, or in a table whose model root does not resolve, have no established owner and remain native.

## 12. Planar sketches

PmDc records follow the grammar of this section and sections 11, 13, and 14 in segment major versions 15 through 23, 25, and 28 through 31. In segment major versions 15 through 22, PmDc content records start with this 22-byte header:

```text
header_value u32
header_id u16
next_reference u32
flags u32
context_reference u32
source_index u32
```

Later segment major versions extend the header. From version 23, i32 stamps follow `source_index`: one in version 23 and two from version 25. From segment major version 28, PmDc content records start with this 26-byte header, and the stamps follow it:

```text
header_value u32
header_id u16
next_reference u32
flags u32
flags_extension u32 = 0
context_reference u32
source_index u32
```

Records whose body starts with a state value store no stamps: planar sketches, sketch constraints, features, pattern features, and end-of-features records. A stamp is −1 or a nonnegative value. Model roots and parameter tables, which have no context and source fields, store `flags_extension` after `flags` from version 28.

Type `114d8790d011f8d10008cabc0663dc09` is a planar sketch. The content header is followed by `state:i32`, `count_value:u32`, a type-8 entity-reference array, transform and direction references, and two u32 values. A trailing type-2 reference list is optional. The entity-reference array contains geometric entities, constraints, and helper records.

PmDc references store a one-based record index in bits 0 through 30 and a qualifier in bit 31. The qualifier does not change the selected record. A zero index is null.

A type-8 reference array starts with u16 values `8, 0x3000` and a u32 count. A nonempty array then stores two u16 metadata values and the counted PmDc references. A type-2 reference list starts with u16 values `2, 0x3000` and a u32 count. A nonempty list then stores two u32 metadata values and the counted references.

Planar sketch entities add `entity_flags:u32` and `sketch_reference:u32` after the content header. The sketch reference identifies the owning planar sketch.

The planar entity types are:

- `35df52ced011d0d20008ccbc0663dc09`: point. It stores a two-f64 position, endpoint-of and center-of type-2 lists, and an optional association tail.
- `3adf52ced011d0d20008ccbc0663dc09`: line. It stores a type-2 endpoint list, versioned auxiliary type-2 lists, and two-f64 origin and direction vectors.
- `3bdf52ced011d0d20008ccbc0663dc09`: circle. It stores a type-2 point list, versioned auxiliary type-2 lists, a center-point reference, a positive f64 radius, and a u8 state.
- `60d40745d111bee680006fb1e13554c7`: ellipse. It stores a type-2 point list, versioned auxiliary type-2 lists, a center-point reference, a two-f64 major-axis direction, positive f64 major and minor radii, and a u8 state.

A point tail is absent or starts with `state:u32`. From segment major version 25, state 2 selects the paired association form. This form is:

```text
state u32 = 2
scalar_marker u16[2] = [2, 0x3000]
scalar_count u32 = 2
scalar_metadata u32[2]
scalar_values f64[2]
operand_marker u16[2] = [2, 0x3000]
operand_count u32 = 2
operand_metadata u32[2]
first_auxiliary_marker u16[2] = [2, 0x3000]
first_auxiliary_count u32 = 0
first_operand u32
second_auxiliary_marker u16[2] = [2, 0x3000]
second_auxiliary_count u32 = 0
second_operand u32
```

The metadata words are opaque u32 values. The scalar values are finite and have no interval restriction. The two operands are packed PmDc entity references with the index and qualifier fields of section 11. The paired association tail ends the point record. Before segment major version 25, each state value precedes a type-2 association-reference list that ends the record. From version 25, state values other than 2 precede a type-2 association-reference list and a type-2 link-reference list. The stored position defines the solved point coordinates.

The auxiliary lists of a line, circle, or ellipse are one type-2 list, or a u32 gate pair followed by its lists. Gate `1, 0` precedes one or two type-2 lists. Gate `0, 0` stores no list.

Sketch coordinates and radii use internal centimetres. Neutral planar coordinates and lengths multiply these values by 10 to obtain millimetres. The first two members of a line's endpoint list are its start and end point references and define its bounded direction. In a line, circle, or ellipse point list, each member after the second is a point record that lies on the curve.

The point list of a circle or ellipse starts with two members. Two null references mark a closed curve. Two point references mark a bounded curve: the first is its start point and the second its end point. A circle with state 1 is a full circle and has two null leading members. A circle with state 0 is an arc and has two leading point references. An arc runs counterclockwise about the sketch normal from its start point to its end point, on the circle of the stored center and radius. Its end points lie on that circle within the kernel linear tolerance, `1e-6` internal centimetres.

A full circle or a closed ellipse forms a closed profile by itself. Line and arc records form a profile loop when every endpoint in their connected component has degree two and the ordered traversal closes.

Type `184d8790d011f8d10008cabc0663dc09` stores a compressed 4-by-4 sketch transform after the content header. An optional u32 value `0x203` precedes two u16 masks. For matrix element bit `b`, a clear zero-mask bit and clear value-mask bit select one following f64. A clear zero-mask bit and set value-mask bit select positive one. A set zero-mask bit and clear value-mask bit select zero. Both bits set select negative one. Explicit values occur in row-major order. Translation values use internal centimetres.

Type `40df52ced011d0d20008ccbc0663dc09` stores a sketch direction. It adds `entity_flags:u32`, `parameter:f64`, an optional u32 extension, and a three-f64 direction vector after the content header. The transform's first column is the sketch u-axis. Its third column and the direction vector are the same oriented plane normal. The last matrix row is `[0, 0, 0, 1]`.

A planar constraint starts with the content header, `state:i32`, and a group reference. Segment major versions 15 and 16 store the parameter reference immediately after the group reference. Segment major versions 17 through 22 store two type-6 maps before the parameter reference. A type-6 map starts with u16 values `6, 0x3000` and a u32 count. A nonempty map then stores two u32 metadata values. The first map stores reference/f64 pairs. The second map stores reference/reference pairs.

Coincident, tangent, distance, circle-center, and midpoint relations can store a solver hint in the two constraint maps: in each map, at most one entry for each of the relation's two operands. A scalar entry is a position on that operand: a normalized line parameter, or an unwrapped angle in radians about a circle center. A reference-map value is null, except in a tangent relation, where it is the unqualified value 0, 1, or 2. For a line operand, 1 and 2 state that the tangency is at the start or the end of the line, and 0 that it is between them. The hint does not change the relation.

Constraint types `944d8790`, `954d8790`, `964d8790`, `974d8790`, `984d8790`, and `994d8790` with the same remaining 12 identifier bytes as the planar-sketch type encode coincident, parallel, perpendicular, tangent, horizontal, and vertical relations. Coincident stores two entity references. Parallel and perpendicular store two entity references and a u16 orientation. Tangent stores two entity references and an optional u32 extension. Horizontal and vertical store one entity reference and a u8 state.

Types `00c0ac00d1115fe0800066b1e13554c7` and `40ff8336d1115fe0800066b1e13554c7` encode horizontal and vertical distance dimensions. Each stores two entity references, a replacement parameter reference, and four u32 values after the constraint header. Type `00b71b67d11168e0800066b1e13554c7` encodes a radius dimension and stores one u32 state, an entity reference, and four u32 values. Type `e096df74d11169e0800066b1e13554c7` encodes a diameter dimension and stores an auxiliary reference, an entity reference, and four u32 values. The constraint-header parameter drives radius and diameter. The replacement parameter drives horizontal and vertical distance.

Type `008c10e1d11102e680006db1e13554c7` binds a circular entity to its center-point entity. Type `d07d2c44d11189e680006fb1e13554c7` gives two circular entities equal radii. Type `e0c281c6d11110e680006eb1e13554c7` gives two lines equal lengths. Each stores its two ordered entity references after the constraint header.

Type `58850511d211e39560000cb38932edb0` encodes a distance dimension. After the constraint header it stores `value:u32`, two entity references, and six f64 values. The constraint-header parameter drives the Euclidean distance between the two entities: between two points, from a point to a line's carrier, or between two parallel line carriers.

Type `100a0d59d111cae680006fb1e13554c7` encodes an angle dimension between two lines. After the constraint header it stores `value:u32`, two line references, two f64 values, and `flags:u16`. The constraint-header parameter, in radians, is the angle between the two line directions or its supplement.

Types `bf70e821d011d0d20008ccbc0663dc09`, `b07b170fd11196e680006fb1e13554c7`, `c0a3558fd1115ee0800066b1e13554c7`, `5052da64d1115ee0800066b1e13554c7`, `e07b9a5ad1115de0800066b1e13554c7`, and `107cdd0dd1119ce680006fb1e13554c7` encode midpoint, fix, horizontal-alignment, vertical-alignment, collinear, and symmetric relations. Their constraint-header parameter is null. After the constraint header:

- midpoint stores a line or arc reference and a point reference, followed from save version 20 by `value:u32`; the point is the midpoint of the curve;
- fix stores one entity reference;
- horizontal alignment stores two point references; the points have the same v coordinate;
- vertical alignment stores two point references; the points have the same u coordinate;
- collinear stores two line references and `state:u16`; the lines lie on one infinite line;
- symmetric stores two entity references and an axis line reference; the axis line's carrier reflects the first entity onto the second.

Type `384853529f445255e1e1dab410eb06c4` is a regular-polygon record. It starts with the planar-entity prefix and then stores a reference-entity reference and `value:u32`. The reference entity is the polygon's center point, or a line. Type `1f89dbfbd84778b6b0967285c7ec8bb9` is a polygon relation. After the constraint header it stores two entity references, the polygon's reference entity, the polygon-record reference twice, and two u32 values. The two entities are consecutive polygon edges or consecutive polygon vertices.

Type `bc144b4ed211fea4600010b38932edb0` encodes an equal-distance relation. After the constraint header, whose parameter is null, it stores two pairs of line references. The lines of each pair are parallel, and the two pairs have the same separation.

Sketch blocks use three record types. Type `4c12f14c1d4f0fb4d3f80f8244528fcb` is a block instance. It starts with the planar-entity prefix and then stores a block-definition reference, a block-group reference, u16 value and zero masks, the explicit values of a 3-by-3 placement matrix, and `value:u32`. The masks select the matrix cells in row-major order with the rule of the sketch transform record; mask bits above bit 8 are clear. Type `7e71a918ee49612a268c549c6584f7fe` is a block member link. It starts with the planar-entity prefix and then stores a type-2 list of the block instance and the reference of one sketch point or line that the instance contains. Type `cf8d56f2324a27ddb666f1894a9d1617` is a block group. It stores the constraint header without its parameter reference, followed by a type-2 list of the block instance and its member links.

A block instance places the curves of its block definition in the sketch that lists the instance. The sketch reference of each definition entity is the block definition. The placement matrix maps the definition point `(x, y, 1)` to the sketch point in internal centimetres, and its upper-left 2-by-2 part is orthonormal. A placed arc runs counterclockwise in the sketch, so a matrix with negative determinant exchanges the arc's start and end points. The instance gives the curves it places associative identifiers that no entity-style link stores. The first identifier is the instance's associative identifier plus 2. The full circles of the definition take one identifier each, in record order. Then each other curve of the definition, in record order, takes one identifier, followed by one identifier for each of its two end points that has no identifier yet.

## 13. Feature records

PmDc type `914d8790d011f8d10008cabc0663dc09` stores one feature record. Its fixed prefix contains the 22-byte content header, `state:i32`, `outline_value:u32`, and a type-2 property-reference list prefix. The ordered property references follow the list metadata. A final u32 value follows the reference list and ends the record.

PmDc type `24fd418fd211ac6e00082aab32a3dc09` stores an end-of-features record. Its 26-byte payload contains the 22-byte content header followed by `state:i32`. Section 12 states the header extensions of later segment major versions.

Feature property records use the same 22-byte content header. The following enumeration records add `type_value:i16` and `value:u16` and then end:

- `28be9a72d111440900084eba32a3dc09`: part operation;
- `297d6392d1113cb9000831bd0663dc09`: extent;
- `117ccd43d2119658a00021803603c8c9`: hole form;
- `2788f278c54dd7be4313b3986039b52e`: fillet form;
- `7339fdce7a4e4011bee343897908ba92`: auxiliary feature enumeration.

Types `1990e1b8d311467bc000e39545df724f` and `1790e1b8d311467bc000e39545df724f` use the same enumeration grammar. Their type values are 3 and 4 respectively. Both use the versioned content header of section 12, including its stamps.

Type `3200aa7dd2112b836000f3a89dccefb0` stores the chamfer form. It adds the same `type_value:i16` and `value:u16` fields, followed by a zero u32 terminal value.

The part-operation, hole-form, fillet-form, and chamfer-form records use type values 5, 3, 2, and 2 respectively. Extent records use type value 11 through segment major version 21 and type value 12 from segment major version 22.

Type `284d8790d011f8d10008cabc0663dc09` adds a counted UTF-16LE name, `name_value:u32`, and a Boolean byte. The Boolean byte is 0 or 1. Types `91739422d11107cf000835bd0663dc09`, `71f23ed8d2115094a00049803603c8c9`, `ae70680ed14a1e86d76248b03c2a96e1`, and `dae9481bd211dc2c00083eab1b14dc09` add one type-2 reference list. They identify a boundary patch, feature dimensions, an object collection, and constant-radius fillet edge sets respectively.

Type `f1b2bc24d01121dd0008d0bc0663dc09` stores a face collection. After the content header it stores one type-2 reference list and ends. Each non-null member selects a record in the same PmDc segment.

Type `ccb59f50d1114f6b0008a1ba32a3dc09` stores a face item. After the content header it stores a type-2 `u32` index-reference list. An empty list selects no face and ends the record without a trailer. A nonempty list is followed by `terminal_value:i32 = -1`, then the record ends. The index values are native topology selectors, not one-based PmDc record references.

Type `7a25f2efd11105af000847b00524dc09` stores a selection list. After the content header it stores one type-2 reference list and ends. Each non-null member selects a record in the same PmDc segment.

Type `9f8f2a40d21120e66000aaabc8433cb0` stores a Boolean with an empty name. It starts with the versioned content header of section 12, including its stamps. The header is followed by `name_count:u32 = 0`, `name_value:u32 = 0`, and `value:u8`. The value is 0 or 1 and ends the record.

Type `dfd51dbbd1116e72000817bd0663dc09` adds a counted UTF-16LE name and three u32 values: the name value, nominal value, and model value. Type `474d8790d011f8d10008cabc0663dc09` adds one body reference. Type `3b2477a4d1118f96000826bd0663dc09` adds an entity-link reference and one u8 value. Type `2c9256724d4d6d709427fd964d84df16` adds transform, point, and value references.

Before segment major version 28, a feature label record with type `2ba4482bd2115864600074b79b49ebb0` starts with this 26-byte linked-element header:

```text
header_value u32
header_id u16
values u32[2]
owner_reference u32
parent_reference u32
next_reference u32
```

From segment major version 28, the record starts with this 30-byte linked-element header:

```text
header_value u32
header_id u16
values u32[2]
flags_extension u32 = 0
owner_reference u32
parent_reference u32
next_reference u32
```

The header is followed by `index:u32`, a type-2 participant-reference list, a counted UTF-16LE label, and a 16-byte class identifier. The label's code units can include unpaired surrogates. The owner reference identifies the labeled record. Each participant is a one-based record reference in the same PmDc segment. Other records with the same type identifier use different payload forms and do not satisfy the label grammar.

Type `154d8790d011f8d10008cabc0663dc09` is an entity-style link and also starts with the linked-element header. It then stores `value:u32`, `associative_id:u32`, and `entity_type:u32`. An entity-style link whose owner is a planar sketch entity gives that entity its associative identifier within the sketch. Entity type 1 marks a point, entity type 2 marks a curve, and entity type `0x08000000` marks a sketch block instance. A profile-selection record selects an entity-style link or a kernel-entity link through its entity-link reference.

Type `21750fccd1112780e38619962259017a` is a kernel-entity link. Its payload contains:

```text
header_value u32
header_id u16
selection_source u32
state u8
body_reference u32
entity_id u32
entity_type u32
```

The body reference selects a PmDc kernel-body record of type `5c5945f6d5113313100060a6bba647b5`. The entity identifier selects one entity of that body. `selection_source` equals the content-header source index of the selection record that references the link.

A PmDc kernel-body record uses the record envelope and footer of the active part kernel carrier (§5). Its footer selected key is the record's own qualified reference. Its kernel bytes hold ASM profile bodies. A profile body contains wires or planar faces. The kernel-entity link of a profile selection selects the body whose ASM body key equals the link's entity identifier. Each wire or face boundary loop of that body is a profile boundary ring. Face loops store their edges through the ordered coedge references. Model-space lengths in these bodies use internal centimetres.

A profile body holds one or more closed loops for each selected key. Each of its edges carries one ASM naming attribute. The class chain `NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib` stores five common attribute fields, followed by the integers `naming_type`, 0, `associative_id`, 0, and 2. The class chain `ATTRIB_CUSTOM-attrib` stores the same five common fields, the string `INV_NMX_GEN_TAG`, integer 1, and the same five naming integers. The value 2 marks an edge name. `naming_type` equals the entity type of the kernel-entity link. `associative_id` equals the associative identifier of the sketch curve that the edge comes from, in the sketch that the extrusion consumes: the identifier of the curve's curve-type entity-style link, or the identifier that a block instance gives a curve it places. The end points of each edge lie on that curve in the sketch plane, and the edge runs along the part of the curve between them. On a full circle, the point at the middle of the edge's parameter interval selects which of the two arcs between the end points the edge runs along. An edge whose end points lie at the end points of its curve runs along the whole curve. A full circle can run through more than one edge. Each of these edges carries the name of the circle, and the circle is then the only curve of the wire. Consecutive edges of a wire share an end point, and the last edge ends where the first one starts.

Type `1641d6aad211db2c00083eab1b14dc09` stores one constant-radius fillet edge set. The content header is followed by edge-collection, radius, selection-mode, and continuity references. From segment major version 23 a fifth reference follows; it is null or a `284d8790d011f8d10008cabc0663dc09` Boolean record. Type `514d8790d011f8d10008cabc0663dc09` stores an edge collection as a type-2 reference list after the content header. The members identify type `82695c37d111516b0008a1ba32a3dc09` edge-item records. An edge item stores a type-2 `u32` index-reference list after the content header, an `i32` when the list is not empty, and a final `u32`.

Type `4a374949d211001d00083bab1b14dc09` stores a fillet edge-selection enumeration. It adds `type_value:u32` and `value:u32` after the content header. Type value 4 and value 0 select explicitly listed edges.

Feature-label class identifiers `3111a90cd0118b83000819b00524dc09`, `dc15f7f1d1114205000830b00524dc09`, `3f7100f9d2118b6f6000f0a89dccefb0`, and `1a7d751fd2119c54a00020803603c8c9` identify extrusion, fillet, chamfer, and hole records respectively. Feature-label class identifier `b54dd6f6d311c4966000f2b21d6eefb0` identifies a sheet-metal corner round. Its feature record uses the constant-radius edge-fillet slots and rounds the selected edges. Type `08dcddc3d311d0976000a7a99dccefb0` is a sheet-metal flange record. After the content header it stores `state:i32`, a type-2 reference list, and a u32 value. The list names the plate, bend, and corner feature records that form the flange, in that order. Type `904bc1c4d311ff2860004da99dccefb0` is a sheet-metal face record with the same layout; its list names one plate feature record. The face label's participants are the face's planar sketch and that plate.

A feature label with class identifier `c65f5855d311730c60003aa99dccefb0` labels a sheet-metal plate. A plate uses property slots 0 through 3 for its profile selection, direction, thickness-side control, and thickness parameter, slot 7 for an edge collection, and slot 8 for its feature dimensions. Type `473f20fcd11101cf000835bd0663dc09` is the plate profile selection and has the profile-selection layout. The direction is the normal of the face sketch. A Boolean thickness-side control selects thickness along this normal when false and against it when true. Type `69b9505efb4bbaa925c3dfa870817810` is a thickness-side enumeration. Its versioned content header, including stamps, is followed by `type_value:i16` and `value:u16`, then the record ends. Type value 2 and value 0 select thickness along the normal.

A plate with a surface-body reference in slot 6 has that body as its result. The result-collection plate form has 17 properties, null slot 6, a result object collection in slot 15, and a part-operation enumeration in slot 16. Part-operation value 1 creates a new body. With no bend operands, the plate is the closed profile region of its face sketch, thickened from the sketch plane by its thickness parameter on the selected side. A plate that names a record in slot 4 or in slots 9 through 13 is joined to an existing face by a bend.

An extrusion uses property slots 0 through 7 for part operation, boundary patch, direction, reverse direction, forward length, taper angle, extent, and symmetric extent. Slot 23 repeats the boundary-patch reference. Slot 26 identifies the result object collection. Part-operation enumeration values 1, 2, 3, and 4 mean new body, cut, join, and intersection. Extent values 1, 2, 4, 5, 6, and 7 mean fixed dimension, symmetric fixed dimension, through next, through all, from-to, and to. A fixed dimension uses the slot-4 length. Extent 2 occurs only with a true symmetric-extent Boolean, and its length is the total travel split evenly around the profile plane. A true symmetric-extent Boolean with extent 5 extends through all material on both sides of the profile plane. Other extents with a true symmetric-extent Boolean are outside the supported branch. A boundary patch contains profile-selection references. The feature label's sole participant identifies the owning planar sketch. Each profile selection is one or more closed rings of whole curves or parts of curves of that sketch. The rings of one boundary patch bound the extruded region: a selected ring that lies inside an odd number of other selected rings bounds a hole in the region of the innermost selected ring that contains it.

The extent-7 target is the work plane in slot 11, or the model face recorded by a termination flag in slot 13 and a surface in slot 18. Exactly one of the two forms is present: a nonzero slot 11 occurs with null slots 13 and 18. Both forms store a plane and no face identity. The extrusion ends at that plane. The extent-6 extrusion starts at the work plane in slot 8, or at the face recorded by the termination flag in slot 10 and the surface in slot 17, and ends at the extent-7 target. The start target has the same two forms, and it also stores a plane and no face identity. A two-sided extrusion has an extent enumeration in slot 29. Slots 27, 28, and 29 hold the length, taper angle, and extent of the side opposite the extrusion direction.

Type `fd24893dd211fa99000840ab0663dc09` is a termination flag. It adds one `value:u8` after the content header. A face target requires value 1.

Type `f0801215d4119e8eef90a8ac7f9ac3ff` is a surface record. After the content header it stores three record references, `state:u8`, `form:u32`, and `value:u32`. Form 25 is a plane: nine f64 values follow and end the record. They are the origin, the u axis, and the v axis in internal centimetres. The plane normal is the cross product of the u and v axes.

Type `42df52ced011d0d20008ccbc0663dc09` is a work plane. After the content header it stores `entity_flags:u32` and two u32 values. Segment major versions above 21 add one u32 value. An optional u32 `0x203` and one u32 value follow. The record ends with nine f64 values: the origin, the u axis, and the v axis in internal centimetres. The plane normal is the cross product of the u and v axes.

A constant-radius edge fillet uses slot 0 for the fillet edge-set collection, slot 11 for the fillet form, and slot 15 for the result object collection. Fillet form value 0 is an edge fillet. Each edge set identifies its edge collection and length parameter. Selection type 4 with value 0 lists the exact edge items. The edge set also carries its continuity Boolean. For a sheet-metal corner round, a null continuity reference supplies no additional continuity condition; the edge collection, explicit selection mode, and constant radius still define the edge fillet.

Type `534d8790d011f8d10008cabc0663dc09` stores an edge naming selector. An edge item's index-reference values name selector `source_index` values in the same PmDc segment. They are not record ordinals or ASM entity indexes.

The fixed selector prefix after the content header contains `state:i32`, `enabled:u8`, `form:u8`, a body reference, `topology:u32`, `qualifier:u32`, and `naming_form:u32`. The direct edge form stores −1, 1, 5, a surface-body reference, 2, 0, and 2. A type-8 integer array follows. Its sole value is the generating feature's content-header `source_index`. The next type-8 integer array is empty. A zero byte, an edge-item reference, a context reference, and `key:u32` end the selector.

In a schema-44 ASM carrier, an `ATTRIB_CUSTOM-attrib` record has five common attribute fields followed by a family string and family values. The common fields are an attribute reference, a signed integer, two attribute-chain references, and the owning entity reference. Family `INV_NMX_MATCHED_ATTRIB` stores the integers 1, 44, 1, `selector_source_index`, −1, `key`, and `qualifier`. Qualifier 0 identifies the selected edge; qualifier 1 can identify its vertices. The pair (`selector_source_index`, `key`) joins the direct edge selector to its matching attribute. The owner reference identifies the edge record in the ASM entity-revision namespace. The attribute and edge can be in the construction-history archive rather than the active body.

A generated name on that same owning edge identifies the edge in the generating feature's result. Family `INV_NMX_GEN_TAG` stores 1, 44, 1, `feature_source_index`, −1, `associative_id`, 0, and `role`. Edge roles are 2, 10, and 18. Family `INV_NMX_SWEEPGENERATED_TAG` stores the same first seven integers followed by `role` and seven zero integers. Edge roles 10 and 18 are distinct names. `feature_source_index` equals the generator value in the selector. The associative identifier names an entity-style link in the sketch consumed by that feature. Role 18 uses a point link; roles 2 and 10 use a curve link. The family, associative identifier, qualifier, and role together form a feature-local persistent name. The name is scoped to the generating feature's result. The construction-history record framing and entity-revision namespace follow [asm.md §3.2](asm.md#32-delta_state-records).

An equal-distance edge chamfer uses slot 0 for its edge collection, slot 2 for its length, slot 4 for the chamfer form, slot 5 for direction reversal, and slot 11 for the result object collection. Chamfer values 0, 1, and 2 mean equal distance, distance and angle, and two distances. The equal-distance branch does not consume an oriented support face.

A revolve uses slot 0 for its part operation, slot 1 for its boundary patch, slot 2 for its axis, slot 3 for its extent, slot 4 for its angle parameter, slot 7 for its feature dimensions, and slot 17 for the result object collection. The feature label's sole participant identifies the owning planar sketch, and the boundary patch selects its profile as for an extrusion. Extent value 3 is a full revolution; the angle parameter does not apply to it.

Type `896cf08ed1113c0460007cb801f31bb0` is a work axis. After the content header it stores `entity_flags:u32` and two u32 values; segment major versions above 21 add one u32 value. A type-2 reference list follows, then an optional u32 `0x203`, two f64 triples, and `state:u8`. The first triple is a point on the axis and the second is the span from that point to the other end of the axis segment, both in internal centimetres. The axis direction is the span direction. Origin work axes span from −1 to 1 along their direction.

A hole uses slots 0 through 6 for hole form, diameter, depth, entry diameter, entry depth, entry angle, and drill-point angle. Slots 8, 9, 16, 17, 21, and 24 identify its transform, extent, a direction, a Boolean, a placement, and the result object collection. Hole-form values 0, 1, 2, and 3 mean drilled, countersink, counterbore, and spotface. The placement transform translation uses internal centimetres. The direction record supplies the drilling direction. The placement record repeats the transform reference and retains its point and value references.

Feature-label class `0619aa28d411c77a6000c4a4cae1fbb0` identifies a Thread annotation on selected cylindrical faces. Its generic feature record has nine property slots. Slot 0 is the face collection; it contains at least one face item. Slots 1, 2, and 6 are Boolean properties. Slots 3 and 4 are numeric parameter references. Slot 5 is an edge collection, and slot 8 is the feature-dimension collection. Slot 7 is a native property reference. A Thread annotation has no result object collection.

Feature-label class identifier `ecf481729c4d5f925f2192b4a83096ac` identifies Thicken. Its 18-slot property vector has these roles:

| Slot | Property |
| --- | --- |
| 0 | Selection list `7a25f2efd11105af000847b00524dc09` |
| 1 | Part-operation enumeration |
| 2, 3, 4, 7, 10, 16, 17 | Boolean controls |
| 5, 8, 11 | Null references |
| 6 | Thickness length parameter |
| 9 | Feature-dimension reference list |
| 12 | Dimensionless numeric parameter |
| 13 | Control record `4c40b0a3ac4b9504e719b28a70e0e818` |
| 14 | Control record `c11c7b63ea4d840b68b76e8c67bcbc48` |
| 15 | Result object collection |

Slot 6 supplies the thickness length in internal centimetres. Slot 12 supplies a dimensionless value; it is not a second thickness. Both parameters use the ownership rules of section 11. Part-operation value 3 selects a solid join, and value 2 selects a cut. Slot 0 supplies the selected support geometry through a selection list. Each non-null selection reference resolves in the same PmDc segment. These references do not identify faces of the final model body.

Feature-label class identifier `12963cb8d1118a2860008bb801f31bb0` identifies a Shell operation. Slot 1 identifies the removed-face collection. An empty collection removes no face. Collection members are non-null and distinct by record index. Slot 2 identifies a selection list. Slot 3 identifies the wall-thickness length parameter, with units as defined in section 11. Slot 4 stores a reference. Slot 9 identifies the result object collection, whose members identify the output bodies.

A result object collection contains one or more surface-body references. This ordered surface-body list is the feature's native result-body identity.

Types `44326720d211c51d60002aab01f31bb0` and `b5a9d9fad211053360002cab01f31bb0` store rectangular-pattern and mirror feature records. Both start with the generic 30-byte feature prefix through `outline_value`, followed by a type-2 property-reference list, `value:u32`, a type-2 participant-reference list, six property references, and `control:u8`.

For segment major versions 15 through 20, the rectangular-pattern suffix contains 20 more property references. From segment major version 21, it contains 26 more property references. From segment major version 29, eight u32 extension values follow the first 12 of these references, and from version 30 six more u32 extension values end the suffix. The mirror suffix contains five more property references, six u32 extension values from segment major version 21, and two final property references. From segment major version 29, eight u32 extension values separate the two final property references. The record ends after the applicable suffix.

### 13.1 Feature history

A model root (§11) also owns the browser feature tree of its model. The root label of a model root is the feature label whose owner reference selects the model root. The tree of the document model root is the part modeling history. The tree of a flat-pattern model root is the flat-pattern history. A segment has a history only when it has exactly one document model root.

The participant list of a root label is the ordered top level of the browser tree. Each participant is one tree member:

- A feature, pattern-feature, or planar-sketch record is a history node.
- The end-of-features record is the end-of-part marker. Top-level members after the marker are below the end of part.
- Another record that owns a feature label is a group. The participants of its label are its nested members, in order.

The participants of a feature label that are planar-sketch records are the sketches that the feature consumes. History order is the post-order of the tree: the consumed sketches of a feature precede it, and the nested members of a group take the position of the group. A record has one position; a repeated reference does not move it. The model history precedes the flat-pattern history.

The participants of a pattern-feature record are its seed features.

Content-header flag bit 2 (`0x4`) marks a suppressed feature, pattern-feature, or sketch record. Flag bit 19 (`0x80000`) marks a feature, pattern-feature, or sketch record below the end-of-part marker.

### 13.2 Loft and sweep operands

Feature-label classes `e0ef542fd111340900084eba32a3dc09`, `808b00afd41140881000fca5bba647b5`, and `05daba90d111386100080bbd0663dc09` identify solid loft, surface loft, and sweep respectively. Operand references select records in the feature's PmDc segment.

Type `509e2f31d1110344000881ba32a3dc09` stores an ordered loft-selection list. The content header is followed by one type-2 reference list and the record ends. Each member is a non-null reference to a type `e850314bd111d641000880ba32a3dc09` loft-section selection. A loft-section selection with an empty type-7 map contains:

```text
content_header
entity_link_reference u32
value u8
condition_reference u32
parameter_references u32[2]
map_value u32 = 0
map_marker u32 = 0x30000007
map_count u32 = 0
source_value u32
option_reference u32
```

The record ends after `option_reference`. Section 12 defines the content-header extensions and stamps. `entity_link_reference` selects a kernel-entity link. The link's `selection_source` equals the section's content-header `source_index`, and its non-null body reference selects a kernel-body record. `condition_reference` selects type `6c1c6c3c9a43e3947318e2bbd0c39ff8`. Both parameter references select numeric parameter records of type `264d8790d011f8d10008cabc0663dc09`. The option reference is null or selects a Boolean record. `value` and `source_value` are unsigned scalar fields, not references. All their bit patterns are valid. `source_value` is distinct from the content-header source index.

Types `6c1c6c3c9a43e3947318e2bbd0c39ff8`, `02e2e3e0d111480900084eba32a3dc09`, `b7849ca26d4811e38b95b3a06051b440`, `ed2ef031904b9b04d79918b91f1619c8`, and `e211b05c3e46583e928a0cbf3a085834` use the content-header enumeration grammar, with `type_value:i16` and `value:u16`. They identify the loft section condition, loft construction control, sweep construction control, sweep orientation control, and sweep scaling control. The record ends after `value`. Both scalar fields admit all bit patterns.

Loft property slots are:

| Slot | Operand |
| ---: | --- |
| 0 | Ordered section list |
| 1 | Part operation |
| 2 | Boolean construction control |
| 6 | Surface-loft result body wrapper |
| 7 | Auxiliary loft-selection list |
| 9 | Boolean construction control |
| 10 | Loft construction enumeration |
| 13 | Solid-loft result object collection |

Sweep property slots are:

| Slot | Operand |
| ---: | --- |
| 0 | Boundary patch |
| 1 | Path selection |
| 2 | Part operation |
| 3 | Angular parameter |
| 4 | Extent enumeration |
| 7 | Feature dimensions |
| 8 | Sweep construction enumeration |
| 9 | Sweep orientation enumeration |
| 10 | Sweep scaling enumeration |
| 15 | Result object collection |

### 13.3 Split property references

Feature-label class `baa87183d311347bc000e39545df724f` identifies a split feature. For each non-null property reference, the zero-based slot determines the record type:

Slot | Record type
--- | ---
0 | `1990e1b8d311467bc000e39545df724f` enumeration
1 | `284d8790d011f8d10008cabc0663dc09` Boolean
3 | `1790e1b8d311467bc000e39545df724f` enumeration
4 | `91739422d11107cf000835bd0663dc09` boundary patch
5 | `42df52ced011d0d20008ccbc0663dc09` work plane
6 | `9f8f2a40d21120e66000aaabc8433cb0` Boolean
7 | `83aadad54f4375dacd1003b260a8a422` body selector
8 | `474d8790d011f8d10008cabc0663dc09` surface-body wrapper
9 | `ae70680ed14a1e86d76248b03c2a96e1` object collection

The slot-3 enumeration selects the tool reference. Value 2 selects the work plane in slot 5. Value 3 selects the surface-body wrapper in slot 8. The unused tool reference is null. The work-plane frame follows the grammar and units in section 13.

### 13.4 Attached plates and bend properties

The 17-property attached-plate form has these slots:

| Slot | Operand |
| ---: | --- |
| 0 | Plate profile selection |
| 1 | Face-sketch normal direction |
| 2 | Thickness-side control |
| 3 | Thickness length parameter |
| 4 | Attachment control `92596ba9d311efa86000b3a99dccefb0` |
| 5, 6 | Null |
| 7 | Edge collection |
| 8 | Feature dimensions |
| 9 | Corner control `83ce7786a946cde11ebd99b4fe02afb2` |
| 10 | Bend relief `64880596d3119a76600081a99dccefb0` |
| 11 | Transition `e9345026d5114c081000e596c28b7ab5` |
| 12 | Attachment selection list `ce2d9b29ad43d8802f132c9a40c9ba90` |
| 13 | Plate scalar control `051772e5a74f641a01386e8f3cc02218` |
| 14 | Null |
| 15 | Result object collection |
| 16 | Part operation; value 3 joins the existing body |

The attachment selection list contains a type-2 reference list after the versioned content header and stamps. Each member selects a selection wrapper of type `799c7cfa5a4ef5b2597b6494fc798db5`. A wrapper contains the versioned content header and stamps followed by `edges:u32`, then ends. `edges` references an edge collection. Its edge-item indexes name edge-selector source indexes as defined in section 13. This path selects edges on the input sheet. It does not name the new plate's profile edges. The plate and bend edge collection can be empty while the attachment selection list is nonempty.

Feature-label class `f258f74cd3114f24600047a99dccefb0` identifies a bend. Its 11-property form has these slots:

| Slot | Operand |
| ---: | --- |
| 0 | Edge collection shared with plate slot 7 |
| 1 | Inside-radius length parameter |
| 2 | Thickness length parameter shared with plate slot 3 |
| 3 | Attachment control shared with plate slot 4 |
| 4 | Bend relief shared with plate slot 10 |
| 5 | Null |
| 6 | Feature dimensions |
| 7 | Transition shared with plate slot 11 |
| 8 | Bend scalar control `3441b690d511d8026000a28d4aa6fbb0` |
| 9 | Result object collection shared with plate slot 15 |
| 10 | Boolean control |

The bend radius in slot 1 repeats the radius reference of the attachment control. The flange record owns ordered plate, bend, and corner children. Each child is a separate generic feature with its own source index, label owner, state, and history position. Sharing an attachment control identifies the plate and bend pair. The bend is not a property sub-record of the plate. The plate and bend refer to a common result-body wrapper; result identity alone does not select a history revision. Neither the attachment control nor the bend's property vector contains an angle parameter.

Feature-label classes `7f588068b74378a0559515946f7a1c12`, `852f6bb0d411db6f60004daa9dccefb0`, and `aaf21093d344209a04cc799656cd7e09` identify a corner, a hem, and a flat-pattern definition respectively. The seven-property corner form uses slots 0 through 6 for corner control, null, feature dimensions, attachment selection list, plate profile selection, null, and result object collection. The corner shares the plate's corner control, attachment selection list, profile selection, and result collection.

The ten-property hem form uses slots 0 through 9 for plate-profile selection, direction, thickness parameter, control `e5b810b3d411a37a600050aa9dccefb0`, edge collection, bend relief, body selector `83aadad54f4375dacd1003b260a8a422`, feature dimensions, transition, and bend scalar control. The hem is a separate labeled generic feature.

All property layouts below start after the versioned content header and stamps. All named references use the packed PmDc record-reference grammar of section 12. Length parameters use internal centimetres. Their formula graph preserves the distinction between a literal value and a reference to a sheet-metal style parameter.

Attachment control fields are:

```text
control u32
thickness u32
first_mode u32
directions u32[2]
radius u32
second_mode u32
relief_width u32
minimum_remnant u32
first_option u32
states u8[3]
second_option u32
```

`control`, `first_option`, and `second_option` reference Boolean properties. `thickness`, `radius`, `relief_width`, and `minimum_remnant` reference length parameters. `directions` reference two direction records. `first_mode` and `second_mode` reference enumeration type `7267cc3bd31104a96000b3a99dccefb0`. The extended attachment form adds one terminal `extension:u32` reference to type `93596ba9d311efa86000b3a99dccefb0`. The record ends after `second_option` or `extension`.

Type `93596ba9d311efa86000b3a99dccefb0` is an attachment-state record. It uses the state content header without stamps, followed by these fields:

```text
state i32
thickness u32
radius u32
attachment u32
relief_width u32
minimum_remnant u32
options u32[2]
```

`attachment` references the attachment control or a hem control of type `e5b810b3d411a37a600050aa9dccefb0`. The remaining references repeat its length-parameter and Boolean operands. The record ends after `options`.

Bend relief fields are:

```text
control u32
width u32
depth u32
form u32
minimum_remnant u32
seam_gap u32
```

`control` references a Boolean. `width`, `depth`, `minimum_remnant`, and `seam_gap` reference length parameters. `form` references enumeration type `720c3e2bd311976060007aa99dccefb0`. The width and minimum-remnant references repeat those of the attachment control. The record ends after `seam_gap`.

Transition fields are:

```text
form u32
control u32
radius u32
```

`form` references enumeration type `07a66130d4118895600053aa9dccefb0`, `control` references a Boolean, and `radius` references a transition-radius length parameter. The record ends after `radius`.

Corner control fields are:

```text
control u32
gap u32
size u32
relief_form u32
corner_form u32
secondary_form u32
jacobi_radius u32
value u32
option u32
```

`control` and `option` reference Booleans. `gap`, `size`, and `jacobi_radius` reference length parameters. `relief_form` and `secondary_form` reference type `720c3e2bd311976060007aa99dccefb0`; `corner_form` references type `e0a8d8afd3117d5a0008928f8b1f0f69`. `value` is a raw u32 word. The record ends after `option`.

Types `7267cc3bd31104a96000b3a99dccefb0`, `720c3e2bd311976060007aa99dccefb0`, `e0a8d8afd3117d5a0008928f8b1f0f69`, and `07a66130d4118895600053aa9dccefb0` contain the versioned content header and stamps, followed by `type_value:i16` and `value:u16`, then end. Their type values are 9, 304 or 501, 8, and 4 respectively. An enumeration value is a scalar, not a record reference.

The plate scalar control contains `value:u32` after its versioned content header and stamps. The base bend scalar control contains `value:u32` after that header and stamps. The extended bend scalar control contains `value:u32` and `state:u8`. Each record ends after its last stated field.

## 14. Document kind

`Pm*` segment families identify a part document. `Am*` segment families identify an assembly document. A document that contains both families has the distinct `mixed_part_assembly` kind. Property metadata can identify a part, assembly, drawing, or presentation only when segment-family evidence does not already identify the kind.
