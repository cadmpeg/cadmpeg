# Rhino 3DM Open Items

This document lists the parts of the Rhino 3DM format that we do not know. The specification `rhino_3dm.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Third-party plug-ins

### PP-01. Plug-in class payloads

**Question.** What payload grammar and semantics does each class UUID that a third-party plug-in registers select?

**Known.** `rhino_3dm.md` §7 "An object record is:" through `rhino_3dm.md` §7 "The object type is a category bitfield, not a class identity. The UUID chunk" define the class wrapper, class UUID, bounded class-data payload, and checksum boundary. `rhino_3dm.md` §20.5 "The 32-byte" through `rhino_3dm.md` §20.5 "The 32-byte" define the opaque-record identity for a class that the built-in registry does not define.

**Need.** We must know the grammar and semantics to decode the class payload as typed geometry, topology, presentation, or document data.

### PP-02. Plug-in userdata payloads

**Question.** What payload grammar and semantics does each third-party userdata class UUID and item UUID pair select?

**Known.** `rhino_3dm.md` §7.2 "A class userdata chunk begins with a packed version byte." through `rhino_3dm.md` §7.2 "The header has the checksum selected by its typecode." define the userdata header, application identity, version fields, anonymous payload boundary, and legacy archive-version rule.

**Need.** We must know the grammar and semantics to decode the bounded userdata payload as typed data.

### PP-03. Plug-in dictionary entries

**Question.** What value grammar and semantics does each plug-in-defined dictionary entry select?

**Known.** `rhino_3dm.md` §6.3 "| dictionary                |" through `rhino_3dm.md` §6.3 "| dictionary                |" define the dictionary chunk typecodes. `rhino_3dm.md` §7.2 "A class userdata chunk begins with a packed version byte." through `rhino_3dm.md` §7.2 "The header has the checksum selected by its typecode." define the containing userdata boundary and identity.

**Need.** We must know the value grammar and semantics to decode a plug-in dictionary without treating its entries as one opaque record.

### PP-04. Plug-in application records

**Question.** What payload grammar and semantics does each plug-in-defined application record select?

**Known.** `rhino_3dm.md` §7 "The normal V2+ table sequence is:" through `rhino_3dm.md` §7 "Optional tables may be absent. A table is a bounded table chunk containing" define bounded table records and user tables. `rhino_3dm.md` §20.5 "The 32-byte" through `rhino_3dm.md` §20.5 "The 32-byte" define the identity and byte boundary of a remaining opaque record.

**Need.** We must know the grammar and semantics to transfer the application record as typed document data.

### PP-05. Plug-in object-attribute items

**Question.** What width, payload grammar, and semantics does each plug-in-defined object-attribute item ID select?

**Known.** `rhino_3dm.md` §9.2 "The payload" through `rhino_3dm.md` §9.2 "minor 0: items 1..21" define the payload and version gate for each built-in object-attribute item ID through 41. The item stream has no general length field for an unknown item.

**Need.** We must know the width and grammar to find the next item boundary and to transfer the item as typed object state.

### PP-06. Plug-in layer-extension items

**Question.** What width, payload grammar, and semantics does each plug-in-defined layer-extension item ID select?

**Known.** `rhino_3dm.md` §8.3 "Gated fields" through `rhino_3dm.md` §8.3 "The extension stream is item byte, payload, next item byte, terminated by item" define the stream terminator, payload, and version gate for each built-in layer-extension item ID through 36. The item stream has no general length field for an unknown item.

**Need.** We must know the width and grammar to find the next item boundary and to transfer the item as typed layer state.

## 2. Later built-in versions

### FV-01. Unregistered built-in classes

**Question.** What payload grammar and semantics does each later built-in class UUID select?

**Known.** `rhino_3dm.md` §7 "An object record is:" through `rhino_3dm.md` §7 "The object type is a category bitfield, not a class identity. The UUID chunk" define a class wrapper independently of the class-data grammar. `rhino_3dm.md` §20.5 "The 32-byte" through `rhino_3dm.md` §20.5 "The 32-byte" require a complete unregistered class record to remain one named opaque record.

**Need.** We must know the grammar and semantics to add the class to the built-in registry and to transfer its typed data.

### FV-02. Later object-attribute items

**Question.** What width, payload grammar, version gate, and semantics does each later built-in object-attribute item ID select?

**Known.** `rhino_3dm.md` §9.2 "The payload" through `rhino_3dm.md` §9.2 "minor 0: items 1..21" define item IDs 1 through 41 and their introduction gates. The tagged stream has no general length field for a later item.

**Need.** We must know the width and grammar to find the next item boundary and to extend the built-in object-attribute model.

### FV-03. Later layer-extension items

**Question.** What width, payload grammar, version gate, and semantics does each later built-in layer-extension item ID select?

**Known.** `rhino_3dm.md` §8.3 "Gated fields" through `rhino_3dm.md` §8.3 "The extension stream is item byte, payload, next item byte, terminated by item" define item IDs 28 through 36 and their introduction gates. The extension stream has no general length field for a later item.

**Need.** We must know the width and grammar to find the next item boundary and to extend the built-in layer model.

### FV-04. Later major payload versions

**Question.** What complete payload grammar and semantics does each built-in major version that `rhino_3dm.md` does not define select?

**Known.** `rhino_3dm.md` §5 "A packed payload version is one byte:" through `rhino_3dm.md` §5 "These forms" define packed and anonymous payload-version fields. Each containing long or anonymous chunk supplies the complete payload boundary.

**Need.** We must know the grammar and semantics to decode the new major version as typed data.

### FV-05. Later minor-version suffixes

**Question.** What field grammar and semantics does each later built-in minor-version suffix select?

**Known.** `rhino_3dm.md` §5 "A packed payload version is one byte:" through `rhino_3dm.md` §5 "These forms" define packed and anonymous minor-version fields. A bounded payload fixes the end of the suffix but does not give its field boundaries.

**Need.** We must know the field grammar and semantics to decode the suffix and to distinguish it from malformed trailing bytes.

## 3. Legacy geometry

### LG-01. V1 geometry payloads

**Question.** What grammar and semantics does each V1 geometry payload use?

**Known.** `rhino_3dm.md` §1 "Rhino 3DM is" through `rhino_3dm.md` §1 "V1 uses a flat-chunk grammar and may omit the end marker. V2 and later use the" define V1 as a flat chunk stream. `rhino_3dm.md` §4.1 "For V1, CRC16 is selected by the legacy chunk cases: legacy geometry chunks," through `rhino_3dm.md` §4.1 "The stored CRC16 is little-endian. Test vectors are:" define the V1 geometry checksum rule. The specification does not define the geometry fields inside these chunks.

**Need.** We must know the payload grammar and semantics to decode V1 geometry as typed neutral geometry.

### LG-02. V2 geometry payloads

**Question.** What grammar and semantics does each V2 geometry payload use?

**Known.** `rhino_3dm.md` §1 "Rhino 3DM is" through `rhino_3dm.md` §1 "V1 uses a flat-chunk grammar and may omit the end marker. V2 and later use the" define V2 as a table sequence with four-byte chunk values. `rhino_3dm.md` §4.1 "For V2 and later, a long chunk with `TCODE_CRC` set ends with a four-byte" through `rhino_3dm.md` §4.1 "For V2 and later, a long chunk with `TCODE_CRC` set ends with a four-byte" define its CRC32 rule. The specification does not define the geometry fields inside these records.

**Need.** We must know the payload grammar and semantics to decode V2 geometry as typed neutral geometry.

## 4. Agreement with openNURBS

### ON-01. Container checksum coverage

**Question.** Which bytes does the stored CRC32 of a container chunk cover?

**Known.** openNURBS accumulates a chunk checksum over the bytes that the writer puts at that chunk's own nesting level. `ON_BinaryArchive::BeginWrite3dmChunk` gives a child chunk its own accumulator, `ON_BinaryArchive::UpdateCRC` feeds only the current level, and `ON_BinaryArchive::EndRead3dmChunk` compares the stored value against the accumulation of that level (`opennurbs_archive.cpp`). A chunk header, a complete nested chunk, and the stored checksum bytes stay outside the covered range. A chunk that holds only nested children covers an empty range, and CRC32 of an empty range is zero. `rhino_3dm.md` §4.1 "For V2 and later, a long chunk with `TCODE_CRC` set ends with a four-byte" states the same rule.

**Conflict.** `verify_checksum` in `crates/cadmpeg-codec-rhino/src/chunks.rs` passes the complete chunk body as one range. `verify_checksum_ranges`, in the same file, applies the rule above, and one caller uses it, in `brep.rs`. Thirteen decode call sites use the whole-body form: `container.rs`, `objects.rs`, `settings.rs`, `instances.rs`, two in `mesh.rs`, two in `subd.rs`, two in `extrusion.rs`, and two more in `brep.rs`. Measured against the 153 Rhino-authored `.3dm` files in the openNURBS `example_files` tree, 87 of the 93 accepted files report a checksum mismatch. The CRC32 algorithm agrees on both sides; both compute zlib CRC32.

**Note.** Two further defects come from the same model. `checksum_warning` in `container.rs` binds `expected` to the value the codec computed and `actual` to the value the file stores, so the message names the file's own checksum as `got`. `container.rs` also skips the check when a `TCODE_OBJECT_RECORD` or `TCODE_LAYER_RECORD` child stores an all-zero checksum. Under the coverage rule above, zero is the value openNURBS writes for such a chunk, so this special case has no remaining work.

**Need.** Each call site must give the ranges it wrote at its own level. Until then the warning that a genuine file produces carries no information, and a real corruption stays inside that noise.

### ON-02. Angular dimension measurement

**Question.** Which arc does the measurement of an angular dimension report?

**Known.** `ON_DimAngular::Measurement` in `opennurbs_dimension.cpp` normalizes the first extension angle to zero and returns the counterclockwise sweep to the second extension angle. It computes the dimension-line angle and tests it, and both arms of that test return the same sweep, so the dimension-line point does not select the arc.

**Conflict.** `angular_measurement` in `crates/cadmpeg-codec-rhino/src/dimensions.rs` returns TAU minus the counterclockwise sweep when the dimension-line direction is outside that sweep. A dimension placed on the reflex side then measures 270 degrees where openNURBS measures 90. The unit test `angular_measurement_selects_the_arc_containing_the_dimension_line` asserts the codec's result, and its name states the selection rule that openNURBS does not apply.

**Need.** We must decide which value the neutral measurement carries. The uncertainty that remains is that Rhino's own display code is outside the public openNURBS tree, so an angular value that Rhino draws on screen can come from code we cannot read.

### ON-03. Unit system `none`

**Question.** What scale does a document with unit system `none` give to its geometry?

**Known.** `rhino_3dm.md` §8.2 "The units/tolerances structure begins with an ordinary `i32` structure version," lists value 0 as `none` and value 255 as `unset`, and both are legal stored settings. openNURBS reads such a document and returns its geometry.

**Conflict.** `unit_scale` in `crates/cadmpeg-codec-rhino/src/decode.rs` reads `millimeters_per_unit`, which `settings.rs` leaves empty for `none` and for `unset`. Every curve, mesh, and surface arm of `decode.rs` then takes the guard branch and records the loss "simple geometry retained because document units are unavailable". Five files in the openNURBS `example_files` tree carry unit system `none`, at archive versions 4 and 50. Three of the five raise that loss and transfer no geometry, and all five transfer zero object records.

**Need.** A unitless document has a valid coordinate space with no millimetre binding. The decoder must transfer the geometry at scale 1.0 and record one document-level loss that says the unit system is unknown.

### ON-04. Strictness rules that openNURBS does not apply

**Question.** Which of the codec's framing refusals must stay fatal?

**Known.** Four rules refuse a file that openNURBS reads.

- `chunk_at` in `crates/cadmpeg-codec-rhino/src/chunks.rs` refuses any typecode with bit `0x4000` set outside a small allowed set. This catches `TCODE_ENDOFFILE_GOO` and the plug-in and user typecodes that carry that bit. openNURBS has no such rule; `opennurbs_archive.cpp` derives the chunk shape from the short bit and the declared value.
- `chunk_at` treats a negative long value as fatal. `ON_BinaryArchive::PushBigChunk` builds its long-chunk predicate as `big_value >= 0`, so a negative value demotes the chunk to a bodyless one and reading continues.
- `chunk_at` requires the `TCODE_ENDOFFILE` declared length to equal the file-size field width, and `parse_eof` treats a file-size mismatch as fatal. `ON_BinaryArchive::Read3dmEndMark` requires the length to be at least that width and records the stored size without comparing it to the input length.
- `container.rs` refuses a table that has no `TCODE_ENDOFTABLE` marker. `ON_BinaryArchive::EndRead3dmTable` does not look for the marker.

`rhino_3dm.md` §4 "Bit `0x00004000` is reserved and is zero in valid typecodes." and `rhino_3dm.md` §5 "The stored size includes the 32-byte header, all preceding chunks, the EOF" state the codec's side of the first and third rules.

**Conflict.** Each rule turns a file that openNURBS reads into a complete decode failure. The specification states the same rule that the decoder applies. openNURBS applies a different one.

**Need.** We must decide, for each rule, whether it stays fatal, becomes a warning with recovery, or is removed. The decision changes the specification and the decoder together.

### ON-05. Latent legacy constants and version gates

**Question.** Which legacy constants must change before V1 decoding goes past the header?

**Known.** Four values disagree with openNURBS and are unreachable while V1 support stays header-only.

- `chunk_at` sign-extends a four-byte chunk value with `reader.i32()? as i64`. `ON_IsUnsignedChunkTypecode` in `opennurbs_archive.cpp` selects zero extension for every long typecode and for four short typecodes.
- `TCODE_SUMMARY` in `chunks.rs` is `0x0000_0002`. `opennurbs_3dm.h` defines `TCODE_SUMMARY` as `TCODE_INTERFACE | 0x0013`, which is `0x0200_0013`.
- `crc16` in `chunks.rs` shifts each byte into the high half of the remainder, which is the plain CRC-CCITT form. `ON_CRC16` in `opennurbs_crc.cpp` puts the byte in the low half after the table lookup, which is the augmented form. openNURBS seeds a V1 chunk with 1, appends the stored bytes to the running remainder, and treats a final remainder of zero as agreement (`ON_BinaryArchive::EndRead3dmChunk`). The codec compares its remainder against the stored value. `rhino_3dm.md` §4.1 "CRC16 is non-reflected CRC-CCITT:" carries both forms at once: its pseudocode gives `0xbeef` for the message `123456789` at seed 0, which is the openNURBS value, and the test vector below it gives `0x31c3`, which is the codec's value.
- `checksum_kind` in `chunks.rs` selects CRC16 for the V1 class-UUID chunk `0x0002_fffb`. `ON_BinaryArchive::PushBigChunk` selects it for `TCODE_OPENNURBS_OBJECT | TCODE_CRC | 0x7FFD`, which is `0x0002_fffd`.

**Note.** `mesh_payload` in `writer.rs` selects the mesh minor version with `archive_version == 50`. `opennurbs_mesh.cpp` keys the same field on an archive version of 60 or above. The two agree because `RhinoArchiveVersion` in `lib.rs` holds only 50, 60, 70, and 80. Written as `< 60` the branch stays correct if that set grows.

**Need.** We must correct the four values before a V1 decoder reads a payload, and we must resolve the two CRC16 forms in `rhino_3dm.md` §4.1 into one.

### ON-06. V5 legacy dimension fields

**Question.** What values do the V5 legacy dimension fields hold that the decoder supplies from a constant?

**Known.** Four fields in `crates/cadmpeg-codec-rhino/src/dimensions.rs` and `annotations.rs` come from a constant or are dropped.

- `horizontal_direction` is set to `[1.0, 0.0]`. `ON_DimLinear::Create` in `opennurbs_dimension.cpp` projects a reference horizontal vector into plane coordinates with `ON_Plane::ClosestPointTo` and stores the result.
- `allow_text_scaling` is `minor < 1 || annotation.bool()?`, so an old record gets true. `ON_OBSOLETE_V5_Annotation::Read` in `opennurbs_internal_V2_annotation.cpp` sets `m_annotative_scale` to false before it reads, with the stated reason that text in old files must behave as it did in those files.
- The V5 angular arm sets `first_extension_offset` and `second_extension_offset` to zero and keeps no stored value.
- One `annotation_type` field of type `i32` carries the V5 enum on the legacy path and the V6 enum on the modern path. The numeric values collide. `ON_INTERNAL_OBSOLETE::V5_eAnnotationType` in `opennurbs_internal_defines.h` gives 4 to `dtDimDiameter` and 8 to `dtDimOrdinate`. `ON::AnnotationType` in `opennurbs_defines.h` gives 4 to `Radius` and 8 to `CenterMark`.

**Need.** A consumer of the neutral dimension cannot tell a V5 diameter from a V6 radius while one field carries two enums. The other three fields must come from the record or from the openNURBS default.

### ON-07. Empty-string write form

**Question.** How does openNURBS write a string with no characters?

**Known.** `ON_BinaryArchive::WriteUTF16String` in `opennurbs_archive.cpp` counts the UTF-16 elements, adds one for the terminator when the count is above zero, writes that count as a `u32`, and writes elements only when the count is above zero. An empty string therefore stores count 0 and no elements. `utf16` in `crates/cadmpeg-codec-rhino/src/writer.rs` always appends the terminator, so an empty string stores count 1 and one zero `u16`. The read side agrees with openNURBS on both forms: it is UTF-16LE and the count includes the terminator.

**Need.** A written archive differs from a Rhino-written archive in every empty string field. The decoder is unaffected, so this is a byte-fidelity item.

## 5. Transfer evidence

### TE-01. Object transfer on Rhino-authored files

**Question.** Why does an object class fail on a Rhino-authored file where the committed fixture for the same class passes?

**Known.** The openNURBS distribution ships 153 Rhino-authored `.3dm` files in its `example_files` tree, spanning V1 through V8. Measured against that set, the codec's object-record traversal agrees with the object walk of openNURBS' own `example_read` on 93 of 93 files that the codec decodes, and geometry transfer reaches 175 of 2,869 objects, which is 6.1 percent. Per stored archive version, as files and decoded objects over total objects: 3 gives 34 files and 0 of 2,477; 4 gives 14 files and 0 of 72; 50 gives 19 files and 79 of 198; 60 gives 11 files and 33 of 37; 70 gives 12 files and 36 of 46; 80 gives 3 files and 27 of 39. Every failure is counted as a loss.

On archive version 50 and above, 134 object records stay undecoded and keep their class. The census is `ON_Brep` 95, `ON_LineCurve` 9, `ON_Extrusion` 9, `ON_Text` 8, `ON_OBSOLETE_V5_Leader` 4, the Brep class `F06FC243-A32A-4608-9DD8-A7D2C4CE2A36` 4, `ON_PolylineCurve` 2, `ON_Mesh` 1, `ON_OBSOLETE_V5_TextObject` 1, and `ON_NurbsSurface` 1. The crate holds a decoder for the brep, curve, mesh, extrusion, and surface families, and the committed fixture for each of those families passes.

The committed fixtures are minimal instances of what the decoder already expects. All 28 are between 32 and 2,794 bytes. The builders in `crates/cadmpeg-codec-rhino/src/archive_test_support.rs` write every long chunk with an eight-byte length and compute every CRC32 over the complete flat body, which are the two assumptions the decoder makes in `chunks.rs`.

**Need.** We must find, for each class in the list, which byte-level difference separates a Rhino-authored record from the fixture. Until that is known, a passing fixture gives no information about a Rhino-authored file of the same class.

### TE-02. Witness strategy and the support claim

**Question.** Which files give an uncorrelated witness that the codec reads and writes 3DM?

**Known.** The committed evidence is self-authored. Of the 28 fixtures under `crates/cadmpeg-codec-rhino/tests/golden/fixtures`, one comes from the codec's own writer, two are 32-byte refusal inputs, two are 216-byte header-only documents, and the rest come from the hand-authored builders in `archive_test_support.rs`. Nothing confirms that any of them is a 3DM file that Rhino would open.

openNURBS supplies the uncorrelated witnesses. Its `example_files` tree holds 153 Rhino-authored files across V1 through V8. Its `example_read` program reads a file, lists the model geometry objects, and prints a chunk dump under `-chunkdump`, which makes it a differential oracle at the framing level and at the object level. Its `example_write` program runs and emits Rhino-readable files. `Internal_WriteExampleModel` in `example_write.cpp` passes an archive version to `ONX_Model::Write`, so a build selects the version.

The encode goldens under `tests/golden/encode` hold 52 files. Fifty are the same refusal text, "not implemented yet: Rhino native records require explicit survival handling". The other two are the archive-version-50 and archive-version-80 outputs for `generated_point.3dm`, which the writer itself produced. The encode tree therefore pins one refusal path and one round trip of the writer's own output.

**Need.** We need a second fixture tier that mirrors the structure of `example_files`, synthesized rather than copied, and pinned by the per-archive-version object-transfer ratio in TE-01 so that a regression in that ratio fails a test. We must also re-measure the L8 support claim per archive version. Archive version 50 transfers 79 of 198 objects today, and L8 assumes that the whole document transfers.
