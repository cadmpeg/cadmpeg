# Autodesk Inventor IPT/IAM/IDW

This specification defines the byte semantics of the supported Autodesk Inventor document envelope. Multi-byte integers are little-endian unless a section states otherwise.

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/inventor.md`](../layouts/inventor.md), generated from `docs/layouts/inventor.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. `cargo test -p cadmpeg --test layout_tables` proves the two agree.

## 1. Compound container

An IPT, IAM, or IDW document is a Compound File Binary container. The container follows the CFB sector, FAT, DIFAT, mini-FAT, directory, and sibling-tree rules. The directory contains the `RSeStorage` storage. A positive format identification also requires a structurally reached `RSeStorage/RSeSegInfo` stream or a stream at `RSeStorage/V<n>/RSeDb`, where `<n>` is one or more decimal digits.

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

Section 9 is a counted sequence of tagged reference entries. Each entry starts with `key:u16` and `tag:u8`. Tags 1 and 2 carry a 16-byte identifier. Tag 3 carries two u32 values. Entries have no padding, and the sequence exhausts the section payload. A section-9 payload that is not a complete tagged sequence, and whose length is the count times 19 bytes, keeps its entries untyped.

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

`UFRxDoc` schemas 11 through 15 start with a u16 schema and a u16 section-version count. The section-version table governs optional header fields. The schema-15 representation branch follows the header-pair table with a u16 state, a u16 design-view count, and a u16 representation prefix. Parts and assemblies then store that many design-view entries. Each entry is a counted UTF-16LE name and a counted UTF-16LE kind, and the entries keep the order of the design-view list. Both document kinds continue with a two-u16 secondary LOD state, a counted UTF-16LE active design-view name, and a two-u16 state pair; they omit the older header-version-flags field. The active design-view name selects one entry of the list. Schema 15 then stores a u32 and a u16 header state, one more u32, and the u16 invariant 1. The model-state table precedes the external-reference table. Each model-state record contains a u8 prefix, counted UTF-16LE name, two-u16 state pair, u32 prefix count, u32 parameter count, the counted parameter records, and a suffix. A part's suffix has 77 bytes. An assembly's suffix has 77 bytes when section-version entry 2 is 16 or earlier and 81 bytes when it is 18 or later. Each parameter record contains a counted UTF-16LE name, u8 tag, u16 kind, u16 state, counted UTF-16LE value, and u16 trailer.

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

The related references of an `AmDc` occurrence select occurrence state records of type `614d8790d011f8d10008cabc0663dc09` in the same segment. Record references use bit 31 as a qualifier and bits 0 through 30 as a one-based record index; the referenced record ordinal is the index minus one. A state record starts with:

```text
header_value u32
header_id u16
next_reference u32
flags u32
```

The remaining bytes of a state record are retained. Flag bit `0x02000000` marks the occurrence as grounded. An occurrence is grounded when a state record that its related references select has this bit set.

The `DesignViewSegment` stores one representation record of type `41a2c29dd211cc99a0002aa976d456c9` for each design-view representation. Its payload starts with:

```text
header_value u32
kind u16
name counted UTF-16LE
list_marker u16 = 2
list_signature u16 = 0x3000
member_count u32
member_metadata u32[2] when member_count is nonzero
member_references u32[member_count]
```

The remaining bytes of a representation record are retained. The member references select records of the same segment. The active representation of an assembly is the one representation record whose name equals the active design-view name of the `UFRxDoc` header. When no representation record or more than one representation record has that name, no representation is active and no occurrence is hidden. Each representation lists its own members, so a visibility-off record that the user makes in one representation is a member of that representation only.

A visibility-off record has type `2123c271d21187a9a0002ea976d456c9`. Its payload is:

```text
header_value u32
kind u16
list_marker u16 = 2
list_signature u16 = 0x3000
key_count u32 >= 1
key_metadata u32[2]
occurrence_keys u32[key_count]
trailer u8 = 0
```

The remaining bytes of a visibility-off record are retained. A visibility-off record hides an occurrence when the active representation lists it as a member. A key path of one key names the root occurrence whose `occurrence_key` equals that key. A key path of two keys names an occurrence inside an assembly that a root occurrence instantiates: the first key is the `occurrence_key` of the root occurrence, and the second key is the `occurrence_key` of an occurrence in the occurrence table of that assembly. A representation that lists no such record leaves the occurrence visible.

A `UFRxDoc` occurrence joins the `AmDc` and `AmGraphics` records through `occurrence_id`. It joins its external prototype through `reference_id`: the one external reference with the same `reference_id` is the prototype. Copies of one file keep its document identifier, so the document identifier alone does not select a copy. When the occurrence stores a first-section document identifier, the prototype must store the same document identifier. Otherwise, and when no reference or several references have the `reference_id`, the occurrence is unresolved. The external-reference occurrence count equals the number of joined `UFRxDoc` occurrences. Each occurrence in the current document is a root placement. A referenced assembly remains an unresolved external prototype; its internal occurrences are not inserted into the current document.

Inventor placement translations use centimetres. Neutral occurrence translations use millimetres, so the first three elements in matrix column 3 are multiplied by 10. The remaining matrix coefficients are unchanged. External-reference state bit `0x2000` marks its occurrence as suppressed. A suppressed occurrence has `visible = false`; when it has no graphics placement, its neutral transform is identity. An occurrence that the active design-view representation hides has `visible = false`. A grounded occurrence maps to a neutral `AssemblyJoint` with `JointOperands::Grounded` and one connector on the occurrence, in the frame of its placement. An active occurrence without a finite affine graphics placement is not transferred and produces an assembly-placement loss.

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

The payload ends after `terminal_value`. The first six fields are the content header of section 12; later segment major versions extend it as that section states. PmDc references store a one-based record index in bits 0 through 30. Bit 31 is retained as a qualifier and does not change the index arithmetic. Every non-null parameter, expression, or unit reference resolves within the same PmDc segment. Surface-body references use the address spaces in section 13.4.

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

Planar sketch entities add `entity_flags:u32` and `sketch_reference:u32` after the content header. The sketch reference identifies the owning planar sketch. Entity-flag bit `0x8000` of a line marks an axis line for distance dimensions. Entity-flag bits `0x40`, `0x1000`, and `0x40000` do not mark construction geometry. Bit `0x80000` of a circle, ellipse, or spline, bit `0x200` of a line or circle, and bit `0x8000` of a line do not mark construction geometry. Bit `0x4` of a spline entity does not mark construction geometry. Bits `0x80000` and `0x200000` of a line mark construction geometry: a construction line bounds no profile loop.

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

The point list of a circle or ellipse starts with two members. Two null references mark a closed curve. Two point references mark a bounded curve: the first is its start point and the second its end point. A circle with state 1 is a full circle and has two null leading members. A circle with state 0 is an arc and has two leading point references. A closed ellipse has state 1 and two leading point references whose packed values are zero. An arc runs counterclockwise about the sketch normal from its start point to its end point, on the circle of the stored center and radius. Its end points lie on that circle within the kernel linear tolerance, `1e-6` internal centimetres.

A full circle, a closed ellipse, or a closed periodic polynomial spline forms a closed profile by itself. Line and arc records form a profile loop when every endpoint in their connected component has degree two and the ordered traversal closes.

Type `d42f37f9d11115d3000847b00524dc09` stores a planar spline. After the planar-entity prefix it stores a type-2 point-reference list, nine u32 values, a type-2 u32 list, `state:u32`, a primary spline curve, `curve_state:u8`, two type-2 auxiliary reference lists, three u32 values, `secondary_prefix:u8`, `secondary_present:u32`, `secondary_state:u8`, a secondary spline curve, and nine terminal u32 values.

A form-1 spline curve has this fixed prefix:

```text
form u32 = 1
degree u32
tolerance f64
knot_count u32
knot_value u32
scalar_width u32 = 8
```

The polynomial branch follows this prefix with `knot_count` f64 knots, two zero u32 values, `scalar_width:u32 = 8`, `pole_count:u32`, `pole_value:u32`, and `scalar_width:u32 = 8`. It then stores `pole_count` two-f64 poles. Its fixed tail is:

```text
pole_tolerance f64
values u32[2]
interval f64[2]
terminal_value f64
scalar_width u32 = 8
```

The poles use internal centimetres. The degree is positive. The knot vector is the full nondecreasing vector, with `knot_count = pole_count + degree + 1`. `curve_state = 2`, curve values `1, 1`, equal primary and secondary curves, equal first and last poles, and clamped endpoint knots define a closed periodic polynomial spline. Each endpoint knot has multiplicity `degree + 1`. Its interval is the knot domain from knot `degree` through knot `pole_count`, with a lower first value than last value. The listed points are not the spline's control polygon or its two boundary endpoints. In this closed branch, the initial nine values, the three auxiliary values, both secondary one-byte values, the spline state, the two knot values, and the curve terminal value are zero. The nine terminal u32 values are `2, 0, 0, 0, 0, 0, 0, 0, 0`; `pole_value` equals `pole_count`.

The endpoint-bound open cubic form has segment major version 23 and `entity_flags = 0x40`. Its two point references select distinct point records in the owning sketch. The point list and the u32 point-value list have type 2 and metadata `2, 0`. The point-value list is `0, 1`. The first point position equals the first primary pole, and the second point position equals the last primary pole. The first and last poles differ. Increasing knot parameter runs from the first point to the second.

This open form has equal primary and secondary arrays. It uses the spline curve prefix, knot lane, pole lane, and tail above with these controls:

| Field | Value |
| --- | --- |
| Initial nine u32 values | `0x30000002, 0, 0, 0, 0, 0, 0, 0, 0` |
| Spline state | `0` |
| Curve state | `2` |
| Auxiliary lists | Two empty type-2 lists |
| Auxiliary values | `0, 0, 0` |
| Secondary prefix, presence, and state | `0`, `1`, `0` |
| Nine terminal values | `2, 0, 0, 0, 0, 0, 0, 0, 0` |
| Array form and degree | `0`, `3` |
| Array tolerance | `1e-9` |
| Knot value | `knot_count` |
| Knot values | `0, 0` |
| Pole value | `8` |
| Pole tolerance | `1e-12` |
| Curve values | `1, 1` |
| Curve terminal value | `0` |

The open cubic knot vector has `knot_count = pole_count + 4`. Each endpoint knot has multiplicity four. Its interval is the full nonempty knot domain from knot 3 through knot `pole_count`. The curve is polynomial and nonperiodic.

Type `9e146d7cd1115bd5000847b00524dc09` stores a spline-point relation. After the constraint header it stores a spline reference and a point reference, in that order.

Type `184d8790d011f8d10008cabc0663dc09` stores a compressed 4-by-4 sketch transform after the content header. An optional u32 value `0x203` precedes two u16 masks. For matrix element bit `b`, a clear zero-mask bit and clear value-mask bit select one following f64. A clear zero-mask bit and set value-mask bit select positive one. A set zero-mask bit and clear value-mask bit select zero. Both bits set select negative one. Explicit values occur in row-major order. Translation values use internal centimetres.

Type `40df52ced011d0d20008ccbc0663dc09` stores a sketch direction. It adds `entity_flags:u32`, `parameter:f64`, an optional u32 extension, and a three-f64 direction vector after the content header. The transform's first column is the sketch u-axis. Its third column and the direction vector are the same oriented plane normal. The last matrix row is `[0, 0, 0, 1]`.

A planar constraint starts with the content header, `state:i32`, and a group reference. Segment major versions 15 and 16 store the parameter reference immediately after the group reference. Segment major versions 17 through 22 store two type-6 maps before the parameter reference. A type-6 map starts with u16 values `6, 0x3000` and a u32 count. A nonempty map then stores two u32 metadata values. The first map stores reference/f64 pairs. The second map stores reference/reference pairs.

Coincident, tangent, distance, circle-center, and midpoint relations can store a solver hint in the two constraint maps: in each map, at most one entry for each of the relation's two operands. A scalar entry is a position on that operand: a normalized line parameter, or an unwrapped angle in radians about a circle center. A reference-map value is null, except in a tangent relation, where it is the unqualified value 0, 1, or 2. For a line operand, 1 and 2 state that the tangency is at the start or the end of the line, and 0 that it is between them. The hint does not change the relation.

Constraint types `944d8790`, `954d8790`, `964d8790`, `974d8790`, `984d8790`, and `994d8790` with the same remaining 12 identifier bytes as the planar-sketch type encode coincident, parallel, perpendicular, tangent, horizontal, and vertical relations. Coincident stores two entity references. Parallel and perpendicular store two entity references and a u16 orientation. Tangent stores two entity references and an optional u32 extension. Horizontal and vertical store one entity reference and a u8 state.

Types `00c0ac00d1115fe0800066b1e13554c7` and `40ff8336d1115fe0800066b1e13554c7` encode horizontal and vertical distance dimensions. Each stores two entity references, a replacement parameter reference, and four u32 values after the constraint header. Type `00b71b67d11168e0800066b1e13554c7` encodes a radius dimension and stores one u32 state, an entity reference, and four u32 values. Type `e096df74d11169e0800066b1e13554c7` encodes a diameter dimension and stores an auxiliary reference, an entity reference, and four u32 values. The constraint-header parameter drives radius and diameter. The replacement parameter drives horizontal and vertical distance.

Type `008c10e1d11102e680006db1e13554c7` binds a circular entity to its center-point entity. Type `d07d2c44d11189e680006fb1e13554c7` gives two circular entities equal radii. Type `e0c281c6d11110e680006eb1e13554c7` gives two lines equal lengths. Each stores its two ordered entity references after the constraint header.

Type `58850511d211e39560000cb38932edb0` encodes a distance dimension. After the constraint header it stores `value:u32`, two entity references, and six f64 values. The constraint-header parameter drives the Euclidean distance between the two entities: between two points, from a point to a line's carrier, or between two parallel line carriers. When an operand is a line whose entity flags include bit `0x8000`, the parameter value is twice that distance.

Type `100a0d59d111cae680006fb1e13554c7` encodes an angle dimension between two lines. After the constraint header it stores `value:u32`, two line references, two f64 values, and `flags:u16`. The constraint-header parameter, in radians, is the angle between the two line directions or its supplement.

Types `bf70e821d011d0d20008ccbc0663dc09`, `b07b170fd11196e680006fb1e13554c7`, `c0a3558fd1115ee0800066b1e13554c7`, `5052da64d1115ee0800066b1e13554c7`, `e07b9a5ad1115de0800066b1e13554c7`, and `107cdd0dd1119ce680006fb1e13554c7` encode midpoint, fix, horizontal-alignment, vertical-alignment, collinear, and symmetric relations. Their constraint-header parameter is null. After the constraint header:

- midpoint stores a line or arc reference and a point reference, followed from save version 20 by `value:u32`; in the neutral midpoint form the value is 0 and the point is the midpoint of the curve;
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
- `7339fdce7a4e4011bee343897908ba92`: auxiliary feature enumeration;
- `fb7982ef3347f24f718133968efffa3f`: face-motion mode.

Types `1990e1b8d311467bc000e39545df724f` and `1790e1b8d311467bc000e39545df724f` use the same enumeration grammar. Their type values are 3 and 4 respectively. Both use the versioned content header of section 12, including its stamps.

Type `3200aa7dd2112b836000f3a89dccefb0` stores the chamfer form. It adds the same `type_value:i16` and `value:u16` fields, followed by a zero u32 terminal value.

The part-operation, hole-form, fillet-form, and chamfer-form records use type values 5, 3, 2, and 2 respectively. Extent records use type value 11 through segment major version 21 and type value 12 from segment major version 22. The face-motion mode uses type value 2.

Type `284d8790d011f8d10008cabc0663dc09` adds a counted UTF-16LE name, `name_value:u32`, and a Boolean byte. The Boolean byte is 0 or 1. Types `91739422d11107cf000835bd0663dc09`, `71f23ed8d2115094a00049803603c8c9`, `ae70680ed14a1e86d76248b03c2a96e1`, and `dae9481bd211dc2c00083eab1b14dc09` add one type-2 reference list. They identify a boundary patch, feature dimensions, an object collection, and constant-radius fillet edge sets respectively.

Type `f1b2bc24d01121dd0008d0bc0663dc09` stores a face collection. After the content header it stores one type-2 reference list and ends. Each member names a distinct face-selection item of type `ccb59f50d1114f6b0008a1ba32a3dc09` in the same PmDc segment. The face collection and its items define a native selection independent of the current kernel topology.

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

The body reference selects a kernel-body record in the link's segment, of type `5c5945f6d5113313100060a6bba647b5`. The entity identifier selects one entity of that body. In the state-0 scalar-selection form, `selection_source` equals the content-header source index of the selection record that references the link. A PmBRep kernel-entity link uses the same payload grammar. Its body reference selects the active part kernel carrier. Its entity identifier is the nonnegative ASM body key of the selected body when a surface-body wrapper references the link. The kernel entity type is retained; its numeric value does not select the body.

A PmDc kernel-body record uses the record envelope and footer of the active part kernel carrier (§5). Its footer selected key is the record's own qualified reference. Its kernel bytes hold ASM profile bodies. A profile body contains wires or planar faces. The kernel-entity link of a profile selection selects the body whose ASM body key equals the link's entity identifier. Each closed wire or face boundary loop of that body is a profile boundary ring. An open wire is an edge chain with two distinct boundary vertices. Face loops store their edges through the ordered coedge references. Model-space lengths in these bodies use internal centimetres.

A closed profile selection holds one or more closed loops for each selected key. Each of its edges carries one ASM naming attribute. The class chain `NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib` stores five common attribute fields, followed by the integers `naming_type`, 0, `associative_id`, 0, and 2. The class chain `ATTRIB_CUSTOM-attrib` stores the same five common fields, the string `INV_NMX_GEN_TAG`, integer 1, and the same five naming integers. The value 2 marks an edge name. `naming_type` equals the entity type of the kernel-entity link. `associative_id` equals the associative identifier of the sketch curve that the edge comes from, in the sketch that the extrusion consumes: the identifier of the curve's curve-type entity-style link, or the identifier that a block instance gives a curve it places. The end points of each edge lie on that curve in the sketch plane, and the edge runs along the part of the curve between them. On a full circle, the point at the middle of the edge's parameter interval selects which of the two arcs between the end points the edge runs along. An edge whose end points lie at the end points of its curve runs along the whole curve. A full circle can run through more than one edge. Each of these edges carries the name of the circle, and the circle is then the only curve of the wire. Consecutive edges of a wire share an end point, and the last edge ends where the first one starts.

Type `1641d6aad211db2c00083eab1b14dc09` stores one constant-radius fillet edge set. The content header is followed by edge-collection, radius, selection-mode, and continuity references. The continuity reference is null or selects a Boolean record. From segment major version 23 a fifth reference follows; it is null or a `284d8790d011f8d10008cabc0663dc09` Boolean record. Type `514d8790d011f8d10008cabc0663dc09` stores an edge collection as a type-2 reference list after the content header. The members identify type `82695c37d111516b0008a1ba32a3dc09` edge-item records. An edge item stores a type-2 `u32` index-reference list after the content header, an `i32` when the list is not empty, and a final `u32`.

Type `4a374949d211001d00083bab1b14dc09` stores a fillet edge-selection enumeration. It adds `type_value:u32` and `value:u32` after the content header. Type value 4 and value 0 select explicitly listed edges. Type value `0x00020004` with value 0 is a second selection descriptor. The edge set, not its edge collection alone, identifies the native selection for this descriptor.

A single closed periodic polynomial spline is a whole profile ring when its named wire has one edge, the edge's spline degree and full knot vector equal those of the named sketch spline, its ordered poles map to the sketch poles in the sketch plane within the kernel linear tolerance, its parameter interval is the full knot domain, and both edge vertices equal the spline seam within that tolerance. The carrier and sketch curve have no rational weights. This join uses the edge's naming attribute and the consumed sketch's curve-type entity-style link.

Feature-label class identifiers `3111a90cd0118b83000819b00524dc09`, `dc15f7f1d1114205000830b00524dc09`, `3f7100f9d2118b6f6000f0a89dccefb0`, and `1a7d751fd2119c54a00020803603c8c9` identify extrusion, fillet, chamfer, and hole records respectively. Feature-label class identifier `b54dd6f6d311c4966000f2b21d6eefb0` identifies a sheet-metal corner round. Its feature record uses the constant-radius edge-fillet slots and rounds the selected edges. Type `08dcddc3d311d0976000a7a99dccefb0` is a sheet-metal flange record. After the content header it stores `state:i32`, a type-2 reference list, and a u32 value. The list names the plate, bend, and corner feature records that form the flange, in that order. Type `904bc1c4d311ff2860004da99dccefb0` is a sheet-metal face record with the same envelope. The one-plate form names one plate feature record; its label lists the planar sketch and that plate. The plate-and-bend form names the plate and bend feature records in that order; its label lists the planar sketch, plate, and bend. The flange label's participants are its planar sketch, plate, bend, and corner, in that order. The flange and face state headers have no content-header stamps.

The segment-major-29 one-plate face suffix occupies 28 bytes after the state content header. It stores `state:i32 = -1`, `marker:u16 = 2`, `signature:u16 = 0x3000`, `count:u32 = 1`, `metadata:u32[2] = [1, 16]`, one qualified plate reference, and `value:u32 = 0`. The segment-major-19 plate-and-bend face suffix occupies 32 bytes after the state content header. It stores `state:i32 = -1`, `marker:u16 = 2`, `signature:u16 = 0x3000`, `count:u32 = 2`, `metadata:u32[2] = [2, 0]`, qualified plate and bend references in that order, and `value:u32 = 0`. Each suffix ends after `value`.

A feature label with class identifier `c65f5855d311730c60003aa99dccefb0` labels a sheet-metal plate. A plate uses property slots 0 through 3 for its profile selection, direction, thickness-side control, and thickness parameter, slot 7 for an edge collection, and slot 8 for its feature dimensions. Type `473f20fcd11101cf000835bd0663dc09` is the plate profile selection and has the profile-selection layout. For a plate owned by the one-plate form of a `904bc1c4d311ff2860004da99dccefb0` face record, the direction is the normal of the face sketch. A Boolean thickness-side control selects thickness along this normal when false and against it when true. Type `69b9505efb4bbaa925c3dfa870817810` is a thickness-side enumeration. Its versioned content header, including stamps, is followed by `type_value:i16` and `value:u16`, then the record ends. Type value 2 and value 0 select thickness along the normal.

A plate with a surface-body reference in slot 6 has that body as its result. The result-collection plate form has 17 properties, null slot 6, a result object collection in slot 15, and a part-operation enumeration in slot 16. Part-operation value 1 creates a new body. With no bend operands, the plate is the closed profile region of its face sketch, thickened from the sketch plane by its thickness parameter on the selected side. A plate that names a record in slot 4 or in slots 9 through 13 is joined to an existing face by a bend.

An extrusion uses property slots 0 through 7 for part operation, boundary patch, direction, reverse direction, forward length, taper angle, extent, and symmetric extent. The repeated-patch form stores the same boundary-patch reference in slots 1 and 23. Slot 26 identifies the result object collection. The direction vector supplies the travel direction. A true slot-3 Boolean negates it. Part-operation enumeration values 1, 2, 3, and 4 mean new body, cut, join, and intersection. Extent values 1, 2, 4, 5, 6, and 7 mean fixed dimension, symmetric fixed dimension, through next, through all, from-to, and to. A fixed dimension uses the slot-4 length. Extent 2 occurs only with a true symmetric-extent Boolean, and its length is the total travel split evenly around the profile plane. A true symmetric-extent Boolean with extent 5 extends through all material on both sides of the profile plane. Other extents with a true symmetric-extent Boolean are outside the supported branch.

The zero-control extrusion form has these operand values:

| Operand | Value |
| --- | --- |
| Slot-2 direction `entity_flags` | 0 |
| Slot-2 direction `parameter` | 0 |
| Slot-2 direction `extension` | Absent or 0 |
| Type `3b2477a4d1118f96000826bd0663dc09` profile-selection `value` | 0 |

A boundary patch contains profile-selection references. Its ordered selections identify the stored profile. Each selection's entity-link reference identifies its selected profile. The other entities and constraints of the consumed sketch have separate record identities. The feature label's sole participant identifies the owning planar sketch. Each profile selection is one or more closed rings of whole curves or parts of curves of that sketch. The rings of one boundary patch bound the extruded region: a selected ring that lies inside an odd number of other selected rings bounds a hole in the region of the innermost selected ring that contains it. A sketch loop that no selection selects and that lies inside that region is covered by the region: the region is the area inside its exterior ring minus the area inside each of its hole rings.

The extent-7 target is the work plane in slot 11, the model face recorded by a termination flag in slot 13 and a surface in slot 18, or the work point in slot 25. Exactly one of the three forms is present: a nonzero slot 11 occurs with null slots 13, 18, and 25; the face form occurs with null slots 11 and 25; the work-point form occurs with null slots 11, 13, and 18. The work-plane and face forms store a plane and no face identity. The extrusion ends at that plane. When the travel direction is perpendicular to the profile plane and the point lies strictly ahead of that plane along the travel direction, the zero-control work-point form ends at a plane through the point. Its normal is the profile-plane normal oriented along the travel direction. Its u axis is the profile-plane u axis. The extent-6 extrusion starts at the work plane in slot 8, or at the face recorded by the termination flag in slot 10 and the surface in slot 17, and ends at the extent-7 target. The start target has the same two forms, and it also stores a plane and no face identity. A two-sided extrusion has an extent enumeration in slot 29. Slots 27, 28, and 29 hold the length, taper angle, and extent of the side opposite the extrusion direction.

Type `3edf52ced011d0d20008ccbc0663dc09` is a work point. After the content header it stores `entity_flags:u32` and two u32 values; segment major versions above 21 add one u32 value. Three f64 values follow: the point in internal centimetres. Two type-2 reference lists end the record. In segment major version 19, the work-point body before its two reference lists occupies 36 bytes. The zero-control work-point form has zero entity flags, zero u32 values, and two empty reference lists.

Type `fd24893dd211fa99000840ab0663dc09` is a termination flag. It adds one `value:u8` after the content header. An extrusion face target requires value 1.

Type `f0801215d4119e8eef90a8ac7f9ac3ff` is a surface record. After the content header it stores three record references, `state:u8`, `form:u32`, and `value:u32`; segment major versions above 21 add one u32 extension value after the three references. Form 25 is a plane: nine f64 values follow and end the record. They are the origin, the u axis, and the v axis in internal centimetres. The plane normal is the cross product of the u and v axes. An extrusion face target has a zero extension.

Type `42df52ced011d0d20008ccbc0663dc09` is a work plane. After the content header it stores `entity_flags:u32` and two u32 values. Segment major versions above 21 add one u32 value. An optional u32 `0x203` and one u32 value follow. The record ends with nine f64 values: the origin, the u axis, and the v axis in internal centimetres. The plane normal is the cross product of the u and v axes. A work plane that owns a feature label is a browser node. A browser work plane whose u and v axes are orthonormal is a datum plane with that origin, u axis, and normal. A browser work plane with other axes stays a native work-plane feature with the loss `feature.work-plane-frame-native`.

The extrusion construction rules above describe feature-label class `3111a90cd0118b83000819b00524dc09`. Feature-label class `f0a497e5d411d8841000faa5bba647b5` identifies a surface extrusion. Its 30-slot form stores a part-operation enumeration with type value 5 and value 5 in slot 0. Slots 1 and 23 select the same boundary patch. Slot 16 selects a surface-body record of type `474d8790d011f8d10008cabc0663dc09`; slot 26 is null. The boundary patch selects a kernel profile body through its profile-selection and kernel-entity-link records. The selected body contains an open or closed wire.

A constant-radius edge fillet has form value 0 in slot 11, null slots 1 and 10, a nonempty ordered edge-set collection in slot 0, and a result object collection in slot 15. Fillet form value 0 is an edge fillet. Each edge set supplies its own positive radius and edge collection. Selection type 4 with value 0 lists the exact edge items. The continuity Boolean of each edge set is false. A sheet-metal corner round can have a null continuity reference; this supplies no additional continuity condition. The fifth edge-set reference is absent, null, or a false Boolean.

Type `534d8790d011f8d10008cabc0663dc09` stores an edge naming selector. An edge item's index-reference values name selector `source_index` values in the same PmDc segment. They are not record ordinals or ASM entity indexes.

The fixed selector prefix after the content header contains `state:i32`, `enabled:u8`, `form:u8`, a body reference, `topology:u32`, `qualifier:u32`, and `naming_form:u32`. The direct edge form stores −1, 1, 5, a surface-body reference, 2, 0, and 2. In PmDc segment major version 19 the selector has no `qualifier` word: the direct edge form stores −1, 1, 5, a surface-body reference, 2, and 2. A type-8 integer array follows. Its sole value is the generating feature's content-header `source_index`. The next type-8 integer array is empty. A zero byte, an edge-item reference, a context reference, and `key:u32` end the selector.

The selector's edge-item reference selects its owning edge-item record in the same PmDc segment. An edge item with an empty index-reference list can be the target of this reference.

In a schema-44 ASM carrier, an `ATTRIB_CUSTOM-attrib` record has five common attribute fields followed by a family string and family values. The common fields are an attribute reference, a signed integer, two attribute-chain references, and the owning entity reference. Family `INV_NMX_MATCHED_ATTRIB` stores the integers 1, 44, 1, `selector_source_index`, −1, `key`, and `qualifier`. Qualifier 0 identifies the selected edge; qualifier 1 can identify its vertices. The pair (`selector_source_index`, `key`) joins the direct edge selector to its matching attribute. The owner reference identifies the edge record in the ASM entity-revision namespace. The attribute and edge can be in the construction-history archive rather than the active body.

In a schema-44 carrier, generated naming families `INV_NMX_GEN_TAG`, `INV_NMX_SWEEPGENERATED_TAG`, `INV_NMX_BLEND_TAG`, `INV_NMX_FLANGE_TAG`, and `INV_NMX_BEND_TAG` have the same five common attribute fields. Their family values start with 1, 44, 1, `feature_source_index`, and −1. The source index names the generating feature, including a plate, bend, or corner. The owning entity reference names the attributed face, edge, or vertex. The remaining integers are part of the generated name. A generated source index identifies a naming contribution; it does not identify a browser history position or a body revision.

Family `INV_NMX_DUP` stores the integers 1 and 44, then four opaque integer key values. It has no feature-source field. Its owner also has a separate generated-name attribute, which supplies the feature source. The four key values form an ordered opaque tuple, separate from that source name.

In schema-38, schema-40, and schema-42 carriers, class `NMx_Matched_Entity-NamingMatching-attrib` has five common attribute fields followed by the integers `schema`, 1, `selector_source_index`, −1, `key`, and `qualifier`, where `schema` is the carrier schema. It joins the direct edge selector as family `INV_NMX_MATCHED_ATTRIB` does in a schema-44 carrier. In schema-38, schema-40, and schema-42 carriers, the class chains `NMx_Generated_Brep_tag`, `NMx_Sweep_Generated_Brep_tag`, `NMx_flange_tag`, `NMx_blend_tag`, `NMx_bend_tag`, and `NMx_Hole_Brep_tag` continue with `NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib`. After the five common attribute fields they store the carrier schema, a positive source-name count, that many ordered `(feature_source_index, name_value)` integer pairs, and a family-specific integer tail. The tail lengths are 3, 10, 7, 7, 8, and 2 respectively, in the class order stated above. A source index in any pair names a feature in that generated entity name. `name_value` is a signed naming value. Its value can be −1. The source-name count bounds the pair vector; the tail is not part of that vector.

A generated name on that same owning edge identifies the edge in the generating feature's result. Family `INV_NMX_GEN_TAG` stores 1, 44, 1, `feature_source_index`, −1, `associative_id`, 0, and `role`. Edge roles are 2, 10, and 18. Family `INV_NMX_SWEEPGENERATED_TAG` stores the same first seven integers followed by `role` and seven zero integers. Edge roles 10 and 18 are distinct names. `feature_source_index` equals the generator value in the selector. The associative identifier names an entity-style link in the sketch consumed by that feature. Role 18 uses a point link; roles 2 and 10 use a curve link. The family, associative identifier, qualifier, and role together form a feature-local persistent name. The name is scoped to the generating feature's result. The matching attribute selects the stored owning edge revision. Its start and end vertex references select the stored point revisions; its curve reference and parameter endpoints select its carrier interval. A straight carrier uses its stored origin and direction, with the edge sense applied to the parameters. The midpoint of the interval supplies an interior point, and the signed carrier direction supplies its unit tangent. These references identify geometry at the selected history input independently of the active body's edge records. The construction-history record framing and entity-revision namespace follow [asm.md §3.2](asm.md#32-delta_state-records).

In a schema-38, schema-40, or schema-42 carrier, an `NMx_Generated_Brep_tag` or `NMx_Sweep_Generated_Brep_tag` edge name with source-name count 1 and name value −1 stores `associative_id`, 0, and `role` as its first three tail integers, as `INV_NMX_GEN_TAG` does; the sweep tail continues with seven zero integers, as `INV_NMX_SWEEPGENERATED_TAG` does. The `feature_source_index` of that name is the generator of the selector.

An equal-distance edge chamfer uses slot 0 for its edge collection, slot 2 for its length, slot 4 for the chamfer form, slot 5 for direction reversal, and slot 11 for the result object collection. The slot-5 direction Boolean is true. Chamfer values 0, 1, and 2 mean equal distance, distance and angle, and two distances. The equal-distance branch does not consume an oriented support face.

In chamfer forms 1 and 2, slot 1 selects the support-face collection. Slot 2 is a length parameter; slot 3 is a second length parameter; slot 10 is an angle parameter in radians. Form 1 uses the slot-2 distance and slot-10 angle. The distance is positive and the angle is strictly between zero and pi. Form 2 uses the two positive lengths in slots 2 and 3. Equal values in those two slots define equal setbacks on both support sides, independent of side order. The native oriented selection consists of the feature record, its slot-0 edge collection, its slot-1 support-face collection, and its slot-5 direction reversal.

A revolve has 20 property slots. A one-sided closed-profile solid revolve has these slot roles and control values:

| Slot | Property or value |
| --- | --- |
| 0 | Part operation, with values 1 through 4 as for an extrusion |
| 1 | Boundary patch |
| 2 | Work axis |
| 3 | Extent enumeration |
| 4 | Angular parameter, in radians |
| 5 | Enumeration `c26aa0c728412312abf0728f9d598bc9`, with type value 3 |
| 6, 8, 13, 14, 15, 18, 19 | Null references |
| 7 | Feature-dimension list with one dimension of the slot-4 parameter |
| 9 | Auxiliary enumeration with type value 4 and value 0 |
| 10 | Boolean true |
| 11 | Null or repeated slot-1 reference |
| 12 | Boolean false |
| 16 | Boolean control of type `284d8790d011f8d10008cabc0663dc09`, with value 0 or 1 |
| 17 | Result object collection |

The feature label's sole participant identifies the owning planar sketch. The boundary patch selects its profile as for an extrusion. Slot 1 selects the stored profile when slot 11 is null. A non-null slot 11 repeats the same boundary-patch record, including every member of a multi-member patch. It supplies no additional profile selection. Extent value 1 is a fixed angular travel given by slot 4. With slot-5 value 0, the travel is positive rotation about the work-axis span. The angle is greater than zero. Extent value 3 is a full revolution. Its stored slot-4 angle is inactive. The profile lies on one side of the work axis; profile points on the axis belong to neither side. The slot-5 enumeration uses the versioned content header, followed by `type_value:i16` and `value:u16`, and ends there. A full revolution has slot-5 value 0 or 1. Both values specify a full turn.

Type `b1377779d2116794a00049803603c8c9` is a feature parameter dimension. It starts with `header_value:u32`, `header_id:u16`, and a type-2 annotation list. The annotation list prefix contains `marker:u32 = 0x30000002` and `item_count:u32`. A nonzero count adds two u32 metadata words and annotation data bytes. A zero count has no metadata or data. The annotation data fills the record up to the nine-byte terminal tail. Its tail contains five annotation bytes and a parameter reference. The terminal parameter reference identifies the parameter that the dimension annotates. The dimension header has no content-header stamps or flags extension.

Type `896cf08ed1113c0460007cb801f31bb0` is a work axis. After the content header it stores `entity_flags:u32` and two u32 values; segment major versions above 21 add one u32 value. A type-2 reference list follows, then an optional u32 `0x203`, two f64 triples, and `state:u8`. The first triple is a point on the axis and the second is the span from that point to the other end of the axis segment, both in internal centimetres. The axis direction is the span direction. Origin work axes span from −1 to 1 along their direction.

The revolve work-axis form has zero entity flags, zero u32 values, an empty reference list, no `0x203` marker, state 0 or 1, and a nonzero span.

A hole uses slots 0 through 6 for hole form, diameter, depth, entry diameter, entry depth, entry angle, and drill-point angle. Slots 7, 8, 9, 16, 17, 21, and 24 identify its center-point selection, transform, extent, direction, Boolean, placement, and result object collection. Slots 26 and 27 optionally select Boolean properties. Hole-form values 0, 1, 2, and 3 mean drilled, countersink, counterbore, and spotface. The bore diameter is positive. A counterbore or spotface has a cylindrical entry recess with the slot-3 diameter and slot-4 depth. The recess diameter exceeds the bore diameter, and its depth is positive. For a drilled hole, counterbore, or spotface, the slot-6 drill-point angle defines the bottom: zero gives a flat bottom; a nonzero value is an included angle strictly between zero and pi. A countersink uses the slot-3 entry diameter and slot-5 included angle. The entry diameter exceeds the bore diameter, and the included angle is strictly between zero and pi.

Type `b1226593d2115359a00021803603c8c9` stores a hole center-point selection. The content header is followed by a type-2 reference list and the record ends. When slot 21 is null, the center-point list is nonempty and the hole label has one participant, which identifies the owning planar sketch. Each center reference selects a distinct point in that sketch's entity list. The point's sketch reference selects that sketch. The sketch's transform and direction references equal hole slots 8 and 16. A selected point `(u, v)` gives the model-space position `M * (u, v, 0, 1)`, where `M` is the slot-8 transform. Positions and transform translations use internal centimetres. The direction record is the sketch-plane normal. With a false slot-17 Boolean, the hole drills along the negated normal, into the material below the sketch plane. A non-null slot-21 placement repeats the slot-8 transform reference and has non-null point and value references. Its position is the transform translation.

Hole extent values 1, 4, 5, and 7 mean fixed depth, through next, through all, and to. A fixed-depth hole uses the positive slot-2 depth. A to hole has a slot-15 termination flag. Exactly one of slots 13 and 14 is non-null. Flag value 1 selects the slot-13 work plane with the plane-frame grammar above. Flag value 0 selects the slot-14 native face-selection record of type `ccb59f50d1114f6b0008a1ba32a3dc09`. The face-item grammar above applies. One nonzero full u32 index selects one terminating face in the native face-selection graph. The work-plane target carries a plane frame; the face target carries a face selection. Neither target adds an offset.

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

A result object collection contains one or more distinct surface-body references in the same PmDc segment. This ordered surface-body list is the feature's native result-body identity.

Feature-label class identifiers `dea6a68f4c4b529188ca31b5630198c8`, `e99f832b7a4295a1d9763383bea17cce`, and `e6da3f26924b5dab8e1bc5a93fead7de` identify Move, MoveBody, and Rotate respectively. The class identifier, not the display name, identifies the operation.

Each class has a three-slot body-motion form. Slot 0 is an object collection of the selected surface-body wrappers. The collection members are distinct wrappers in the same PmDc segment. Slot 1 is a transform list of type `29fe0008d511dd3510003697ab9f0ab5`. Slot 2 contains feature dimensions. A transform-list record contains the versioned content header and one type-2 reference list, then ends. In the single-transform form the list has one member, a transform of type `184d8790d011f8d10008cabc0663dc09`. It applies to every selected body. The matrix maps a model-space point `p` to `R*p + t`, with column vectors. The linear part is a proper orthogonal rotation; the last row is `0, 0, 0, 1`. Translation values use internal centimetres. The selected bodies keep their identities; this form creates no copies.

Type `83aadad54f4375dacd1003b260a8a422` is an ordered body selection. It contains the versioned content header and one type-2 reference list, then ends. Each non-null member names a distinct surface-body wrapper in the same PmDc segment. A null member selects no body. The wrapper identity is the persistent body selection. Combine slot 0, Split slot 7, and the 13-slot Move form's slot 8 use this selection.

Move also has a 13-slot face-motion form. Slot 0 is a face collection of type `f1b2bc24d01121dd0008d0bc0663dc09`. Slot 1 is the face-motion mode. Slot 8 is the selection of affected bodies. Slot 9 contains feature dimensions. Slot 11 is a Boolean. Slot 12 is the model-space motion transform. Face-motion mode 2 with null slots 2 through 7 and 10, a true slot-11 Boolean, and an identity linear part translates the selected faces by the transform translation. The body identities in slot 8 persist after this motion.

A feature with label class `336723c07e40eacb2eff669e9add87f8` is Combine. Its property list has four entries. Slot 0 is a body selector with exactly one target-body wrapper. Slot 1 is an object collection with one or more tool-body wrappers, in order. Slot 2 is a part-operation enumeration with type value 5: values 2, 3, and 4 mean cut, join, and intersection. Slot 3 is the keep-tools Boolean: true retains the tool bodies and false consumes them. Target and tool wrapper identities are distinct, and each tool occurs once. The result retains the target wrapper identity. The slot-0 selector names this result identity. The slot-1 collection supplies operands, not result identities.

Types `44326720d211c51d60002aab01f31bb0` and `b5a9d9fad211053360002cab01f31bb0` store rectangular-pattern and mirror feature records. Both start with the generic 30-byte feature prefix through `outline_value`, followed by a type-2 property-reference list, `value:u32`, a type-2 participant-reference list, six property references, and `control:u8`.

For segment major versions 15 through 20, the rectangular-pattern suffix contains 20 more property references. From segment major version 21, it contains 26 more property references. From segment major version 29, eight u32 extension values follow the first 12 of these references, and from version 30 six more u32 extension values end the suffix. The mirror suffix contains five more property references, six u32 extension values from segment major version 21, and two final property references. From segment major version 29, eight u32 extension values separate the two final property references. The record ends after the applicable suffix.

A feature label with class identifier `a326c2b6d211263660002cab01f31bb0` identifies a mirror. The 13 property slots have the same indices in each suffix form. Extension values do not take slot indices. The identity-frame, zero-option mirror has these operands:

| Slot | Record | Value or role |
| --- | --- | --- |
| 0 | `184d8790d011f8d10008cabc0663dc09` | Identity operand placement matrix. |
| 1 | `bef06e8d354cc7aad7c88db7ea9b0ffb` | Selection control `0x30000004`, value 0. |
| 2 | `dfd51dbbd1116e72000817bd0663dc09` | Nominal and model values 0. |
| 3 | Null | No auxiliary operand. |
| 4 | `5a4a6899d2117fec6000b0abc8433cb0` | Nine zero option bytes. |
| 5 | `71f23ed8d2115094a00049803603c8c9` | Empty feature-dimension list. |
| 6 | `284d8790d011f8d10008cabc0663dc09` | False selects feature mode; true selects body mode. |
| 7 | `284d8790d011f8d10008cabc0663dc09` | False. |
| 8 | Null | No generated-instance map. |
| 9 | Object collection in feature mode; null in body mode | Affected result surface-body records. |
| 10 | Null in feature mode; object collection in body mode | Source surface-body records. |
| 11 | Null or `284d8790d011f8d10008cabc0663dc09` | Null or false. |
| 12 | `42df52ced011d0d20008ccbc0663dc09` | Reflection plane in part coordinates. |

Mirror auxiliary payloads follow the versioned content header and stamps. In the empty-selection form, type `bef06e8d354cc7aad7c88db7ea9b0ffb` has an 8-byte payload: `control:u32` at offset 0 and `value:u32` at offset 4. In the zero-option form, type `5a4a6899d2117fec6000b0abc8433cb0` has nine zero bytes at offset 0. Offsets are relative to the end of the header and stamps. Each record ends after this payload.

Boolean operands and the slot-2 variable have `name_value` 0. The pattern's property list is empty, `value` is 0, and `control` is 0. Through segment major version 20, no extension values occur. From version 21, extension value 0 references a false Boolean property, and extension values 1 through 5 are zero. From version 29, extension values 6 through 13 are `[0, 0, 0, 0, 0, 2, 0, 0]`.

The slot-12 plane has zero entity flags, three zero u32 values through segment major version 21 or four thereafter, and no frame marker. Its u and v axes are perpendicular unit vectors. With plane origin `o` and unit normal `n = normalize(u × v)`, reflection is `p' = p - 2 n dot(n, p - o)`. Its linear part is `I - 2 n nᵀ`, and its translation is `2 n dot(n, o)`. The plane origin uses internal centimetres. The slot-0 matrix places the operands; it does not store the reflection.

In feature mode, the nonempty pattern participant list gives distinct ordered seeds. Each seed is a generic feature or pattern feature that precedes the mirror under the same model root. Slot 9 gives a nonempty ordered collection of distinct result surface-body records. In body mode, the pattern participant list is empty. Slot 10 gives a nonempty ordered collection of distinct source surface-body records. With a null slot 8, the source and result use the same surface-body identities. In both modes, the mirror label's participant list is empty.

### 13.1 Feature history

A model root (§11) also owns the browser feature tree of its model. The root label of a model root is the feature label whose owner reference selects the model root. The tree of the document model root is the part modeling history. The tree of a flat-pattern model root is the flat-pattern history. A segment has a history only when it has exactly one document model root.

The participant list of a root label is the ordered top level of the browser tree. Each participant is one tree member:

- A feature, pattern-feature, planar-sketch, or work-plane record is a history node.
- The end-of-features record is the end-of-part marker. Top-level members after the marker are below the end of part.
- Another record that owns a feature label is a group. The participants of its label are its nested members, in order.

The participants of a feature label that are planar-sketch records are the sketches that the feature consumes. The participants of a work-plane label that are work-plane records are the work planes that it is built from. A work plane that only such a label lists takes its position immediately before the work plane that lists it. History order is the post-order of the tree: the consumed sketches of a feature precede it, and the nested members of a group take the position of the group. A record has one position; a repeated reference does not move it. The model history precedes the flat-pattern history.

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

A non-null property reference in a table slot selects one of the stated record types. A null reference selects no record. The two loft classes use the same slot types.

Loft property slots are:

| Slot | Operand | Record type |
| ---: | --- | --- |
| 0 | Ordered section list | `509e2f31d1110344000881ba32a3dc09` |
| 1 | Part operation | `28be9a72d111440900084eba32a3dc09` |
| 2 | Boolean construction control | `284d8790d011f8d10008cabc0663dc09` |
| 6 | Surface-loft result body wrapper | `474d8790d011f8d10008cabc0663dc09` |
| 7 | Auxiliary loft-selection list | `509e2f31d1110344000881ba32a3dc09` |
| 9 | Boolean construction control | `284d8790d011f8d10008cabc0663dc09` |
| 10 | Loft construction enumeration | `02e2e3e0d111480900084eba32a3dc09` |
| 13 | Solid-loft result object collection | `ae70680ed14a1e86d76248b03c2a96e1` |

The positional two-section solid-loft form has 14 property slots in a type-2 reference list. Slots 3, 4, 5, 6, 7, 8, 11, and 12 are null. Slot 1 selects an enumeration with type 5 and value 1. Slots 2 and 9 select false Boolean records. Slot 10 selects an enumeration with type 2 and value 0. The list at slot 0 has two distinct section references. The collection at slot 13 has one result-body reference.

Each section in this form has `value = 0`, `map_value = 0`, and `source_value = 1`. Its condition has enumeration type 2 and value 0. Its option is null or false. The first numeric parameter is dimensionless, with nominal and model values 0. The second numeric parameter is angular, with nominal and model values π/2 radians.

Sweep property slots are:

| Slot | Operand | Record type |
| ---: | --- | --- |
| 0 | Boundary patch | `91739422d11107cf000835bd0663dc09` |
| 1 | Path selection | `3b2477a4d1118f96000826bd0663dc09` or `473f20fcd11101cf000835bd0663dc09` |
| 2 | Part operation | `28be9a72d111440900084eba32a3dc09` |
| 3 | Angular parameter | `264d8790d011f8d10008cabc0663dc09` |
| 4 | Extent enumeration | `297d6392d1113cb9000831bd0663dc09` |
| 7 | Feature dimensions | `71f23ed8d2115094a00049803603c8c9` |
| 8 | Sweep construction enumeration | `b7849ca26d4811e38b95b3a06051b440` |
| 9 | Sweep orientation enumeration | `ed2ef031904b9b04d79918b91f1619c8` |
| 10 | Sweep scaling enumeration | `e211b05c3e46583e928a0cbf3a085834` |
| 15 | Result object collection | `ae70680ed14a1e86d76248b03c2a96e1` |

The zero-control circular solid sweep form uses a type-2 property vector with at least 16 slots. Slots 5, 6, 11, 12, 13, and 14, and all slots after 15, are null. Slot 2 has enumeration type 5 and value 1, which selects a new solid body. Slot 4 has the versioned extent type from section 13 and value 3. Slots 8, 9, and 10 have type/value pairs `(3, 0)`, `(2, 0)`, and `(2, 0)`. Slot 3 is angular, with nominal and model values 0. A non-null slot 7 selects a feature-dimension list with one member; that member's parameter reference selects the slot-3 parameter record. Slot 15 contains one non-null result-body reference.

Slot 0 contains one profile selection with value 0 and a non-null kernel-entity link. Its selected boundary is one complete circle in one consumed planar sketch. Slot 1 is a profile selection with value 0 and a non-null kernel-entity link. Its selected path is one complete ordered open wire. The complete result consists of two planar circular caps at the path endpoints and one cylindrical or toroidal side face for each named path span. Each cap circle, cylindrical radius, and toroidal minor radius equals the section radius. All these faces belong to the generating sweep and to one solid body.

Schema-44 generated-face attributes use type `ATTRIB_CUSTOM-attrib`. Five common attribute fields precede a family string and the integers 1, 44, 1, `generator`, and −1. `generator` is the generating feature's content-header source index. The target of the attribute is the generated face.

Family `INV_NMX_SWEEPGENERATED_TAG` then stores `section`, `span`, `role`, and seven zero integers. Role 20 identifies a side face; `section` and `span` identify the associative names of its section curve and path curve. Roles 4 and 12 identify the two terminal cap faces and have zero `section` and `span` values.

Family `INV_NMX_GEN_TAG` with the trailing integers 0, `section`, 260 identifies a loft cap. Family `INV_NMX_ORDERED_GEN_TAG` with the trailing integer 2 and two section groups identifies a loft side face. Each group is a curve name, a section, 258, and two tail integers. The first group names `curve` and `first_section`, and the second group names `last_curve` and `last_section`. One group has the tail 0, 0, and the other group has the tail 4, `member`. The cap `section` and the side `first_section` and `last_section` are source indices of the section sketches. `member` is the one-based side-face member index. In the positional form, `curve` is the associative name of a curve in the selected first-section boundary. `last_curve` is the associative name of a curve in the selected last-section boundary. The side face stores a procedural loft surface. Its ordered profile members state any support surfaces and directions; a null support surface and an absent direction impose neither support continuity nor a direction constraint. The cap and side attributes refer to the same generating feature.

A sweep path selection uses the profile-selection grammar. Its kernel-entity link selects one keyed wire. The wire's ordered edges are the complete selected chain. A path wire can be open. Its edge naming attributes use the same naming-type and associative-name grammar as section boundaries.

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

The slot-0 enumeration has type value 3. Value 3 is a solid split: the slot-1 Boolean is false, slot 2 is null, the slot-4 boundary-patch list is empty, the slot-7 selection has one surface-body wrapper, and the slot-9 collection has two distinct surface-body wrappers. The slot-7 wrapper is the body that the tool partitions. The slot-9 wrappers are the result bodies, one for each side of the tool. A work-plane tool is a work-plane browser node (section 13.1), and the split extends its plane through the body. A surface tool is a surface-body wrapper distinct from the slot-7 wrapper, and the split extends its faces through the body.

### 13.4 Stored-body base features

Feature-label class `2d11a90cd0118b83000819b00524dc09` identifies a stored-body base feature. Its property vector has three slots. Slots 0 and 1 select surface-body wrappers of type `474d8790d011f8d10008cabc0663dc09`; slot 2 is null. Slot 0 is the stored source body. Its body reference is null or a qualified reference to a kernel-entity link in the same PmDc segment. The link selects a stored kernel-body record in that segment. Its entity identifier selects the unique body with that nonnegative ASM body key in the stored carrier. Slot 1 is the result-body identity. Its body reference is a non-null unqualified external reference. The two wrappers have distinct record identities. Their kernel-entity links have state 0 and `selection_source` equal to the referencing wrapper's `source_index`.

An unqualified surface-body reference is a one-based index in its PmDc metadata section 10. A qualified surface-body reference is a one-based record index in its own segment. The packed word 0 is null. A qualified reference with index 0 does not select a body.

In the surface-body reference form, metadata section 7 has a segment-pair form with 32-byte entries: `segment_id:bytes[16]`, `revision_id:bytes[16]`. Its discriminator is its entry count. Section 8 contains 20-byte revision entries: `revision_id:bytes[16]`, `segment_pair_index:u16`, `reference_count:u16`. The segment-pair index is zero-based into section 7. The revision identifier identifies the referenced revision; it need not equal the segment pair's revision identifier. Section 10 contains 8-byte external entries: `revision_index:u32`, `key_extension:u16`, `header_id:u16`. The revision index is zero-based into section 8. Each section's payload length equals its entry count times its entry width. A revision entry's reference count equals the number of section-10 entries that select it.

The zero-extension external form has `key_extension = 0` and selects the unique kernel-entity link whose `header_value` is 0 and whose `header_id` equals the external entry's header identifier, in the unique segment whose identifier equals the selected section-7 segment identifier. The current-model Base result form selects a kernel-entity link in the document model root's named PmBRep segment. The link's qualified body reference and the carrier footer's selected key select the same active carrier record. The link's entity identifier joins the unique body with that ASM body key in this carrier. A body key is scoped to its carrier. A result identity can survive later changes to that body; it does not state that the current body is the Base source shape. A result whose body is absent from the current model has no current-body join.

This current-model join applies to every surface-body wrapper. A nonzero unqualified wrapper body reference addresses metadata section 10 in the wrapper's segment. Its zero-extension entry, revision entry, and segment-pair entry select the PmBRep kernel-entity link. The document model root names that PmBRep segment. The link has state 0 and a selection source equal to the wrapper's source index. Its qualified carrier reference equals the active carrier footer key. The carrier identity and entity identifier select one ASM body key and thus one current body. Null references, stored-carrier references, and body keys absent from the active carrier have no current-body identity.

A feature result collection lists persistent surface-body wrapper identities in member order. Each wrapper with a current-body join identifies the produced or modified current body. Combine retains its target wrapper in its result. Move, MoveBody, and Rotate retain the selected wrapper identities. Split names its result wrappers separately from its target wrapper. A selection has a complete current-body join only when each selected wrapper names one current body and those body identities are distinct.

The derived stored-body form has feature-label class `eecb298e2c45650820104eb55d062c80`. In segment major version 19 its three property slots select the stored source-body wrapper, the result-body wrapper, and a source property of type `3cc048210042420ba62103b20b7f4891`, respectively. All three references are non-null and qualified. The label participant list is empty. The source-body wrapper has a non-null qualified body reference. The two body wrappers have the stored-source and current-result identity rules above. The source property in segment major version 19 contains the content header followed by one qualified component reference, then ends.

In segment major version 19, a derived component has type `a034363fd31191b5c0000385e89c6b4f` and a state content header with no stamps. Its 32-byte body contains:

```text
state i32 = -1
references u32[4]
marker u32 = 0x30000004
count u32 = 0
terminal u32
```

The component ends after `terminal`. The four references and the terminal reference are non-null qualified references in the same segment. Their target types, in order, are `9f34363fd31191b5c0000385e89c6b4f`, `604d8790d011f8d10008cabc0663dc09`, `184d8790d011f8d10008cabc0663dc09`, `d77e5d25d311f2b5c0000385e89c6b4f`, and `a7fbcb1db64f16cfe979019eae94a4df`. The third reference selects a transform record. The result-body wrapper selects the current-model body independently of that reference.

### 13.5 Attached plates and bend properties

The 17-property attached-plate form has these slots:

| Slot | Operand |
| ---: | --- |
| 0 | Construction-wire profile selection |
| 1 | Direction reference |
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

The 15-property attached-plate form has the operands of slots 0 through 14 above, except that slot 6 holds a surface-body property. That body is the result of the plate. The form has no slot 15 and no slot 16.

One flange control form has the following values and reference relations. Direction comparisons use normalized vectors. Parallel directions have dot product at least `1 - 1e-10`; antiparallel directions have dot product at most `-1 + 1e-10`; perpendicular directions have absolute dot product at most `1e-10`.

| Operand | Value or relation |
| --- | --- |
| Plate property count | 17, or 15 with a surface-body property in slot 6 |
| Plate slots 5, 14 | Null |
| Plate slot 6 | Null in the 17-property form; surface-body property in the 15-property form |
| Plate slot 0 | Type `473f20fcd11101cf000835bd0663dc09`, profile selection, value 0 |
| Plate slot 1 | Antiparallel to the first attachment direction |
| Plate slot 2 | Boolean false or enumeration `(2, 0)` places thickness along the consumed sketch normal |
| Plate slot 3 | Positive thickness length |
| Plate slot 7 | Empty edge collection |
| Plate slot 8 | Feature-dimension reference list |
| Plate slot 9 | Corner control with the fields below |
| Plate slot 13 | Scalar 0 |
| Plate slot 16 | Part-operation enumeration `(5, 3)` in the 17-property form |
| Attachment control, first option, second option | Boolean false |
| Attachment first mode, second mode | Enumeration `(9, 1)` |
| Attachment states | `[1, 1, 1]` |
| Attachment first direction | Parallel or antiparallel to the consumed sketch U axis; its sign does not change thickness side |
| Attachment second direction | Parallel to the consumed sketch V axis, `normal × U` |
| Attachment thickness | Same reference as plate slot 3 |
| Attachment extension | Absent, or state 10 with repeated thickness, radius, attachment, relief width, minimum remnant, and option references |
| Relief control | Boolean true |
| Relief form | Same enumeration type and value as the document-style bend-relief form |
| Relief width, minimum remnant | Same references as the attachment operands |
| Relief width, depth | Positive lengths |
| Relief minimum remnant, seam gap | Nonnegative lengths |
| Transition control | Boolean true |
| Transition form | Same enumeration type and value as the document-style transition form |
| Transition radius | Nonnegative length |
| Corner control, option | Boolean |
| Corner relief, corner, and secondary forms | Enumerations |
| Corner raw value | 0 |
| Corner gap, size, Jacobi radius | Nonnegative lengths |
| Bend property count | 11 with the 17-property plate; 9 with the 15-property plate |
| Bend slot 5 | Null in the 11-property form; same reference as plate slot 6 in the 9-property form |
| Bend slot 6 | Feature-dimension reference list |
| Bend slot 8 | Scalar 6, state 1 in the 11-property form; scalar 0 without a state in the 9-property form |
| Bend slot 10 | Boolean true in the 11-property form |
| Bend slots 0, 2, 3, 4, 7 | Same references as plate slots 7, 3, 4, 10, 11 respectively |
| Bend slot 9 | Same reference as plate slot 15 in the 11-property form |
| Bend slot 1 | Same positive radius-length reference as the attachment radius |
| Document style | Same segment and save-version major as the plate |

The corner-control values configure the flange's corner child feature. The plate and bend do not consume them.

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

The 9-property bend form has slots 0 through 8 of that table. Its slot 5 holds the surface-body property shared with plate slot 6, and it has no slot 9 and no slot 10.

Type `08dcddc3d311d0976000a7a99dccefb0` stores the flange parent. It contains the state content header without stamps, `state:i32`, a type-2 reference list, and `value:u32`, then ends. The reference list selects the plate, bend, and corner generic-feature records in that order. The parent's feature label has four ordered participants: the consumed planar sketch, that plate, that bend, and that corner. The consumed sketch defines the plate's planar profile. The attached-plate slot-0 kernel selection stores its construction wire; that wire is not the closed boundary of the consumed sketch.

For a plate owned by a `08dcddc3d311d0976000a7a99dccefb0` flange record, slot 1 is antiparallel to the first attachment direction. Slot 1 is in the sketch plane. It does not define the thickness direction. Slot 2 Boolean false or enumeration `(2, 0)` places the plate between the sketch plane and its positive-normal offset by the thickness. Reversing the first attachment direction does not reverse this offset.

The bend radius in slot 1 repeats the radius reference of the attachment control. The flange record owns ordered plate, bend, and corner children. Each child is a separate generic feature with its own source index, label owner, state, and history position. Sharing an attachment control identifies the plate and bend pair. The bend is not a property sub-record of the plate. The plate and bend refer to a common result-body wrapper; result identity alone does not select a history revision. Neither the attachment control nor the bend's property vector contains an angle parameter.

Feature-label classes `7f588068b74378a0559515946f7a1c12`, `852f6bb0d411db6f60004daa9dccefb0`, and `aaf21093d344209a04cc799656cd7e09` identify a corner, a hem, and a flat-pattern definition respectively. The corner has six or seven properties. Its slots are:

| Slot | Operand |
| ---: | --- |
| 0 | Corner control shared with plate slot 9 |
| 1 | Surface-body property shared with plate slot 6 in the six-property form; null in the seven-property form |
| 2 | Feature dimensions |
| 3 | Attachment selection list shared with plate slot 12 |
| 4 | Plate profile selection shared with plate slot 0 |
| 5 | Null |
| 6 | Result object collection; absent in the six-property form |

Each attachment selection wrapper selects an edge collection. Each edge item in that collection stores its ordered edge-selector source indexes. One wrapper can select several edges. The flange owns one bend feature independently of the number of selected edges. The corner control and selections are shared operands, not references to the parent flange. The corner result collection has a distinct identity or repeats the plate result collection. Distinct collections can contain the same surface-body member. The ordered flange children and label participants identify the owning plate, bend, and corner.

The single-edge unchanged corner form has one selected attachment edge and the shared plate/bend/corner operands above. Its edge selector resolves to one persistent generated edge of the preceding sheet through the naming join in section 13. The six-property form repeats plate slot 6 as its result-body property. The seven-property form repeats plate slot 15 as its result collection. Its corner form is type 8 value 3, its option is true, and its raw word is 0. Its control and relief enumerations are one of:

| Control | Relief form | Secondary form |
| --- | --- | --- |
| true | 304:103 | 304:303 |
| true | 501:103 | 501:303 |
| false | 501:100 | 501:303 |

The plate, bend, and corner occupy active positions in the document model history. The corner source index occurs in no generated-name attribute in the current carrier or its stored construction-history revisions. The record framing covers the carrier and archive, and each generated name uses a source-name prefix defined in this section or section 13. The corner leaves the input body geometry and topology unchanged. Its gap, size, Jacobi radius, and other control operands remain stored values. The selected edge count is one; the number of generic bend children alone does not identify this form.

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

The sheet-metal style record has type identifier `13234560d3117a0d6000ecb21d6eefb0`. For PmDc segment major versions 29 and 31, its operand prefix contains:

```text
prefix bytes[30]
thickness u32
bend_radius u32
bend_relief_form u32
bend_relief_width u32
bend_relief_depth u32
corner_relief_form u32
corner_relief_size u32
seam_gap u32
minimum_remnant u32
option u32
values u32[3]
transition_form u32
transition_radius u32
```

For PmDc segment major versions 20 and 23, the same operand sequence has `prefix bytes[26]` in place of `prefix bytes[30]`. For PmDc segment major version 19, the same operand sequence has `prefix bytes[26]` in place of `prefix bytes[30]` and `values u32[8]` in place of `values u32[3]`. The operand prefix ends after `transition_radius`; the record continues with a suffix.

The length operands reference the parameters `Thickness`, `BendRadius`, `BendReliefWidth`, `BendReliefDepth`, `CornerReliefSize`, `SeamGap`, `MinimumRemnant`, and `TransitionRadius`. The relief-form references select type `720c3e2bd311976060007aa99dccefb0`. The transition-form reference selects type `07a66130d4118895600053aa9dccefb0`. The option references a Boolean. Enumeration identity includes both `type_value` and `value`; equal values in distinct types do not identify equal forms. A length formula that consists of a parameter-reference expression to the style operand uses that style parameter. A literal expression is an explicit length operand, including when its evaluated value equals the style parameter.

The plate scalar control contains `value:u32` after its versioned content header and stamps. The base bend scalar control contains `value:u32` after that header and stamps. The extended bend scalar control contains `value:u32` and `state:u8`. Each record ends after its last stated field.

### 13.6 Rectangular patterns

Feature-label class `dac27a76d2110d2e60002aab01f31bb0` identifies a rectangular pattern. The participant list of the pattern record gives the seed feature records in seed order. Each seed is a distinct feature or pattern-feature record in the same segment and precedes the pattern in the same model history.

The suffix property slots are numbered from zero, including the six references before `control`. Slot 0 is the pattern placement transform. Slot 1 is the instance-suppression record. Slot 2 is the computation-mode integer variable. Slot 4 is the auxiliary selection-control record. Slot 5 is the feature-dimension collection. Slot 6 selects the seed mode: false selects features from the participant list; true selects bodies from the object collection in slot 10 and has an empty participant list. Feature mode uses slot 9 for the result object collection. The result collection has the surface-body identity rule stated above.

The direction-slot base is 12 in segment major versions 15 through 20, and 18 from version 21. For direction index `i` equal to 0 or 1, the slots are:

| Slot | Meaning |
| --- | --- |
| `base + i` | Instance count, including the seed, in the integer variable's equal nominal and model values |
| `base + 2 + i` | Spacing parameter, in internal centimetres |
| `base + 4 + i` | Direction selection |
| `base + 6 + i` | Direction control 0 Boolean |
| `base + 8 + i` | Direction control 1 Boolean |
| `base + 10 + i` | Direction control 2 Boolean |
| `base + 12 + i` | Direction control 3 Boolean |

A null second count selects one direction and requires null second spacing, direction, and control slots. A present second count selects two directions and requires its spacing, direction, and control records. Counts are positive u32 values. Their product is at least 2 and fits u32. The fixed-spacing feature branch has nominal and model computation-mode values zero and an identity placement transform. Its prefix property-reference list is empty, its `control` byte is zero, and its extension values are zero except extension value 5, which equals the product of the direction counts, with an absent second direction counted as 1. From segment major version 21, slots 12 through 17 are null in this branch. In this branch, the slot-6 and slot-11 Booleans are false, and all four controls of each present direction are false. The spacing is positive. The displacement of instance `(u, v)` is `u * spacing_0 * direction_0 + v * spacing_1 * direction_1`, with zero-based coordinates below the corresponding counts. A direction is normalized before its spacing is applied. The seed is at `(0, 0)`. A suppression pair `(i, j)` omits the instance at zero-based grid position `(i - 1, j - 1)`. The direction counts retain their nominal grid sizes.

Type `bef06e8d354cc7aad7c88db7ea9b0ffb` stores instance suppressions. After the content header, it stores the u16 pair `4, 0x3000` and a u32 count. A nonzero count adds two zero u32 metadata values, then that many pairs of u32 coordinates. The coordinates are one-based direction indices, including the seed. Each pair is unique, is inside the count rectangle, and is not `(1, 1)`. An empty list ends after its count. Feature suppression in the content-header flags is independent of these individual instance suppressions.

Type `5a4a6899d2117fec6000b0abc8433cb0` stores auxiliary selection controls. After the content header it stores two record references and one u8 control. The fixed-spacing feature branch has zero references and a zero control in this record, a false slot-11 Boolean, and null slots 3, 7, 8, and 10. A body seed is a member of the slot-10 object collection, with the same surface-body reference grammar as a result collection.

A direct direction selection names a direction record of type `40df52ced011d0d20008ccbc0663dc09`. This record has entity flags 0, parameter 0, and an absent or zero extension in the fixed-spacing feature branch. A work-axis geometry reference in a cached selection uses the work-axis grammar above, including its versioned content-header stamps. The fixed-spacing work-axis form has zero entity flags and u32 values, an empty reference list, no `0x203` marker, state 0 or 1, and a nonzero span.

Types `7b4544a2ab4d9e16850c48a21edf9e3a` and `aa7b97a57045795897b51396ffc5d631` store cached line selections. Their content headers have no stamps. The single-line branch has selection form 1 and curve form 23. The header is followed by `state:i32`, a reversal-Boolean reference, a geometry reference, `selection_form:u32`, u16 value 0, and `curve_form:u32`. Selection form 1 with curve form 23 stores three finite f64 triples: a line origin, a nonzero line span, and a selection point. A u8 value 1 follows. Type `7b4544a2ab4d9e16850c48a21edf9e3a` ends with two source references; type `aa7b97a57045795897b51396ffc5d631` ends with one. The fixed-spacing feature branch has state 1 or 2 for type `7b4544a2ab4d9e16850c48a21edf9e3a`, state −1 for type `aa7b97a57045795897b51396ffc5d631`, and non-null geometry and source references that resolve in the same segment. The span supplies the direction. A geometry reference to a work axis has the same normalized span as that work axis. The referenced reversal Boolean negates the direction before the spacing is applied.

### 13.7 DeleteFace property vector

Feature-label class `686e16d3334f637161a6a19bdd3d2be8` identifies DeleteFace. Its four-slot form stores a type-2 property-reference list with these slots:

| Slot | Record type | Property |
| ---: | --- | --- |
| 0 | `f1b2bc24d01121dd0008d0bc0663dc09` | Face collection |
| 1 | `284d8790d011f8d10008cabc0663dc09` | Boolean |
| 2 | `284d8790d011f8d10008cabc0663dc09` | Boolean |
| 3 | `83aadad54f4375dacd1003b260a8a422` | Ordered body selection |

The face collection references face-item records of type `ccb59f50d1114f6b0008a1ba32a3dc09`. Each face item stores its type-2 u32 index-reference list with the grammar in section 13. The ordered body selection references surface-body wrappers of type `474d8790d011f8d10008cabc0663dc09`. Property references, face-collection member references, and body-selection member references select records in the feature's PmDc segment.

### 13.8 Tagging selections

Feature-label class `e899dbdfd31165afc0000185e89c6b4f` identifies Tagging. In segment major versions 19, 23, and 31, its owner uses the generic feature grammar of section 13. The property vector uses the type-2 reference-list grammar of section 12 and has one slot:

| Slot | Property |
| --- | --- |
| 0 | Non-null qualified reference to an object collection of type `ae70680ed14a1e86d76248b03c2a96e1` |

The collection contains a type-2 reference list. Each member is a non-null qualified reference to a surface-body wrapper of type `474d8790d011f8d10008cabc0663dc09` in the same PmDc segment. Each wrapper has a non-null unqualified body reference, with the external-reference grammar of section 13.4. The Tagging label has an empty participant list.

### 13.9 Stored-body composites

Feature-label class `0843c7c2244c2dee62b95ab01118d108` identifies a stored-body composite. In segment major versions 19, 23, and 25, its owner uses the generic feature grammar of section 13. Its type-2 property vector has three non-null qualified references:

| Slot | Record type | Role |
| ---: | --- | --- |
| 0 | `1f4102caa041e9f2ffd878825538b1fe` | Stored source-body set |
| 1 | `1f4102caa041e9f2ffd878825538b1fe` | Result-body set |
| 2 | `284d8790d011f8d10008cabc0663dc09` | Boolean control |

The label participant list is empty. The Boolean has an empty name, `name_value = 0`, and a value of false or true.

A body-set record of type `1f4102caa041e9f2ffd878825538b1fe` uses the versioned content header and stamps of section 12. Its zero-value form then contains this body-set prefix:

```text
value u32 = 0
```

A type-2 reference list follows the prefix and ends the record. The source-body set contains qualified references to kernel-entity links in the same PmDc segment. Each link has state 3 and selects a stored kernel carrier of type `5c5945f6d5113313100060a6bba647b5` in that segment. The result-body set contains unqualified external references with the metadata address spaces of section 13.4. A zero-extension entry selects the kernel-entity link by segment identity and header identifier. That link has state 3. A result link joins one current body when its qualified body reference selects the active PmBRep carrier and its entity identifier equals that body's carrier-scoped ASM body key.

Within one composite, each body set has distinct kernel-link `selection_source` values. The source and result sets have the same ordered `selection_source` sequence. Equal values pair one stored source selection with its result selection. The result set gives the stored result bodies of the feature, in member order.

## 14. Document kind

`Pm*` segment families identify a part document. `Am*` segment families identify an assembly document. A document that contains both families has the distinct `mixed_part_assembly` kind. Property metadata can identify a part, assembly, drawing, or presentation only when segment-family evidence does not already identify the kind.

## 15. Drawing documents

An IDW drawing uses the CFB and RSe envelope of sections 1 through 4. The segment registry identifies these drawing families:

| Registry type | Segment family |
| --- | --- |
| `DlDocDcSegmentType` | Drawing document |
| `DlBRxSegmentType` | Drawing browser |
| `DlDirectorySegmentType` | Drawing directory |
| `AppSegmentType` | Drawing application |
| `DlSheetDcSegmentType` | Sheet definition |
| `DlSheetSmSegmentType` | Sheet cache |
| `DlSheetDlSegmentType` | Sheet display |

A segment's registry type, segment identifier, and registry version govern its record family. A display name does not select a drawing family.

The following drawing record forms use segment major version 19. Other segment versions keep these records untyped.

| Segment | Type identifier | Record |
| --- | --- | --- |
| `DlSheetDcSegmentType` | `cd907cd3d01116fa60000dbd861c3cb0` | Sheet |
| `DlSheetDcSegmentType` | `5942cb22d111c557600037bd861c3cb0` | Drawing view |
| `DlSheetDcSegmentType` | `c1965962d4114e8e600095873fa6fbb0` | Alternate drawing view |
| `DlBRxSegmentType` | `69a56f6bd2112c3d6000b58d3125c3b0` | Modeled-document browser reference |
| `DlSheetSmSegmentType` | `81136d8ad111566b600038bd861c3cb0` | Cached view |

The sheet prefix contains `header:bytes[14]` and `identifier:bytes[16]`. A u32-counted UTF-16LE label follows the prefix. A type-2 property-reference list follows the label. The sheet property-list prefix contains `marker:u16 = 2`, `signature:u16 = 0x3000`, `count:u32 = 6`, and `metadata:u32[2]`. Its six packed references follow. The six property references are qualified local references to type `754d8790d011f8d10008cabc0663dc09`. The first two auxiliary lists contain qualified local references to type `b2206557d11196a460001b8aeb49cdb0`. The view lists contain non-null qualified local references. The sheet controls contain `controls:bytes[24]`. A type-2 view-reference list follows the controls, then the sheet tail. The view-reference list names drawing-view records of type `5942cb22d111c557600037bd861c3cb0` in the same sheet segment. The sheet tail prefix contains `prefix:bytes[14]`. Three type-2 auxiliary reference lists follow. The third list names alternate-drawing-view records of type `c1965962d4114e8e600095873fa6fbb0`. The sheet terminal contains `terminal:bytes[8]` and ends the record.

The view prefix contains `header:bytes[22]` and `values:u32[2]`. The view operand-list prefix contains `marker:u16 = 2`, `signature:u16 = 0x3000`, `count:u32 = 19`, and `metadata:u32[2]`. Its nineteen ordered packed references follow. The operand references are non-null qualified references in the same sheet segment. Their target classes are:

| Slot | Target type identifier |
| --- | --- |
| 0 | `2e187506d2111f5e6000b3856c2387b0` |
| 1 | `9ff49f1dd11101e96000a8ade3bc87b0` |
| 2 | `3edf52ced011d0d20008ccbc0663dc09` |
| 3 | `3edf52ced011d0d20008ccbc0663dc09` |
| 4 | `4debf5fad41120726000bebead9287b0` |
| 5 | `35df52ced011d0d20008ccbc0663dc09` |
| 6 | `474dd06b2047478c9da481bcfae149d5` |
| 7 | `229630f7d111a466600027a5602d6bb0` |
| 8 | `5c42cb22d111c557600037bd861c3cb0` |
| 9 | `284d8790d011f8d10008cabc0663dc09` |
| 10 | `284d8790d011f8d10008cabc0663dc09` |
| 11 | `ebf44026d111e457600037bd861c3cb0` |
| 12 | `264d8790d011f8d10008cabc0663dc09` |
| 13 | `264d8790d011f8d10008cabc0663dc09` |
| 14 | `e9ee786bd11195a460009dade3bc87b0` |
| 15 | `e9ee786bd11195a460009dade3bc87b0` |
| 16 | `2e187506d2111f5e6000b3856c2387b0` |
| 17 | `9ff49f1dd11101e96000a8ade3bc87b0` |
| 18 | `35df52ced011d0d20008ccbc0663dc09` |

The list is followed by `word:u16`, `values:u32[2]`, a u32 extension in the alternate-view form, and u32-counted UTF-16LE text. The opaque tail follows the text. The counted text length is a u32 immediately before its UTF-16LE code units. The view header contains `header_value:u32`, `header_id:u16`, and `remainder:bytes[16]`.

A type-2 list with zero items contains only its marker, signature, and u32 count. A nonempty type-2 list also contains two u32 metadata values and one packed u32 reference per item. A packed local reference uses bits 0 through 30 as a one-based record index. Bit 31 is the qualifier. A zero index selects no record.

The modeled-document browser prefix contains `prefix:bytes[49]`. A u32-counted UTF-16LE path follows. Its suffix is `.ipt` or `.iam`. The opaque tail follows the path.

The cached-view prefix contains `prefix:bytes[28]` and `view_reference:u32`. The view reference is non-null and unqualified. It is a one-based index in metadata section 10. In the zero-high-word, zero-key-extension form, the first u32 of the section-10 entry is a zero-based index in section 8. The selected revision entry gives a zero-based segment-pair index in section 7. It selects one drawing-view record of type `5942cb22d111c557600037bd861c3cb0` in the selected sheet segment by `header_value = 0` and equal `header_id`. A cached view whose reference selects any other record keeps its record untyped. The opaque tail follows the reference.

Drawing metadata section 7 has 32-byte segment-pair entries. Section 8 has 20-byte revision entries. Section 10 has 8-byte entries containing a u32 reference word and two u16 values. The section-8 count field is `reference_count:u16`. Section 9 has the tagged entries of section 3.

`RSeEmbeddings/ThirdPartyStorages/_DWFSnapshot` contains opaque file bytes.
