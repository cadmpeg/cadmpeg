# Autodesk Inventor IPT/IAM

This specification defines the byte semantics of the supported Autodesk Inventor IPT/IAM envelope. Multi-byte integers are little-endian unless a section states otherwise.

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

Kernel bytes beginning with `ASM BinaryFile4` or `ASM BinaryFile8` use the ASM header and SAB grammar in [asm.md](asm.md). Kernel bytes beginning with `ACIS BinaryFile` are retained as an ACIS carrier. A kernel signature validates the already typed record. It is not a record locator.

## 6. OLE properties and preview

Root streams that contain an OLE Property Set self-identify through their byte-order marker, section directory, and FMTID. Section offsets bound each property. Code-page properties select LPSTR decoding. LPWSTR values contain UTF-16LE code units. Scalar, vector, FILETIME, BLOB, and clipboard values retain their type code. A clipboard preview is emitted only when its payload has a recognized image signature.

## 7. Protein package

The root `Protein` stream starts with a u32 payload length. Zero length is the complete four-byte empty form. A nonzero length equals the exact remaining byte count, and the remaining bytes are one ZIP archive. ZIP entry names are unique and do not contain absolute, parent, current-directory, empty, NUL, or backslash path components.

`InstanceProperties.bin` uses a 16-byte header followed by 136-byte pages. Page bytes 4 through 7 equal `80 00 01 00` for a record start or `80 00 00 00` for a continuation. A terminal page starts with `ff ff ff ff`; its u16 at offset 4 gives the used payload bytes at offset 8. Packaged XML schemas select the typed property order and carriers. Asset definitions form a catalog. A topology assignment requires a separate typed assignment record.

## 8. External references

`UFRxDoc` schemas 11 through 15 start with a u16 schema and a u16 section-version count. The section-version table governs optional header fields. The schema-15 representation/model-state branch adds a u16 representation prefix, two counted UTF-16LE representation strings, and a counted UTF-16LE active model-state name with a two-u16 state pair; it omits the older header-version-flags field. Its model-state table precedes the external-reference table. Each model-state record contains a u8 prefix, counted UTF-16LE name, two-u16 state pair, u32 prefix count, u32 parameter count, the counted parameter records, and a 77-byte suffix. Each parameter record contains a counted UTF-16LE name, u8 tag, u16 kind, u16 state, counted UTF-16LE value, and u16 trailer.

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

## 11. Document kind

`Pm*` segment families identify a part document. `Am*` segment families identify an assembly document. A document that contains both families has the distinct `mixed_part_assembly` kind. Property metadata can identify a part, assembly, drawing, or presentation only when segment-family evidence does not already identify the kind.
